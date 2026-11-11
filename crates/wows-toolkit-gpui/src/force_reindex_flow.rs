use std::collections::BTreeSet;

use super::*;
use crate::force_reindex::Confirm;
use crate::force_reindex::Fetchable;
use crate::force_reindex::ReindexCandidate;
use crate::force_reindex::ReindexScope;
use crate::force_reindex::ReindexSummary;
use crate::force_reindex::Release;
use crate::replay_index::IndexProgress;
use crate::replay_inspector::GameDataCache;

/// A run's downloads, fetched one release at a time so Stop takes effect
/// between them.
struct Fetching {
    summary: ReindexSummary,
    game_data: GameDataCache,
    cancel: Arc<AtomicBool>,
    base: PathBuf,
    /// Releases still to fetch, the next one last.
    queue: Vec<Release>,
    fetched: usize,
    failed: BTreeSet<Release>,
}

impl App {
    /// Every replay a palette command re-indexes, each once, as a forced
    /// re-index sees it.
    pub(super) fn listed_reindex_candidates(&self, cx: &gpui_kit::App) -> Vec<ReindexCandidate> {
        self.replay_inspector.read(cx).reindex_candidates(cx)
    }

    /// The install's replays directory, which the index's own source is
    /// rooted at whichever listing a pass was started from. `None` while no
    /// game directory is set.
    pub(super) fn install_replays_root(&self) -> Option<PathBuf> {
        let wows_dir = &self.settings()?.wows_dir;
        if wows_dir.is_empty() {
            return None;
        }

        Some(std::path::Path::new(wows_dir).join("replays"))
    }

    /// Takes `settings` as loaded, without what applying them starts.
    /// Test-only.
    #[cfg(test)]
    pub(crate) fn seed_settings_for_test(&mut self, settings: crate::settings::GpuiSettings) {
        self.settings = SettingsState::Loaded(Box::new(settings));
    }

    /// Forces a re-index of the listed replays a palette command names.
    pub(super) fn force_reindex_scope(&mut self, scope: ReindexScope, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = scope.select(self.listed_reindex_candidates(cx));
        let inspector = self.replay_inspector.clone();
        self.prepare_force_reindex(chosen, Confirm::Always, &inspector, window, cx);
    }

    /// Works out what re-indexing `candidates` takes, asks when `confirm` says
    /// to, and runs it.
    pub(super) fn prepare_force_reindex(
        &mut self,
        candidates: Vec<ReindexCandidate>,
        confirm: Confirm,
        inspector: &Entity<ReplayInspectorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.index_cancel.is_some() {
            crate::toast::warn(t!("ui.reindex.busy_index").into_owned(), window, cx);
            return;
        }
        if candidates.is_empty() {
            crate::toast::info(t!("ui.reindex.nothing_listed").into_owned(), window, cx);
            return;
        }
        let Some(game_data) = inspector.read(cx).game_data() else {
            crate::toast::warn(t!("ui.reindex.no_game_data").into_owned(), window, cx);
            return;
        };
        if self.install_replays_root().is_none() {
            crate::toast::warn(t!("ui.reindex.no_game_data").into_owned(), window, cx);
            return;
        }

        // A directory listing and an index file read per release, so off the
        // UI thread. A release whose build number is already loaded counts as
        // readable whatever its version: the loaded builds are kept by number.
        let builds = crate::force_reindex::builds(&candidates);
        let reading = game_data.clone();
        cx.spawn_in(window, async move |this, cx| {
            let local: BTreeSet<Release> = cx
                .background_spawn(async move {
                    builds
                        .iter()
                        .map(|need| need.release)
                        .filter(|release| reading.can_read_build(release.build.0, Some(&release.version_said())))
                        .collect()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.plan_force_reindex(candidates, local, confirm, game_data, window, cx)
            });
        })
        .detach();
    }

    /// Asks the repository for the releases this machine cannot read, then
    /// puts the summary to the reader.
    fn plan_force_reindex(
        &mut self,
        candidates: Vec<ReindexCandidate>,
        local: BTreeSet<Release>,
        confirm: Confirm,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let missing: Vec<Release> = crate::force_reindex::builds(&candidates)
            .into_iter()
            .map(|need| need.release)
            .filter(|release| !local.contains(release))
            .collect();

        // Nothing to fetch, or nowhere to fetch it to: what cannot be read
        // locally is left out, and the summary says why.
        if missing.is_empty() {
            let summary = crate::force_reindex::summarize(&candidates, &local, &Fetchable::Releases(BTreeSet::new()));
            self.confirm_force_reindex(summary, confirm, game_data, window, cx);
            return;
        }
        let Some(base) = self.cache_base() else {
            let summary = crate::force_reindex::summarize(&candidates, &local, &Fetchable::NoCacheDirectory);
            self.confirm_force_reindex(summary, confirm, game_data, window, cx);
            return;
        };
        if self.cache.busy() {
            crate::toast::warn(t!("ui.reindex.busy_cache").into_owned(), window, cx);
            return;
        }

        crate::toast::info(t!("ui.reindex.planning").into_owned(), window, cx);
        let requests: Vec<(u32, Option<String>)> =
            missing.iter().map(|release| (release.build.0, Some(release.version_said()))).collect();
        let held = window.window_handle();
        game_data_cache::plan(
            |this: &mut Self| &mut this.cache,
            base,
            requests,
            self.proxy_url(),
            &cx.entity(),
            cx,
            move |this, outcome, cx| {
                let plan = match outcome {
                    game_data_cache::CacheOutcome::Planned { plan } => plan,
                    // An unanswered question establishes nothing about what is
                    // published, so nothing is put to the reader as unobtainable.
                    outcome => {
                        if let game_data_cache::CacheOutcome::Failed(reason) = &outcome {
                            tracing::warn!("force re-index: the download could not be planned: {reason}");
                        }
                        this.job_said.push(JobReport::warn(t!("ui.dialogs.download_plan_failed").into_owned()));
                        cx.notify();
                        return;
                    }
                };
                let fetchable = match crate::force_reindex::fetchable_from_plan(&missing, &plan.resolved) {
                    Ok(fetchable) => fetchable,
                    // What could not be asked about is not known to be absent,
                    // so the run stops rather than skipping those replays.
                    Err(unreachable) => {
                        tracing::warn!("force re-index: {unreachable}");
                        this.job_said.push(JobReport::warn(t!("ui.dialogs.download_plan_failed").into_owned()));
                        cx.notify();
                        return;
                    }
                };
                let summary = crate::force_reindex::summarize(&candidates, &local, &Fetchable::Releases(fetchable));

                // The plan lands with the app leased and no window in hand, so
                // the question is put once both are free.
                cx.spawn(async move |this, cx| {
                    let _ = held.update(cx, |_root: gpui_kit::AnyView, window, cx| {
                        let _ = this
                            .update(cx, |this, cx| this.confirm_force_reindex(summary, confirm, game_data, window, cx));
                    });
                })
                .detach();
            },
        );
    }

    /// Puts the summary to the reader, or runs it straight away when `confirm`
    /// allows and there is nothing to say.
    fn confirm_force_reindex(
        &mut self,
        summary: ReindexSummary,
        confirm: Confirm,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if summary.replays.is_empty() {
            crate::toast::warn(t!("ui.reindex.nothing_readable", count = summary.skipped()).into_owned(), window, cx);
            return;
        }
        if summary.runs_unasked(confirm) {
            self.run_force_reindex(&summary, game_data, cx);
            return;
        }

        let lines = summary.lines();
        let said: SharedString = lines.join("\n").into();
        let summary = Rc::new(summary);
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let entity = entity.clone();
            let summary = Rc::clone(&summary);
            let game_data = game_data.clone();
            alert
                .title(t!("ui.reindex.confirm_title").into_owned())
                .description(
                    v_flex()
                        .id("force-reindex-summary")
                        .test_support()
                        .aria_label(said.clone())
                        .gap_1()
                        .children(lines.iter().map(|line| div().text_sm().child(line.clone()))),
                )
                .ok_text(t!("ui.reindex.confirm_ok").into_owned())
                .show_cancel(true)
                .on_ok(move |_event, _window, cx| {
                    let game_data = game_data.clone();
                    let _ = entity.update(cx, |this, cx| this.run_force_reindex(&summary, game_data, cx));
                    true
                })
        });
    }

    /// Fetches what the summary needs, then reads its replays again.
    ///
    /// Holds the index pass from the start, downloads included, so the
    /// Settings tab's Stop button ends it and a second pass cannot start
    /// beside it.
    fn run_force_reindex(&mut self, summary: &ReindexSummary, game_data: GameDataCache, cx: &mut Context<Self>) {
        if self.index_cancel.is_some() {
            self.job_said.push(JobReport::warn(t!("ui.reindex.busy_index").into_owned()));
            cx.notify();
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.index_cancel = Some(Arc::clone(&cancel));
        self.index_outcome = None;
        // These builds have been put to the reader; the relist after a download
        // would otherwise offer the ones still missing a second time.
        self.offered_builds
            .extend(summary.downloads.iter().chain(&summary.unobtainable).map(|need| need.release.build.0));
        cx.notify();

        let requested: BTreeSet<Release> = summary.downloads.iter().map(|need| need.release).collect();
        let base = if requested.is_empty() { None } else { self.cache_base() };
        let base = match base {
            Some(_) if self.cache.busy() => {
                self.job_said.push(JobReport::warn(t!("ui.reindex.busy_cache").into_owned()));
                None
            }
            base => base,
        };
        let Some(base) = base else {
            // Nothing to fetch, or no way to fetch it now: the replays waiting
            // on a download are left out.
            let (paths, dropped) = summary.readable_after(&requested);
            self.start_reindex_pass(paths, summary.skipped() + dropped, game_data, cancel, cx);
            return;
        };

        // Popped from the end, so reversed to fetch newest first.
        let queue: Vec<Release> = summary.downloads.iter().rev().map(|need| need.release).collect();
        let fetching =
            Fetching { summary: summary.clone(), game_data, cancel, base, queue, fetched: 0, failed: BTreeSet::new() };
        self.fetch_next(fetching, cx);
    }

    /// Fetches the next release a run needs, or moves on to reading once
    /// there is none left or Stop was pressed.
    fn fetch_next(&mut self, mut fetching: Fetching, cx: &mut Context<Self>) {
        let next = if fetching.cancel.load(Ordering::Relaxed) { None } else { fetching.queue.pop() };
        let Some(release) = next else {
            self.fetched_all(fetching, cx);
            return;
        };

        let build =
            wows_data_mgr::download_repo::BuildUpdateStatus { build: release.build.0, version: release.version_said() };
        game_data_cache::download(
            |this: &mut Self| &mut this.cache,
            fetching.base.clone(),
            vec![build],
            self.proxy_url(),
            &cx.entity(),
            cx,
            move |this, outcome, cx| {
                match outcome {
                    game_data_cache::CacheOutcome::Downloaded { fetched, failed } => {
                        fetching.fetched += fetched;
                        if !failed.is_empty() {
                            fetching.failed.insert(release);
                        }
                    }
                    game_data_cache::CacheOutcome::Failed(reason) => {
                        tracing::warn!(
                            build = release.build.0,
                            "force re-index: the game data was not fetched: {reason}"
                        );
                        fetching.failed.insert(release);
                    }
                    // A download answers with one of the two above.
                    _ => {
                        fetching.failed.insert(release);
                    }
                }
                this.fetch_next(fetching, cx);
            },
        );
    }

    /// Reports the downloads as one job, then reads what can now be read.
    fn fetched_all(&mut self, fetching: Fetching, cx: &mut Context<Self>) {
        let Fetching { summary, game_data, cancel, queue, fetched, failed, .. } = fetching;

        if fetched > 0 || !failed.is_empty() {
            let failed_builds = failed.iter().map(|release| release.build.0).collect();
            self.cache_job_finished(game_data_cache::CacheOutcome::Downloaded { fetched, failed: failed_builds }, cx);
        }
        // Which rows have a preview and which builds are missing are both
        // decided as a directory is walked.
        if fetched > 0 {
            self.replay_inspector.update(cx, |view, cx| view.relist(cx));
        }

        if cancel.load(Ordering::Relaxed) {
            self.index_cancel = None;
            let said = t!("ui.reindex.stopped_before_reading").into_owned();
            self.index_outcome = Some(said.clone());
            self.job_said.push(JobReport::warn(said));
            cx.notify();
            return;
        }

        let unavailable: BTreeSet<Release> = failed.into_iter().chain(queue).collect();
        let (paths, dropped) = summary.readable_after(&unavailable);
        self.start_reindex_pass(paths, summary.skipped() + dropped, game_data, cancel, cx);
    }

    /// Reads `paths` again, reporting through the index pass's progress line.
    ///
    /// `skipped` is what the run left out before this pass, for the message
    /// at the end.
    fn start_reindex_pass(
        &mut self,
        paths: Vec<PathBuf>,
        skipped: usize,
        game_data: GameDataCache,
        cancel: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        let (Some(pool), Some(runtime)) = (settings_store::pool(cx), runtime::runtime(cx)) else {
            self.index_cancel = None;
            self.job_said.push(JobReport::failed(t!("ui.reindex.no_index").into_owned()));
            cx.notify();
            return;
        };
        // Checked when the run was prepared; the setting can be cleared while a
        // download runs.
        let Some(root) = self.install_replays_root() else {
            self.index_cancel = None;
            self.job_said.push(JobReport::failed(t!("ui.reindex.no_game_data").into_owned()));
            cx.notify();
            return;
        };

        self.index_progress = Some(IndexProgress { total: paths.len() as u64, ..IndexProgress::default() });
        cx.notify();
        let progress_tx = self.follow_index_progress(cx);

        let stop = Arc::clone(&cancel);
        self._index_build = Some(cx.spawn(async move |this, cx| {
            let ran = cx
                .background_spawn(async move {
                    crate::replay_index::reindex_replays(&runtime, &pool, &root, &paths, &game_data, &stop, |step| {
                        let _ = progress_tx.unbounded_send(step);
                    })
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.index_cancel = None;
                this.index_progress = None;
                let report = match ran {
                    Ok(progress) => reindex_report(progress, skipped, cancel.load(Ordering::Relaxed)),
                    Err(err) => JobReport::failed(err.to_string()),
                };
                this.index_outcome = Some(report.said.clone());
                this.job_said.push(report);

                // The rows read what the index held before the pass.
                this.replay_inspector.update(cx, |view, cx| view.reload_index_summaries(cx));
                cx.notify();
            });
        }));
    }
}

/// What a finished forced re-index says.
///
/// A pass stopped part-way counts the replays it did not reach as skipped,
/// so the three figures still add up to what was asked for.
fn reindex_report(progress: IndexProgress, skipped: usize, stopped: bool) -> JobReport {
    let unreached = progress.total.saturating_sub(progress.done);
    let skipped = skipped as u64 + unreached;
    let said = if stopped {
        t!("ui.reindex.stopped", indexed = progress.indexed, failed = progress.failed, skipped = skipped)
    } else {
        t!("ui.reindex.done", indexed = progress.indexed, failed = progress.failed, skipped = skipped)
    }
    .into_owned();

    if progress.failed > 0 || skipped > 0 || stopped { JobReport::warn(said) } else { JobReport::ok(said) }
}

#[cfg(test)]
mod tests {
    // Named rather than globbed: the parent's glob brings in `gpui_kit::test`,
    // which would shadow the standard `#[test]`.
    use rust_i18n::t;

    use super::reindex_report;
    use crate::replay_index::IndexProgress;

    #[test]
    fn a_stopped_pass_counts_what_it_did_not_reach_as_skipped() {
        let progress = IndexProgress { done: 3, total: 10, indexed: 2, failed: 1, skipped: 0 };

        let report = reindex_report(progress, 4, true);

        assert_eq!(report.said, t!("ui.reindex.stopped", indexed = 2, failed = 1, skipped = 11));
    }
}

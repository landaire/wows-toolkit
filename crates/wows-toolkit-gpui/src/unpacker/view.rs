//! The Unpacker tab: a build selector over a dock holding one browser pane
//! per VFS source, plus the extraction queue those panes feed.
//!
//! Mirrors the egui app's `build_unpacker_tab`: the version bar appears only
//! when the install carries more than one build, and the package and
//! assets.bin browsers are separate tabs of an inner dock.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::h_flex;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::browser::BrowserEvent;
use super::browser::BrowserPanel;
use super::browser::BrowserSource;
use super::search_panel::SearchPanel;
use super::search_panel::SearchPanelEvent;
use wows_toolkit_viewmodel::unpacker::extract::ExtractOutcome;
use wows_toolkit_viewmodel::unpacker::extract::ExtractProgress;
use wows_toolkit_viewmodel::unpacker::extract::expand_to_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_files;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::queue::ExtractQueue;
use wows_toolkit_viewmodel::unpacker::search::ContentSearchHit;
use wows_toolkit_viewmodel::unpacker::search::SearchProgress;
use wows_toolkit_viewmodel::unpacker::search::compile_query;
use wows_toolkit_viewmodel::unpacker::search::files_to_scan;
use wows_toolkit_viewmodel::unpacker::search::scan;

/// One message from a running scan.
enum SearchUpdate {
    Hit(ContentSearchHit),
    Progress(SearchProgress),
}

/// A game build number. A newtype so a build cannot be mixed up with the
/// other bare integers on this tab (queue lengths, progress counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BuildNumber(pub u32);

impl std::fmt::Display for BuildNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// One entry in the build combo. Local newtype: `BuildNumber` is ours but
/// `SearchableListItem` is not, and carrying the build keeps the confirm
/// handler off a string round trip.
#[derive(Clone)]
struct BuildItem(BuildNumber);

impl SearchableListItem for BuildItem {
    type Value = BuildNumber;

    fn title(&self) -> SharedString {
        SharedString::from(self.0.to_string())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

/// What the extraction queue is doing.
enum ExtractState {
    Idle,
    /// Expanding the queue's directories. The file count is not known yet, so
    /// there is no fraction to show.
    Counting,
    Running(ExtractProgress),
    Failed(String),
    /// Finished or cancelled, with what actually reached disk.
    Done(ExtractOutcome),
}

pub struct UnpackerView {
    /// Builds found in the install, newest first -- the order the egui combo
    /// lists them.
    builds: Vec<BuildNumber>,
    selected_build: Option<BuildNumber>,
    /// The directory and build whose VFS the package pane currently holds.
    loaded_source: Option<(PathBuf, BuildNumber)>,
    /// Bumped per build-load request so a stale result can be dropped.
    load_generation: u64,
    build_select: Entity<SelectState<SearchableVec<BuildItem>>>,
    wows_dir: Option<PathBuf>,
    dock_area: Entity<DockArea>,
    pkg_browser: Entity<BrowserPanel>,
    assets_browser: Entity<BrowserPanel>,
    /// Entries queued for extraction. The shared queue owns the rules about
    /// what queueing a listing means and about queueing the same entry twice.
    queue: ExtractQueue,
    extract_state: ExtractState,
    /// Set to stop an in-flight extraction; replaced per run.
    stop_flag: Arc<AtomicBool>,
    /// One per open search panel, kept so its events keep reaching this tab.
    search_subscriptions: Vec<Subscription>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl UnpackerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pkg_browser = cx.new(|cx| BrowserPanel::new(BrowserSource::Pkg, window, cx));
        let assets_browser = cx.new(|cx| BrowserPanel::new(BrowserSource::AssetsBin, window, cx));
        let dock_area = cx.new(|cx| DockArea::new("unpacker-dock", None, window, cx));

        dock_area.update(cx, |dock, cx| {
            dock.add_panel(pkg_browser.clone(), DockPlacement::Center, None, window, cx);
            dock.add_panel(assets_browser.clone(), DockPlacement::Center, None, window, cx);
        });

        let build_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(false));

        let subscriptions = vec![
            cx.subscribe_in(&pkg_browser, window, Self::on_browser_event),
            cx.subscribe_in(&assets_browser, window, Self::on_browser_event),
            cx.subscribe_in(&build_select, window, |this, _state, event, window, cx| {
                let SelectEvent::Confirm(Some(build)) = event else {
                    return;
                };
                this.select_build(*build, window, cx);
            }),
        ];

        Self {
            builds: Vec::new(),
            selected_build: None,
            loaded_source: None,
            load_generation: 0,
            build_select,
            wows_dir: None,
            dock_area,
            pkg_browser,
            assets_browser,
            queue: ExtractQueue::new(),
            extract_state: ExtractState::Idle,
            stop_flag: Arc::new(AtomicBool::new(false)),
            search_subscriptions: Vec::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Adopts the WoWs directory, enumerates its builds and loads the newest.
    /// An empty directory clears the panes rather than leaving stale data.
    pub fn apply_settings(&mut self, wows_dir: String, window: &mut Window, cx: &mut Context<Self>) {
        if wows_dir.is_empty() {
            self.wows_dir = None;
            self.builds.clear();
            self.selected_build = None;
            self.loaded_source = None;
            self.pkg_browser.update(cx, |pane, cx| pane.clear(window, cx));
            self.assets_browser.update(cx, |pane, cx| pane.clear(window, cx));
            cx.notify();
            return;
        }

        let dir = PathBuf::from(&wows_dir);
        let mut builds: Vec<BuildNumber> = match wowsunpack::game_data::list_available_builds(&dir) {
            Ok(builds) => builds.into_iter().map(BuildNumber).collect(),
            Err(err) => {
                tracing::warn!("unpacker: could not list builds in {}: {err}", dir.display());
                Vec::new()
            }
        };
        builds.sort_unstable();
        builds.reverse();

        self.wows_dir = Some(dir);
        self.build_select.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(builds.iter().copied().map(BuildItem).collect::<Vec<_>>()), window, cx);
        });
        self.builds = builds;

        match self.builds.first().copied() {
            Some(newest) => self.select_build(newest, window, cx),
            None => {
                self.selected_build = None;
                self.pkg_browser.update(cx, |pane, cx| pane.clear(window, cx));
                self.assets_browser.update(cx, |pane, cx| pane.clear(window, cx));
                cx.notify();
            }
        }
    }

    /// Loads `build`'s package VFS in the background and hands it to the
    /// package pane. The assets.bin pane stays on its own loading path.
    ///
    /// The guard keys on the directory as well as the build: two installs can
    /// carry the same build number, and comparing the build alone would leave
    /// the previous install's VFS on screen under the new directory.
    fn select_build(&mut self, build: BuildNumber, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.wows_dir.clone() else { return };
        if self.loaded_source.as_ref() == Some(&(dir.clone(), build)) {
            return;
        }
        self.selected_build = Some(build);
        self.loaded_source = Some((dir.clone(), build));

        if let Some(index) = self.builds.iter().position(|b| *b == build) {
            self.build_select.update(cx, |state, cx| {
                state.set_selected_index(Some(IndexPath::new(index)), window, cx);
            });
        }

        // Stamps this request so a slower earlier load cannot overwrite a
        // later one that already finished.
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;

        self.pkg_browser.update(cx, |pane, cx| pane.set_loading(cx));
        let pkg_browser = self.pkg_browser.clone();
        cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    wowsunpack::game_data::build_game_vfs_for_build(&dir, build.0).map_err(|err| format!("{err}"))
                })
                .await;

            let still_current = this.update(cx, |this, _cx| this.load_generation == generation).unwrap_or(false);
            if !still_current {
                return;
            }

            let _ = pkg_browser.update_in(cx, |pane, window, cx| match loaded {
                Ok(vfs) => pane.set_vfs(vfs, window, cx),
                Err(reason) => pane.set_failed(reason, cx),
            });
        })
        .detach();

        cx.notify();
    }

    fn on_browser_event(
        &mut self,
        _browser: &Entity<BrowserPanel>,
        event: &BrowserEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BrowserEvent::Extract(entries) => {
                self.queue.push_listed_files(entries);
                cx.notify();
            }
            BrowserEvent::Search { source, query, path_filter, files } => {
                self.open_search(*source, query.clone(), path_filter.clone(), files.clone(), _window, cx);
            }
            BrowserEvent::View { path, kind } => {
                // The in-app file viewers are a separate piece of the tab.
                tracing::debug!("unpacker: view requested for {} ({kind:?})", path.as_str());
            }
        }
    }

    /// Opens a results panel and starts the scan behind it. Each run gets its
    /// own panel, matching the egui app, so an earlier result set survives.
    fn open_search(
        &mut self,
        source: BrowserSource,
        query: String,
        path_filter: String,
        files: Arc<FileList>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pattern) = compile_query(&query) else {
            tracing::warn!("unpacker: the search query {query:?} could not be compiled");
            return;
        };

        let stop_flag = Arc::new(AtomicBool::new(false));
        let panel = cx.new(|cx| SearchPanel::new(query.into(), source, stop_flag.clone(), cx));
        self.dock_area.update(cx, |dock, cx| {
            dock.add_panel(panel.clone(), DockPlacement::Bottom, None, window, cx);
        });
        self.search_subscriptions.push(cx.subscribe_in(&panel, window, Self::on_search_panel_event));

        // Weak, so the running scan does not keep a closed panel alive; the
        // panel's own `Drop` then sets the stop flag and the worker winds up.
        let panel = panel.downgrade();
        cx.spawn(async move |_this, cx| {
            let (tx, mut rx) = futures::channel::mpsc::unbounded::<SearchUpdate>();
            let worker = cx.background_spawn(async move {
                let targets = files_to_scan(&files, &path_filter);
                scan(
                    &targets,
                    &pattern,
                    |hit| {
                        let _ = tx.unbounded_send(SearchUpdate::Hit(hit));
                    },
                    |progress| {
                        let _ = tx.unbounded_send(SearchUpdate::Progress(progress));
                    },
                    || stop_flag.load(Ordering::Relaxed),
                );
            });

            // Drained in batches: a broad query matches faster than the
            // foreground can repaint, and applying one hit per wake-up turns
            // the scan into a per-hit relayout.
            while let Some(first) = futures::StreamExt::next(&mut rx).await {
                let mut hits = Vec::new();
                let mut latest_progress = None;
                let mut pending = Some(first);
                while let Some(update) = pending.take().or_else(|| rx.try_recv().ok()) {
                    match update {
                        SearchUpdate::Hit(hit) => hits.push(hit),
                        SearchUpdate::Progress(progress) => latest_progress = Some(progress),
                    }
                }

                let applied = panel.update(cx, |panel, cx| {
                    if let Some(progress) = latest_progress {
                        panel.set_progress(progress, cx);
                    }
                    panel.extend_hits(hits, cx);
                });
                if applied.is_err() {
                    return;
                }
            }

            worker.await;
            let _ = panel.update(cx, |panel, cx| panel.finish(cx));
        })
        .detach();
    }

    fn on_search_panel_event(
        &mut self,
        _panel: &Entity<SearchPanel>,
        event: &SearchPanelEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let SearchPanelEvent::View { path, kind } = event;
        tracing::debug!("unpacker: view requested for {} ({kind:?})", path.as_str());
    }

    fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.queue.clear();
        self.extract_state = ExtractState::Idle;
        cx.notify();
    }

    /// Asks for a destination, then writes the queue to it off the UI thread.
    fn start_extraction(&mut self, cx: &mut Context<Self>) {
        if self.queue.is_empty() || matches!(self.extract_state, ExtractState::Counting | ExtractState::Running(_)) {
            return;
        }
        let Some(output_dir) = rfd::FileDialog::new().set_title("Extract to").pick_folder() else {
            return;
        };

        let queued = self.queue.take();
        let stop_flag = Arc::new(AtomicBool::new(false));
        self.stop_flag = stop_flag.clone();
        self.extract_state = ExtractState::Counting;

        cx.spawn(async move |this, cx| {
            let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<ExtractProgress>();
            let worker = cx.background_spawn(async move {
                let files = expand_to_files(&queued);
                extract_files(
                    &files,
                    &output_dir,
                    |progress| {
                        let _ = progress_tx.unbounded_send(progress);
                    },
                    || stop_flag.load(Ordering::Relaxed),
                )
            });

            while let Some(progress) = futures::StreamExt::next(&mut progress_rx).await {
                let _ = this.update(cx, |this, cx| {
                    this.extract_state = ExtractState::Running(progress);
                    cx.notify();
                });
            }

            let outcome = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.extract_state = match outcome {
                    Ok(outcome) => ExtractState::Done(outcome),
                    Err(err) => ExtractState::Failed(format!("{err}")),
                };
                cx.notify();
            });
        })
        .detach();

        cx.notify();
    }

    fn cancel_extraction(&mut self, cx: &mut Context<Self>) {
        self.stop_flag.store(true, Ordering::Relaxed);
        cx.notify();
    }
}

impl Focusable for UnpackerView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for UnpackerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        // The egui tab shows the version bar only when there is a choice to
        // make; a single-build install gets no chrome for it.
        let version_bar = (self.builds.len() > 1).then(|| {
            h_flex()
                .flex_none()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border)
                .child(div().text_xs().opacity(0.6).child("Version"))
                .child(Select::new(&self.build_select).id("unpacker-build").small().w(px(160.)))
        });

        let status: Option<String> = match &self.extract_state {
            ExtractState::Idle => None,
            ExtractState::Counting => Some("Counting files...".to_string()),
            ExtractState::Running(progress) => Some(format!("Extracting {} of {}", progress.written, progress.total)),
            ExtractState::Failed(reason) => Some(format!("Extraction failed: {reason}")),
            ExtractState::Done(ExtractOutcome::Completed { written }) => Some(format!("Extracted {written} files")),
            ExtractState::Done(ExtractOutcome::Stopped { written }) => Some(format!("Cancelled after {written} files")),
        };
        let running = match &self.extract_state {
            ExtractState::Running(progress) => Some(*progress),
            _ => None,
        };
        let busy = running.is_some() || matches!(self.extract_state, ExtractState::Counting);

        let queue_bar = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().text_xs().opacity(0.6).child(format!("{} queued", self.queue.len())))
            .when_some(running, |this, progress| {
                this.child(
                    div()
                        .w(px(160.))
                        .child(Progress::new("unpacker-extract-progress").value(progress.fraction() * 100.)),
                )
            })
            .when_some(status, |this, status| this.child(div().flex_1().text_xs().opacity(0.6).child(status)))
            .child(
                Button::new("unpacker-extract")
                    .label("Extract...")
                    .compact()
                    .disabled(self.queue.is_empty() || busy)
                    .on_click(cx.listener(|this, _event, _window, cx| this.start_extraction(cx))),
            )
            .child(
                Button::new("unpacker-cancel")
                    .label("Cancel")
                    .compact()
                    .disabled(!busy)
                    .on_click(cx.listener(|this, _event, _window, cx| this.cancel_extraction(cx))),
            )
            .child(
                Button::new("unpacker-clear-queue")
                    .label("Clear")
                    .compact()
                    .disabled(self.queue.is_empty())
                    .on_click(cx.listener(|this, _event, _window, cx| this.clear_queue(cx))),
            );

        v_flex()
            .id("unpacker-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .when_some(version_bar, |this, bar| this.child(bar))
            .child(queue_bar)
            .child(div().flex_1().min_h(px(0.)).child(self.dock_area.clone()))
    }
}

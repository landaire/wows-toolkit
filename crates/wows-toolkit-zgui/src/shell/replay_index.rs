//! Building the replay index over the install's replays directory, as the settings drawer and the
//! palette start it.
//!
//! The build outlives the drawer, which is rebuilt each time it opens, so its state is the shell's
//! and its task is detached.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use rust_i18n::t;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_replay_data::replay_index::IndexProgress;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::blocking;
use zgui::task::spawn_detached;

use crate::services::Job;
use crate::services::Progress;
use crate::services::progress;
use crate::shell::Shell;

/// The running build, how far it has got, and what the last one said.
#[derive(Clone, Copy)]
pub struct ReplayIndexJobs {
    /// How far the running build has got. `None` while none runs.
    progress: RwSignal<Option<IndexProgress>, LocalStorage>,
    /// What the last build said when it stopped. Cleared when the next starts.
    outcome: RwSignal<Option<String>, LocalStorage>,
    /// The Stop flag the running build checks.
    cancel: StoredValue<Option<Arc<AtomicBool>>>,
    /// Whether a pass the replays tab started is reading replays.
    folder_pass: RwSignal<bool, LocalStorage>,
}

impl ReplayIndexJobs {
    pub fn new() -> Self {
        Self {
            progress: RwSignal::new_local(None),
            outcome: RwSignal::new_local(None),
            cancel: StoredValue::new(None),
            folder_pass: RwSignal::new_local(false),
        }
    }

    pub fn is_running(self) -> bool {
        self.progress.with(Option::is_some)
    }

    pub fn is_running_untracked(self) -> bool {
        self.progress.with_untracked(Option::is_some)
    }

    pub fn progress(self) -> Option<IndexProgress> {
        self.progress.get()
    }

    pub fn outcome(self) -> Option<String> {
        self.outcome.get()
    }

    /// Asks the running build to stop after the replay it is reading.
    pub fn stop(self) {
        if let Some(cancel) = self.cancel.get_value() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Tells the shell whether a pass the replays tab started is reading replays, so its own
    /// build waits for it.
    pub fn set_folder_pass(self, running: bool) {
        if self.folder_pass.get_untracked() != running {
            self.folder_pass.set(running);
        }
    }

    /// Forgets the replays tab's pass, as the tab goes. `try_`, since the shell may be going too.
    pub fn clear_folder_pass(self) {
        let _ = self.folder_pass.try_set(false);
    }
}

impl Default for ReplayIndexJobs {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Walks the install's replays directory and indexes what is in it.
    ///
    /// The whole directory rather than what changed: this is the control for building an index
    /// that is not there, or rebuilding one whose rows an older parse got wrong. Refused without a
    /// game directory, since the replays are under it, and while another build runs.
    pub fn build_replay_index(self, mode: IndexMode) {
        let index = self.replay_index;
        // Two passes at once would each load and save the unreadable-file ledger over the other's.
        if index.progress.with_untracked(Option::is_some) || index.folder_pass.get_untracked() {
            self.toasts().warn(t!("ui.reindex.busy_index").into_owned());
            return;
        }

        let services = self.services();
        let Some(wows_dir) = services.settings.read_untracked(|settings| settings.wows_dir.clone()) else { return };
        if wows_dir.is_empty() {
            return;
        }
        let Some(pool) = services.settings.pool() else { return };
        let Some(runtime) = services.runtime() else { return };
        let Some(game_data) = services.game_data.cache_untracked() else { return };
        let Some(proxy) = services.settings.read_untracked(|settings| settings.proxy_url.clone()) else { return };

        let root = Path::new(&wows_dir).join("replays");
        let cancel = Arc::new(AtomicBool::new(false));
        index.cancel.set_value(Some(Arc::clone(&cancel)));
        index.outcome.set(None);
        index.progress.set(Some(IndexProgress::default()));

        let named = t!("ui.app.status_indexing").into_owned();
        let held = services.jobs.start(Job { name: named.clone(), progress: None });
        let (reports, received) = futures::channel::mpsc::unbounded::<IndexProgress>();

        spawn_detached(async move {
            let working = blocking(move || {
                let live =
                    wows_toolkit_config::index::query::ensure_default_source(&pool, &root, jiff::Timestamp::now());
                let source = runtime.block_on(live)?;
                let rating = owner_rating(&runtime, &proxy);
                wows_toolkit_replay_data::replay_index::build_index(
                    &runtime,
                    &pool,
                    source,
                    &root,
                    &game_data,
                    rating.as_ref(),
                    &cancel,
                    mode,
                    |step| {
                        let _ = reports.unbounded_send(step);
                    },
                )
            });
            let built = progress::follow(working, received, |step| {
                index.progress.set(Some(step));
                held.report(Job { name: named.clone(), progress: Progress::new(step.done, step.total) });
            })
            .await;
            drop(held);
            // `try_`, as the shell may be gone by the time the build answers.
            if index.progress.try_with_untracked(|_| ()).is_none() {
                return;
            }

            index.progress.set(None);
            index.cancel.set_value(None);
            let said = match built {
                Ok(progress) => {
                    let said = if progress.skipped > 0 {
                        t!(
                            "ui.settings.index.built_with_skipped",
                            indexed = progress.indexed,
                            skipped = progress.skipped,
                            failed = progress.failed
                        )
                        .into_owned()
                    } else {
                        t!("ui.settings.index.built", indexed = progress.indexed, failed = progress.failed).into_owned()
                    };
                    if progress.failed > 0 {
                        self.toasts().warn(said.clone());
                    } else {
                        self.toasts().ok(said.clone());
                    }
                    said
                }
                Err(err) => {
                    let said = err.to_string();
                    self.say_failure(said.clone());
                    said
                }
            };
            // Said as well as written under the button: a build started from the palette finishes
            // while the reader is somewhere else.
            index.outcome.set(Some(said));
        });
    }
}

/// The expected-values table each indexed row's owner rating is computed from, read from the
/// cache and refreshed when stale. Run on an indexing thread, since a refresh is a download.
pub fn owner_rating(runtime: &tokio::runtime::Runtime, proxy_url: &str) -> Option<PersonalRatingData> {
    runtime.block_on(wows_toolkit_services::personal_rating::load_refreshed(crate::http::USER_AGENT, proxy_url))
}

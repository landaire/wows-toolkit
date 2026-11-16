//! The per-build result mappings battle results are read through: brought up to date for every
//! listed build, and again for a build a battle was read through the wrong one of.

use std::collections::BTreeSet;

use landaire_ui::Notice;
use landaire_ui::ToastKey;
use rust_i18n::t;
use wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus;
use wows_toolkit_viewmodel::bug_report::record_constants_fetch;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;

use crate::services::constants::Missed;
use crate::services::constants::Outcome;
use crate::services::constants::Provenance;
use crate::services::constants::Unpublished;
use crate::services::constants::Wanted;
use crate::tabs::replays::panel::ReparseCause;
use crate::tabs::replays::workspace::ReplayWorkspace;

/// Keys the notice that a battle was read through a mapping that does not fit, which stands until
/// one that does is on disk.
const CONSTANTS_UNFIT: &str = "replay-constants-unfit";

/// What this session has asked about mappings.
#[derive(Clone, Copy)]
pub struct ResultMappings {
    /// Builds whose mapping has had an answer this session, so a second scan does not ask again.
    asked: StoredValue<BTreeSet<u32>, LocalStorage>,
    /// Builds already reported as read through the wrong mapping, so a second replay on the same
    /// build does not say it again.
    mismatched: StoredValue<BTreeSet<u32>, LocalStorage>,
}

impl ResultMappings {
    pub fn new() -> Self {
        Self { asked: StoredValue::new_local(BTreeSet::new()), mismatched: StoredValue::new_local(BTreeSet::new()) }
    }

    /// Brings the mapping of each of `builds` up to date: the cached one when it fits the build,
    /// else GitHub's, else one generated from the installed client. Small files, so fetched
    /// without asking.
    pub fn fetch_missing(self, workspace: ReplayWorkspace, builds: Vec<(u32, Option<String>)>) {
        #[cfg(test)]
        if !crate::tabs::replays::testing::fetches_allowed() {
            return;
        }

        let wanted: Vec<Wanted> = self.asked.with_value(|asked| {
            builds
                .into_iter()
                .filter(|(build, _)| !asked.contains(build))
                .map(|(build, release)| Wanted { build, release })
                .collect()
        });
        if wanted.is_empty() {
            return;
        }
        let reserved: Vec<u32> = wanted.iter().map(|wanted| wanted.build).collect();
        // Reserved while the update runs, so a second listing's report does not ask again.
        self.asked.update_value(|asked| asked.extend(reserved.iter().copied()));

        let shell = workspace.shell;
        workspace.under_tab(|| {
            spawn_local(async move {
                let outcomes = crate::shell::constants::update(shell, wanted).await;

                let mut fetched = 0usize;
                let mut arrived = false;
                let mut fitted = Vec::new();
                let mut answered = Vec::new();
                for (build, outcome) in &outcomes {
                    let build = *build;
                    record_github(build, outcome);
                    match outcome {
                        Outcome::Current { provenance, .. } => {
                            fitted.push(build);
                            answered.push(build);
                            match provenance {
                                Provenance::GitHub => {
                                    tracing::info!(build, "constants: the result mapping was fetched");
                                    fetched += 1;
                                    arrived = true;
                                }
                                Provenance::Generated { .. } => arrived = true,
                                // Nothing changed on disk.
                                Provenance::Cached => {}
                            }
                        }
                        Outcome::Fallback { missed, .. } => {
                            if !worth_asking_again(missed) {
                                answered.push(build);
                            }
                        }
                    }
                }

                // A build with no answer worth keeping, or one this update did not reach, is asked
                // again on a later scan, and a later mismatch on it tries again.
                let released = |build: &u32| reserved.contains(build) && !answered.contains(build);
                self.asked.update_value(|asked| asked.retain(|build| !released(build)));
                // A mismatch can be resolved by constants another writer cached since it was
                // reported, which reads as `Cached` here.
                let settled = self.mismatched.try_update_value(|mismatched| {
                    let resolved = mismatched.iter().any(|build| fitted.contains(build));
                    mismatched.retain(|build| !released(build) && !fitted.contains(build));
                    (resolved, mismatched.is_empty())
                });
                let resolved = settled.is_some_and(|(resolved, _)| resolved);
                let unfit = settled.map(|(_, empty)| !empty);
                if !arrived && !resolved {
                    return;
                }
                if unfit == Some(false) {
                    shell.toasts().withdraw(&ToastKey::new(CONSTANTS_UNFIT));
                }
                if fetched > 0 {
                    shell.toasts().info(t!("ui.replay.constants_written", count = fetched).into_owned());
                }
                // What is open was read through the mapping that was there before.
                workspace.reparse_all(ReparseCause::GameDataArrived);
            });
        });
    }

    /// Says that a battle was read through the wrong mapping, and brings the build's mapping up to
    /// date. What is on disk is kept until one that fits replaces it: a mapping imported by hand is
    /// the only one some builds have.
    pub fn recover(self, workspace: ReplayWorkspace, build: u32, version: Option<String>) {
        let first = self.mismatched.try_update_value(|mismatched| mismatched.insert(build)).unwrap_or(false);
        if !first {
            return;
        }
        workspace.shell.toasts().show(
            Notice::warn(t!("ui.messages.constants_version_mismatch").into_owned())
                .keyed(ToastKey::new(CONSTANTS_UNFIT))
                .sticky(),
        );
        // The commit the published mappings were last checked at is no answer now, so the next
        // check fetches rather than reporting the repository unchanged.
        workspace.services.settings.edit(keys::CONSTANTS_FILE_COMMIT, |settings| {
            settings.constants_commit = None;
            None::<String>
        });
        // Asked again whatever this session already asked: that answer is what does not fit.
        self.asked.update_value(|asked| {
            asked.remove(&build);
        });
        self.fetch_missing(workspace, vec![(build, version)]);
    }
}

impl Default for ResultMappings {
    fn default() -> Self {
        Self::new()
    }
}

/// Notes for the bug report how GitHub answered for `build`, when it was asked.
fn record_github(build: u32, outcome: &Outcome) {
    let unpublished = match outcome {
        Outcome::Current { provenance: Provenance::GitHub, .. } => {
            record_constants_fetch(ConstantsFetchStatus::Succeeded);
            return;
        }
        Outcome::Current { provenance: Provenance::Cached, .. } => return,
        Outcome::Current { unpublished, .. } => unpublished.as_ref(),
        Outcome::Fallback { missed: Missed::NoClient { unpublished, .. }, .. }
        | Outcome::Fallback { missed: Missed::Generation { unpublished, .. }, .. } => Some(unpublished),
        Outcome::Fallback { .. } => None,
    };

    match unpublished {
        Some(Unpublished::Unreachable { reason }) => {
            tracing::warn!(build, %reason, "constants: the result mapping was not fetched");
            record_constants_fetch(ConstantsFetchStatus::Failed(format!("build {build}: {reason}")));
        }
        Some(Unpublished::Stale { .. }) => record_constants_fetch(ConstantsFetchStatus::Succeeded),
        None => {}
    }
}

/// Whether a build with no mapping that fits is worth asking about again this session. GitHub's
/// answer and a failed generation stand until the next session; what failed for want of a network
/// or a disk may not.
fn worth_asking_again(missed: &Missed) -> bool {
    match missed {
        Missed::NoClient { unpublished, .. } => matches!(unpublished, Unpublished::Unreachable { .. }),
        Missed::Generation { .. } => false,
        Missed::Store { .. } | Missed::WorkerStopped { .. } => true,
    }
}

//! The per-build result mappings battle results are read through: fetched for every listed build
//! that has none, and fetched again for a build a battle was read through the wrong one of.

use std::collections::BTreeSet;

use landaire_ui::Notice;
use landaire_ui::ToastKey;
use rust_i18n::t;
use wows_toolkit_services::constants::Cached;
use wows_toolkit_services::constants::Fetched;
use wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus;
use wows_toolkit_viewmodel::bug_report::record_constants_fetch;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;

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

    /// Fetches the mapping for any of `builds` that has none, or for all of them under
    /// `Cached::Replace`. Small files, so fetched without asking.
    pub fn fetch_missing(self, workspace: ReplayWorkspace, builds: Vec<(u32, Option<String>)>, cached: Cached) {
        #[cfg(test)]
        if !crate::tabs::replays::testing::fetches_allowed() {
            return;
        }

        let wanted: Vec<(u32, Option<String>)> = self.asked.with_value(|asked| {
            builds
                .into_iter()
                .filter(|(build, _)| !asked.contains(build))
                .filter(|(build, _)| cached == Cached::Replace || !wows_toolkit_services::constants::is_cached(*build))
                .collect()
        });
        if wanted.is_empty() {
            return;
        }
        // Reserved while the fetch runs, so a second listing's report does not fetch them again.
        self.asked.update_value(|asked| asked.extend(wanted.iter().map(|(build, _)| *build)));

        let toasts = workspace.shell.toasts();
        workspace.under_tab(|| {
            spawn_local(async move {
                let outcomes =
                    blocking(move || wows_toolkit_services::constants::fetch_for_builds(wanted, cached)).await;

                let mut written = 0usize;
                let mut failed = Vec::new();
                let mut fitted = Vec::new();
                for (build, outcome) in outcomes {
                    match outcome {
                        Fetched::Written { build, actual } => {
                            record_constants_fetch(ConstantsFetchStatus::Succeeded);
                            tracing::info!(build, actual, "constants: the result mapping was fetched");
                            written += 1;
                            fitted.push(build);
                        }
                        Fetched::AlreadyOnDisk => {}
                        // Released: a repository that refused once is asked again on a later scan,
                        // and a later mismatch on the build tries again.
                        Fetched::Failed(reason) => {
                            record_constants_fetch(ConstantsFetchStatus::Failed(format!("build {build}: {reason}")));
                            tracing::warn!(build, %reason, "constants: the result mapping was not fetched");
                            failed.push(build);
                        }
                    }
                }

                self.asked.update_value(|asked| asked.retain(|build| !failed.contains(build)));
                let unfit = self.mismatched.try_update_value(|mismatched| {
                    mismatched.retain(|build| !failed.contains(build) && !fitted.contains(build));
                    !mismatched.is_empty()
                });
                if written == 0 {
                    return;
                }
                if unfit == Some(false) {
                    toasts.withdraw(&ToastKey::new(CONSTANTS_UNFIT));
                }
                toasts.info(t!("ui.replay.constants_written", count = written).into_owned());
                // What is open was read through the mapping that was there before.
                workspace.reparse_all(ReparseCause::GameDataArrived);
            });
        });
    }

    /// Says that a battle was read through the wrong mapping, and fetches the right one over it.
    /// What is on disk is kept until a fetch replaces it: a mapping imported by hand is the only
    /// one some builds have.
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
        self.fetch_missing(workspace, vec![(build, version)], Cached::Replace);
    }
}

impl Default for ResultMappings {
    fn default() -> Self {
        Self::new()
    }
}

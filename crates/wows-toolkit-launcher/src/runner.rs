//! Which Proton or umu build would run the game, found off the UI thread so the Play control
//! can ask for setup before a launch fails.

use std::sync::Arc;

use wgc_launch::ResolvedRunner;
use wgc_launch::RunnerChoice;
use wgc_launch::RunnerError;

use crate::launcher::Event;
use crate::launcher::Launcher;
use crate::launcher::LauncherEnv;
use crate::settings::LauncherSettings;

/// The runner the current settings resolve to, as far as this run has found out.
#[derive(Debug, Clone)]
pub enum RunnerState {
    /// Not looked for yet, or looking.
    Unknown,
    Found(ResolvedRunner),
    Missing(Arc<RunnerError>),
}

impl Launcher {
    pub fn runner(&self) -> &RunnerState {
        &self.runner
    }

    /// Whether Play needs a runner set up first. `Unknown` does not count, so the control does
    /// not flash a setup prompt while the search runs.
    pub fn needs_runner_setup(&self) -> bool {
        matches!(self.runner, RunnerState::Missing(_))
    }

    /// Looks for the runner `settings` choose. Run again after the choice changes.
    pub fn start_runner_probe(&mut self, settings: &LauncherSettings, env: &LauncherEnv) {
        let Some(dirs) = env.runtime_dirs.clone() else {
            return;
        };

        let choice = settings.runner.clone();
        self.runner = RunnerState::Unknown;
        let sender = self.sender(env);
        env.runtime.spawn_blocking(move || {
            let result = wgc_launch::Runner::detect(&choice, &dirs);
            sender.send(Event::RunnerProbed { choice, result });
        });
    }

    pub(crate) fn apply_runner_probe(
        &mut self,
        choice: RunnerChoice,
        result: Result<ResolvedRunner, RunnerError>,
        settings: &LauncherSettings,
    ) {
        // The choice changed while the search ran; a newer probe answers for it.
        if choice != settings.runner {
            return;
        }

        self.runner = match result {
            Ok(runner) => RunnerState::Found(runner),
            Err(err) => RunnerState::Missing(Arc::new(err)),
        };
    }
}

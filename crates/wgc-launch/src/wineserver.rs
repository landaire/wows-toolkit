//! Ends the Wine processes a session leaves in the toolkit's prefix: Wine's services, and
//! anything the game left behind, which would otherwise keep the session running.

use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Once;

use crate::error::PrefixShutdownError;
use crate::runner::ResolvedRunner;

/// Whether a session ends its prefix's Wine processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixShutdown {
    /// Once the runner exits, and when the session is killed. For a program that exits only
    /// after the game and everything it started, such as the WGC relay.
    WithRunner,
    /// Never. A replay's stub exits while the client it started still runs.
    Never,
}

/// `wineserver -k` for one session's prefix, run at most once.
#[derive(Debug, Clone)]
pub(crate) struct PrefixServer(Arc<PrefixServerState>);

impl PrefixServer {
    /// `None` for a runner without a prefix.
    pub(crate) fn new(runner: &ResolvedRunner) -> Option<PrefixServer> {
        let prefix = runner.prefix().root()?.to_path_buf();

        Some(PrefixServer(Arc::new(PrefixServerState { runner: runner.clone(), prefix, ended: Once::new() })))
    }

    /// Ends every process in the prefix, the first time it is called. A later call returns once
    /// the first has finished.
    pub(crate) fn end(&self) {
        self.0.ended.call_once(|| {
            if let Err(err) = self.kill() {
                tracing::warn!("could not end the Wine processes in {}: {err}", self.0.prefix.display());
            }
        });
    }

    /// Runs the runner's own `wineserver`; the host may have no Wine of its own. A prefix with no
    /// server running has nothing to end; `wineserver -k` then exits with a failure status.
    fn kill(&self) -> Result<(), PrefixShutdownError> {
        let PrefixServerState { runner, prefix, .. } = &*self.0;
        let program =
            runner.wineserver().ok_or_else(|| PrefixShutdownError::NoWineserver { prefix: prefix.clone() })?;

        let status = Command::new(&program)
            .arg("-k")
            .env("WINEPREFIX", prefix)
            .stdin(Stdio::null())
            .status()
            .map_err(|source| PrefixShutdownError::Spawn { program, source })?;
        tracing::debug!("wineserver -k for {} exited with {status}", prefix.display());

        Ok(())
    }
}

/// Identity: the handles of one session share its server.
impl PartialEq for PrefixServer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PrefixServer {}

#[derive(Debug)]
struct PrefixServerState {
    runner: ResolvedRunner,
    prefix: PathBuf,
    ended: Once,
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::LaunchArgs;
    use crate::dirs::RuntimeDirs;
    use crate::host::ProtonInstall;
    use crate::lock::SessionLock;
    use crate::runner::Runner;
    use crate::session::GameProcess;
    use crate::spawn::LaunchSpec;
    use crate::spawn::WindowsProgram;
    use crate::view::InstallView;

    /// A Steam Proton runner whose `proton` script runs `proton_body` and whose `wineserver`
    /// appends its arguments and `WINEPREFIX` to `record`.
    struct FakeRuntime {
        _tmp: tempfile::TempDir,
        runner: ResolvedRunner,
        install: PathBuf,
        record: PathBuf,
    }

    impl FakeRuntime {
        fn new(proton_body: &str) -> FakeRuntime {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let record = root.join("wineserver-calls");
            let proton_dir = root.join("Proton - Experimental");
            script(&proton_dir.join("proton"), proton_body);
            script(
                &proton_dir.join("files/bin/wineserver"),
                &format!("printf '%s WINEPREFIX=%s\\n' \"$*\" \"$WINEPREFIX\" >> '{}'", record.display()),
            );
            let install = root.join("World_of_Warships");
            std::fs::create_dir_all(&install).unwrap();

            let proton = ProtonInstall::from_dir(&proton_dir).unwrap();
            let runner = ResolvedRunner::new(
                Runner::SteamProton { proton, steam_root: root.join("Steam") },
                RuntimeDirs::new(&root.join("app")),
            );

            FakeRuntime { _tmp: tmp, runner, install, record }
        }

        fn spawn(&self, prefix_shutdown: PrefixShutdown) -> GameProcess {
            let view = InstallView::Direct { root: self.install.clone() };
            let program =
                WindowsProgram { exe: view.game_exe(), args: Vec::new(), env: Vec::new(), shared_dirs: Vec::new() };
            let spec = LaunchSpec {
                lock: SessionLock::lock(self.runner.dirs()).unwrap(),
                runner: &self.runner,
                view: &view,
                program,
                args: &LaunchArgs::default(),
                prefix_shutdown,
            };

            crate::spawn::spawn(spec, |_| {}).unwrap()
        }

        fn prefix(&self) -> &Path {
            self.runner.prefix().root().unwrap()
        }

        /// Every `wineserver` run, one line each.
        fn wineserver_calls(&self) -> Vec<String> {
            match std::fs::read_to_string(&self.record) {
                Ok(text) => text.lines().map(str::to_owned).collect(),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(err) => panic!("could not read {}: {err}", self.record.display()),
            }
        }
    }

    fn script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_relay_session_ends_its_prefix_once_the_runner_exits() {
        let fake = FakeRuntime::new("exit 0");

        fake.spawn(PrefixShutdown::WithRunner).wait().unwrap();

        assert_eq!(fake.wineserver_calls(), [format!("-k WINEPREFIX={}", fake.prefix().display())]);
    }

    #[test]
    fn killing_a_relay_session_ends_its_prefix_once() {
        let fake = FakeRuntime::new("exec sleep 60");
        let process = fake.spawn(PrefixShutdown::WithRunner);

        process.handle().kill().unwrap();
        process.wait().unwrap();

        assert_eq!(fake.wineserver_calls(), [format!("-k WINEPREFIX={}", fake.prefix().display())]);
    }

    #[test]
    fn a_session_that_does_not_end_its_prefix_leaves_wineserver_alone() {
        let fake = FakeRuntime::new("exec sleep 60");
        let process = fake.spawn(PrefixShutdown::Never);

        process.handle().kill().unwrap();
        process.wait().unwrap();

        assert!(fake.wineserver_calls().is_empty());
    }
}

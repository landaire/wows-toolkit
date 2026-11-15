//! Builds and spawns the runner command for a Windows program inside an install view.

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use crate::LaunchArgs;
use crate::dirs::InstallKey;
use crate::env::GameEnv;
use crate::error::SpawnError;
use crate::host::HostEnv;
use crate::overlay::FuseMount;
use crate::overlay::OverlayLayers;
use crate::overlay::OverlayTools;
use crate::overlay::bwrap_prefix;
use crate::runner::ResolvedRunner;
use crate::runner::Runner;
use crate::session;
use crate::session::GameProcess;
use crate::session::SessionId;
use crate::view::InstallView;

/// WoWs on Steam. Proton and umu select per-game fixes by this id.
const WOWS_APP_ID: &str = "552990";

/// The Windows program the runner starts: the WGC relay, or the game itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsProgram {
    /// Host path of the `.exe`.
    pub exe: PathBuf,
    pub args: Vec<OsString>,
    /// Applied to the runner's environment; Wine passes it to Windows processes.
    pub env: Vec<(OsString, OsString)>,
    /// Host directories the program opens outside the install and `$HOME`,
    /// which a umu container must expose.
    pub shared_dirs: Vec<PathBuf>,
}

impl WindowsProgram {
    /// The game stub, which opens `replay` instead of the port.
    pub fn replay(view: &InstallView, replay: &Path) -> Self {
        Self {
            exe: view.game_exe(),
            args: vec![dos_path(replay)],
            env: Vec::new(),
            shared_dirs: replay.parent().map(Path::to_path_buf).into_iter().collect(),
        }
    }
}

/// The Windows path of a host path. Wine maps `Z:` to `/`; every runner
/// here keeps host paths at their host locations.
#[cfg(unix)]
pub fn dos_path(path: &Path) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    let mut bytes = b"Z:".to_vec();
    bytes.extend(path.as_os_str().as_bytes().iter().map(|byte| if *byte == b'/' { b'\\' } else { *byte }));
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
pub fn dos_path(path: &Path) -> OsString {
    path.as_os_str().to_owned()
}

pub struct LaunchSpec<'a> {
    pub runner: &'a ResolvedRunner,
    pub view: &'a InstallView,
    pub program: WindowsProgram,
    /// `extra` follows `program.args`; `env` applies before `program.env`.
    pub args: &'a LaunchArgs,
}

/// Starts the program through the runner inside the view. `prepare`
/// decorates the command last, for process hardening.
pub fn spawn(spec: LaunchSpec<'_>, prepare: impl FnOnce(&mut Command)) -> Result<GameProcess, SpawnError> {
    // One shared prefix and one game client: a second session would join
    // the first one's wineserver and see the wrong install.
    if cfg!(target_os = "linux") {
        let running = session::running_session_pids().map_err(SpawnError::Scan)?;
        if !running.is_empty() {
            return Err(SpawnError::SessionRunning { pids: running });
        }
    }

    for dir in spec.runner.owned_dirs() {
        std::fs::create_dir_all(&dir).map_err(|source| SpawnError::CreateDir { path: dir, source })?;
    }

    let (session, plan_view, mount) = match spec.view {
        InstallView::Direct { root } => {
            let session = SessionId::new(InstallKey::from_canonical_root(root).as_str());
            (session, PlanView::Direct { root: root.clone() }, None)
        }
        InstallView::Overlay(overlay) => {
            let session = SessionId::new(overlay.key().as_str());
            let layers = overlay.layers();
            match overlay.tools() {
                OverlayTools::KernelBwrap { bwrap } => {
                    (session, PlanView::KernelOverlay { bwrap: bwrap.clone(), layers: layers.clone() }, None)
                }
                OverlayTools::FuseOverlayfs { fuse_overlayfs, fusermount } => {
                    let mount = FuseMount::mount(layers, fuse_overlayfs, fusermount)?;
                    (session, PlanView::Direct { root: layers.merged.clone() }, Some(mount))
                }
            }
        }
    };

    let plan = CommandPlan::for_runner(
        spec.runner,
        &plan_view,
        &spec.program,
        spec.args,
        &HostEnv::from_env(),
        Some(&session),
    );
    tracing::info!("launching {:?} {:?}", plan.program, plan.args);

    let mut command = plan.command();
    command.stdin(Stdio::null());
    // Its own process group, so a terminal's Ctrl-C to the toolkit spares the game.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    prepare(&mut command);

    match command.spawn() {
        Ok(child) => Ok(GameProcess::new(child, session, mount)),
        Err(source) => {
            if let Some(mount) = mount
                && let Err(e) = mount.unmount()
            {
                tracing::warn!("could not unmount the game overlay after a failed launch: {e}");
            }
            Err(SpawnError::Spawn { program: plan.program, source })
        }
    }
}

/// How the planned command reaches the install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanView {
    /// `root` is a real directory on the host: a writable install or a mounted fuse overlay.
    Direct { root: PathBuf },
    /// The command runs inside bwrap, which mounts the overlay at `layers.merged`.
    KernelOverlay { bwrap: PathBuf, layers: OverlayLayers },
}

impl PlanView {
    fn game_root(&self) -> &Path {
        match self {
            Self::Direct { root } => root,
            Self::KernelOverlay { layers, .. } => &layers.merged,
        }
    }
}

/// A fully resolved command, kept apart from `Command` so it can be inspected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandPlan {
    program: PathBuf,
    args: Vec<OsString>,
    cwd: PathBuf,
    env: GameEnv,
}

impl CommandPlan {
    pub(crate) fn for_runner(
        runner: &ResolvedRunner,
        view: &PlanView,
        program: &WindowsProgram,
        args: &LaunchArgs,
        host: &HostEnv,
        session: Option<&SessionId>,
    ) -> Self {
        let game_root = view.game_root();
        let dirs = runner.dirs();

        // Later layers win: host environment, user env, program env, runner
        // requirements. Windows programs need the whole Windows environment
        // (`SystemRoot`, `APPDATA`); Linux runners get the allowlist only.
        let mut env = match runner.runner() {
            Runner::Native => GameEnv::inherit(host),
            Runner::SteamProton { .. } | Runner::Umu { .. } => GameEnv::from_host(host),
        };
        for (name, value) in &args.env {
            env.set(name, value);
        }
        for (name, value) in &program.env {
            env.set(name, value);
        }

        let mut argv: Vec<OsString> = Vec::new();
        match runner.runner() {
            Runner::Native => {}
            Runner::SteamProton { proton, steam_root } => {
                env.set("STEAM_COMPAT_DATA_PATH", dirs.steam_compat_data());
                env.set("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root);
                env.set("STEAM_COMPAT_INSTALL_PATH", game_root);
                env.set("SteamAppId", WOWS_APP_ID);
                env.set("SteamGameId", WOWS_APP_ID);
                argv.push(proton.script().into_os_string());
                argv.push("run".into());
            }
            Runner::Umu { umu_run, proton } => {
                env.set("WINEPREFIX", dirs.umu_prefix());
                env.set("GAMEID", format!("umu-{WOWS_APP_ID}"));
                env.set("UMU_FOLDERS_PATH", dirs.umu_folders());
                env.set("XDG_CACHE_HOME", dirs.umu_cache());
                env.set("STEAM_COMPAT_INSTALL_PATH", game_root);
                if let Some(proton) = proton {
                    env.set("PROTONPATH", proton.dir());
                }

                // pressure-vessel hides host paths outside $HOME unless listed.
                let mut shared: Vec<PathBuf> = env
                    .get("PRESSURE_VESSEL_FILESYSTEMS_RW")
                    .map(|value| std::env::split_paths(value).collect())
                    .unwrap_or_default();
                shared.push(game_root.to_path_buf());
                if let Some(dir) = program.exe.parent().filter(|dir| dir.is_absolute()) {
                    shared.push(dir.to_path_buf());
                }
                shared.extend(program.shared_dirs.iter().cloned());
                let mut seen = std::collections::HashSet::new();
                shared.retain(|dir| seen.insert(dir.clone()));
                if let Ok(joined) = std::env::join_paths(shared) {
                    env.set("PRESSURE_VESSEL_FILESYSTEMS_RW", joined);
                }

                argv.push(umu_run.clone().into_os_string());
            }
        }
        if let Some(session) = session {
            env.set(session::SESSION_ENV, session.as_str());
        }

        argv.push(program.exe.clone().into_os_string());
        argv.extend(program.args.iter().cloned());
        argv.extend(args.extra.iter().map(OsString::from));

        let cwd = match runner.runner() {
            Runner::Native => program.exe.parent().map(Path::to_path_buf).unwrap_or_else(|| game_root.to_path_buf()),
            Runner::SteamProton { .. } | Runner::Umu { .. } => game_root.to_path_buf(),
        };
        if let PlanView::KernelOverlay { bwrap, layers } = view {
            let mut wrapped = bwrap_prefix(bwrap, layers);
            wrapped.append(&mut argv);
            argv = wrapped;
        }

        let mut argv = argv.into_iter();
        let program = PathBuf::from(argv.next().expect("argv always holds the program"));
        Self { program, args: argv.collect(), cwd, env }
    }

    pub(crate) fn program(&self) -> &Path {
        &self.program
    }

    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).current_dir(&self.cwd).env_clear().envs(self.env.iter());
        command
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::dirs::RuntimeDirs;
    use crate::host::ProtonInstall;

    #[derive(serde::Serialize)]
    struct PlanSnapshot {
        argv: Vec<String>,
        cwd: String,
        env: BTreeMap<String, String>,
    }

    fn snapshot(plan: &CommandPlan) -> PlanSnapshot {
        PlanSnapshot {
            argv: std::iter::once(plan.program.display().to_string())
                .chain(plan.args.iter().map(|arg| arg.to_string_lossy().into_owned()))
                .collect(),
            cwd: plan.cwd.display().to_string(),
            env: plan
                .env
                .iter()
                .map(|(name, value)| (name.to_string_lossy().into_owned(), value.to_string_lossy().into_owned()))
                .collect(),
        }
    }

    fn host_env() -> HostEnv {
        HostEnv::from_vars(
            [
                ("HOME", "/home/u"),
                ("USER", "u"),
                ("PATH", "/nix/store/abc-rust/bin:/home/u/.local/bin:/usr/bin"),
                ("LD_LIBRARY_PATH", "/nix/store/abc-vulkan/lib"),
                ("RUST_LOG", "debug"),
                ("VK_ICD_FILENAMES", "/usr/share/vulkan/icd.d/lvp_icd.x86_64.json"),
                ("WAYLAND_DISPLAY", "wayland-1"),
                ("DISPLAY", ":0"),
                ("XDG_RUNTIME_DIR", "/run/user/1000"),
                ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
                ("LC_TIME", "en_GB.UTF-8"),
                ("STEAM_COMPAT_DATA_PATH", "/elsewhere"),
            ]
            .into_iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect(),
        )
    }

    fn relay() -> WindowsProgram {
        WindowsProgram {
            exe: PathBuf::from(
                "/data/game-runtime/umu/prefix/drive_c/ProgramData/Wargaming.net/GameCenter/wgc-relay.exe",
            ),
            args: vec![dos_path(Path::new("/data/game-runtime/installs/wows/merged/WorldOfWarships.exe"))],
            env: vec![("WGC_RELAY_ADDR".into(), "127.0.0.1:40000".into()), ("WGC_RELAY_TOKEN".into(), "secret".into())],
            shared_dirs: vec![PathBuf::from("/mnt/replays")],
        }
    }

    fn launch_args() -> LaunchArgs {
        LaunchArgs { extra: vec!["-safe".into()], env: vec![("PROTON_LOG".into(), "1".into())] }
    }

    fn layers() -> OverlayLayers {
        OverlayLayers {
            lower: PathBuf::from("/mnt/games/World_of_Warships"),
            upper: PathBuf::from("/data/game-runtime/installs/wows/upper"),
            work: PathBuf::from("/data/game-runtime/installs/wows/work"),
            merged: PathBuf::from("/data/game-runtime/installs/wows/merged"),
        }
    }

    fn proton(dir: &Path) -> ProtonInstall {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("proton"), b"").unwrap();
        ProtonInstall::from_dir(dir).unwrap()
    }

    #[test]
    fn steam_proton_in_kernel_overlay() {
        let tmp = tempfile::tempdir().unwrap();
        let proton = proton(&tmp.path().join("Proton - Experimental"));
        let runner = ResolvedRunner::new(
            Runner::SteamProton { proton, steam_root: PathBuf::from("/home/u/.local/share/Steam") },
            RuntimeDirs::new(Path::new("/data")),
        );
        let view = PlanView::KernelOverlay { bwrap: PathBuf::from("/usr/bin/bwrap"), layers: layers() };
        let plan = CommandPlan::for_runner(
            &runner,
            &view,
            &relay(),
            &launch_args(),
            &host_env(),
            Some(&SessionId::new("wows")),
        );

        let mut snap = snapshot(&plan);
        let proton_dir = tmp.path().display().to_string();
        snap.argv.iter_mut().for_each(|arg| *arg = arg.replace(&proton_dir, "[proton-parent]"));
        insta::assert_yaml_snapshot!(snap);
    }

    #[test]
    fn umu_with_writable_install() {
        let runner = ResolvedRunner::new(
            Runner::Umu { umu_run: PathBuf::from("/usr/bin/umu-run"), proton: None },
            RuntimeDirs::new(Path::new("/data")),
        );
        let view = PlanView::Direct { root: PathBuf::from("/games/World_of_Warships") };
        let plan = CommandPlan::for_runner(
            &runner,
            &view,
            &relay(),
            &launch_args(),
            &host_env(),
            Some(&SessionId::new("wows")),
        );

        insta::assert_yaml_snapshot!(snapshot(&plan));
    }

    #[test]
    fn dos_path_maps_to_drive_z() {
        assert_eq!(
            dos_path(Path::new("/mnt/e/WoWs/replays/a b.wowsreplay")),
            OsString::from(r"Z:\mnt\e\WoWs\replays\a b.wowsreplay")
        );
    }
}

//! Runner selection: Steam's Proton when Steam is installed, umu otherwise.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::dirs::RuntimeDirs;
use crate::error::PrefixInitError;
use crate::error::RunnerError;
use crate::host::HostEnv;
use crate::host::HostSearch;
use crate::host::ProtonInstall;
use crate::host::SteamRuntime;
use crate::lock::SessionLock;
use crate::session::GameHandle;
use crate::session::SessionId;
use crate::session::SessionMarker;
use crate::spawn::CommandPlan;
use crate::spawn::PlanView;
use crate::spawn::WindowsProgram;

/// The persisted runner preference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunnerChoice {
    /// Steam's Proton Experimental or newest Proton, else umu-run on `PATH`.
    #[default]
    Auto,
    SteamProton {
        proton_dir: PathBuf,
    },
    Umu {
        /// `None`: `umu-run` on `PATH`.
        umu_run: Option<PathBuf>,
        /// `None`: umu downloads and uses UMU-Proton.
        proton_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runner {
    /// The game runs directly (Windows).
    Native,
    /// `<proton>/proton run`, with the Steam client installed but not necessarily running.
    SteamProton {
        proton: ProtonInstall,
        steam_root: PathBuf,
    },
    Umu {
        umu_run: PathBuf,
        proton: Option<ProtonInstall>,
    },
}

impl Runner {
    pub fn detect(choice: &RunnerChoice, dirs: &RuntimeDirs) -> Result<ResolvedRunner, RunnerError> {
        Self::detect_with(choice, dirs, &HostSearch::from_env())
    }

    pub fn detect_with(
        choice: &RunnerChoice,
        dirs: &RuntimeDirs,
        search: &HostSearch,
    ) -> Result<ResolvedRunner, RunnerError> {
        let runner = if cfg!(windows) { Self::Native } else { Self::select(choice, search)? };

        Ok(ResolvedRunner::new(runner, dirs.clone()))
    }

    fn select(choice: &RunnerChoice, search: &HostSearch) -> Result<Self, RunnerError> {
        match choice {
            RunnerChoice::Auto => {
                if let Some(steam_root) = search.steam_root()
                    && let Some(proton) = search.proton_installs(&steam_root).into_iter().next()
                {
                    return Ok(Self::SteamProton { proton, steam_root });
                }

                let umu_run = search.umu_run().ok_or(RunnerError::NoRunner)?;
                Ok(Self::Umu { umu_run, proton: None })
            }
            RunnerChoice::SteamProton { proton_dir } => {
                let proton = ProtonInstall::from_dir(proton_dir)
                    .ok_or_else(|| RunnerError::ProtonNotFound { dir: proton_dir.clone() })?;
                let steam_root = search.steam_root().ok_or(RunnerError::SteamNotFound)?;
                Ok(Self::SteamProton { proton, steam_root })
            }
            RunnerChoice::Umu { umu_run, proton_dir } => {
                let umu_run = match umu_run {
                    Some(path) if path.is_file() => path.clone(),
                    Some(path) => return Err(RunnerError::UmuNotFound { path: Some(path.clone()) }),
                    None => search.umu_run().ok_or(RunnerError::UmuNotFound { path: None })?,
                };
                let proton = match proton_dir {
                    Some(dir) => Some(
                        ProtonInstall::from_dir(dir).ok_or_else(|| RunnerError::ProtonNotFound { dir: dir.clone() })?,
                    ),
                    None => None,
                };
                Ok(Self::Umu { umu_run, proton })
            }
        }
    }
}

/// A runner bound to its toolkit-owned prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRunner {
    runner: Runner,
    prefix: WinePrefix,
    dirs: RuntimeDirs,
}

impl ResolvedRunner {
    pub fn new(runner: Runner, dirs: RuntimeDirs) -> Self {
        let prefix = match &runner {
            Runner::Native => WinePrefix { root: None },
            Runner::SteamProton { .. } => WinePrefix { root: Some(dirs.steam_compat_data().join("pfx")) },
            Runner::Umu { .. } => WinePrefix { root: Some(dirs.umu_prefix()) },
        };

        Self { runner, prefix, dirs }
    }

    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    pub fn prefix(&self) -> &WinePrefix {
        &self.prefix
    }

    pub fn dirs(&self) -> &RuntimeDirs {
        &self.dirs
    }

    /// One-time work the next launch will do before the game starts.
    pub fn first_run_work(&self) -> FirstRunWork {
        let prefix_creation = self.prefix.root.is_some() && !self.prefix.is_initialized();

        let mut umu_downloads = Vec::new();
        if let Runner::Umu { proton, .. } = &self.runner {
            let folders = self.dirs.umu_folders();
            let runtime = match proton {
                Some(proton) => proton.required_runtime(),
                None => {
                    if !has_entry_with_prefix(&self.dirs.umu_compat_tools(), "UMU-Proton") {
                        umu_downloads.push(UmuComponent::UmuProton);
                    }
                    Some(SteamRuntime::Sniper)
                }
            };

            if let Some(runtime) = runtime {
                let present = runtime.umu_folder().is_some_and(|name| folders.join("umu").join(name).is_dir());
                if !present {
                    umu_downloads.push(UmuComponent::Runtime(runtime));
                }
            }
        }

        FirstRunWork { prefix_creation, umu_downloads }
    }

    /// Creates the Wine prefix by running `wineboot -u` through the runner.
    /// Blocking; takes seconds with Steam's Proton and minutes when umu downloads.
    /// The lock keeps a session or another setup from using the prefix meanwhile.
    pub fn initialize_prefix(&self, _lock: &SessionLock) -> Result<(), PrefixInitError> {
        let Some(root) = self.prefix.root() else {
            return Ok(());
        };

        let marker = SessionMarker { id: SessionId::new("prefix-setup"), install: None };
        let plan = self.prefix_setup_plan(&HostEnv::from_env(), &marker);

        for dir in self.owned_dirs() {
            std::fs::create_dir_all(&dir).map_err(|source| PrefixInitError::CreateDir { path: dir, source })?;
        }

        let mut command = plan.command();
        command.stdin(std::process::Stdio::null());
        let status = command
            .status()
            .map_err(|source| PrefixInitError::Spawn { program: plan.program().to_path_buf(), source })?;
        // wineserver outlives wineboot. A game started meanwhile would join it under the setup marker.
        GameHandle::from_marker(marker, None, None).wait_exit();
        if !status.success() {
            return Err(PrefixInitError::Failed { status });
        }
        if !self.prefix.is_initialized() {
            return Err(PrefixInitError::NotCreated { path: root.to_path_buf() });
        }

        Ok(())
    }

    /// `wineboot -u` through the runner. `marker` lets a later toolkit process see setup's Wine
    /// processes after this one exits.
    pub(crate) fn prefix_setup_plan(&self, host: &HostEnv, marker: &SessionMarker) -> CommandPlan {
        let program = WindowsProgram {
            exe: PathBuf::from("wineboot"),
            args: vec!["-u".into()],
            env: Vec::new(),
            shared_dirs: Vec::new(),
        };

        CommandPlan::for_runner(
            self,
            &PlanView::Direct { root: self.dirs.root().to_path_buf() },
            &program,
            &crate::LaunchArgs::default(),
            host,
            marker,
        )
    }

    /// Directories the runner requires to exist before it starts.
    pub(crate) fn owned_dirs(&self) -> Vec<PathBuf> {
        match &self.runner {
            Runner::Native => Vec::new(),
            Runner::SteamProton { .. } => vec![self.dirs.steam_compat_data()],
            Runner::Umu { .. } => vec![self.dirs.umu_prefix(), self.dirs.umu_folders(), self.dirs.umu_cache()],
        }
    }

    /// The `wineserver` of the Wine build the runner starts. With no Proton chosen, umu runs the
    /// newest UMU-Proton it has downloaded. `None` for native launches and before that download.
    pub(crate) fn wineserver(&self) -> Option<PathBuf> {
        match &self.runner {
            Runner::Native => None,
            Runner::SteamProton { proton, .. } | Runner::Umu { proton: Some(proton), .. } => proton.wineserver(),
            Runner::Umu { proton: None, .. } => {
                let entries = std::fs::read_dir(self.dirs.umu_compat_tools()).ok()?;
                let mut installs: Vec<ProtonInstall> = entries
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("UMU-Proton"))
                    .filter_map(|entry| ProtonInstall::from_dir(&entry.path()))
                    .collect();
                installs.sort_by(ProtonInstall::preference);

                installs.first()?.wineserver()
            }
        }
    }
}

/// The Wine prefix a runner uses. Absent for native launches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinePrefix {
    root: Option<PathBuf>,
}

impl WinePrefix {
    /// The directory that holds `drive_c` and the registry files.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn drive_c(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join("drive_c"))
    }

    pub fn is_initialized(&self) -> bool {
        self.root.as_ref().is_some_and(|root| root.join("system.reg").is_file())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstRunWork {
    pub prefix_creation: bool,
    pub umu_downloads: Vec<UmuComponent>,
}

impl FirstRunWork {
    pub fn is_empty(&self) -> bool {
        !self.prefix_creation && self.umu_downloads.is_empty()
    }
}

/// Something umu downloads on first use (about 700 MB in total).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UmuComponent {
    UmuProton,
    Runtime(SteamRuntime),
}

fn has_entry_with_prefix(dir: &Path, prefix: &str) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(prefix)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn auto_prefers_steam_proton_then_umu() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        executable(&bin.join("umu-run"));
        let dirs = RuntimeDirs::new(&home.path().join("data"));
        let search = HostSearch::with_home(home.path().to_path_buf(), Some(bin.clone().into_os_string()));

        let resolved = Runner::detect_with(&RunnerChoice::Auto, &dirs, &search).unwrap();
        assert!(matches!(resolved.runner(), Runner::Umu { proton: None, .. }));
        assert_eq!(resolved.prefix().root(), Some(dirs.umu_prefix().as_path()));

        // Steam without any Proton still falls back to umu.
        let steam = home.path().join(".local/share/Steam/steamapps/common");
        std::fs::create_dir_all(&steam).unwrap();
        let resolved = Runner::detect_with(&RunnerChoice::Auto, &dirs, &search).unwrap();
        assert!(matches!(resolved.runner(), Runner::Umu { .. }));

        executable(&steam.join("Proton - Experimental/proton"));
        let resolved = Runner::detect_with(&RunnerChoice::Auto, &dirs, &search).unwrap();
        assert!(matches!(resolved.runner(), Runner::SteamProton { .. }));
        assert_eq!(resolved.prefix().root(), Some(dirs.steam_compat_data().join("pfx").as_path()));
    }

    #[cfg(not(windows))]
    #[test]
    fn auto_without_steam_or_umu_reports_no_runner() {
        let home = tempfile::tempdir().unwrap();
        let dirs = RuntimeDirs::new(home.path());
        let search = HostSearch::with_home(home.path().to_path_buf(), None);
        assert!(matches!(Runner::detect_with(&RunnerChoice::Auto, &dirs, &search), Err(RunnerError::NoRunner)));
    }

    #[test]
    fn first_run_work_lists_missing_umu_parts_and_prefix() {
        let data = tempfile::tempdir().unwrap();
        let dirs = RuntimeDirs::new(data.path());
        let resolved =
            ResolvedRunner::new(Runner::Umu { umu_run: PathBuf::from("/usr/bin/umu-run"), proton: None }, dirs.clone());
        assert_eq!(
            resolved.first_run_work(),
            FirstRunWork {
                prefix_creation: true,
                umu_downloads: vec![UmuComponent::UmuProton, UmuComponent::Runtime(SteamRuntime::Sniper)]
            }
        );

        std::fs::create_dir_all(dirs.umu_folders().join("Steam/compatibilitytools.d/UMU-Proton-10.0-4")).unwrap();
        std::fs::create_dir_all(dirs.umu_folders().join("umu/steamrt3")).unwrap();
        std::fs::create_dir_all(dirs.umu_prefix()).unwrap();
        std::fs::write(dirs.umu_prefix().join("system.reg"), b"").unwrap();
        assert!(resolved.first_run_work().is_empty());
    }

    #[test]
    fn umu_without_a_chosen_proton_uses_the_newest_umu_proton_wineserver() {
        let data = tempfile::tempdir().unwrap();
        let dirs = RuntimeDirs::new(data.path());
        let resolved =
            ResolvedRunner::new(Runner::Umu { umu_run: PathBuf::from("/usr/bin/umu-run"), proton: None }, dirs.clone());
        assert_eq!(resolved.wineserver(), None);

        for name in ["UMU-Proton-9.0-3", "UMU-Proton-10.0-4"] {
            let dir = dirs.umu_compat_tools().join(name);
            executable(&dir.join("proton"));
            executable(&dir.join("files/bin/wineserver"));
        }
        assert_eq!(resolved.wineserver(), Some(dirs.umu_compat_tools().join("UMU-Proton-10.0-4/files/bin/wineserver")));
    }
}

//! Host discovery: the Steam client, Proton builds and umu-run. Filesystem
//! and PATH lookups only, so detection is cheap enough to run from UI code.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

/// The parts of the process environment that discovery reads. Captured once
/// so detection is deterministic and testable.
#[derive(Debug, Clone, Default)]
pub struct HostSearch {
    home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    path: Option<OsString>,

    /// `STEAM_EXTRA_COMPAT_TOOLS_PATHS`, which Steam also honors.
    extra_compat_tools: Vec<PathBuf>,
    system_compat_tools: Vec<PathBuf>,
}

impl HostSearch {
    pub fn from_env() -> Self {
        let extra_compat_tools = std::env::var_os("STEAM_EXTRA_COMPAT_TOOLS_PATHS")
            .map(|paths| std::env::split_paths(&paths).collect())
            .unwrap_or_default();

        Self {
            home: std::env::var_os("HOME").map(PathBuf::from),
            xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            path: std::env::var_os("PATH"),
            extra_compat_tools,
            system_compat_tools: vec![
                PathBuf::from("/usr/share/steam/compatibilitytools.d"),
                PathBuf::from("/usr/local/share/steam/compatibilitytools.d"),
            ],
        }
    }

    /// A search rooted at `home` with the given `PATH`, ignoring the process environment.
    pub fn with_home(home: PathBuf, path: Option<OsString>) -> Self {
        Self {
            home: Some(home),
            xdg_data_home: None,
            path,
            extra_compat_tools: Vec::new(),
            system_compat_tools: Vec::new(),
        }
    }

    /// The Steam client root: the first candidate that has `steamapps/`.
    pub fn steam_root(&self) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(xdg) = &self.xdg_data_home {
            candidates.push(xdg.join("Steam"));
        }
        if let Some(home) = &self.home {
            candidates.push(home.join(".local/share/Steam"));
            candidates.push(home.join(".steam/root"));
            candidates.push(home.join(".steam/steam"));
        }

        candidates
            .into_iter()
            .filter(|candidate| candidate.join("steamapps").is_dir())
            .find_map(|candidate| std::fs::canonicalize(candidate).ok())
    }

    /// Every Proton build Steam would offer, best first: Proton Experimental,
    /// then numbered releases newest first, then the rest (Hotfix, custom builds).
    pub fn proton_installs(&self, steam_root: &Path) -> Vec<ProtonInstall> {
        let mut parents: Vec<PathBuf> = std::iter::once(steam_root.to_path_buf())
            .chain(wgc_install::steam::libraries_from_root(steam_root))
            .map(|library| library.join("steamapps").join("common"))
            .collect();
        parents.push(steam_root.join("compatibilitytools.d"));
        parents.extend(self.extra_compat_tools.iter().cloned());
        parents.extend(self.system_compat_tools.iter().cloned());

        let mut seen = std::collections::HashSet::new();
        let mut installs = Vec::new();
        for parent in parents {
            let Ok(entries) = std::fs::read_dir(&parent) else {
                continue;
            };

            for entry in entries.flatten() {
                let Ok(dir) = std::fs::canonicalize(entry.path()) else {
                    continue;
                };
                if !seen.insert(dir.clone()) {
                    continue;
                }
                if let Some(install) = ProtonInstall::from_dir(&dir) {
                    installs.push(install);
                }
            }
        }

        installs.sort_by(ProtonInstall::preference);
        installs
    }

    /// `umu-run` on `PATH`.
    pub fn umu_run(&self) -> Option<PathBuf> {
        self.find_on_path(OsStr::new("umu-run"))
    }

    pub(crate) fn find_on_path(&self, program: &OsStr) -> Option<PathBuf> {
        let path = self.path.as_ref()?;
        std::env::split_paths(path).map(|dir| dir.join(program)).find(|candidate| is_executable(candidate))
    }
}

/// A Proton build: a directory with a `proton` script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonInstall {
    dir: PathBuf,
    name: String,
    kind: ProtonKind,

    /// First field of Proton's `version` file, a build timestamp. Orders
    /// builds that have no version in their name.
    build_stamp: Option<u64>,
}

impl ProtonInstall {
    pub fn from_dir(dir: &Path) -> Option<Self> {
        if !dir.join("proton").is_file() {
            return None;
        }

        let name = dir.file_name()?.to_string_lossy().into_owned();
        let kind = ProtonKind::from_dir_name(&name);
        let build_stamp = std::fs::read_to_string(dir.join("version"))
            .ok()
            .and_then(|text| text.split_whitespace().next().and_then(|stamp| stamp.parse().ok()));

        Some(Self { dir: dir.to_path_buf(), name, kind, build_stamp })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn kind(&self) -> ProtonKind {
        self.kind
    }

    /// The directory name, as Steam lists the build.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn script(&self) -> PathBuf {
        self.dir.join("proton")
    }

    /// The Steam runtime this build runs in under umu, from `toolmanifest.vdf`.
    pub fn required_runtime(&self) -> Option<SteamRuntime> {
        let manifest = std::fs::read_to_string(self.dir.join("toolmanifest.vdf")).ok()?;
        let line = manifest.lines().find(|line| line.contains("\"require_tool_appid\""))?;
        let app_id: u32 = line.split('"').nth(3)?.parse().ok()?;
        Some(SteamRuntime::from_app_id(app_id))
    }

    fn preference(a: &Self, b: &Self) -> Ordering {
        fn rank(kind: ProtonKind) -> u8 {
            match kind {
                ProtonKind::Experimental => 0,
                ProtonKind::Versioned(_) => 1,
                ProtonKind::Other => 2,
            }
        }

        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| match (a.kind, b.kind) {
                (ProtonKind::Versioned(va), ProtonKind::Versioned(vb)) => vb.cmp(&va),
                _ => Ordering::Equal,
            })
            .then_with(|| b.build_stamp.cmp(&a.build_stamp))
            .then_with(|| a.dir.cmp(&b.dir))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonKind {
    Experimental,
    Versioned(ProtonVersion),
    Other,
}

impl ProtonKind {
    /// "Proton - Experimental", "Proton 10.0", "GE-Proton10-25", "UMU-Proton-10.0-4".
    fn from_dir_name(name: &str) -> Self {
        if name == "Proton - Experimental" {
            return Self::Experimental;
        }

        let Some(at) = name.find("Proton") else {
            return Self::Other;
        };
        let rest = name[at + "Proton".len()..].trim_start_matches([' ', '-']);
        let mut numbers = rest.split(['.', '-']);
        let Some(Ok(major)) = numbers.next().map(str::parse::<u32>) else {
            return Self::Other;
        };
        let minor = numbers.next().and_then(|part| part.parse::<u32>().ok());

        Self::Versioned(ProtonVersion { major, minor })
    }
}

/// Ordered by `major`, then `minor`; an absent minor sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtonVersion {
    pub major: u32,
    pub minor: Option<u32>,
}

/// Steam Linux Runtime generations, named as umu names their folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamRuntime {
    Soldier,
    Sniper,
    SteamRt4,
    Unknown { app_id: u32 },
}

impl SteamRuntime {
    fn from_app_id(app_id: u32) -> Self {
        match app_id {
            1_391_110 => Self::Soldier,
            1_628_350 => Self::Sniper,
            4_183_110 => Self::SteamRt4,
            _ => Self::Unknown { app_id },
        }
    }

    /// The folder umu downloads this runtime into, under `$UMU_FOLDERS_PATH/umu`.
    pub fn umu_folder(self) -> Option<&'static str> {
        match self {
            Self::Soldier => Some("steamrt2"),
            Self::Sniper => Some("steamrt3"),
            Self::SteamRt4 => Some("steamrt4"),
            Self::Unknown { .. } => None,
        }
    }
}

/// The process environment, captured for building a clean game environment.
#[derive(Debug, Clone, Default)]
pub(crate) struct HostEnv {
    vars: Vec<(OsString, OsString)>,
}

impl HostEnv {
    pub(crate) fn from_env() -> Self {
        Self { vars: std::env::vars_os().collect() }
    }

    #[cfg(test)]
    pub(crate) fn from_vars(vars: Vec<(OsString, OsString)>) -> Self {
        Self { vars }
    }

    pub(crate) fn vars(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.vars.iter().map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_proton(parent: &Path, name: &str, stamp: Option<u64>) {
        let dir = parent.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("proton"), b"#!/bin/sh\n").unwrap();
        if let Some(stamp) = stamp {
            std::fs::write(dir.join("version"), format!("{stamp} build\n")).unwrap();
        }
    }

    #[test]
    fn proton_builds_are_ordered_experimental_then_newest() {
        let home = tempfile::tempdir().unwrap();
        let steam = home.path().join(".local/share/Steam");
        let common = steam.join("steamapps/common");
        let compat_tools = steam.join("compatibilitytools.d");
        std::fs::create_dir_all(&common).unwrap();
        std::fs::create_dir_all(compat_tools.join("not-a-tool")).unwrap();
        std::fs::create_dir_all(common.join("Proton BattlEye Runtime")).unwrap();
        fake_proton(&common, "Proton 9.0", Some(1_700_000_000));
        fake_proton(&common, "Proton 10.0", Some(1_710_000_000));
        fake_proton(&common, "Proton Hotfix", Some(1_790_000_000));
        fake_proton(&common, "Proton - Experimental", Some(1_793_000_000));
        fake_proton(&compat_tools, "GE-Proton10-25", None);
        fake_proton(&compat_tools, "Custom", Some(1_600_000_000));

        let search = HostSearch::with_home(home.path().to_path_buf(), None);
        let root = search.steam_root().expect("steam root");
        let names: Vec<String> = search.proton_installs(&root).iter().map(|proton| proton.name().to_owned()).collect();

        insta::assert_yaml_snapshot!(names);
    }

    #[test]
    fn proton_kind_reads_names_steam_and_custom_builds_use() {
        let kinds: Vec<(&str, ProtonKind)> = [
            "Proton - Experimental",
            "Proton 10.0",
            "Proton 9.0-GE",
            "GE-Proton10-25",
            "UMU-Proton-10.0-4",
            "Proton Hotfix",
        ]
        .into_iter()
        .map(|name| (name, ProtonKind::from_dir_name(name)))
        .collect();

        insta::assert_debug_snapshot!(kinds);
    }

    #[test]
    fn no_steam_root_without_steamapps() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".local/share/Steam")).unwrap();
        assert!(HostSearch::with_home(home.path().to_path_buf(), None).steam_root().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn umu_run_is_found_only_when_executable() {
        use std::os::unix::fs::PermissionsExt;

        let bin = tempfile::tempdir().unwrap();
        let umu = bin.path().join("umu-run");
        std::fs::write(&umu, b"#!/bin/sh\n").unwrap();
        let search = HostSearch::with_home(PathBuf::from("/nonexistent"), Some(bin.path().as_os_str().to_owned()));
        assert_eq!(search.umu_run(), None);

        std::fs::set_permissions(&umu, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(search.umu_run(), Some(umu));
    }
}

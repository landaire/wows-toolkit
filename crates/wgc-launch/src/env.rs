//! The game's environment, built from an allowlist of the toolkit's own.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ffi::OsString;

use crate::host::HostEnv;

/// Variables the game and the runner need from the desktop session. Anything
/// else (`LD_LIBRARY_PATH`, renderer overrides, Rust logging) stays with the toolkit.
const PASSTHROUGH: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TZ",
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_DESKTOP",
    "XDG_CURRENT_DESKTOP",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "DBUS_SESSION_BUS_ADDRESS",
    "DBUS_SYSTEM_BUS_ADDRESS",
    "PULSE_SERVER",
    "PIPEWIRE_REMOTE",
];

/// Search-path variables kept with Nix store entries removed: a toolkit run
/// from a Nix shell would otherwise hand its toolchain to Proton's Python.
const FILTERED_PATHS: &[&str] = &["PATH", "XDG_DATA_DIRS"];

const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// An ordered environment, so command construction is deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameEnv {
    vars: BTreeMap<OsString, OsString>,
}

impl GameEnv {
    /// The complete host environment, for programs that run natively.
    pub fn inherit(host: &HostEnv) -> Self {
        Self { vars: host.vars().map(|(name, value)| (name.to_owned(), value.to_owned())).collect() }
    }

    pub fn from_host(host: &HostEnv) -> Self {
        let mut vars = BTreeMap::new();
        for (name, value) in host.vars() {
            let Some(name_str) = name.to_str() else {
                continue;
            };

            if PASSTHROUGH.contains(&name_str) || name_str.starts_with("LC_") {
                vars.insert(name.to_owned(), value.to_owned());
            } else if FILTERED_PATHS.contains(&name_str)
                && let Some(filtered) = without_nix_store(value)
            {
                vars.insert(name.to_owned(), filtered);
            }
        }
        vars.entry(OsString::from("PATH")).or_insert_with(|| OsString::from(DEFAULT_PATH));

        Self { vars }
    }

    pub fn set(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) {
        self.vars.insert(name.into(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars.get(OsStr::new(name)).map(OsString::as_os_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.vars.iter().map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }
}

fn without_nix_store(value: &OsStr) -> Option<OsString> {
    let kept: Vec<_> = std::env::split_paths(value).filter(|entry| !entry.starts_with("/nix/store")).collect();
    if kept.is_empty() {
        return None;
    }

    std::env::join_paths(kept).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_the_system_path_when_only_nix_entries_exist() {
        let host = HostEnv::from_vars(vec![(OsString::from("PATH"), OsString::from("/nix/store/abc-python/bin"))]);
        assert_eq!(GameEnv::from_host(&host).get("PATH"), Some(OsStr::new(DEFAULT_PATH)));
    }
}

use std::path::Path;

use crate::settings::LauncherSettings;

/// Outcome of a Proton auto-detect attempt, so the settings panel and the
/// per-install "Set up Proton" prompt report it identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonSetup {
    /// Binary set and, because none was chosen, a prefix too.
    Ready(PrefixChoice),
    /// Binary set; an existing user-chosen prefix was left alone.
    BinaryOnly,
    /// A Proton binary was found but no app data dir exists for a managed prefix.
    NoManagedPrefixDir,
    /// Steam is installed but carries no Proton under its libraries.
    NoProton,
    /// No Steam installation was found.
    NoSteam,
}

/// Which prefix auto-detect chose when none was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixChoice {
    /// The prefix Steam made for its own copy of WoWs.
    SteamWows,
    /// A toolkit-managed prefix Proton initializes on first launch.
    Managed,
}

/// How a front end should present a [`ProtonSetup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Success,
    Warning,
    Error,
}

impl ProtonSetup {
    /// Whether the settings were changed, so the caller knows to persist.
    pub fn changed(self) -> bool {
        matches!(self, ProtonSetup::Ready(_) | ProtonSetup::BinaryOnly)
    }

    pub fn severity(self) -> Severity {
        match self {
            ProtonSetup::Ready(_) | ProtonSetup::BinaryOnly => Severity::Success,
            ProtonSetup::NoManagedPrefixDir | ProtonSetup::NoProton => Severity::Warning,
            ProtonSetup::NoSteam => Severity::Error,
        }
    }
}

impl std::fmt::Display for ProtonSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ProtonSetup::Ready(PrefixChoice::SteamWows) => "Proton detected. Reusing your Steam WoWs prefix.",
            ProtonSetup::Ready(PrefixChoice::Managed) => {
                "Proton detected. Using a toolkit-managed prefix (created on first launch)."
            }
            ProtonSetup::BinaryOnly => "Proton binary detected.",
            ProtonSetup::NoManagedPrefixDir => {
                "Found a Proton binary, but could not determine a data directory for a managed prefix. Set the prefix manually."
            }
            ProtonSetup::NoProton => {
                "Found a Steam installation, but no Proton install under its library. Install Proton via Steam, then try again."
            }
            ProtonSetup::NoSteam => "Could not find a Steam installation. Proton needs Steam installed.",
        })
    }
}

/// Fill in `settings.proton_binary` and, when unset, `settings.proton_prefix`
/// from the local Steam/Proton install. A non-Steam (WGC-style) install has no
/// Steam prefix to reuse, so it falls back to a toolkit-managed prefix under
/// `storage_dir` that Proton initializes on first launch.
pub fn autodetect(settings: &mut LauncherSettings, storage_dir: Option<&Path>) -> ProtonSetup {
    let Some(steam_root) = wgc_launch::proton::find_steam_root() else {
        return ProtonSetup::NoSteam;
    };
    // With no prefix chosen yet there is no `config_info` to read, so
    // `find_proton_in_library` falls back to the newest Proton in the library.
    let compat_data = settings.proton_prefix.clone().unwrap_or_else(|| steam_root.clone());
    let Some(binary) = wgc_launch::proton::find_proton_in_library(&steam_root, &compat_data) else {
        return ProtonSetup::NoProton;
    };
    settings.proton_binary = Some(binary);
    if settings.proton_prefix.is_some() {
        return ProtonSetup::BinaryOnly;
    }
    if let Some(existing) = wgc_launch::proton::steam_wows_prefix() {
        settings.proton_prefix = Some(existing);
        return ProtonSetup::Ready(PrefixChoice::SteamWows);
    }
    match storage_dir {
        Some(base) => {
            settings.proton_prefix = Some(wgc_launch::proton::managed_prefix(base));
            ProtonSetup::Ready(PrefixChoice::Managed)
        }
        None => ProtonSetup::NoManagedPrefixDir,
    }
}

/// Whether an install needs Proton set up before it can launch: on Linux a
/// non-Steam install runs through Proton, and Steam installs through Steam.
pub fn needs_setup(install: &wgc_install::GameInstall, settings: &LauncherSettings) -> bool {
    cfg!(target_os = "linux") && !install.source.is_read_only_content() && settings.proton_config().is_none()
}

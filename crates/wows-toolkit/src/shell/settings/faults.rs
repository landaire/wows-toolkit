//! What keeps a setting from working as it stands. A row's mark and note, the section strip and the
//! gear all read this one source, so each clears the moment the fault does.

use std::path::Path;

use wows_toolkit_viewmodel::mcp::McpAddressError;
use wt_translations::t;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::shell::Shell;
use crate::shell::settings::Setting;
use crate::shell::settings::SettingsSection;
use crate::shell::settings::present;
use crate::shell::twitch::TwitchPaste;
use crate::shell::twitch::TwitchStatus;

/// One reason a setting does not work as it stands.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SettingFault {
    /// The game directory holds no install.
    WowsDirInvalid,
    /// The last paste was not a credential.
    TwitchPasteRefused { reason: String },
    /// Twitch would not take the stored credential.
    TwitchTokenRefused,
    /// Exporting is on and its directory is not there.
    AutoExportMissing,
    /// Cached builds a validation found out of date or damaged.
    CacheRepairNeeded { builds: usize },
    /// The last download into the cache failed.
    CacheDownloadFailed { reason: String },
    /// The typed MCP bind address is not an IP address.
    McpAddressInvalid { input: String },
    /// The typed or saved MCP bind address is not a loopback one, so the server does not start.
    McpAddressNotLoopback { input: String },
    /// The typed MCP port is not one from 1 to 65535.
    McpPortInvalid { input: String },
    /// Another program holds the MCP server's port.
    McpPortInUse,
}

impl SettingFault {
    /// What the row's note says about it, in the current language.
    pub fn message(&self) -> String {
        match self {
            Self::WowsDirInvalid => t!("ui-messages-wows_dir_invalid").into_owned(),
            Self::TwitchPasteRefused { reason } | Self::CacheDownloadFailed { reason } => reason.clone(),
            Self::TwitchTokenRefused => t!("ui-messages-twitch_token_invalid").into_owned(),
            Self::AutoExportMissing => t!("ui-settings-replay-export_path_missing").into_owned(),
            Self::CacheRepairNeeded { builds } => {
                t!("ui-settings-wows-cache-repair_needed", count = builds).into_owned()
            }
            Self::McpAddressInvalid { input } => t!("ui-settings-mcp-address_invalid", input = input).into_owned(),
            Self::McpAddressNotLoopback { input } => {
                t!("ui-settings-mcp-address_not_loopback", input = input).into_owned()
            }
            Self::McpPortInvalid { input } => t!("ui-settings-mcp-port_invalid", input = input).into_owned(),
            Self::McpPortInUse => t!("ui-settings-mcp-port_in_use").into_owned(),
        }
    }
}

/// What the auto-export look last saw.
struct Looked {
    /// `None` until the settings are read.
    directory: Option<String>,
    opened: bool,
}

/// Every fault `setting` has now, tracked.
pub fn setting_faults(shell: Shell, setting: Setting) -> Vec<SettingFault> {
    let mut faults = Vec::new();

    match setting {
        Setting::WowsDirectory => {
            if shell.settings_drawer.wows_dir_invalid.get() {
                faults.push(SettingFault::WowsDirInvalid);
            }
        }
        Setting::TwitchToken => {
            if let Some(TwitchPaste::Refused { reason }) = shell.twitch.paste() {
                faults.push(SettingFault::TwitchPasteRefused { reason });
            }
            if shell.twitch.status() == TwitchStatus::Refused {
                faults.push(SettingFault::TwitchTokenRefused);
            }
        }
        Setting::AutoExport => {
            // A directory nothing is written to loses nothing.
            let enabled = shell.services().settings.read(|settings| settings.replay.auto_export_data) == Some(true);
            if enabled && shell.settings_drawer.auto_export_missing.get() {
                faults.push(SettingFault::AutoExportMissing);
            }
        }
        Setting::CachedBuilds => shell.cache.with(|cache| {
            if !cache.repair.is_empty() {
                faults.push(SettingFault::CacheRepairNeeded { builds: cache.repair.len() });
            }
            if let Some(reason) = cache.failure.clone() {
                faults.push(SettingFault::CacheDownloadFailed { reason });
            }
        }),
        Setting::McpBindAddress => {
            match shell.mcp.address_refused() {
                Some(McpAddressError::NotAnAddress { input }) => faults.push(SettingFault::McpAddressInvalid { input }),
                Some(McpAddressError::NotLoopback { input }) => {
                    faults.push(SettingFault::McpAddressNotLoopback { input });
                }
                None => {}
            }
            let saved = shell.services().settings.read(|settings| settings.mcp.bind_address);
            if let Some(saved) = saved.filter(|saved| !saved.is_loopback()) {
                faults.push(SettingFault::McpAddressNotLoopback { input: saved.to_string() });
            }
        }
        Setting::McpPort => {
            if let Some(input) = shell.mcp.port_refused() {
                faults.push(SettingFault::McpPortInvalid { input });
            }
            if shell.mcp_port_in_use() {
                faults.push(SettingFault::McpPortInUse);
            }
        }
        // Nothing else has a state that keeps it from working.
        _ => {}
    }

    faults
}

/// Whether a listed setting of `section` has a fault, tracked.
pub fn section_faulted(shell: Shell, section: SettingsSection) -> bool {
    Setting::ALL.into_iter().filter(|setting| setting.section() == section).any(|setting| faulted(shell, setting))
}

/// Whether any listed setting has a fault, tracked.
pub fn any_faulted(shell: Shell) -> bool {
    Setting::ALL.into_iter().any(|setting| faulted(shell, setting))
}

/// Whether `setting` has a fault and a row to show it on. The fault is read first: most settings
/// have none, and whether a row is listed reads the whole settings snapshot.
fn faulted(shell: Shell, setting: Setting) -> bool {
    !setting_faults(shell, setting).is_empty() && present(shell, setting)
}

/// Looks for the auto-export directory whenever it changes and each time the drawer opens. A
/// directory that is not there cannot be written to, and the reader has no other way to find that
/// out until a battle is lost. Looked for off the UI thread, since a directory on an offline share
/// can take many seconds to answer.
pub fn watch_auto_export_dir(shell: Shell) {
    let settings = shell.services().settings;
    let control = shell.settings_drawer;
    let looking = StoredValue::new_local(None::<Task>);

    let checking = RenderEffect::new(move |previous: Option<Looked>| {
        let now = Looked {
            directory: settings.read(|settings| settings.replay.auto_export_path.clone()),
            opened: control.is_open(),
        };
        let changed = previous.as_ref().is_none_or(|previous| previous.directory != now.directory);
        let opening = now.opened && previous.as_ref().is_none_or(|previous| !previous.opened);
        if !changed && !opening {
            return now;
        }

        if let Some(Some(earlier)) = looking.try_update_value(Option::take) {
            earlier.cancel();
        }
        // Unmarked until a new directory has been looked for. One looked for again keeps its mark
        // meanwhile, so the gear does not flicker as the drawer opens.
        if changed {
            control.auto_export_missing.set(false);
        }
        let Some(wanted) = now.directory.clone().filter(|directory| !directory.is_empty()) else {
            return now;
        };

        let task = spawn_local(async move {
            let present = blocking(move || Path::new(&wanted).is_dir()).await;
            control.auto_export_missing.try_set(!present);
        });
        looking.set_value(Some(task));

        now
    });
    on_cleanup_local(move || drop(checking));
}

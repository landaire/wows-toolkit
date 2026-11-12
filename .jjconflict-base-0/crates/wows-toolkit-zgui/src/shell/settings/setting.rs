//! The settings the drawer lists, and the sections it groups them in.

use rust_i18n::t;
use wows_toolkit_icons::Glyph;

use crate::shell::tabs::AppTab;

/// A group of settings, in the order the drawer lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Appearance,
    Game,
    GameDataCache,
    ReplayIndex,
    Replays,
    Collaboration,
    Twitch,
    ArmorViewer,
}

impl SettingsSection {
    pub const ALL: [Self; 9] = [
        Self::General,
        Self::Appearance,
        Self::Game,
        Self::GameDataCache,
        Self::ReplayIndex,
        Self::Replays,
        Self::Collaboration,
        Self::Twitch,
        Self::ArmorViewer,
    ];

    pub fn name(self) -> String {
        match self {
            Self::General => t!("ui.settings.panel.general"),
            Self::Appearance => t!("ui.settings.panel.appearance"),
            Self::Game => t!("ui.settings.panel.game"),
            Self::GameDataCache => t!("ui.settings.wows.cache.heading"),
            Self::ReplayIndex => t!("ui.settings.index.heading"),
            Self::Replays => t!("ui.settings.panel.replays"),
            Self::Collaboration => t!("ui.settings.panel.collaboration"),
            Self::Twitch => t!("ui.settings.panel.twitch"),
            Self::ArmorViewer => t!(AppTab::ArmorViewer.label_key()),
        }
        .into_owned()
    }

    pub const fn glyph(self) -> Glyph {
        match self {
            Self::General => Glyph::Settings,
            Self::Appearance => Glyph::Paintbrush,
            Self::Game => Glyph::FolderOpen,
            Self::GameDataCache => Glyph::Archive,
            Self::ReplayIndex => Glyph::Database,
            Self::Replays => Glyph::Table,
            Self::Collaboration => Glyph::Users,
            Self::Twitch => Glyph::Twitch,
            Self::ArmorViewer => Glyph::Shield,
        }
    }
}

/// One setting, as a row of the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Language,
    DataSharing,
    CodeIntegrity,
    Proxy,
    CheckForUpdates,
    EnableLogging,
    DataFolder,
    Theme,
    Zoom,
    WowsDirectory,
    AutoDump,
    CacheDirectory,
    CachedBuilds,
    ReplayIndex,
    CurrentReplayPath,
    ShowRawXp,
    ShowObservedDamage,
    ShowEntityId,
    ShowHeals,
    ReplayPreviews,
    AutoExport,
    DisplayName,
    SuppressIpWarning,
    DisableAutoOpen,
    TwitchGetToken,
    TwitchToken,
    TwitchChannel,
    ArmorPlateEdges,
    ArmorWaterline,
    ArmorOpaqueHull,
    /// Stands in for the three above while nothing has been saved.
    ArmorNoDefaults,
}

impl Setting {
    /// Every setting, in the order the drawer lists them.
    pub const ALL: [Self; 31] = [
        Self::Language,
        Self::DataSharing,
        Self::CodeIntegrity,
        Self::Proxy,
        Self::CheckForUpdates,
        Self::EnableLogging,
        Self::DataFolder,
        Self::Theme,
        Self::Zoom,
        Self::WowsDirectory,
        Self::AutoDump,
        Self::CacheDirectory,
        Self::CachedBuilds,
        Self::ReplayIndex,
        Self::CurrentReplayPath,
        Self::ShowRawXp,
        Self::ShowObservedDamage,
        Self::ShowEntityId,
        Self::ShowHeals,
        Self::ReplayPreviews,
        Self::AutoExport,
        Self::DisplayName,
        Self::SuppressIpWarning,
        Self::DisableAutoOpen,
        Self::TwitchGetToken,
        Self::TwitchToken,
        Self::TwitchChannel,
        Self::ArmorPlateEdges,
        Self::ArmorWaterline,
        Self::ArmorOpaqueHull,
        Self::ArmorNoDefaults,
    ];

    pub const fn section(self) -> SettingsSection {
        match self {
            Self::Language
            | Self::DataSharing
            | Self::CodeIntegrity
            | Self::Proxy
            | Self::CheckForUpdates
            | Self::EnableLogging
            | Self::DataFolder => SettingsSection::General,
            Self::Theme | Self::Zoom => SettingsSection::Appearance,
            Self::WowsDirectory => SettingsSection::Game,
            Self::AutoDump | Self::CacheDirectory | Self::CachedBuilds => SettingsSection::GameDataCache,
            Self::ReplayIndex => SettingsSection::ReplayIndex,
            Self::CurrentReplayPath
            | Self::ShowRawXp
            | Self::ShowObservedDamage
            | Self::ShowEntityId
            | Self::ShowHeals
            | Self::ReplayPreviews
            | Self::AutoExport => SettingsSection::Replays,
            Self::DisplayName | Self::SuppressIpWarning | Self::DisableAutoOpen => SettingsSection::Collaboration,
            Self::TwitchGetToken | Self::TwitchToken | Self::TwitchChannel => SettingsSection::Twitch,
            Self::ArmorPlateEdges | Self::ArmorWaterline | Self::ArmorOpaqueHull | Self::ArmorNoDefaults => {
                SettingsSection::ArmorViewer
            }
        }
    }
}

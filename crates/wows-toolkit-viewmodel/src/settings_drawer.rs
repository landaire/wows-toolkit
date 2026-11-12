//! The settings the desktop settings drawers list, the sections they group them in, and the
//! search over their names and descriptions.

use std::borrow::Cow;

use rust_i18n::t;
use wows_toolkit_config::ReplayExportFormat;

use crate::text_filter::FilterQuery;

/// The formats a battle can be auto-exported in, in the order the egui combo lists them.
pub const EXPORT_FORMATS: [ReplayExportFormat; 3] =
    [ReplayExportFormat::Json, ReplayExportFormat::Csv, ReplayExportFormat::Cbor];

/// A group of settings, in the order the drawer lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            // The Armor Viewer tab's own name.
            Self::ArmorViewer => t!("ui.tabs.armor_viewer"),
        }
        .into_owned()
    }
}

/// One setting, as a row of the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    ArmorOpaqueHull,
    /// Stands in for the two above while nothing has been saved.
    ArmorNoDefaults,
}

impl Setting {
    /// Every setting, in the order the drawer lists them.
    pub const ALL: [Self; 30] = [
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
            Self::ArmorPlateEdges | Self::ArmorOpaqueHull | Self::ArmorNoDefaults => SettingsSection::ArmorViewer,
        }
    }

    pub fn title(self) -> Cow<'static, str> {
        match self {
            Self::Language => t!("ui.settings.app.language"),
            Self::DataSharing => t!("ui.settings.app.data_sharing_mode"),
            Self::CodeIntegrity => t!("ui.settings.app.code_integrity"),
            Self::Proxy => t!("ui.settings.app.proxy_url"),
            Self::CheckForUpdates => t!("ui.settings.app.check_for_updates"),
            Self::EnableLogging => t!("ui.settings.app.enable_logging"),
            Self::DataFolder => t!("ui.settings.panel.data_folder"),
            Self::Theme => t!("ui.settings.app.theme"),
            Self::Zoom => t!("ui.settings.panel.zoom"),
            Self::WowsDirectory => t!("ui.settings.wows.directory_hint"),
            Self::AutoDump => t!("ui.settings.wows.cache.auto_dump"),
            Self::CacheDirectory => t!("ui.settings.panel.cache_directory"),
            Self::CachedBuilds => t!("ui.settings.panel.cached_builds"),
            Self::ReplayIndex => t!("ui.settings.index.build"),
            Self::CurrentReplayPath => t!("ui.settings.replay.current_path"),
            Self::ShowRawXp => t!("ui.settings.replay.show_raw_xp"),
            Self::ShowObservedDamage => t!("ui.settings.replay.show_observed_damage"),
            Self::ShowEntityId => t!("ui.settings.replay.show_entity_id"),
            Self::ShowHeals => t!("ui.settings.replay.show_heals"),
            Self::ReplayPreviews => t!("ui.settings.replay.enable_previews"),
            Self::AutoExport => t!("ui.settings.replay.auto_export_data"),
            Self::DisplayName => t!("ui.settings.session.display_name"),
            Self::SuppressIpWarning => t!("ui.settings.session.suppress_ip_warning"),
            Self::DisableAutoOpen => t!("ui.settings.session.disable_auto_open"),
            Self::TwitchGetToken => t!("ui.settings.twitch.get_token"),
            Self::TwitchToken => t!("ui.settings.panel.token"),
            Self::TwitchChannel => t!("ui.settings.twitch.monitored_channel"),
            Self::ArmorPlateEdges => t!("ui.armor.plate_edges"),
            Self::ArmorOpaqueHull => t!("ui.armor.opaque_hull"),
            Self::ArmorNoDefaults => t!("ui.settings.panel.armor_defaults"),
        }
    }

    pub fn description(self) -> Cow<'static, str> {
        match self {
            Self::Language => t!("ui.settings.panel.language_description"),
            Self::DataSharing => t!("ui.settings.panel.data_sharing_description"),
            Self::CodeIntegrity => t!("ui.settings.panel.code_integrity_description"),
            Self::Proxy => t!("ui.settings.app.proxy_url_hint"),
            Self::CheckForUpdates => t!("ui.settings.panel.check_for_updates_description"),
            Self::EnableLogging => t!("ui.settings.app.enable_logging_tooltip"),
            Self::DataFolder => t!("ui.settings.app.open_data_dir_tooltip"),
            Self::Theme => t!("ui.settings.panel.theme_description"),
            Self::Zoom => t!("ui.settings.panel.zoom_description"),
            Self::WowsDirectory => t!("ui.settings.wows.description"),
            Self::AutoDump => t!("ui.settings.wows.cache.description"),
            Self::CacheDirectory => t!("ui.settings.panel.cache_directory_description"),
            Self::CachedBuilds => t!("ui.settings.wows.cache.actions_tooltip"),
            Self::ReplayIndex => t!("ui.settings.index.description"),
            Self::CurrentReplayPath => t!("ui.settings.panel.current_path_description"),
            Self::ShowRawXp => t!("ui.settings.panel.show_raw_xp_description"),
            Self::ShowObservedDamage => t!("ui.settings.panel.show_observed_damage_description"),
            Self::ShowEntityId => t!("ui.settings.panel.show_entity_id_description"),
            Self::ShowHeals => t!("ui.settings.panel.show_heals_description"),
            Self::ReplayPreviews => t!("ui.settings.replay.enable_previews_tooltip"),
            Self::AutoExport => t!("ui.settings.panel.auto_export_description"),
            Self::DisplayName => t!("ui.settings.panel.display_name_description"),
            Self::SuppressIpWarning => t!("ui.settings.session.ip_warning_tooltip"),
            Self::DisableAutoOpen => t!("ui.settings.session.auto_open_tooltip"),
            Self::TwitchGetToken => t!("ui.settings.panel.get_token_description"),
            Self::TwitchToken => t!("ui.settings.twitch.paste_token_tooltip"),
            Self::TwitchChannel => t!("ui.settings.panel.monitored_channel_description"),
            Self::ArmorPlateEdges | Self::ArmorOpaqueHull => t!("ui.settings.panel.armor_saved_description"),
            Self::ArmorNoDefaults => t!("ui.settings.armor.no_defaults"),
        }
    }

    /// Whether the control sits under the text rather than beside it: the fields and button rows
    /// are wider than the space beside a title.
    pub const fn stacked(self) -> bool {
        matches!(
            self,
            Self::DataSharing
                | Self::CodeIntegrity
                | Self::Proxy
                | Self::Theme
                | Self::Zoom
                | Self::WowsDirectory
                | Self::CacheDirectory
                | Self::CachedBuilds
                | Self::ReplayIndex
                | Self::CurrentReplayPath
                | Self::AutoExport
                | Self::DisplayName
                | Self::TwitchToken
                | Self::TwitchChannel
        )
    }

    /// Whether the name or the description matches `query`. An inactive query matches everything.
    pub fn matches(self, query: &FilterQuery) -> bool {
        query.keeps(&self.title()) || query.keeps(&self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(typed: &str) -> Vec<Setting> {
        let query = FilterQuery::new(typed);
        Setting::ALL.into_iter().filter(|setting| setting.matches(&query)).collect()
    }

    #[test]
    fn an_empty_or_blank_query_keeps_every_setting() {
        assert_eq!(kept(""), Setting::ALL);
        assert_eq!(kept("   "), Setting::ALL);
        assert!(!FilterQuery::new("  ").is_active());
    }

    #[test]
    fn a_query_reads_descriptions_in_any_case() {
        assert_eq!(
            kept("results table"),
            [Setting::ShowRawXp, Setting::ShowObservedDamage, Setting::ShowEntityId, Setting::ShowHeals]
        );
        assert_eq!(kept("  RAW XP "), [Setting::ShowRawXp]);
        assert!(kept("zzz").is_empty());
    }

    #[test]
    fn every_setting_belongs_to_a_listed_section() {
        for setting in Setting::ALL {
            assert!(SettingsSection::ALL.contains(&setting.section()), "{setting:?}");
        }
    }
}

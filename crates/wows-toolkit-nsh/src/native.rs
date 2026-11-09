use nsh_core::prelude::*;
use rust_i18n::Backend as _;

/// One top-level application tab.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct AppTab {
    /// Stable tab identifier.
    pub id: String,
    /// Text shown in the tab strip.
    pub label: String,
    /// Short description shown in navigation search.
    pub description: String,
}

/// Localized text used by the application shell.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShellText {
    /// Label for the tactics board command.
    pub tactics_board: String,
    /// Label for the update command.
    pub check_updates: String,
    /// Label for the about command.
    pub about: String,
    /// Label for the quit command.
    pub quit: String,
    /// Label for opening a replay.
    pub open_replay: String,
    /// Message shown after an update check finds nothing.
    pub app_up_to_date: String,
}

/// State required before the first application frame.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StartupState {
    /// Window and application title.
    pub title: String,
    /// Application version.
    pub version: String,
    /// Selected locale code.
    pub locale: String,
    /// Localized shell text.
    pub text: ShellText,
    /// Top-level tabs in display order.
    pub tabs: Vec<AppTab>,
}

/// Native application services exposed to the TSX interface.
#[module]
pub struct Toolkit;

#[module]
impl Toolkit {
    /// Returns the state required to construct the application shell.
    pub fn startup(locale: Option<String>) -> StartupState {
        let locale = selected_locale(locale);
        let backend = wt_translations::TranslationsBackend::load();
        let text = |key: &str, fallback: &str| backend.translate(&locale, key).unwrap_or(fallback).to_owned();
        StartupState {
            title: "WoWs Toolkit".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            locale: locale.clone(),
            text: ShellText {
                tactics_board: text("ui.windows.tactics_board", "Tactics Board"),
                check_updates: text("ui.menu.check_updates", "Check for Updates"),
                about: text("ui.menu.about", "About"),
                quit: text("ui.menu.quit", "Quit"),
                open_replay: text("ui.replay.open_file", "Open Replay File..."),
                app_up_to_date: text("ui.messages.app_up_to_date", "Application up-to-date"),
            },
            tabs: vec![
                tab(
                    "replay-inspector",
                    &text("ui.tabs.replay_parser", "Replay Inspector"),
                    "Open and inspect a replay",
                ),
                tab("launcher", &text("ui.tabs.launcher", "Launcher"), "Manage and start game installations"),
                tab("statistics", &text("ui.tabs.stats", "Stats"), "Analyze replay statistics"),
                tab("player-tracker", &text("ui.tabs.player_tracker", "Player Tracker"), "Track player battle results"),
                tab("search", &text("ui.tabs.search", "Search"), "Search game data"),
                tab("armor-viewer", &text("ui.tabs.armor_viewer", "Armor Viewer"), "Inspect ship armor models"),
                tab("unpacker", &text("ui.tabs.unpacker", "Resource Unpacker"), "Browse and extract game files"),
                tab("settings", &text("ui.tabs.settings", "Settings"), "Configure WoWs Toolkit"),
            ],
        }
    }
}

fn selected_locale(requested: Option<String>) -> String {
    requested
        .filter(|locale| wt_translations::SUPPORTED_LANGUAGES.iter().any(|language| language.code == locale))
        .or_else(|| {
            sys_locale::get_locale()
                .and_then(|locale| wt_translations::system_locale_to_wows(&locale).map(str::to_owned))
        })
        .unwrap_or_else(|| "en".to_owned())
}

fn tab(id: &str, label: &str, description: &str) -> AppTab {
    AppTab { id: id.to_owned(), label: label.to_owned(), description: description.to_owned() }
}

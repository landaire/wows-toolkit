use nsh_core::prelude::*;
use wows_toolkit_viewmodel::settings::keys;

use crate::icons::Icon;

/// One fixed top-level application tab.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct AppTab {
    /// Stable tab identifier.
    pub id: String,
    /// Catalog key of the tab's label, read in the current language on each draw.
    pub label_key: String,
    /// The glyph before the label, as GPUI's `AppTab::glyph`.
    pub icon: Icon,
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
    /// Fixed top-level tabs in display order.
    pub tabs: Vec<AppTab>,
}

/// Native application services exposed to the TSX interface.
#[module]
pub struct Toolkit;

#[module]
impl Toolkit {
    /// Returns the state required to construct the application shell.
    pub fn startup(locale: Option<String>) -> StartupState {
        // An unreadable settings row falls back to the system's language, as
        // a first launch does.
        let persisted = locale.or_else(|| {
            wows_toolkit_config::load_startup_setting::<Option<String>>(keys::LOCALE).ok().flatten().flatten()
        });
        let locale = selected_locale(persisted);
        StartupState {
            title: "WoWs Toolkit".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            locale,
            tabs: vec![
                tab("replay-inspector", "ui.tabs.replay_parser", Icon::Search),
                tab("launcher", "ui.tabs.launcher", Icon::Rocket),
                tab("statistics", "ui.tabs.stats", Icon::ChartColumn),
                tab("player-tracker", "ui.tabs.player_tracker", Icon::VenetianMask),
                tab("search", "ui.tabs.search", Icon::Search),
                tab("armor-viewer", "ui.tabs.armor_viewer", Icon::Shield),
                tab("unpacker", "ui.tabs.unpacker", Icon::Archive),
                tab("settings", "ui.tabs.settings", Icon::Settings),
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

fn tab(id: &str, label_key: &str, icon: Icon) -> AppTab {
    AppTab { id: id.to_owned(), label_key: label_key.to_owned(), icon }
}

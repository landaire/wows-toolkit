//! The tabs of the strip, and what a tab puts in the shell around it.

use std::path::Path;
use std::path::PathBuf;

use landaire_ui::PanelSide;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;

/// One of the tabs every window has, in the order the strip shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppTab {
    ReplayInspector,
    Launcher,
    Stats,
    PlayerTracker,
    Search,
    ArmorViewer,
    Unpacker,
}

impl AppTab {
    /// Left to right. Settings are not a tab but the drawer the gear at the end of the strip opens.
    pub const ALL: [Self; 7] = [
        Self::ReplayInspector,
        Self::Launcher,
        Self::Stats,
        Self::PlayerTracker,
        Self::Search,
        Self::ArmorViewer,
        Self::Unpacker,
    ];

    pub const fn label_key(self) -> &'static str {
        match self {
            Self::ReplayInspector => "ui.tabs.replay_parser",
            Self::Stats => "ui.tabs.stats",
            Self::PlayerTracker => "ui.tabs.player_tracker",
            Self::Search => "ui.tabs.search",
            Self::ArmorViewer => "ui.tabs.armor_viewer",
            Self::Unpacker => "ui.tabs.unpacker",
            Self::Launcher => "ui.tabs.launcher",
        }
    }

    /// The tab's name in the reader's language.
    pub fn label(self) -> String {
        t!(self.label_key()).into_owned()
    }

    pub const fn glyph(self) -> Glyph {
        match self {
            Self::ReplayInspector => Glyph::ClipboardList,
            Self::Stats => Glyph::ChartColumn,
            Self::PlayerTracker => Glyph::VenetianMask,
            Self::Search => Glyph::Search,
            Self::ArmorViewer => Glyph::Shield,
            Self::Unpacker => Glyph::Archive,
            Self::Launcher => Glyph::Rocket,
        }
    }

    /// What the strip knows the tab by, and what a test finds it by.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReplayInspector => "replay-inspector",
            Self::Launcher => "launcher",
            Self::Stats => "stats",
            Self::PlayerTracker => "player-tracker",
            Self::Search => "search",
            Self::ArmorViewer => "armor-viewer",
            Self::Unpacker => "unpacker",
        }
    }
}

/// Names one directory stats tab for as long as it is open. Not saved: the tabs last one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirectoryStatsId(pub u64);

/// A stats tab over every replay in one directory, shown beside the session's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryStatsTab {
    pub id: DirectoryStatsId,
    /// Canonical, so two spellings of one directory share a tab.
    pub root: PathBuf,
}

impl DirectoryStatsTab {
    pub fn label(&self) -> String {
        t!("ui.tabs.directory_stats", dir = wows_toolkit_replay_data::paths::shortened_path(&self.root)).into_owned()
    }
}

/// Any tab of the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TabKey {
    App(AppTab),
    DirectoryStats(DirectoryStatsId),
}

impl TabKey {
    /// The strip's value for the tab.
    pub fn value(self) -> String {
        match self {
            Self::App(tab) => tab.name().to_owned(),
            Self::DirectoryStats(id) => format!("directory-stats-{}", id.0),
        }
    }

    /// The tab the strip's `value` names, if it names one.
    pub fn parse(value: &str) -> Option<Self> {
        if let Some(tab) = AppTab::ALL.into_iter().find(|tab| tab.name() == value) {
            return Some(Self::App(tab));
        }

        let id = value.strip_prefix("directory-stats-")?.parse().ok()?;
        Some(Self::DirectoryStats(DirectoryStatsId(id)))
    }
}

/// Canonical, so two spellings of one directory share a tab. A path that will not resolve is
/// kept as given: the read reports it empty.
pub fn canonical_root(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// The icon a toggle for a panel on `side` shows.
pub const fn panel_glyph(side: PanelSide) -> Glyph {
    match side {
        PanelSide::Left => Glyph::PanelLeft,
        PanelSide::Right => Glyph::PanelRight,
        PanelSide::Bottom => Glyph::PanelBottom,
    }
}

/// The side's name, for a panel that has no better one, such as a dock's.
pub fn panel_side_name(side: PanelSide) -> String {
    match side {
        PanelSide::Left => t!("ui.app.panel_left"),
        PanelSide::Right => t!("ui.app.panel_right"),
        PanelSide::Bottom => t!("ui.app.panel_bottom"),
    }
    .into_owned()
}

/// One side panel a tab can show or put away from the status bar.
#[derive(Clone, Debug)]
pub struct PanelToggle {
    /// What a test finds the toggle by.
    pub test_id: &'static str,
    /// What the panel is called, already translated, for the toggle's tooltip.
    pub name: String,
    pub side: PanelSide,
    /// Whether the panel is showing. The toggle writes it, and the tab's panel follows it.
    pub open: RwSignal<bool, LocalStorage>,
}

/// A modifier key that switches a view to an alternate reading while held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierKey {
    /// Ctrl, or Cmd on macOS.
    Primary,
    Shift,
    /// Alt, or Option on macOS.
    Alt,
}

impl ModifierKey {
    /// The key as the platform's keyboard labels it.
    pub const fn label(self) -> &'static str {
        let mac = cfg!(target_os = "macos");
        match self {
            Self::Primary if mac => "Cmd",
            Self::Primary => "Ctrl",
            Self::Shift => "Shift",
            Self::Alt if mac => "Option",
            Self::Alt => "Alt",
        }
    }

    pub const fn is_down(self, modifiers: zgui::vocab::Modifiers) -> bool {
        match self {
            Self::Primary => modifiers.primary(),
            Self::Shift => modifiers.shift(),
            Self::Alt => modifiers.alt(),
        }
    }
}

/// What a view shows while `key` is held, named for the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeHint {
    pub key: ModifierKey,
    /// Translation key of the mode's name.
    pub name_key: &'static str,
}

/// What one tab puts in the shell around it: its panel toggles and its held-key mode in the
/// status bar.
///
/// The shell makes one per tab and provides it to the tab's content, which finds it with
/// [`TabChrome::current`]. The status bar shows the selected tab's.
#[derive(Clone, Copy)]
pub struct TabChrome {
    toggles: RwSignal<Vec<PanelToggle>, LocalStorage>,
    mode: RwSignal<Option<ModeHint>, LocalStorage>,
}

impl TabChrome {
    pub fn new() -> Self {
        Self { toggles: RwSignal::new_local(Vec::new()), mode: RwSignal::new_local(None) }
    }

    /// Publishes this chrome to the tab content built below the calling scope.
    pub fn provide(self) {
        provide_local_context(self);
    }

    /// The chrome of the tab the caller is in. Called from a component's body; `None` outside a
    /// tab, such as in a secondary window.
    pub fn current() -> Option<Self> {
        use_local_context::<Self>()
    }

    /// Replaces the panel toggles the status bar shows for this tab.
    pub fn set_toggles(self, toggles: Vec<PanelToggle>) {
        self.toggles.set(toggles);
    }

    /// Says which mode the tab is in, or that it is in none. The status bar names it while its key
    /// is held.
    pub fn set_mode(self, mode: Option<ModeHint>) {
        self.mode.set(mode);
    }

    pub fn toggles(self) -> Vec<PanelToggle> {
        self.toggles.get()
    }

    pub fn mode(self) -> Option<ModeHint> {
        self.mode.get()
    }
}

impl Default for TabChrome {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_value_names_its_tab() {
        for tab in AppTab::ALL {
            assert_eq!(TabKey::parse(&TabKey::App(tab).value()), Some(TabKey::App(tab)));
        }

        let directory = TabKey::DirectoryStats(DirectoryStatsId(7));
        assert_eq!(TabKey::parse(&directory.value()), Some(directory));
        assert_eq!(TabKey::parse("settings"), None);
    }
}

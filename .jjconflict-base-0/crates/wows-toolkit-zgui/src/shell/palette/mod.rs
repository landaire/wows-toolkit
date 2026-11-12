//! The command palette: everything the window can do, by name.
//!
//! The root list is made of offers. The shell offers what it can do itself as it is built, and a
//! tab offers what it can do with [`PaletteControl::offer`] and [`PaletteControl::offer_mode`] as
//! it is built. An offer lasts as long as the scope that made it, and the root list shows the
//! offers in the order of [`RootCommand`] and [`PaletteMode`], whoever made them.
//!
//! A command either runs in place or asks a tab to do something: [`PaletteRun::Request`] shows
//! the tab, which takes the request with [`PaletteControl::take_requests`]. A tab that is not
//! built yet is built by being shown, so a request reaches it either way.
//!
//! A mode is a second list the root entry leads into, filled from its [`ModeSource`] as the
//! reader types. Escape, or Backspace in an empty field, returns to the root list.

mod builtin;
pub mod modes;
#[cfg(test)]
mod tests;
mod view;

use std::path::PathBuf;
use std::rc::Rc;

use futures::future::LocalBoxFuture;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_services::dialog::Answer;
use wows_toolkit_viewmodel::palette::PaletteMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;

use crate::shell::Shell;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;
use crate::text_marks::Marks;
use crate::text_marks::marks;

pub use crate::shell::palette::builtin::open_replay_file;
pub use crate::shell::palette::view::CommandPaletteProps;

/// One command of the root list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RootCommand {
    /// Shows a tab. The Search tab is [`RootCommand::AdvancedSearch`] instead.
    GoTo(AppTab),
    OpenSettings,
    AdvancedSearch,
    GamesIDiedIn,
    GamesIWon,
    OpenReplayFile,
    /// Reads every replay that is not indexed yet.
    IndexAllReplays,
    /// Reads every listed replay again and rewrites its index entry, once confirmed.
    ReindexAll,
    /// Reads every replay again and rewrites what the index holds, once confirmed.
    RefreshPersistedData,
    /// Takes a result mapping the reader points at as the loaded build's.
    ImportConstants,
    /// Contributes every listed battle not sent yet, as the data-sharing setting asks.
    SendAllReplays,
    /// The same, sending again what the ledger says was sent.
    SendAllReplaysAgain,
    CopyLatestLog,
    CopyBugReportInfo,
    SetTheme(PaletteTheme),
}

impl RootCommand {
    /// What the entry says, in the reader's language.
    pub fn label(self) -> String {
        let key = match self {
            Self::GoTo(tab) => return t!("ui.palette.go_to", tab = tab.label()).into_owned(),
            Self::SetTheme(theme) => {
                return t!("ui.palette.set_theme", theme = t!(theme.label_key())).into_owned();
            }
            Self::OpenSettings => "ui.settings.panel.open",
            Self::AdvancedSearch => "ui.palette.advanced_search",
            Self::GamesIDiedIn => "ui.palette.games_i_died_in",
            Self::GamesIWon => "ui.palette.games_i_won",
            Self::OpenReplayFile => "ui.replay.open_manually",
            Self::IndexAllReplays => "ui.settings.replay.index_all_replays",
            Self::ReindexAll => "ui.reindex.palette_all",
            Self::RefreshPersistedData => "ui.replay.refresh_persisted_data",
            Self::ImportConstants => "ui.replay.import_constants",
            Self::SendAllReplays => "ui.palette.send_all_replays",
            Self::SendAllReplaysAgain => "ui.palette.send_all_replays_again",
            Self::CopyLatestLog => "ui.replay.copy_latest_log",
            Self::CopyBugReportInfo => "ui.replay.copy_bug_report_info",
        };

        t!(key).into_owned()
    }
}

/// The themes the palette offers by name, as the egui palette does. The others are chosen in the
/// settings drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteTheme {
    System,
    Dark,
    Light,
}

impl PaletteTheme {
    pub const ALL: [Self; 3] = [Self::System, Self::Dark, Self::Light];

    pub const fn choice(self) -> ThemeChoice {
        match self {
            Self::System => ThemeChoice::System,
            Self::Dark => ThemeChoice::Dark,
            Self::Light => ThemeChoice::Light,
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::System => "ui.settings.app.theme_system",
            Self::Dark => "ui.settings.app.theme_dark",
            Self::Light => "ui.settings.app.theme_light",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// One entry of the root list: a command, or the way into a mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RootItem {
    Command(RootCommand),
    Mode(PaletteMode),
}

impl RootItem {
    /// Where the entry is listed, as the egui palette lists it: tabs first because going somewhere
    /// is the common reason to open it, then the searches, then the settings-like actions.
    fn position(self) -> u16 {
        match self {
            Self::Command(RootCommand::GoTo(tab)) => {
                AppTab::ALL.iter().position(|listed| *listed == tab).map_or(0, |at| at as u16)
            }
            Self::Command(RootCommand::OpenSettings) => 10,
            Self::Command(RootCommand::AdvancedSearch) => 11,
            Self::Mode(PaletteMode::Players) => 20,
            Self::Mode(PaletteMode::MyShips) => 21,
            Self::Mode(PaletteMode::ArmorShips) => 22,
            Self::Command(RootCommand::GamesIDiedIn) => 30,
            Self::Command(RootCommand::GamesIWon) => 31,
            Self::Command(RootCommand::OpenReplayFile) => 40,
            Self::Mode(PaletteMode::ReplayFolders) => 41,
            Self::Command(RootCommand::IndexAllReplays) => 42,
            Self::Mode(PaletteMode::ReindexVersions) => 43,
            Self::Command(RootCommand::ReindexAll) => 44,
            Self::Command(RootCommand::RefreshPersistedData) => 45,
            Self::Command(RootCommand::ImportConstants) => 46,
            Self::Command(RootCommand::SendAllReplays) => 47,
            Self::Command(RootCommand::SendAllReplaysAgain) => 48,
            Self::Command(RootCommand::CopyLatestLog) => 49,
            Self::Command(RootCommand::CopyBugReportInfo) => 50,
            Self::Command(RootCommand::SetTheme(PaletteTheme::System)) => 60,
            Self::Command(RootCommand::SetTheme(PaletteTheme::Dark)) => 61,
            Self::Command(RootCommand::SetTheme(PaletteTheme::Light)) => 62,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Command(command) => command.label(),
            Self::Mode(mode) => t!(mode.label_key()).into_owned(),
        }
    }

    /// What the entry is told apart by on the list and in a test.
    fn value(self) -> String {
        match self {
            Self::Command(RootCommand::GoTo(tab)) => format!("go-to-{}", tab.name()),
            Self::Command(RootCommand::SetTheme(theme)) => format!("set-theme-{}", theme.name()),
            Self::Command(command) => command_name(command).to_owned(),
            Self::Mode(mode) => format!("mode-{}", mode_name(mode)),
        }
    }
}

/// What a test finds a mode's list by.
pub fn mode_name(mode: PaletteMode) -> &'static str {
    match mode {
        PaletteMode::Players => "players",
        PaletteMode::MyShips => "my-ships",
        PaletteMode::ArmorShips => "armor-ships",
        PaletteMode::ReindexVersions => "reindex-versions",
        PaletteMode::ReplayFolders => "replay-folders",
    }
}

fn command_name(command: RootCommand) -> &'static str {
    match command {
        RootCommand::GoTo(_) => "go-to",
        RootCommand::SetTheme(_) => "set-theme",
        RootCommand::OpenSettings => "open-settings",
        RootCommand::AdvancedSearch => "advanced-search",
        RootCommand::GamesIDiedIn => "games-i-died-in",
        RootCommand::GamesIWon => "games-i-won",
        RootCommand::OpenReplayFile => "open-replay-file",
        RootCommand::IndexAllReplays => "index-all-replays",
        RootCommand::ReindexAll => "reindex-all",
        RootCommand::RefreshPersistedData => "refresh-persisted-data",
        RootCommand::ImportConstants => "import-constants",
        RootCommand::SendAllReplays => "send-all-replays",
        RootCommand::SendAllReplaysAgain => "send-all-replays-again",
        RootCommand::CopyLatestLog => "copy-latest-log",
        RootCommand::CopyBugReportInfo => "copy-bug-report-info",
    }
}

/// The parameter a ship is named by in the game data, such as `PASB018`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShipParamIndex(pub String);

/// What a command asks a tab to do. The tab takes it with [`PaletteControl::take_requests`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabRequest {
    /// Puts the query in the Search tab's bar and runs it.
    Search(String),
    /// Opens one ship in the Armor Viewer.
    ViewArmor { param_index: ShipParamIndex, display_name: String },
}

impl TabRequest {
    /// The tab the request is for.
    pub fn tab(&self) -> AppTab {
        match self {
            Self::Search(_) => AppTab::Search,
            Self::ViewArmor { .. } => AppTab::ArmorViewer,
        }
    }
}

/// What choosing an entry does, once the palette has closed.
#[derive(Clone)]
pub enum PaletteRun {
    Action(Rc<dyn Fn()>),
    /// Shows the request's tab, which takes it.
    Request(TabRequest),
}

impl PaletteRun {
    pub fn action(run: impl Fn() + 'static) -> Self {
        Self::Action(Rc::new(run))
    }
}

/// One row of a mode's list.
#[derive(Clone)]
pub struct ModeRow {
    pub label: String,
    /// Where the query matched `label`.
    pub marks: Marks,
    /// Drawn dimmed after the label, such as a directory's whole path.
    pub detail: Option<String>,
    /// Where the query matched `detail`.
    pub detail_marks: Marks,
    pub glyph: Option<Glyph>,
    /// Drawn dimmed and marked, for a recent directory that is gone.
    pub missing: bool,
    pub run: PaletteRun,
}

impl ModeRow {
    pub fn new(label: String, run: PaletteRun) -> Self {
        Self {
            label,
            marks: Marks::default(),
            detail: None,
            detail_marks: Marks::default(),
            glyph: None,
            missing: false,
            run,
        }
    }
}

/// What a mode lists for one query.
#[derive(Clone)]
pub enum ModeRows {
    /// Waiting on something to load; says what.
    Loading(String),
    Ready(Vec<ModeRow>),
    /// Nothing can be listed; says why.
    Unavailable(String),
}

/// A future resolving to a mode's rows.
pub type ModeQuery = LocalBoxFuture<'static, ModeRows>;

/// Where a mode's rows come from.
#[derive(Clone)]
pub enum ModeSource {
    /// Filtered in memory as the reader types. Called in a reactive scope, so the list follows
    /// any signal it reads.
    Local(Rc<dyn Fn(&str) -> ModeRows>),
    /// Read off the UI thread once the reader pauses typing.
    Query(Rc<dyn Fn(String) -> ModeQuery>),
}

/// A mode as offered to the palette.
#[derive(Clone)]
pub struct ModeSpec {
    pub source: ModeSource,
    /// Run as the mode is entered, as for starting a load its rows wait on.
    pub on_enter: Option<Rc<dyn Fn()>>,
}

/// Which list the palette shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Root,
    Mode(PaletteMode),
}

/// Names one opening of a list, so a list opened again starts afresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OpeningId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Opening {
    layer: Layer,
    id: OpeningId,
    /// The offer a mode's list was opened from.
    offer: Option<OfferId>,
}

/// Names one offer, so the scope that made it takes back that one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OfferId(u64);

#[derive(Clone)]
struct Offered<K, V> {
    id: OfferId,
    key: K,
    value: V,
}

/// One root entry as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RootRow {
    item: RootItem,
    label: String,
    marks: Marks,
}

/// Whether the palette is open and on which list, and what it offers.
#[derive(Clone, Copy)]
pub struct PaletteControl {
    /// `None` while closed.
    opening: RwSignal<Option<Opening>, LocalStorage>,
    commands: RwSignal<Vec<Offered<RootCommand, PaletteRun>>, LocalStorage>,
    modes: RwSignal<Vec<Offered<PaletteMode, ModeSpec>>, LocalStorage>,
    requests: RwSignal<Vec<TabRequest>, LocalStorage>,
    next: StoredValue<u64>,
}

impl PaletteControl {
    pub fn new() -> Self {
        Self {
            opening: RwSignal::new_local(None),
            commands: RwSignal::new_local(Vec::new()),
            modes: RwSignal::new_local(Vec::new()),
            requests: RwSignal::new_local(Vec::new()),
            next: StoredValue::new(0),
        }
    }

    fn next(self) -> u64 {
        let next = self.next.get_value();
        self.next.update_value(|counter| *counter += 1);

        next
    }

    pub fn is_open(self) -> bool {
        self.opening.with(Option::is_some)
    }

    /// The list on screen, `None` while closed.
    pub fn layer(self) -> Option<Layer> {
        self.opening.get().map(|opening| opening.layer)
    }

    fn layer_untracked(self) -> Option<Layer> {
        self.opening.get_untracked().map(|opening| opening.layer)
    }

    /// Opens the palette on its root list.
    pub fn open_root(self) {
        self.show(Layer::Root, None);
    }

    /// Opens the palette on `mode`'s list, or on the root list while nothing offers the mode.
    pub fn open_mode(self, mode: PaletteMode) {
        let latest = self.modes.with_untracked(|modes| {
            modes
                .iter()
                .rev()
                .find(|offered| offered.key == mode)
                .map(|offered| (offered.id, offered.value.on_enter.clone()))
        });
        let Some((offer, on_enter)) = latest else {
            self.open_root();
            return;
        };

        self.show(Layer::Mode(mode), Some(offer));
        if let Some(on_enter) = on_enter {
            on_enter();
        }
    }

    fn show(self, layer: Layer, offer: Option<OfferId>) {
        let id = OpeningId(self.next());
        self.opening.set(Some(Opening { layer, id, offer }));
    }

    pub fn close(self) {
        if self.opening.with_untracked(Option::is_some) {
            self.opening.set(None);
        }
    }

    /// Lists `command` on the root list until the calling scope is cleaned up. A later offer of
    /// the same command is the one run while both stand.
    pub fn offer(self, command: RootCommand, run: PaletteRun) {
        let id = OfferId(self.next());
        self.commands.update(|commands| commands.push(Offered { id, key: command, value: run }));
        on_cleanup_local(move || {
            self.commands.try_update(|commands| commands.retain(|offered| offered.id != id));
        });
    }

    /// Lists `mode` on the root list until the calling scope is cleaned up. A later offer of the
    /// same mode is the one entered while both stand. A mode open on the offer taken back opens
    /// again from the latest offer left, or the root list when none is.
    pub fn offer_mode(self, mode: PaletteMode, spec: ModeSpec) {
        let id = OfferId(self.next());
        self.modes.update(|modes| modes.push(Offered { id, key: mode, value: spec }));
        on_cleanup_local(move || {
            self.modes.try_update(|modes| modes.retain(|offered| offered.id != id));

            let open =
                self.opening.try_with_untracked(|opening| opening.is_some_and(|opening| opening.offer == Some(id)));
            if open == Some(true) {
                self.open_mode(mode);
            }
        });
    }

    /// Takes every request for `tab`, oldest first, subscribing to the next one.
    pub fn take_requests(self, tab: AppTab) -> Vec<TabRequest> {
        if !self.requests.with(|requests| requests.iter().any(|request| request.tab() == tab)) {
            return Vec::new();
        }

        let mut taken = Vec::new();
        self.requests.update(|requests| {
            let (theirs, others) = std::mem::take(requests).into_iter().partition(|request| request.tab() == tab);
            taken = theirs;
            *requests = others;
        });

        taken
    }

    fn offered_spec(self, offer: OfferId) -> Option<ModeSpec> {
        self.modes.with_untracked(|modes| {
            modes.iter().find(|offered| offered.id == offer).map(|offered| offered.value.clone())
        })
    }

    fn command_run(self, command: RootCommand) -> Option<PaletteRun> {
        self.commands.with_untracked(|commands| {
            commands.iter().rev().find(|offered| offered.key == command).map(|offered| offered.value.clone())
        })
    }

    /// The root list's entries whose label matches `query`: every entry in list order for an
    /// inactive query, else the matches best first, each marked where it matched.
    fn root_rows(self, query: &str) -> Vec<RootRow> {
        let mut items: Vec<RootItem> =
            self.commands.with(|commands| commands.iter().map(|offered| RootItem::Command(offered.key)).collect());
        self.modes.with(|modes| items.extend(modes.iter().map(|offered| RootItem::Mode(offered.key))));
        items.sort_by_key(|item| item.position());
        items.dedup();

        let query = FilterQuery::new(query);
        let keyed = items.into_iter().map(|item| {
            let label = item.label();
            (FilterKey::new(&label), RootRow { item, label, marks: Marks::default() })
        });
        query
            .rank(keyed, |(key, _)| key)
            .into_iter()
            .map(|(_, row)| RootRow { marks: marks(&query, &row.label), ..row })
            .collect()
    }

    /// Does what the root entry `item` says.
    fn choose_root(self, shell: Shell, item: RootItem) {
        match item {
            RootItem::Mode(mode) => self.open_mode(mode),
            RootItem::Command(command) => {
                let Some(run) = self.command_run(command) else { return };
                self.run(shell, run);
            }
        }
    }

    /// Closes the palette and does what `run` says.
    fn run(self, shell: Shell, run: PaletteRun) {
        self.close();
        match run {
            PaletteRun::Action(action) => {
                // Every command but opening the settings acts on the page, which the drawer
                // would cover, and that one opens the drawer itself.
                shell.settings_drawer.close();
                action();
            }
            PaletteRun::Request(request) => self.send(shell, request),
        }
    }

    /// Shows the request's tab, which takes it with [`Self::take_requests`].
    pub fn send(self, shell: Shell, request: TabRequest) {
        let tab = request.tab();
        self.requests.update(|requests| requests.push(request));
        shell.select(TabKey::App(tab));
    }
}

impl Default for PaletteControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Awaits a file dialog's answer. A dialog that could not be opened is logged and reported, and
/// answers `None` like a dismissed one.
pub(crate) async fn chosen(shell: Shell, asked: impl Future<Output = Answer>) -> Option<PathBuf> {
    // A closure rather than a match: off Linux the error type is empty, and an arm for it would be
    // an unreachable pattern.
    asked.await.unwrap_or_else(|err| {
        tracing::error!("file dialog could not be opened: {err}");
        shell.toasts().failed(t!("ui.dialogs.file_dialog_failed", reason = err).into_owned());
        None
    })
}

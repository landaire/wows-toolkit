//! `ReplayPanel`: one dock tab per open replay. Starts `load::spawn_parse` in
//! the background on construction and, once it completes, either shows the
//! per-replay outcome row plus the real `PlayerTable` (Milestones 1-3) or an
//! error message. Mirrors the egui app's `ReplayTab`/`ReplayTabViewer`
//! (`ui/replay_parser/mod.rs` ~120/4749): the tab title is "{ship} - {map}"
//! once loaded, `t!("ui.replay.loading")`'s "Loading..." until then. The
//! outcome row mirrors `build_replay_view`'s Row 1: the Win/Loss/Draw label
//! with the same trophy/sad-face/notches glyph, drawn from the same Phosphor
//! font the egui app uses (see `crate::icons`).
//!
//! Beside it sits the single-battle PR badge, showing what
//! `populate_personal_ratings` scored this replay's own row, in that band's
//! chip colors (`build_replay_view`'s `pr_chip`). It deviates from that
//! version in one respect: egui rates a replay whose results are missing as
//! though it dealt zero damage (`to_battle_stats`'s `unwrap_or_default`),
//! which reads as a real "Bad" rating for a battle whose damage is simply
//! unknown. Unknown damage is left unrated here, so no badge appears.
//!
//! The Export menu writes the same document the egui app does
//! (`wows_toolkit_viewmodel::replay_export`), in the same three formats.
//!
//! **Chat.** The egui app's chat button (`ui/replay_parser/mod.rs` ~3000-3016)
//! toggles a standalone window; this port toggles an inline side panel next
//! to the table instead (the brief's stated v1 tradeoff), built from
//! `chat.rs`'s `ChatPanel`. The button itself mirrors the egui original:
//! `.selected(show_chat)` while open, disabled with a "no chat" note when the
//! replay's chat log is empty. The note is a hardcoded string literal matching
//! the egui original's `ui.replay.no_chat` translation value verbatim; this
//! crate has no `t!()`/i18n lookup wired for it.
//!
//! **Debug mode.** `debug` (seeded from `ReplayInspectorView`'s session flag,
//! itself seeded from `AppPreferences.debug_mode` in the shared config DB)
//! lifts NDA hiding in `table` and reveals two buttons mirroring the egui
//! app's debug menu (`mod.rs` ~3018-3060): "Raw Metadata" (the replay
//! header/metadata JSON) and "Raw Results" (the battle-results JSON,
//! disabled when the replay carries none). Both share the chat button's side
//! panel slot (`SidePanel`) rather than opening a standalone window, the same
//! v1 tradeoff as chat. The Actions column's per-row "View Raw Player
//! Metadata" item (`table.rs::build_actions_menu`) shares the same slot:
//! `table` emits `PlayerTableEvent::ViewRawJson` on click, `on_table_event`
//! (subscribed in `apply_result`) builds a `RawJsonPanel` for that row's JSON
//! and opens it as `SidePanel::RawPlayerMetadata`.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use wows_toolkit_config::ReplaySettings;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;
use wowsunpack::vfs::VfsPath;

use crate::icons;

use super::chat::ChatPanel;
use super::columns::BattleOutcome;
use super::columns::ColorRole;
use super::columns::ReplayColumn;
use super::debug_view::RawJsonPanel;
use super::load::GameDataCache;
use super::load::ParsedReplay;
use super::load::ReplayLoadError;
use super::load::spawn_parse;
use super::model::MatchContext;
use super::model::ReplayReportModel;
use super::model::separate_number;
use super::table::PlayerTable;
use super::table::PlayerTableEvent;
use super::table::resolve_color;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_toolkit_config::index::query;
use wows_toolkit_viewmodel::replay_export::FlattenedVehicle;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wows_toolkit_viewmodel::twitch;

const LOADING_TITLE: &str = "Loading...";
const FAILED_TITLE: &str = "Failed to load replay";
const SIDE_PANEL_WIDTH: Pixels = px(360.);
const EXPORT_MENU_WIDTH: Pixels = px(220.);

/// Which entity (if any) occupies the panel's single side-panel slot. Chat
/// and the debug-mode raw viewers share the slot rather than each having
/// their own, since only one is useful to look at at a time and the egui app
/// itself only ever shows one debug viewer window at once.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SidePanel {
    None,
    Chat,
    RawMetadata,
    RawResults,
    /// The same results with their positional arrays resolved to named
    /// fields, which is what the egui debug menu's "Battle Results: Mapped
    /// JSON" opens.
    MappedResults,
    /// The Actions menu's "View Raw Player Metadata" item (`table.rs`'s
    /// `PlayerTableEvent::ViewRawJson`); backed by
    /// `LoadedReplay::raw_player_metadata_panel`, rebuilt with the clicked
    /// row's JSON each time the event fires (see `on_table_event`).
    RawPlayerMetadata,
}

/// The payloads the debug-mode viewers show, bundled so the loaded-state
/// builder takes one argument for them rather than three.
struct DebugPayloads {
    raw_metadata_json: String,
    /// `None` when the replay carries no battle-results packet (the player
    /// left before the server sent one).
    raw_results_json: Option<String>,
    /// `None` for the same reason, and also when the resolution produced
    /// nothing readable.
    mapped_results_json: Option<String>,
}

/// A loaded replay's tab title and outcome, plus the real player table, the
/// chat panel (`None` for a chat-less replay so its toggle button can be
/// disabled rather than opening onto an empty panel, matching the egui app),
/// and the debug-mode raw-JSON viewers (`raw_results_panel` is `None` when
/// the replay carries no battle-results packet, see
/// `load::ParsedReplay::raw_results_json`; `raw_player_metadata_panel` is
/// `None` until the Actions menu's "View Raw Player Metadata" item is
/// clicked at least once, see `on_table_event`).
struct LoadedReplay {
    title: SharedString,
    battle_result: Option<BattleResult>,
    table: Entity<PlayerTable>,
    chat_panel: Option<Entity<ChatPanel>>,
    raw_metadata_panel: Entity<RawJsonPanel>,
    raw_results_panel: Option<Entity<RawJsonPanel>>,
    mapped_results_panel: Option<Entity<RawJsonPanel>>,
    raw_player_metadata_panel: Option<Entity<RawJsonPanel>>,
}

enum LoadState {
    Loading,
    // Boxed because a loaded replay carries the whole presentation model and
    // every side panel, which a loading or failed one does not.
    Loaded(Box<LoadedReplay>),
    Failed(ReplayLoadError),
}

pub struct ReplayPanel {
    focus_handle: FocusHandle,
    state: LoadState,
    /// Which entity occupies the side-panel slot; mirrors the egui app's
    /// `show_game_chat` temp-data flag for `SidePanel::Chat`, plus this
    /// port's own state for the two debug viewers egui opens as standalone
    /// windows instead (see the module doc).
    side_panel: SidePanel,
    /// Debug mode lifts NDA hiding (threaded into `table`'s `debug` flag) and
    /// reveals the raw-metadata/raw-results viewer buttons. Seeded from
    /// `ReplayInspectorView`'s session debug flag at construction, kept live
    /// afterward by `set_debug` (the RI's runtime toggle; see `view.rs`).
    debug: bool,
    /// The visible-column set to apply once this replay finishes loading
    /// (`apply_result` overwrites the model's default `ReplayColumn::ALL` with
    /// this), and to forward straight to `table` when already loaded. Seeded
    /// from `columns::default_columns(&replay_settings)` at construction, kept
    /// live afterward by `set_columns` (the header toolbar's column-filter
    /// checkboxes; see `view.rs`).
    columns: Vec<ReplayColumn>,
    /// The expected-values table the Personal Rating column and the outcome
    /// row's PR badge are computed against. Handed to `spawn_parse` so a
    /// replay opened after the table loaded is rated as it parses, and kept
    /// here so `set_personal_rating` can fill in a replay that was already
    /// open when the table arrived.
    personal_rating: Option<Arc<PersonalRatingData>>,
    /// The match the Export menu writes, in its debug form. `None` until the
    /// parse finishes, which is what keeps the menu disabled until then.
    export: Option<ExportedMatch>,
    /// Where a finished parse writes itself without being asked, if anywhere.
    auto_export: AutoExport,
    /// The replay this tab is reading, kept because an auto-export is named
    /// after it.
    path: PathBuf,
    /// What the last export did, shown beside the menu.
    export_status: Option<String>,
    _parse_task: Task<()>,
    /// Subscription to `table`'s `PlayerTableEvent`s, live once the replay
    /// finishes loading (`apply_result` creates both `table` and this
    /// together). `None` while still loading, since there is no `table` yet
    /// to subscribe to.
    _table_subscription: Option<Subscription>,
}

impl ReplayPanel {
    pub fn new(setup: PanelSetup, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let PanelSetup { path, game_data, debug, columns, personal_rating, auto_export } = setup;
        let focus_handle = cx.focus_handle();
        let parse_task = spawn_parse(path.clone(), game_data, personal_rating.clone(), cx);
        let parse_task = cx.spawn_in(window, async move |this, cx| {
            let result = parse_task.await;
            let _ = this.update_in(cx, |this, window, cx| this.apply_result(result, window, cx));
        });

        Self {
            focus_handle,
            state: LoadState::Loading,
            side_panel: SidePanel::None,
            debug,
            columns,
            personal_rating,
            export: None,
            export_status: None,
            auto_export,
            path,
            _parse_task: parse_task,
            _table_subscription: None,
        }
    }

    /// Applies a runtime debug-mode toggle from `ReplayInspectorView`:
    /// updates `debug`, threads it into the already-loaded table (if any),
    /// and closes a debug-only side panel that just lost its gate rather than
    /// leaving it stranded open with no way to reopen it.
    pub fn set_debug(&mut self, debug: bool, cx: &mut Context<Self>) {
        self.debug = debug;
        if !debug
            && matches!(self.side_panel, SidePanel::RawMetadata | SidePanel::RawResults | SidePanel::MappedResults)
        {
            self.side_panel = SidePanel::None;
        }
        if let LoadState::Loaded(loaded) = &self.state {
            loaded.table.update(cx, |table, cx| table.set_debug(debug, cx));
        }
        cx.notify();
    }

    /// Applies a new visible-column set from the header toolbar's
    /// column-filter checkboxes (`view.rs::ReplayInspectorView::set_column_filter`).
    /// Remembered for `apply_result` (in case the parse has not finished yet)
    /// and forwarded straight to `table` when it has.
    pub fn set_columns(&mut self, columns: Vec<ReplayColumn>, cx: &mut Context<Self>) {
        self.columns = columns.clone();
        if let LoadState::Loaded(loaded) = &self.state {
            loaded.table.update(cx, |table, cx| table.set_columns(columns, cx));
        }
    }

    /// Asks for a destination and writes the match to it.
    ///
    /// The document is held in its debug form, so an ordinary export strips a
    /// copy here rather than reparsing the replay.
    /// Writes this battle out on its own, for a reader who asked for every
    /// battle rather than this one.
    ///
    /// Named after the replay file so a directory of exports reads in the
    /// same order as the directory of replays it came from. A battle opened
    /// twice is written twice, over the same name, which is what the egui
    /// app does with the same setting.
    fn write_auto_export(&mut self, cx: &mut Context<Self>) {
        let AutoExport::To { directory, format } = &self.auto_export else { return };
        let Some(export) = self.export.clone() else { return };
        let export = if self.debug { export } else { export.stripped() };

        let Some(stem) = self.path.file_stem() else {
            tracing::warn!(path = %self.path.display(), "auto-export: the replay has no name to write under");
            return;
        };
        let path = directory.join(stem).with_extension(format.extension());
        let format = *format;

        cx.spawn(async move |this, cx| {
            let written = cx.background_spawn(async move { write_export(&export, &path, format) }).await;
            if let Err(err) = written {
                tracing::warn!("auto-export failed: {err}");
                let _ = this.update(cx, |this, cx| {
                    this.export_status = Some(err.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn export_match(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        let Some(export) = self.export.clone() else {
            return;
        };
        let export = if self.debug { export } else { export.stripped() };

        let name = match &self.state {
            LoadState::Loaded(loaded) => loaded.title.replace([' ', '/'], "_"),
            _ => "replay".to_string(),
        };
        let asked = crate::dialog::save_file(Some("Export results"), &format!("{name}.{}", format.extension()), None);

        cx.spawn(async move |this, cx| {
            let Some(path) = asked.await else { return };
            let written = cx.background_spawn(async move { write_export(&export, &path, format) }).await;
            let _ = this.update(cx, |this, cx| {
                this.export_status = match written {
                    Ok(()) => Some("Results exported".to_string()),
                    Err(err) => {
                        tracing::warn!("replay export failed: {err}");
                        Some(err.to_string())
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Applies an expected-values table that arrived after this tab opened
    /// (`view.rs::ReplayInspectorView::set_personal_rating`). Remembered for
    /// `apply_result` in case the parse has not finished yet, and pushed
    /// straight into an already-loaded table otherwise. Idempotent: rows that
    /// already carry a rating are left alone.
    pub fn set_personal_rating(&mut self, table: Arc<PersonalRatingData>, cx: &mut Context<Self>) {
        self.personal_rating = Some(table.clone());
        if let LoadState::Loaded(loaded) = &self.state {
            loaded.table.update(cx, |player_table, cx| player_table.populate_personal_ratings(&table, cx));
            cx.notify();
        }
    }

    /// Looks up who was in the monitored Twitch channel's chat around this
    /// battle and flags the rows their logins plausibly name.
    ///
    /// The observations are in the shared database, which the egui app fills
    /// from its own poll and this one from `App::start_twitch_poll`; a
    /// lookup that finds nothing simply leaves the rows unflagged.
    fn load_twitch_candidates(&self, table: Entity<PlayerTable>, battle_at: jiff::Timestamp, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let start = battle_at.as_second() + (twitch::WINDOW_BEFORE_MINUTES * 60.0) as i64;
        let end = battle_at.as_second() + (twitch::WINDOW_AFTER_MINUTES * 60.0) as i64;

        cx.spawn(async move |_this, cx| {
            let found =
                crate::runtime::spawn(cx, async move { query::observations_in_window(&pool, start, end).await }).await;
            let rows = match found {
                Ok(Ok(rows)) => rows,
                Ok(Err(err)) => {
                    tracing::warn!("replay inspector: the chat lookup failed: {err}");
                    return;
                }
                Err(err) => {
                    tracing::warn!("replay inspector: the chat lookup did not complete: {err}");
                    return;
                }
            };
            let observations: Vec<(String, jiff::Timestamp)> = rows
                .into_iter()
                .filter_map(|(login, seen_at)| {
                    jiff::Timestamp::from_second(seen_at).ok().map(|seen_at| (login, seen_at))
                })
                .collect();
            if observations.is_empty() {
                return;
            }
            table.update(cx, |table, cx| table.populate_twitch_candidates(&observations, cx));
        })
        .detach();
    }

    fn apply_result(
        &mut self,
        result: Result<ParsedReplay, ReplayLoadError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state = match result {
            Ok(ParsedReplay {
                model,
                export,
                game_data,
                raw_metadata_json,
                raw_results_json,
                mapped_results_json,
                fire_chance,
                session_stat: _,
                // The index reads this from its own pass over the directory,
                // not from a replay opened for reading.
                indexable: _,
            }) => {
                self.export = Some(export);
                self.write_auto_export(cx);
                let payloads = DebugPayloads { raw_metadata_json, raw_results_json, mapped_results_json };
                self.loaded_state(model, game_data.vfs().clone(), fire_chance, payloads, window, cx)
            }
            Err(err) => {
                // The panel says what went wrong; the toast is so a reader
                // who has moved on still learns it did, which is what the
                // egui app reports here (`ui/replay_parser/mod.rs`).
                crate::toast::failed(t!("ui.messages.replay_load_failed").to_string(), window, cx);
                LoadState::Failed(err)
            }
        };
        cx.notify();
    }

    /// Builds the loaded state from a parsed model: the tab title, the player
    /// table (subscribed to), and the chat/raw-JSON side panels. Split out of
    /// `apply_result` so tests can reach it with a fabricated model and an
    /// in-memory VFS, without a real replay and game install to parse.
    fn loaded_state(
        &mut self,
        mut model: ReplayReportModel,
        vfs: VfsPath,
        fire_chance: Option<EffectiveFireChance>,
        payloads: DebugPayloads,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> LoadState {
        let DebugPayloads { raw_metadata_json, raw_results_json, mapped_results_json } = payloads;
        model.columns = self.columns.clone();
        model.set_fire_chance(fire_chance);
        // The table may have arrived while this replay was parsing, in which
        // case `spawn_parse` never saw it. Populating before `PlayerTable::new`
        // keeps the PR column sortable from the first frame.
        if let Some(table) = self.personal_rating.as_ref() {
            model.populate_personal_ratings(table);
        }
        let ship_name = model.rows.iter().find(|row| row.is_self).map(|row| row.ship_name.clone()).unwrap_or_default();
        let title: SharedString =
            if ship_name.is_empty() { model.map.clone() } else { format!("{ship_name} - {}", model.map) }.into();
        let battle_result = model.battle_result;
        let chat = std::mem::take(&mut model.chat);
        let chat_title = title.to_string();
        let chat_panel = (!chat.is_empty()).then(|| cx.new(|cx| ChatPanel::new(chat, chat_title, cx)));
        let battle_at = model.timestamp;
        let table = cx.new(|cx| PlayerTable::new(model, vfs, self.debug, cx));
        self.load_twitch_candidates(table.clone(), battle_at, cx);
        self._table_subscription = Some(cx.subscribe_in(&table, window, Self::on_table_event));
        let raw_metadata_panel = cx.new(|cx| RawJsonPanel::new(raw_metadata_json.into(), window, cx));
        let raw_results_panel = raw_results_json.map(|json| cx.new(|cx| RawJsonPanel::new(json.into(), window, cx)));
        let mapped_results_panel =
            mapped_results_json.map(|json| cx.new(|cx| RawJsonPanel::new(json.into(), window, cx)));

        LoadState::Loaded(Box::new(LoadedReplay {
            title,
            battle_result,
            table,
            chat_panel,
            raw_metadata_panel,
            raw_results_panel,
            mapped_results_panel,
            raw_player_metadata_panel: None,
        }))
    }

    /// A panel already showing `model`, with no parse behind it. Test-only:
    /// production panels always reach this state through `apply_result`.
    #[cfg(test)]
    pub(crate) fn loaded_for_test(
        model: ReplayReportModel,
        personal_rating: Option<Arc<PersonalRatingData>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self {
            focus_handle: cx.focus_handle(),
            state: LoadState::Loading,
            side_panel: SidePanel::None,
            debug: false,
            columns: model.columns.clone(),
            personal_rating,
            // Stands in for the document the parse builds: the Export menu's
            // gate is that there is one, not what is in it.
            export: Some(ExportedMatch::new(&super::test_support::fixture_empty_battle_report(), &[], &[], true)),
            export_status: None,
            auto_export: AutoExport::Off,
            path: PathBuf::from("test.wowsreplay"),
            _parse_task: Task::ready(()),
            _table_subscription: None,
        };
        let vfs: VfsPath = wowsunpack::vfs::MemoryFS::new().into();
        let payloads =
            DebugPayloads { raw_metadata_json: String::from("{}"), raw_results_json: None, mapped_results_json: None };
        panel.state = panel.loaded_state(model, vfs, None, payloads, window, cx);
        panel
    }
}

impl EventEmitter<PanelEvent> for ReplayPanel {}

impl Focusable for ReplayPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ReplayPanel {
    fn panel_name(&self) -> &'static str {
        "ReplayPanel"
    }
}

impl Panel for ReplayPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        match &self.state {
            LoadState::Loading => SharedString::from(LOADING_TITLE),
            LoadState::Loaded(loaded) => loaded.title.clone(),
            LoadState::Failed(_) => SharedString::from(FAILED_TITLE),
        }
    }
}

/// The outcome badge: Win/Loss/Draw, colored and labeled exactly like the
/// egui app's Row 1 (see the module doc). Renders nothing when the replay
/// carries no battle result yet, matching the egui app's `if let
/// Some(battle_result)` gate (an incomplete-results replay, e.g. one still in
/// progress when saved).
fn outcome_badge(battle_result: Option<BattleResult>) -> AnyElement {
    let Some(result) = battle_result else {
        return h_flex().into_any_element();
    };

    let (glyph, label, outcome) = match result {
        BattleResult::Win(_) => (icons::TROPHY, "Victory", BattleOutcome::Win),
        BattleResult::Loss(_) => (icons::SMILEY_SAD, "Defeat", BattleOutcome::Loss),
        BattleResult::Draw => (icons::NOTCHES, "Draw", BattleOutcome::Draw),
    };
    let color = resolve_color(ColorRole::WinLoss(outcome));

    h_flex()
        .flex_none()
        .gap_1()
        .items_center()
        .px_2()
        .py_1()
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(icons::icon(glyph))
        .child(label)
        .into_any_element()
}

/// The Export dropdown, mirroring the egui app's three Export Results items.
/// Disabled until the parse finishes, since there is nothing to write before
/// then.
fn export_menu(panel: Entity<ReplayPanel>, can_export: bool) -> impl IntoElement {
    let trigger = Button::new("replay-export-trigger")
        .child(icons::icon(icons::DOWNLOAD_SIMPLE))
        .label(t!("ui.replay.section_export").to_string())
        .compact()
        .disabled(!can_export)
        .when(!can_export, |this| this.tooltip(t!("ui.replay.loading").to_string()));

    Popover::new("replay-export").trigger(trigger).content(move |_state, _window, _cx| {
        let panel = panel.clone();
        v_flex().w(EXPORT_MENU_WIDTH).gap_1().p_1().children(ExportFormat::ALL.map(|format| {
            let panel = panel.clone();
            Button::new(format.id()).label(format.label()).compact().on_click(move |_event, _window, cx: &mut App| {
                panel.update(cx, |this, cx| this.export_match(format, cx));
            })
        }))
    })
}

/// What a tab needs to start reading a replay.
///
/// Passed as one value rather than as six loose arguments, so a caller cannot
/// transpose two of them.
pub struct PanelSetup {
    pub path: PathBuf,
    pub game_data: GameDataCache,
    /// Whether the debug-only columns and side panels are offered.
    pub debug: bool,
    pub columns: Vec<ReplayColumn>,
    /// The expected-values table, when one has been read.
    pub personal_rating: Option<Arc<PersonalRatingData>>,
    pub auto_export: AutoExport,
}

/// Whether a finished parse writes itself out, and where.
///
/// A directory that is not there is not one: the setting is read at open
/// time, so a directory removed since then simply turns the writing off
/// rather than failing once per battle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AutoExport {
    #[default]
    Off,
    To {
        directory: PathBuf,
        format: ExportFormat,
    },
}

impl From<wows_toolkit_config::ReplayExportFormat> for ExportFormat {
    fn from(format: wows_toolkit_config::ReplayExportFormat) -> ExportFormat {
        match format {
            wows_toolkit_config::ReplayExportFormat::Json => ExportFormat::Json,
            wows_toolkit_config::ReplayExportFormat::Cbor => ExportFormat::Cbor,
            wows_toolkit_config::ReplayExportFormat::Csv => ExportFormat::Csv,
        }
    }
}

impl AutoExport {
    /// Reads the setting, refusing a directory that is not one.
    pub fn from_settings(settings: &ReplaySettings) -> AutoExport {
        if !settings.auto_export_data {
            return AutoExport::Off;
        }
        let directory = PathBuf::from(&settings.auto_export_path);
        if !directory.is_dir() {
            return AutoExport::Off;
        }
        AutoExport::To { directory, format: ExportFormat::from(settings.auto_export_format) }
    }
}

/// What the Export menu writes.
///
/// JSON and CBOR carry the whole match; CSV carries one flattened row per
/// vehicle, which is what a spreadsheet can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Cbor,
    Csv,
}

impl ExportFormat {
    const ALL: [ExportFormat; 3] = [Self::Json, Self::Cbor, Self::Csv];

    fn label(self) -> String {
        match self {
            Self::Json => t!("ui.replay.export_results_json").into_owned(),
            Self::Cbor => t!("ui.replay.export_results_cbor").into_owned(),
            Self::Csv => t!("ui.replay.export_results_csv").into_owned(),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Cbor => "cbor",
            Self::Csv => "csv",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Json => "replay-export-json",
            Self::Cbor => "replay-export-cbor",
            Self::Csv => "replay-export-csv",
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ExportError {
    #[error("could not create {path}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {reason}")]
    Write { path: PathBuf, reason: String },
}

/// Writes `match_data` to `path`.
fn write_export(match_data: &ExportedMatch, path: &Path, format: ExportFormat) -> Result<(), ExportError> {
    let file = std::io::BufWriter::new(
        std::fs::File::create(path).map_err(|source| ExportError::Create { path: path.to_path_buf(), source })?,
    );
    let failed = |reason: String| ExportError::Write { path: path.to_path_buf(), reason };

    match format {
        ExportFormat::Json => serde_json::to_writer(file, match_data).map_err(|err| failed(err.to_string())),
        ExportFormat::Cbor => ciborium::into_writer(match_data, file).map_err(|err| failed(err.to_string())),
        ExportFormat::Csv => {
            let mut writer = csv::WriterBuilder::new().has_headers(true).from_writer(file);
            for vehicle in match_data.vehicles.iter().cloned() {
                writer.serialize(FlattenedVehicle::from(vehicle)).map_err(|err| failed(err.to_string()))?;
            }
            writer.flush().map_err(|err| failed(err.to_string()))
        }
    }
}

/// The PR badge's element id, so a test can assert on its presence and label.
const PR_BADGE_ID: &str = "replay-personal-rating-badge";

/// The badge's text, verbatim the egui `pr_chip` call's format string.
fn personal_rating_label(rating: &PersonalRatingResult) -> String {
    format!("PR: {:.0} ({})", rating.pr, rating.category.name())
}

/// The chip tint the egui `pr_chip` paints behind its text.
const PR_CHIP_TINT: f32 = personal_rating::CHIP_TINT_ALPHA as f32 / 255.;

/// The single-battle PR badge, mirroring the egui app's `pr_chip` call beside
/// the outcome label: "PR: {score} ({band})" in the band's color over a faint
/// tint of it. Renders nothing when no expected-values table has been applied
/// yet, or when this replay's own row carries no rating, matching that call's
/// `if let Some(pr_result)` gate.
fn personal_rating_badge(rating: PersonalRatingResult) -> AnyElement {
    let color = resolve_color(ColorRole::PrTier(rating.category));
    let text = personal_rating_label(&rating);
    let mut tint = resolve_color(ColorRole::PrTierTint(rating.category));
    tint.a = PR_CHIP_TINT;

    h_flex()
        .id(PR_BADGE_ID)
        .test_support()
        .aria_label(text.clone())
        .flex_none()
        .items_center()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(tint)
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(text)
        .into_any_element()
}

/// One side-panel toggle button's static shape, bundled into a struct so
/// `side_panel_button` stays under clippy's argument-count limit (mirrors
/// `table.rs::RowLayout`'s reason for existing).
struct SidePanelButtonSpec {
    id: &'static str,
    icon: IconName,
    label_key: &'static str,
    panel: SidePanel,
    enabled: bool,
    disabled_tooltip_key: Option<&'static str>,
}

/// One side-panel toggle button: selected while `spec.panel` is the active
/// `SidePanel`, disabled when `spec.enabled` is false (with
/// `spec.disabled_tooltip` shown on hover), clicking toggles between
/// `spec.panel` and `SidePanel::None` via `ReplayPanel::toggle_side_panel`.
fn side_panel_button(spec: SidePanelButtonSpec, current: SidePanel, cx: &mut Context<ReplayPanel>) -> AnyElement {
    let panel = spec.panel;
    Button::new(spec.id)
        .icon(spec.icon)
        .label(t!(spec.label_key).into_owned())
        .compact()
        .selected(current == panel)
        .disabled(!spec.enabled)
        .when_some((!spec.enabled).then_some(spec.disabled_tooltip_key).flatten(), |this, key| {
            this.tooltip(t!(key).into_owned())
        })
        .on_click(cx.listener(move |this, _event, _window, cx| this.toggle_side_panel(panel, cx)))
        .into_any_element()
}

/// The header row: the outcome badge on the left, the chat toggle and (when
/// `debug` is on) the raw-metadata/raw-results debug viewer toggles on the
/// right. Mirrors the egui app's Row 1 layout plus its debug-mode buttons
/// (see the module doc); `has_chat`/`has_results` disable their respective
/// buttons exactly like `ui.add_enabled(...)` there, since a chat-less or
/// results-less replay has nothing to show.
/// What the header row draws, bundled so it stays under clippy's
/// argument-count limit (the same reason `SidePanelButtonSpec` exists).
struct HeaderState {
    battle_result: Option<BattleResult>,
    personal_rating: Option<PersonalRatingResult>,
    has_chat: bool,
    has_results: bool,
    /// Whether the results resolved into their named form, which is a
    /// separate viewer from the raw payload.
    has_mapped_results: bool,
    /// Whether the recording player is in a test ship, which is the only
    /// case the hide-my-stats toggle means anything in.
    self_is_test_ship: bool,
    /// Whether their own figures are currently hidden.
    self_stats_hidden: bool,
    /// Whether the parse has produced a document to export yet.
    can_export: bool,
    export_status: Option<String>,
    debug: bool,
    side_panel: SidePanel,
}

/// The subdued line under the header: who was recording, which match, and
/// what each side dealt.
///
/// Mirrors the egui app's Row 2, down to the interpunct between the fields
/// and the win/loss tones on the two damage figures.
fn match_context_line(context: &MatchContext, team_damage: (u64, u64)) -> AnyElement {
    let (friendly, enemy) = team_damage;
    let dim = crate::theme::text_dim();
    let separator = || div().flex_none().text_xs().text_color(dim).child(SEPARATOR);
    let field = |text: String| div().flex_none().text_xs().text_color(dim).child(text);

    // The clan tag reads as part of the name, so it sits against it rather
    // than as a field of its own.
    let who = match context.clan_tag.as_ref() {
        Some(clan) => format!("{clan} {}", context.player_name),
        None => context.player_name.clone(),
    };

    let mut line = h_flex().id("replay-match-context").test_support().flex_none().items_center().gap_1().px_2().pb_1();
    let mut drawn = 0usize;
    for text in [&who, &context.game_type, &context.version, &context.game_mode, &context.map] {
        if text.is_empty() {
            continue;
        }
        if drawn > 0 {
            line = line.child(separator());
        }
        line = line.child(field(text.clone()));
        drawn += 1;
    }

    line.child(separator())
        .child(field(t!("ui.replay.team_damage").into_owned()))
        .child(
            div().flex_none().text_xs().text_color(rgb(crate::theme::semantic().win)).child(separate_number(friendly)),
        )
        .child(field(" : ".to_string()))
        .child(div().flex_none().text_xs().text_color(rgb(crate::theme::semantic().loss)).child(separate_number(enemy)))
        .child(field(format!(" ({})", separate_number(friendly + enemy))))
        .into_any_element()
}

/// What the egui line puts between its fields.
const SEPARATOR: &str = "-";

/// The header's Actions menu.
///
/// Only what this port can actually do is here: the egui menu also carries
/// the match timeline and the other-team perspective, neither of which the
/// port has. Shown only for a test ship, which is the one case hiding your
/// own figures means anything in.
fn actions_menu(panel: Entity<ReplayPanel>, hidden: bool) -> impl IntoElement + use<> {
    Button::new("replay-actions").label(t!("ui.replay.actions").into_owned()).compact().dropdown_menu(
        move |menu, _window, _cx| {
            let panel = panel.clone();
            menu.item(PopupMenuItem::new(t!("ui.replay.hide_my_stats").into_owned()).checked(hidden).on_click(
                move |_event, _window, cx| {
                    panel.update(cx, |panel, cx| panel.set_self_stats_hidden(!hidden, cx));
                },
            ))
        },
    )
}

fn header_row(state: HeaderState, cx: &mut Context<ReplayPanel>) -> AnyElement {
    let HeaderState {
        battle_result,
        personal_rating,
        has_chat,
        has_results,
        has_mapped_results,
        self_is_test_ship,
        self_stats_hidden,
        can_export,
        export_status,
        debug,
        side_panel,
    } = state;

    let chat_button = side_panel_button(
        SidePanelButtonSpec {
            id: "replay-chat-toggle",
            icon: IconName::PanelRight,
            label_key: "ui.replay.chat",
            panel: SidePanel::Chat,
            enabled: has_chat,
            disabled_tooltip_key: Some("ui.replay.no_chat"),
        },
        side_panel,
        cx,
    );

    let mut buttons = h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .when_some(export_status, |this, status| {
            this.child(div().text_xs().text_color(crate::theme::text_dim()).child(status))
        })
        .when(self_is_test_ship, |row| row.child(actions_menu(cx.entity(), self_stats_hidden)))
        .child(export_menu(cx.entity(), can_export))
        .child(chat_button);
    if debug {
        buttons = buttons
            .child(side_panel_button(
                SidePanelButtonSpec {
                    id: "replay-debug-raw-metadata",
                    icon: IconName::File,
                    label_key: "ui.replay.debug.raw_metadata",
                    panel: SidePanel::RawMetadata,
                    enabled: true,
                    disabled_tooltip_key: None,
                },
                side_panel,
                cx,
            ))
            .child(side_panel_button(
                SidePanelButtonSpec {
                    id: "replay-debug-raw-results",
                    icon: IconName::File,
                    label_key: "ui.replay.debug.raw_results",
                    panel: SidePanel::RawResults,
                    enabled: has_results,
                    disabled_tooltip_key: Some("ui.replay.debug.no_results_packet"),
                },
                side_panel,
                cx,
            ))
            .child(side_panel_button(
                SidePanelButtonSpec {
                    id: "replay-debug-mapped-results",
                    icon: IconName::File,
                    label_key: "ui.replay.debug.mapped_results",
                    panel: SidePanel::MappedResults,
                    enabled: has_mapped_results,
                    disabled_tooltip_key: Some("ui.replay.debug.no_results_packet"),
                },
                side_panel,
                cx,
            ));
    }

    h_flex()
        .flex_none()
        .items_center()
        .justify_between()
        .pr_2()
        .gap_1()
        .child(
            h_flex()
                .flex_none()
                .items_center()
                .gap_1()
                // A replay recorded before the battle ended carries no
                // results, so every server figure in the table is absent
                // rather than zero. Said up front, as the egui app does.
                .when(!has_results, |this| {
                    this.child(
                        h_flex()
                            .id("replay-incomplete-results")
                            .test_support()
                            .gap_1()
                            .items_center()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(crate::theme::semantic().warn))
                            .child(crate::icons::icon(crate::icons::INFO))
                            .child(t!("ui.replay.incomplete_results").into_owned())
                            .tooltip(|window, cx| {
                                Tooltip::new(t!("ui.replay.incomplete_results_tooltip").into_owned()).build(window, cx)
                            }),
                    )
                })
                .child(outcome_badge(battle_result))
                .when_some(personal_rating, |this, rating| this.child(personal_rating_badge(rating))),
        )
        .child(buttons)
        .into_any_element()
}

impl ReplayPanel {
    /// Hides or shows the recording player's own figures, for a test ship.
    fn set_self_stats_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let LoadState::Loaded(loaded) = &self.state else { return };
        loaded.table.update(cx, |table, cx| table.set_self_stats_hidden(hidden, cx));
        cx.notify();
    }

    /// Toggles the side-panel slot: switching to whichever of `panel`
    /// is not already showing, or closing it if `panel` is already active.
    fn toggle_side_panel(&mut self, panel: SidePanel, cx: &mut Context<Self>) {
        self.side_panel = if self.side_panel == panel { SidePanel::None } else { panel };
        cx.notify();
    }

    /// Handles `table`'s `PlayerTableEvent`s: on `ViewRawJson` (the Actions
    /// menu's "View Raw Player Metadata" item), builds a fresh `RawJsonPanel`
    /// for the clicked row's JSON and opens it in the side-panel slot.
    /// Unlike `toggle_side_panel`, this always opens rather than toggling --
    /// clicking a different row's "View Raw Player Metadata" while the panel
    /// is already showing should swap its content, not close it.
    fn on_table_event(
        &mut self,
        _table: &Entity<PlayerTable>,
        event: &PlayerTableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PlayerTableEvent::ViewRawJson(json) = event;
        if let LoadState::Loaded(loaded) = &mut self.state {
            loaded.raw_player_metadata_panel = Some(cx.new(|cx| RawJsonPanel::new(json.clone(), window, cx)));
        }
        self.side_panel = SidePanel::RawPlayerMetadata;
        cx.notify();
    }
}

impl Render for ReplayPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            LoadState::Loading => {
                div().p_2().text_sm().text_color(crate::theme::text_dim()).child(LOADING_TITLE).into_any_element()
            }
            LoadState::Failed(err) => v_flex()
                .p_2()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(FAILED_TITLE))
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(err.to_string()))
                .into_any_element(),
            LoadState::Loaded(loaded) => {
                let has_chat = loaded.chat_panel.is_some();
                let has_results = loaded.raw_results_panel.is_some();
                let has_mapped_results = loaded.mapped_results_panel.is_some();
                let self_is_test_ship = loaded.table.read(cx).self_is_test_ship();
                let self_stats_hidden = loaded.table.read(cx).self_stats_hidden();
                let table = loaded.table.clone();
                let battle_result = loaded.battle_result;
                let personal_rating = loaded.table.read(cx).self_personal_rating();
                let team_damage = loaded.table.read(cx).team_damage();
                let border = cx.theme().border;

                let side_panel_entity: Option<AnyView> = match self.side_panel {
                    SidePanel::None => None,
                    SidePanel::Chat => loaded.chat_panel.clone().map(|panel| panel.into()),
                    SidePanel::RawMetadata => Some(loaded.raw_metadata_panel.clone().into()),
                    SidePanel::RawResults => loaded.raw_results_panel.clone().map(|panel| panel.into()),
                    SidePanel::MappedResults => loaded.mapped_results_panel.clone().map(|panel| panel.into()),
                    SidePanel::RawPlayerMetadata => loaded.raw_player_metadata_panel.clone().map(|panel| panel.into()),
                };

                let context_line = match_context_line(loaded.table.read(cx).match_context(), team_damage);

                v_flex()
                    .size_full()
                    .child(header_row(
                        HeaderState {
                            battle_result,
                            personal_rating,
                            has_chat,
                            has_results,
                            has_mapped_results,
                            self_is_test_ship,
                            self_stats_hidden,
                            can_export: self.export.is_some(),
                            export_status: self.export_status.clone(),
                            debug: self.debug,
                            side_panel: self.side_panel,
                        },
                        cx,
                    ))
                    .child(context_line)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h(px(0.))
                            .child(div().flex_1().min_w(px(0.)).h_full().child(table))
                            .when_some(side_panel_entity, |row, side_panel| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .w(SIDE_PANEL_WIDTH)
                                        .h_full()
                                        .border_l_1()
                                        .border_color(border)
                                        .child(side_panel),
                                )
                            }),
                    )
                    .into_any_element()
            }
        };

        v_flex().id("replay-panel").track_focus(&self.focus_handle).size_full().child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::MatchContext;
    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;
    use std::sync::Arc;
    use wows_replay_insights::personal_rating::PersonalRatingCategory;
    use wows_replay_insights::personal_rating::PersonalRatingResult;
    use wows_replays::analyzer::battle_controller::BattleResult;
    use wows_replays::types::Relation;
    use wows_replays::types::TeamId;
    use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

    use super::ExportFormat;
    use super::ExportedMatch;
    use super::PR_BADGE_ID;
    use super::ReplayPanel;
    use super::personal_rating_label;
    use super::write_export;
    use crate::replay_inspector::columns::ReplayColumn;
    use crate::replay_inspector::model::PlayerRow;
    use crate::replay_inspector::model::ReplayReportModel;
    use crate::replay_inspector::test_support::FIXTURE_PR_SHIP_ID;
    use crate::replay_inspector::test_support::base_row;
    use crate::replay_inspector::test_support::fixture_personal_rating_data;

    /// A won battle whose self row dealt exactly the fixture's expected
    /// damage and frags. The win carries the win-rate term to twice expected,
    /// so the single battle scores 700 + 300 + 650 = 1650.
    fn model_at_expected_values() -> ReplayReportModel {
        let self_row = PlayerRow {
            ship_id: Some(FIXTURE_PR_SHIP_ID.into()),
            actual_damage: Some(50_000),
            kills: Some(1),
            ..base_row(1, Relation::new(0), true)
        };
        let enemy = PlayerRow { ship_id: None, ..base_row(2, Relation::new(2), false) };

        ReplayReportModel {
            self_team: TeamId::from(0i64),
            rows: vec![self_row, enemy],
            battle_result: Some(BattleResult::Win(0)),
            columns: ReplayColumn::ALL.to_vec(),
            map: "Ocean".to_string(),
            chat: Vec::new(),
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            context: MatchContext::default(),
        }
    }

    /// The menu opens only once there is something to write, and each item
    /// writes the format it names.
    #[gpui_kit::test]
    fn the_export_menu_opens_once_a_replay_has_loaded(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, window, cx)
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-export-json").is_none(), "the formats live behind the trigger");
            window.click("replay-export-trigger", cx);
            for format in ExportFormat::ALL {
                assert!(window.try_find(format.id()).is_some(), "{} is offered", format.label());
            }
        })
        .expect("the window is open");
    }

    /// The recording player's own figures can be hidden when they are in a
    /// test ship, which is the case the NDA covers.
    #[gpui_kit::test]
    fn a_test_ship_s_own_figures_can_be_hidden(cx: &mut TestAppContext) {
        let mut model = model_at_expected_values();
        for row in &mut model.rows {
            if row.is_self {
                row.is_test_ship = true;
            }
        }

        cx.update(gpui_kit::init);
        let window = cx
            .open_window(size(px(1400.), px(600.)), |window, cx| ReplayPanel::loaded_for_test(model, None, window, cx));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-actions").is_some(), "a test ship is offered the toggle");
        })
        .expect("the window is open");

        window
            .update(cx, |panel, _window, cx| {
                panel.set_self_stats_hidden(true, cx);
                let super::LoadState::Loaded(loaded) = &panel.state else { panic!("the panel is loaded") };
                assert!(loaded.table.read(cx).self_stats_hidden(), "the figures are hidden");
            })
            .expect("the window is open");
    }

    /// Dragging a column header's grip widens that column, and every row
    /// follows it. A column nobody dragged keeps fitting its content.
    #[test]
    fn the_auto_export_setting_refuses_a_directory_that_is_not_one() {
        use super::AutoExport;
        use wows_toolkit_config::ReplayExportFormat;
        use wows_toolkit_config::ReplaySettings;

        let off = ReplaySettings { auto_export_data: false, ..ReplaySettings::default() };
        assert_eq!(AutoExport::from_settings(&off), AutoExport::Off, "the setting is what turns it on");

        let nowhere = ReplaySettings {
            auto_export_data: true,
            auto_export_path: "G:/does-not-exist".to_string(),
            ..ReplaySettings::default()
        };
        assert_eq!(AutoExport::from_settings(&nowhere), AutoExport::Off, "a missing directory is not one to write to");

        let here = ReplaySettings {
            auto_export_data: true,
            auto_export_path: std::env::temp_dir().to_string_lossy().into_owned(),
            auto_export_format: ReplayExportFormat::Csv,
            ..ReplaySettings::default()
        };
        assert!(
            matches!(AutoExport::from_settings(&here), AutoExport::To { format, .. } if format == ExportFormat::Csv),
            "the chosen format carries through"
        );
    }

    #[gpui_kit::test]
    fn dragging_a_header_grip_widens_that_column(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1400.), px(600.)), |window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, window, cx)
        });

        let grip = ("replay-header-grip", ReplayColumn::ShipName as usize);
        let before = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find(grip).bounds()
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            let from = before.center();
            window.drag(from, gpui_kit::point(from.x + px(60.), from.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        let after = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find(grip).bounds()
            })
            .expect("the window is open");

        assert!(
            after.origin.x > before.origin.x,
            "the grip moved right with the column it widens, from {:?} to {:?}",
            before.origin.x,
            after.origin.x
        );
    }

    /// The recording player's row carries the fire-chance block, and nobody
    /// else's does: the statistic is about the shells this client fired.
    #[gpui_kit::test]
    fn the_fire_chance_block_is_on_the_recording_player_s_row(cx: &mut TestAppContext) {
        use std::collections::BTreeMap;
        use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;

        let fire_chance = EffectiveFireChance {
            he_shells_fired: 120,
            hits: 60,
            narrowed: BTreeMap::new(),
            he_hits_on_a_ship: 40,
            hits_without_a_target_ship: 0,
            not_applicable: 0,
            eligible_hits: 40,
            fires: 3,
            expected_fires: Some(4.2),
            per_ship: Vec::new(),
            exclusions: BTreeMap::new(),
            section_predictions: Vec::new(),
            set_fire_ribbons: 3,
            unattributed_fires: 0,
            unattributed_reasons: BTreeMap::new(),
            formula_base: Some(0.12),
            formula: Vec::new(),
        };

        let mut model = model_at_expected_values();
        model.set_fire_chance(Some(fire_chance));

        let on_self = model.rows.iter().filter(|row| row.fire_chance.is_some()).count();
        assert_eq!(on_self, 1, "exactly one row carries the statistic");
        let carried = model
            .rows
            .iter()
            .find(|row| row.is_self)
            .and_then(|row| row.fire_chance.as_ref())
            .expect("on the self row");

        // The block's own wording, which is the shared reading of the result.
        let counts = wows_toolkit_viewmodel::fire_chance::counts_text(carried.fires, carried.eligible_hits);
        assert!(counts.contains("3 fires"), "got {counts:?}");
        assert!(counts.contains("40 hits"), "got {counts:?}");
        assert_eq!(
            wows_toolkit_viewmodel::fire_chance::expected_fires_text(carried).as_deref(),
            Some("expected 4.2 fires")
        );

        // The panel draws it without a window of its own to open.
        let _ = cx;
    }

    /// The line under the header names the match, and a replay with no
    /// results in it says so rather than showing an empty table of figures.
    #[gpui_kit::test]
    fn the_header_names_the_match_and_flags_missing_results(cx: &mut TestAppContext) {
        let mut model = model_at_expected_values();
        model.context = MatchContext {
            clan_tag: Some("[RAIN]".to_string()),
            player_name: "gapedd".to_string(),
            game_type: "Random Battle".to_string(),
            version: "15.7.0".to_string(),
            game_mode: "Domination".to_string(),
            map: "Two Brothers".to_string(),
        };

        cx.update(gpui_kit::init);
        let window = cx
            .open_window(size(px(1400.), px(600.)), |window, cx| ReplayPanel::loaded_for_test(model, None, window, cx));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-match-context").is_some(), "the match-context line is drawn");
            // `loaded_for_test` carries no results payload, which is the
            // pending case.
            assert!(window.try_find("replay-incomplete-results").is_some(), "and the missing results are flagged");
        })
        .expect("the window is open");
    }

    /// The markers beside a name: a hidden profile, a chat login that
    /// plausibly names the player, and a connection that dropped.
    ///
    /// Each is drawn only for the row it belongs to, so a table that drew
    /// them for everyone would read as an accusation of everyone.
    #[gpui_kit::test]
    fn a_name_carries_its_own_markers(cx: &mut TestAppContext) {
        use wows_replay_insights::battle_report::ConnectionNote;
        use wows_toolkit_viewmodel::twitch::SniperCandidate;

        let mut model = model_at_expected_values();
        model.rows[0].is_hidden_profile = true;
        model.rows[0].twitch_candidates = vec![SniperCandidate { login: "harvey635".to_string(), minutes: vec![3] }];
        model.rows[0].connection = Some(ConnectionNote::NeverConnected);

        cx.update(gpui_kit::init);
        let window = cx
            .open_window(size(px(1400.), px(600.)), |window, cx| ReplayPanel::loaded_for_test(model, None, window, cx));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("replay-hidden-profile", 0usize)).is_some(), "the hidden-profile eye is drawn");
            assert!(window.try_find(("replay-twitch", 0usize)).is_some(), "the twitch chip is drawn");
            assert!(window.try_find(("replay-disconnect", 0usize)).is_some(), "the connection marker is drawn");

            assert!(window.try_find(("replay-hidden-profile", 1usize)).is_none(), "and not on a row without them");
            assert!(window.try_find(("replay-twitch", 1usize)).is_none(), "and not on a row without them");
            assert!(window.try_find(("replay-disconnect", 1usize)).is_none(), "and not on a row without them");
        })
        .expect("the window is open");
    }

    /// Every format writes a file the caller can read back, and CSV carries a
    /// header row plus one row per vehicle.
    #[test]
    fn each_export_format_writes_its_own_shape() {
        use crate::replay_inspector::test_support::fixture_empty_battle_report;

        let normalized = fixture_empty_battle_report();
        let export = ExportedMatch::new(&normalized, &[], &[], true);
        let dir = std::env::temp_dir().join(format!("wt-gpui-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");

        for format in ExportFormat::ALL {
            let path = dir.join(format!("match.{}", format.extension()));
            write_export(&export, &path, format).expect("the export writes");

            assert!(path.is_file(), "{} wrote no file", format.label());
        }

        // CSV writes one row per vehicle and the fixture has none to zip raw
        // players against, so an empty file is the honest result there; the
        // whole-document formats always carry the metadata.
        assert!(!std::fs::read(dir.join("match.cbor")).expect("the CBOR exists").is_empty());

        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("match.json")).expect("the JSON exists"))
                .expect("the JSON parses back");
        assert!(json.get("metadata").is_some(), "the document keeps its metadata");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn the_pr_badge_reports_the_self_rows_rating(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let table = Arc::new(fixture_personal_rating_data());
        let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), Some(table), window, cx)
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(PR_BADGE_ID).label(),
                Some("PR: 1650 (Very Good)"),
                "expected damage and frags plus the win scores 1650"
            );
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn a_replay_with_no_rating_table_shows_no_pr_badge(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, window, cx)
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find(PR_BADGE_ID).is_none(),
                "the egui app draws no chip when it cannot compute a rating"
            );
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn a_rating_table_arriving_after_the_parse_still_fills_the_badge_in(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, window, cx)
        });

        let table = Arc::new(fixture_personal_rating_data());
        window.update(cx, |panel, _window, cx| panel.set_personal_rating(table, cx)).expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(PR_BADGE_ID).label(), Some("PR: 1650 (Very Good)"));
        })
        .expect("the window is open");
    }

    /// A table that names no ships rates nothing, so the badge stays away
    /// rather than showing a rating computed against nothing.
    #[gpui_kit::test]
    fn a_table_with_no_expected_values_rates_nothing(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, window, cx)
        });

        window
            .update(cx, |panel, _window, cx| panel.set_personal_rating(Arc::new(PersonalRatingData::new()), cx))
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(PR_BADGE_ID).is_none(), "an empty table rates nothing");
        })
        .expect("the window is open");
    }

    #[test]
    fn the_badge_label_matches_the_egui_chips_wording() {
        let rating = PersonalRatingResult::new(1150.0);
        assert_eq!(rating.category, PersonalRatingCategory::Average);
        assert_eq!(personal_rating_label(&rating), "PR: 1150 (Average)");
        assert_eq!(personal_rating_label(&PersonalRatingResult::new(2456.4)), "PR: 2456 (Super Unicum)");
    }
}

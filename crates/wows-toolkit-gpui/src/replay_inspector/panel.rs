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
use gpui_kit::component::popover::Popover;
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
use super::model::ReplayReportModel;
use super::table::PlayerTable;
use super::table::PlayerTableEvent;
use super::table::resolve_color;
use wows_toolkit_viewmodel::replay_export::FlattenedVehicle;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;

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
    /// The Actions menu's "View Raw Player Metadata" item (`table.rs`'s
    /// `PlayerTableEvent::ViewRawJson`); backed by
    /// `LoadedReplay::raw_player_metadata_panel`, rebuilt with the clicked
    /// row's JSON each time the event fires (see `on_table_event`).
    RawPlayerMetadata,
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
    raw_player_metadata_panel: Option<Entity<RawJsonPanel>>,
}

enum LoadState {
    Loading,
    Loaded(LoadedReplay),
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
    pub fn new(
        path: PathBuf,
        game_data: GameDataCache,
        debug: bool,
        columns: Vec<ReplayColumn>,
        personal_rating: Option<Arc<PersonalRatingData>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let parse_task = spawn_parse(path, game_data, personal_rating.clone(), cx);
        let parse_task = cx.spawn(async move |this, cx| {
            let result = parse_task.await;
            let _ = this.update(cx, |this, cx| this.apply_result(result, cx));
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
        if !debug && matches!(self.side_panel, SidePanel::RawMetadata | SidePanel::RawResults) {
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

    fn apply_result(&mut self, result: Result<ParsedReplay, ReplayLoadError>, cx: &mut Context<Self>) {
        self.state = match result {
            Ok(ParsedReplay { model, export, game_data, raw_metadata_json, raw_results_json }) => {
                self.export = Some(export);
                self.loaded_state(model, game_data.vfs().clone(), raw_metadata_json, raw_results_json, cx)
            }
            Err(err) => LoadState::Failed(err),
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
        raw_metadata_json: String,
        raw_results_json: Option<String>,
        cx: &mut Context<Self>,
    ) -> LoadState {
        model.columns = self.columns.clone();
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
        let table = cx.new(|cx| PlayerTable::new(model, vfs, self.debug, cx));
        self._table_subscription = Some(cx.subscribe(&table, Self::on_table_event));
        let raw_metadata_panel = cx.new(|cx| RawJsonPanel::new(raw_metadata_json.into(), cx));
        let raw_results_panel = raw_results_json.map(|json| cx.new(|cx| RawJsonPanel::new(json.into(), cx)));

        LoadState::Loaded(LoadedReplay {
            title,
            battle_result,
            table,
            chat_panel,
            raw_metadata_panel,
            raw_results_panel,
            raw_player_metadata_panel: None,
        })
    }

    /// A panel already showing `model`, with no parse behind it. Test-only:
    /// production panels always reach this state through `apply_result`.
    #[cfg(test)]
    pub(crate) fn loaded_for_test(
        model: ReplayReportModel,
        personal_rating: Option<Arc<PersonalRatingData>>,
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
            _parse_task: Task::ready(()),
            _table_subscription: None,
        };
        let vfs: VfsPath = wowsunpack::vfs::MemoryFS::new().into();
        panel.state = panel.loaded_state(model, vfs, String::from("{}"), None, cx);
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

/// What the Export menu writes.
///
/// JSON and CBOR carry the whole match; CSV carries one flattened row per
/// vehicle, which is what a spreadsheet can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
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
    /// Whether the parse has produced a document to export yet.
    can_export: bool,
    export_status: Option<String>,
    debug: bool,
    side_panel: SidePanel,
}

fn header_row(state: HeaderState, cx: &mut Context<ReplayPanel>) -> AnyElement {
    let HeaderState {
        battle_result,
        personal_rating,
        has_chat,
        has_results,
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
                .child(outcome_badge(battle_result))
                .when_some(personal_rating, |this, rating| this.child(personal_rating_badge(rating))),
        )
        .child(buttons)
        .into_any_element()
}

impl ReplayPanel {
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
    fn on_table_event(&mut self, _table: Entity<PlayerTable>, event: &PlayerTableEvent, cx: &mut Context<Self>) {
        let PlayerTableEvent::ViewRawJson(json) = event;
        if let LoadState::Loaded(loaded) = &mut self.state {
            loaded.raw_player_metadata_panel = Some(cx.new(|cx| RawJsonPanel::new(json.clone(), cx)));
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
                let table = loaded.table.clone();
                let battle_result = loaded.battle_result;
                let personal_rating = loaded.table.read(cx).self_personal_rating();
                let border = cx.theme().border;

                let side_panel_entity: Option<AnyView> = match self.side_panel {
                    SidePanel::None => None,
                    SidePanel::Chat => loaded.chat_panel.clone().map(|panel| panel.into()),
                    SidePanel::RawMetadata => Some(loaded.raw_metadata_panel.clone().into()),
                    SidePanel::RawResults => loaded.raw_results_panel.clone().map(|panel| panel.into()),
                    SidePanel::RawPlayerMetadata => loaded.raw_player_metadata_panel.clone().map(|panel| panel.into()),
                };

                v_flex()
                    .size_full()
                    .child(header_row(
                        HeaderState {
                            battle_result,
                            personal_rating,
                            has_chat,
                            has_results,
                            can_export: self.export.is_some(),
                            export_status: self.export_status.clone(),
                            debug: self.debug,
                            side_panel: self.side_panel,
                        },
                        cx,
                    ))
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
        }
    }

    /// The menu opens only once there is something to write, and each item
    /// writes the format it names.
    #[gpui_kit::test]
    fn the_export_menu_opens_once_a_replay_has_loaded(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, cx)
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
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), Some(table), cx)
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
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, cx)
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
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, cx)
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
        let window = cx.open_window(size(px(900.), px(600.)), |_window, cx| {
            ReplayPanel::loaded_for_test(model_at_expected_values(), None, cx)
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

//! `ReplayPanel`: one dock tab per open replay. Starts `load::spawn_parse` in
//! the background on construction and, once it completes, either shows the
//! per-replay outcome row plus the real `PlayerTable` (Milestones 1-3) or an
//! error message. Mirrors the egui app's `ReplayTab`/`ReplayTabViewer`
//! (`ui/replay_parser/mod.rs` ~120/4749): the tab title is "{ship} - {map}"
//! once loaded, `t!("ui.replay.loading")`'s "Loading..." until then. The
//! outcome row mirrors `build_replay_view`'s Row 1 (`mod.rs` ~2836-2852):
//! Win/Loss/Draw label, colored `LIGHT_GREEN`/`LIGHT_RED`/`LIGHT_YELLOW`.
//! `IconName` has no bundled trophy/sad-face/notches glyphs, so this uses the
//! closest available icons (thumbs up/down, minus) instead.
//!
//! **Deferred**, not implemented in this milestone:
//! - The single-battle PR badge (`PersonalRatingData::calculate_pr`) -- needs
//!   PR reference data this loader does not fetch yet.
//! - The export menu (JSON/CBOR/CSV via `util::replay_export`) -- that module
//!   lives in the egui crate; porting it is out of scope here.
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
//! v1 tradeoff as chat.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::Disableable;
use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::Selectable;
use gpui_component::button::Button;
use gpui_component::dock::Panel;
use gpui_component::dock::PanelEvent;
use gpui_component::h_flex;
use gpui_component::v_flex;
use wows_replays::analyzer::battle_controller::BattleResult;

use super::chat::ChatPanel;
use super::columns::BattleOutcome;
use super::columns::ColorRole;
use super::debug_view::RawJsonPanel;
use super::load::GameDataCache;
use super::load::ParsedReplay;
use super::load::ReplayLoadError;
use super::load::spawn_parse;
use super::table::PlayerTable;
use super::table::resolve_color;

const LOADING_TITLE: &str = "Loading...";
const FAILED_TITLE: &str = "Failed to load replay";
const SIDE_PANEL_WIDTH: Pixels = px(360.);

/// Which entity (if any) occupies the panel's single side-panel slot. Chat
/// and the two debug-mode raw viewers share the slot rather than each having
/// their own, since only one is useful to look at at a time and the egui app
/// itself only ever shows one debug viewer window at once.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SidePanel {
    None,
    Chat,
    RawMetadata,
    RawResults,
}

/// A loaded replay's tab title and outcome, plus the real player table, the
/// chat panel (`None` for a chat-less replay so its toggle button can be
/// disabled rather than opening onto an empty panel, matching the egui app),
/// and the two debug-mode raw-JSON viewers (`raw_results_panel` is `None`
/// when the replay carries no battle-results packet; see
/// `load::ParsedReplay::raw_results_json`).
struct LoadedReplay {
    title: SharedString,
    battle_result: Option<BattleResult>,
    table: Entity<PlayerTable>,
    chat_panel: Option<Entity<ChatPanel>>,
    raw_metadata_panel: Entity<RawJsonPanel>,
    raw_results_panel: Option<Entity<RawJsonPanel>>,
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
    _parse_task: Task<()>,
}

impl ReplayPanel {
    pub fn new(path: PathBuf, game_data: GameDataCache, debug: bool, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let parse_task = spawn_parse(path, game_data, cx);
        let parse_task = cx.spawn(async move |this, cx| {
            let result = parse_task.await;
            let _ = this.update(cx, |this, cx| this.apply_result(result, cx));
        });

        Self { focus_handle, state: LoadState::Loading, side_panel: SidePanel::None, debug, _parse_task: parse_task }
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

    fn apply_result(&mut self, result: Result<ParsedReplay, ReplayLoadError>, cx: &mut Context<Self>) {
        self.state = match result {
            Ok(ParsedReplay { mut model, game_data, raw_metadata_json, raw_results_json }) => {
                let ship_name =
                    model.rows.iter().find(|row| row.is_self).map(|row| row.ship_name.clone()).unwrap_or_default();
                let title: SharedString =
                    if ship_name.is_empty() { model.map.clone() } else { format!("{ship_name} - {}", model.map) }
                        .into();
                let battle_result = model.battle_result;
                let chat = std::mem::take(&mut model.chat);
                let chat_panel = (!chat.is_empty()).then(|| cx.new(|cx| ChatPanel::new(chat, cx)));
                let vfs = game_data.vfs().clone();
                let table = cx.new(|cx| PlayerTable::new(model, vfs, self.debug, cx));
                let raw_metadata_panel = cx.new(|cx| RawJsonPanel::new(raw_metadata_json.into(), cx));
                let raw_results_panel = raw_results_json.map(|json| cx.new(|cx| RawJsonPanel::new(json.into(), cx)));
                LoadState::Loaded(LoadedReplay {
                    title,
                    battle_result,
                    table,
                    chat_panel,
                    raw_metadata_panel,
                    raw_results_panel,
                })
            }
            Err(err) => LoadState::Failed(err),
        };
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for ReplayPanel {}

impl Focusable for ReplayPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ReplayPanel {
    fn panel_name(&self) -> &'static str {
        "ReplayPanel"
    }

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

    let (icon, label, outcome) = match result {
        BattleResult::Win(_) => (IconName::ThumbsUp, "Victory", BattleOutcome::Win),
        BattleResult::Loss(_) => (IconName::ThumbsDown, "Defeat", BattleOutcome::Loss),
        BattleResult::Draw => (IconName::Minus, "Draw", BattleOutcome::Draw),
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
        .child(Icon::new(icon))
        .child(label)
        .into_any_element()
}

/// One side-panel toggle button's static shape, bundled into a struct so
/// `side_panel_button` stays under clippy's argument-count limit (mirrors
/// `table.rs::RowLayout`'s reason for existing).
struct SidePanelButtonSpec {
    id: &'static str,
    icon: IconName,
    label: &'static str,
    panel: SidePanel,
    enabled: bool,
    disabled_tooltip: Option<&'static str>,
}

/// One side-panel toggle button: selected while `spec.panel` is the active
/// `SidePanel`, disabled when `spec.enabled` is false (with
/// `spec.disabled_tooltip` shown on hover), clicking toggles between
/// `spec.panel` and `SidePanel::None` via `ReplayPanel::toggle_side_panel`.
fn side_panel_button(spec: SidePanelButtonSpec, current: SidePanel, cx: &mut Context<ReplayPanel>) -> AnyElement {
    let panel = spec.panel;
    Button::new(spec.id)
        .icon(spec.icon)
        .label(spec.label)
        .compact()
        .selected(current == panel)
        .disabled(!spec.enabled)
        .when_some((!spec.enabled).then_some(spec.disabled_tooltip).flatten(), |this, tooltip| this.tooltip(tooltip))
        .on_click(cx.listener(move |this, _event, _window, cx| this.toggle_side_panel(panel, cx)))
        .into_any_element()
}

/// The header row: the outcome badge on the left, the chat toggle and (when
/// `debug` is on) the raw-metadata/raw-results debug viewer toggles on the
/// right. Mirrors the egui app's Row 1 layout plus its debug-mode buttons
/// (see the module doc); `has_chat`/`has_results` disable their respective
/// buttons exactly like `ui.add_enabled(...)` there, since a chat-less or
/// results-less replay has nothing to show.
fn header_row(
    battle_result: Option<BattleResult>,
    has_chat: bool,
    has_results: bool,
    debug: bool,
    side_panel: SidePanel,
    cx: &mut Context<ReplayPanel>,
) -> AnyElement {
    let chat_button = side_panel_button(
        SidePanelButtonSpec {
            id: "replay-chat-toggle",
            icon: IconName::PanelRight,
            label: "Chat",
            panel: SidePanel::Chat,
            enabled: has_chat,
            disabled_tooltip: Some("No chat messages were sent in this replay"),
        },
        side_panel,
        cx,
    );

    let mut buttons = h_flex().flex_none().items_center().gap_1().child(chat_button);
    if debug {
        buttons = buttons
            .child(side_panel_button(
                SidePanelButtonSpec {
                    id: "replay-debug-raw-metadata",
                    icon: IconName::File,
                    label: "Raw Metadata",
                    panel: SidePanel::RawMetadata,
                    enabled: true,
                    disabled_tooltip: None,
                },
                side_panel,
                cx,
            ))
            .child(side_panel_button(
                SidePanelButtonSpec {
                    id: "replay-debug-raw-results",
                    icon: IconName::File,
                    label: "Raw Results",
                    panel: SidePanel::RawResults,
                    enabled: has_results,
                    disabled_tooltip: Some("This replay has no battle-results packet"),
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
        .child(outcome_badge(battle_result))
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
}

impl Render for ReplayPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            LoadState::Loading => div().p_2().text_sm().opacity(0.6).child(LOADING_TITLE).into_any_element(),
            LoadState::Failed(err) => v_flex()
                .p_2()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(FAILED_TITLE))
                .child(div().text_sm().opacity(0.6).child(err.to_string()))
                .into_any_element(),
            LoadState::Loaded(loaded) => {
                let has_chat = loaded.chat_panel.is_some();
                let has_results = loaded.raw_results_panel.is_some();
                let table = loaded.table.clone();
                let battle_result = loaded.battle_result;
                let border = cx.theme().border;

                let side_panel_entity: Option<AnyView> = match self.side_panel {
                    SidePanel::None => None,
                    SidePanel::Chat => loaded.chat_panel.clone().map(|panel| panel.into()),
                    SidePanel::RawMetadata => Some(loaded.raw_metadata_panel.clone().into()),
                    SidePanel::RawResults => loaded.raw_results_panel.clone().map(|panel| panel.into()),
                };

                v_flex()
                    .size_full()
                    .child(header_row(battle_result, has_chat, has_results, self.debug, self.side_panel, cx))
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

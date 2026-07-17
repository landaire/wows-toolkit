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

use std::path::PathBuf;

use gpui::*;
use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::dock::Panel;
use gpui_component::dock::PanelEvent;
use gpui_component::h_flex;
use gpui_component::v_flex;
use wows_replays::analyzer::battle_controller::BattleResult;

use super::columns::BattleOutcome;
use super::columns::ColorRole;
use super::load::GameDataCache;
use super::load::ReplayLoadError;
use super::load::spawn_parse;
use super::model::ReplayReportModel;
use super::table::PlayerTable;
use super::table::resolve_color;

const LOADING_TITLE: &str = "Loading...";
const FAILED_TITLE: &str = "Failed to load replay";

/// A loaded replay's tab title and outcome, plus the real player table.
/// Split out of `LoadState` so `title()` (called on every render by the dock)
/// does not need to re-derive the title string from the model each time.
struct LoadedReplay {
    title: SharedString,
    battle_result: Option<BattleResult>,
    table: Entity<PlayerTable>,
}

enum LoadState {
    Loading,
    Loaded(LoadedReplay),
    Failed(ReplayLoadError),
}

pub struct ReplayPanel {
    focus_handle: FocusHandle,
    state: LoadState,
    _parse_task: Task<()>,
}

impl ReplayPanel {
    pub fn new(path: PathBuf, game_data: GameDataCache, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let parse_task = spawn_parse(path, game_data, cx);
        let parse_task = cx.spawn(async move |this, cx| {
            let result = parse_task.await;
            let _ = this.update(cx, |this, cx| this.apply_result(result, cx));
        });

        Self { focus_handle, state: LoadState::Loading, _parse_task: parse_task }
    }

    fn apply_result(&mut self, result: Result<ReplayReportModel, ReplayLoadError>, cx: &mut Context<Self>) {
        self.state = match result {
            Ok(model) => {
                let ship_name =
                    model.rows.iter().find(|row| row.is_self).map(|row| row.ship_name.clone()).unwrap_or_default();
                let title: SharedString =
                    if ship_name.is_empty() { model.map.clone() } else { format!("{ship_name} - {}", model.map) }
                        .into();
                let battle_result = model.battle_result;
                let table = cx.new(|cx| PlayerTable::new(model, cx));
                LoadState::Loaded(LoadedReplay { title, battle_result, table })
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

/// The outcome row: Win/Loss/Draw, colored and labeled exactly like the egui
/// app's Row 1 (see the module doc). Renders nothing when the replay carries
/// no battle result yet, matching the egui app's `if let Some(battle_result)`
/// gate (an incomplete-results replay, e.g. one still in progress when saved).
fn outcome_row(battle_result: Option<BattleResult>) -> AnyElement {
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

impl Render for ReplayPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            LoadState::Loading => div().p_2().text_sm().opacity(0.6).child(LOADING_TITLE).into_any_element(),
            LoadState::Failed(err) => v_flex()
                .p_2()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(FAILED_TITLE))
                .child(div().text_sm().opacity(0.6).child(err.to_string()))
                .into_any_element(),
            LoadState::Loaded(loaded) => v_flex()
                .size_full()
                .child(outcome_row(loaded.battle_result))
                .child(div().flex_1().min_h(px(0.)).child(loaded.table.clone()))
                .into_any_element(),
        };

        v_flex().id("replay-panel").track_focus(&self.focus_handle).size_full().child(body)
    }
}

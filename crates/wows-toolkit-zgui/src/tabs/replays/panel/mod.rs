//! One open replay: its header (the outcome, the match, the recording player's figures counted up
//! once, the team damage), the player table its owner supplies, and a side panel that slides in
//! with the chat or, in debug mode, the raw JSON payloads.
//!
//! The panel reads its replay with `replay_data::load::parse_replay_with_alts` off the UI thread,
//! with a job on the status bar, and again whenever its [`PanelControl`] asks or another recording
//! of the battle is added.
//!
//! # The table slot
//!
//! The panel does not draw the player table. Its owner passes a [`TableSlot`], built once per
//! loaded battle with a [`PanelTable`]: the model (a signal, replaced when the recording player's
//! figures are hidden or a rating table arrives), the debug flag, the [`PlayerDetail`] that draws
//! a row's expanded content, and the callback that shows a row's raw metadata in the side panel.
//! [`player_table_slot`](crate::tabs::replays::player_table_slot) is the slot that draws the
//! replay's [`PlayerTable`](crate::tabs::replays::table::PlayerTable).

pub mod chat;
pub mod expanded;
mod figures;
mod raw_json;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::FadeInProps;
use landaire_ui::LoadingLineProps;
use landaire_ui::TableSkeletonProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::load::ParsedReplay;
use wows_toolkit_replay_data::load::parse_replay_with_alts;
use wows_toolkit_replay_data::model::ChatMessage;
use wows_toolkit_replay_data::model::MatchContext;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::ReplayReportModel;
use wows_toolkit_replay_data::model::battle_title;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::FlattenedVehicle;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wowsunpack::game_params::types::Species;
use zgui::component;
use zgui::css;
use zgui::reactive::ArcRwSignal;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateUntracked;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuCheckboxItemProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuLabelProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Job;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::ModeHint;
use crate::tabs::replays::panel::chat::ChatPanelProps;
use crate::tabs::replays::panel::expanded::ColumnDetailProps;
use crate::tabs::replays::panel::expanded::SHARE_MODE;
use crate::tabs::replays::panel::expanded::ShareRows;
use crate::tabs::replays::panel::expanded::has_column_detail;
use crate::tabs::replays::panel::figures::DamageSplitProps;
use crate::tabs::replays::panel::figures::OwnFigures;
use crate::tabs::replays::panel::figures::StatTilesProps;
use crate::tabs::replays::panel::raw_json::RawJsonPanelProps;
use crate::tabs::replays::table::PlayerDetail;
use crate::tabs::replays::table::PlayerDetailCx;

const SHEET: &str = "wt-replay-panel";

const CSS: &str = css!(
    ".wt-rp {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .wt-rp-loaded { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-rp-failed {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px;
        font-size: 14px;
    }
    .wt-rp-failed__title { font-weight: 700; }
    .wt-rp-dim { color: var(--wt-color-text-dim); }
    .wt-rp-win { color: var(--wt-color-win); }
    .wt-rp-loss { color: var(--wt-color-loss); }
    .wt-rp-band { display: flex; flex-direction: column; flex: none; }
    .wt-rp-band[data-result=\"win\"] {
        background: linear-gradient(90deg, color-mix(in srgb, var(--wt-color-win) 12%, transparent), transparent);
    }
    .wt-rp-band[data-result=\"loss\"] {
        background: linear-gradient(90deg, color-mix(in srgb, var(--wt-color-loss) 12%, transparent), transparent);
    }
    .wt-rp-band[data-result=\"draw\"] {
        background: linear-gradient(90deg, color-mix(in srgb, var(--wt-color-draw) 12%, transparent), transparent);
    }
    .wt-rp-header {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 4px;
        padding-right: 8px;
    }
    .wt-rp-header__start, .wt-rp-header__end {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
    }
    .wt-rp-outcome {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        font-weight: 700;
    }
    .wt-rp-outcome[data-result=\"win\"] { color: var(--wt-color-win); }
    .wt-rp-outcome[data-result=\"loss\"] { color: var(--wt-color-loss); }
    .wt-rp-outcome[data-result=\"draw\"] { color: var(--wt-color-draw); }
    .wt-rp-incomplete {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        padding-left: 8px;
        font-size: 14px;
        font-weight: 700;
        color: var(--wt-color-warn);
    }
    .wt-rp-status { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-rp-chat-toggle[data-selected] { background-color: var(--lui-color-selection); }
    .zui-menu__label.wt-rp-menu-heading { color: var(--wt-color-text-dim); font-weight: 400; }
    .wt-rp-context {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 0 8px 4px 8px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
    }
    .wt-rp-context__item { display: flex; flex-direction: row; flex: none; align-items: center; gap: 4px; }
    .wt-rp-figures {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 0 8px 4px 8px;
    }
    .wt-rp-tile {
        display: flex;
        flex-direction: column;
        flex: none;
        min-width: 64px;
        padding: 2px 8px;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
        background-color: var(--lui-color-card);
    }
    .wt-rp-tile__label { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-rp-tile__value { font-size: 14px; font-weight: 700; font-variant-numeric: tabular-nums; }
    .wt-rp-split {
        display: flex;
        flex-direction: row;
        align-items: center;
        min-width: 0;
        gap: 4px;
        font-size: 12px;
        font-variant-numeric: tabular-nums;
    }
    .wt-rp-split__bar {
        display: flex;
        flex-direction: row;
        min-width: 40px;
        width: 160px;
        height: 6px;
        overflow: hidden;
        border: 1px solid var(--lui-color-border);
        border-radius: 2px;
    }
    .wt-rp-split__allies { height: 100%; background-color: var(--wt-color-win); }
    .wt-rp-split__enemies { height: 100%; background-color: var(--wt-color-loss); }
    .wt-rp-body { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-rp-table { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-rp-side { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-rp-side__head {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-rp-side__title {
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 14px;
        font-weight: 700;
    }
    .wt-rp-chat { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-rp-chat__toolbar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-rp-chat__log { flex: 1 1 auto; min-height: 0; }
    .wt-rp-chat__row {
        position: relative;
        display: flex;
        flex-direction: row;
        padding: 6px 8px;
        border-bottom: 1px solid var(--lui-color-border);
        font-size: 14px;
    }
    .wt-rp-chat__text { display: flex; flex-direction: column; gap: 2px; padding-right: 24px; min-width: 0; }
    .wt-rp-chat__who { display: flex; flex-direction: row; gap: 4px; }
    .wt-rp-chat__clan { color: var(--wt-clan-light); }
    @media (prefers-color-scheme: dark) {
        .wt-rp-chat__clan { color: var(--wt-clan-dark); }
    }
    .wt-rp-chat__body { white-space: pre-wrap; }
    .wt-rp-chat__copy { position: absolute; top: 4px; right: 4px; visibility: hidden; }
    .wt-rp-chat__row:hover > .wt-rp-chat__copy { visibility: visible; }"
);

/// How wide the side panel opens, and how far it may be dragged.
const SIDE_PANEL_WIDTH: landaire_ui::PanelWidth = landaire_ui::PanelWidth(360.0);
const SIDE_PANEL_RANGE: landaire_ui::WidthRange =
    landaire_ui::WidthRange { min: landaire_ui::PanelWidth(240.0), max: landaire_ui::PanelWidth(720.0) };

/// How many other recordings one tab reads alongside its own. Each is a decompressed packet
/// stream held while the tab is open; a full enemy team is eleven.
const MAX_ALTS: usize = 11;

/// What the side panel shows. Chat and the debug viewers share it, since only one is useful at a
/// time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePanel {
    None,
    Chat,
    RawMetadata,
    RawResults,
    /// The results with their positional arrays resolved to named fields.
    MappedResults,
    /// One row's metadata, opened from that row's menu.
    RawPlayerMetadata,
}

impl SidePanel {
    const fn title_key(self) -> Option<&'static str> {
        match self {
            SidePanel::None => None,
            SidePanel::Chat => Some("ui.replay.game_chat"),
            SidePanel::RawMetadata => Some("ui.replay.debug.raw_metadata"),
            SidePanel::RawResults => Some("ui.replay.debug.raw_results"),
            SidePanel::MappedResults => Some("ui.replay.debug.mapped_results"),
            SidePanel::RawPlayerMetadata => Some("ui.replay.debug.player_metadata"),
        }
    }

    const fn debug_only(self) -> bool {
        matches!(self, SidePanel::RawMetadata | SidePanel::RawResults | SidePanel::MappedResults)
    }
}

/// Why a tab reads its replay again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReparseCause {
    /// The file on disk changed, which is how the results of a battle the client left early
    /// arrive.
    FileRewritten,
    /// A mapping or game data the battle is read through has arrived.
    GameDataArrived,
    /// A recording of the same battle was added or taken back out.
    AltPerspective,
}

impl ReparseCause {
    /// The cause of one read that answers both requests. A file change outranks the others,
    /// since only it is announced.
    fn with(self, other: Self) -> Self {
        if other == Self::FileRewritten { other } else { self }
    }
}

/// What the panel's owner drives it through, and what the panel read. `Copy`; make one per panel
/// in a scope that outlives the panel's view, so a view the dock builds again shows the battle
/// without reading the file again.
#[derive(Clone, Copy)]
pub struct PanelControl {
    reparse: RwSignal<Option<(u64, ReparseCause)>, LocalStorage>,
    state: RwSignal<LoadState, LocalStorage>,
    /// The other recordings the battle is read through.
    alts: RwSignal<Vec<PathBuf>, LocalStorage>,
    /// The recording just added, on trial in the next read and only that one.
    alt_on_trial: StoredValue<Option<PathBuf>>,
    /// Whether the header's figures have counted up.
    counted: StoredValue<bool>,
    /// Why a read cut short with its view was asked for, owed by the next view.
    owed: StoredValue<Option<ReparseCause>>,
    /// The open rows that list damage shares, which Alt flips.
    share_rows: ShareRows,
}

impl PanelControl {
    pub fn new() -> Self {
        Self {
            reparse: RwSignal::new_local(None),
            state: RwSignal::new_local(LoadState::Loading),
            alts: RwSignal::new_local(Vec::new()),
            alt_on_trial: StoredValue::new(None),
            counted: StoredValue::new(false),
            owed: StoredValue::new(None),
            share_rows: ShareRows::new(),
        }
    }

    /// Reads the replay again.
    pub fn reparse(self, cause: ReparseCause) {
        self.reparse.update(|asked| {
            let next = asked.map_or(1, |(count, _)| count + 1);
            *asked = Some((next, cause));
        });
    }

    /// Lets go of the battle read, as the panel closes for good.
    pub fn release(self) {
        // Untracked: a view still up is going away this turn, and is not built again for it.
        self.state.try_update_untracked(|state| *state = LoadState::Loading);
        self.alts.try_update_untracked(Vec::clear);
    }

    /// Hands a read the view could not finish to the next view, with the recording it had on
    /// trial. Called as a view is disposed of, when the control may be going too.
    fn owe(self, cause: Option<ReparseCause>, trial: Option<PathBuf>) {
        if trial.is_some() {
            self.alt_on_trial.try_set_value(trial);
        }
        if let Some(cause) = cause {
            self.owed.try_update_value(|owed| *owed = Some(owed.map_or(cause, |owed| owed.with(cause))));
        }
    }

    /// Why the battle failed to read, when it did.
    #[cfg(test)]
    pub fn failure(self) -> Option<String> {
        self.state.with_untracked(|state| match state {
            LoadState::Failed(reason) => Some(reason.clone()),
            LoadState::Loading | LoadState::Loaded(_) => None,
        })
    }

    /// What the panel's tab says about its battle. Reactive.
    pub fn title(self) -> PanelTitle {
        self.state.with(|state| match state {
            LoadState::Loading => PanelTitle::Reading,
            LoadState::Loaded(loaded) => PanelTitle::Read { title: loaded.title.clone(), ship: loaded.ship.clone() },
            LoadState::Failed(_) => PanelTitle::Failed,
        })
    }

    /// The held-key mode the panel's open rows offer. Reactive.
    pub fn mode(self) -> Option<ModeHint> {
        self.share_rows.any().then_some(SHARE_MODE)
    }

    /// Marks `player`'s open row as listing damage shares, as its detail does once drawn.
    #[cfg(test)]
    pub fn note_shares_for_test(self, player: wows_replays::types::AccountId) {
        self.share_rows.mark(player, wows_toolkit_replay_data::columns::ReplayColumn::ActualDamage, true);
    }
}

impl Default for PanelControl {
    fn default() -> Self {
        Self::new()
    }
}

/// A panel asked for its battle to be played back, with the other recordings it reads it through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRequest {
    pub path: PathBuf,
    pub alts: Vec<PathBuf>,
}

/// A panel read a battle whose result mapping does not belong to its build; the owner can fetch
/// one and then [`PanelControl::reparse`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstantsMismatched {
    pub build: u32,
    pub version: Option<String>,
}

/// What a panel's tab says about its battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelTitle {
    Reading,
    /// "{ship} - {map}", with the recording player's ship when the battle names it.
    Read {
        title: String,
        ship: Option<TitleShip>,
    },
    Failed,
}

/// The recording player's ship, as its tab marks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleShip {
    /// The nation's name in the game params.
    pub nation: Option<String>,
    pub class: Species,
}

/// What the table slot is built with.
#[derive(Clone)]
pub struct PanelTable {
    pub model: Signal<Rc<ReplayReportModel>, LocalStorage>,
    pub debug: Signal<bool, LocalStorage>,
    pub detail: PlayerDetail,
    /// Shows a row's raw metadata JSON in the side panel.
    pub on_view_raw_json: UnsyncCallback<String>,
}

/// Builds the player table for a loaded battle.
pub type TableSlot = Rc<dyn Fn(PanelTable) -> AnyView>;

/// What the panel tells its owner.
#[derive(Clone, Default)]
pub struct PanelEvents {
    /// The Actions menu's "Render replay". The item is left out without it.
    pub on_render: Option<UnsyncCallback<RenderRequest>>,
    pub on_constants_mismatched: Option<UnsyncCallback<ConstantsMismatched>>,
}

/// A read battle and what its panels show. Owned by no scope, so it outlives the view that read it.
struct Loaded {
    title: String,
    /// The recording player's ship, for the tab.
    ship: Option<TitleShip>,
    battle_result: Option<BattleResult>,
    model: ArcRwSignal<Rc<ReplayReportModel>>,
    chat: Option<Rc<[ChatMessage]>>,
    raw_metadata: Rc<str>,
    raw_results: Option<Rc<str>>,
    mapped_results: Option<Rc<str>>,
    own_figures: Option<OwnFigures>,
    export: ExportedMatch,
    export_stem: String,
}

#[derive(Clone)]
enum LoadState {
    Loading,
    Loaded(Rc<Loaded>),
    Failed(String),
}

impl LoadState {
    fn loaded(&self) -> Option<Rc<Loaded>> {
        match self {
            LoadState::Loaded(loaded) => Some(Rc::clone(loaded)),
            _ => None,
        }
    }
}

/// A read under way, as the next view asks for it again when this one goes before it lands.
#[derive(Clone)]
struct Underway {
    cause: Option<ReparseCause>,
    trial: Option<PathBuf>,
}

/// Which branch of [`LoadState`] is up, so a re-read that lands does not rebuild the view twice.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StateKind {
    Loading,
    Loaded,
    Failed,
}

/// The model with `edit` applied. The model is shared with the table, so a change is a new one.
fn edited_model(model: &ReplayReportModel, edit: impl FnOnce(&mut ReplayReportModel)) -> Rc<ReplayReportModel> {
    let mut copy = ReplayReportModel {
        self_team: model.self_team,
        rows: model.rows.clone(),
        battle_result: model.battle_result,
        columns: model.columns.clone(),
        map: model.map.clone(),
        chat: model.chat.clone(),
        timestamp: model.timestamp,
        context: model.context.clone(),
    };
    edit(&mut copy);
    Rc::new(copy)
}

/// What a panel opens: the replay, the columns it starts with, and the rating table if one is
/// loaded.
#[derive(Clone)]
pub struct PanelSetup {
    pub path: PathBuf,
    pub columns: Vec<ReplayColumn>,
    pub personal_rating: Option<Arc<PersonalRatingData>>,
}

/// One open replay.
#[component]
pub fn ReplayPanel(
    setup: PanelSetup,
    table: TableSlot,
    #[prop(optional)] control: Option<PanelControl>,
    #[prop(optional)] events: PanelEvents,
    /// The visible columns, when the owner changes them after the panel opens.
    #[prop(into, optional)]
    columns: Option<Signal<Vec<ReplayColumn>, LocalStorage>>,
    /// The rating table, when it arrives after the panel opens.
    #[prop(into, optional)]
    personal_rating: Option<Signal<Option<Arc<PersonalRatingData>>, LocalStorage>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let shell = Shell::current();
    let control = control.unwrap_or_default();
    let PanelSetup { path, columns: initial_columns, personal_rating: initial_rating } = setup;

    let state = control.state;
    // A test hands the panel a battle rather than a replay to parse.
    #[cfg(test)]
    if let Some(loaded) = tests::SEED.with(|seed| seed.borrow_mut().take()) {
        state.set(LoadState::Loaded(Rc::new(loaded)));
    }
    let side = RwSignal::new_local(SidePanel::None);
    // What the slot shows, kept while it slides shut.
    let shown_side = RwSignal::new_local(SidePanel::None);
    let player_metadata: RwSignal<Option<Rc<str>>, LocalStorage> = RwSignal::new_local(None);
    let side_width = RwSignal::new_local(SIDE_PANEL_WIDTH);
    let alts = control.alts;
    let parsing = RwSignal::new_local(false);
    // A read asked for while another runs, folded into one follow-up read.
    let queued: StoredValue<Option<ReparseCause>> = StoredValue::new(None);
    // The read under way, handed to the next view if this one goes first.
    let underway: StoredValue<Option<Underway>> = StoredValue::new(None);
    // Work started from a menu or an effect runs under the panel, so it outlives them.
    let owner = StoredValue::new_local(Owner::current());
    let export_status: RwSignal<Option<String>, LocalStorage> = RwSignal::new_local(None);
    let debug = Signal::derive_local(move || shell.debug_mode.get());
    let path = StoredValue::new(path);
    let columns_now = StoredValue::new(initial_columns);
    let rating_now = StoredValue::new(initial_rating);
    let events = StoredValue::new_local(events);

    // A read cut short with this view, and any asked for behind it, fall to the next view.
    on_cleanup_local(move || {
        let Some(cut) = underway.try_get_value().flatten() else { return };
        control.owe(cut.cause, cut.trial);
        control.owe(queued.try_get_value().flatten(), None);
    });

    // A debug viewer whose gate closes goes with it, so it is not left open with no way back.
    follow(move |_| {
        if !debug.get() && side.get_untracked().debug_only() {
            side.set(SidePanel::None);
        }
    });
    follow(move |_| {
        let now = side.get();
        if now != SidePanel::None {
            shown_side.set(now);
        }
    });

    if let Some(columns) = columns {
        follow(move |_| {
            let columns = columns.get();
            columns_now.set_value(columns.clone());
            if let Some(loaded) = state.with_untracked(LoadState::loaded) {
                loaded.model.update(|model| *model = edited_model(model, |model| model.columns = columns));
            }
        });
    }
    if let Some(personal_rating) = personal_rating {
        follow(move |_| {
            let Some(table) = personal_rating.get() else { return };
            rating_now.set_value(Some(Arc::clone(&table)));
            if let Some(loaded) = state.with_untracked(LoadState::loaded) {
                loaded
                    .model
                    .update(|model| *model = edited_model(model, |model| model.populate_personal_ratings(&table)));
            }
        });
    }

    let read = move |cause: Option<ReparseCause>| {
        if parsing.get_untracked() {
            if let Some(cause) = cause {
                queued.update_value(|queued| *queued = Some(queued.map_or(cause, |queued| queued.with(cause))));
            }
            return;
        }
        let Some(cache) = services.game_data.cache_untracked() else { return };
        let primary = path.get_value();
        let alts_now = alts.get_untracked();
        let rating = rating_now.get_value();
        // The recording just added is on trial in this read, and only this one.
        let trial = control.alt_on_trial.get_value();
        control.alt_on_trial.set_value(None);
        underway.set_value(Some(Underway { cause, trial: trial.clone() }));
        parsing.set(true);
        spawn_in(owner.get_value(), async move {
            let _job = services.jobs.start(Job { name: t!("ui.replay.loading").into_owned(), progress: None });
            let result = blocking(move || parse_replay_with_alts(&primary, &alts_now, &cache, rating.as_deref())).await;
            underway.set_value(None);
            parsing.set(false);
            apply_result(ApplyCx { state, alts, trial, columns_now, rating_now, events, shell, cause }, result);
            if let Some(next) = queued.get_value() {
                queued.set_value(None);
                control.reparse(next);
            }
        });
    };

    // The first read waits for the game data; later ones come from the control. A view the dock
    // builds again shows what was read, unless the view before it was cut off mid-read.
    let first_read = StoredValue::new(false);
    follow(move |_| {
        if services.game_data.cache().is_some() && !first_read.get_value() {
            first_read.set_value(true);
            let owed = control.owed.get_value();
            control.owed.set_value(None);
            let unread = state.with_untracked(|state| matches!(state, LoadState::Loading));
            if unread || owed.is_some() {
                read(owed);
            }
        }
    });
    follow(move |previous: Option<()>| {
        let asked = control.reparse.get();
        if previous.is_some()
            && let Some((_, cause)) = asked
        {
            read(Some(cause));
        }
    });

    let add_alt = move || {
        if alts.with_untracked(Vec::len) >= MAX_ALTS {
            shell.toasts().warn(t!("ui.replay.load_alt_perspective_limit", limit = MAX_ALTS).into_owned());
            return;
        }
        let Some(cache) = services.game_data.cache_untracked() else { return };
        let asked = wows_toolkit_services::dialog::pick_file(
            Some(&t!("ui.replay.load_alt_perspective")),
            Some(wows_toolkit_services::dialog::REPLAYS),
        );
        let primary = path.get_value();
        spawn_in(owner.get_value(), async move {
            let alt = match asked.await {
                Ok(Some(alt)) => alt,
                Ok(None) => return,
                Err(err) => {
                    shell.toasts().failed(t!("ui.dialogs.file_dialog_failed", reason = err).into_owned());
                    return;
                }
            };
            let checked = alt.clone();
            let check = blocking(move || wows_toolkit_replay_data::load::check_alt(&primary, &checked, &cache)).await;
            match check {
                Ok(()) => {
                    if alts.with_untracked(|kept| kept.iter().any(|kept| same_file(kept, &alt))) {
                        return;
                    }
                    control.alt_on_trial.set_value(Some(alt.clone()));
                    alts.update(|kept| kept.push(alt));
                    control.reparse(ReparseCause::AltPerspective);
                }
                Err(refused) => {
                    shell
                        .toasts()
                        .failed(t!("ui.replay.load_alt_perspective_failed", error = refused.to_string()).into_owned());
                }
            }
        });
    };

    let add_alt: Rc<dyn Fn()> = Rc::new(add_alt);
    let on_view_raw_json = UnsyncCallback::new(move |json: String| {
        player_metadata.set(Some(Rc::from(json)));
        side.set(SidePanel::RawPlayerMetadata);
    });
    let table = StoredValue::new_local(table);
    let kind = zgui::reactive::Memo::new(move |_| match state.get() {
        LoadState::Loading => StateKind::Loading,
        LoadState::Loaded(_) => StateKind::Loaded,
        LoadState::Failed(_) => StateKind::Failed,
    });

    let body = move || match kind.get() {
        StateKind::Loading => AnyView::new(view! {
            TableSkeleton(label = t!("ui.replay.loading").into_owned(), attr:data-testid = "replay-panel-loading")
        }),
        StateKind::Failed => {
            let reason = move || {
                state.with(|state| match state {
                    LoadState::Failed(reason) => reason.clone(),
                    _ => String::new(),
                })
            };
            AnyView::new(view! {
                column(class = "wt-rp-failed", attr:data-testid = "replay-panel-failed") {
                    text(class = "wt-rp-failed__title") {{t!("ui.messages.replay_load_failed").into_owned()}}
                    text(class = "wt-rp-dim") {{reason}}
                }
            })
        }
        StateKind::Loaded => {
            // The loaded view is built per battle read, from the latest one.
            let loaded = Signal::derive_local(move || state.get().loaded());
            let run = !control.counted.get_value();
            control.counted.set_value(true);
            let header = HeaderCx {
                loaded,
                side,
                debug,
                parsing,
                export_status,
                alts,
                path,
                events,
                add_alt: Rc::clone(&add_alt),
                export: ExportCx { owner: owner.get_value(), shell },
            };
            let table_view = move || {
                let loaded = loaded.get()?;
                let read = ArcRwSignal::clone(&loaded.model);
                let model: Signal<Rc<ReplayReportModel>, LocalStorage> = Signal::derive_local(move || read.get());
                let alt_held = Signal::derive_local(move || shell.held_modifiers().alt());
                let detail: PlayerDetail = Rc::new(move |cx: PlayerDetailCx| {
                    let PlayerDetailCx { row, rows, column, art } = cx;
                    if !has_column_detail(column, &row, &rows, debug.get()) {
                        return None;
                    }
                    Some(AnyView::new(view! {
                        ColumnDetail(
                            column = column,
                            row = row,
                            rows = rows,
                            debug = debug,
                            alt_held = alt_held,
                            art = art,
                            share_rows = control.share_rows
                        )
                    }))
                });
                Some(table.with_value(|table| table(PanelTable { model, debug, detail, on_view_raw_json })))
            };
            AnyView::new(view! {
                FadeIn(class = "wt-rp-loaded") {
                    HeaderBand(header = header.clone(), run_count = run)
                    landaire_ui::SideSplit(
                        open = Signal::derive_local(move || side.get() != SidePanel::None),
                        width = side_width,
                        range = SIDE_PANEL_RANGE,
                        edge = landaire_ui::SideEdge::Right,
                        resize_label = t!("ui.replay.game_chat").into_owned(),
                        class = "wt-rp-body"
                    ) {
                        landaire_ui::SidePanel(slot) {
                            SideContent(kind = shown_side, side = side, loaded = loaded, player_metadata = player_metadata)
                        }
                        box(class = "wt-rp-table", attr:data-testid = "replay-table-slot") {{table_view}}
                    }
                }
            })
        }
    };

    view! {
        column(class = "wt-rp", attr:data-testid = "replay-panel") {{body}}
    }
}

/// Whether two paths name the same recording, through the filesystem's answer where it has one.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// What a landed read is folded in with.
struct ApplyCx {
    state: RwSignal<LoadState, LocalStorage>,
    alts: RwSignal<Vec<PathBuf>, LocalStorage>,
    /// The recording added for this read, dropped again if the read fails.
    trial: Option<PathBuf>,
    columns_now: StoredValue<Vec<ReplayColumn>>,
    rating_now: StoredValue<Option<Arc<PersonalRatingData>>>,
    events: StoredValue<PanelEvents, LocalStorage>,
    shell: Shell,
    cause: Option<ReparseCause>,
}

fn apply_result(cx: ApplyCx, result: Result<ParsedReplay, ReplayLoadError>) {
    let ApplyCx { state, alts, trial, columns_now, rating_now, events, shell, cause } = cx;
    // A battle the client left early gets its results when the game rewrites the file; that
    // re-read is worth saying.
    let awaited_results = cause == Some(ReparseCause::FileRewritten)
        && state.with_untracked(|state| matches!(state, LoadState::Loaded(loaded) if loaded.raw_results.is_none()));

    match result {
        Ok(parsed) => {
            // The recording on trial merged, so it stays.
            if parsed.indexable.constants_fit == wows_toolkit_viewmodel::index_rows::ConstantsFit::Mismatched
                && let Some(build) = parsed.indexable.version_build
                && let Some(on_mismatch) = events.with_value(|events| events.on_constants_mismatched)
            {
                on_mismatch.run(ConstantsMismatched { build, version: Some(parsed.model.context.version.clone()) });
            }
            let loaded = loaded_state(parsed, columns_now.get_value(), rating_now.get_value());
            if awaited_results && loaded.raw_results.is_some() {
                shell.toasts().info(t!("ui.replay.results_arrived", battle = loaded.title.clone()).into_owned());
            }
            state.set(LoadState::Loaded(Rc::new(loaded)));
        }
        Err(err) => {
            // A read that failed only because of the recording just added drops it. The battle
            // on screen is the one read without it.
            if let Some(refused) = trial {
                alts.update(|kept| kept.retain(|kept| !same_file(kept, &refused)));
                shell
                    .toasts()
                    .failed(t!("ui.replay.load_alt_perspective_failed", error = err.to_string()).into_owned());
                return;
            }
            shell.toasts().failed(t!("ui.messages.replay_load_failed").into_owned());
            state.set(LoadState::Failed(err.to_string()));
        }
    }
}

/// Builds the loaded state from a parse.
fn loaded_state(parsed: ParsedReplay, columns: Vec<ReplayColumn>, rating: Option<Arc<PersonalRatingData>>) -> Loaded {
    let ParsedReplay {
        mut model,
        export,
        export_stem,
        raw_metadata_json,
        raw_results_json,
        mapped_results_json,
        fire_chance,
        ..
    } = parsed;
    model.columns = columns;
    model.set_fire_chance(fire_chance);
    // The table may have arrived while this replay was parsing.
    if let Some(rating) = rating.as_ref() {
        model.populate_personal_ratings(rating);
    }
    loaded_from_model(
        model,
        export,
        export_stem,
        DebugPayloads {
            raw_metadata: raw_metadata_json,
            raw_results: raw_results_json,
            mapped_results: mapped_results_json,
        },
    )
}

/// The debug viewers' payloads.
struct DebugPayloads {
    raw_metadata: String,
    raw_results: Option<String>,
    mapped_results: Option<String>,
}

fn loaded_from_model(
    mut model: ReplayReportModel,
    export: ExportedMatch,
    export_stem: String,
    payloads: DebugPayloads,
) -> Loaded {
    let own = model.rows.iter().find(|row| row.is_self);
    let title = battle_title(own.map(|row| row.ship_name.as_str()).unwrap_or_default(), &model.map);
    let ship = own.map(|row| TitleShip { nation: row.ship_nation.clone(), class: row.ship_class });
    let chat = std::mem::take(&mut model.chat);
    Loaded {
        title,
        ship,
        battle_result: model.battle_result,
        own_figures: OwnFigures::from_rows(&model.rows),
        model: ArcRwSignal::new(Rc::new(model)),
        chat: (!chat.is_empty()).then(|| Rc::from(chat)),
        raw_metadata: Rc::from(payloads.raw_metadata),
        raw_results: payloads.raw_results.map(Rc::from),
        mapped_results: payloads.mapped_results.map(Rc::from),
        export,
        export_stem,
    }
}

/// What the header band reads.
#[derive(Clone)]
struct HeaderCx {
    loaded: Signal<Option<Rc<Loaded>>, LocalStorage>,
    side: RwSignal<SidePanel, LocalStorage>,
    debug: Signal<bool, LocalStorage>,
    parsing: RwSignal<bool, LocalStorage>,
    export_status: RwSignal<Option<String>, LocalStorage>,
    alts: RwSignal<Vec<PathBuf>, LocalStorage>,
    path: StoredValue<PathBuf>,
    events: StoredValue<PanelEvents, LocalStorage>,
    add_alt: Rc<dyn Fn()>,
    export: ExportCx,
}

/// Where an export runs and reports.
#[derive(Clone)]
struct ExportCx {
    owner: Option<Owner>,
    shell: Shell,
}

fn result_name(result: Option<BattleResult>) -> Option<&'static str> {
    match result? {
        BattleResult::Win(_) => Some("win"),
        BattleResult::Loss(_) => Some("loss"),
        BattleResult::Draw => Some("draw"),
    }
}

/// The band over the table: the header row, the match line and the figures, tinted by the result.
#[component]
fn HeaderBand(header: HeaderCx, run_count: bool) -> impl IntoView {
    let loaded = header.loaded;
    // Tiles built again for a later read of the model show their figures as they are.
    let count_next = StoredValue::new(run_count);
    let result = move || {
        loaded.with(|loaded| loaded.as_ref().and_then(|loaded| result_name(loaded.battle_result)).map(str::to_owned))
    };
    let context =
        move || loaded.with(|loaded| loaded.as_ref().map(|loaded| loaded.model.with(|model| model.context.clone())));
    let figures = move || {
        let loaded = loaded.get()?;
        let model = loaded.model.get();
        let hidden = self_stats_hidden(&model.rows);
        // A test ship's own figures stay off the tiles while the table hides them.
        let own = loaded.own_figures.filter(|_| !hidden);
        let rating =
            model.rows.iter().find(|row| row.is_self).and_then(|row| row.personal_rating.clone()).filter(|_| !hidden);
        let (allies, enemies) = figures::team_damage(&model.rows);
        let counting = count_next.get_value();
        count_next.set_value(false);
        Some(AnyView::new(view! {
            StatTiles(own = own, rating = rating, counting = counting)
            DamageSplit(allies = allies, enemies = enemies)
        }))
    };

    view! {
        column(class = "wt-rp-band", attr:data-result = result) {
            HeaderRow(header = header)
            MatchContextSlot(context = Signal::derive_local(context))
            row(class = "wt-rp-figures") {{figures}}
        }
    }
}

fn self_stats_hidden(rows: &[PlayerRow]) -> bool {
    rows.iter().any(|row| row.is_self && row.manual_stat_hide_toggle)
}

fn self_is_test_ship(rows: &[PlayerRow]) -> bool {
    rows.iter().any(|row| row.is_self && row.is_test_ship)
}

/// The outcome on the left; the status, the Actions menu and the chat toggle on the right.
#[component]
fn HeaderRow(header: HeaderCx) -> impl IntoView {
    let HeaderCx { loaded, side, parsing, export_status, .. } = header.clone();
    let has_results = move || loaded.with(|loaded| loaded.as_ref().is_some_and(|loaded| loaded.raw_results.is_some()));
    let has_chat = move || loaded.with(|loaded| loaded.as_ref().is_some_and(|loaded| loaded.chat.is_some()));
    let outcome = move || loaded.with(|loaded| loaded.as_ref().and_then(|loaded| loaded.battle_result));
    let chat_label = t!("ui.replay.chat").into_owned();
    let tip = Signal::derive_local(move || {
        if has_chat() { t!("ui.replay.chat").into_owned() } else { t!("ui.replay.no_chat").into_owned() }
    });

    view! {
        row(class = "wt-rp-header") {
            row(class = "wt-rp-header__start") {
                Show(when = move || !has_results()) {
                    Tooltip {
                        TooltipTrigger {
                            row(class = "wt-rp-incomplete", attr:data-testid = "replay-incomplete-results") {
                                AppIcon(glyph = wows_toolkit_icons::Glyph::Info)
                                text {{t!("ui.replay.incomplete_results").into_owned()}}
                            }
                        }
                        TooltipContent {{t!("ui.replay.incomplete_results_tooltip").into_owned()}}
                    }
                }
                {move || outcome().map(|result| AnyView::new(view! { OutcomeBadge(result = result) }))}
            }
            row(class = "wt-rp-header__end") {
                Show(when = move || parsing.get()) {
                    LoadingLine(label = t!("ui.replay.loading").into_owned())
                }
                {move || export_status.get().map(|status| AnyView::new(view! { text(class = "wt-rp-status", attr:data-testid = "replay-export-status") {{status}} }))}
                ActionsMenu(header = header.clone())
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            class = "wt-rp-chat-toggle",
                            disabled = Signal::derive_local(move || !has_chat()),
                            attr:data-testid = "replay-chat-toggle",
                            attr:data-selected = move || (side.get() == SidePanel::Chat).then(String::new),
                            on:click = move |_| side.update(|side| *side = if *side == SidePanel::Chat { SidePanel::None } else { SidePanel::Chat })
                        ) {
                            AppIcon(glyph = wows_toolkit_icons::Glyph::PanelRight)
                            text {{chat_label}}
                        }
                    }
                    TooltipContent {{move || tip.get()}}
                }
            }
        }
    }
}

/// Win, Loss or Draw with its glyph, coloured as the egui header colours it.
#[component]
fn OutcomeBadge(result: BattleResult) -> impl IntoView {
    let (glyph, label) = match result {
        BattleResult::Win(_) => (wows_toolkit_icons::Glyph::Trophy, "Victory"),
        BattleResult::Loss(_) => (wows_toolkit_icons::Glyph::FaceSlightlyFrowning, "Defeat"),
        BattleResult::Draw => (wows_toolkit_icons::Glyph::Equal, "Draw"),
    };
    view! {
        row(class = "wt-rp-outcome", attr:data-result = result_name(Some(result)).map(str::to_owned), attr:data-testid = "replay-outcome") {
            AppIcon(glyph = glyph)
            text {{label}}
        }
    }
}

/// The match line, once a battle is read.
#[component]
fn MatchContextSlot(#[prop(into)] context: Signal<Option<MatchContext>, LocalStorage>) -> impl IntoView {
    move || context.get().map(|context| AnyView::new(view! { MatchContextLine(context = context) }))
}

/// Spawns `work` under `owner`, so it outlives the menu or effect that started it and ends with
/// the panel.
fn spawn_in(owner: Option<Owner>, work: impl Future<Output = ()> + 'static) {
    match owner {
        Some(owner) => owner.with(|| spawn_local(work)),
        None => spawn_local(work),
    };
}

/// Runs `effect` now and whenever what it reads changes, for as long as the calling scope lives.
fn follow<T: 'static>(effect: impl FnMut(Option<T>) -> T + 'static) {
    let running = RenderEffect::new(effect);
    on_cleanup_local(move || drop(running));
}

/// What the egui line puts between its fields.
const SEPARATOR: &str = "-";

/// Who was recording and which match, separated as the egui line separates them. Wraps rather
/// than running off a narrow panel; a separator wraps with the field after it.
#[component]
fn MatchContextLine(context: MatchContext) -> impl IntoView {
    let who = match context.clan_tag.as_ref() {
        Some(clan) => format!("{clan} {}", context.player_name),
        None => context.player_name.clone(),
    };
    let fields: Vec<String> = [who, context.game_type, context.version, context.game_mode, context.map]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect();
    let items: Vec<AnyView> = fields
        .into_iter()
        .enumerate()
        .map(|(at, text)| {
            let separator = (at > 0).then_some(SEPARATOR);
            AnyView::new(view! {
                row(class = "wt-rp-context__item", attr:data-testid = "replay-match-field") {
                    {separator.map(|separator| view! { text {{separator}} })}
                    text {{text}}
                }
            })
        })
        .collect();
    view! { row(class = "wt-rp-context", attr:data-testid = "replay-match-context") {{items}} }
}

/// What the Export menu offers, in the egui menu's order.
const EXPORT_FORMATS: [ReplayExportFormat; 3] =
    [ReplayExportFormat::Json, ReplayExportFormat::Cbor, ReplayExportFormat::Csv];

fn export_label(format: ReplayExportFormat) -> String {
    match format {
        ReplayExportFormat::Json => t!("ui.replay.export_results_json").into_owned(),
        ReplayExportFormat::Cbor => t!("ui.replay.export_results_cbor").into_owned(),
        ReplayExportFormat::Csv => t!("ui.replay.export_results_csv").into_owned(),
    }
}

pub fn export_extension(format: ReplayExportFormat) -> &'static str {
    match format {
        ReplayExportFormat::Json => "json",
        ReplayExportFormat::Cbor => "cbor",
        ReplayExportFormat::Csv => "csv",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("could not create {path}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {reason}")]
    Write { path: PathBuf, reason: String },
}

/// Writes `match_data` to `path`. JSON and CBOR carry the whole match; CSV one flattened row per
/// vehicle.
pub fn write_export(match_data: &ExportedMatch, path: &Path, format: ReplayExportFormat) -> Result<(), ExportError> {
    let file = std::io::BufWriter::new(
        std::fs::File::create(path).map_err(|source| ExportError::Create { path: path.to_path_buf(), source })?,
    );
    let failed = |reason: String| ExportError::Write { path: path.to_path_buf(), reason };

    match format {
        ReplayExportFormat::Json => serde_json::to_writer(file, match_data).map_err(|err| failed(err.to_string())),
        ReplayExportFormat::Cbor => ciborium::into_writer(match_data, file).map_err(|err| failed(err.to_string())),
        ReplayExportFormat::Csv => {
            let mut writer = csv::WriterBuilder::new().has_headers(true).from_writer(file);
            for vehicle in match_data.vehicles.iter().cloned() {
                writer.serialize(FlattenedVehicle::from(vehicle)).map_err(|err| failed(err.to_string()))?;
            }
            writer.flush().map_err(|err| failed(err.to_string()))
        }
    }
}

/// Asks where to write the match and writes it. The match is held in its debug form, so an
/// ordinary export strips a copy.
fn export_match(
    loaded: &Loaded,
    format: ReplayExportFormat,
    debug: bool,
    status: RwSignal<Option<String>, LocalStorage>,
    cx: ExportCx,
) {
    let export = if debug { loaded.export.clone() } else { loaded.export.clone().stripped() };
    let asked = wows_toolkit_services::dialog::save_file(
        Some(&t!("ui.replay.export_results_title")),
        &format!("{}.{}", loaded.export_stem, export_extension(format)),
        None,
    );
    let shell = cx.shell;
    spawn_in(cx.owner, async move {
        let path = match asked.await {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(err) => {
                shell.toasts().failed(t!("ui.dialogs.file_dialog_failed", reason = err).into_owned());
                return;
            }
        };
        let written = blocking(move || write_export(&export, &path, format)).await;
        status.set(Some(match written {
            Ok(()) => t!("ui.replay.results_exported").into_owned(),
            Err(err) => {
                tracing::warn!("replay export failed: {err}");
                err.to_string()
            }
        }));
    });
}

/// Everything that is not flipped back and forth, sectioned as the egui menu sections it.
#[component]
fn ActionsMenu(header: HeaderCx) -> impl IntoView {
    let HeaderCx { loaded, side, debug, export_status, alts, path, events, add_alt, export, .. } = header;
    let export = StoredValue::new_local(export);
    let on_render = events.with_value(|events| events.on_render);
    let is_test_ship = move || {
        loaded.with(|loaded| {
            loaded.as_ref().is_some_and(|loaded| loaded.model.with(|model| self_is_test_ship(&model.rows)))
        })
    };
    let hidden = move || {
        loaded.with(|loaded| {
            loaded.as_ref().is_some_and(|loaded| loaded.model.with(|model| self_stats_hidden(&model.rows)))
        })
    };
    let set_hidden = UnsyncCallback::new(move |hide: bool| {
        let Some(loaded) = loaded.get_untracked() else { return };
        loaded.model.update(|model| {
            *model = edited_model(model, |model| {
                if let Some(row) = model.rows.iter_mut().find(|row| row.is_self) {
                    row.manual_stat_hide_toggle = hide;
                }
            });
        });
    });
    let alt_label = move || match alts.with(Vec::len) {
        0 => t!("ui.replay.load_alt_perspective").into_owned(),
        merged => format!("{} ({merged})", t!("ui.replay.load_alt_perspective")),
    };
    let exports = move || {
        EXPORT_FORMATS
            .into_iter()
            .map(|format| {
                AnyView::new(view! {
                    MenuItem(
                        disabled = Signal::derive_local(move || loaded.with(Option::is_none)),
                        attr:data-testid = format!("replay-export-{}", export_extension(format)),
                        on_select = UnsyncCallback::new(move |()| {
                            if let Some(loaded) = loaded.get_untracked() {
                                export_match(&loaded, format, debug.get_untracked(), export_status, export.get_value());
                            }
                        })
                    ) {{export_label(format)}}
                })
            })
            .collect::<Vec<_>>()
    };
    let load_alt = UnsyncCallback::new(move |()| add_alt());
    let debug_items = move || {
        if !debug.get() {
            return None;
        }
        let has_results = loaded.with(|loaded| loaded.as_ref().is_some_and(|loaded| loaded.raw_results.is_some()));
        let has_mapped = loaded.with(|loaded| loaded.as_ref().is_some_and(|loaded| loaded.mapped_results.is_some()));
        let items: Vec<AnyView> = [
            (SidePanel::RawMetadata, "ui.replay.debug.raw_metadata", true),
            (SidePanel::RawResults, "ui.replay.debug.raw_results", has_results),
            (SidePanel::MappedResults, "ui.replay.debug.mapped_results", has_mapped),
        ]
        .into_iter()
        .map(|(which, label, enabled)| {
            AnyView::new(view! {
                MenuCheckboxItem(
                    checked = Binding::controlled(Signal::derive_local(move || side.get() == which), |_| {}),
                    disabled = !enabled,
                    on_change = UnsyncCallback::new(move |_| side.update(|side| *side = if *side == which { SidePanel::None } else { which }))
                ) {{t!(label).into_owned()}}
            })
        })
        .collect();
        Some(AnyView::new(view! {
            MenuSeparator()
            MenuLabel(class = "wt-rp-menu-heading") {{t!("ui.replay.section_debug").into_owned()}}
            {items}
        }))
    };

    view! {
        DropdownMenu {
            DropdownMenuTrigger(variant = ButtonVariant::Outline, size = ButtonSize::Sm, attr:data-testid = "replay-actions") {
                {t!("ui.replay.actions").into_owned()}
            }
            DropdownMenuContent {
                MenuLabel(class = "wt-rp-menu-heading") {{t!("ui.replay.section_match").into_owned()}}
                {on_render.map(|on_render| AnyView::new(view! {
                    MenuItem(
                        attr:data-testid = "replay-render",
                        on_select = UnsyncCallback::new(move |()| on_render.run(RenderRequest { path: path.get_value(), alts: alts.get_untracked() }))
                    ) {{t!("ui.replay.context.render_replay").into_owned()}}
                }))}
                MenuItem(attr:data-testid = "replay-load-alt", on_select = load_alt) {{alt_label}}
                Show(when = is_test_ship) {
                    MenuCheckboxItem(
                        checked = Binding::controlled(Signal::derive_local(hidden), |_| {}),
                        attr:data-testid = "replay-hide-my-stats",
                        on_change = set_hidden
                    ) {{t!("ui.replay.hide_my_stats").into_owned()}}
                }
                MenuSeparator()
                MenuLabel(class = "wt-rp-menu-heading") {{t!("ui.replay.section_export").into_owned()}}
                {exports}
                {debug_items}
            }
        }
    }
}

/// The side panel's caption over what it shows. A row's metadata is opened from the row's menu,
/// which opens rather than toggles, so that panel has a close button of its own.
#[component]
fn SideContent(
    kind: RwSignal<SidePanel, LocalStorage>,
    side: RwSignal<SidePanel, LocalStorage>,
    loaded: Signal<Option<Rc<Loaded>>, LocalStorage>,
    player_metadata: RwSignal<Option<Rc<str>>, LocalStorage>,
) -> impl IntoView {
    let title = move || kind.get().title_key().map(|key| t!(key).into_owned()).unwrap_or_default();
    let close_label = StoredValue::new(t!("ui.replay.close_side_panel").into_owned());
    let content = move || {
        let loaded = loaded.get()?;
        match kind.get() {
            SidePanel::None => None,
            SidePanel::Chat => {
                let messages = loaded.chat.clone()?;
                Some(AnyView::new(view! { ChatPanel(messages = messages, title = loaded.title.clone()) }))
            }
            SidePanel::RawMetadata => Some(raw(Rc::clone(&loaded.raw_metadata))),
            SidePanel::RawResults => loaded.raw_results.clone().map(raw),
            SidePanel::MappedResults => loaded.mapped_results.clone().map(raw),
            SidePanel::RawPlayerMetadata => player_metadata.get().map(raw),
        }
    };

    view! {
        column(class = "wt-rp-side", attr:data-testid = "replay-side-panel") {
            row(class = "wt-rp-side__head") {
                text(class = "wt-rp-side__title") {{title}}
                Show(when = move || kind.get() == SidePanel::RawPlayerMetadata) {
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::IconSm,
                                attr:data-testid = "replay-side-panel-close",
                                a11y:label = close_label.get_value(),
                                on:click = move |_| side.set(SidePanel::None)
                            ) {
                                AppIcon(glyph = wows_toolkit_icons::Glyph::PanelRightClose)
                            }
                        }
                        TooltipContent {{close_label.get_value()}}
                    }
                }
            }
            {content}
        }
    }
}

fn raw(content: Rc<str>) -> AnyView {
    AnyView::new(view! { RawJsonPanel(content = content) })
}

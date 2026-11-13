//! The Player Tracker tab: everyone met in indexed battles over a chosen window of time, the clans
//! they came from, and the battle in progress.
//!
//! The three sections are panels of one dock, so they can be split and docked; the status bar
//! shows and hides the dock's side regions once a section has been moved into one.

mod live;
mod model;
mod roster;
mod tables;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::EscapeClears;
use landaire_ui::PanelEntry;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::PanelSide;
use landaire_ui::SavedNode;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use zgui::component;
use zgui::css;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::SelectContentProps;
use zgui_ui::SelectItemProps;
use zgui_ui::SelectProps;
use zgui_ui::SelectTriggerProps;
use zgui_ui::SelectTriggerSize;
use zgui_ui::SelectValueProps;
use zgui_ui::SeparatorOrientation;
use zgui_ui::SeparatorProps;
use zgui_ui::prelude::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::PanelToggle;
use crate::shell::tabs::TabChrome;
use crate::shell::tabs::TabKey;
use crate::shell::tabs::panel_side_name;
use crate::tabs::dock_menu_strings;
use crate::tabs::player_tracker::live::LiveSourceConfig;
use crate::tabs::player_tracker::model::LoadState;
use crate::tabs::player_tracker::model::TrackerModel;
use crate::tabs::player_tracker::roster::CurrentMatchSectionProps;
use crate::tabs::player_tracker::tables::ClansSectionProps;
use crate::tabs::player_tracker::tables::IconButtonProps;
use crate::tabs::player_tracker::tables::PlayersSectionProps;

const SHEET: &str = "wt-player-tracker";

const CSS: &str = css!(
    ".wt-tracker {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .wt-tracker-toolbar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-tracker-toolbar__period { width: 150px; }
    .wt-tracker-toolbar__filter { display: flex; flex-direction: row; align-items: center; gap: 4px; flex: none; }
    .wt-tracker-toolbar__search { display: flex; flex-direction: row; align-items: center; gap: 4px; flex: none; width: 260px; }
    .wt-tracker-toolbar__field { flex: 1 1 auto; }
    .wt-tracker-toolbar__spacer { flex: 1 1 auto; min-width: 0; }
    .wt-tracker-dock { display: flex; flex: 1 1 auto; min-height: 0; }
    .wt-tracker-fill { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-tracker-section {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        color: var(--lui-color-text);
    }
    .wt-tracker-header, .wt-tracker-cells, .wt-tracker-modes, .wt-tracker-team__heading,
    .wt-tracker-team__header, .wt-tracker-roster-row, .wt-tracker-member {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        padding: 0 8px;
    }
    .wt-tracker-header, .wt-tracker-modes, .wt-tracker-team__header {
        flex: none;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-tracker-heading { justify-content: flex-start; gap: 4px; font-weight: 700; font-size: 12px; padding: 0; }
    .wt-tracker-heading[data-active] { color: var(--lui-color-text-bright); }
    .wt-tracker-list { flex: 1 1 auto; min-height: 0; }
    .wt-tracker-row { display: flex; flex-direction: column; }
    .wt-tracker-cells { height: 24px; font-size: 13px; }
    .wt-tracker-cells:hover { background-color: var(--lui-color-widget-hot); }
    .wt-tracker-col--name { display: flex; flex-direction: row; align-items: center; gap: 4px; width: 220px; flex: none; min-width: 0; }
    .wt-tracker-col--tag { display: flex; flex-direction: row; align-items: center; gap: 4px; width: 160px; flex: none; min-width: 0; }
    .wt-tracker-col--clan { width: 120px; flex: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .wt-tracker-col--members { width: 120px; flex: none; }
    .wt-tracker-col--count { width: 110px; flex: none; }
    .wt-tracker-col--range { width: 170px; flex: none; }
    .wt-tracker-col--seen { width: 200px; flex: none; }
    .wt-tracker-caret Icon, .wt-tracker-caret > * { transform: rotate(0deg); transition: transform var(--lui-motion-turn) var(--lui-motion-ease); }
    .wt-tracker-caret[data-open] > * { transform: rotate(90deg); }
    .wt-tracker-faint { color: var(--wt-color-text-faint); }
    .wt-tracker-dim { color: var(--wt-color-text-dim); }
    .wt-tracker-small { font-size: 12px; }
    .wt-tracker-truncate { min-width: 0; flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    [data-severity=\"noted\"] { color: var(--wt-color-division); }
    [data-severity=\"warned\"] { color: var(--wt-color-warn); }
    [data-severity=\"heavy\"] { color: var(--wt-color-loss); }
    .wt-tracker-detail {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 4px 8px 4px 220px;
    }
    .wt-tracker-detail--clan { gap: 1px; padding-left: 160px; }
    .wt-tracker-account { align-self: flex-start; }
    .wt-tracker-note { display: flex; flex-direction: column; gap: 4px; }
    .wt-tracker-note__error { color: var(--wt-color-error); }
    .wt-tracker-status {
        display: flex;
        flex-direction: row;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        gap: 8px;
        font-size: 13px;
        color: var(--wt-color-text-dim);
    }
    .wt-tracker-modes { gap: 4px; }
    .wt-tracker-modes__gap { width: 8px; }
    .wt-tracker-mode[data-selected] { background-color: var(--lui-color-selection); color: var(--lui-color-text-bright); }
    .wt-tracker-roster { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-tracker-roster__note { display: flex; flex-direction: row; align-items: center; gap: 6px; flex: none; padding: 4px 8px; }
    .wt-tracker-roster__scroll { flex: 1 1 auto; min-height: 0; overflow-y: auto; }
    .wt-tracker-teams { display: flex; flex-direction: row; align-items: flex-start; }
    .wt-tracker-teams__rule { width: 1px; align-self: stretch; background-color: var(--lui-color-border); }
    .wt-tracker-team { display: flex; flex-direction: column; flex: 1 1 0; min-width: 0; }
    .wt-tracker-team__heading { padding: 4px 8px; }
    .wt-tracker-team__title { font-size: 13px; font-weight: 700; }
    .wt-tracker-team__header { font-size: 12px; font-weight: 700; }
    .wt-tracker-roster-row { height: 24px; font-size: 13px; }
    .wt-tracker-roster-row[data-band] { background-color: color-mix(in srgb, var(--wt-pr-band-hue) 8%, transparent); }
    .wt-tracker-roster-col--class { width: 16px; height: 16px; flex: none; display: flex; align-items: center; }
    .wt-tracker-class-icon { width: 16px; height: 16px; }
    .wt-tracker-roster-col--mark { width: 16px; flex: none; display: flex; align-items: center; }
    .wt-tracker-roster-col--chip { width: 24px; flex: none; display: flex; align-items: center; }
    .wt-tracker-roster-col--name { flex: 1 1 0; min-width: 96px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .wt-tracker-roster-col--ship { width: 130px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .wt-tracker-roster-col--stat { width: 64px; min-width: 0; overflow: hidden; }
    .wt-tracker-roster-col--met { width: 80px; min-width: 0; overflow: hidden; }
    [data-tint=\"self\"] { color: var(--wt-color-text-strong); }
    [data-tint=\"ally\"] { color: var(--wt-color-win); }
    [data-tint=\"enemy\"] { color: var(--wt-color-loss); }
    [data-tint=\"division\"] { color: var(--wt-color-division); }
    [data-tint=\"abuser\"] { color: var(--wt-color-abuser); }
    .wt-tracker-chip { padding: 0 4px; border-radius: 3px; background-color: color-mix(in srgb, var(--wt-pr-band-hue) 18%, transparent); color: var(--wt-pr-band-text); }
    .wt-tracker-band { display: flex; align-items: center; color: var(--wt-pr-band-text); }
    [data-band=\"bad\"] { --wt-pr-band-hue: var(--wt-pr-bad-hue); --wt-pr-band-text: var(--wt-pr-bad-text); }
    [data-band=\"below-average\"] { --wt-pr-band-hue: var(--wt-pr-below-average-hue); --wt-pr-band-text: var(--wt-pr-below-average-text); }
    [data-band=\"average\"] { --wt-pr-band-hue: var(--wt-pr-average-hue); --wt-pr-band-text: var(--wt-pr-average-text); }
    [data-band=\"good\"] { --wt-pr-band-hue: var(--wt-pr-good-hue); --wt-pr-band-text: var(--wt-pr-good-text); }
    [data-band=\"very-good\"] { --wt-pr-band-hue: var(--wt-pr-very-good-hue); --wt-pr-band-text: var(--wt-pr-very-good-text); }
    [data-band=\"great\"] { --wt-pr-band-hue: var(--wt-pr-great-hue); --wt-pr-band-text: var(--wt-pr-great-text); }
    [data-band=\"unicum\"] { --wt-pr-band-hue: var(--wt-pr-unicum-hue); --wt-pr-band-text: var(--wt-pr-unicum-text); }
    [data-band=\"super-unicum\"] { --wt-pr-band-hue: var(--wt-pr-super-unicum-hue); --wt-pr-band-text: var(--wt-pr-super-unicum-text); }
    .wt-tracker-pre { white-space: pre-wrap; }"
);

/// The tracker's three sections, in the order the egui app lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Players,
    CurrentMatch,
    Clans,
}

impl Section {
    const ALL: [Self; 3] = [Self::Players, Self::CurrentMatch, Self::Clans];

    const fn id(self) -> &'static str {
        match self {
            Self::Players => "players",
            Self::CurrentMatch => "current-match",
            Self::Clans => "clans",
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.player_tracker.subtab_players",
            Self::CurrentMatch => "ui.player_tracker.subtab_current_match",
            Self::Clans => "ui.player_tracker.subtab_clans",
        }
    }

    fn from_id(id: &PanelId) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.id() == id.as_str())
    }
}

/// The dock's side regions as status-bar toggles, each test-findable by side.
const DOCK_SIDES: [(DockRegion, PanelSide, &str); 3] = [
    (DockRegion::Left, PanelSide::Left, "tracker-dock-toggle-left"),
    (DockRegion::Right, PanelSide::Right, "tracker-dock-toggle-right"),
    (DockRegion::Bottom, PanelSide::Bottom, "tracker-dock-toggle-bottom"),
];

#[cfg(test)]
thread_local! {
    /// The model of the tab built last on this thread, for tests to seed.
    static BUILT: std::cell::Cell<Option<TrackerModel>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub fn built_model() -> Option<TrackerModel> {
    BUILT.with(std::cell::Cell::get)
}

#[component]
pub fn PlayerTrackerTab() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let shell = Shell::current();
    let model = TrackerModel::new(services);
    #[cfg(test)]
    BUILT.with(|built| built.set(Some(model)));

    // The index is read once the database is open and the tab has been built, which is the first
    // time it is shown: its aggregates are too heavy to run at startup.
    let started = RwSignal::new_local(false);
    keep(RenderEffect::new(move |_| {
        if started.get_untracked() {
            return;
        }
        // The pool is not reactive; the settings state it is opened with is.
        let opened = services.settings.read(|_| ()).is_some();
        if let Some(pool) = services.settings.pool().filter(|_| opened) {
            started.set(true);
            model.start(pool);
        }
    }));

    // The live watch follows the install and its game data.
    keep(RenderEffect::new(move |_| {
        let wows_dir = services.settings.read(|settings| settings.wows_dir.clone()).filter(|dir| !dir.is_empty());
        let config = wows_dir
            .zip(services.game_data.cache())
            .map(|(dir, game_data)| LiveSourceConfig { replay_dir: PathBuf::from(dir).join("replays"), game_data });
        model.live.watch(services, config);
    }));

    let dock = tracker_dock(model);
    let chrome = TabChrome::current();
    dock_toggles(dock, chrome);

    // The stats lookup waits until the Current Match section is on screen: its budget is small.
    keep(RenderEffect::new(move |_| {
        let tab_shown = shell.selected() == Some(TabKey::App(AppTab::PlayerTracker));
        model.live.set_shown(services, tab_shown && section_visible(dock, Section::CurrentMatch));
    }));

    // The clans table's inputs are whole-index scans, read once per refresh and only once the
    // table is on screen.
    keep(RenderEffect::new(move |_| {
        let tab_shown = shell.selected() == Some(TabKey::App(AppTab::PlayerTracker));
        let idle = model.clan_load.with(|state| *state == LoadState::Idle);
        if idle && tab_shown && section_visible(dock, Section::Clans) {
            model.load_clan_inputs_if_idle();
        }
    }));

    let locale = services.locale;
    view! {
        column(class = "wt-tracker", attr:data-testid = "player-tracker-tab") {
            // Rebuilt with the language, so every label it holds follows it.
            {move || {
                locale.track();
                view! { Toolbar(model = model, shell = shell) }
            }}
            Dock(
                state = dock,
                class = "wt-tracker-dock",
                menu = dock_menu_strings(locale)
            )
        }
    }
}

/// Whether `section` is the shown panel of its group, in a region that is open.
fn section_visible(dock: DockState, section: Section) -> bool {
    let id = PanelId::new(section.id());
    let layout = dock.dump();
    [&layout.center, &layout.left, &layout.right, &layout.bottom]
        .into_iter()
        .filter(|region| region.open)
        .any(|region| region.root.as_ref().is_some_and(|node| node_shows(node, &id)))
}

fn node_shows(node: &SavedNode, id: &PanelId) -> bool {
    match node {
        SavedNode::Tabs { active, .. } => active == id,
        SavedNode::Split { parts, .. } => parts.iter().any(|part| node_shows(&part.node, id)),
    }
}

/// The dock with the three sections in one group, the players in front.
fn tracker_dock(model: TrackerModel) -> DockState {
    let kind = PanelKind::new("tracker-section");
    let locale = Services::current().locale;
    let mut registry = PanelRegistry::new();
    registry.register(
        kind.clone(),
        PanelFactory::new(
            |id: &PanelId| Section::from_id(id).map(|section| t!(section.label_key()).into_owned()).unwrap_or_default(),
            move |id: &PanelId| {
                let section = Section::from_id(id);
                // Rebuilt with the language, so every label a section holds follows it.
                AnyView::new(move || {
                    locale.track();
                    match section {
                        Some(Section::Players) => AnyView::new(view! { PlayersSection(model = model) }),
                        Some(Section::CurrentMatch) => AnyView::new(view! { CurrentMatchSection(model = model) }),
                        Some(Section::Clans) => AnyView::new(view! { ClansSection(model = model) }),
                        None => AnyView::new(()),
                    }
                })
            },
        ),
    );
    let dock = DockState::new(registry);
    let mut group = None;
    for section in Section::ALL {
        let entry = PanelEntry { closable: false, ..PanelEntry::new(PanelId::new(section.id()), kind.clone()) };
        let target = group.map_or(DockTarget::Region(DockRegion::Center), DockTarget::Group);
        match dock.add_panel(target, entry) {
            Ok(added) => group = Some(added),
            Err(err) => tracing::error!("player tracker: the {section:?} section was not docked: {err}"),
        }
    }
    if let Err(err) = dock.activate(&PanelId::new(Section::Players.id())) {
        tracing::error!("player tracker: the players section was not shown: {err}");
    }
    dock
}

/// Puts a status-bar toggle for each side region a section has been moved into, and keeps the
/// toggle and the region's open state in step.
fn dock_toggles(dock: DockState, chrome: Option<TabChrome>) {
    let opens = DOCK_SIDES.map(|(region, _, _)| RwSignal::new_local(dock.is_region_open(region)));
    for ((region, _, _), open) in DOCK_SIDES.into_iter().zip(opens) {
        keep(RenderEffect::new(move |_| {
            let shown = dock.is_region_open(region);
            if open.get_untracked() != shown {
                open.set(shown);
            }
        }));
        keep(RenderEffect::new(move |_| {
            let wanted = open.get();
            if dock.is_region_open(region) != wanted {
                dock.set_region_open(region, wanted);
            }
        }));
    }
    let Some(chrome) = chrome else { return };
    let locale = Services::current().locale;
    keep(RenderEffect::new(move |_| {
        locale.track();
        let toggles = DOCK_SIDES
            .into_iter()
            .zip(opens)
            .filter(|((region, _, _), _)| !dock.is_region_empty(*region))
            .map(|((_, side, test_id), open)| PanelToggle { test_id, name: panel_side_name(side), side, open })
            .collect();
        chrome.set_toggles(toggles);
    }));
}

/// Keeps `effect` running until the calling scope is cleaned up.
fn keep(effect: RenderEffect<()>) {
    on_cleanup_local(move || drop(effect));
}

/// The period, the filter, and the actions that act on every section.
#[component]
fn Toolbar(model: TrackerModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;
    let period = Signal::derive_local(move || format!("{:?}", model.period.get()));
    let choose = move |chosen: String| {
        if let Some(period) = TimePeriod::ALL.into_iter().find(|period| format!("{period:?}") == chosen) {
            model.set_period(period);
        }
    };
    let periods = Signal::derive_local(move || {
        locale.track();
        TimePeriod::ALL.to_vec()
    });
    let filter = RwSignal::new_local(String::new());
    // The saved filter arrives after the field is built; the field writes back as it is typed in.
    keep(RenderEffect::new(move |_| {
        let saved = model.filter.get();
        if filter.get_untracked() != saved {
            filter.set(saved);
        }
    }));
    keep(RenderEffect::new(move |_| model.set_filter(filter.get())));
    let show_division_mates = Signal::derive_local(move || Checked::from(model.show_division_mates.get()));

    let escape = EscapeClears::new(filter);
    view! {
        row(class = "wt-tracker-toolbar") {
            Select(value = Binding::controlled(period, choose)) {
                SelectTrigger(
                    size = SelectTriggerSize::Sm,
                    label = t!("ui.player_tracker.time_period").into_owned(),
                    class = "wt-tracker-toolbar__period",
                    attr:data-testid = "tracker-period"
                ) {
                    SelectValue()
                }
                SelectContent {
                    for period in move || periods.get(), key = |period: &TimePeriod| *period as usize {
                        SelectItem(value = format!("{period:?}"), text = t!(period.label_key()).into_owned()) {
                            {t!(period.label_key()).into_owned()}
                        }
                    }
                }
            }
            Separator(orientation = SeparatorOrientation::Vertical)
            row(class = "wt-tracker-toolbar__search") {
                AppIcon(glyph = Glyph::Search)
                Input(
                    value = filter,
                    placeholder = t!("ui.player_tracker.filter_hint").into_owned(),
                    label = t!("ui.player_tracker.filter_hint").into_owned(),
                    class = "wt-tracker-toolbar__field",
                    attr:data-testid = "tracker-filter",
                    on:key_down = move |ev| {
                        escape.key_down(ev);
                    }
                )
            }
            box(class = "wt-tracker-toolbar__spacer")
            Separator(orientation = SeparatorOrientation::Vertical)
            IconButton(
                glyph = Glyph::RotateCcw,
                hover = t!("ui.player_tracker.populate_from_replays").into_owned(),
                test_id = "tracker-repopulate".to_owned(),
                on_press = move || {
                    if let Some(pool) = model.services.settings.pool() {
                        model.refresh(pool);
                    }
                }
            )
            IconButton(
                glyph = Glyph::Trash,
                hover = t!("ui.player_tracker.clear_stats").into_owned(),
                test_id = "tracker-clear".to_owned(),
                on_press = move || confirm_clear(shell, model)
            )
            Tooltip {
                TooltipTrigger {
                    row(class = "wt-tracker-toolbar__filter") {
                        Checkbox(
                            checked = Binding::controlled(show_division_mates, move |checked: Checked| {
                                model.set_show_division_mates(checked == Checked::Yes);
                            }),
                            attr:data-testid = "tracker-show-division-mates",
                            a11y:label = t!("ui.player_tracker.show_division_mates").into_owned()
                        )
                        text {{t!("ui.player_tracker.show_division_mates").into_owned()}}
                    }
                }
                TooltipContent {{t!("ui.player_tracker.show_division_mates_hover").into_owned()}}
            }
        }
    }
}

/// Asks before forgetting every tracked player: what goes is every encounter, alias and note, and
/// nothing brings the notes back.
fn confirm_clear(shell: Shell, model: TrackerModel) {
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "tracker-clear-dialog") {
                    AlertDialogHeader {
                        AlertDialogTitle {{t!("ui.player_tracker.clear_stats").into_owned()}}
                        AlertDialogDescription {{t!("confirm.clear_all_session_stats").into_owned()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(attr:data-testid = "tracker-clear-confirm", on:click = move |_| model.clear_tracked()) {
                            {t!("ui.buttons.yes").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

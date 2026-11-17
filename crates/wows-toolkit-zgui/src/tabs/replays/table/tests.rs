use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::MotionPreference;
use landaire_ui::Scheme;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_replay_insights::match_contribution::FactorScore;
use wows_replay_insights::match_contribution::MatchContributionComponents;
use wows_replay_insights::match_contribution::MatchContributionFactor;
use wows_replay_insights::match_contribution::MatchContributionInputs;
use wows_replay_insights::match_contribution::MatchContributionResult;
use wows_replay_insights::match_contribution::MatchContributionVersion;
use wows_replay_insights::match_contribution::Ratio;
use wows_replay_insights::match_contribution::Share;
use wows_replay_insights::match_contribution::TankingRatios;
use wows_replay_insights::match_contribution::TeamDamageShare;
use wows_replay_insights::personal_rating::PersonalRatingCategory;
use wows_replay_insights::personal_rating::rate_single_battle;
use wows_replays::types::AccountId;
use wows_toolkit_replay_data::damage_mix::DamageKind;
use wows_toolkit_replay_data::damage_mix::DamageLine;
use wows_toolkit_replay_data::sample::sample_model;
use wows_toolkit_replay_data::test_support::FIXTURE_PR_SHIP_ID;
use wows_toolkit_replay_data::test_support::fixture_personal_rating_data;
use wows_toolkit_viewmodel::rating_breakdown::RatingBreakdown;
use wows_toolkit_viewmodel::replay_sort::SortColumn;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::Signal;
use zgui::reactive::provide_local_context;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use super::*;
use crate::services::saved_for_test;
use crate::theme::SemanticTokensProps;

/// Rows tall enough to hold the sample's players in one view.
const SIZE: WindowSize = WindowSize { width: 1280.0, height: 720.0 };

thread_local! {
    /// The open rows of the table built last on this thread.
    pub(super) static EXPANDED: Cell<Option<RwSignal<HashSet<RowId>, LocalStorage>>> = const { Cell::new(None) };
    /// The selected rows of the table built last on this thread.
    pub(super) static SELECTION: Cell<Option<RwSignal<Selection<RowId>, LocalStorage>>> = const { Cell::new(None) };
    /// The services of the table built last on this thread.
    static SERVICES: Cell<Option<Services>> = const { Cell::new(None) };
    /// The model of the table built last on this thread.
    static MODEL: Cell<Option<RwSignal<Rc<ReplayReportModel>, LocalStorage>>> = const { Cell::new(None) };
}

/// The table over `model`, as the Replays tab holds it. With `with_detail`, each open row's detail
/// is a box naming the player and the third column drawn. With `saved_sort`, the settings are
/// read and hold that sort. With `height`, the table is held to that many pixels and scrolls.
#[component]
fn Harness(
    scheme: Scheme,
    model: Rc<ReplayReportModel>,
    with_detail: bool,
    #[prop(optional)] saved_sort: Option<SortOrder>,
    #[prop(optional)] height: Option<f32>,
) -> impl IntoView {
    let services = Services::provide(None);
    if let Some(replay_sort) = saved_sort {
        services.settings.adopt_unsaved(wows_toolkit_services::settings::StartupSettings {
            replay_sort,
            ..crate::shell::settings::tests::test_settings()
        });
    }
    SERVICES.with(|slot| slot.set(Some(services)));
    let model = RwSignal::new_local(model);
    MODEL.with(|slot| slot.set(Some(model)));
    let theme = match scheme {
        Scheme::Light => ThemeChoice::Light,
        _ => ThemeChoice::Dark,
    };
    provide_local_context(crate::shell::Appearance {
        theme: RwSignal::new_local(theme),
        theme_preview: RwSignal::new_local(None),
        zoom: RwSignal::new_local(ZoomFactor::DEFAULT),
        motion: RwSignal::new_local(MotionPreference::Full),
    });
    let debug = Signal::stored_local(false);
    // Built inside the theme root, under its context.
    let table = move || {
        if !with_detail {
            return AnyView::new(view! { PlayerTable(model = model, debug = debug) });
        }
        let detail: PlayerDetail = Rc::new(|cx: PlayerDetailCx| {
            if !matches!(cx.column, ReplayColumn::Name | ReplayColumn::Kills) {
                return None;
            }
            let id = format!("detail-{}-{}", cx.row.db_id.raw(), cx.column.storage_key());
            let name = cx.row.display_name.clone();
            Some(AnyView::new(view! { box(attr:data-testid = id, style:padding = "8px") {{name}} }))
        });
        AnyView::new(view! { PlayerTable(model = model, debug = debug, detail = detail) })
    };
    let held = move || match height {
        Some(height) => AnyView::new(view! {
            box(style = format!("display: flex; flex-direction: column; height: {height}px")) {{table}}
        }),
        None => table(),
    };

    view! {
        ThemeRoot(choice = Signal::stored_local(landaire_ui::ThemeChoice::Fixed(scheme))) {
            SemanticTokens(choice = Signal::stored_local(theme))
            ToastHost {
                {held}
            }
        }
    }
}

/// `model` with each player's dealt and received damage split by type, so the damage cells draw
/// their bands.
fn split_damage(mut model: ReplayReportModel) -> ReplayReportModel {
    let split = |total: u64| {
        vec![
            DamageLine { kind: DamageKind::Ap, label: "AP", amount: total / 2 },
            DamageLine { kind: DamageKind::He, label: "HE", amount: total / 4 },
            DamageLine { kind: DamageKind::Fire, label: "Fire", amount: total - total / 2 - total / 4 },
        ]
    };
    for row in &mut model.rows {
        row.actual_damage_by_type = row.actual_damage.map(split);
        row.received_damage_by_type = row.received_damage.map(split);
    }

    model
}

fn open_table(drawing: Drawing, scheme: Scheme) -> (HeadlessWindow, RwSignal<HashSet<RowId>, LocalStorage>) {
    open_table_over(drawing, scheme, split_damage(sample_model()), true)
}

fn open_table_over(
    drawing: Drawing,
    scheme: Scheme,
    model: ReplayReportModel,
    with_detail: bool,
) -> (HeadlessWindow, RwSignal<HashSet<RowId>, LocalStorage>) {
    let model = Rc::new(model);
    let options = HeadlessOptions { drawing, desktop: Some(Appearance::Dark), ..HeadlessOptions::new(SIZE) };
    let mut window = HeadlessWindow::open(options, move || {
        view! { Harness(scheme = scheme, model = Rc::clone(&model), with_detail = with_detail) }
    })
    .expect("the table opens headlessly");
    window.advance(Duration::from_millis(300));
    let expanded = EXPANDED.with(Cell::take).expect("the table was built");

    (window, expanded)
}

/// The vertical place of each player's row, top to bottom.
fn row_order(window: &HeadlessWindow) -> Vec<i64> {
    let model = sample_model();
    let mut placed: Vec<(f32, i64)> = model
        .rows
        .iter()
        .filter_map(|row| {
            let id = row.db_id.raw();
            let cell = window.by_test_id(&format!("player-name-{id}"))?;
            Some((window.bounds(cell)?.origin.y.0, id))
        })
        .collect();
    placed.sort_by(|a, b| a.0.total_cmp(&b.0));

    placed.into_iter().map(|(_, id)| id).collect()
}

fn expected_order(order: SortOrder) -> Vec<i64> {
    let mut model = sample_model();
    sort_rows(&mut model.rows, model.self_team, order, false);

    model.rows.iter().map(|row| row.db_id.raw()).collect()
}

/// The row of the sample's player `db_id`.
fn sample_row(db_id: i64) -> RowId {
    let position = sample_model().rows.iter().position(|row| row.db_id.raw() == db_id);

    RowId(position.expect("the sample has the player"))
}

/// The cell in `column` on the line `element` is drawn on.
fn cell_on_line_of(window: &HeadlessWindow, element: Element, column: ReplayColumn) -> Element {
    let line = window.bounds(element).expect("the element has a box");
    let middle = line.origin.y.0 + line.size.height.0 / 2.0;
    window
        .all_by_attribute("data-column", column.storage_key())
        .into_iter()
        .find(|cell| {
            window
                .bounds(*cell)
                .is_some_and(|cell| cell.origin.y.0 <= middle && middle < cell.origin.y.0 + cell.size.height.0)
        })
        .expect("the line has the cell")
}

#[test]
fn bots_sharing_an_account_each_keep_their_own_row() {
    let mut model = sample_model();
    let bots = ["Bot_Alpha", "Bot_Bravo", "Bot_Charlie"];
    let enemies = model.rows.iter_mut().filter(|row| row.team_id != model.self_team);
    for (row, name) in enemies.zip(bots) {
        row.db_id = AccountId(0);
        row.is_bot = true;
        name.clone_into(&mut row.display_name);
    }
    let (mut window, expanded) = open_table_over(Drawing::Recorded, Scheme::Dark, model, true);
    let names = window.texts_by_class("wt-pt-text");
    for bot in bots {
        assert_eq!(names.iter().filter(|name| *name == bot).count(), 1, "{bot} has a row of its own");
    }

    let bravo = window.by_label("Bot_Bravo").expect("the bot's name is drawn");
    let kills = cell_on_line_of(&window, bravo, ReplayColumn::Kills);
    window.double_click(kills).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    assert_eq!(expanded.with_untracked(HashSet::len), 1, "only the bot pressed opens");
    assert_eq!(window.all_by_attribute("data-testid", "detail-0-name").len(), 1, "one detail is drawn");

    let bravo = window.by_label("Bot_Bravo").expect("the bot's name is drawn");
    window.right_click(bravo).expect("the name takes a right click");
    window.advance(Duration::from_millis(300));
    let heading = window.by_test_id("player-menu-name").expect("the menu names its player");
    let named = window.a11y(heading).and_then(|state| state.label);
    assert_eq!(named.as_deref(), Some("Bot_Bravo"), "the menu is the pressed bot's");
    let copy_cell = t!("ui.replay.context.copy_cell").into_owned();
    assert!(window.by_role_and_label(Role::MenuItem, &copy_cell).is_some(), "the pressed cell is the bot's");
}

#[test]
fn a_press_on_a_header_sorts_by_it_descending_first_then_ascending() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    assert_eq!(row_order(&window), expected_order(SortOrder::default()), "the table opens sorted by ship class");

    let kills = ReplayColumn::Kills.label();
    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    window.click(header).expect("the header takes a click");
    window.advance(Duration::from_millis(300));
    assert_eq!(row_order(&window), expected_order(SortOrder::Desc(SortColumn::Kills)));

    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    window.click(header).expect("the header takes a click");
    window.advance(Duration::from_millis(300));
    assert_eq!(row_order(&window), expected_order(SortOrder::Asc(SortColumn::Kills)));
}

#[test]
fn the_sort_opens_as_saved_and_a_press_saves_it_for_every_table() {
    let model = Rc::new(split_damage(sample_model()));
    let options = HeadlessOptions { desktop: Some(Appearance::Dark), ..HeadlessOptions::new(SIZE) };
    let mut window = HeadlessWindow::open(options, move || {
        view! {
            Harness(
                scheme = Scheme::Dark,
                model = Rc::clone(&model),
                with_detail = false,
                saved_sort = SortOrder::Desc(SortColumn::Kills)
            )
        }
    })
    .expect("the table opens headlessly");
    window.advance(Duration::from_millis(300));
    assert_eq!(row_order(&window), expected_order(SortOrder::Desc(SortColumn::Kills)), "the saved sort holds");

    let kills = ReplayColumn::Kills.label();
    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    window.click(header).expect("the header takes a click");
    window.advance(Duration::from_millis(300));

    assert_eq!(row_order(&window), expected_order(SortOrder::Asc(SortColumn::Kills)));
    let saved = saved_for_test();
    let (_, json) = saved.iter().rev().find(|(key, _)| *key == keys::REPLAY_SORT).expect("the sort is saved");
    assert_eq!(json, r#"{"Asc":"Kills"}"#, "saved in the form the egui app reads");
    let services = SERVICES.with(Cell::take).expect("the harness ran");
    let shared = services.settings.read_untracked(|settings| settings.replay_sort);
    assert_eq!(shared, Some(SortOrder::Asc(SortColumn::Kills)), "a table opened next starts from it");
}

/// The Kills cell on player `id`'s line: the Name cell's text keeps a double click to itself, so
/// tests open a row from here.
pub(crate) fn kills_cell(window: &HeadlessWindow, id: i64) -> Element {
    let name = window.by_test_id(&format!("player-name-{id}")).expect("the player's name is drawn");
    let line = window.bounds(name).expect("the name has a box");
    let middle = line.origin.y.0 + line.size.height.0 / 2.0;
    let on_line = |cell: &Element| {
        window
            .bounds(*cell)
            .is_some_and(|cell| cell.origin.y.0 <= middle && middle < cell.origin.y.0 + cell.size.height.0)
    };
    window
        .all_by_attribute("data-column", ReplayColumn::Kills.storage_key())
        .into_iter()
        .find(on_line)
        .expect("the player's line has a Kills cell")
}

#[test]
fn a_double_click_opens_a_rows_detail_and_its_disclosure_closes_it() {
    let (mut window, expanded) = open_table(Drawing::Recorded, Scheme::Dark);
    let cell = kills_cell(&window, 2);

    window.double_click(cell).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    assert!(window.by_test_id("detail-2-name").is_some(), "the row's detail is open");

    let caret = window.by_test_id("player-caret-2").expect("the row has a disclosure");
    window.click(caret).expect("the disclosure takes a click");
    window.advance(Duration::from_millis(600));
    assert!(expanded.with_untracked(HashSet::is_empty), "the disclosure closed the row");
    assert!(window.by_test_id("detail-2-name").is_none(), "the detail is gone once it has closed");
}

#[test]
fn a_double_click_on_the_players_name_leaves_the_row_closed() {
    let (mut window, expanded) = open_table(Drawing::Recorded, Scheme::Dark);
    let name = window.by_test_id("player-name-text-2").expect("the second player's name is drawn");

    window.double_click(name).expect("the name takes a double click");
    window.advance(Duration::from_millis(400));
    assert!(expanded.with_untracked(HashSet::is_empty), "no row opens");
}

#[test]
fn the_disclosure_opens_and_closes_a_row_without_selecting_it() {
    let (mut window, expanded) = open_table(Drawing::Recorded, Scheme::Dark);

    for _ in 0..2 {
        let caret = window.by_test_id("player-caret-2").expect("the row has a disclosure");
        window.click(caret).expect("the disclosure takes a click");
        window.advance(Duration::from_millis(600));
    }

    assert!(expanded.with_untracked(HashSet::is_empty), "the second press closed the row");
    let rows = window.by_role(Role::Row);
    let selected = rows.into_iter().filter(|row| window.a11y(*row).and_then(|state| state.selected) == Some(true));
    assert_eq!(selected.count(), 0, "no row is selected");
}

/// The rows `selection` holds.
fn selected_rows(selection: RwSignal<Selection<RowId>, LocalStorage>) -> HashSet<RowId> {
    selection.with_untracked(|selection| selection.keys().copied().collect())
}

/// The top of player `id`'s name.
fn name_top(window: &HeadlessWindow, id: i64) -> f32 {
    let name = window.by_test_id(&format!("player-name-{id}")).expect("the player's name is drawn");

    window.bounds(name).expect("the name has a box").origin.y.0
}

/// A column the filters hide, or a change of language, leaves the table as the reader had it:
/// scrolled as far, with the same row selected and the keyboard on it.
#[test]
fn a_column_filter_or_a_language_change_keeps_the_scroll_and_the_selection() {
    let model = Rc::new(split_damage(sample_model()));
    let options = HeadlessOptions { desktop: Some(Appearance::Dark), ..HeadlessOptions::new(SIZE) };
    let mut window = HeadlessWindow::open(options, move || {
        view! { Harness(scheme = Scheme::Dark, model = Rc::clone(&model), with_detail = true, height = 300.0) }
    })
    .expect("the table opens headlessly");
    window.advance(Duration::from_millis(300));
    let model = MODEL.with(Cell::take).expect("the table was built");
    let services = SERVICES.with(Cell::take).expect("the harness ran");
    EXPANDED.with(Cell::take);
    let selection = SELECTION.with(Cell::take).expect("the table was built");
    let only = |db_id: i64| HashSet::from([sample_row(db_id)]);

    let order = expected_order(SortOrder::default());
    // The sixth row is in view before and after the scroll.
    let (watched, next) = (order[5], order[6]);
    window.click(kills_cell(&window, watched)).expect("the row takes a click");
    window.advance(Duration::from_millis(300));
    assert_eq!(selected_rows(selection), only(watched), "a click selects the row");
    let port = window.by_class("lui-grid__port").expect("the grid scrolls");
    let unscrolled = name_top(&window, watched);
    window.wheel(port, 60.0).expect("the grid takes the wheel");
    window.advance(Duration::from_millis(300));
    let scrolled = name_top(&window, watched);
    assert!(scrolled < unscrolled, "the wheel scrolled the rows");

    let hidden = ReplayColumn::BaseXp;
    let mut filtered = split_damage(sample_model());
    assert!(filtered.columns.contains(&hidden), "the sample shows the column hidden here");
    filtered.columns.retain(|col| *col != hidden);
    model.set(Rc::new(filtered));
    window.advance(Duration::from_millis(300));
    assert!(window.by_role_and_label(Role::ColumnHeader, &hidden.label()).is_none(), "the column is hidden");
    assert_eq!(name_top(&window, watched), scrolled, "a filter change keeps the scroll");
    assert_eq!(selected_rows(selection), only(watched), "a filter change keeps the selection");

    let locale = services.locale;
    locale.choose(crate::i18n::LanguageCode::new("de"));
    window.advance(Duration::from_millis(300));
    let german = ReplayColumn::Kills.label();
    let relabelled = window.by_role_and_label(Role::ColumnHeader, &german).is_some();
    let kept_scroll = name_top(&window, watched);
    let kept_selection = selected_rows(selection);
    locale.choose(crate::i18n::LanguageCode::new("en"));
    window.advance(Duration::from_millis(300));
    assert_ne!(german, ReplayColumn::Kills.label(), "German names the column differently");
    assert!(relabelled, "the header follows the language");
    assert_eq!(kept_scroll, scrolled, "a language change keeps the scroll");
    assert_eq!(kept_selection, only(watched), "a language change keeps the selection");

    window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    window.advance(Duration::from_millis(300));
    assert_eq!(selected_rows(selection), only(next), "the keyboard moves on from the row it was on");
}

#[test]
fn pinning_a_column_from_its_header_is_saved_under_its_key() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let kills = ReplayColumn::Kills.label();
    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");

    window.right_click(header).expect("the header takes a right click");
    window.advance(Duration::from_millis(300));
    let pin = t!("ui.replay.context.pin_column").into_owned();
    let item = window.by_role_and_label(Role::MenuItem, &pin).expect("the header menu offers to pin");
    window.click(item).expect("the item takes a click");
    window.advance(Duration::from_millis(300));

    let saved = saved_for_test();
    let (_, json) =
        saved.iter().rev().find(|(key, _)| *key == COLUMN_ARRANGEMENT_KEY).expect("the arrangement is saved");
    let stored: StoredArrangement = serde_json::from_str(json).expect("the saved arrangement reads back");
    assert_eq!(stored.pinned, vec!["name", "ship_name", "kills"]);
}

#[test]
fn a_column_dragged_narrow_fits_an_open_detail_and_saves_only_the_drag() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let kills = ReplayColumn::Kills.label();
    let header_width = |window: &mut HeadlessWindow| {
        let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
        window.bounds(header).expect("the header has a box").size.width.0
    };
    let saved_widths = || {
        let saved = saved_for_test();
        saved.into_iter().filter(|(key, _)| *key == COLUMN_WIDTHS_KEY).map(|(_, json)| json).collect::<Vec<_>>()
    };

    let grip = window.by_role_and_label(Role::Splitter, &format!("Resize {kills}")).expect("the grip is drawn");
    let from = window.bounds(grip).expect("the grip has a box");
    let to = Point::<DevicePx, Device>::new(DevicePx(from.origin.x.0 - 40.0), from.origin.y);
    window.drag(grip, to, 8).expect("the grip has a box");
    window.advance(Duration::from_millis(300));
    let set = header_width(&mut window);
    let dragged = saved_widths();
    assert!(!dragged.is_empty(), "the drag saved the widths");

    let cell = kills_cell(&window, 2);
    window.double_click(cell).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    let detail = window.by_test_id("detail-2-kills").expect("the Kills detail is open");
    let needs = window.bounds(detail).expect("the detail has a box").size.width.0;
    assert!(set < needs, "the fixture needs the detail ({needs}) wider than the drag ({set})");
    assert_eq!(header_width(&mut window), needs, "the column widens to the detail");

    let cell = kills_cell(&window, 2);
    window.double_click(cell).expect("the row takes a double click");
    window.advance(Duration::from_millis(600));
    assert_eq!(header_width(&mut window), set, "closed, the column returns to the width dragged to");
    assert_eq!(saved_widths(), dragged, "and the detail saved no width of its own");
}

#[test]
fn only_a_menu_opened_on_a_cell_offers_to_copy_that_cell() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let copy_cell = t!("ui.replay.context.copy_cell").into_owned();
    let copy_row = t!("ui.replay.context.copy_row").into_owned();

    let name = window.by_test_id("player-name-1").expect("the first player's name is drawn");
    window.right_click(name).expect("the cell takes a right click");
    window.advance(Duration::from_millis(300));
    assert!(window.by_role_and_label(Role::MenuItem, &copy_cell).is_some(), "the cell's menu copies the cell");
    assert!(window.by_role_and_label(Role::MenuItem, &copy_row).is_some());
    window.press_key(zgui::vocab::NamedKey::Escape, zgui::vocab::Modifiers::NONE);
    window.advance(Duration::from_millis(300));

    let row = window.by_test_id("player-name-1").expect("the first player's name is drawn");
    window.hover(row).expect("the row takes the pointer");
    window.advance(Duration::from_millis(100));
    let dots = window.by_test_id("player-actions-1").expect("the row has its actions");
    window.click(dots).expect("the dots take a click");
    window.advance(Duration::from_millis(300));
    assert!(window.by_role_and_label(Role::MenuItem, &copy_row).is_some(), "the dots offer the row");
    assert!(window.by_role_and_label(Role::MenuItem, &copy_cell).is_none(), "the dots stand over no cell");
}

#[test]
fn the_dots_on_a_selected_row_keep_the_whole_selection() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let name = window.by_test_id("player-name-1").expect("the first player's name is drawn");
    window.click(name).expect("the row takes a click");
    let name = window.by_test_id("player-name-2").expect("the second player's name is drawn");
    window.click_with(name, zgui::vocab::Modifiers::PRIMARY).expect("the row takes a click");
    window.advance(Duration::from_millis(100));
    let selected = |window: &mut HeadlessWindow| {
        let rows = window.by_role(Role::Row);
        rows.into_iter().filter(|row| window.a11y(*row).and_then(|state| state.selected) == Some(true)).count()
    };
    assert_eq!(selected(&mut window), 2);

    window.hover(name).expect("the row takes the pointer");
    window.advance(Duration::from_millis(100));
    let dots = window.by_test_id("player-actions-2").expect("the row has its actions");
    window.click(dots).expect("the dots take a click");
    window.advance(Duration::from_millis(300));
    let copy_row = t!("ui.replay.context.copy_row").into_owned();
    assert!(window.by_role_and_label(Role::MenuItem, &copy_row).is_some(), "the dots open the row menu");
    assert_eq!(selected(&mut window), 2, "the dots leave the selection as it was");
}

#[test]
fn the_dots_name_their_menu_except_while_it_is_open() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let past_delay = landaire_ui::TOOLTIP_DELAY + Duration::from_millis(150);
    let hint = t!("ui.replay.row_actions_hint").into_owned();

    let name = window.by_test_id("player-name-1").expect("the first player's name is drawn");
    window.hover(name).expect("the row takes the pointer");
    window.advance(Duration::from_millis(100));
    let dots = window.by_test_id("player-actions-1").expect("the row has its actions");
    window.hover(dots).expect("the dots take the pointer");
    window.advance(past_delay);
    assert_eq!(window.texts_by_class("lui-tooltip"), [hint], "resting on the dots says what they do");

    window.click(dots).expect("the dots take a click");
    window.advance(Duration::from_millis(300));
    let copy_row = t!("ui.replay.context.copy_row").into_owned();
    assert!(window.by_role_and_label(Role::MenuItem, &copy_row).is_some(), "the dots open the row menu");
    window.hover(name).expect("the row takes the pointer");
    window.advance(Duration::from_millis(100));
    window.hover(dots).expect("the dots take the pointer");
    window.advance(past_delay);
    assert!(window.texts_by_class("lui-tooltip").is_empty(), "the open menu is not covered by the tip");
}

#[test]
fn each_twitch_login_in_the_menu_is_marked_as_a_copy() {
    let mut model = sample_model();
    let logins = ["harvey635", "harvey_635"];
    let row = model.rows.iter_mut().find(|row| row.db_id.raw() == 1).expect("the sample has player 1");
    row.twitch_candidates =
        logins.iter().map(|login| SniperCandidate { login: (*login).to_owned(), minutes: vec![3] }).collect();
    let (mut window, _) = open_table_over(Drawing::Recorded, Scheme::Dark, model, false);

    let chip = window.by_test_id("player-twitch").expect("the row has its Twitch chip");
    window.click(chip).expect("the chip takes a click");
    window.advance(Duration::from_millis(300));
    let copies: Vec<_> =
        window.all_by_attribute("data-icon", "copy").into_iter().filter_map(|icon| window.bounds(icon)).collect();
    for login in logins {
        let item = window.by_role_and_label(Role::MenuItem, login).expect("the menu offers each login");
        let item = window.bounds(item).expect("the item has a box");
        assert!(copies.iter().any(|icon| item.contains(icon.origin)), "{login} shows the copy icon");
    }
}

#[test]
fn without_a_detail_no_row_offers_to_open() {
    let (mut window, expanded) = open_table_over(Drawing::Recorded, Scheme::Dark, sample_model(), false);
    assert!(window.by_test_id("player-caret-2").is_none(), "no disclosure without a detail");

    let cell = kills_cell(&window, 2);
    window.double_click(cell).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    assert!(expanded.with_untracked(HashSet::is_empty), "no row opens");
}

#[test]
fn an_open_detail_follows_a_column_pinned_after_it_opened() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let cell = kills_cell(&window, 2);
    window.double_click(cell).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    let kills_key = ReplayColumn::Kills.storage_key();
    let part = format!("detail-2-{kills_key}");
    assert!(window.by_test_id(&part).is_some(), "the row's detail is open");

    let kills = ReplayColumn::Kills.label();
    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    window.right_click(header).expect("the header takes a right click");
    window.advance(Duration::from_millis(300));
    let pin = t!("ui.replay.context.pin_column").into_owned();
    let item = window.by_role_and_label(Role::MenuItem, &pin).expect("the header menu offers to pin");
    window.click(item).expect("the item takes a click");
    window.advance(Duration::from_millis(600));

    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    let header = window.bounds(header).expect("the header has a box");
    let part = window.bounds(window.by_test_id(&part).expect("the part is drawn")).expect("the part has a box");
    assert!((part.origin.x.0 - header.origin.x.0).abs() < 4.0, "Kills' part stays under Kills");
    assert!(part.origin.y.0 > header.origin.y.0, "and below it");
}

#[test]
fn a_rows_stripe_follows_its_team_over_its_relation() {
    let mut model = sample_model();
    let self_team = model.self_team;
    let row = model.rows.iter_mut().find(|row| row.team_id == self_team && !row.is_self).expect("an ally");
    row.relation = wows_replays::types::Relation::new(2);
    let allies = model.rows.iter().filter(|row| row.team_id == self_team && !row.is_self).count();
    let enemies = model.rows.iter().filter(|row| row.team_id != self_team).count();

    let (window, _) = open_table_over(Drawing::Recorded, Scheme::Dark, model, true);
    assert_eq!(window.all_by_attribute("data-mark", "ally").len(), allies, "the stripe shows the row's team");
    assert_eq!(window.all_by_attribute("data-mark", "enemy").len(), enemies);
    assert_eq!(window.all_by_attribute("data-mark", "owner").len(), 1);
}

#[test]
fn a_rows_stripe_runs_unbroken_down_the_table() {
    let (window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let mut stripes: Vec<_> = ["owner", "ally", "enemy"]
        .into_iter()
        .flat_map(|mark| window.all_by_attribute("data-mark", mark))
        .filter_map(|stripe| window.bounds(stripe))
        .collect();
    stripes.sort_by(|a, b| a.origin.y.0.total_cmp(&b.origin.y.0));
    assert!(stripes.len() > 2, "every row has a stripe");
    let breaks: Vec<f32> = stripes
        .windows(2)
        .map(|pair| pair[1].origin.y.0 - (pair[0].origin.y.0 + pair[0].size.height.0))
        .filter(|gap| gap.abs() >= 0.5)
        .collect();
    // Only the heading over the second team stands between two stripes.
    assert!(breaks.len() <= 1, "neighbouring stripes meet: {breaks:?}");
}

#[test]
fn a_disconnect_chips_text_reads_on_its_ground_at_every_depth() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let palette = scheme.palette();
        for depth in [0.0, 0.5, 1.0] {
            let ground = cells::offline_ground(palette, depth);
            let ratio = landaire_ui::ContrastRatio::between(landaire_ui::text_on(ground), ground);
            assert!(ratio >= landaire_ui::ContrastRatio::READABLE, "{scheme:?} at {depth}: {}", ratio.value());
        }
    }
}

#[test]
fn each_team_opens_under_its_heading_while_the_sort_keeps_it_together() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let top = |window: &HeadlessWindow, id: &str| {
        let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is drawn"));
        window.bounds(element).expect("drawn").origin.y.0
    };
    let first = format!("player-name-{}", row_order(&window)[0]);
    assert!(top(&window, "team-heading-ally") < top(&window, &first), "the first heading is over the first row");
    assert!(top(&window, "team-heading-ally") < top(&window, "team-heading-enemy"));

    let kills = ReplayColumn::Kills.label();
    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    window.click(header).expect("the header takes a click");
    window.advance(Duration::from_millis(300));
    let mut model = sample_model();
    sort_rows(&mut model.rows, model.self_team, SortOrder::Desc(SortColumn::Kills), false);
    let together = wows_toolkit_replay_data::team_totals::team_runs(&model.rows, model.self_team).is_some();
    assert_eq!(window.by_test_id("team-heading-ally").is_some(), together, "headings only over whole teams");
}

#[test]
fn a_disconnect_chip_deepens_from_thirty_to_ninety_seconds() {
    let offline = |secs| wows_replay_insights::battle_report::TimeOffline::new(Duration::from_secs(secs));

    assert_eq!(cells::offline_depth(offline(20)), 0.0);
    assert_eq!(cells::offline_depth(offline(60)), 0.5);
    assert_eq!(cells::offline_depth(offline(120)), 1.0);
    assert_eq!(cells::offline_label(offline(72)), "1m 12s");
    assert_eq!(cells::offline_label(offline(45)), "45s");
}

/// A match contribution with every part filled in, adding up to `mc`.
pub(crate) fn sample_match_contribution(mc: f64) -> MatchContributionResult {
    let score = |factor: MatchContributionFactor, share: f64| FactorScore { points: mc * share, basis: factor.basis() };
    let components = MatchContributionComponents {
        damage: score(MatchContributionFactor::Damage, 0.4),
        kills: score(MatchContributionFactor::Kills, 0.2),
        team_damage_share: Some(score(MatchContributionFactor::TeamDamageShare, 0.15)),
        tanking: Some(score(MatchContributionFactor::Tanking, 0.1)),
        spotting: Some(score(MatchContributionFactor::Spotting, 0.1)),
        objectives: Some(score(MatchContributionFactor::Objectives, 0.05)),
        anti_air: None,
        torpedo_warnings: None,
        critical_bursts: None,
    };
    let inputs = MatchContributionInputs {
        weighted_damage: 84_000.0,
        expected_damage: 61_000.0,
        kills: 2.0,
        expected_kills: 0.85,
        team_damage_share: Some(TeamDamageShare { share: Share(0.18), even: Share(0.083) }),
        potential_damage: 1_250_000,
        tanking: Some(TankingRatios { class: Ratio(1.6), class_peers: 5, team: Ratio(1.4) }),
        spotting_damage: Some(32_000),
        spotting_baseline: 21_000.0,
        ships_spotted: 2.4,
        ships_spotted_baseline: 1.8,
        objective_points: None,
        planes_killed: None,
        anti_air_damage: None,
    };

    MatchContributionResult {
        mc,
        category: PersonalRatingCategory::from_pr(mc),
        version: MatchContributionVersion::CURRENT,
        components,
        inputs,
        torpedo_detection: None,
        critical_bursts: None,
    }
}

/// The sample battle with the self row rated on both scales, each keeping its parts.
fn rated_model() -> ReplayReportModel {
    let mut model = split_damage(sample_model());
    let table = fixture_personal_rating_data();
    let row = model.rows.iter_mut().find(|row| row.is_self).expect("the sample has a self row");
    row.personal_rating = rate_single_battle(&table, FIXTURE_PR_SHIP_ID.into(), 72_000, 2, true);
    row.match_contribution = Some(sample_match_contribution(1_480.0));

    model
}

/// The self row's cell in `column`.
fn self_cell(window: &HeadlessWindow, column: ReplayColumn) -> Element {
    let id = sample_model().rows.iter().find(|row| row.is_self).expect("the sample has a self row").db_id.raw();
    let name = window.by_test_id(&format!("player-name-{id}")).expect("the player's name is drawn");
    let line = window.bounds(name).expect("the name has a box");
    let middle = line.origin.y.0 + line.size.height.0 / 2.0;
    window
        .all_by_attribute("data-column", column.storage_key())
        .into_iter()
        .find(|cell| {
            window
                .bounds(*cell)
                .is_some_and(|cell| cell.origin.y.0 <= middle && middle < cell.origin.y.0 + cell.size.height.0)
        })
        .expect("the self row has the cell")
}

#[test]
fn a_rating_cell_lists_its_parts_largest_first_on_hover() {
    let model = rated_model();
    let row = model.rows.iter().find(|row| row.is_self).cloned().expect("the sample has a self row");
    let (mut window, _) = open_table_over(Drawing::Recorded, Scheme::Dark, model, false);

    let cases = [
        (
            ReplayColumn::MatchContribution,
            "mc",
            row.match_contribution.as_ref().map(|mc| RatingBreakdown::match_contribution(mc, None)),
        ),
        (
            ReplayColumn::PersonalRating,
            "pr",
            row.personal_rating.as_ref().and_then(|pr| RatingBreakdown::personal_rating(pr, None)),
        ),
    ];
    for (column, kind, breakdown) in cases {
        let breakdown = breakdown.expect("the rating keeps its parts");
        let cell = self_cell(&window, column);
        window.hover(cell).expect("the cell has a box");
        window.advance(Duration::from_millis(800));

        assert!(window.by_test_id(&format!("rating-{kind}-hover")).is_some(), "the {kind} hover opens");
        let labels: Vec<String> = window.texts_by_class("wt-rh__label");
        let expected: Vec<String> = breakdown.lines.iter().map(|line| line.label.clone()).collect();
        assert_eq!(labels, expected, "one line per part, largest first");

        window.hover_at(Point::new(DevicePx(1.0), DevicePx(1.0)));
        window.advance(Duration::from_millis(400));
    }
}

/// The text of the plain hover over `column`'s cell on the self row.
fn plain_hover(window: &mut HeadlessWindow, column: ReplayColumn) -> Vec<String> {
    let cell = self_cell(window, column);
    window.hover(cell).expect("the cell has a box");
    window.advance(Duration::from_millis(800));

    window.texts_by_class("wt-pt-hover")
}

#[test]
fn a_hidden_rows_rating_names_only_its_band() {
    let mut model = rated_model();
    let row = model.rows.iter_mut().find(|row| row.is_self).expect("the sample has a self row");
    row.manual_stat_hide_toggle = true;
    let band = row.match_contribution.as_ref().expect("the row is rated").category.name().to_owned();
    let (mut window, _) = open_table_over(Drawing::Recorded, Scheme::Dark, model, false);

    let hover = plain_hover(&mut window, ReplayColumn::MatchContribution);
    assert!(window.by_test_id("rating-mc-hover").is_none(), "the parts would show the hidden inputs");
    assert_eq!(hover, [band]);
}

#[test]
fn a_withheld_match_contribution_says_why_it_is_missing() {
    for reason in [MatchContributionWithheld::EarlyExit, MatchContributionWithheld::UnratedBattle] {
        let mut model = sample_model();
        model.match_contribution_withheld = Some(reason);
        let (mut window, _) = open_table_over(Drawing::Recorded, Scheme::Dark, model, false);

        let hover = plain_hover(&mut window, ReplayColumn::MatchContribution);
        assert_eq!(hover, [reason.explanation()], "{reason:?}");
    }
}

/// Writes the table in both appearances, with one row open, and the row part way open as its
/// detail fades in, to `target/zgui-shots`.
#[test]
#[ignore = "needs a graphics device"]
fn the_player_table_renders_in_dark_and_light() {
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots");
    for (scheme, name) in [(Scheme::Dark, "player-table-dark"), (Scheme::Light, "player-table-light")] {
        let (mut window, expanded) = open_table(Drawing::Device, scheme);
        expanded.set(HashSet::from([sample_row(1)]));
        window.advance(Duration::from_millis(600));

        window.save_png(&shots.join(format!("{name}.png"))).expect("the window is drawn to a PNG");
    }

    let (mut window, expanded) = open_table(Drawing::Device, Scheme::Dark);
    expanded.set(HashSet::from([sample_row(1)]));
    window.advance(Duration::from_millis(80));
    window.save_png(&shots.join("player-table-opening-dark.png")).expect("the window is drawn to a PNG");
}

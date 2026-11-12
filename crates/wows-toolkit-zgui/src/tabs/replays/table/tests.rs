use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::MotionPreference;
use landaire_ui::Scheme;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_replay_data::damage_mix::DamageKind;
use wows_toolkit_replay_data::damage_mix::DamageLine;
use wows_toolkit_replay_data::sample::sample_model;
use wows_toolkit_replay_data::sort::SortColumn;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use zgui::reactive::Signal;
use zgui::reactive::provide_local_context;
use zgui::vocab::Role;

use super::*;
use crate::services::saved_for_test;
use crate::theme::SemanticTokensProps;

/// Rows tall enough to hold the sample's players in one view.
const SIZE: WindowSize = WindowSize { width: 1280.0, height: 720.0 };

/// The table over `model`, as the Replays tab holds it. With `with_detail`, each open row's detail
/// is a box naming the player and the third column drawn.
#[component]
fn Harness(
    scheme: Scheme,
    expanded: RwSignal<HashSet<AccountId>, LocalStorage>,
    model: Rc<ReplayReportModel>,
    with_detail: bool,
) -> impl IntoView {
    Services::provide(None);
    let model = Signal::stored_local(model);
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
            return AnyView::new(view! { PlayerTable(model = model, debug = debug, expanded = expanded) });
        }
        let detail: PlayerDetail = Rc::new(|cx: PlayerDetailCx| {
            let id = format!("detail-{}", cx.row.db_id.raw());
            let name = cx.row.display_name.clone();
            let columns = cx.columns;
            let third = move || columns.get().get(2).map(|col| col.storage_key().to_owned());
            AnyView::new(view! {
                box(attr:data-testid = id, attr:data-third-column = third, style:padding = "8px") {{name}}
            })
        });
        AnyView::new(view! { PlayerTable(model = model, debug = debug, detail = detail, expanded = expanded) })
    };

    view! {
        ThemeRoot(choice = Signal::stored_local(landaire_ui::ThemeChoice::Fixed(scheme))) {
            SemanticTokens(choice = Signal::stored_local(theme))
            ToastHost {
                {table}
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

fn open_table(drawing: Drawing, scheme: Scheme) -> (HeadlessWindow, RwSignal<HashSet<AccountId>, LocalStorage>) {
    open_table_over(drawing, scheme, split_damage(sample_model()), true)
}

fn open_table_over(
    drawing: Drawing,
    scheme: Scheme,
    model: ReplayReportModel,
    with_detail: bool,
) -> (HeadlessWindow, RwSignal<HashSet<AccountId>, LocalStorage>) {
    let expanded = RwSignal::new_local(HashSet::new());
    let model = Rc::new(model);
    let options = HeadlessOptions { drawing, desktop: Some(Appearance::Dark), ..HeadlessOptions::new(SIZE) };
    let mut window = HeadlessWindow::open(options, move || {
        view! { Harness(scheme = scheme, expanded = expanded, model = Rc::clone(&model), with_detail = with_detail) }
    })
    .expect("the table opens headlessly");
    window.advance(Duration::from_millis(300));

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
fn a_double_click_opens_a_rows_detail_and_its_disclosure_closes_it() {
    let (mut window, expanded) = open_table(Drawing::Recorded, Scheme::Dark);
    let name = window.by_test_id("player-name-2").expect("the second player's name is drawn");

    window.double_click(name).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    assert!(window.by_test_id("detail-2").is_some(), "the row's detail is open");

    let caret = window.by_test_id("player-caret-2").expect("the row has a disclosure");
    window.click(caret).expect("the disclosure takes a click");
    window.advance(Duration::from_millis(600));
    assert!(expanded.with_untracked(HashSet::is_empty), "the disclosure closed the row");
    assert!(window.by_test_id("detail-2").is_none(), "the detail is gone once it has closed");
}

#[test]
fn pinning_a_column_from_its_header_is_saved_under_the_gpui_key() {
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
fn without_a_detail_no_row_offers_to_open() {
    let (mut window, expanded) = open_table_over(Drawing::Recorded, Scheme::Dark, sample_model(), false);
    assert!(window.by_test_id("player-caret-2").is_none(), "no disclosure without a detail");

    let name = window.by_test_id("player-name-2").expect("the second player's name is drawn");
    window.double_click(name).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    assert!(expanded.with_untracked(HashSet::is_empty), "no row opens");
}

#[test]
fn an_open_detail_follows_a_column_pinned_after_it_opened() {
    let (mut window, _) = open_table(Drawing::Recorded, Scheme::Dark);
    let name = window.by_test_id("player-name-2").expect("the second player's name is drawn");
    window.double_click(name).expect("the row takes a double click");
    window.advance(Duration::from_millis(400));
    assert!(window.by_test_id("detail-2").is_some(), "the row's detail is open");
    let kills_key = ReplayColumn::Kills.storage_key();
    assert!(window.by_attribute("data-third-column", kills_key).is_none());

    let kills = ReplayColumn::Kills.label();
    let header = window.by_role_and_label(Role::ColumnHeader, &kills).expect("the Kills header is drawn");
    window.right_click(header).expect("the header takes a right click");
    window.advance(Duration::from_millis(300));
    let pin = t!("ui.replay.context.pin_column").into_owned();
    let item = window.by_role_and_label(Role::MenuItem, &pin).expect("the header menu offers to pin");
    window.click(item).expect("the item takes a click");
    window.advance(Duration::from_millis(300));

    assert!(window.by_attribute("data-third-column", kills_key).is_some(), "the detail sees Kills pinned third");
}

#[test]
fn a_rows_stripe_follows_its_team_over_its_relation() {
    let mut model = sample_model();
    let self_team = model.self_team;
    let row = model.rows.iter_mut().find(|row| row.team_id == self_team && !row.is_self).expect("an ally");
    row.relation = wows_replays::types::Relation::new(2);
    let id = row.db_id.raw();

    let (window, _) = open_table_over(Drawing::Recorded, Scheme::Dark, model, true);
    assert!(window.by_test_id(&format!("player-stripe-{id}-ally")).is_some(), "the stripe shows the row's team");
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

/// Writes the table in both appearances, with one row open, to `target/zgui-shots`.
#[test]
#[ignore = "needs a graphics device"]
fn the_player_table_renders_in_dark_and_light() {
    for (scheme, name) in [(Scheme::Dark, "player-table-dark"), (Scheme::Light, "player-table-light")] {
        let (mut window, expanded) = open_table(Drawing::Device, scheme);
        expanded.set(HashSet::from([AccountId::from(1i64)]));
        window.advance(Duration::from_millis(600));

        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"));
        window.save_png(&path).expect("the window is drawn to a PNG");
    }
}

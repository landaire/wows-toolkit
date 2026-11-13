use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::test_support::base_row;
use wows_toolkit_replay_data::test_support::fire_chance_result;
use wows_toolkit_viewmodel::breakdown::BreakdownRow;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Set;
use zgui::view;

use super::DamageDirection;
use super::ExpandedRowProps;
use super::damage_event_rows;
use super::group_ribbons;
use super::has_column_detail;
use super::interaction_amount;
use super::interaction_percentage;
use super::reorder_bulge_after_main_caliber;
use super::share_text;
use super::shows_damage_shares;
use super::spotting_target_rows;
use crate::shell::ShellHarnessProps;

/// The block expands under Name, after the damage events; Kills has nothing of its own.
#[test]
fn the_fire_chance_block_expands_under_the_name_column() {
    let mut row = base_row(1, Relation::new(0), true);
    row.fire_chance = Some(fire_chance_result());

    assert!(has_column_detail(ReplayColumn::Name, &row, &[], false), "the Name column carries it");
    assert!(!has_column_detail(ReplayColumn::Kills, &row, &[], false), "and the Kills column does not");
}

/// The grid draws a line per kind of damage event the battle produced, and none for the rest.
#[test]
fn the_damage_event_grid_lists_only_the_events_a_row_recorded() {
    let mut row = base_row(1, Relation::new(1), false);
    row.fires = Some(4);
    row.citadels = Some(2);

    let rows = damage_event_rows(&row);

    assert_eq!(rows.len(), 2, "got {rows:?}");
    assert_eq!(rows[1].value, "2", "the counts read as numbers: {rows:?}");
}

fn ribbon(name: &str) -> RibbonResult {
    RibbonResult {
        name: name.to_string(),
        display_name: name.to_string(),
        description: None,
        icon_key: name.to_string(),
        is_subribbon: false,
        count: 1,
    }
}

/// Ribbons that stand alone come first; a ribbon with sub-ribbons is kept with them.
#[test]
fn ribbons_with_sub_ribbons_follow_the_standalone_ones() {
    let sub = |name: &str| RibbonResult { is_subribbon: true, ..ribbon(name) };
    let (fire, main, pen, ricochet, spotted, bulge) = (
        ribbon("RIBBON_BURN"),
        ribbon("RIBBON_MAIN_CALIBER"),
        sub("RIBBON_MAIN_CALIBER_PENETRATION"),
        sub("RIBBON_MAIN_CALIBER_RICOCHET"),
        ribbon("RIBBON_SPOTTED"),
        sub("RIBBON_BULGE"),
    );
    let groups = group_ribbons(vec![&fire, &main, &pen, &bulge, &ricochet, &spotted]);
    let names = |ribbons: &[&RibbonResult]| ribbons.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&groups.standalone), ["RIBBON_BURN", "RIBBON_SPOTTED"]);
    assert_eq!(groups.families.len(), 1);
    assert_eq!(
        names(&groups.families[0]),
        ["RIBBON_MAIN_CALIBER", "RIBBON_MAIN_CALIBER_PENETRATION", "RIBBON_BULGE", "RIBBON_MAIN_CALIBER_RICOCHET"]
    );
}

#[test]
fn the_bulge_ribbon_follows_the_main_caliber_one_wherever_it_started() {
    let bulge = ribbon("RIBBON_BULGE");
    let main = ribbon("RIBBON_MAIN_CALIBER");
    let other = ribbon("RIBBON_CITADEL");
    let names = |ribbons: &[&RibbonResult]| ribbons.iter().map(|r| r.name.as_str().to_owned()).collect::<Vec<_>>();

    let mut before = vec![&bulge, &main, &other];
    reorder_bulge_after_main_caliber(&mut before);
    assert_eq!(names(&before), ["RIBBON_MAIN_CALIBER", "RIBBON_BULGE", "RIBBON_CITADEL"]);

    let mut after = vec![&main, &other, &bulge];
    reorder_bulge_after_main_caliber(&mut after);
    assert_eq!(names(&after), ["RIBBON_MAIN_CALIBER", "RIBBON_BULGE", "RIBBON_CITADEL"]);

    let mut absent = vec![&other, &main];
    reorder_bulge_after_main_caliber(&mut absent);
    assert_eq!(names(&absent), ["RIBBON_CITADEL", "RIBBON_MAIN_CALIBER"], "nothing moves without a bulge ribbon");
}

fn interaction(dealt: u64, dealt_pct: f64, received: u64, received_pct: f64) -> DamageInteraction {
    DamageInteraction {
        damage_dealt: dealt,
        damage_dealt_percentage: dealt_pct,
        damage_received: received,
        damage_received_percentage: received_pct,
        ..Default::default()
    }
}

#[test]
fn each_direction_reads_its_own_amount_and_share_and_alt_inverts_the_share() {
    let mut i = interaction(42_000, 56.4, 12_000, 38.1);
    i.damage_dealt_inverse_percentage = 71.2;
    i.damage_received_inverse_percentage = 9.4;

    assert_eq!(interaction_amount(&i, DamageDirection::Dealt), 42_000);
    assert_eq!(interaction_amount(&i, DamageDirection::Received), 12_000);
    assert!((interaction_percentage(&i, DamageDirection::Dealt, false) - 56.4).abs() < 1e-9);
    assert!((interaction_percentage(&i, DamageDirection::Received, false) - 38.1).abs() < 1e-9);
    assert!((interaction_percentage(&i, DamageDirection::Dealt, true) - 71.2).abs() < 1e-9);
    assert!((interaction_percentage(&i, DamageDirection::Received, true) - 9.4).abs() < 1e-9);
}

/// Each mode says whose total the share is of, and the hint says how to reach the other mode.
#[test]
fn a_damage_share_explains_itself_in_each_mode() {
    rust_i18n::set_locale("en");
    let said = |direction, inverted| share_text(direction, inverted, 34.4, "Yamato", "Iowa");

    assert_eq!(
        said(DamageDirection::Dealt, false),
        "34% of the damage Yamato dealt went to Iowa.\nHold Alt to invert."
    );
    assert_eq!(
        said(DamageDirection::Dealt, true),
        "34% of the damage Iowa received came from Yamato.\nRelease Alt to invert back."
    );
    assert_eq!(
        said(DamageDirection::Received, false),
        "34% of the damage Yamato received came from Iowa.\nHold Alt to invert."
    );
    assert_eq!(
        said(DamageDirection::Received, true),
        "34% of the damage Iowa dealt went to Yamato.\nRelease Alt to invert back."
    );
}

/// Alt is worth naming only while it changes something: an open row with damage shares under a
/// shown damage column.
#[test]
fn alt_names_a_mode_only_over_damage_shares_on_screen() {
    let mut open = base_row(1, Relation::new(0), true);
    open.damage_interactions = Some([(AccountId(2), interaction(1_000, 10.0, 0, 0.0))].into_iter().collect());
    let bare = base_row(3, Relation::new(1), false);
    let damage = [ReplayColumn::ActualDamage];

    assert!(shows_damage_shares([&open], damage, false));
    assert!(!shows_damage_shares([&bare], damage, false), "the open row has no shares");
    assert!(!shows_damage_shares([], damage, false), "no row is open");
    assert!(!shows_damage_shares([&open], [ReplayColumn::Kills], false), "no damage column is shown");
}

/// Largest share first; a victim with no spotting damage, or one no row names, gets no line.
#[test]
fn spotting_lines_list_named_victims_by_amount() {
    let spotted = |spotting_damage: u64, spotting_damage_percentage: Option<f64>| DamageInteraction {
        spotting_damage,
        spotting_damage_percentage,
        ..Default::default()
    };
    let mut row = base_row(1, Relation::new(0), true);
    row.damage_interactions = Some(
        [
            (AccountId(2), spotted(1_000, Some(20.0))),
            (AccountId(3), spotted(4_000, Some(80.0))),
            (AccountId(4), spotted(0, None)),
            (AccountId(9), spotted(500, Some(10.0))),
        ]
        .into_iter()
        .collect(),
    );
    let named =
        |id: i64, ship: &str| PlayerRow { ship_name: ship.to_string(), ..base_row(id, Relation::new(2), false) };

    assert_eq!(
        spotting_target_rows(&row, &[named(2, "Low"), named(3, "High"), named(4, "None")]),
        vec![
            BreakdownRow { label: "High".to_string(), value: "4,000 (80%)".to_string() },
            BreakdownRow { label: "Low".to_string(), value: "1,000 (20%)".to_string() },
        ]
    );

    // Without a spotting total there is no share to show.
    row.damage_interactions = Some([(AccountId(3), spotted(4_000, None))].into_iter().collect());
    assert_eq!(
        spotting_target_rows(&row, &[named(3, "High")]),
        vec![BreakdownRow { label: "High".to_string(), value: "4,000".to_string() }]
    );
}

fn open_detail(
    row: PlayerRow,
    columns: Vec<ReplayColumn>,
    debug: bool,
) -> (HeadlessWindow, RwSignal<bool, zgui::reactive::LocalStorage>) {
    let alt = Rc::new(std::cell::Cell::new(None));
    let handed = Rc::clone(&alt);
    let window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 900.0, height: 600.0 }), move || {
        let alt_held = RwSignal::new_local(false);
        handed.set(Some(alt_held));
        let row = Arc::new(row.clone());
        let rows: super::Rows = Arc::from(vec![(*row).clone()]);
        let columns: Rc<[ReplayColumn]> = Rc::from(columns.clone());
        view! {
            ShellHarness(theme = ThemeChoice::Dark) {
                ExpandedRow(columns = columns, row = row, rows = rows, debug = debug, alt_held = alt_held)
            }
        }
    })
    .expect("the detail opens");
    (window, alt.get().expect("the view was built"))
}

/// Each achievement is one tile, and only one earned more than once carries a count.
#[test]
fn achievements_are_tiles_counted_only_above_one() {
    let achievement = |name: &str, count| AchievementResult {
        name: name.to_string(),
        display_name: name.to_string(),
        description: String::new(),
        icon_key: name.to_string(),
        count,
    };
    let mut row = base_row(1, Relation::new(0), true);
    row.achievements = vec![achievement("Kraken", 1), achievement("Fireproof", 3)];

    let (window, _) = open_detail(row, vec![ReplayColumn::Name], false);

    assert!(window.by_test_id("replay-achievement").is_some());
    assert!(window.by_attribute("data-count-of", "Fireproof").is_some(), "a repeated award has a count");
    assert!(window.by_attribute("data-count-of", "Kraken").is_none(), "a single award has none");
}

/// An enemy's build stays hidden unless debug mode lifts it.
#[test]
fn an_enemy_build_shows_only_in_debug_mode() {
    let mut row = base_row(2, Relation::new(2), false);
    row.skill_hover_text = Some("Captain".to_string());

    let (window, _) = open_detail(row.clone(), vec![ReplayColumn::Skills], false);
    assert!(window.by_test_id("replay-build").is_none());

    drop(window);
    let (window, _) = open_detail(row, vec![ReplayColumn::Skills], true);
    assert!(window.by_test_id("replay-build").is_some());
}

/// A row whose figures are under NDA shows the NDA line under its damage, not the breakdown.
#[test]
fn a_hidden_row_shows_no_damage_breakdown() {
    let mut row = base_row(2, Relation::new(2), false);
    row.is_test_ship = true;
    row.damage_interactions = Some([(AccountId(1), interaction(1_000, 10.0, 0, 0.0))].into_iter().collect());

    let (window, _) = open_detail(row, vec![ReplayColumn::ActualDamage], false);

    assert!(window.by_attribute("data-column", "ActualDamage").is_some(), "the column still shows its line");
    assert!(window.by_test_id("replay-damage-interactions").is_none());
}

/// Holding Alt flips every share to the other player's total.
#[test]
fn alt_flips_the_shares_shown() {
    let mut row = base_row(1, Relation::new(0), true);
    let mut shared = interaction(1_000, 10.0, 0, 0.0);
    shared.damage_dealt_inverse_percentage = 90.0;
    row.damage_interactions = Some([(AccountId(1), shared)].into_iter().collect());

    let (mut window, alt) = open_detail(row, vec![ReplayColumn::ActualDamage], false);
    assert!(window.by_label("1,000 (10%)").is_some(), "the plain share is of this row's total");

    alt.set(true);
    window.advance(std::time::Duration::from_millis(16));
    assert!(window.by_label("1,000 (90%)").is_some(), "Alt shows the other player's share");
}

//! The Search tab, driven headlessly through the shell.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::tabs::search::model::Search;

thread_local! {
    /// The last Search tab built on this thread, for the tests to drive.
    static BUILT: Cell<Option<Search>> = const { Cell::new(None) };
}

pub(super) fn publish(search: Search) {
    BUILT.with(|built| built.set(Some(search)));
}

pub(crate) fn built() -> Search {
    BUILT.with(Cell::get).expect("the Search tab was built")
}

fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(400));
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
    window.click(element).unwrap_or_else(|error| panic!("{id} can be clicked: {error:?}"));
    settle(window);
}

/// One result, so a row is drawn with every action available.
fn hit(path: &str, outcome: MatchOutcome, map: &str) -> MatchHit {
    MatchHit {
        arena_id: wows_replays::types::ArenaId::from(1i64),
        timestamp: jiff::Timestamp::from_second(1_760_000_000).expect("a timestamp in range"),
        map: map.to_string(),
        game_mode: "Domination".to_string(),
        game_mode_id: None,
        game_type: "RandomBattle".to_string(),
        match_group: "pvp".to_string(),
        version_build: Some(13_187_581),
        source_id: wows_toolkit_config::index::rows::SourceId(1),
        outcome,
        self_account_id: None,
        self_ship_id: None,
        self_ship_name: Some("Thunderer".to_string()),
        self_survived: Some(true),
        self_damage: Some(112_345),
        self_kills: Some(2),
        self_pr: Some(1543.0),
        results_available: true,
        replay_path: PathBuf::from(path),
        file_mtime: None,
    }
}

fn sample_hits() -> Vec<MatchHit> {
    vec![
        hit("/replays/one.wowsreplay", MatchOutcome::Win, "Okinawa"),
        hit("/replays/two.wowsreplay", MatchOutcome::Loss, "Ocean"),
        hit("/replays/three.wowsreplay", MatchOutcome::Draw, "Hotspot"),
    ]
}

/// Opens the shell on the Search tab with an empty bar. The saved row is read and run first:
/// every test in the process shares one config database, so what another test saved is cleared
/// rather than left to land under this one.
fn open_search(options: HeadlessOptions) -> HeadlessWindow {
    let mut window = open_shell(options);
    click(&mut window, "search");
    settle_in_real_time(&mut window);
    built().set_query_text(String::new());
    settle_in_real_time(&mut window);
    window
}

/// Opens the Search tab showing `hits`, all on disk.
fn open_with_hits(options: HeadlessOptions, hits: Vec<MatchHit>) -> (HeadlessWindow, Search) {
    let mut window = open_search(options);
    let search = built();
    let on_disk = vec![true; hits.len()];
    search.show_results_for_test(hits, on_disk);
    settle(&mut window);
    (window, search)
}

/// Typing a term and pressing Enter turns it into a pill, empties the box and remembers the query.
#[test]
fn enter_turns_the_typed_term_into_a_pill() {
    let mut window = open_search(HeadlessOptions::new(REVIEW_SIZE));
    let search = built();
    click(&mut window, "search-query");
    window.type_text("map:ocean");
    settle(&mut window);
    window.press_key(NamedKey::Enter, Modifiers::NONE);
    settle(&mut window);

    assert_eq!(search.committed.get_untracked(), "map:ocean", "the term is a pill");
    assert!(search.pending.get_untracked().is_empty(), "the box is empty for the next term");
    assert!(window.by_test_id("search-pill-0").is_some(), "the pill is drawn");
    assert_eq!(search.history.get_untracked().first().map(String::as_str), Some("map:ocean"));
}

/// A structural edit can be undone and redone from the keyboard, and a query that does not parse
/// says where.
#[test]
fn a_pill_edit_undoes_and_redoes() {
    let mut window = open_search(HeadlessOptions::new(REVIEW_SIZE));
    let search = built();
    search.set_query_text("map:ocean outcome=win".to_owned());
    settle(&mut window);

    click(&mut window, "search-query");
    // Backspace in the empty box takes the pill before the caret.
    window.press_key(NamedKey::Backspace, Modifiers::NONE);
    settle(&mut window);
    let edited = search.full_query();
    assert!(!edited.contains("win"), "the last pill went: {edited}");
    assert!(search.can_undo());

    window.press_character('z', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(search.full_query().contains("win"), "undo puts it back: {}", search.full_query());
    assert!(search.can_redo());

    window.press_character('y', Modifiers::PRIMARY);
    settle(&mut window);
    assert_eq!(search.full_query(), edited, "redo takes it again");

    search.set_query_text("map:(".to_owned());
    settle(&mut window);
    assert!(window.by_test_id("search-parse-error").is_some(), "the parse error is shown");
}

/// Up walks back through the queries run, and Down returns to the text the walk started from.
#[test]
fn the_arrows_walk_the_history() {
    let mut window = open_search(HeadlessOptions::new(REVIEW_SIZE));
    let search = built();
    search.history.set(vec!["outcome=win".to_owned(), "map:ocean".to_owned()]);
    click(&mut window, "search-query");

    window.press_key(NamedKey::ArrowUp, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(search.full_query(), "outcome=win");
    window.press_key(NamedKey::ArrowUp, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(search.full_query(), "map:ocean");
    window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(search.full_query(), "", "the walk ends on the text it started from");
}

/// Typing offers completions; the arrows walk them and Enter takes the highlighted one into the
/// bar rather than running the query.
#[test]
fn a_completion_is_taken_with_the_keyboard() {
    let mut window = open_search(HeadlessOptions::new(REVIEW_SIZE));
    let search = built();
    click(&mut window, "search-query");
    window.type_text("ma");
    settle(&mut window);
    assert!(search.completions_open.get_untracked(), "typing opens the completions");
    assert!(window.by_test_id("search-completion-0").is_some(), "they are listed");

    let first = search.offered_untracked()[0].replacement.clone();
    window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    window.press_key(NamedKey::Enter, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(search.full_query(), first.trim(), "the highlighted row was taken");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(!search.completions_open.get_untracked(), "Escape closes the dropdown");
}

/// The palette's "Games I won" puts its query in the bar and shows the tab.
#[test]
fn the_palette_seeds_a_search() {
    let mut window = open_shell(HeadlessOptions::new(REVIEW_SIZE));
    window.press_character('k', Modifiers::PRIMARY);
    settle(&mut window);
    click(&mut window, "palette-item-games-i-won");

    assert!(tab_selected(&mut window, "search"), "the Search tab is shown");
    assert_eq!(built().committed.get_untracked(), "outcome:win");
}

/// A double click opens the battle in the replays tab; the row's dots open its menu instead.
#[test]
fn double_clicking_a_result_opens_it() {
    let (mut window, _search) = open_with_hits(HeadlessOptions::new(REVIEW_SIZE), sample_hits());
    let other = window.by_attribute("data-outcome", "loss").expect("the second row's outcome cell is drawn");
    window.click(other).expect("the row can be clicked");
    settle(&mut window);
    assert!(!tab_selected(&mut window, "replay-inspector"), "one click does not open the battle");

    let row = window.by_attribute("data-outcome", "draw").expect("the third row is drawn");
    window.double_click(row).expect("the row can be double clicked");
    settle(&mut window);
    assert!(tab_selected(&mut window, "replay-inspector"), "the replays tab is shown");
}

/// Whether the tab strip shows the tab `name` as the selected one.
fn tab_selected(window: &mut HeadlessWindow, name: &str) -> bool {
    let tab = window.by_test_id(name).unwrap_or_else(|| panic!("the {name} tab is on the strip"));
    window.a11y(tab).and_then(|state| state.selected) == Some(true)
}

/// A header press re-sorts; the first press on another column starts it on its own direction.
#[test]
fn a_header_press_sorts_by_that_column() {
    let (mut window, search) = open_with_hits(HeadlessOptions::new(REVIEW_SIZE), sample_hits());
    let header = window.by_label("Map").expect("the Map header is drawn");
    window.click(header).expect("the header can be clicked");
    settle(&mut window);
    assert_eq!(search.sort.get_untracked().column, SortColumn::Map);
}

/// Pointing at a row starts its preview, and leaving the results ends it.
#[test]
fn hovering_a_row_previews_that_battle() {
    let (mut window, _search) = open_with_hits(HeadlessOptions::new(REVIEW_SIZE), sample_hits());
    let cell = window.by_attribute("data-outcome", "loss").expect("the second row is drawn");
    window.hover(cell).expect("the row can be hovered");
    settle(&mut window);
    let card = window.by_test_id("hover-preview").expect("the card is up beside the row");
    let placed = window.bounds(card).expect("the card has a box");
    let row = window.bounds(cell).expect("the cell has a box");
    let clear = placed.origin.y.0 >= row.origin.y.0 + row.size.height.0
        || placed.origin.y.0 + placed.size.height.0 <= row.origin.y.0;
    assert!(clear, "the card leaves its row uncovered: card {placed:?}, row {row:?}");

    let dots = window.by_test_id("search-row-actions-1").expect("the row carries its dots");
    window.hover(dots).expect("the dots can be hovered");
    settle(&mut window);
    let card = window.by_test_id("hover-preview").expect("the card stays up along the row");
    assert_eq!(window.bounds(card).map(|moved| moved.origin), Some(placed.origin), "the card holds still");

    let bar = window.by_test_id("search-bar").expect("the bar is drawn");
    window.hover(bar).expect("the bar can be hovered");
    settle(&mut window);
    assert!(window.by_test_id("hover-preview").is_none(), "leaving the results ends the preview");
}

/// Moving from a row to the header leaves the results' rows, and the preview with them.
#[test]
fn moving_from_a_row_to_the_header_ends_the_preview() {
    let (mut window, _search) = open_with_hits(HeadlessOptions::new(REVIEW_SIZE), sample_hits());
    let cell = window.by_attribute("data-outcome", "loss").expect("the second row is drawn");
    window.hover(cell).expect("the row can be hovered");
    settle(&mut window);
    assert!(window.by_test_id("hover-preview").is_some(), "the card is up for the row");

    let header = window.by_label("Map").expect("the Map header is drawn");
    window.hover(header).expect("the header can be hovered");
    settle(&mut window);
    assert!(window.by_test_id("hover-preview").is_none(), "the card went with the row");
}

#[test]
fn a_result_row_carries_the_dots_that_open_its_actions() {
    let (window, search) = open_with_hits(HeadlessOptions::new(REVIEW_SIZE), sample_hits());
    assert!(window.by_test_id("search-row-actions-0").is_some());
    assert_eq!(search.results.get().hits.len(), 3);
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn the_search_tab_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "search-dark"), (Appearance::Light, "search-light")] {
        let (mut window, search) = open_with_hits(review_options(appearance), sample_hits());
        search.set_query_text("map:ocean outcome=win".to_owned());
        click(&mut window, "search-query");
        window.type_text("ma");
        settle_in_real_time(&mut window);
        search.show_results_for_test(sample_hits(), vec![true, true, false]);
        settle(&mut window);
        println!("{}", save_shot(&mut window, name).display());
    }
}

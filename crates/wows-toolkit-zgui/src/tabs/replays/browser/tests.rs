use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::TreeModel;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_replay_data::replay_dir::DirChange;
use wows_toolkit_replay_data::replay_dir::read_replay;
use wows_toolkit_replay_data::simulated_watch;
use wows_toolkit_viewmodel::replay_folders::FolderHistory;
use wows_toolkit_viewmodel::replay_folders::PINNED_FOLDERS;
use wows_toolkit_viewmodel::replay_folders::PinnedFolders;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Modifiers;

use super::folders::BrowserControl;
use super::folders::FolderId;
use super::folders::FolderSource;
use super::folders::FolderState;
use super::folders::LANDING_PULSE;
use super::folders::MADE;
use super::listing::Listing;
use super::tree::BrowserTree;
use super::tree::NodeKey;
use super::tree::TreeInputs;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;

const FIXTURES: [&str; 3] = [
    "10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay",
    "10000_1767829698_20260107_182737_PJSB018-Yamato-1944_50_Gold_harbor.wowsreplay",
    "10000_1773241614_20260214_215653_PVSB719-Valparaiso_42_Neighbors.wowsreplay",
];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wows-replay-insights/tests/fixtures/replays").join(name)
}

/// A directory holding copies of the first `count` fixtures, by its canonical path.
fn replay_dir(count: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let root = std::fs::canonicalize(dir.path()).expect("the directory resolves");
    for name in &FIXTURES[..count] {
        std::fs::copy(fixture(name), root.join(name)).expect("the replay is copied");
    }

    (dir, root)
}

/// The shell, with the replays tab's browser and simulated directory watches.
fn open() -> (HeadlessWindow, BrowserControl) {
    simulated_watch::install();
    let window = open_shell(HeadlessOptions::new(WindowSize { width: 900.0, height: 700.0 }));
    let control = MADE.with(std::cell::Cell::get).expect("the replays tab made its browser");

    (window, control)
}

fn tabs(control: BrowserControl) -> Vec<(FolderSource, bool, FolderState)> {
    control
        .folders
        .with_untracked(|folders| folders.iter().map(|tab| (tab.source.clone(), tab.pinned, tab.state)).collect())
}

fn folder_id(control: BrowserControl, root: &Path) -> FolderId {
    control
        .folders
        .with_untracked(|folders| folders.iter().find(|tab| tab.source == FolderSource::Chosen(root.to_path_buf())).map(|tab| tab.id))
        .expect("the directory has a tab")
}

fn listing(control: BrowserControl, root: &Path) -> Listing {
    control.listing(folder_id(control, root)).expect("the tab has a listing")
}

fn listed(listing: Listing) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = listing.files.with_untracked(|files| files.iter().map(|file| file.path.clone()).collect());
    paths.sort();

    paths
}

fn row(window: &HeadlessWindow, path: &Path) -> landaire_ui::test_support::Element {
    window.by_test_id(&format!("replay-row-{}", path.display())).expect("the replay has a row")
}

/// Lands `name` in `root` and has the simulated watch report it.
fn land(root: &Path, name: &str, window: &mut HeadlessWindow) -> PathBuf {
    let path = root.join(format!("landed-{name}"));
    std::fs::copy(fixture(name), &path).expect("the replay is copied");
    simulated_watch::report(root, DirChange::Appeared(path.clone()));
    settle_in_real_time(window);

    path
}

#[test]
fn opening_a_folder_adds_or_selects_one_tab_and_the_install_stays_first() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let (_dir, root) = replay_dir(2);

    control.open_folder(root.clone());
    settle_in_real_time(&mut window);

    let shown = tabs(control);
    assert_eq!(shown[0].0, FolderSource::Install, "the install's tab stays first");
    assert_eq!(shown[1], (FolderSource::Chosen(root.clone()), false, FolderState::Listed));
    assert_eq!(control.selected.get_untracked(), folder_id(control, &root), "the opened folder is selected");
    assert_eq!(listed(listing(control, &root)).len(), 2, "both replays are listed");
    assert_eq!(control.history.with_untracked(|history| history.folders().to_vec()), [root.clone()]);

    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    assert_eq!(tabs(control).len(), 2, "opening it again selects its tab rather than adding one");
    assert_eq!(control.pinned_folders(), PinnedFolders::default(), "and it is not pinned");
}

#[test]
fn a_pinned_folder_is_watched_and_saved_in_tab_order() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_first_dir, first) = replay_dir(1);
    let (_second_dir, second) = replay_dir(1);
    control.open_folder(first.clone());
    control.open_folder(second.clone());
    settle_in_real_time(&mut window);

    control.pin_folder(folder_id(control, &first), true);
    control.pin_folder(folder_id(control, &second), true);
    settle_in_real_time(&mut window);

    assert_eq!(control.pinned_folders(), PinnedFolders(vec![first.clone(), second.clone()]), "kept in tab order");
    let saved = crate::services::saved_for_test();
    let last = saved.iter().rev().find(|(key, _)| *key == PINNED_FOLDERS).expect("the pins were saved");
    let stored: PinnedFolders = serde_json::from_str(&last.1).expect("the pins are JSON");
    assert_eq!(stored, PinnedFolders(vec![first.clone(), second]));
    assert_eq!(simulated_watch::watching(&first), 1, "a pinned folder is watched");

    control.pin_folder(folder_id(control, &first), false);
    settle_in_real_time(&mut window);
    assert_eq!(simulated_watch::watching(&first), 0, "unpinning stops the updates");
}

#[test]
fn a_new_arrival_is_tagged_until_opened_or_left() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(1);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    control.pin_folder(folder_id(control, &root), true);
    settle_in_real_time(&mut window);

    let opened = land(&root, FIXTURES[1], &mut window);
    let left = land(&root, FIXTURES[2], &mut window);
    let shown = listing(control, &root);
    assert!(shown.holds(&opened) && shown.holds(&left), "the watch lists both arrivals");
    assert!(shown.arrivals.with_untracked(|arrivals| arrivals.contains(&opened) && arrivals.contains(&left)));
    assert!(window.by_test_id("replay-row-new").is_some(), "an arrival carries the New tag");

    window.click(row(&window, &opened)).expect("the row has a box");
    window.advance(Duration::from_millis(50));
    assert!(shown.arrivals.with_untracked(|arrivals| arrivals.contains(&opened)), "selecting a row does not open it");
    window.double_click(row(&window, &opened)).expect("the row has a box");
    window.advance(Duration::from_millis(50));
    assert!(!shown.arrivals.with_untracked(|arrivals| arrivals.contains(&opened)), "opening it takes the tag off");
    assert!(shown.arrivals.with_untracked(|arrivals| arrivals.contains(&left)), "and only its own");

    control.select_folder(FolderId::INSTALL);
    assert!(shown.arrivals.with_untracked(std::collections::HashSet::is_empty), "leaving the listing takes the rest off");
}

#[test]
fn a_badge_counts_unseen_replays_after_the_landing_ring() {
    let (mut window, control) = open();
    let (_busy_dir, busy) = replay_dir(1);
    let (_quiet_dir, quiet) = replay_dir(1);
    control.adopt_pinned_folders(PinnedFolders(vec![busy.clone(), quiet.clone()]));
    settle_in_real_time(&mut window);
    let busy_id = folder_id(control, &busy);
    control.select_folder(folder_id(control, &quiet));

    let path = root_copy(&busy, FIXTURES[1]);
    simulated_watch::report(&busy, DirChange::Appeared(path));
    for _ in 0..10 {
        window.advance(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(25));
    }
    let badge = format!("replay-folder-badge-{}", busy_id.raw());
    assert!(window.by_test_id("replay-folder-ring-1").is_some(), "the landing sends a ring from the dot");
    assert!(window.by_test_id(&badge).is_none(), "the count waits for the ring");

    window.advance(LANDING_PULSE);
    let shown = window.by_test_id(&badge).expect("the count goes up once the ring has spread");
    assert_eq!(window.a11y(shown).and_then(|state| state.label), Some("1".to_owned()));

    control.select_folder(busy_id);
    window.advance(Duration::from_millis(50));
    assert!(window.by_test_id(&badge).is_none(), "selecting the folder clears its count");
}

fn root_copy(root: &Path, name: &str) -> PathBuf {
    let path = root.join(format!("landed-{name}"));
    std::fs::copy(fixture(name), &path).expect("the replay is copied");

    path
}

#[test]
fn a_watched_change_reaches_only_its_own_folder() {
    let (mut window, control) = open();
    let (_pinned_dir, pinned) = replay_dir(1);
    let (_other_dir, other) = replay_dir(1);
    control.adopt_pinned_folders(PinnedFolders(vec![pinned.clone(), other.clone()]));
    settle_in_real_time(&mut window);

    let landed = land(&pinned, FIXTURES[2], &mut window);

    assert!(listing(control, &pinned).holds(&landed));
    assert!(!listing(control, &other).holds(&landed), "the other folder's listing is untouched");
}

#[test]
fn a_missing_pinned_folder_keeps_its_tab_and_can_be_unpinned() {
    let (mut window, control) = open();
    let gone = std::env::temp_dir().join("wt-zgui-browser-gone").join("nowhere");
    let (_kept_dir, kept) = replay_dir(1);
    control.adopt_pinned_folders(PinnedFolders(vec![gone.clone(), kept.clone()]));
    settle_in_real_time(&mut window);

    let id = folder_id(control, &gone);
    assert_eq!(control.tab(id).map(|tab| tab.state), Some(FolderState::Missing), "the tab stays, marked missing");
    control.select_folder(id);
    window.advance(Duration::from_millis(50));
    let action = window.by_test_id("replay-folder-missing-action").expect("the notice offers to unpin");
    window.click(action).expect("the button has a box");
    window.advance(Duration::from_millis(50));

    assert!(control.tab(id).is_none(), "unpinning a missing folder closes its tab");
    assert_eq!(control.pinned_folders(), PinnedFolders(vec![kept]));
}

#[test]
fn ctrl_click_builds_a_selection_and_a_plain_click_replaces_it() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(3);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let shown = listing(control, &root);
    let paths = listed(shown);

    window.click(row(&window, &paths[0])).expect("the row has a box");
    window.click_with(row(&window, &paths[1]), Modifiers::PRIMARY).expect("the row has a box");
    window.advance(Duration::from_millis(50));
    assert_eq!(shown.selected_paths(), [paths[0].clone(), paths[1].clone()], "two ctrl-clicks mark two replays");

    window.click_with(row(&window, &paths[1]), Modifiers::PRIMARY).expect("the row has a box");
    window.advance(Duration::from_millis(50));
    assert_eq!(shown.selected_paths(), [paths[0].clone()], "clicking again takes one out");

    window.click(row(&window, &paths[2])).expect("the row has a box");
    window.advance(Duration::from_millis(50));
    assert_eq!(shown.selected_paths(), [paths[2].clone()], "a plain click replaces the set");
}

fn summary(outcome: MatchOutcome, self_pr: Option<f64>) -> RowSummary {
    RowSummary {
        outcome,
        self_damage: Some(50_000),
        self_kills: Some(1),
        self_survived: Some(true),
        self_pr,
        division_id: None,
        division_mates: Vec::new(),
        results_available: true,
        file_mtime: None,
    }
}

fn scanned(names: &[&str]) -> Vec<wows_toolkit_replay_data::replay_dir::ScannedReplay> {
    names.iter().map(|name| read_replay(fixture(name)).expect("the fixture's header reads")).collect()
}

#[test]
fn a_rated_row_carries_its_rating_and_outcome() {
    let files = scanned(&FIXTURES[..2]);
    let (rated, unrated) = (files[0].path.clone(), files[1].path.clone());
    let summaries = HashMap::from([
        (rated.clone(), summary(MatchOutcome::Win, Some(1650.4))),
        (unrated.clone(), summary(MatchOutcome::Unknown, None)),
    ]);
    let tree = BrowserTree::build(&TreeInputs {
        files: &files,
        summaries: &summaries,
        cache: None,
        fallback: None,
        grouping: ReplayGrouping::Date,
        locale: None,
    });

    let rated = tree.leaf(&rated).expect("the replay is listed");
    let rating = rated.personal_rating.as_ref().expect("the index rated the battle");
    assert_eq!(format!("{:.0}", rating.pr), "1650");
    assert_eq!(rated.outcome, MatchOutcome::Win);
    let unrated = tree.leaf(&unrated).expect("the replay is listed");
    assert!(unrated.personal_rating.is_none(), "an unrated battle has no chip");
    assert_eq!(unrated.outcome, MatchOutcome::Unknown);
}

#[test]
fn grouping_by_date_puts_each_replay_under_its_day_and_keeps_group_keys() {
    let files = scanned(&FIXTURES);
    let summaries = HashMap::new();
    let inputs = TreeInputs { files: &files, summaries: &summaries, cache: None, fallback: None, grouping: ReplayGrouping::Date, locale: None };
    let tree = BrowserTree::build(&inputs);

    assert_eq!(tree.roots().len(), 3, "three battles on three days make three groups");
    for root in tree.roots() {
        assert!(matches!(root, NodeKey::Group(_)));
        assert_eq!(tree.children(root).len(), 1);
    }
    assert_eq!(tree.order().len(), 3);
    let again = BrowserTree::build(&inputs);
    assert_eq!(tree.roots(), again.roots(), "a rebuild names the same groups, so their state survives it");

    let flat = BrowserTree::build(&TreeInputs { grouping: ReplayGrouping::None, ..inputs });
    assert!(flat.roots().iter().all(|key| matches!(key, NodeKey::Leaf(_))), "no grouping lists the replays alone");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the listing"]
fn the_listing_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "replay-browser-dark"), (Appearance::Light, "replay-browser-light")] {
        simulated_watch::install();
        let mut window = open_shell(review_options(appearance));
        let control = MADE.with(std::cell::Cell::get).expect("the replays tab made its browser");
        control.adopt_pinned_folders(PinnedFolders::default());
        let (_dir, root) = replay_dir(3);
        let (_other_dir, other) = replay_dir(1);
        control.open_folder(other.clone());
        control.pin_folder(folder_id(control, &other), true);
        control.open_folder(root.clone());
        settle_in_real_time(&mut window);
        control.pin_folder(folder_id(control, &root), true);
        settle_in_real_time(&mut window);
        let landed = land(&root, FIXTURES[0], &mut window);
        land(&other, FIXTURES[1], &mut window);
        window.advance(LANDING_PULSE);
        window.click(row(&window, &landed)).expect("the row has a box");
        window.advance(Duration::from_millis(300));

        println!("{}", save_shot(&mut window, name).display());
    }
}

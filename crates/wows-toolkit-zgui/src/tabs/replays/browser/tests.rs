use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::TreeModel;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_replay_data::replay_dir::DirChange;
use wows_toolkit_replay_data::replay_dir::ScanProgress;
use wows_toolkit_replay_data::replay_dir::read_replay;
use wows_toolkit_replay_data::simulated_watch;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::replay_folders::FolderHistory;
use wows_toolkit_viewmodel::replay_folders::PINNED_FOLDERS;
use wows_toolkit_viewmodel::replay_folders::PinnedFolders;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

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
        .with_untracked(|folders| {
            folders.iter().find(|tab| tab.source == FolderSource::Chosen(root.to_path_buf())).map(|tab| tab.id)
        })
        .expect("the directory has a tab")
}

fn listing(control: BrowserControl, root: &Path) -> Listing {
    control.listing(folder_id(control, root)).expect("the tab has a listing")
}

fn listed(listing: Listing) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> =
        listing.files.with_untracked(|files| files.iter().map(|file| file.path.clone()).collect());
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
    assert_eq!(control.history.with_untracked(|history| history.folders().to_vec()), std::slice::from_ref(&root));

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
    assert!(
        shown.arrivals.with_untracked(std::collections::HashSet::is_empty),
        "leaving the listing takes the rest off"
    );
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

/// Takes `wows` as the game directory, without saving it.
fn adopt_game_dir(control: BrowserControl, wows: &Path) {
    let services = control.shell().services();
    let pool = services.settings.pool().expect("the settings database is open");
    let runtime = services.runtime().expect("the runtime is up");
    let mut settings = runtime.block_on(StartupSettings::load(&pool));
    settings.wows_dir = wows.display().to_string();
    services.settings.adopt_unsaved(settings);
}

/// Runs frames for less than a landing ring takes, letting the watch's reports in.
fn settle_briefly(window: &mut HeadlessWindow) {
    for _ in 0..5 {
        window.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_superseded_scan_does_not_overwrite_the_current_progress() {
    let (mut window, control) = open();
    let (_dir, root) = replay_dir(1);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let shown = listing(control, &root);

    shown.relist();
    let superseded = shown.scan_generation();
    shown.relist();
    let shown_total = || shown.progress.with_untracked(|progress| progress.map(|progress| progress.total));
    shown.show_scan_progress(superseded, ScanProgress { done: 7, total: 9 });
    assert_ne!(shown_total(), Some(9), "the earlier scan's progress is dropped");

    shown.show_scan_progress(shown.scan_generation(), ScanProgress { done: 7, total: 9 });
    assert_eq!(shown_total(), Some(9), "the current scan's is shown");
}

#[test]
fn a_chosen_folder_is_scanned_only_once_it_is_found() {
    let (mut window, control) = open();
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let gone = dir.path().join("nowhere");
    control.adopt_pinned_folders(PinnedFolders(vec![gone.clone()]));
    settle_in_real_time(&mut window);

    let id = folder_id(control, &gone);
    assert_eq!(control.tab(id).map(|tab| tab.state), Some(FolderState::Missing));
    assert_eq!(listing(control, &gone).directory.get_untracked(), None, "the missing directory was not scanned");
    assert_eq!(simulated_watch::asked(&gone), 0, "nor watched");
}

#[test]
fn landings_in_one_folder_share_one_wait_that_closing_it_drops() {
    let (mut window, control) = open();
    let (_busy_dir, busy) = replay_dir(0);
    let (_quiet_dir, quiet) = replay_dir(0);
    control.adopt_pinned_folders(PinnedFolders(vec![busy.clone(), quiet.clone()]));
    settle_in_real_time(&mut window);
    let busy_id = folder_id(control, &busy);
    control.select_folder(folder_id(control, &quiet));

    for name in &FIXTURES[..2] {
        simulated_watch::report(&busy, DirChange::Appeared(root_copy(&busy, name)));
    }
    settle_briefly(&mut window);
    let waiting = || {
        control.pending_landings.with_value(|pending| {
            pending.iter().map(|(id, landing)| (*id, landing.paths.len())).collect::<Vec<(FolderId, usize)>>()
        })
    };
    assert_eq!(waiting(), [(busy_id, 2)], "both landings wait on one timer");

    window.advance(LANDING_PULSE);
    let badge = window.by_test_id(&format!("replay-folder-badge-{}", busy_id.raw())).expect("the count goes up");
    assert_eq!(window.a11y(badge).and_then(|state| state.label), Some("2".to_owned()));
    assert!(waiting().is_empty(), "the wait is dropped once it has counted");

    simulated_watch::report(&busy, DirChange::Appeared(root_copy(&busy, FIXTURES[2])));
    settle_briefly(&mut window);
    assert_eq!(waiting(), [(busy_id, 1)]);
    control.close_folder(busy_id);
    assert!(waiting().is_empty(), "closing the folder drops its wait");
}

#[test]
fn a_folder_folded_into_the_install_takes_its_watch_message_with_it() {
    let (mut window, control) = open();
    let (_dir, wows) = replay_dir(0);
    let replays = wows.join("replays");
    std::fs::create_dir(&replays).expect("the replays directory is made");
    control.adopt_pinned_folders(PinnedFolders(vec![replays.clone()]));
    settle_in_real_time(&mut window);
    let id = folder_id(control, &replays);

    simulated_watch::fail(&replays, "the share went away");
    settle_briefly(&mut window);
    assert!(window.by_class("zui-toast__action").is_some(), "the stopped watch is said, with a restart");

    adopt_game_dir(control, &wows);
    for _ in 0..20 {
        if control.tab(id).is_none() {
            break;
        }
        settle_briefly(&mut window);
    }
    assert!(control.tab(id).is_none(), "the install's tab takes the directory over");
    window.advance(Duration::from_millis(400));
    assert!(window.by_class("zui-toast__action").is_none(), "and the message about its watch goes with it");
}

#[cfg(unix)]
#[test]
fn a_folder_reached_through_a_link_to_the_install_directory_selects_the_install() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let (_dir, wows) = replay_dir(0);
    let replays = wows.join("replays");
    std::fs::create_dir(&replays).expect("the replays directory is made");
    adopt_game_dir(control, &wows);
    settle_in_real_time(&mut window);
    let link = wows.join("linked");
    std::os::unix::fs::symlink(&replays, &link).expect("the link is made");

    control.open_folder(link);
    settle_in_real_time(&mut window);

    assert_eq!(tabs(control).len(), 1, "no tab is added for the install's own directory");
    assert_eq!(control.selected.get_untracked(), FolderId::INSTALL);
    assert!(
        control.history.with_untracked(|history| history.folders().is_empty()),
        "nor is the install's directory kept as a recent one"
    );
}

#[cfg(unix)]
#[test]
fn a_pinned_link_to_the_install_directory_is_unpinned_into_the_install() {
    let (mut window, control) = open();
    let (_dir, wows) = replay_dir(0);
    let replays = wows.join("replays");
    std::fs::create_dir(&replays).expect("the replays directory is made");
    adopt_game_dir(control, &wows);
    settle_in_real_time(&mut window);
    let link = wows.join("linked");
    std::os::unix::fs::symlink(&replays, &link).expect("the link is made");

    control.adopt_pinned_folders(PinnedFolders(vec![link]));
    settle_in_real_time(&mut window);

    assert_eq!(tabs(control), [(FolderSource::Install, false, FolderState::Listed)], "the install is not pinned");
    assert_eq!(control.pinned_folders(), PinnedFolders::default(), "and the alias is no longer kept");
}

#[test]
fn a_former_install_directory_opens_as_a_folder_of_its_own() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let (_dir, wows) = replay_dir(0);
    let replays = wows.join("replays");
    std::fs::create_dir(&replays).expect("the replays directory is made");
    adopt_game_dir(control, &wows);
    settle_in_real_time(&mut window);

    adopt_game_dir(control, Path::new(""));
    settle_in_real_time(&mut window);
    control.open_folder(replays.clone());
    settle_in_real_time(&mut window);

    let id = folder_id(control, &replays);
    assert_eq!(control.selected.get_untracked(), id, "the directory has a tab of its own");
    assert_eq!(control.tab(id).map(|tab| tab.state), Some(FolderState::Listed));
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

#[test]
fn a_rows_menu_leaves_the_selection_on_the_selected_replay() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(2);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let shown = listing(control, &root);
    let paths = listed(shown);

    window.click(row(&window, &paths[0])).expect("the row has a box");
    window.right_click(row(&window, &paths[1])).expect("the row has a box");
    window.advance(Duration::from_millis(300));
    let open = t!("ui.replay.context.open_in_new_tab").into_owned();
    assert!(window.by_role_and_label(Role::MenuItem, &open).is_some(), "the row's menu is open");
    assert_eq!(shown.selected_paths(), [paths[0].clone()], "the menu leaves the selection");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(500));
    assert_eq!(shown.selected_paths(), [paths[0].clone()], "closing the menu leaves the selection");
}

#[test]
fn a_group_header_shows_no_replay_preview() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(3);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let paths = listed(listing(control, &root));

    window.hover(row(&window, &paths[0])).expect("the row has a box");
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("hover-preview").is_some(), "the card is up for the replay row");

    // The preview ends as the pointer leaves the row's content, so that content has to be the
    // whole row.
    let content = window.bounds(row(&window, &paths[0])).expect("the row has a box");
    let middle = content.origin.y.0 + content.size.height.0 / 2.0;
    let items = window.by_role(Role::TreeItem);
    let tree_row = items
        .into_iter()
        .filter_map(|item| window.bounds(item))
        .find(|bounds| bounds.origin.y.0 <= middle && middle < bounds.origin.y.0 + bounds.size.height.0)
        .expect("the replay sits in a tree row");
    assert_eq!(
        (content.origin.x.0, content.size.width.0),
        (tree_row.origin.x.0, tree_row.size.width.0),
        "a replay row's content spans the row"
    );

    let header = window.by_class("wt-row__group-label").expect("the replays are grouped by date");
    window.hover(header).expect("the header has a box");
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("hover-preview").is_none(), "a group header has no preview");

    window.hover(row(&window, &paths[1])).expect("the row has a box");
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("hover-preview").is_some(), "the next replay row has its card");
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
    let inputs = TreeInputs {
        files: &files,
        summaries: &summaries,
        cache: None,
        fallback: None,
        grouping: ReplayGrouping::Date,
        locale: None,
    };
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

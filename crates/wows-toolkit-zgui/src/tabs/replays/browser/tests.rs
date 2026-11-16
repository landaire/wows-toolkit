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
use wows_core::pseudonym::NameDisplay;
use wows_core::rating_scope::RatedBattle;
use wows_core::rating_scope::RatingScope;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_replay_data::replay_dir::DirChange;
use wows_toolkit_replay_data::replay_dir::HeaderReadError;
use wows_toolkit_replay_data::replay_dir::ScanProgress;
use wows_toolkit_replay_data::replay_dir::read_replay;
use wows_toolkit_replay_data::simulated_panic;
use wows_toolkit_replay_data::simulated_watch;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::personal_rating::BattleRating;
use wows_toolkit_viewmodel::replay_folders::FolderHistory;
use wows_toolkit_viewmodel::replay_folders::PINNED_FOLDERS;
use wows_toolkit_viewmodel::replay_folders::PinnedFolders;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
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
use super::listing::ListingWatch;
use super::tree::BrowserTree;
use super::tree::LeafMc;
use super::tree::NodeKey;
use super::tree::TreeInputs;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::tabs::replays::BUILT;
use crate::tabs::replays::workspace::ReplayWorkspace;

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

fn workspace() -> ReplayWorkspace {
    BUILT.with(std::cell::Cell::get).expect("the replays tab made its workspace")
}

fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths
}

/// The tabs of dropped replays with their labels, in tab order.
fn dropped_tabs(control: BrowserControl) -> Vec<(FolderId, String)> {
    control.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|tab| matches!(tab.source, FolderSource::Dropped { .. }))
            .map(|tab| (tab.id, tab.label()))
            .collect()
    })
}

fn has_row(window: &HeadlessWindow, path: &Path) -> bool {
    window.by_test_id(&format!("replay-row-{}", path.display())).is_some()
}

/// Replays dropped together are listed in one tab of their own, which is selected, and none of
/// them opens until its row is opened.
#[test]
fn dropped_replays_are_listed_in_one_tab_and_none_opens() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let workspace = workspace();
    let (_dir, root) = replay_dir(3);
    let [first, second, third] = FIXTURES.map(|name| root.join(name));

    workspace.shell.dropped(vec![first.clone(), third.clone()]);
    settle_in_real_time(&mut window);

    let dropped = dropped_tabs(control);
    assert_eq!(
        dropped.iter().map(|(_, label)| label.as_str()).collect::<Vec<_>>(),
        ["Dropped"],
        "one tab lists the drop"
    );
    let id = dropped[0].0;
    assert_eq!(control.selected.get_untracked(), id, "and it is selected");
    let listing = control.listing(id).expect("the tab has a listing");
    assert_eq!(listed(listing), sorted(vec![first.clone(), third.clone()]), "it lists exactly the dropped replays");
    assert_eq!(listing.watch(), ListingWatch::Unwatched, "and follows no directory");
    assert!(has_row(&window, &first) && has_row(&window, &third), "each has a row");
    assert!(!has_row(&window, &second), "the replay beside them has none");
    assert!(workspace.open_replays().is_empty(), "no replay opens");
    assert!(control.history.with_untracked(|history| history.folders().is_empty()), "nothing is recent");
    assert_eq!(control.pinned_folders(), PinnedFolders::default(), "or pinned");

    window.double_click(row(&window, &third)).expect("the row has a box");
    settle_in_real_time(&mut window);
    assert_eq!(workspace.open_replays(), [third], "a double click opens the replay");
}

/// One dropped replay opens rather than being listed.
#[test]
fn one_dropped_replay_opens() {
    let (mut window, control) = open();
    let workspace = workspace();
    let (_dir, root) = replay_dir(1);
    let one = root.join(FIXTURES[0]);

    workspace.shell.dropped(vec![one.clone()]);
    settle_in_real_time(&mut window);

    assert_eq!(workspace.open_replays(), [one], "the replay opens");
    assert!(dropped_tabs(control).is_empty(), "and is not listed");
}

/// A drop of replays, directories and other files lists each directory in its own tab and the
/// replays together in another, and says once how many files it left alone.
#[test]
fn a_mixed_drop_lists_folders_and_replays_and_counts_what_it_skipped() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let workspace = workspace();
    let (_first_dir, first_folder) = replay_dir(1);
    let (_second_dir, second_folder) = replay_dir(1);
    let (_dir, root) = replay_dir(2);
    let one = root.join(FIXTURES[0]);
    let two = root.join(FIXTURES[1]);
    let notes = root.join("notes.txt");
    let more = root.join("more.txt");
    std::fs::write(&notes, b"notes").expect("the file is written");
    std::fs::write(&more, b"more").expect("the file is written");

    workspace.shell.dropped(vec![first_folder.clone(), one.clone(), notes, second_folder.clone(), more, two.clone()]);
    settle_in_real_time(&mut window);

    let listed_sources: Vec<FolderSource> = tabs(control).into_iter().map(|(source, _, _)| source).collect();
    assert!(listed_sources.contains(&FolderSource::Chosen(first_folder)), "the first directory is listed");
    assert!(listed_sources.contains(&FolderSource::Chosen(second_folder)), "the second directory is listed");
    let dropped = dropped_tabs(control);
    assert_eq!(dropped.len(), 1, "the replays share one tab: {listed_sources:?}");
    let listing = control.listing(dropped[0].0).expect("the tab has a listing");
    assert_eq!(listed(listing), sorted(vec![one, two]), "which lists both");
    assert_eq!(control.selected.get_untracked(), dropped[0].0, "and is selected");
    assert!(workspace.open_replays().is_empty(), "no replay opens");
    let toasts = window.texts_by_class("lui-toasts").concat();
    let skipped = t!("ui.messages.drop_skipped", count = 2).into_owned();
    assert_eq!(toasts.matches(&skipped).count(), 1, "one message counts the files left alone: {toasts:?}");
}

/// Each drop of several replays lists them in a tab of its own, numbered after the first.
#[test]
fn each_drop_lists_its_replays_in_a_tab_of_its_own() {
    let (mut window, control) = open();
    let workspace = workspace();
    let (_dir, root) = replay_dir(3);
    let [first, second, third] = FIXTURES.map(|name| root.join(name));

    workspace.shell.dropped(vec![first.clone(), second.clone()]);
    settle_in_real_time(&mut window);
    workspace.shell.dropped(vec![second.clone(), third.clone()]);
    settle_in_real_time(&mut window);

    let dropped = dropped_tabs(control);
    let labels: Vec<&str> = dropped.iter().map(|(_, label)| label.as_str()).collect();
    assert_eq!(labels, ["Dropped", "Dropped 2"]);
    let lists = |id| listed(control.listing(id).expect("the tab has a listing"));
    assert_eq!(lists(dropped[0].0), sorted(vec![first, second.clone()]), "the first drop keeps its replays");
    assert_eq!(lists(dropped[1].0), sorted(vec![second, third]), "the second lists its own");
    assert_eq!(control.selected.get_untracked(), dropped[1].0, "the latest is selected");
}

/// A tab of dropped replays closes like a directory's, and takes its listing with it.
#[test]
fn closing_a_tab_of_dropped_replays_takes_its_listing() {
    let (mut window, control) = open();
    let workspace = workspace();
    let (_dir, root) = replay_dir(2);
    workspace.shell.dropped(vec![root.join(FIXTURES[0]), root.join(FIXTURES[1])]);
    settle_in_real_time(&mut window);
    let id = dropped_tabs(control)[0].0;

    let close = window.by_test_id(&format!("replay-folder-close-{}", id.raw())).expect("the tab can be closed");
    window.click(close).expect("the close button takes a click");
    settle_in_real_time(&mut window);

    assert!(dropped_tabs(control).is_empty(), "the tab is gone");
    assert!(control.listing(id).is_none(), "with its listing");
    assert_eq!(control.selected.get_untracked(), FolderId::INSTALL, "and the install's tab is selected");
}

/// A dropped replay whose header cannot be read, or whose read panics, is listed as a row saying
/// so, beside the replays that read.
#[test]
fn a_dropped_replay_that_cannot_be_read_has_a_row_saying_so() {
    let (mut window, control) = open();
    let workspace = workspace();
    let (_dir, root) = replay_dir(2);
    let good = root.join(FIXTURES[0]);
    let faulty = root.join(FIXTURES[1]);
    let broken = root.join("broken.wowsreplay");
    std::fs::write(&broken, b"not a replay").expect("the file is written");
    simulated_panic::panic_on_parse(&faulty);

    workspace.shell.dropped(vec![good.clone(), broken.clone(), faulty.clone()]);
    settle_in_real_time(&mut window);

    let listing = control.listing(dropped_tabs(control)[0].0).expect("the tab has a listing");
    assert_eq!(listed(listing), std::slice::from_ref(&good), "the readable replay is listed");
    let unreadable = listing.unreadable.with_untracked(Clone::clone);
    let paths: Vec<&Path> = unreadable.iter().map(|replay| replay.path.as_path()).collect();
    assert_eq!(paths, [broken.as_path(), faulty.as_path()], "the others are listed as unreadable");
    assert!(matches!(unreadable[0].error, HeaderReadError::Failed { .. }), "{:?}", unreadable[0].error);
    assert!(matches!(unreadable[1].error, HeaderReadError::Panicked { .. }), "{:?}", unreadable[1].error);
    assert!(has_row(&window, &good), "the replay has its row");
    for path in [&broken, &faulty] {
        let id = format!("replay-row-unreadable-{}", path.display());
        assert!(window.by_test_id(&id).is_some(), "{} has a row saying it cannot be read", path.display());
    }
}

/// Real replays dropped together, such as the 15.10 Siegfried replays whose parse once panicked,
/// are each listed. Reads the replays named by `WT_DROPPED_REPLAYS`, a list separated as `PATH`
/// is.
#[test]
#[ignore = "needs the replays named by WT_DROPPED_REPLAYS"]
fn real_dropped_replays_are_each_listed() {
    let named = std::env::var_os("WT_DROPPED_REPLAYS").expect("WT_DROPPED_REPLAYS names replays");
    let mut paths: Vec<PathBuf> = std::env::split_paths(&named).collect();
    paths.push(fixture(FIXTURES[0]));
    let (mut window, control) = open();
    let workspace = workspace();

    workspace.shell.dropped(paths.clone());
    settle_in_real_time(&mut window);

    let listing = control.listing(dropped_tabs(control)[0].0).expect("the tab has a listing");
    assert_eq!(listing.unreadable.with_untracked(Vec::len), 0, "every replay reads");
    assert_eq!(listed(listing), sorted(paths.clone()), "and is listed");
    for path in &paths {
        assert!(has_row(&window, path), "{} has a row", path.display());
    }
}

/// The header's open button drops a menu, and its directory item lists the directory picked.
#[test]
fn the_open_menu_lists_a_picked_directory() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let (_dir, root) = replay_dir(2);

    let trigger = window.by_test_id("replay-listing-open").expect("the header has an open button");
    window.click(trigger).expect("the open button has a box");
    window.advance(Duration::from_millis(300));
    let file = t!("ui.replay.open_file").into_owned();
    assert!(window.by_role_and_label(Role::MenuItem, &file).is_some(), "the menu offers a replay file");
    let directory = t!("ui.replay.open_directory").into_owned();
    let item = window.by_role_and_label(Role::MenuItem, &directory).expect("the menu offers a directory");

    wows_toolkit_services::dialog::take_asked();
    wows_toolkit_services::dialog::pick_next(root.clone());
    window.click(item).expect("the item has a box");
    settle_in_real_time(&mut window);

    assert_eq!(wows_toolkit_services::dialog::take_asked(), [wows_toolkit_services::dialog::Asked::Folder]);
    assert_eq!(control.selected.get_untracked(), folder_id(control, &root), "the picked directory is selected");
    assert_eq!(listed(listing(control, &root)).len(), 2, "its replays are listed");
}

/// The install's tab names its directory first, and says nothing in its place before there is one.
#[test]
fn a_tab_without_a_directory_has_no_blank_tooltip_line() {
    let (mut window, control) = open();
    settle_in_real_time(&mut window);
    assert_eq!(control.listing(FolderId::INSTALL).and_then(|listing| listing.directory.get_untracked()), None);

    let tooltip = super::folders::folder_tooltip(control, FolderId::INSTALL);
    assert!(!tooltip.starts_with('\n'), "the tooltip starts with what it says, got {tooltip:?}");
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

/// Sets "Autoload Latest Replay", which is on until the reader turns it off.
fn set_autoload(window: &mut HeadlessWindow, control: BrowserControl, on: bool) {
    control.shell().services().settings.edit(
        wows_toolkit_viewmodel::settings::keys::AUTO_LOAD_LATEST_REPLAY,
        |settings| {
            settings.auto_load_latest_replay = on;
            on
        },
    );
    settle_in_real_time(window);
}

#[test]
fn a_new_arrival_is_tagged_until_opened_or_left() {
    let (mut window, control) = open();
    set_autoload(&mut window, control, false);
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

/// A double-click with a modifier held marks rows rather than opening one.
#[test]
fn a_modified_double_click_selects_without_opening() {
    let (mut window, control) = open();
    set_autoload(&mut window, control, false);
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(1);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    control.pin_folder(folder_id(control, &root), true);
    settle_in_real_time(&mut window);
    let landed = land(&root, FIXTURES[1], &mut window);
    let shown = listing(control, &root);

    window.click_with(row(&window, &landed), Modifiers::SHIFT).expect("the row has a box");
    window.click_with(row(&window, &landed), Modifiers::SHIFT).expect("the row has a box");
    window.advance(Duration::from_millis(50));

    assert!(shown.arrivals.with_untracked(|arrivals| arrivals.contains(&landed)), "the replay was not opened");
}

/// A battle landed in a pinned folder on screen.
struct Landed {
    _dir: tempfile::TempDir,
    listing: Listing,
    path: PathBuf,
}

impl Landed {
    /// Lands a battle and returns once the listing holds it, while its row still glows.
    fn new(window: &mut HeadlessWindow, control: BrowserControl) -> Self {
        control.adopt_pinned_folders(PinnedFolders::default());
        let (dir, root) = replay_dir(0);
        control.open_folder(root.clone());
        settle_in_real_time(window);
        control.pin_folder(folder_id(control, &root), true);
        settle_in_real_time(window);
        let listing = listing(control, &root);

        let path = root.join(FIXTURES[1]);
        std::fs::copy(fixture(FIXTURES[1]), &path).expect("the replay is copied");
        simulated_watch::report(&root, DirChange::Appeared(path.clone()));
        // The header is read off the UI thread, so frames are run in step with real time.
        for _ in 0..100 {
            if listing.holds(&path) {
                break;
            }
            window.advance(Duration::from_millis(16));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(listing.holds(&path), "the watch lists the battle");
        window.advance(Duration::from_millis(32));

        Self { _dir: dir, listing, path }
    }

    fn is_new(&self) -> bool {
        self.listing.arrivals.with_untracked(|arrivals| arrivals.contains(&self.path))
    }

    fn glows(&self, window: &HeadlessWindow) -> bool {
        window.all_by_class("wt-row-arrived").contains(&row(window, &self.path))
    }
}

/// A battle opened by "Autoload Latest Replay" as it lands is not tagged New, and its row glows
/// as any arrival's does.
#[test]
fn an_autoloaded_arrival_glows_but_is_not_tagged() {
    let (mut window, control) = open();
    set_autoload(&mut window, control, true);

    let landed = Landed::new(&mut window, control);

    assert_eq!(workspace().open_replays(), std::slice::from_ref(&landed.path), "the battle opened as it landed");
    assert!(!landed.is_new(), "so it is not new");
    assert!(window.by_test_id("replay-row-new").is_none(), "and its row has no New tag");
    assert!(landed.glows(&window), "the row glows as it arrives");
    window.advance(Duration::from_millis(1300));
    assert!(!landed.glows(&window), "once");
}

/// A battle opened from outside the listing, as search, the palette and a lone drop do, is not
/// tagged New either.
#[test]
fn an_arrival_opened_from_elsewhere_is_not_tagged() {
    let (mut window, control) = open();
    set_autoload(&mut window, control, false);
    let landed = Landed::new(&mut window, control);
    assert!(window.by_test_id("replay-row-new").is_some(), "the arrival carries the New tag");

    workspace().shell.request_replay(crate::shell::ReplayRequest::OpenReplay(landed.path.clone()));
    settle_in_real_time(&mut window);

    assert_eq!(workspace().open_replays(), std::slice::from_ref(&landed.path));
    assert!(!landed.is_new(), "opening it takes the tag off");
    assert!(window.by_test_id("replay-row-new").is_none());
}

/// Enter on a group header opens or closes the group, as a click on it does.
#[test]
fn enter_on_a_group_header_opens_and_closes_it() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(1);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let shown = listing(control, &root);
    let expanded =
        || shown.expanded.with_untracked(|expanded| expanded.iter().any(|key| matches!(key, NodeKey::Group(_))));
    assert!(expanded(), "the group opens expanded");

    let header = window.by_class("wt-row__group-label").expect("the replays are grouped by date");
    window.click(header).expect("the header has a box");
    window.advance(Duration::from_millis(50));
    assert!(!expanded(), "a click closes it");

    window.press_key(NamedKey::Enter, Modifiers::NONE);
    window.advance(Duration::from_millis(50));
    assert!(expanded(), "Enter opens it again");

    window.press_key(NamedKey::Enter, Modifiers::NONE);
    window.advance(Duration::from_millis(50));
    assert!(!expanded(), "and closes it");
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

/// The events the listing sent since the last call.
fn sent() -> Vec<super::ReplayBrowserEvent> {
    super::SENT.with(|sent| std::mem::take(&mut *sent.borrow_mut()))
}

/// Opens `target`'s menu and picks the item labelled `label`.
fn pick(window: &mut HeadlessWindow, target: landaire_ui::test_support::Element, label: &str) {
    window.right_click(target).expect("the row has a box");
    window.advance(Duration::from_millis(300));
    let item = window.by_role_and_label(Role::MenuItem, label).unwrap_or_else(|| panic!("the menu offers {label}"));
    window.click(item).expect("the item has a box");
    window.advance(Duration::from_millis(300));
}

#[test]
fn a_lone_rows_menu_renders_it_to_video_or_the_clipboard() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(2);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let paths = listed(listing(control, &root));
    sent();

    for key in ["ui.replay.context.render_to_video", "ui.replay.context.render_to_clipboard"] {
        let target = row(&window, &paths[0]);
        pick(&mut window, target, &t!(key));
    }

    assert_eq!(
        sent(),
        [
            super::ReplayBrowserEvent::RenderManyToVideo(vec![paths[0].clone()]),
            super::ReplayBrowserEvent::RenderManyToClipboard(vec![paths[0].clone()]),
        ]
    );
}

#[test]
fn a_groups_menu_renders_and_records_session_stats_for_its_replays() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    let (_dir, root) = replay_dir(1);
    control.open_folder(root.clone());
    settle_in_real_time(&mut window);
    let paths = listed(listing(control, &root));
    sent();

    for key in [
        "ui.replay.context.render_to_video",
        "ui.replay.context.render_to_clipboard",
        "ui.replay.context.add_session_stats_one",
    ] {
        let header = window.by_class("wt-row__group-label").expect("the replays are grouped by date");
        pick(&mut window, header, &t!(key));
    }

    assert_eq!(
        sent(),
        [
            super::ReplayBrowserEvent::RenderManyToVideo(paths.clone()),
            super::ReplayBrowserEvent::RenderManyToClipboard(paths.clone()),
            super::ReplayBrowserEvent::SessionStats { paths, replace: false },
        ]
    );
}

#[test]
fn a_battle_landing_in_the_folder_on_screen_is_announced_and_one_elsewhere_is_only_counted() {
    let (mut window, control) = open();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let (_behind_dir, behind) = replay_dir(0);
    let (_shown_dir, shown) = replay_dir(0);
    control.open_folder(behind.clone());
    control.open_folder(shown.clone());
    settle_in_real_time(&mut window);
    // Pinned folders are the ones watched.
    control.pin_folder(folder_id(control, &behind), true);
    control.pin_folder(folder_id(control, &shown), true);
    settle_in_real_time(&mut window);
    sent();

    let landed = land(&shown, FIXTURES[0], &mut window);
    let elsewhere = land(&behind, FIXTURES[1], &mut window);

    let appeared: Vec<(PathBuf, String, bool)> = sent()
        .into_iter()
        .filter_map(|event| match event {
            super::ReplayBrowserEvent::ReplayAppeared { path, battle, followed } => Some((path, battle, followed)),
            _ => None,
        })
        .collect();
    let followed: Vec<(&Path, bool)> = appeared.iter().map(|(path, _, followed)| (path.as_path(), *followed)).collect();
    assert_eq!(followed, [(landed.as_path(), true), (elsewhere.as_path(), false)]);
    let toasts = window.texts_by_class("lui-toasts").concat();
    assert!(toasts.contains(&appeared[0].1), "the battle on screen is announced: {toasts:?}");
    assert!(!toasts.contains(&appeared[1].1), "the one behind is not: {toasts:?}");
    window.advance(LANDING_PULSE);
    assert_eq!(control.unseen(folder_id(control, &behind)), 1, "the one behind is counted on its tab");
}

/// Game data over nothing, held as loaded.
fn game_data_ready(control: BrowserControl) {
    let provider = wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
        .expect("an empty provider builds");
    let vfs: wowsunpack::vfs::VfsPath = wowsunpack::vfs::MemoryFS::new().into();
    let data = wows_toolkit_replay_data::load::LoadedGameData::for_test(provider, vfs, 1);
    control.shell().services().game_data.set_ready_for_test(std::sync::Arc::new(data));
}

#[test]
fn an_install_with_no_replays_is_warned_of_once_the_game_data_loads() {
    for (count, warned) in [(0, true), (1, false)] {
        let (mut window, control) = open();
        let (_dir, wows) = replay_dir(0);
        let replays = wows.join("replays");
        std::fs::create_dir(&replays).expect("the replays directory is made");
        for name in &FIXTURES[..count] {
            std::fs::copy(fixture(name), replays.join(name)).expect("the replay is copied");
        }
        adopt_game_dir(control, &wows);
        settle_in_real_time(&mut window);
        let warning = t!("ui.messages.no_replays_detected").into_owned();
        assert!(!window.texts_by_class("lui-toasts").concat().contains(&warning), "nothing is said before the load");

        game_data_ready(control);
        settle_in_real_time(&mut window);
        let toasts = window.texts_by_class("lui-toasts").concat();
        assert_eq!(toasts.contains(&warning), warned, "{count} replays: {toasts:?}");
    }
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

fn summary(outcome: MatchOutcome, rating_scope: RatingScope, self_pr: Option<f64>) -> RowSummary {
    RowSummary {
        outcome,
        self_damage: Some(50_000),
        self_kills: Some(1),
        self_survived: Some(true),
        self_pr,
        rating_scope,
        // Rated battles in these tests also reached their end, so carry a match contribution.
        self_mc: self_pr.map(|pr| pr - 200.0),
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
    let random = RatingScope::Rated(RatedBattle::Random);
    let files = scanned(&FIXTURES);
    let (rated, missing, unrated) = (files[0].path.clone(), files[1].path.clone(), files[2].path.clone());
    let summaries = HashMap::from([
        (rated.clone(), summary(MatchOutcome::Win, random, Some(1650.4))),
        (missing.clone(), summary(MatchOutcome::Unknown, random, None)),
        (unrated.clone(), summary(MatchOutcome::Loss, RatingScope::Unrated, None)),
    ]);
    let tree = BrowserTree::build(&TreeInputs {
        files: &files,
        unreadable: &[],
        summaries: &summaries,
        cache: None,
        fallback: None,
        grouping: ReplayGrouping::Date,
        locale: None,
        names: NameDisplay::Real,
    });

    let rated = tree.leaf(&rated).expect("the replay is listed");
    let Some(BattleRating::Rated(rating)) = rated.personal_rating.as_ref() else {
        panic!("the index rated the battle: {:?}", rated.personal_rating);
    };
    assert_eq!(format!("{:.0}", rating.pr), "1650");
    let Some(LeafMc::Rated(mc)) = rated.match_contribution else {
        panic!("the index rated the battle's MC: {:?}", rated.match_contribution);
    };
    assert_eq!(format!("{mc:.0}"), "1450");
    assert_eq!(rated.outcome, MatchOutcome::Win);
    let missing = tree.leaf(&missing).expect("the replay is listed");
    assert!(missing.personal_rating.is_none(), "a battle the index has not rated has no chip");
    assert_eq!(missing.outcome, MatchOutcome::Unknown);
    let unrated = tree.leaf(&unrated).expect("the replay is listed");
    assert_eq!(unrated.personal_rating, Some(BattleRating::Unrated), "an unrated battle says so");
    assert_eq!(unrated.match_contribution, Some(LeafMc::Unrated), "an unrated battle says so for MC too");
}

#[test]
fn grouping_by_date_puts_each_replay_under_its_day_and_keeps_group_keys() {
    let files = scanned(&FIXTURES);
    let summaries = HashMap::new();
    let inputs = TreeInputs {
        files: &files,
        unreadable: &[],
        summaries: &summaries,
        cache: None,
        fallback: None,
        grouping: ReplayGrouping::Date,
        locale: None,
        names: NameDisplay::Real,
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
        set_autoload(&mut window, control, false);
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

/// The ways a row can be scored, in the order [`scored_listing`] lists them.
const SCORINGS: [&str; 5] = ["both", "pr-only", "mc-only", "neither", "unrated"];

/// A listing of one replay per entry of [`SCORINGS`], each scored that way, by path.
fn scored_listing(window: &mut HeadlessWindow, control: BrowserControl) -> (tempfile::TempDir, Vec<PathBuf>) {
    let random = RatingScope::Rated(RatedBattle::Random);
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let root = std::fs::canonicalize(dir.path()).expect("the directory resolves");
    let paths: Vec<PathBuf> = SCORINGS
        .iter()
        .zip(FIXTURES.iter().cycle())
        .map(|(scoring, name)| {
            let path = root.join(format!("{scoring}-{name}"));
            std::fs::copy(fixture(name), &path).expect("the replay is copied");
            path
        })
        .collect();
    control.adopt_pinned_folders(PinnedFolders::default());
    control.open_folder(root.clone());
    settle_in_real_time(window);

    let summaries = HashMap::from([
        (paths[0].clone(), summary(MatchOutcome::Win, random, Some(1650.4))),
        (paths[1].clone(), RowSummary { self_mc: None, ..summary(MatchOutcome::Loss, random, Some(2480.0)) }),
        (paths[2].clone(), RowSummary { self_mc: Some(980.0), ..summary(MatchOutcome::Win, random, None) }),
        (paths[3].clone(), summary(MatchOutcome::Draw, random, None)),
        (paths[4].clone(), summary(MatchOutcome::Loss, RatingScope::Unrated, None)),
    ]);
    listing(control, &root).summaries.set(summaries);
    settle_in_real_time(window);

    (dir, paths)
}

/// Where a score chip sits in its row.
#[derive(Clone, Copy, Debug)]
struct ChipPlace {
    /// From the row's top.
    top: f32,
    /// From the row's top.
    bottom: f32,
    /// From the window's left.
    right: f32,
}

/// The chip of `kind` drawn inside the row of `path`, if the row has one.
fn chip_in_row(window: &HeadlessWindow, path: &Path, kind: &str) -> Option<ChipPlace> {
    let bounds = window.bounds(row(window, path)).expect("the row has a box");
    let row_top = bounds.origin.y.0;
    let row_bottom = row_top + bounds.size.height.0;
    let mut found: Vec<ChipPlace> = window
        .all_by_attribute("data-rating", kind)
        .into_iter()
        .filter_map(|chip| window.bounds(chip))
        .filter(|chip| row_top <= chip.origin.y.0 && chip.origin.y.0 + chip.size.height.0 <= row_bottom)
        .map(|chip| ChipPlace {
            top: chip.origin.y.0 - row_top,
            bottom: chip.origin.y.0 + chip.size.height.0 - row_top,
            right: chip.origin.x.0 + chip.size.width.0,
        })
        .collect();
    assert!(found.len() <= 1, "a row shows one {kind} at most, got {found:?}");

    found.pop()
}

/// A row's PR sits above its MC, inside the row, and each keeps its slot in every row so the
/// scores line up down the listing.
#[test]
fn a_rows_scores_stack_pr_above_mc_and_line_up_across_rows() {
    let (mut window, control) = open();
    let (_dir, paths) = scored_listing(&mut window, control);
    let [both, pr_only, mc_only, neither, unrated] = <[PathBuf; 5]>::try_from(paths).expect("one row per scoring");

    let pr = chip_in_row(&window, &both, "pr").expect("the row shows its PR");
    let mc = chip_in_row(&window, &both, "mc").expect("the row shows its MC");
    assert!(pr.bottom <= mc.top, "PR {pr:?} sits above MC {mc:?}");
    assert_eq!(pr.right, mc.right, "the two end at the same edge");

    // Rows sit at fractional offsets, so a slot's top rounds to within a device pixel.
    let same_slot = |chip: ChipPlace, slot: ChipPlace| (chip.top - slot.top).abs() <= 1.0 && chip.right == slot.right;

    let alone = chip_in_row(&window, &pr_only, "pr").expect("the row shows its PR");
    assert!(same_slot(alone, pr), "a PR alone keeps the PR slot: {alone:?} against {pr:?}");
    assert!(chip_in_row(&window, &pr_only, "mc").is_none());

    let alone = chip_in_row(&window, &mc_only, "mc").expect("the row shows its MC");
    assert!(same_slot(alone, mc), "an MC alone keeps the MC slot: {alone:?} against {mc:?}");
    assert!(chip_in_row(&window, &mc_only, "pr").is_none());

    assert!(chip_in_row(&window, &neither, "pr").is_none() && chip_in_row(&window, &neither, "mc").is_none());

    let dash = chip_in_row(&window, &unrated, "pr").expect("an unrated battle shows a dash for PR");
    assert!(same_slot(dash, pr), "the PR dash keeps the PR slot: {dash:?} against {pr:?}");
    let dash = chip_in_row(&window, &unrated, "mc").expect("an unrated battle shows a dash for MC");
    assert!(same_slot(dash, mc), "the MC dash keeps the MC slot: {dash:?} against {mc:?}");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the listing's scores"]
fn the_listing_scores_render_in_dark_and_light() {
    for (appearance, name) in
        [(Appearance::Dark, "replay-browser-scores-dark"), (Appearance::Light, "replay-browser-scores-light")]
    {
        simulated_watch::install();
        let mut window = open_shell(review_options(appearance));
        let control = MADE.with(std::cell::Cell::get).expect("the replays tab made its browser");
        let (_dir, _paths) = scored_listing(&mut window, control);

        println!("{}", save_shot(&mut window, name).display());
    }
}

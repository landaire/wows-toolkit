use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::DockRegion;
use landaire_ui::DockTarget;
use landaire_ui::Edge;
use landaire_ui::PanelEntry;
use landaire_ui::PanelKind;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_replays::types::AccountId;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::prelude::Set;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use super::BUILT;
use super::layout;
use super::workspace::Docked;
use super::workspace::OpenTarget;
use super::workspace::ReplayWorkspace;
use super::workspace::TabShown;
use super::workspace::WatchId;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::shell::ReplayRequest;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;
use crate::tabs::replays::panel::expanded::SHARE_MODE;
use crate::windows::renderer::session::RenderRequest;
use crate::windows::renderer::view::SHORTCUTS_MODE;

const WINDOW: WindowSize = WindowSize { width: 1200.0, height: 800.0 };
/// How long the listing takes to slide shut.
const SLIDE: Duration = Duration::from_millis(400);

fn open() -> (HeadlessWindow, ReplayWorkspace) {
    let window = open_shell(HeadlessOptions::new(WINDOW));
    let workspace = BUILT.with(std::cell::Cell::get).expect("the replays tab made its workspace");

    (window, workspace)
}

fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths
}

fn bounds_of(window: &HeadlessWindow, id: &str) -> zgui::geom::Rect<DevicePx, Device> {
    let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is drawn"));
    window.bounds(element).unwrap_or_else(|| panic!("{id} has a box"))
}

/// Whether `id` takes room on screen.
fn laid_out(window: &HeadlessWindow, id: &str) -> bool {
    window
        .by_test_id(id)
        .and_then(|element| window.bounds(element))
        .is_some_and(|bounds| bounds.size.width.0 > 0.0 && bounds.size.height.0 > 0.0)
}

#[test]
fn panel_ids_name_what_they_show() {
    for docked in [
        Docked::Replay(PathBuf::from("/replays/a b.wowsreplay")),
        Docked::Renderer(PathBuf::from("/replays/c.wowsreplay")),
        Docked::Watched(WatchId(42)),
    ] {
        assert_eq!(Docked::of(&docked.panel_id()), Some(docked.clone()), "{docked:?} reads back from its id");
    }
    assert_eq!(Docked::of(&landaire_ui::PanelId::new("chart:1")), None);
}

/// A plain open takes the place of the replay on screen, and "Open in New Tab" is how a second one
/// is asked for.
#[test]
fn a_plain_open_replaces_the_replay_on_screen() {
    let (mut window, workspace) = open();

    assert_eq!(workspace.open(PathBuf::from("first.wowsreplay"), OpenTarget::ShowingTab), Some(TabShown::Opened));
    window.advance(Duration::from_millis(32));
    assert_eq!(workspace.open_replays(), [PathBuf::from("first.wowsreplay")], "the first replay opens a tab");

    workspace.open(PathBuf::from("second.wowsreplay"), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(32));
    assert_eq!(workspace.open_replays(), [PathBuf::from("second.wowsreplay")], "the second takes its place");

    workspace.open(PathBuf::from("third.wowsreplay"), OpenTarget::NewTab);
    window.advance(Duration::from_millis(32));
    assert_eq!(
        sorted(workspace.open_replays()),
        [PathBuf::from("second.wowsreplay"), PathBuf::from("third.wowsreplay")],
        "a new tab is asked for by name"
    );
}

/// Re-opening a replay already open brings its tab forward rather than a second copy of itself.
#[test]
fn re_opening_the_showing_replay_leaves_it_alone() {
    let (mut window, workspace) = open();

    workspace.open(PathBuf::from("first.wowsreplay"), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(32));
    assert_eq!(
        workspace.open(PathBuf::from("first.wowsreplay"), OpenTarget::ShowingTab),
        Some(TabShown::BroughtForward)
    );
    assert_eq!(workspace.open_replays(), [PathBuf::from("first.wowsreplay")]);
}

/// A plain open replaces the replay showing in its group, and leaves one behind another tab alone.
#[test]
fn a_plain_open_replaces_only_the_replay_showing() {
    let (mut window, workspace) = open();

    workspace.open(PathBuf::from("behind.wowsreplay"), OpenTarget::ShowingTab);
    workspace.open(PathBuf::from("front.wowsreplay"), OpenTarget::NewTab);
    window.advance(Duration::from_millis(32));
    workspace.open(PathBuf::from("next.wowsreplay"), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(32));

    assert_eq!(
        sorted(workspace.open_replays()),
        [PathBuf::from("behind.wowsreplay"), PathBuf::from("next.wowsreplay")]
    );
}

/// The replay strip ends in the dock's menu, which closes the replay on screen.
#[test]
fn the_replay_strip_menu_closes_the_replay_on_screen() {
    let (mut window, workspace) = open();
    workspace.open(PathBuf::from("behind.wowsreplay"), OpenTarget::ShowingTab);
    workspace.open(PathBuf::from("front.wowsreplay"), OpenTarget::NewTab);
    window.advance(Duration::from_millis(32));

    let dock = bounds_of(&window, "replays-dock");
    let mut menus = Vec::new();
    for button in window.by_role(Role::Button) {
        if window.a11y(button).and_then(|state| state.label).as_deref() == Some("More Options") {
            menus.push(button);
        }
    }
    let menu = menus
        .into_iter()
        .find(|button| window.bounds(*button).is_some_and(|bounds| dock.contains(bounds.origin)))
        .expect("the replay strip has a menu");
    window.click(menu).expect("the menu button has a box");
    window.advance(Duration::from_millis(400));
    assert!(window.by_role_and_label(Role::MenuItem, "Close").is_some(), "the menu offers Close");

    // The missing files raise toasts over the menu's items, so the keyboard chooses.
    window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    window.press_key(NamedKey::Enter, Modifiers::NONE);
    window.advance(Duration::from_millis(400));

    assert_eq!(workspace.open_replays(), [PathBuf::from("behind.wowsreplay")], "the replay on screen is closed");
}

/// The dock shows a drop target until something opens, and the tab's title is the file's name
/// until the battle is read.
#[test]
fn the_empty_dock_says_what_can_be_dropped_on_it() {
    let (mut window, workspace) = open();
    assert!(laid_out(&window, "replay-pane-empty"), "an empty dock shows the drop target");

    workspace.open(PathBuf::from("missing.wowsreplay"), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(64));
    assert!(window.by_test_id("replay-pane-empty").is_none(), "an open replay takes its place");
    assert!(window.by_label("missing").is_some(), "the tab is named for the file while it reads");
}

/// Closing a viewport's tab ends its session; one popped out keeps it and loses only the tab.
#[test]
fn a_closed_viewport_ends_and_a_popped_out_one_moves() {
    let (mut window, workspace) = open();
    let closed = PathBuf::from("closed.wowsreplay");
    let popped = PathBuf::from("popped.wowsreplay");

    workspace.render(RenderRequest { path: closed.clone(), alts: Vec::new() });
    workspace.render(RenderRequest { path: popped.clone(), alts: Vec::new() });
    window.advance(Duration::from_millis(32));
    assert_eq!(sorted(workspace.open_renderers()), [closed.clone(), popped.clone()]);

    let closing = workspace.session_for_test(&closed).expect("the viewport has a session");
    workspace.remove(&Docked::Renderer(closed));
    window.advance(Duration::from_millis(32));
    assert!(!closing.is_open(), "closing the tab ended the session");

    let moving = workspace.session_for_test(&popped).expect("the viewport has a session");
    moving.popped_out.set(true);
    window.advance(Duration::from_millis(32));
    assert!(workspace.open_renderers().is_empty(), "the popped-out viewport left the dock");
    assert!(moving.is_open(), "and its session went with it");
    moving.close();
}

/// A second ask for the same battle's playback brings the open viewport forward.
#[test]
fn rendering_an_open_replay_again_brings_its_viewport_forward() {
    let (mut window, workspace) = open();
    let path = PathBuf::from("once.wowsreplay");

    workspace.render(RenderRequest { path: path.clone(), alts: Vec::new() });
    let first = workspace.session_for_test(&path).expect("the viewport has a session");
    workspace.render(RenderRequest { path: path.clone(), alts: Vec::new() });
    window.advance(Duration::from_millis(32));

    assert_eq!(workspace.open_renderers(), std::slice::from_ref(&path));
    let second = workspace.session_for_test(&path).expect("the viewport has a session");
    assert_eq!(first.title(), second.title(), "the same session is shown");
    workspace.remove(&Docked::Renderer(path));
}

/// What Search, a drop or the palette asks of the replays tab lands there; only a clipboard render
/// leaves the reader where they were.
#[test]
fn replay_requests_reach_the_tab() {
    let (mut window, workspace) = open();
    let shell = workspace.shell;

    shell.select(TabKey::App(AppTab::Search));
    shell.request_replay(ReplayRequest::OpenReplay(PathBuf::from("asked.wowsreplay")));
    window.advance(Duration::from_millis(64));
    assert_eq!(shell.selected_untracked(), Some(TabKey::App(AppTab::ReplayInspector)), "opening shows the replays");
    assert_eq!(workspace.open_replays(), [PathBuf::from("asked.wowsreplay")]);

    shell.request_replay(ReplayRequest::RenderReplay(PathBuf::from("played.wowsreplay")));
    window.advance(Duration::from_millis(64));
    assert_eq!(workspace.open_renderers(), [PathBuf::from("played.wowsreplay")]);
    workspace.remove(&Docked::Renderer(PathBuf::from("played.wowsreplay")));

    shell.select(TabKey::App(AppTab::Search));
    shell.request_replay(ReplayRequest::RenderToClipboard(PathBuf::from("clipped.wowsreplay")));
    window.advance(Duration::from_millis(64));
    assert_eq!(shell.selected_untracked(), Some(TabKey::App(AppTab::Search)), "a clipboard render stays put");
}

/// The listing's own controls draw in its header, and the status bar puts the listing away.
#[test]
fn the_listing_header_carries_the_listing_controls() {
    let (mut window, _workspace) = open();

    for id in ["replay-listing-options", "replay-listing-open"] {
        assert!(laid_out(&window, id), "{id} is in the listing header");
    }

    let toggle = window.by_test_id("replay-listing-toggle").expect("the status bar toggles the listing");
    window.click(toggle).expect("the toggle has a box");
    window.advance(SLIDE);
    assert!(!laid_out(&window, "replay-listing-options"), "the listing header is away with it");

    window.click(toggle).expect("the toggle has a box");
    window.advance(SLIDE);
    assert!(laid_out(&window, "replay-listing-options"), "and back with it");
}

/// The header comes first, the folder tabs directly under it, then the listing.
#[test]
fn the_listing_header_sits_above_the_folder_tabs() {
    let (window, _workspace) = open();

    let header = bounds_of(&window, "replay-listing-header");
    let tabs = bounds_of(&window, "replay-folder-tabs");
    let rows = bounds_of(&window, "replay-listing");
    assert!(header.bottom() <= tabs.top(), "the header {header:?} is above the tabs {tabs:?}");
    assert!(tabs.bottom() <= rows.top(), "the tabs {tabs:?} are above the listing {rows:?}");
}

/// The listing runs down the tab rather than sitting at its content height.
#[test]
fn the_listing_runs_down_the_tab() {
    let (window, _workspace) = open();

    let listing = bounds_of(&window, "replay-browser");
    assert!(listing.size.height.0 > WINDOW.height / 2.0, "the listing runs down the tab, got {listing:?}");
}

fn drag_listing_edge(window: &mut HeadlessWindow) -> (f32, f32) {
    let before = bounds_of(window, "replay-browser").size.width.0;
    let edge = window.by_role_and_label(Role::Splitter, "Replays").expect("the listing's edge is announced");
    let at = window.bounds(edge).expect("the edge has a box");
    let to = Point::<DevicePx, Device>::new(DevicePx(at.origin.x.0 + 80.0), DevicePx(at.origin.y.0 + 100.0));
    window.drag(edge, to, 4).expect("the edge drags");
    window.advance(Duration::from_millis(32));

    (before, bounds_of(window, "replay-browser").size.width.0)
}

/// The boundary between the listing and the replays beside it drags.
#[test]
fn the_listing_is_adjustable_in_width() {
    let (mut window, _workspace) = open();

    let (before, after) = drag_listing_edge(&mut window);
    assert!(after > before, "the listing widened with the drag, from {before} to {after}");
}

/// The listing keeps its place, and its drag, once a replay is open beside it.
#[test]
fn the_listing_survives_a_replay_being_opened_beside_it() {
    let (mut window, workspace) = open();
    workspace.open(PathBuf::from("first.wowsreplay"), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(32));

    let (before, after) = drag_listing_edge(&mut window);
    assert!(before > 0.0, "the listing still has width with a replay open");
    assert!(after > before, "the boundary still drags with a replay open, from {before} to {after}");
}

/// A restored layout brings back the replays still on disk and nothing that cannot be rebuilt.
#[test]
fn a_restored_layout_keeps_what_can_be_put_back() {
    let (mut window, workspace) = open();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let kept = dir.path().join("kept.wowsreplay");
    std::fs::write(&kept, b"").expect("the replay is written");
    let gone = dir.path().join("gone.wowsreplay");

    workspace.open(kept.clone(), OpenTarget::NewTab);
    workspace.open(gone.clone(), OpenTarget::NewTab);
    let viewport = Docked::Watched(WatchId(7));
    workspace
        .dock
        .add_panel(
            DockTarget::Region(DockRegion::Center),
            PanelEntry::new(viewport.panel_id(), PanelKind::new("watched")),
        )
        .expect("a watched panel docks");
    window.advance(Duration::from_millis(32));
    let saved = workspace.dock.dump();

    workspace.remove(&Docked::Replay(kept.clone()));
    workspace.remove(&Docked::Replay(gone.clone()));
    let _ = workspace.dock.remove(&viewport.panel_id());
    window.advance(Duration::from_millis(32));
    assert!(workspace.open_replays().is_empty());

    layout::restore(workspace, &saved, vec![kept.clone()]);
    window.advance(Duration::from_millis(32));
    assert_eq!(workspace.open_replays(), std::slice::from_ref(&kept));
    let docked: Vec<Docked> = workspace.dock.panels().iter().filter_map(|entry| Docked::of(&entry.id)).collect();
    assert_eq!(docked, [Docked::Replay(kept)], "the missing replay and the watched panel are left out");
}

/// Splitting the dock keeps an open replay's view, which neither reads the file again nor forgets
/// what it read.
#[test]
fn a_split_keeps_an_open_replays_view() {
    let (mut window, workspace) = open();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("unreadable.wowsreplay");
    std::fs::write(&path, b"not a replay").expect("the replay is written");
    workspace.open(path.clone(), OpenTarget::ShowingTab);
    settle_in_real_time(&mut window);
    let read = workspace.read_failure_for_test(&path).expect("the replay was read");
    let shown = window.by_test_id("replay-panel-failed").expect("the view shows the read");

    // Gone, so a second read would fail for another reason.
    std::fs::remove_file(&path).expect("the replay is removed");
    let group = workspace.dock.group_of(&Docked::Replay(path.clone()).panel_id()).expect("the replay is docked");
    let beside = PanelEntry::new(Docked::Watched(WatchId(7)).panel_id(), PanelKind::new("watched"));
    workspace.dock.add_panel(DockTarget::Beside { group, edge: Edge::Right }, beside).expect("a panel docks beside it");
    settle_in_real_time(&mut window);

    assert!(laid_out(&window, "replay-panel-failed"), "the view still shows the read");
    assert_eq!(window.by_test_id("replay-panel-failed"), Some(shown), "and is the view it was");
    assert_eq!(workspace.read_failure_for_test(&path), Some(read), "and the file was not read again");
}

/// The status bar carries the listing's toggle on the replays tab.
#[test]
fn the_status_bar_holds_the_listing_toggle_on_the_replays_tab() {
    let (mut window, workspace) = open();
    assert!(window.by_test_id("replay-listing-toggle").is_some(), "the toggle is on the bar");

    workspace.shell.select(TabKey::App(AppTab::Search));
    window.advance(Duration::from_millis(64));
    assert!(window.by_test_id("replay-listing-toggle").is_none(), "and away from it on another tab");
}

/// The status bar names the mode of the panel on screen. A replay or a viewport behind another tab
/// says nothing, however its rows are open.
#[test]
fn only_the_panel_on_screen_names_its_mode() {
    let (mut window, workspace) = open();
    let behind = PathBuf::from("behind.wowsreplay");
    let front = PathBuf::from("front.wowsreplay");
    let mode = || workspace.shell.selected_chrome().and_then(|chrome| chrome.mode());

    workspace.open(behind.clone(), OpenTarget::ShowingTab);
    workspace.open(front.clone(), OpenTarget::NewTab);
    window.advance(Duration::from_millis(32));
    workspace.control_for_test(&behind).expect("the replay is open").note_shares_for_test(AccountId(1));
    window.advance(Duration::from_millis(32));
    assert_eq!(mode(), None, "the replay showing lists no damage shares");

    workspace.dock.activate(&Docked::Replay(behind.clone()).panel_id()).expect("the replay is docked");
    window.advance(Duration::from_millis(32));
    assert_eq!(mode(), Some(SHARE_MODE), "brought forward, its shares are named");

    let viewport = PathBuf::from("viewport.wowsreplay");
    workspace.render(RenderRequest { path: viewport.clone(), alts: Vec::new() });
    window.advance(Duration::from_millis(32));
    assert_eq!(mode(), Some(SHORTCUTS_MODE), "the viewport in front names its shortcuts");

    workspace.dock.activate(&Docked::Replay(behind).panel_id()).expect("the replay is docked");
    window.advance(Duration::from_millis(32));
    assert_eq!(mode(), Some(SHARE_MODE), "the viewport behind it does not overwrite the replay's");
    workspace.remove(&Docked::Renderer(viewport));
}

/// A collapsed listing is stored with the replay settings.
#[test]
fn collapsing_the_listing_is_saved() {
    let (mut window, workspace) = open();
    let toggle = window.by_test_id("replay-listing-toggle").expect("the status bar toggles the listing");
    window.click(toggle).expect("the toggle has a box");
    window.advance(SLIDE);

    let collapsed = workspace.services.settings.read_untracked(|settings| settings.replay.listing_collapsed);
    assert_eq!(collapsed, Some(true));

    window.click(toggle).expect("the toggle has a box");
    window.advance(SLIDE);
    settle_in_real_time(&mut window);
}

/// A press on the listing's options opens their popover.
#[test]
fn the_listing_options_open_on_click() {
    let (mut window, _workspace) = open();
    assert!(!laid_out(&window, "replay-listing-options-menu"));

    let options = window.by_test_id("replay-listing-options").expect("the options button is drawn");
    window.click(options).expect("the button has a box");
    window.advance(Duration::from_millis(300));
    assert!(laid_out(&window, "replay-listing-options-menu"), "the options popover opened");
    assert!(laid_out(&window, "directory-game-data"));
}

/// The tab empty, with a replay open, and playing it back, in both themes, for review. Reads the
/// install named by `WT_WOWS_DIR` and the replay named by `WT_REPLAY`.
#[test]
#[ignore = "draws on the graphics device; set WT_WOWS_DIR and WT_REPLAY to review the tab"]
fn the_replays_tab_renders_in_dark_and_light() {
    let wows_dir = std::env::var("WT_WOWS_DIR").expect("WT_WOWS_DIR names a game install");
    let replay = std::env::var_os("WT_REPLAY").map(PathBuf::from).expect("WT_REPLAY names a replay");
    assert!(Path::new(&replay).is_file(), "{} is a replay", replay.display());
    let _fetches = super::testing::allow_fetches();
    for (appearance, name) in [(Appearance::Dark, "replays-tab-dark"), (Appearance::Light, "replays-tab-light")] {
        let mut window = open_shell(review_options(appearance));
        let workspace = BUILT.with(std::cell::Cell::get).expect("the replays tab made its workspace");
        workspace.shell.apply_wows_dir(wows_dir.clone());
        for _ in 0..30 {
            settle_in_real_time(&mut window);
            if matches!(workspace.services.game_data.state_untracked(), crate::services::GameDataState::Ready(_)) {
                break;
            }
        }
        // The install's directory holds replays from builds this machine lacks, which are offered.
        settle_in_real_time(&mut window);
        if let Some(cancel) = window.by_test_id("download-offer-cancel") {
            println!("{}", save_shot(&mut window, &format!("{name}-offer")).display());
            window.click(cancel).expect("the offer's cancel has a box");
            settle_in_real_time(&mut window);
        }
        println!("{}", save_shot(&mut window, &format!("{name}-empty")).display());

        workspace.open(replay.clone(), OpenTarget::ShowingTab);
        for _ in 0..4 {
            settle_in_real_time(&mut window);
        }
        println!("{}", save_shot(&mut window, name).display());

        workspace.render(RenderRequest { path: replay.clone(), alts: Vec::new() });
        for _ in 0..8 {
            settle_in_real_time(&mut window);
        }
        println!("{}", save_shot(&mut window, &format!("{name}-renderer")).display());
        workspace.remove(&Docked::Renderer(replay.clone()));
        workspace.remove(&Docked::Replay(replay.clone()));
        settle_in_real_time(&mut window);
    }
}

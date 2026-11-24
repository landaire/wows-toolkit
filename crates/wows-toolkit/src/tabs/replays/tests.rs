use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
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
use wows_core::pseudonym;
use wows_minimap_renderer::draw_command::ConsumableAvailability;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::RosterRow;
use wows_minimap_renderer::draw_command::RosterSide;
use wows_replay_insights::timeline::HealthSnapshot;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_config::index::query;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_dir::MissingBuild;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::track::FrameIndex;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_replay_data::sample::sample_model;
use wows_toolkit_services::collab::CollabState;
use wows_toolkit_services::collab::SharedWindow;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::upload::RawUploadDeadline;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_params::types::Vehicle;
use wowsunpack::game_types::GameParamId;
use wowsunpack::recognized::Recognized;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use wt_collab_client::OpenReplay;
use wt_translations::t;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::Owner;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use super::BUILT;
use super::arrivals::Arrivals;
use super::layout;
use super::panel::ReparseCause;
use super::session_stats::SessionReads;
use super::sharing::SharingReads;
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
use crate::tabs::armor_viewer::library::ViewerFeed;
use crate::tabs::replays::browser::FolderId;
use crate::tabs::replays::panel::expanded::SHARE_MODE;
use crate::windows::renderer::session::RenderRequest;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::session::Status;
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

fn new_arrivals(workspace: ReplayWorkspace) -> Arrivals {
    workspace
        .under_tab(|| Arrivals::new(workspace, SharingReads::new(), SessionReads::new(workspace, workspace.browser())))
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

/// A replay tab's right-click menu speaks the reader's language.
#[test]
fn the_replay_tab_menu_is_translated() {
    let (mut window, workspace) = open();
    workspace.open(PathBuf::from("behind.wowsreplay"), OpenTarget::ShowingTab);
    workspace.open(PathBuf::from("front.wowsreplay"), OpenTarget::NewTab);
    window.advance(Duration::from_millis(32));

    let dock = bounds_of(&window, "replays-dock");
    let tab = window
        .by_role(Role::Tab)
        .into_iter()
        .find(|tab| window.bounds(*tab).is_some_and(|bounds| dock.contains(bounds.origin)))
        .expect("the dock shows a replay tab");
    window.right_click(tab).expect("the tab has a box");
    window.advance(Duration::from_millis(400));

    for key in ["ui-tabs-close_tab", "ui-tabs-close_other_tabs", "ui-tabs-close_tabs_to_the_right"] {
        let label = t!(key).into_owned();
        assert!(window.by_role_and_label(Role::MenuItem, &label).is_some(), "the menu offers {label}");
    }
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

/// Both the listener retained by the viewport and the seek retained by the viewer outlive the
/// menus that supplied them. Closing the viewport still makes the viewer's seek harmless.
#[test]
fn armor_playback_outlives_the_menus_that_opened_it() {
    let (mut window, workspace) = open();
    let path = PathBuf::from("armor.wowsreplay");
    let menu = workspace.under_tab(Owner::new);
    menu.with(|| workspace.render(RenderRequest { path: path.clone(), alts: Vec::new() }));
    menu.cleanup();
    window.advance(Duration::from_millis(32));
    let session = workspace.session_for_test(&path).expect("the viewport survived its opening menu");
    let entity = EntityId::from(7u32);
    seed_armor_playback(session, entity);

    let ship_menu = workspace.under_tab(Owner::new);
    ship_menu.with(|| session.show_armor(entity));
    ship_menu.cleanup();
    let shell = workspace.shell;
    assert_eq!(shell.selected_untracked(), Some(TabKey::App(AppTab::ArmorViewer)));
    let mut feeds = shell.armor.take_feed().into_iter();
    let ViewerFeed::Show { shown, seek, source } = feeds.next().expect("the viewer was asked to show armor") else {
        panic!("the first request opens the ship");
    };
    assert!(feeds.next().is_none(), "only the selected ship was sent");
    assert_eq!(shown.target.param_index, "PRSC610");
    assert_eq!(shown.health, Some(100.0));
    let seek = seek.expect("the incoming-fire log can seek playback");

    seek(GameClock(12.0));
    assert_eq!(session.playback.with_untracked(|playback| playback.at()), FrameIndex::new(1));
    let mut feeds = shell.armor.take_feed().into_iter();
    let ViewerFeed::Followed { source: following, followed } =
        feeds.next().expect("the viewer followed the playback seek")
    else {
        panic!("moving playback updates the followed ship");
    };
    assert!(feeds.next().is_none());
    assert_eq!(following, source, "the same viewer feed follows playback");
    assert_eq!(followed.at, GameClock(12.0));
    assert_eq!(followed.health, Some(60.0));

    workspace.remove(&Docked::Renderer(path));
    assert!(!session.is_open(), "closing stops playback before its view unmounts");
    assert_eq!(session.playback.with_untracked(|playback| playback.at()), FrameIndex::new(1));
    seek(GameClock(30.0));
    assert_eq!(session.playback.with_untracked(|playback| playback.at()), FrameIndex::new(1));
    window.advance(Duration::from_millis(32));
    assert!(!session.is_open());
    assert!(session.status.try_with_untracked(|_| ()).is_none(), "the dock released the closed viewport's state");
    seek(GameClock(30.0));
    assert!(shell.armor.take_feed().is_empty(), "a closed playback sends no further armor updates");
}

fn seed_armor_playback(session: RendererSession, entity: EntityId) {
    let vehicle = Vehicle::builder()
        .level(10)
        .group("cruiser".to_owned())
        .maybe_abilities(None)
        .upgrades(Vec::new())
        .maybe_config_data(None)
        .maybe_model_path(None)
        .maybe_armor(None)
        .maybe_hit_locations(None)
        .permoflages(Vec::new())
        .camera_trajectories(Vec::new())
        .maybe_ttx_components(None)
        .innate_skills(Vec::new())
        .build();
    let param_id = GameParamId::from(1u32);
    let param = Param::builder()
        .id(param_id)
        .index("PRSC610".to_owned())
        .name("PRSC610_Moskva".to_owned())
        .species(Recognized::Known(Species::Cruiser))
        .nation("Russia".to_owned())
        .data(ParamData::Vehicle(vehicle))
        .build();
    let provider = GameMetadataProvider::from_params_no_specs(vec![param]).expect("the ship provider builds");
    let vfs: VfsPath = MemoryFS::new().into();
    session
        .hold_game_data_for_test(GameDataCache::holding_for_test(Arc::new(LoadedGameData::for_test(provider, vfs, 1))));

    let roster = DrawCommand::TeamRoster {
        side: RosterSide::Friendly,
        x: 0,
        y: 0,
        width: 200,
        height: 400,
        rows: vec![RosterRow {
            entity_id: entity,
            team_id: 0,
            player_name: "Player".to_owned(),
            clan_tag: None,
            clan_color: None,
            ship_name: "Moskva".to_owned(),
            ship_param_id: Some(param_id),
            class_icon_key: None,
            species: Some(Species::Cruiser),
            hp: None,
            heal_availability: ConsumableAvailability::Unavailable,
            is_dead: false,
            is_self: true,
            is_spotted: false,
            is_disconnected: false,
            kills: 0,
            damage_dealt: 0.0,
            seconds_since_damage: None,
            consumables: Vec::new(),
        }],
    };
    session.status.set(Status::Ready(Arc::new(Track {
        frames: vec![vec![roster]; 3],
        clocks: vec![GameClock(0.0), GameClock(12.0), GameClock(30.0)],
        battle_start: GameClock(0.0),
        battle_end: None,
        replay_end: GameClock(30.0),
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        space: String::new(),
        video_stem: String::new(),
        trails: Default::default(),
        map_origin: (0.0, 0.0),
    })));
    let timeline = ShipShotTimeline {
        hits: Vec::new(),
        health_history: BTreeMap::from([
            (GameClock(0.0), HealthSnapshot { health: 100.0, max_health: 100.0 }),
            (GameClock(12.0), HealthSnapshot { health: 60.0, max_health: 100.0 }),
        ]),
        hull: None,
    };
    session.events.set(BattleEvents::from_read(Vec::new(), None, HashMap::from([(entity, timeline)])));
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

/// Asking again for a battle whose viewport has a window of its own finds that viewport, rather
/// than docking a second one and baking the battle again.
#[test]
fn rendering_a_popped_out_replay_again_finds_its_viewport() {
    let (mut window, workspace) = open();
    let path = PathBuf::from("popped.wowsreplay");

    workspace.render(RenderRequest { path: path.clone(), alts: Vec::new() });
    let popped = workspace.session_for_test(&path).expect("the viewport has a session");
    popped.popped_out.set(true);
    window.advance(Duration::from_millis(32));
    assert!(workspace.open_renderers().is_empty(), "the popped-out viewport left the dock");

    workspace.render(RenderRequest { path: path.clone(), alts: Vec::new() });
    window.advance(Duration::from_millis(32));
    assert!(workspace.open_renderers().is_empty(), "no second viewport is docked");
    assert!(workspace.session_for_test(&path).is_none());
    popped.close();
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

/// The header's open menu opens the replay file picked.
#[test]
fn the_open_menu_opens_a_picked_replay() {
    let (mut window, workspace) = open();

    let trigger = window.by_test_id("replay-listing-open").expect("the header has an open button");
    window.click(trigger).expect("the open button has a box");
    window.advance(Duration::from_millis(300));
    let file = t!("ui-replay-open_file").into_owned();
    let item = window.by_role_and_label(Role::MenuItem, &file).expect("the menu offers a replay file");

    wows_toolkit_services::dialog::take_asked();
    wows_toolkit_services::dialog::pick_next(PathBuf::from("picked.wowsreplay"));
    window.click(item).expect("the item has a box");
    settle_in_real_time(&mut window);

    assert_eq!(wows_toolkit_services::dialog::take_asked(), [wows_toolkit_services::dialog::Asked::File]);
    assert_eq!(workspace.open_replays(), [PathBuf::from("picked.wowsreplay")], "the picked replay opens");
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

/// A watched battle closes when the host closes its replay, and every one closes when the session
/// ends: neither has anything more to show.
#[test]
fn a_watched_battle_closes_when_the_session_stops_listing_it() {
    let (mut window, workspace) = open();
    let shell = workspace.shell;
    let battle = |replay_id: u64| OpenReplay {
        replay_id,
        replay_name: format!("battle {replay_id}"),
        map_image_png: Vec::new(),
        game_version: "15.0".to_owned(),
        map_name: "16_OC_bees_to_honey".to_owned(),
        display_name: "Sleeping Giant".to_owned(),
    };
    let watched = || {
        let mut ids: Vec<u64> = workspace
            .dock
            .panels_untracked()
            .iter()
            .filter_map(|entry| match Docked::of(&entry.id) {
                Some(Docked::Watched(watch)) => Some(watch.0),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids
    };
    shell.collab.edit_for_test(shell, |collab| collab.state.lock().open_replays = vec![battle(7), battle(8)]);
    for replay_id in [7, 8] {
        workspace.watch(SharedWindow {
            replay_id,
            replay_name: format!("battle {replay_id}"),
            map: "Sleeping Giant".to_owned(),
            map_name: "16_OC_bees_to_honey".to_owned(),
            art_png: None,
        });
    }
    window.advance(Duration::from_millis(100));
    assert_eq!(watched(), [7, 8], "both battles are watched");

    shell.collab.edit_for_test(shell, |collab| collab.state.lock().open_replays = vec![battle(8)]);
    window.advance(Duration::from_millis(100));
    assert_eq!(watched(), [8], "the battle the host closed is closed here");

    shell.collab.edit_for_test(shell, CollabState::leave);
    window.advance(Duration::from_millis(100));
    assert!(watched().is_empty(), "and the rest close as the session ends");
}

/// A parse that panics fails that replay's tab as any failed read does, and the app goes on
/// reading other replays.
#[test]
fn a_parse_that_panics_fails_its_tab_and_the_app_goes_on() {
    let (mut window, workspace) = open();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let faulting = dir.path().join("faulting.wowsreplay");
    wows_toolkit_replay_data::simulated_panic::panic_on_parse(&faulting);
    workspace.open(faulting.clone(), OpenTarget::ShowingTab);
    settle_in_real_time(&mut window);

    let failure = workspace.read_failure_for_test(&faulting).expect("the panicked read fails the tab");
    assert!(failure.contains("a simulated decoder fault"), "the tab names the fault: {failure}");
    assert!(laid_out(&window, "replay-panel-failed"), "the tab shows the failure");

    let next = dir.path().join("next.wowsreplay");
    std::fs::write(&next, b"not a replay").expect("the replay is written");
    workspace.open(next.clone(), OpenTarget::NewTab);
    settle_in_real_time(&mut window);
    assert!(workspace.read_failure_for_test(&next).is_some(), "a later replay is still read");
}

/// A replay whose build this machine lacks offers that build for download, as the listing offers
/// the builds its replays need.
#[test]
fn a_replay_of_a_missing_build_offers_the_build() {
    let (mut window, workspace) = open();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let name = "10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay";
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wows-replay-insights/tests/fixtures/replays").join(name);
    let path = dir.path().join(name);
    std::fs::copy(fixture, &path).expect("the replay is copied");
    super::testing::take_held_offers();

    workspace.open(path.clone(), OpenTarget::ShowingTab);
    settle_in_real_time(&mut window);

    assert!(workspace.read_failure_for_test(&path).is_some(), "the replay does not read");
    let expected = MissingBuild { build: 9_251_401, version: Some("13.11.0".to_owned()), replays: 1 };
    assert_eq!(super::testing::take_held_offers(), [expected], "its build is offered");
}

/// The download offer a replay of a missing build raises, for review. Plans the download against
/// the published repository.
#[test]
#[ignore = "reaches the game data repository and draws on the graphics device"]
fn a_missing_builds_offer_renders() {
    let _fetches = super::testing::allow_fetches();
    let mut window = open_shell(review_options(Appearance::Dark));
    let workspace = BUILT.with(std::cell::Cell::get).expect("the replays tab made its workspace");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let name = "10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay";
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wows-replay-insights/tests/fixtures/replays").join(name);
    let path = dir.path().join(name);
    std::fs::copy(fixture, &path).expect("the replay is copied");

    workspace.open(path, OpenTarget::ShowingTab);
    for _ in 0..10 {
        settle_in_real_time(&mut window);
        if window.by_test_id("download-offer").is_some() {
            break;
        }
    }

    assert!(window.by_test_id("download-offer").is_some(), "the build is offered");
    println!("{}", save_shot(&mut window, "replay-missing-build-offer").display());
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
    let row = crate::tabs::replays::table::RowId::from_position(0);
    workspace.control_for_test(&behind).expect("the replay is open").note_shares_for_test(row);
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

/// The listing options' anonymize box saves the setting, and the open replay's table names its
/// players and clans by pseudonym while it is ticked.
#[test]
fn the_anonymize_box_swaps_the_tables_names_for_pseudonyms() {
    let (mut window, workspace) = open();
    super::panel::tests::seed_next_panel(sample_model());
    workspace.open(PathBuf::from("battle.wowsreplay"), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(600));
    let real_clan = "[WTK]".to_owned();
    let fake_name = pseudonym::player_name("Ally_Alpha");
    let fake_clan = format!("[{}]", pseudonym::clan_tag("WTK"));
    let names = window.texts_by_class("wt-pt-text");
    assert!(names.iter().any(|text| text == "Ally_Alpha"), "the table starts with the real names: {names:?}");
    assert!(names.contains(&real_clan), "and the real clan tags: {names:?}");

    let options = window.by_test_id("replay-listing-options").expect("the options button is drawn");
    window.click(options).expect("the button has a box");
    window.advance(Duration::from_millis(300));
    let anonymize = window.by_test_id("replay-listing-anonymize-names").expect("the anonymize box is drawn");
    window.click(anonymize).expect("the box has a box");
    window.advance(Duration::from_millis(300));

    let saved = workspace.services.settings.read_untracked(|settings| settings.replay.anonymize_player_names);
    assert_eq!(saved, Some(true), "the box sets the replay setting");
    let names = window.texts_by_class("wt-pt-text");
    assert!(names.contains(&fake_name), "the row reads its pseudonym: {names:?}");
    assert!(names.contains(&fake_clan), "and its clan's: {names:?}");
    assert!(!names.iter().any(|text| text == "Ally_Alpha" || *text == real_clan), "no real name is left: {names:?}");

    window.click(anonymize).expect("the box has a box");
    window.advance(Duration::from_millis(300));
    let saved = workspace.services.settings.read_untracked(|settings| settings.replay.anonymize_player_names);
    assert_eq!(saved, Some(false), "a second press clears it");
    let names = window.texts_by_class("wt-pt-text");
    assert!(names.iter().any(|text| text == "Ally_Alpha"), "and the real names return: {names:?}");
}

/// A login seen in the monitored channel's chat around the battle flags the player it names.
#[test]
fn a_player_seen_in_twitch_chat_gets_the_chip() {
    let (mut window, workspace) = open();
    let services = workspace.services;
    let runtime = services.runtime().expect("the runtime is up");
    let pool = runtime.block_on(wows_toolkit_config::test_pool());
    let settings = runtime.block_on(StartupSettings::load(&pool));
    services.settings.adopt(settings, pool.clone());
    let model = sample_model();
    let seen_at = model.timestamp.as_second() + 60;
    runtime
        .block_on(query::record_twitch_observations(&pool, &[("ally_alpha".to_string(), seen_at)]))
        .expect("the observation is written");

    super::panel::tests::seed_next_panel(model);
    workspace.open(PathBuf::from("battle.wowsreplay"), OpenTarget::ShowingTab);
    settle_in_real_time(&mut window);

    assert!(window.by_test_id("player-twitch").is_some(), "the row seen in chat carries the chip");
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

/// A battle held for its results is read again when its grace lapses, and not before.
#[test]
fn a_battle_held_for_its_results_is_read_again_when_its_grace_lapses() {
    let (mut window, workspace) = open();
    let arrivals = new_arrivals(workspace);
    let path = PathBuf::from("held.wowsreplay");
    let deadline = RawUploadDeadline(jiff::Timestamp::now() + jiff::SignedDuration::from_secs(60));

    arrivals.hold(workspace, path.clone(), deadline);
    window.advance(Duration::from_secs(30));
    assert!(arrivals.holds(&path), "the battle is held until its deadline");
    window.advance(Duration::from_secs(40));
    assert!(!arrivals.holds(&path), "the battle is read again once its grace lapses");
}

/// An open battle is read again once the game's names are in the new language, so its ships,
/// bots, builds and awards are named in it.
#[test]
fn the_open_replays_are_read_again_in_a_new_language() {
    let (mut window, workspace) = open();
    let path = PathBuf::from("battle.wowsreplay");
    workspace.open(path.clone(), OpenTarget::ShowingTab);
    window.advance(Duration::from_millis(32));
    let control = workspace.control_for_test(&path).expect("the replay is open");
    assert_eq!(control.asked_reparse_for_test(), None);

    workspace.services.game_data.note_catalogs_rewritten_for_test();
    window.advance(Duration::from_millis(32));

    assert_eq!(control.asked_reparse_for_test(), Some(ReparseCause::LanguageChanged));
}

/// A newer write of a held battle is read at once, and the hold for the older one is dropped.
#[test]
fn a_rewrite_of_a_held_battle_releases_its_hold() {
    let (mut window, workspace) = open();
    let arrivals = new_arrivals(workspace);
    let path = PathBuf::from("rewritten.wowsreplay");
    let deadline = RawUploadDeadline(jiff::Timestamp::now() + jiff::SignedDuration::from_secs(60));

    arrivals.hold(workspace, path.clone(), deadline);
    window.advance(Duration::from_millis(32));
    assert!(arrivals.holds(&path));
    arrivals.changed(workspace, path.clone(), FolderId::INSTALL);
    assert!(!arrivals.holds(&path), "the rewrite is read now rather than at the old deadline");
}

/// Two reads of one replay never run at once, so both cannot find it unsent and send it twice.
/// One asked for meanwhile runs once the first ends.
#[test]
fn a_replay_being_read_for_sharing_is_not_read_again_until_that_read_ends() {
    let (_window, workspace) = open();
    let reads = workspace.under_tab(SharingReads::new);
    let asked_again = Rc::new(Cell::new(0));
    let counted = Rc::clone(&asked_again);
    reads.on_again(move |_| counted.set(counted.get() + 1));
    let path = PathBuf::from("busy.wowsreplay");

    assert!(reads.begin(&path));
    assert!(!reads.begin(&path), "a second read waits for the first");
    assert_eq!(asked_again.get(), 0);
    reads.finish(&path);
    assert_eq!(asked_again.get(), 1, "the waiting read runs once the first has ended");
    assert!(reads.begin(&path), "and may start then");
    reads.finish(&path);
    assert_eq!(asked_again.get(), 1, "a read nothing waited on runs nothing after it");
}

/// A game directory set after the startup sweep is swept in turn.
#[test]
fn a_game_directory_set_later_is_swept_in_turn() {
    let (_window, workspace) = open();
    let services = workspace.services;
    services.settings.use_private_database_for_test();
    let arrivals = new_arrivals(workspace);
    let set_wows_dir = |dir: &str| {
        services.settings.edit(wows_toolkit_viewmodel::settings::keys::WOWS_DIR, |settings| {
            settings.wows_dir = dir.to_owned();
            dir.to_owned()
        })
    };

    set_wows_dir("/games/first");
    arrivals.sweep(workspace);
    assert_eq!(arrivals.swept().as_deref(), Some("/games/first"));

    set_wows_dir("/games/second");
    arrivals.sweep(workspace);
    assert_eq!(arrivals.swept().as_deref(), Some("/games/second"), "the new directory is swept");
}

/// Sending every listed battle before the settings have loaded says why nothing was sent.
#[test]
fn sending_every_replay_before_the_settings_load_says_so() {
    let mut window = crate::interaction_tests::open_bare_shell();
    window.press_character('k', Modifiers::PRIMARY);
    window.advance(Duration::from_millis(400));
    window.type_text("send all");
    window.advance(Duration::from_millis(400));

    let command = window.by_test_id("palette-item-send-all-replays").expect("the command is offered");
    window.click(command).expect("the command has a box");
    window.advance(Duration::from_millis(400));

    let toasts = window.texts_by_class("lui-toasts").concat();
    assert!(toasts.contains(t!("ui-replay-contribute_not_ready").as_ref()), "{toasts:?}");
}

//! Site screenshots of the armor viewer, player tracker, unpacker, launcher, and tactics board.

use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::test_support::DisplayScale;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_replays::ReplayFile;
use wows_replays::types::AccountId;
use wows_toolkit_launcher::test_support;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::load_current_build;
use wows_toolkit_replay_data::tactics::BoardTool;
use wows_toolkit_viewmodel::armor::options_panel::OptionSection;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::match_stats::Region;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentity;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Role;

use super::APPEARANCES;
use super::env_path;
use super::open_site_shell;
use super::options;
use super::save;
use super::show_appearance;
use super::use_site_storage;
use crate::interaction_tests::settle_in_real_time;
use crate::launcher::tests::LauncherWindow;
use crate::launcher::tests::eu;
use crate::launcher::tests::na;
use crate::launcher::tests::up_to_date;
use crate::services::GameDataRequest;
use crate::tabs::player_tracker::built_model;
use crate::tabs::unpacker::pane::PaneState;
use crate::windows::tactics::tests::BoardWindow;
use crate::windows::tactics::tests::tool_id;

fn press(window: &mut HeadlessWindow, test_id: &str) {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
    window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
    window.advance(Duration::from_millis(400));
}

/// The install's current build, read once per call.
fn game_data() -> GameDataCache {
    // The cache takes its dump directory from the storage directory when it is built.
    use_site_storage();
    let dir = env_path("WT_WOWS_DIR");
    let cache = GameDataCache::new(dir.clone());
    load_current_build(&dir, &cache).expect("the build loads");

    cache
}

/// A strong lobby's figures for the player at `index`: win rates 50 to 65 percent, PR 1200 to
/// 2600, and matching damage and battle counts. The spread is fixed so the shot is repeatable.
fn strong_stats(index: usize, account_id: AccountId, ship_id: wows_replays::types::GameParamId) -> PlayerStatsOut {
    let step = (index * 7 % 24) as f64 / 23.0;
    let win_rate = 50.0 + 15.0 * step;
    let pr = 1200.0 + 1400.0 * step;
    let battles = 3_000 + (index as i64 * 1_337 % 17_000);
    let ship_battles = 80 + (index as i64 * 53 % 900);

    PlayerStatsOut {
        account_id,
        region: "eu".to_owned(),
        ship_id,
        status: PlayerStatsStatus::Ok,
        battles: Some(battles),
        overall_win_rate: Some(win_rate),
        overall_avg_damage: Some(70_000 + (step * 60_000.0) as i64),
        ship_win_rate: Some(win_rate + 2.0 - 4.0 * ((index % 3) as f64 / 2.0)),
        ship_battles: Some(ship_battles),
        ship_avg_damage: Some(80_000 + (step * 75_000.0) as i64),
        ship_pr: Some(pr + 150.0 - 300.0 * ((index % 4) as f64 / 3.0)),
        pr: Some(pr),
    }
}

/// The newest twelve-a-side battle in the install's replays folder whose build `cache` loads,
/// for a full roster with its ships named.
fn random_battle(cache: &GameDataCache) -> LiveMatch {
    let dir = env_path("WT_WOWS_DIR").join("replays");
    let mut replays: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{} lists: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wowsreplay"))
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    replays.sort();

    replays
        .into_iter()
        .rev()
        .filter_map(|(_, path)| ReplayFile::from_file(&path).ok())
        .filter(|replay| replay.meta.vehicles.len() == 24)
        .filter_map(|replay| LiveMatch::from_meta(&replay.meta))
        .find(|live| live.build.is_some_and(|build| cache.get_or_load_build(build).is_ok()))
        .unwrap_or_else(|| panic!("{} holds a twelve-a-side battle of a loadable build", dir.display()))
}

#[test]
#[ignore = "site screenshot; reads the install named in site_shots/mod.rs"]
fn site_shots_armor() {
    for (appearance, suffix) in APPEARANCES {
        let mut window = open_site_shell(appearance);
        press(&mut window, "armor-viewer");
        let workspace = crate::tabs::armor_viewer::BUILT.with(std::cell::Cell::get).expect("the tab was built");
        workspace.services.game_data.adopt(GameDataRequest {
            wows_dir: Some(env_path("WT_WOWS_DIR")),
            cache_dir: String::new(),
            locale: None,
            auto_dump: false,
        });
        let library = workspace.library;
        for _ in 0..600 {
            settle_in_real_time(&mut window);
            if library.ready_untracked().is_some() {
                break;
            }
        }

        let bundle = library.ready_untracked().expect("the ship data loads");
        let ship = bundle.catalog.ships().find(|ship| ship.display_name == "Yamato").expect("Yamato is listed");
        workspace.open_ship(ship.param_index.clone(), ship.display_name.clone());
        for _ in 0..300 {
            settle_in_real_time(&mut window);
            if workspace.active_untracked().shown_ship_name().as_deref() == Some("Yamato") {
                break;
            }
        }

        // The textured ship: the hull opaque and the armor plates hidden.
        let viewport = workspace.active_untracked();
        viewport.set_all_parts_hidden();
        viewport.set_hull_opaque(true);
        workspace.open_section(OptionSection::HullParts);
        for _ in 0..2 {
            settle_in_real_time(&mut window);
        }
        save(&mut window, "model", suffix);

        // The armor alone: every plate shown and the hull meshes hidden.
        viewport.mutate_hull_visibility(|meshes| meshes.values_mut().for_each(|shown| *shown = false));
        viewport.set_all_parts_visible();
        workspace.open_section(OptionSection::Visibility);
        for _ in 0..2 {
            settle_in_real_time(&mut window);
        }
        save(&mut window, "armor", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install and replay named in site_shots/mod.rs"]
fn site_shots_tracker() {
    let cache = game_data();
    let live = random_battle(&cache);

    for (appearance, suffix) in APPEARANCES {
        // Wider than the other shots, at the same pixel size, so the detailed columns fit.
        let wide = HeadlessOptions {
            size: WindowSize { width: 1600.0, height: 1000.0 },
            scale: DisplayScale(1.0),
            ..options(appearance)
        };
        use_site_storage();
        let mut window = crate::interaction_tests::open_shell(wide);
        show_appearance(&mut window, appearance);
        press(&mut window, "player-tracker");
        settle_in_real_time(&mut window);
        let model = built_model().expect("the Player Tracker tab was built");
        model.set_filter(String::new());
        model.live.seed_over(live.clone(), cache.clone());
        window.advance(Duration::from_millis(400));

        const CLANS: [&str; 6] = ["LARCH", "HERON", "EAST", "MOOR", "CEDAR", "FJORD"];
        let mut identities = LiveIdentities { by_name: Default::default() };
        let mut stats = Vec::new();
        for (index, player) in live.players.iter().enumerate() {
            let account_id = AccountId(500_000_000 + index as i64);
            let clan = (index % 3 != 2).then(|| CLANS[index % CLANS.len()].to_owned());
            let identity = LiveIdentity { account_id, region: Some(Region::Eu), clan, clan_color: None };
            identities.by_name.insert(player.name.to_lowercase(), identity);
            stats.push(strong_stats(index, account_id, player.ship_id));
        }
        model.live.seed_stats(identities, stats);
        window.advance(Duration::from_millis(400));

        let label = rust_i18n::t!("ui.player_tracker.subtab_current_match").into_owned();
        let tab = window.by_role_and_label(Role::Tab, &label).expect("the dock lists the Current Match section");
        window.click(tab).expect("the section's tab can be pressed");
        window.advance(Duration::from_millis(400));
        press(&mut window, "tracker-view-mode-Detailed");
        for _ in 0..60 {
            settle_in_real_time(&mut window);
            if window.by_test_id("tracker-live-note").is_none() {
                break;
            }
        }
        save(&mut window, "tracker", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install named in site_shots/mod.rs"]
fn site_shots_unpacker() {
    for (appearance, suffix) in APPEARANCES {
        let mut window = open_site_shell(appearance);
        crate::interaction_tests::shell().apply_wows_dir(env_path("WT_WOWS_DIR").to_string_lossy().into_owned());
        press(&mut window, "unpacker");
        let model = crate::tabs::unpacker::built_model().expect("the Unpacker tab was built");
        let mut vfs = None;
        for _ in 0..120 {
            settle_in_real_time(&mut window);
            vfs = model.pkg.state.with_untracked(|state| match state {
                PaneState::Ready(loaded) => Some(loaded.vfs.clone()),
                _ => None,
            });
            if vfs.is_some() {
                break;
            }
        }

        let vfs = vfs.expect("the package archives load");
        let file = vfs
            .join("gui")
            .and_then(|gui| gui.walk_dir())
            .expect("the gui directory lists")
            .filter_map(Result::ok)
            .find(|path| path.as_str().ends_with(".xml"))
            .expect("the archives hold an XML file under gui");
        model.pkg.reveal(&file);
        model.open_viewer(file);
        for _ in 0..2 {
            settle_in_real_time(&mut window);
        }
        save(&mut window, "unpacker", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install named in site_shots/mod.rs"]
fn site_shots_launcher() {
    for (appearance, suffix) in APPEARANCES {
        use_site_storage();
        let mut launcher = LauncherWindow::open_with(options(appearance));
        show_appearance(&mut launcher.window, appearance);
        let settings = launcher.enabled_settings(&[(na(), "Captain_Ahab"), (eu(), "Queequeg")]);
        launcher.adopt(settings);
        launcher.found(up_to_date());
        launcher.set_session(na(), test_support::TestSession::SignedIn("Captain_Ahab".into()));
        launcher.press("launcher-play-menu");
        save(&mut launcher.window, "launcher", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install named in site_shots/mod.rs"]
fn site_shots_tactics() {
    let cache = game_data();
    for (appearance, suffix) in APPEARANCES {
        let mut board = BoardWindow::open_with(options(appearance));
        let shown = cache.clone();
        board.model.update(move |board| board.set_game_data(Some(shown)));
        let map = board
            .model
            .with_untracked(|board| board.maps().iter().find(|map| map.space.contains("16_OC_bees_to_honey")).cloned())
            .or_else(|| board.model.with_untracked(|board| board.maps().first().cloned()))
            .expect("the install has maps");
        board.model.update(|board| board.set_map(map));
        for _ in 0..30 {
            board.window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(50));
        }

        let map = board.window.by_test_id("tactics-map").expect("the map is drawn");
        let bounds = board.window.bounds(map).expect("the map has a box");
        // The map art is the largest square the element holds, centred in it.
        let side = bounds.size.width.0.min(bounds.size.height.0);
        let left = bounds.origin.x.0 + (bounds.size.width.0 - side) / 2.0;
        let top = bounds.origin.y.0 + (bounds.size.height.0 - side) / 2.0;
        let at = |x: f32, y: f32| {
            zgui::geom::Point::new(zgui::geom::DevicePx(left + side * x), zgui::geom::DevicePx(top + side * y))
        };

        board.press("tactics-add-cap");
        board.window.click_at(at(0.5, 0.5));
        board.settle();
        board.press("tactics-add-cap");

        let ships = [("X Yamato", 0.3, 0.25), ("X Des Moines", 0.38, 0.32), ("X Shimakaze", 0.22, 0.4)];
        let enemies = [("X Montana", 0.72, 0.7), ("X Moskva", 0.65, 0.78), ("X Gearing", 0.8, 0.6)];
        for (enemy, list) in [(false, ships), (true, enemies)] {
            if enemy {
                board.press("tactics-ship-side");
            }
            for (label, x, y) in list {
                board.press("tactics-ship-search");
                let name = label.trim_start_matches("X ");
                board.window.type_text(name);
                board.settle();
                let choice = board.window.by_label(label).unwrap_or_else(|| panic!("{label} is offered"));
                board.window.click(choice).expect("the ship can be picked");
                board.settle();
                board.window.click_at(at(x, y));
                board.settle();
            }
        }

        board.press(&tool_id(BoardTool::Arrow));
        board.window.drag_from(at(0.34, 0.3), at(0.48, 0.45), 8);
        board.window.drag_from(at(0.3, 0.42), at(0.42, 0.6), 8);
        for _ in 0..20 {
            board.window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(50));
        }
        save(&mut board.window, "tactics", suffix);
    }
}

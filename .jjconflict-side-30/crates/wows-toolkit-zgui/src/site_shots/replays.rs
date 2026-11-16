//! Site screenshots of the replay inspector, renderer, listing, search, and stats.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use landaire_ui::Appearance;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessWindow;
use wows_toolkit_replay_data::columns::ReplayColumn;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use super::APPEARANCES;
use super::env_path;
use super::open_site_shell;
use super::save;
use crate::interaction_tests::settle_in_real_time;
use crate::services::GameDataState;
use crate::tabs::replays::BUILT;
use crate::tabs::replays::testing::allow_fetches;
use crate::tabs::replays::workspace::Docked;
use crate::tabs::replays::workspace::OpenTarget;
use crate::tabs::replays::workspace::ReplayWorkspace;
use crate::windows::renderer::session::RenderRequest;
use crate::windows::renderer::session::Status;

/// The shell on the replays tab with the install's game data loaded.
fn open_with_game_data(appearance: Appearance) -> (HeadlessWindow, ReplayWorkspace) {
    let mut window = open_site_shell(appearance);
    let workspace = BUILT.with(Cell::get).expect("the replays tab made its workspace");
    workspace.shell.apply_wows_dir(env_path("WT_WOWS_DIR").to_string_lossy().into_owned());
    for _ in 0..30 {
        settle_in_real_time(&mut window);
        if matches!(workspace.services.game_data.state_untracked(), GameDataState::Ready(_) | GameDataState::Failed(_))
        {
            break;
        }
    }
    match workspace.services.game_data.state_untracked() {
        GameDataState::Ready(_) => {}
        GameDataState::Failed(reason) => panic!("the install's game data did not load: {reason}"),
        GameDataState::NoInstall | GameDataState::Loading { .. } => {
            panic!("the install's game data did not load within the wait")
        }
    }

    settle_in_real_time(&mut window);
    if let Some(cancel) = window.by_test_id("download-offer-cancel") {
        window.click(cancel).expect("the offer's cancel has a box");
        settle_in_real_time(&mut window);
    }

    (window, workspace)
}

/// The showcase replay open in the inspector.
fn open_showcase(appearance: Appearance) -> (HeadlessWindow, ReplayWorkspace, PathBuf) {
    let (mut window, workspace) = open_with_game_data(appearance);
    let replay = env_path("WT_SITE_REPLAY");
    workspace.open(replay.clone(), OpenTarget::ShowingTab);
    for _ in 0..4 {
        settle_in_real_time(&mut window);
    }

    (window, workspace, replay)
}

/// The recording player's cell in `column`, found by the row's owner mark.
fn own_cell(window: &HeadlessWindow, column: ReplayColumn) -> Element {
    let mark = window.by_attribute("data-mark", "owner").expect("the recording player's row is drawn");
    let line = window.bounds(mark).expect("the mark has a box");
    let middle = line.origin.y.0 + line.size.height.0 / 2.0;

    window
        .all_by_attribute("data-column", column.storage_key())
        .into_iter()
        .find(|cell| {
            window
                .bounds(*cell)
                .is_some_and(|cell| cell.origin.y.0 <= middle && middle < cell.origin.y.0 + cell.size.height.0)
        })
        .expect("the recording player's row has the cell")
}

fn press(window: &mut HeadlessWindow, test_id: &str) {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
    window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
    window.advance(Duration::from_millis(400));
}

#[test]
#[ignore = "site screenshot; reads the install and replay named in site_shots/mod.rs"]
fn site_shots_replay() {
    let _fetches = allow_fetches();
    for (appearance, suffix) in APPEARANCES {
        let (mut window, _workspace, _replay) = open_showcase(appearance);
        window.advance(Duration::from_millis(500));
        save(&mut window, "replay", suffix);

        let cell = own_cell(&window, ReplayColumn::PersonalRating);
        window.hover(cell).expect("the cell has a box");
        window.advance(Duration::from_millis(900));
        save(&mut window, "rating", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install and replay named in site_shots/mod.rs"]
fn site_shots_renderer() {
    let _fetches = allow_fetches();
    for (appearance, suffix) in APPEARANCES {
        let (mut window, workspace, replay) = open_showcase(appearance);
        workspace.render(RenderRequest { path: replay.clone(), alts: Vec::new() });
        settle_in_real_time(&mut window);
        workspace.remove(&Docked::Replay(replay.clone()));
        press(&mut window, "replay-listing-toggle");
        let session = workspace.session_for_test(&replay).expect("the renderer is docked");

        let started = Instant::now();
        while session.status.with_untracked(|status| matches!(status, Status::Baking)) {
            assert!(started.elapsed() < Duration::from_secs(1200), "the bake finishes");
            settle_in_real_time(&mut window);
        }
        if let Status::Failed(reason) = session.status.with_untracked(Clone::clone) {
            panic!("the bake failed: {reason}");
        }

        // Four minutes in, two caps are contested and most ships are in range of each other.
        session.go_to_elapsed(240.0);
        for _ in 0..3 {
            settle_in_real_time(&mut window);
        }
        save(&mut window, "renderer", suffix);

        // The listing's state is saved with the settings; open it again for the other shots.
        press(&mut window, "replay-listing-toggle");
        settle_in_real_time(&mut window);
    }
}

#[test]
#[ignore = "site screenshot; reads the install and replay named in site_shots/mod.rs"]
fn site_shots_listing() {
    let _fetches = allow_fetches();
    for (appearance, suffix) in APPEARANCES {
        let (mut window, _workspace, _replay) = open_showcase(appearance);
        // Group headings share the row class; only replay rows hold a thumbnail.
        let rows = window.all_by_class("wt-row__thumb-slot");
        let row = *rows.get(3).expect("the install lists replays");
        window.hover(row).expect("the row has a box");
        for _ in 0..30 {
            settle_in_real_time(&mut window);
            if window.by_test_id("hover-preview-map").is_some() && window.by_class("zui-spinner").is_none() {
                break;
            }
        }
        save(&mut window, "listing", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install and replay named in site_shots/mod.rs"]
fn site_shots_palette() {
    let _fetches = allow_fetches();
    for (appearance, suffix) in APPEARANCES {
        let (mut window, _workspace, _replay) = open_showcase(appearance);
        window.press_character('k', Modifiers::PRIMARY);
        window.advance(Duration::from_millis(300));
        window.type_text("go to");
        window.advance(Duration::from_millis(400));
        save(&mut window, "palette", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install and replay index named in site_shots/mod.rs"]
fn site_shots_search() {
    for (appearance, suffix) in APPEARANCES {
        let (mut window, _workspace) = open_with_game_data(appearance);
        press(&mut window, "search");
        crate::tabs::search::tests::built().set_query_text("outcome:win".to_owned());
        for _ in 0..3 {
            settle_in_real_time(&mut window);
        }
        save(&mut window, "search", suffix);
    }
}

#[test]
#[ignore = "site screenshot; reads the install and replay index named in site_shots/mod.rs"]
fn site_shots_stats() {
    for (appearance, suffix) in APPEARANCES {
        let (mut window, _workspace) = open_with_game_data(appearance);
        crate::interaction_tests::shell().open_directory_stats(env_path("WT_WOWS_DIR").join("replays"));
        for _ in 0..60 {
            settle_in_real_time(&mut window);
            if window.by_test_id("directory-stats-reload").is_some() {
                break;
            }
        }

        // The twenty most recent games cover five ships, which keeps the table and chart readable.
        press(&mut window, "stats-limit-enabled");
        press(&mut window, "stats-limit-count");
        window.press_character('a', Modifiers::PRIMARY);
        window.type_text("20");
        window.press_key(NamedKey::Enter, Modifiers::NONE);
        settle_in_real_time(&mut window);
        save(&mut window, "stats", suffix);
    }
}

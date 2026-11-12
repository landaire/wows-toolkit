use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::model::ChatMessage;
use wows_toolkit_replay_data::model::MatchContext;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::ReplayReportModel;
use wows_toolkit_replay_data::sample::sample_model;
use wows_toolkit_replay_data::test_support::FIXTURE_PR_SHIP_ID;
use wows_toolkit_replay_data::test_support::base_row;
use wows_toolkit_replay_data::test_support::fixture_empty_battle_report;
use wows_toolkit_replay_data::test_support::fixture_personal_rating_data;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui_ui::ButtonProps;

use super::DebugPayloads;
use super::Loaded;
use super::PanelEvents;
use super::PanelSetup;
use super::PanelTable;
use super::RenderRequest;
use super::ReplayPanelProps;
use super::TableSlot;
use super::export_extension;
use super::figures::PR_TILE_ID;
use super::figures::personal_rating_label;
use super::loaded_from_model;
use super::write_export;
use crate::shell::ShellHarnessProps;

thread_local! {
    /// The battle the next panel built on this thread shows instead of reading its replay.
    pub(super) static SEED: RefCell<Option<Loaded>> = const { RefCell::new(None) };
}

/// A won battle whose self row dealt exactly the fixture's expected damage and frags, so it
/// rates 1650.
fn model_at_expected_values() -> ReplayReportModel {
    let self_row = PlayerRow {
        ship_id: Some(FIXTURE_PR_SHIP_ID.into()),
        actual_damage: Some(50_000),
        kills: Some(1),
        ..base_row(1, Relation::new(0), true)
    };
    let enemy = PlayerRow { ship_id: None, actual_damage: Some(20_000), ..base_row(2, Relation::new(2), false) };

    ReplayReportModel {
        self_team: TeamId::from(0i64),
        rows: vec![self_row, enemy],
        battle_result: Some(BattleResult::Win(0)),
        columns: ReplayColumn::ALL.to_vec(),
        map: "Ocean".to_string(),
        chat: Vec::new(),
        timestamp: sample_model().timestamp,
        context: MatchContext {
            clan_tag: Some("[WG]".to_string()),
            player_name: "Player".to_string(),
            game_type: "Random Battle".to_string(),
            version: "14.1.0".to_string(),
            game_mode: "Domination".to_string(),
            map: "Ocean".to_string(),
        },
    }
}

fn loaded(model: ReplayReportModel, results: bool) -> Loaded {
    let export = ExportedMatch::new(&fixture_empty_battle_report(), &[], &[], true);
    let payloads = DebugPayloads {
        raw_metadata: "{\"map\": \"Ocean\"}".to_string(),
        raw_results: results.then(|| "{}".to_string()),
        mapped_results: None,
    };
    loaded_from_model(model, export, "battle".to_string(), payloads)
}

fn chat_message() -> ChatMessage {
    ChatMessage {
        clock: Default::default(),
        sender_relation: Some(Relation::new(1)),
        sender_name: "Ally".to_string(),
        channel: ChatChannel::Team,
        message: "push B".to_string(),
        clan_tag: None,
        clan_color: None,
    }
}

/// A table that shows the self row's detail and a button that asks for a row's metadata.
fn fake_table() -> TableSlot {
    Rc::new(|table: PanelTable| {
        let PanelTable { model, detail, on_view_raw_json, .. } = table;
        let current = model.get();
        let rows: super::expanded::Rows = Arc::from(current.rows.clone());
        let row =
            Arc::new(current.rows.iter().find(|row| row.is_self).cloned().unwrap_or_else(|| current.rows[0].clone()));
        let columns: Rc<[ReplayColumn]> =
            Rc::from(vec![ReplayColumn::Name, ReplayColumn::Skills, ReplayColumn::ActualDamage]);
        let expanded = detail(row, rows, columns);
        AnyView::new(view! {
            column(attr:data-testid = "fake-table") {
                Button(attr:data-testid = "fake-raw", on:click = move |_| on_view_raw_json.run("{\"id\": 1}".to_string())) {"raw"}
                {expanded}
            }
        })
    })
}

struct Opened {
    window: HeadlessWindow,
}

#[derive(Default)]
struct Opening {
    seed: Option<Loaded>,
    events: PanelEvents,
    rating: Option<Signal<Option<Arc<PersonalRatingData>>, LocalStorage>>,
    theme: Option<ThemeChoice>,
    drawing: Option<Drawing>,
    size: Option<WindowSize>,
}

fn open(opening: Opening) -> Opened {
    let Opening { seed, events, rating, theme, drawing, size } = opening;
    SEED.with(|slot| *slot.borrow_mut() = seed);
    let mut options = HeadlessOptions::new(size.unwrap_or(WindowSize { width: 1100.0, height: 700.0 }));
    if let Some(drawing) = drawing {
        options.drawing = drawing;
    }
    let theme = theme.unwrap_or(ThemeChoice::Dark);
    let mut window = HeadlessWindow::open(options, move || {
        let setup = PanelSetup {
            path: PathBuf::from("battle.wowsreplay"),
            columns: ReplayColumn::ALL.to_vec(),
            personal_rating: None,
        };
        let events = events.clone();
        match rating {
            Some(rating) => AnyView::new(view! {
                ShellHarness(theme = theme) {
                    ReplayPanel(setup = setup, table = fake_table(), events = events, personal_rating = rating)
                }
            }),
            None => AnyView::new(view! {
                ShellHarness(theme = theme) {
                    ReplayPanel(setup = setup, table = fake_table(), events = events)
                }
            }),
        }
    })
    .expect("the panel opens");
    window.advance(Duration::from_millis(600));
    Opened { window }
}

/// Until the replay is read the panel says it is loading.
#[test]
fn a_replay_being_read_shows_that_it_is_loading() {
    let Opened { window } = open(Opening::default());
    assert!(window.by_test_id("replay-panel-loading").is_some());
    assert!(window.by_test_id("replay-table-slot").is_none());
}

/// The header names the match and says when the results are missing.
#[test]
fn the_header_names_the_match_and_flags_missing_results() {
    let Opened { window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), false)), ..Opening::default() });

    assert!(window.by_test_id("replay-match-context").is_some());
    assert!(window.by_test_id("replay-incomplete-results").is_some(), "a replay without results says so");
    assert!(window.by_attribute("data-result", "win").is_some(), "the band is tinted by the result");
    assert!(window.by_test_id("replay-table-slot").is_some(), "the table is drawn under the header");

    drop(window);
    let Opened { window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), true)), ..Opening::default() });
    assert!(window.by_test_id("replay-incomplete-results").is_none());
}

fn rated() -> ReplayReportModel {
    let mut model = model_at_expected_values();
    model.populate_personal_ratings(&fixture_personal_rating_data());
    model
}

/// The PR tile reads as the self row's rating.
#[test]
fn the_pr_badge_reports_the_self_rows_rating() {
    let model = rated();
    let expected = personal_rating_label(model.rows[0].personal_rating.as_ref().expect("the self row is rated"));
    let Opened { mut window } = open(Opening { seed: Some(loaded(model, true)), ..Opening::default() });

    let tile = window.by_test_id(PR_TILE_ID).expect("the PR tile is shown");
    assert_eq!(window.a11y(tile).and_then(|state| state.label), Some(expected));
}

#[test]
fn a_replay_with_no_rating_table_shows_no_pr_badge() {
    let Opened { window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), true)), ..Opening::default() });
    assert!(window.by_test_id(PR_TILE_ID).is_none());
    assert!(window.by_test_id("replay-damage-tile").is_some(), "the other figures are shown");
}

/// A rating table that lands after the battle is read still rates it.
#[test]
fn a_rating_table_arriving_after_the_parse_still_fills_the_badge_in() {
    let table = RwSignal::new_local(None);
    let Opened { mut window } = open(Opening {
        seed: Some(loaded(model_at_expected_values(), true)),
        rating: Some(table.into()),
        ..Opening::default()
    });
    assert!(window.by_test_id(PR_TILE_ID).is_none());

    table.set(Some(Arc::new(fixture_personal_rating_data())));
    window.advance(Duration::from_millis(600));
    assert!(window.by_test_id(PR_TILE_ID).is_some());
}

/// The team damage line splits each side's results total.
#[test]
fn the_team_damage_split_is_drawn() {
    let Opened { window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), true)), ..Opening::default() });
    assert!(window.by_test_id("replay-team-damage").is_some());
    assert_eq!(super::figures::team_damage(&model_at_expected_values().rows), (50_000, 20_000));
}

/// The chat toggle opens the chat beside the table, and a chat-less replay cannot open it.
#[test]
fn the_chat_toggle_opens_the_chat_only_when_there_is_some() {
    let Opened { mut window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), true)), ..Opening::default() });
    let toggle = window.by_test_id("replay-chat-toggle").expect("the toggle is shown");
    assert!(window.a11y(toggle).is_some_and(|state| state.disabled), "no chat, no panel");

    let mut model = model_at_expected_values();
    model.chat = vec![chat_message()];
    drop(window);
    let Opened { mut window } = open(Opening { seed: Some(loaded(model, true)), ..Opening::default() });
    let toggle = window.by_test_id("replay-chat-toggle").expect("the toggle is shown");
    window.click(toggle).expect("the toggle is clickable");
    window.advance(Duration::from_millis(600));
    assert!(window.by_test_id("replay-chat").is_some(), "the chat slides in");
    assert!(window.by_attribute("data-selected", "").is_some(), "the toggle shows it is on");
}

/// The table's request for a row's metadata opens it in the side panel, which has its own close.
#[test]
fn a_rows_raw_metadata_opens_in_the_side_panel_and_closes() {
    let Opened { mut window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), true)), ..Opening::default() });
    let raw = window.by_test_id("fake-raw").expect("the fake table is in the slot");
    window.click(raw).expect("clickable");
    window.advance(Duration::from_millis(600));
    assert!(window.by_test_id("replay-raw-json").is_some());

    let close = window.by_test_id("replay-side-panel-close").expect("the panel has a close");
    window.click(close).expect("clickable");
    window.advance(Duration::from_millis(600));
    assert!(window.by_test_id("replay-side-panel-close").is_none(), "the panel is shut");
}

/// The Actions menu asks for the battle it reads to be rendered.
#[test]
fn the_actions_menu_offers_to_render_the_replay_it_is_reading() {
    let asked: Rc<RefCell<Vec<RenderRequest>>> = Rc::default();
    let heard = Rc::clone(&asked);
    let events = PanelEvents {
        on_render: Some(UnsyncCallback::new(move |request| heard.borrow_mut().push(request))),
        ..PanelEvents::default()
    };
    let Opened { mut window } =
        open(Opening { seed: Some(loaded(model_at_expected_values(), true)), events, ..Opening::default() });

    let actions = window.by_test_id("replay-actions").expect("the menu is offered");
    window.click(actions).expect("clickable");
    window.advance(Duration::from_millis(300));
    let render = window.by_test_id("replay-render").expect("the menu offers to render");
    window.click(render).expect("clickable");
    window.advance(Duration::from_millis(300));

    assert_eq!(*asked.borrow(), vec![RenderRequest { path: PathBuf::from("battle.wowsreplay"), alts: Vec::new() }]);
}

/// A test ship's own figures can be hidden from the menu, which takes them off the tiles.
#[test]
fn a_test_ship_s_own_figures_can_be_hidden() {
    let mut model = model_at_expected_values();
    model.rows[0].is_test_ship = true;
    let Opened { mut window } = open(Opening { seed: Some(loaded(model, true)), ..Opening::default() });
    assert!(window.by_test_id("replay-damage-tile").is_some());

    let actions = window.by_test_id("replay-actions").expect("the menu is offered");
    window.click(actions).expect("clickable");
    window.advance(Duration::from_millis(300));
    let hide = window.by_test_id("replay-hide-my-stats").expect("a test ship's figures can be hidden");
    window.click(hide).expect("clickable");
    window.advance(Duration::from_millis(300));

    assert!(window.by_test_id("replay-damage-tile").is_none(), "the hidden figures leave the tiles");
}

/// Each format writes its own shape.
#[test]
fn each_export_format_writes_its_own_shape() {
    let export = ExportedMatch::new(&fixture_empty_battle_report(), &[], &[], true);
    let dir = tempfile::tempdir().expect("a temporary directory");

    for format in [ReplayExportFormat::Json, ReplayExportFormat::Cbor, ReplayExportFormat::Csv] {
        let path = dir.path().join(format!("match.{}", export_extension(format)));
        write_export(&export, &path, format).expect("the export writes");
        assert!(path.is_file(), "{format:?} wrote no file");
    }

    assert!(!std::fs::read(dir.path().join("match.cbor")).expect("the CBOR exists").is_empty());
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("match.json")).expect("the JSON exists"))
            .expect("the JSON parses back");
    assert!(json.get("metadata").is_some(), "the document keeps its metadata");
}

/// The panel with the sample battle, its chat open, in each appearance.
#[test]
#[ignore = "draws on the graphics device; run by hand to review the panel"]
fn the_replay_panel_renders_in_dark_and_light() {
    for (theme, name) in [(ThemeChoice::Dark, "replay-panel-dark"), (ThemeChoice::Light, "replay-panel-light")] {
        let mut model = sample_model();
        model.populate_personal_ratings(&fixture_personal_rating_data());
        model.battle_result = Some(BattleResult::Win(0));
        model.context = model_at_expected_values().context;
        model.chat = vec![
            chat_message(),
            ChatMessage {
                sender_relation: Some(Relation::new(2)),
                sender_name: "Enemy".to_string(),
                channel: ChatChannel::Global,
                message: "gl hf".to_string(),
                ..chat_message()
            },
        ];
        let Opened { mut window } = open(Opening {
            seed: Some(loaded(model, true)),
            theme: Some(theme),
            drawing: Some(Drawing::Device),
            size: Some(WindowSize { width: 1400.0, height: 900.0 }),
            ..Opening::default()
        });
        let toggle = window.by_test_id("replay-chat-toggle").expect("the toggle is shown");
        window.click(toggle).expect("clickable");
        window.advance(Duration::from_millis(800));
        let path = crate::interaction_tests::save_shot(&mut window, name);
        println!("{}", path.display());
    }
}

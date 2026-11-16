//! The Armor Viewer tab driven in the headless shell.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::DockTarget;
use landaire_ui::Edge;
use landaire_ui::MARK_CLASS;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::decoder::HitType;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::analyzer::decoder::ShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::ShotId;
use wows_replays::types::WorldPos;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::hull_picking::HullMeshIndex;
use wows_toolkit_armor::load_model::ProblemReason;
use wows_toolkit_armor::load_model::SceneProblem;
use wows_toolkit_armor::load_model::fixtures;
use wows_toolkit_armor::load_ship::ArmorZone;
use wows_toolkit_armor::load_ship::EmptyScene;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::SceneSubject;
use wows_toolkit_armor::load_ship::ShipLoadError;
use wows_toolkit_armor::load_ship::ShipSubject;
use wows_toolkit_armor::load_ship::ZonePart;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::scene_load::FirstStage;
use wows_toolkit_armor::textures::LocatedTexture;
use wows_toolkit_armor::textures::PropertyTexture;
use wows_toolkit_armor::textures::SceneMaterial;
use wows_toolkit_armor::textures::SceneTextures;
use wows_toolkit_armor::textures::TextureLookup;
use wows_toolkit_armor::viewport::renderer::HighlightDepth;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollowed;
use wows_toolkit_services::dialog::Asked;
use wows_toolkit_viewmodel::settings::XrayHighlight;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::hardpoint::Hardpoint;
use wowsunpack::export::hardpoint::HardpointKind;
use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::material_textures::PropertyName;
use wowsunpack::export::material_textures::TextureFile;
use wowsunpack::export::material_textures::TextureFiles;
use wowsunpack::export::material_textures::TextureSize;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::mount_armor::VehicleParamIndex;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CameraTrajectory;
use wowsunpack::game_types::Vec2;
use wowsunpack::game_types::Vec3;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::tabs::armor_viewer::BUILT;
use crate::tabs::armor_viewer::library::ArmorBundle;
use crate::tabs::armor_viewer::library::ViewerFeed;
use crate::tabs::armor_viewer::options::OptionSection;
use crate::tabs::armor_viewer::options::OptionSectionsOpen;
use crate::tabs::armor_viewer::options::TextureFocus;
use crate::tabs::armor_viewer::viewport::OpeningLayer;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;
use crate::tabs::armor_viewer::workspace::PaneId;

/// The shell, open on the Armor Viewer tab.
struct ArmorWindow {
    window: HeadlessWindow,
    workspace: ArmorWorkspace,
}

impl ArmorWindow {
    fn open(options: HeadlessOptions) -> Self {
        let mut window = open_shell(options);
        let tab = window.by_test_id("armor-viewer").expect("the strip has an Armor Viewer tab");
        window.click(tab).expect("the tab can be pressed");
        settle_in_real_time(&mut window);
        let workspace = BUILT.with(std::cell::Cell::get).expect("the Armor Viewer tab was built");
        Self { window, workspace }
    }

    /// Tall enough that an open tree is in view below the tools.
    fn open_tall() -> Self {
        let size = landaire_ui::test_support::WindowSize { width: 1600.0, height: 2400.0 };
        Self::open(HeadlessOptions::new(size))
    }

    fn settle(&mut self) {
        self.window.advance(Duration::from_millis(400));
    }

    fn shown(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    fn click(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
        self.settle();
    }

    fn type_into(&mut self, test_id: &str, text: &str) {
        self.click(test_id);
        self.window.type_text(text);
        self.settle();
    }

    fn erase(&mut self, typed: &str) {
        for _ in typed.chars() {
            self.window.press_key(NamedKey::Backspace, Modifiers::NONE);
        }
        self.settle();
    }

    fn marked(&self) -> Vec<String> {
        self.window.texts_by_class(MARK_CLASS)
    }

    /// Where pane `id` is drawn, as its left and top edges. `None` while it is not on screen.
    fn drawn_at(&self, id: PaneId) -> Option<(f32, f32)> {
        let element = self.window.by_attribute("data-panel", id.panel_id().as_str())?;
        let bounds = self.window.bounds(element)?;

        (bounds.size.width.0 > 0.0 && bounds.size.height.0 > 0.0).then_some((bounds.origin.x.0, bounds.origin.y.0))
    }

    /// Presses the close button of the tab named `name` that sits furthest right.
    fn close_rightmost(&mut self, name: &str) {
        let label = format!("Close {name}");
        let closes: Vec<_> = self
            .window
            .by_role(Role::Button)
            .into_iter()
            .filter(|button| self.window.a11y(*button).and_then(|state| state.label).as_deref() == Some(label.as_str()))
            .collect();
        let rightmost = closes
            .into_iter()
            .filter_map(|button| Some((button, self.window.bounds(button)?.origin.x.0)))
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(button, _)| button)
            .unwrap_or_else(|| panic!("a tab named {name} has a close button"));
        self.window.click(rightmost).expect("the close button can be pressed");
        self.settle();
    }

    /// The tab named `name`.
    fn tab(&mut self, name: &str) -> landaire_ui::test_support::Element {
        self.window.by_role_and_label(Role::Tab, name).unwrap_or_else(|| panic!("a tab is named {name}"))
    }

    /// Lets loads run until `done`.
    fn settle_until(&mut self, done: impl Fn() -> bool) {
        for _ in 0..100 {
            if done() {
                return;
            }
            settle_in_real_time(&mut self.window);
        }
        assert!(done(), "the loads settle");
    }

    /// Closes pane `id`'s tab, as its close button does.
    fn close_tab(&mut self, id: PaneId) {
        self.workspace.dock.close(&id.panel_id()).expect("the pane's tab closes");
        self.settle();
    }

    /// Hands the active pane `scene`, as a finished load does, and leaves only `sections` open.
    fn install(&mut self, scene: LoadedShipArmor, sections: &[OptionSection]) {
        let workspace = self.workspace;
        workspace.active_untracked().show_armor(Arc::new(scene), workspace.opening_layer.get_untracked());
        workspace.sections.set(OptionSectionsOpen {
            trajectory: false,
            pen_check: false,
            visibility: false,
            hull: false,
            textures: false,
            display: false,
            camera_rings: false,
            lighting: false,
        });
        for section in sections {
            workspace.sections.update(|open| open.open(*section));
        }
        self.settle();
    }
}

fn group_of(workspace: ArmorWorkspace, id: PaneId) -> Option<landaire_ui::GroupId> {
    workspace.dock.group_of(&id.panel_id())
}

fn turret() -> ModelPath {
    ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path")
}

/// Ship data over an empty store: a load result lands without reading it.
fn bundle() -> ArmorBundle {
    let vfs: VfsPath = MemoryFS::new().into();
    let metadata = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds");
    let assets = ShipAssets::from_parts(&vfs, Arc::new(metadata), Arc::new(Vec::new()));

    ArmorBundle { assets: Arc::new(assets), catalog: Arc::new(ShipCatalog { nations: Vec::new(), hidden: Vec::new() }) }
}

/// A model with two armor zones and two hull part groups.
fn two_zone_scene() -> LoadedShipArmor {
    let part = |name: &str, plate: i32| ZonePart { name: name.to_string(), plates: vec![plate] };
    let mut scene = fixtures::armored_model_scene(turret());
    scene.zone_part_plates = vec![
        ArmorZone { name: "Bow".to_string(), parts: vec![part("Bow_Belt", 1000), part("Bow_Deck", 250)] },
        ArmorZone { name: "Stern".to_string(), parts: vec![part("Stern_Deck", 250)] },
    ];
    scene.hull_part_groups = vec![
        ("Hull".to_string(), vec!["Hull_Fore".to_string(), "Hull_Aft".to_string()]),
        ("Main Battery".to_string(), vec!["Turret_A".to_string()]),
    ];

    scene
}

/// A ship with one camera orbit, over the armored model fixture.
fn ship_scene() -> LoadedShipArmor {
    let mut scene = fixtures::armored_model_scene(turret());
    scene.subject =
        SceneSubject::Ship(ShipSubject { vehicle_param_index: VehicleParamIndex::new("PASB001"), parts: Vec::new() });
    let orbit = CameraTrajectory {
        pos_center: [Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)],
        semi_axes: [Vec2::new(6.0, 9.0), Vec2::new(6.0, 9.0)],
        tags: String::new(),
        ignore_height_multiplier: true,
        outer: None,
    };
    scene.camera_trajectories = vec![("Default".to_string(), orbit)];

    scene
}

fn camo(id: usize, name: &str, origin: CamoOrigin) -> CamoSchemeInfo {
    CamoSchemeInfo {
        id: CamoSchemeId(id),
        display_name: name.to_string(),
        raw_name: name.to_lowercase(),
        origin,
        use_color_scheme: false,
        uv_transforms: HashMap::new(),
    }
}

/// A shell landing on the ship `seconds` into the battle.
fn landed_at(seconds: f32) -> PreExtractedHit {
    let victim = EntityId::from(3u32);
    PreExtractedHit {
        clock: GameClock(seconds),
        hit: ResolvedShotHit {
            clock: GameClock(seconds),
            hit: ShotHit {
                owner_id: victim,
                hit_type: HitType {
                    collision: Recognized::Unknown("0".to_string()),
                    shell_hit: Recognized::Unknown("0".to_string()),
                    raw: 0,
                },
                shot_id: ShotId::from(1u32),
                position: WorldPos::new(0.0, 0.0, 0.0),
                terminal_ballistics: None,
            },
            victim_entity_id: victim,
            salvo: None,
            fired_at: None,
            victim_pose: None,
        },
    }
}

/// Playback moved to the last of `hits`, with the ship at `health`.
fn followed(hits: &[f32], health: f32) -> ArmorFollowed {
    let at = GameClock(*hits.last().expect("playback has reached a hit"));
    ArmorFollowed { hits: hits.iter().copied().map(landed_at).collect(), health: Some(health), at }
}

/// The ships list and the options rail each have a toggle on the status bar, which puts the panel
/// away and brings it back.
#[test]
fn the_status_bar_toggles_the_ships_list_and_the_options_rail() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    assert!(armor.shown("armor-options-panel"), "the rail opens up");

    armor.click("armor-options-toggle");
    assert!(!armor.workspace.options_open.get_untracked());
    armor.window.advance(Duration::from_millis(600));
    assert!(!armor.shown("armor-options-panel"), "the rail is put away");

    armor.click("armor-options-toggle");
    assert!(armor.workspace.options_open.get_untracked());

    armor.click("armor-ships-toggle");
    assert!(!armor.workspace.sidebar_open.get_untracked(), "the ships list is put away");
    armor.click("armor-ships-toggle");
    assert!(armor.workspace.sidebar_open.get_untracked());
}

/// Compare opens a copy of the active pane in a split of its own beside it, which becomes the
/// active pane. Stacking the panes lays the open ones one above another, and the next split opens
/// below. A press in a pane makes it active.
#[test]
fn compare_opens_a_split_beside_the_active_pane() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let first = workspace.active_id();
    workspace
        .active_untracked()
        .show_armor(Arc::new(fixtures::model_scene(turret(), Vec::new())), workspace.opening_layer.get_untracked());

    armor.click("armor-sidebar-compare");
    let compared = workspace.active_id();
    assert_ne!(compared, first, "the new pane is the active one");
    assert!(workspace.active_untracked().shown().is_some(), "and shows what the first one did");
    assert_ne!(group_of(workspace, compared), group_of(workspace, first), "in a group of its own");
    let (left, right) = (armor.drawn_at(first).expect("drawn"), armor.drawn_at(compared).expect("drawn"));
    assert!(left.1 == right.1 && left.0 < right.0, "side by side: {left:?} {right:?}");
    assert!(armor.shown("armor-options-target"), "the rail names the pane it acts on");

    armor.click("armor-sidebar-common");
    armor.click("armor-pane-stack");
    armor.window.press_key(NamedKey::Escape, Modifiers::NONE);
    armor.settle();
    assert!(workspace.common().stack_panes, "the switch stacks the panes");
    let (top, bottom) = (armor.drawn_at(first).expect("drawn"), armor.drawn_at(compared).expect("drawn"));
    assert!(top.0 == bottom.0 && top.1 < bottom.1, "the open panes are stacked: {top:?} {bottom:?}");
    armor.click("armor-sidebar-compare");
    let stacked = workspace.active_id();
    let below = armor.drawn_at(stacked).expect("drawn");
    let above = armor.drawn_at(compared).expect("drawn");
    assert!(below.0 == above.0 && below.1 > above.1, "below the active pane: {above:?} {below:?}");

    let pane = armor.window.by_attribute("data-panel", first.panel_id().as_str()).expect("the first pane is shown");
    armor.window.click(pane).expect("the pane can be pressed");
    armor.settle();
    assert_eq!(workspace.active_id(), first, "a press in a pane makes it active");
}

/// Each pane's tab closes it, by its close button or a middle click, which lets its viewport go;
/// closing the last leaves an empty pane in its place.
#[test]
fn a_pane_tab_closes_by_its_button_or_a_middle_click() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let first = workspace.active_id();
    let first_viewport = workspace.active_untracked();
    let scene = fixtures::model_scene(turret(), Vec::new());
    let name = scene.display_name.clone();
    first_viewport.show_armor(Arc::new(scene), workspace.opening_layer.get_untracked());
    workspace.compare(None);
    armor.settle();
    let compared_viewport = workspace.active_untracked();
    assert_eq!(workspace.panes().len(), 2);

    armor.close_rightmost(&name);
    assert_eq!(workspace.panes().len(), 1);
    assert_eq!(workspace.active_id(), first, "the pane left is the active one");
    assert!(compared_viewport.read_untracked(|_| ()).is_none(), "the closed pane's viewport is let go");

    let tab = armor.tab(&name);
    armor.window.middle_click(tab).expect("the tab takes a middle click");
    armor.settle();
    assert_eq!(workspace.panes().len(), 1, "an empty pane takes the last one's place");
    assert_ne!(workspace.active_id(), first);
    assert!(workspace.active_untracked().shown().is_none());
    assert!(first_viewport.read_untracked(|_| ()).is_none(), "the last pane's viewport is let go too");
    armor.tab(&rust_i18n::t!("ui.armor.empty"));
}

/// A ship opened in a new tab joins the active pane's group and becomes the active pane. Choosing
/// the other tab makes its pane the active one, which is where the next ship opens.
#[test]
fn a_new_tab_joins_the_active_group_and_a_chosen_tab_takes_the_next_ship() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    workspace.library.adopt_for_test(Arc::new(bundle()), None);
    let first = workspace.active_id();
    let first_viewport = workspace.active_untracked();
    let scene = fixtures::model_scene(turret(), Vec::new());
    let name = scene.display_name.clone();
    first_viewport.show_armor(Arc::new(scene), workspace.opening_layer.get_untracked());

    workspace.open_in_new_tab("PASB028".to_owned(), "Iowa".to_owned());
    armor.settle();
    let tabbed = workspace.active_id();
    let tabbed_viewport = workspace.active_untracked();
    assert_ne!(tabbed, first, "the new pane is the active one");
    assert_eq!(group_of(workspace, tabbed), group_of(workspace, first), "in the active pane's group");
    assert!(armor.drawn_at(tabbed).is_some() && armor.drawn_at(first).is_none(), "drawn in place of the first");
    // The store holds no ships, so the load fails in the new pane.
    armor.settle_until(|| tabbed_viewport.scene_problem().is_some());

    let tab = armor.tab(&name);
    armor.window.click(tab).expect("the tab can be chosen");
    armor.settle();
    assert_eq!(workspace.active_id(), first, "choosing its tab makes the pane active");
    assert!(armor.drawn_at(first).is_some());

    workspace.open_ship("PJSB018".to_owned(), "Yamato".to_owned());
    armor.settle_until(|| first_viewport.scene_problem().is_some());
    let subject = |viewport: crate::tabs::armor_viewer::viewport::ArmorViewportState| {
        viewport.scene_problem().map(|problem| problem.subject)
    };
    assert_eq!(subject(first_viewport), Some("Yamato".into()), "the ship opens in the chosen pane");
    assert_eq!(subject(tabbed_viewport), Some("Iowa".into()), "and not in the other");
}

/// A pane whose tab is dragged into a split of its own is shown there and becomes the active pane,
/// keeping its viewport.
#[test]
fn a_pane_dragged_into_a_new_split_becomes_active() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    workspace.library.adopt_for_test(Arc::new(bundle()), None);
    let first = workspace.active_id();
    let first_viewport = workspace.active_untracked();
    first_viewport
        .show_armor(Arc::new(fixtures::model_scene(turret(), Vec::new())), workspace.opening_layer.get_untracked());
    workspace.open_in_new_tab("PASB028".to_owned(), "Iowa".to_owned());
    armor.settle();
    let tabbed = workspace.active_id();
    let group = group_of(workspace, first).expect("the first pane is docked");

    let target = DockTarget::Beside { group, edge: Edge::Right };
    workspace.dock.move_panel(&first.panel_id(), target).expect("the pane moves");
    armor.settle();

    assert_eq!(workspace.active_id(), first, "the moved pane is the active one");
    assert_ne!(group_of(workspace, first), group_of(workspace, tabbed), "in a group of its own");
    assert!(armor.drawn_at(first).is_some() && armor.drawn_at(tabbed).is_some(), "both are drawn");
    assert!(first_viewport.read_untracked(|_| ()).is_some(), "the moved pane keeps its viewport");
}

/// A ship that cannot be read says so in its pane, and the model shown before is not passed off as
/// the ship. A new load takes the reason down.
#[test]
fn a_ship_that_fails_to_load_says_so_in_its_pane() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let viewport = workspace.active_untracked();
    viewport.show_armor(Arc::new(fixtures::model_scene(turret(), Vec::new())), workspace.opening_layer.get_untracked());
    let opening = viewport.begin_load("Iowa".to_string());

    workspace.land_ship_for_test(opening.generation, &bundle(), Err(ShipLoadError::NoVehicle("PASB028".to_string())));

    assert!(viewport.shown().is_none(), "the model shown before is not passed off as the ship");
    assert!(viewport.open_target().is_none(), "the sidebar marks nothing");
    let expected = SceneProblem { subject: "Iowa".into(), reason: ProblemReason::ShipUnreadable };
    assert_eq!(viewport.scene_problem(), Some(expected));

    viewport.begin_load("Yamato".to_string());
    assert_eq!(viewport.scene_problem(), None, "a new load takes the reason down");
}

#[test]
fn a_ship_with_nothing_to_draw_says_so_in_its_pane() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let viewport = workspace.active_untracked();
    let opening = viewport.begin_load("Iowa".to_string());
    let path = ModelPath::parse("content/gameplay/usa/misc/AM001/AM001.model").expect("a model path");
    let scene = fixtures::hull_scene(path, Vec::new(), 1);

    workspace.land_ship_for_test(opening.generation, &bundle(), Ok(FirstStage { scene, rest: Vec::new() }));

    let reason = ProblemReason::Empty(EmptyScene::Meshless);
    assert_eq!(viewport.scene_problem(), Some(SceneProblem { subject: "Iowa".into(), reason }));
}

/// A second selection in a pane stops the first load, whose scene does not replace what is shown
/// when it lands anyway.
#[test]
fn a_superseded_load_is_cancelled_and_never_applied() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let viewport = workspace.active_untracked();
    viewport.show_armor(Arc::new(fixtures::model_scene(turret(), Vec::new())), workspace.opening_layer.get_untracked());
    let first = viewport.begin_load("Iowa".to_string());
    let second = viewport.begin_load("Yamato".to_string());
    assert!(first.cancel.is_cancelled(), "the first load stops");
    assert!(!second.cancel.is_cancelled(), "the second runs on");

    workspace.land_ship_for_test(first.generation, &bundle(), Ok(fixtures::model_first_stage(turret(), Vec::new())));

    assert_eq!(viewport.shown_ship_name(), None, "the superseded load's scene is not shown");
}

/// Clicking the model on screen while another loads keeps it on screen, and puts away replay hits
/// held for the cancelled load.
#[test]
fn a_click_on_the_shown_model_cancels_a_load_in_flight() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let viewport = workspace.active_untracked();
    viewport.show_armor(Arc::new(fixtures::model_scene(turret(), Vec::new())), workspace.opening_layer.get_untracked());
    let other = viewport.begin_load("AGM058".to_string());
    workspace.hold_hits_for_test(workspace.library.new_source(), "PASC001", other.generation);

    workspace.open_model(turret(), None);

    assert!(!workspace.holds_hits_for_test(), "the cancelled load's replay hits are put away");
    assert!(!viewport.is_current_load(other.generation), "the other load no longer lands");
    assert!(other.cancel.is_cancelled(), "and stops");
    assert!(!viewport.cancel_load(), "nothing is loading");
}

/// A replay viewport whose ship the viewer is not showing moves its playback. Its hits are another
/// ship's and must not replace the ones on screen, nor stop the shown replay's moves arriving.
#[test]
fn only_the_replay_the_viewer_shows_moves_its_hits() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let library = workspace.library;
    library.adopt_for_test(Arc::new(bundle()), None);
    let viewport = workspace.active_untracked();
    let (shown, other) = (library.new_source(), library.new_source());
    let opening = viewport.begin_load("Iowa".to_string());
    workspace.hold_hits_for_test(shown, "PASB028", opening.generation);
    workspace.land_ship_for_test(
        opening.generation,
        &bundle(),
        Ok(FirstStage { scene: ship_scene(), rest: Vec::new() }),
    );

    library.send(ViewerFeed::Followed { source: shown, followed: followed(&[1.0], 90.0) });
    armor.settle();
    assert_eq!(viewport.hits_drawn(), 1, "the shown replay's playback moves its hits");

    library.send(ViewerFeed::Followed { source: other, followed: followed(&[1.0, 2.0, 3.0], 40.0) });
    armor.settle();
    assert_eq!(viewport.hits_drawn(), 1, "another replay's playback leaves them");

    library.send(ViewerFeed::Followed { source: shown, followed: followed(&[1.0, 2.0], 80.0) });
    library.send(ViewerFeed::Followed { source: other, followed: followed(&[1.0, 2.0, 3.0, 4.0], 30.0) });
    armor.settle();
    assert_eq!(viewport.hits_drawn(), 2, "another replay's move does not take the place of the shown one's");
}

/// Playback moving while the ship it asked for loads is not lost: the ship lands showing where
/// playback has reached, not where it was when the viewer was asked.
#[test]
fn playback_moving_during_a_load_is_shown_when_the_ship_lands() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let source = workspace.library.new_source();
    let viewport = workspace.active_untracked();
    let opening = viewport.begin_load("Iowa".to_string());
    workspace.hold_hits_for_test(source, "PASB028", opening.generation);

    workspace.follow_hits(source, followed(&[1.0], 90.0));
    workspace.follow_hits(source, followed(&[1.0, 2.0, 3.0], 60.0));
    assert_eq!(viewport.hits_drawn(), 0, "nothing is drawn before the ship lands");

    workspace.land_ship_for_test(
        opening.generation,
        &bundle(),
        Ok(FirstStage { scene: ship_scene(), rest: Vec::new() }),
    );

    assert_eq!(viewport.hits_drawn(), 3, "the latest hits are shown");
    let readout = viewport.hit_readout().expect("the ship's health is shown");
    assert!(readout.contains("60"), "with the latest health: {readout}");
}

/// A replay's playback moves the hits of the pane it was shown in, whichever pane is active, and
/// none once that pane is closed.
#[test]
fn playback_moves_the_hits_of_the_pane_the_replay_was_shown_in() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let source = workspace.library.new_source();
    let shown_in = workspace.active_untracked();
    let first = workspace.active_id();
    let opening = shown_in.begin_load("Iowa".to_string());
    workspace.hold_hits_for_test(source, "PASB028", opening.generation);
    workspace.land_ship_for_test(
        opening.generation,
        &bundle(),
        Ok(FirstStage { scene: ship_scene(), rest: Vec::new() }),
    );
    workspace.compare(None);
    let compared = workspace.active_untracked();
    assert_ne!(workspace.active_id(), first, "the compare pane is the active one");

    workspace.follow_hits(source, followed(&[1.0, 2.0], 80.0));
    assert_eq!(shown_in.hits_drawn(), 2, "the pane the replay was shown in follows it");
    assert_eq!(compared.hits_drawn(), 0, "the active pane is left alone");

    armor.close_tab(first);
    workspace.follow_hits(source, followed(&[1.0, 2.0, 3.0], 70.0));
    assert_eq!(compared.hits_drawn(), 0, "with its pane closed, the replay moves nothing");
}

/// The saved setting read at startup does not undo an opening layer the reader picked while it
/// was being read, and the pick is saved in its place.
#[test]
fn a_picked_opening_layer_outlasts_the_saved_one_read_after() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let mut common = workspace.common();
    common.opening_layer = OpeningLayer::Mesh;
    workspace.set_common(common);

    workspace.apply_defaults(None, true, XrayHighlight::Always);

    assert_eq!(workspace.opening_layer.get_untracked(), OpeningLayer::Mesh);
    let saved = workspace.services.settings.read_untracked(|settings| settings.model_show_armor);
    assert_eq!(saved, Some(false), "the pick is what is saved");
}

/// The saved x-ray mode is adopted, a mode picked while it was being read outlasts it, and the
/// pick is saved in its place.
#[test]
fn the_xray_mode_is_restored_and_a_pick_outlasts_the_saved_one() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    workspace.apply_defaults(None, true, XrayHighlight::Off);
    assert_eq!(workspace.xray_highlight.get_untracked(), XrayHighlight::Off, "restored from the settings");

    let mut common = workspace.common();
    common.xray_highlight = XrayHighlight::WhileHeld;
    workspace.set_common(common);
    workspace.apply_defaults(None, true, XrayHighlight::Always);

    assert_eq!(workspace.xray_highlight.get_untracked(), XrayHighlight::WhileHeld);
    let saved = workspace.services.settings.read_untracked(|settings| settings.model_xray_highlight);
    assert_eq!(saved, Some(XrayHighlight::WhileHeld), "the pick is what is saved");
}

/// Choosing a mode in the common settings menu saves it.
#[test]
fn choosing_an_xray_mode_in_the_menu_saves_it() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    armor.click("armor-sidebar-common");
    armor.click("armor-pane-xray-highlight-off");

    assert_eq!(armor.workspace.xray_highlight.get_untracked(), XrayHighlight::Off);
    let saved = armor.workspace.services.settings.read_untracked(|settings| settings.model_xray_highlight);
    assert_eq!(saved, Some(XrayHighlight::Off));
}

/// Each mode chooses whether a hover in the view shows through what is in front of it, and in
/// the while-held mode pressing and letting go of the key flips it with no pointer move between.
#[test]
fn the_xray_mode_and_the_held_key_choose_the_highlight_depth() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let cases = [
        (XrayHighlight::Always, HighlightDepth::XRay, HighlightDepth::XRay),
        (XrayHighlight::WhileHeld, HighlightDepth::Tested, HighlightDepth::XRay),
        (XrayHighlight::Off, HighlightDepth::Tested, HighlightDepth::Tested),
    ];
    for (mode, released, held) in cases {
        let mut common = workspace.common();
        common.xray_highlight = mode;
        workspace.set_common(common);
        for (key_held, expected) in [(false, released), (true, held), (false, released)] {
            let modifiers = if key_held { Modifiers::PRIMARY } else { Modifiers::NONE };
            armor.window.press_key(NamedKey::Control, modifiers);
            armor.settle();

            let depth = workspace.active_untracked().highlight_depth();
            assert_eq!(depth, expected, "{mode:?}, key held: {key_held}");
        }
    }
}

/// Closing a pane stops the load running into it.
#[test]
fn closing_a_pane_cancels_its_load() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    workspace.compare(None);
    armor.settle();
    let closing = workspace.active_id();
    let opening = workspace.active_untracked().begin_load("Iowa".to_string());

    armor.close_tab(closing);

    assert!(opening.cancel.is_cancelled());
}

/// Ticking a model's hardpoints in the sidebar marks them in the pane showing it, and unticking
/// takes the markers away again.
#[test]
fn a_hardpoint_toggle_marks_and_unmarks_the_open_model() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let workspace = armor.workspace;
    let transform = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 1.0];
    let hardpoint = Hardpoint { name: HardpointName::new("HP_Muzzle"), kind: HardpointKind::Mount, transform };
    let viewport = workspace.active_untracked();
    viewport.show_armor(
        Arc::new(fixtures::model_scene(turret(), vec![hardpoint])),
        workspace.opening_layer.get_untracked(),
    );
    assert_eq!(viewport.hardpoint_labels().len(), 0, "nothing is marked yet");

    workspace.set_hardpoints_visible(HardpointTarget::Model(turret()), true);
    assert_eq!(viewport.hardpoint_labels().len(), 1, "the model's one hardpoint is marked");

    workspace.set_hardpoints_visible(HardpointTarget::Model(turret()), false);
    assert_eq!(viewport.hardpoint_labels().len(), 0, "and unmarked again");
}

#[test]
fn a_model_without_armor_keeps_only_the_mesh_and_display_sections() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    armor.install(fixtures::model_scene(turret(), Vec::new()), &[OptionSection::Display]);

    for gone in ["Trajectory", "PenCheck", "Visibility"] {
        assert!(!armor.shown(&format!("armor-options-section-{gone}")), "{gone} has nothing to act on");
    }
    for kept in ["Hull", "Display"] {
        assert!(armor.shown(&format!("armor-options-section-{kept}")), "{kept} stays");
    }
    for gone in
        ["armor-show-hidden-button", "armor-show-gaps-button", "armor-splash-button", "armor-splash-boxes-button"]
    {
        assert!(!armor.shown(gone), "{gone} is for armor or a ship");
    }
    assert!(armor.shown("armor-export-trigger"), "a model still exports");
    assert!(!armor.shown("armor-cast-hint"), "there is nothing to cast at");
}

#[test]
fn a_model_with_armor_keeps_the_armor_sections_but_not_the_ship_tools() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    armor.install(fixtures::armored_model_scene(turret()), &[]);

    for kept in ["Trajectory", "PenCheck", "Visibility"] {
        assert!(armor.shown(&format!("armor-options-section-{kept}")), "{kept} reads the model's plates");
    }
    assert!(armor.shown("armor-show-hidden-button"), "plate tools stay");
    assert!(!armor.shown("armor-splash-button"), "splash boxes are a ship's");
}

/// In the Hull section the part tree comes before the camouflage picker, and the camouflage
/// picker before the LOD row.
#[test]
fn the_hull_part_tree_comes_before_the_camo_and_lod_pickers() {
    let mut armor = ArmorWindow::open_tall();
    let mut scene = fixtures::hull_scene(turret(), vec![fixtures::hull_mesh()], 2);
    scene.camo_scheme_infos = vec![camo(0, "Default", CamoOrigin::ShipSpecific)].into();
    armor.install(scene, &[OptionSection::Hull]);

    let bounds = |id: &str| {
        let element = armor.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        armor.window.bounds(element).expect("it is laid out")
    };
    let (tree, camo, lod) =
        (bounds("armor-hull-visibility-tree"), bounds("armor-camo-picker"), bounds("armor-hull-lod-0"));
    assert!(tree.bottom() <= camo.top(), "the tree at {tree:?} is above the camo picker at {camo:?}");
    assert!(camo.bottom() <= lod.top(), "the camo picker at {camo:?} is above the LOD row at {lod:?}");
}

/// Each section head is a band across its section's whole width, and a group head across the
/// width of the section it sits in.
#[test]
fn each_folding_head_spans_its_column() {
    let mut armor = ArmorWindow::open_tall();
    let scene = fixtures::hull_scene(turret(), vec![fixtures::hull_mesh()], 1);
    armor.install(scene, &[OptionSection::Hull, OptionSection::Display, OptionSection::Lighting]);

    let bounds = |id: &str| {
        let element = armor.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        armor.window.bounds(element).expect("it is laid out")
    };
    for which in ["Hull", "Display"] {
        let head = bounds(&format!("armor-options-section-{which}"));
        let body = bounds(&format!("armor-options-body-{which}"));
        assert!(
            head.left() <= body.left() && head.right() >= body.right(),
            "{which}'s head at {head:?} spans {body:?}"
        );
    }
    let display = bounds("armor-options-body-Display");
    let group = bounds("armor-options-group-Lighting");
    assert!(group.left() >= display.left(), "the lighting head at {group:?} is inside {display:?}");
    assert_eq!(group.right(), display.right(), "the lighting head reaches the section's edge");
}

/// The zone tree's field keeps the parts it matches, each under its zone opened to show it, counts
/// them, and says when nothing matched; clearing it brings back the zones open as they were left.
#[test]
fn the_zone_filter_shows_matches_under_their_zones_and_clearing_restores_the_tree() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(two_zone_scene(), &[OptionSection::Visibility]);
    armor.workspace.active_untracked().toggle_zone_expanded("Stern".to_string());
    armor.settle();
    let (bow, stern) = ("armor-vis-zone-row-Bow", "armor-vis-zone-row-Stern");
    let belt = "armor-vis-part-row-Bow-Bow_Belt";
    let bow_deck = "armor-vis-part-row-Bow-Bow_Deck";
    let stern_deck = "armor-vis-part-row-Stern-Stern_Deck";
    assert!(!armor.shown(bow_deck), "Bow starts collapsed");
    assert!(armor.shown(stern_deck), "Stern was opened");

    armor.type_into("armor-zone-filter", "deck");
    assert!(armor.shown(bow_deck), "a match in a collapsed zone is shown");
    assert!(armor.shown(stern_deck));
    assert!(!armor.shown(belt), "the belt does not match");
    assert!(armor.shown(bow) && armor.shown(stern), "each match keeps its zone");
    assert!(armor.marked().iter().any(|marked| marked.eq_ignore_ascii_case("deck")), "the match is marked");
    assert!(armor.shown("armor-zone-filter-count"));

    armor.window.type_text("zzz");
    armor.settle();
    assert!(!armor.shown(bow) && !armor.shown(stern), "nothing is left");

    armor.erase("deckzzz");
    assert!(!armor.shown("armor-zone-filter-count"), "no count without a query");
    assert!(!armor.shown(bow_deck), "Bow is collapsed again");
    assert!(armor.shown(stern_deck), "Stern is still open");
}

/// The part tree's field keeps the meshes it matches under their group, drops groups with none,
/// and clearing it closes the groups again.
#[test]
fn the_part_filter_shows_matches_under_their_groups_and_clearing_restores_the_tree() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(two_zone_scene(), &[OptionSection::Hull]);
    let (hull, battery) = ("armor-hull-vis-group-row-Hull", "armor-hull-vis-group-row-Main Battery");
    let (fore, aft) = ("armor-hull-vis-mesh-row-Hull_Fore", "armor-hull-vis-mesh-row-Hull_Aft");
    assert!(!armor.shown(aft), "groups start collapsed");

    armor.type_into("armor-part-filter", "aft");
    assert!(armor.shown(hull) && armor.shown(aft), "the match under its group");
    assert!(!armor.shown(fore) && !armor.shown(battery), "the rest is dropped");

    armor.erase("aft");
    assert!(!armor.shown(aft), "the group is collapsed again");
    assert!(armor.shown(battery), "every group is back");
}

/// The rail's own filter narrows a tree together with the tree's field.
#[test]
fn the_rail_filter_and_a_tree_field_narrow_together() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(two_zone_scene(), &[OptionSection::Visibility]);

    armor.type_into("armor-options-filter", "stern");
    armor.type_into("armor-zone-filter", "deck");

    assert!(armor.shown("armor-vis-part-row-Stern-Stern_Deck"));
    assert!(!armor.shown("armor-vis-zone-row-Bow"), "Bow does not match the rail's filter");
}

/// The rail's filter finds a camouflage, a zone and a slider, each in a collapsed section it opens
/// to show it, and Escape clears the filter and puts every section back as it was.
#[test]
fn the_rail_filter_opens_the_section_holding_a_match_and_escape_restores_the_rail() {
    let mut armor = ArmorWindow::open_tall();
    let mut scene = two_zone_scene();
    scene.camo_scheme_infos =
        vec![camo(0, "Default", CamoOrigin::ShipSpecific), camo(1, "Ocean Wave", CamoOrigin::Universal)].into();
    armor.install(scene, &[]);
    let bodies = ["Visibility", "Hull", "Display"].map(|which| format!("armor-options-body-{which}"));
    assert!(bodies.iter().all(|body| !armor.shown(body)), "every section starts closed");

    let find = |armor: &mut ArmorWindow, query: &str, check: &dyn Fn(&ArmorWindow)| {
        armor.type_into("armor-options-filter", query);
        check(armor);
        armor.window.press_key(NamedKey::Escape, Modifiers::NONE);
        armor.settle();
        assert!(bodies.iter().all(|body| !armor.shown(body)), "Escape after {query:?} puts the sections back closed");
    };
    find(&mut armor, "ocean wave", &|armor| {
        assert!(armor.shown("armor-options-body-Hull"), "the hull section opens");
        assert!(!armor.marked().is_empty(), "the camouflage is drawn marked");
    });
    find(&mut armor, "stern", &|armor| {
        assert!(armor.shown("armor-options-body-Visibility"), "the armor section opens");
        assert!(armor.shown("armor-vis-zone-row-Stern"));
        assert!(!armor.shown("armor-vis-zone-row-Bow"));
    });
    let roll = rust_i18n::t!("ui.armor.roll").into_owned();
    find(&mut armor, &roll, &|armor| {
        assert!(armor.shown("armor-options-body-Display"), "the display section opens");
    });
}

/// A filter that matches nothing in the rail says so.
#[test]
fn a_rail_filter_matching_nothing_says_so() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    armor.type_into("armor-options-filter", "qqqqzzzz");
    assert!(armor.shown("armor-options-no-match"));
}

/// The clear button beside the rail's filter shows only while something is typed, and empties it.
#[test]
fn the_rail_filter_clear_button_empties_the_filter() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    assert!(!armor.shown("armor-options-filter-clear"), "nothing to clear");
    armor.type_into("armor-options-filter", "qqqqzzzz");
    armor.click("armor-options-filter-clear");

    assert!(armor.workspace.options_filter.get_untracked().is_empty());
    assert!(!armor.shown("armor-options-no-match"), "the rail is back");
    assert!(!armor.shown("armor-options-filter-clear"));
}

/// A zone's box clicked with Ctrl/Cmd held shows that zone alone, every time it is clicked so; a
/// plain click only turns the zone on or off. The key is read off the click itself, as when it was
/// pressed while another window had focus.
#[test]
fn a_primary_click_on_a_zone_box_solos_the_zone() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(two_zone_scene(), &[OptionSection::Visibility]);
    let viewport = armor.workspace.active_untracked();
    viewport.adopt_for_test(Arc::new(two_zone_scene()));
    let shown = |zone: &str, part: &str| {
        viewport
            .read_untracked(|model| model.part_visibility.get(&(zone.to_owned(), part.to_owned())).copied())
            .flatten()
            .unwrap_or(true)
    };
    let click = |armor: &mut ArmorWindow, zone: &str, modifiers: Modifiers| {
        let id = format!("armor-vis-zone-{zone}");
        let element = armor.window.by_test_id(&id).unwrap_or_else(|| panic!("{id} is shown"));
        armor.window.click_with(element, modifiers).expect("the box can be pressed");
        armor.settle();
    };

    click(&mut armor, "Bow", Modifiers::PRIMARY);
    assert!(shown("Bow", "Bow_Belt") && shown("Bow", "Bow_Deck"), "the zone is on");
    assert!(!shown("Stern", "Stern_Deck"), "every other zone is off");

    click(&mut armor, "Bow", Modifiers::PRIMARY);
    assert!(shown("Bow", "Bow_Belt") && !shown("Stern", "Stern_Deck"), "a second solo click solos again");

    click(&mut armor, "Stern", Modifiers::NONE);
    assert!(shown("Stern", "Stern_Deck") && shown("Bow", "Bow_Belt"), "a plain click only turns its zone on");

    click(&mut armor, "Stern", Modifiers::PRIMARY);
    assert!(shown("Stern", "Stern_Deck") && !shown("Bow", "Bow_Belt"), "solo moves to the clicked zone");
}

/// A row under a switch that is off is no match for the rail's filter, so the group holding it is
/// dropped; turning the switch on while the filter stands finds it.
#[test]
fn a_rail_filter_finds_a_row_under_a_switch_only_while_it_is_on() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(ship_scene(), &[]);
    armor.type_into("armor-options-filter", "max fov");
    assert!(!armor.shown("armor-options-group-CameraRings"), "the zoom path is off, so nothing in the group matched");

    let viewport = armor.workspace.active_untracked();
    let mut rings = viewport.camera_rings();
    rings.zoom_path = true;
    viewport.set_camera_rings(rings);
    armor.settle();
    assert!(armor.shown("armor-options-group-CameraRings"), "with the zoom path on its choice matches");
    assert!(armor.shown("armor-camera-zoom-path-max"));
}

/// The cast hint wraps inside the rail at twice the text size, and each tool label stays inside
/// its button.
#[test]
fn the_cast_hint_and_tool_labels_stay_inside_the_rail() {
    let zoom = zgui::prelude::Zoom::new(2.0).expect("twice the size is a zoom");
    let options = HeadlessOptions { zoom, ..HeadlessOptions::new(REVIEW_SIZE) };
    let armor = ArmorWindow::open(options);
    let bounds = |id: &str| {
        let element = armor.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        armor.window.bounds(element).expect("it is laid out")
    };
    let rail = bounds("armor-options-panel");
    let inside = |inner: zgui::geom::Rect<zgui::geom::DevicePx, zgui::geom::Device>| {
        inner.left() >= rail.left() && inner.right() <= rail.right()
    };
    let hint = bounds("armor-cast-hint-text");
    assert!(inside(hint), "the hint at {hint:?} is inside the rail at {rail:?}");
    for tool in ["armor-show-hidden-button", "armor-show-gaps-button", "armor-splash-button", "armor-export-trigger"] {
        assert!(inside(bounds(tool)), "{tool} is inside the rail");
    }
}

/// The tool buttons fill an even two-column grid, and the firing ship button spans the rail.
#[test]
fn the_rail_tools_fill_two_even_columns() {
    let armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let bounds = |id: &str| {
        let element = armor.window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        armor.window.bounds(element).expect("it is laid out")
    };
    let grid = bounds("armor-options-tools");
    let [hidden, gaps, splash, boxes, export] = [
        "armor-show-hidden-button",
        "armor-show-gaps-button",
        "armor-splash-button",
        "armor-splash-boxes-button",
        "armor-export-trigger",
    ]
    .map(bounds);

    for button in [hidden, gaps, splash, boxes, export] {
        assert!((button.size.width.0 - hidden.size.width.0).abs() < 1.0, "{button:?} is as wide as {hidden:?}");
        assert!(button.size.width.0 > grid.size.width.0 * 0.45, "{button:?} fills half of {grid:?}");
    }
    for (left, right) in [(hidden, gaps), (splash, boxes)] {
        assert!((left.origin.x.0 - grid.origin.x.0).abs() < 1.0, "{left:?} starts the row");
        assert!((right.right().0 - grid.right().0).abs() < 1.0, "{right:?} ends the row");
    }

    let group = bounds("armor-trajectory");
    let add_ship = bounds("armor-trajectory-add-ship");
    assert!((add_ship.size.width.0 - group.size.width.0).abs() < 1.0, "{add_ship:?} spans {group:?}");
}

/// The tab with two real ships compared side by side, then a third opened as a tab beside the
/// second, drawn in both appearances: the ships list, the panes, the options rail and the legend.
/// Needs a game install named by `WT_WOWS_DIR`.
#[test]
#[ignore = "needs a game install in WT_WOWS_DIR and draws on the graphics device"]
fn the_tab_renders_a_real_ship_in_dark_and_light() {
    let wows_dir = std::env::var("WT_WOWS_DIR").expect("set WT_WOWS_DIR to a WoWs install directory");
    for (appearance, name) in [(Appearance::Dark, "armor-viewer-dark"), (Appearance::Light, "armor-viewer-light")] {
        let mut armor = ArmorWindow::open(review_options(appearance));
        let workspace = armor.workspace;
        workspace.services.game_data.adopt(crate::services::GameDataRequest {
            wows_dir: Some(std::path::PathBuf::from(&wows_dir)),
            cache_dir: String::new(),
            locale: None,
            auto_dump: false,
        });
        let library = workspace.library;
        for _ in 0..600 {
            settle_in_real_time(&mut armor.window);
            if library.ready_untracked().is_some() {
                break;
            }
        }
        let bundle = library.ready_untracked().expect("the ship data loads");
        let ship_named = |name: &str| {
            let ship = bundle
                .catalog
                .ships()
                .find(|ship| ship.display_name == name)
                .unwrap_or_else(|| panic!("{name} is in the catalogue"));
            (ship.param_index.clone(), ship.display_name.clone())
        };
        let (param_index, display_name) = ship_named("Yamato");
        workspace.open_ship(param_index, display_name);
        wait_until_shown(&mut armor, "Yamato");
        workspace.compare(Some(ship_named("Montana")));
        wait_until_shown(&mut armor, "Montana");
        assert_eq!(workspace.panes().len(), 2, "the two ships are compared");
        settle_in_real_time(&mut armor.window);
        assert!(armor.shown("armor-sidebar-tree"), "the ships list is loaded");
        let path = save_shot(&mut armor.window, name);
        println!("{}", path.display());

        let (param_index, display_name) = ship_named("Iowa");
        workspace.open_in_new_tab(param_index, display_name);
        wait_until_shown(&mut armor, "Iowa");
        settle_in_real_time(&mut armor.window);
        let path = save_shot(&mut armor.window, &format!("{name}-tabs"));
        println!("{}", path.display());
    }
}

/// Lets the active pane's load run until it shows the ship called `name`.
fn wait_until_shown(armor: &mut ArmorWindow, name: &str) {
    let shown = |armor: &ArmorWindow| armor.workspace.active_untracked().shown_ship_name().as_deref() == Some(name);
    for _ in 0..300 {
        settle_in_real_time(&mut armor.window);
        if shown(armor) {
            break;
        }
    }
    assert!(shown(armor), "{name} is shown");
}

const HULL_ALBEDO: &str = "content/gameplay/usa/gun/main/textures/AGM034_a.dds";
const HULL_MATERIAL: &str = "content/gameplay/usa/gun/main/textures/AGM034.mfm";

/// The turret mesh drawn with one material, which references an albedo stored in two files and a
/// path the path store lacks.
fn textured_scene() -> LoadedShipArmor {
    scene_with_material(HULL_MATERIAL)
}

/// [`textured_scene`] with its material read from `material`.
fn scene_with_material(material: &str) -> LoadedShipArmor {
    let mut mesh = fixtures::hull_mesh();
    mesh.mfm_path = Some(material.to_string());
    let scene = fixtures::hull_scene(turret(), vec![mesh], 1);
    let file = |path: &str, edge: u32, bytes: u64| TextureFile {
        path: path.to_string(),
        size: TextureSize { width: edge, height: edge },
        bytes,
    };
    let albedo = LocatedTexture {
        path: HULL_ALBEDO.to_string(),
        files: Ok(TextureFiles {
            format: None,
            mip_count: 10,
            files: vec![
                file("content/gameplay/usa/gun/main/textures/AGM034_a.dd0", 1024, 699_192),
                file(HULL_ALBEDO, 512, 174_904),
            ],
        }),
    };
    let material = SceneMaterial {
        mfm_path: Some(material.to_string()),
        meshes: vec![HullMeshIndex::new(0)],
        mesh_names: vec!["Turret".to_string()],
        properties: Some(vec![
            PropertyTexture { property: PropertyName::Known("diffuseMap"), texture: TextureLookup::Found(albedo) },
            PropertyTexture { property: PropertyName::Known("normalMap"), texture: TextureLookup::Unresolved(7) },
        ]),
        name_matched_albedo: None,
    };
    let inventory = SceneTextures::from_materials(vec![material]);
    assert!(scene.texture_inventory.set(inventory).is_ok(), "a fresh scene has no inventory");

    scene
}

impl ArmorWindow {
    /// The labels of the menu `test_id` opens on a secondary click.
    fn menu_of(&mut self, test_id: &str) -> Vec<String> {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.right_click(element).expect("the row takes a secondary click");
        self.settle();
        let items = self.window.by_role(Role::MenuItem);

        items.into_iter().filter_map(|item| self.window.a11y(item).and_then(|state| state.label)).collect()
    }

    fn toasts(&self) -> String {
        self.window.texts_by_class("lui-toasts").concat()
    }
}

#[test]
fn the_textures_section_lists_each_material_with_its_textures() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[OptionSection::Textures]);

    let summary = armor.window.texts_by_class("wt-armor-textures__dim").concat();
    assert!(summary.contains("1 materials, 1 textures"), "the summary counts what was read: {summary:?}");
    assert!(!armor.shown("armor-textures-texture-0-0"), "materials start closed");

    armor.click("armor-textures-expand-0");
    assert!(armor.shown("armor-textures-texture-0-0"), "the albedo is listed");
    assert!(armor.shown("armor-textures-texture-0-1"), "the unresolved normal map is listed");
    let roles = armor.window.texts_by_class("wt-armor-textures__role");
    assert_eq!(roles, ["Albedo (diffuseMap)", "Normal (normalMap)"]);
    let details = armor.window.texts_by_class("wt-armor-textures__dim").concat();
    assert!(details.contains("Unrecognized format, 1024x1024, 2 files, 853.6 KiB"), "{details:?}");
    assert!(details.contains("The path is not in assets.bin"), "{details:?}");
}

/// A material name wider than the rail is cut to one line, so its row is as tall as its count.
#[test]
fn a_long_material_name_keeps_its_row_one_line_tall() {
    let mut armor = ArmorWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    armor.install(
        scene_with_material("content/gameplay/france/gun/main/textures/FGM055_305mm_45_Tourelle_skinned_extended.mfm"),
        &[OptionSection::Textures, OptionSection::Display],
    );
    armor.click("armor-textures-expand-0");
    let bounds = |class: &str| {
        let element = armor.window.by_class(class).unwrap_or_else(|| panic!("{class} is shown"));
        armor.window.bounds(element).expect("it is laid out")
    };
    let (head, count) = (bounds("wt-armor-textures__head"), bounds("wt-armor-textures__count"));
    assert!(
        head.size.height.0 < 2.0 * count.size.height.0,
        "the row at {head:?} is one line, as its count at {count:?} is"
    );
}

#[test]
fn a_texture_row_offers_the_override_actions() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[OptionSection::Textures]);
    armor.click("armor-textures-expand-0");

    assert_eq!(
        armor.menu_of("armor-textures-texture-0-0"),
        ["Copy Path", "Copy 2 res_mods Paths", "Show in Resource Unpacker", "Extract Files..."]
    );
    armor.click("armor-textures-copy-path");
    assert!(armor.toasts().contains(HULL_ALBEDO), "the message names the path: {:?}", armor.toasts());
}

#[test]
fn an_unresolved_texture_offers_only_its_path_id() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[OptionSection::Textures]);
    armor.click("armor-textures-expand-0");

    assert!(armor.menu_of("armor-textures-texture-0-1").is_empty(), "nothing is in the packages to act on");
}

#[test]
fn copying_override_paths_without_game_data_says_why() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[OptionSection::Textures]);
    armor.click("armor-textures-expand-0");

    armor.menu_of("armor-textures-texture-0-0");
    armor.click("armor-textures-copy-override");

    assert!(!armor.toasts().is_empty(), "a message says the mod folder is not known");
    assert!(!armor.toasts().contains(HULL_ALBEDO), "no path was copied: {:?}", armor.toasts());
}

#[test]
fn extracting_texture_files_asks_for_a_folder() {
    let mut armor = ArmorWindow::open_tall();
    armor.workspace.library.adopt_for_test(Arc::new(bundle()), None);
    armor.install(textured_scene(), &[OptionSection::Textures]);
    armor.click("armor-textures-expand-0");
    wows_toolkit_services::dialog::take_asked();

    armor.menu_of("armor-textures-texture-0-0");
    armor.click("armor-textures-extract");

    assert_eq!(wows_toolkit_services::dialog::take_asked(), vec![Asked::Folder]);
}

#[test]
fn showing_a_texture_in_the_unpacker_switches_to_it() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[OptionSection::Textures]);
    armor.click("armor-textures-expand-0");

    armor.menu_of("armor-textures-texture-0-0");
    armor.click("armor-textures-show-in-unpacker");
    settle_in_real_time(&mut armor.window);

    assert!(armor.shown("unpacker-tab"), "the Resource Unpacker is shown");
}

#[test]
fn the_viewport_menu_opens_the_section_at_the_material_of_a_mesh() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[]);
    let viewport = armor.workspace.active_untracked();
    let scene = viewport.scene_key().expect("a scene is shown");

    armor.workspace.show_textures(TextureFocus { viewport, scene, mesh: HullMeshIndex::new(0) });
    armor.settle();

    assert!(armor.shown("armor-options-body-Textures"), "the section is opened");
    assert!(armor.shown("armor-textures-texture-0-0"), "the mesh's material is opened");
}

#[test]
fn the_rail_filter_finds_a_texture_by_its_file_name() {
    let mut armor = ArmorWindow::open_tall();
    armor.install(textured_scene(), &[]);

    armor.type_into("armor-options-filter", "agm034_a");

    assert!(armor.shown("armor-textures-texture-0-0"), "the albedo matches");
    assert!(!armor.shown("armor-textures-texture-0-1"), "the normal map does not");
    assert!(armor.marked().iter().any(|mark| mark.eq_ignore_ascii_case("agm034_a")), "{:?}", armor.marked());
}

/// A real ship's textures, read through the model library, with the material under a mesh opened
/// and drawn for review. Needs a game install named by `WT_WOWS_DIR`.
#[test]
#[ignore = "needs a game install in WT_WOWS_DIR and draws on the graphics device"]
fn a_real_ship_lists_its_textures() {
    let wows_dir = std::env::var("WT_WOWS_DIR").expect("set WT_WOWS_DIR to a WoWs install directory");
    let mut armor = ArmorWindow::open(review_options(Appearance::Dark));
    let workspace = armor.workspace;
    workspace.services.game_data.adopt(crate::services::GameDataRequest {
        wows_dir: Some(std::path::PathBuf::from(&wows_dir)),
        cache_dir: String::new(),
        locale: None,
        auto_dump: false,
    });
    let library = workspace.library;
    for _ in 0..600 {
        settle_in_real_time(&mut armor.window);
        if library.ready_untracked().is_some() {
            break;
        }
    }
    let bundle = library.ready_untracked().expect("the ship data loads");
    let ship = bundle.catalog.ships().find(|ship| ship.display_name == "Yamato").expect("Yamato is in the catalogue");
    workspace.open_ship(ship.param_index.clone(), ship.display_name.clone());
    wait_until_shown(&mut armor, "Yamato");
    // A filter typed before the section is opened reads the textures to match against.
    workspace.options_filter.set("_a.dds".to_string());
    for _ in 0..600 {
        settle_in_real_time(&mut armor.window);
        if armor.shown("armor-textures-summary") {
            break;
        }
    }
    assert!(armor.shown("armor-textures-summary"), "the filter finds the albedos");
    workspace.options_filter.set(String::new());
    workspace.open_section(OptionSection::Textures);
    settle_in_real_time(&mut armor.window);
    assert!(armor.shown("armor-textures-summary"), "the textures are read");

    let viewport = workspace.active_untracked();
    let scene = viewport.scene_key().expect("a scene is shown");
    workspace.show_textures(TextureFocus { viewport, scene, mesh: HullMeshIndex::new(0) });
    settle_in_real_time(&mut armor.window);
    let details = armor.window.texts_by_class("wt-armor-textures__dim").concat();
    assert!(details.contains("BC"), "a block-compressed format is named: {details:?}");

    let path = save_shot(&mut armor.window, "armor-textures-real");
    println!("{}", path.display());
}

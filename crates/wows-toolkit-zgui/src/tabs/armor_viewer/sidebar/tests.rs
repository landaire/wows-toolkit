//! The sidebar driven headlessly over a small ship catalog and model catalogue.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::HeadlessError;
use landaire_ui::MARK_CLASS;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_armor::catalog::TierRange;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::model_refs::OpenTarget;
use wows_toolkit_armor::model_refs::ShipPartKey;
use wows_toolkit_armor::model_refs::ShipPartRef;
use wows_toolkit_armor::model_refs::VehicleParamIndex;
use wows_toolkit_armor::model_tree::ModelBuildings;
use wows_toolkit_armor::model_tree::ModelTreeIndex;
use wows_toolkit_armor::sidebar_tree::RowId;
use wows_toolkit_armor::sidebar_tree::building_row_id;
use wows_toolkit_armor::sidebar_tree::fixture;
use wows_toolkit_armor::sidebar_tree::model_building_row_id;
use wows_toolkit_armor::sidebar_tree::model_row_id;
use wows_toolkit_armor::sidebar_tree::part_row_id;
use wows_toolkit_armor::sidebar_tree::ship_row_id;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::GameParamId;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui::vocab::Toggled;

use super::ArmorSidebarProps;
use super::CommonPaneSettings;
use super::ModelsSource;
use super::ShipChoice;
use super::SidebarCatalog;
use super::SidebarEvent;
use super::built_state;
use super::rows::row_test_id;
use super::state::SidebarState;
use crate::game_art;
use crate::services::saved_for_test;
use crate::shell::ShellHarnessProps;

/// What the test steers the sidebar with.
#[derive(Clone, Copy)]
struct Inputs {
    open: RwSignal<Option<OpenTarget>, LocalStorage>,
    hardpoints: RwSignal<Rc<HashSet<HardpointTarget>>, LocalStorage>,
}

struct Sidebar {
    window: HeadlessWindow,
    state: SidebarState,
    inputs: Inputs,
    events: Rc<RefCell<Vec<SidebarEvent>>>,
}

/// Ship assets over an empty store: the sidebar reads the catalog, and art it does not find.
fn catalog() -> Arc<SidebarCatalog> {
    catalog_over(MemoryFS::new().into())
}

fn catalog_over(vfs: VfsPath) -> Arc<SidebarCatalog> {
    let metadata = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds");
    let assets = ShipAssets::from_parts(&vfs, Arc::new(metadata), Arc::new(Vec::new()));

    Arc::new(SidebarCatalog { assets: Arc::new(assets), catalog: Arc::new(fixture::ships()) })
}

fn index(buildings: ModelBuildings) -> ModelTreeIndex {
    let catalog = fixture::catalog();
    let ships = fixture::ships();
    let references = fixture::references(&ships);

    ModelTreeIndex::build(&catalog, &ships, &references, buildings, &fixture::metadata())
}

impl Sidebar {
    /// The sidebar over the fixture catalogue, with its model listing adopted.
    fn open() -> Self {
        rust_i18n::set_locale("en");
        let options = HeadlessOptions::new(WindowSize { width: 500.0, height: 1000.0 });
        let mut this = Self::mount(options, ThemeChoice::Dark, catalog(), ModelsSource::Loading);

        let catalog = fixture::catalog();
        let references = fixture::references(&fixture::ships());
        this.state.adopt_model_index(index(ModelBuildings::default()));
        this.state.adopt_ship_parts(fixture::ship_parts(&catalog, &references));
        this.settle();

        this
    }

    fn mount(options: HeadlessOptions, theme: ThemeChoice, loaded: Arc<SidebarCatalog>, models: ModelsSource) -> Self {
        Self::try_mount(options, theme, loaded, models).expect("the sidebar opens headlessly")
    }

    fn try_mount(
        options: HeadlessOptions,
        theme: ThemeChoice,
        loaded: Arc<SidebarCatalog>,
        models: ModelsSource,
    ) -> Result<Self, HeadlessError> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let caught: Rc<Cell<Option<Inputs>>> = Rc::new(Cell::new(None));
        let (recorder, inner) = (Rc::clone(&events), Rc::clone(&caught));
        let mut window = HeadlessWindow::open(options, move || {
            let inputs =
                Inputs { open: RwSignal::new_local(None), hardpoints: RwSignal::new_local(Rc::new(HashSet::new())) };
            inner.set(Some(inputs));
            let recorder = Rc::clone(&recorder);
            let on_event = UnsyncCallback::new(move |event: SidebarEvent| recorder.borrow_mut().push(event));
            let loaded = Some(Arc::clone(&loaded));
            let models = models.clone();
            view! {
                ShellHarness(theme = theme) {
                    ArmorSidebar(
                        catalog = Signal::stored_local(loaded),
                        models = Signal::stored_local(models),
                        common = Signal::stored_local(CommonPaneSettings::default()),
                        open = Signal::from(inputs.open),
                        hardpoints = Signal::from(inputs.hardpoints),
                        on_event = on_event
                    )
                }
            }
        })?;
        window.advance(Duration::from_millis(100));
        let state = built_state().expect("the sidebar was built");
        let inputs = caught.get().expect("the inputs were made");

        Ok(Self { window, state, inputs, events })
    }

    fn settle(&mut self) {
        self.window.advance(Duration::from_millis(300));
    }

    fn listed(&self, id: &RowId) -> bool {
        self.state.tree.with_untracked(|tree| tree.contains(id))
    }

    fn selected(&self) -> HashSet<RowId> {
        self.state.selection.get_untracked()
    }

    fn search(&mut self, query: &str) {
        self.state.apply_query(FilterQuery::new(query));
        self.settle();
    }

    fn browse(&mut self, ids: &[&str]) {
        self.state.open_folders(ids);
        self.settle();
    }

    fn click(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} takes a click: {err}"));
        self.settle();
    }

    fn click_row(&mut self, id: &RowId) {
        self.click(&row_test_id(id));
    }

    /// Puts the tree's cursor on row `id` with the tree focused, as arrowing to it does, and
    /// presses Enter.
    fn enter_on(&mut self, id: &RowId) {
        self.state.handle.focus_key(id.clone());
        self.settle();
        self.window.press_key(NamedKey::Enter, Modifiers::NONE);
        self.settle();
    }

    /// Opens row `id`'s menu and lists what it offers.
    fn menu_of(&mut self, id: &RowId) -> Vec<String> {
        let element = self.window.by_test_id(&row_test_id(id)).expect("the row is shown");
        self.window.right_click(element).expect("the row takes a secondary click");
        self.settle();
        let items = self.window.by_role(Role::MenuItem);
        let mut labels: Vec<String> =
            items.into_iter().filter_map(|item| self.window.a11y(item).and_then(|state| state.label)).collect();
        labels.sort();

        labels
    }

    fn events(&self) -> Vec<SidebarEvent> {
        self.events.borrow().clone()
    }

    fn marked(&self) -> Vec<String> {
        self.window.texts_by_class(MARK_CLASS)
    }
}

fn turret() -> wowsunpack::export::model_catalog::ModelPath {
    fixture::path(fixture::IOWA_TURRET)
}

fn iowa_choice() -> ShipChoice {
    ShipChoice { param_index: VehicleParamIndex::new(fixture::IOWA_INDEX), display_name: "Iowa".to_string() }
}

const USA_MODELS: [&str; 3] =
    ["model-folder/nation-usa", "model-folder/nation-usa/Gun", "model-folder/nation-usa/Gun/MainBattery"];

#[test]
fn copy_model_path_reports_the_path_it_copied() {
    let mut sidebar = Sidebar::open();
    for folder in USA_MODELS {
        sidebar.click_row(&RowId::from(folder));
    }
    let turret = model_row_id(&turret());
    assert!(sidebar.listed(&turret), "opening the folders lists the turret");

    sidebar.menu_of(&turret);
    sidebar.click("armor-sidebar-menu-copy-path");

    let toasts = sidebar.window.texts_by_class("lui-toasts").concat();
    assert!(toasts.contains(fixture::IOWA_TURRET), "the message names the path: {toasts:?}");
}

#[test]
fn a_catalogue_model_row_offers_to_open_export_and_copy_the_model() {
    let mut sidebar = Sidebar::open();
    sidebar.search("agm034");
    let turret = model_row_id(&turret());

    assert_eq!(sidebar.menu_of(&turret), ["Copy model path", "Export Model", "Open"]);
}

#[test]
fn a_ship_row_offers_a_comparison_pane_a_new_tab_and_an_export() {
    let mut sidebar = Sidebar::open();
    sidebar.search("iowa");
    let iowa = ship_row_id(fixture::IOWA_INDEX);

    assert_eq!(sidebar.menu_of(&iowa), ["Compare in new split", "Export Ship Model", "Open in new tab"]);
    sidebar.click("armor-sidebar-menu-compare");
    sidebar.menu_of(&iowa);
    sidebar.click("armor-sidebar-menu-new-tab");

    assert_eq!(
        sidebar.events(),
        [SidebarEvent::CompareSplit(Some(iowa_choice())), SidebarEvent::OpenInNewTab(iowa_choice())]
    );
}

#[test]
fn the_hardpoint_checkbox_toggles_without_opening_the_row_and_a_row_click_opens_it() {
    let mut sidebar = Sidebar::open();
    sidebar.search("agm034");
    let turret_row = model_row_id(&turret());

    sidebar.click(&format!("armor-sidebar-hardpoints-{turret_row}"));
    assert_eq!(
        sidebar.events(),
        [SidebarEvent::Hardpoints { target: HardpointTarget::Model(turret()), visible: true }],
        "the checkbox does not open the model"
    );
    assert!(!sidebar.selected().contains(&turret_row), "nor select its row");

    sidebar.click_row(&turret_row);
    assert_eq!(sidebar.events()[1..], [SidebarEvent::OpenModel { model: turret(), from_ship: None }]);
}

#[test]
fn the_pane_drives_the_checkbox_state_and_the_highlighted_row() {
    let mut sidebar = Sidebar::open();
    sidebar.search("agm034");
    let turret_row = model_row_id(&turret());

    sidebar.inputs.hardpoints.set(Rc::new(HashSet::from([HardpointTarget::Model(turret())])));
    sidebar.inputs.open.set(Some(OpenTarget::Model { model: turret(), from_ship: None }));
    sidebar.settle();

    let checkbox = sidebar.window.by_test_id(&format!("armor-sidebar-hardpoints-{turret_row}")).expect("shown");
    assert_eq!(sidebar.window.a11y(checkbox).and_then(|state| state.checked), Some(Toggled::True));
    assert_eq!(sidebar.selected(), HashSet::from([turret_row]), "the open model's row is highlighted");
}

#[test]
fn the_tier_filter_narrows_the_ships_and_saves_the_tiers_when_let_go() {
    let mut sidebar = Sidebar::open();
    sidebar.browse(&["armor-nation-USA", "armor-class-USA-Battleship"]);

    sidebar.state.choose_tiers(TierRange::new(3, 8));
    sidebar.state.commit_tiers(TierRange::new(3, 8));
    sidebar.settle();

    assert!(sidebar.listed(&ship_row_id("PASB001")), "tier III is listed");
    assert!(!sidebar.listed(&ship_row_id(fixture::IOWA_INDEX)), "tier IX is not");
    let saved: Vec<String> = saved_for_test()
        .into_iter()
        .filter(|(key, _)| *key == keys::MODEL_VIEWER_SHIP_TIERS)
        .map(|(_, json)| json)
        .collect();
    assert_eq!(saved, ["[3,8]"], "saved once, when the thumb is let go");
}

#[test]
fn saved_tiers_filter_the_ships() {
    let mut sidebar = Sidebar::open();
    sidebar.browse(&["armor-nation-USA", "armor-class-USA-Battleship"]);

    sidebar.state.set_tiers(TierRange::new(9, 11));
    sidebar.settle();

    assert_eq!(sidebar.state.tiers.get_untracked(), TierRange::new(9, 11));
    assert!(sidebar.listed(&ship_row_id(fixture::IOWA_INDEX)), "Iowa, tier IX, is listed");
    assert!(!sidebar.listed(&ship_row_id("PASB001")), "S. Carolina, tier III, is not");
}

#[test]
fn enter_on_the_highlighted_row_opens_it_or_its_folder() {
    let mut sidebar = Sidebar::open();
    let mut folders = vec!["armor-nation-USA", "armor-class-USA-Battleship"];
    folders.extend(USA_MODELS);
    sidebar.browse(&folders);

    sidebar.enter_on(&model_row_id(&turret()));
    assert_eq!(sidebar.events(), [SidebarEvent::OpenModel { model: turret(), from_ship: None }]);

    sidebar.enter_on(&ship_row_id(fixture::IOWA_INDEX));
    assert_eq!(sidebar.events()[1..], [SidebarEvent::OpenShip(iowa_choice())]);

    let japan = RowId::from("model-folder/nation-japan");
    sidebar.enter_on(&japan);
    assert!(sidebar.listed(&RowId::from("model-folder/nation-japan/Ship")), "the folder opened");
    assert_eq!(sidebar.selected(), HashSet::from([japan]), "and stays highlighted");
}

#[test]
fn show_hidden_ships_lists_them_tagged_and_saves_the_choice() {
    let mut sidebar = Sidebar::open();
    let rental = ship_row_id("PASB708");
    sidebar.browse(&["armor-nation-USA", "armor-class-USA-Battleship"]);
    assert!(!sidebar.listed(&rental), "the clan rental is left out");

    sidebar.state.choose_show_hidden(true);
    sidebar.settle();
    assert!(sidebar.listed(&rental), "the choice lists it");
    let note = sidebar.state.tree.with_untracked(|tree| tree.note(&rental).map(ToString::to_string));
    assert_eq!(note.as_deref(), Some("Clan rental"), "tagged with why it is normally left out");

    sidebar.state.choose_show_hidden(false);
    sidebar.settle();
    assert!(!sidebar.listed(&rental), "and clearing it leaves it out again");
    let saved: Vec<String> = saved_for_test()
        .into_iter()
        .filter(|(key, _)| *key == keys::MODEL_VIEWER_SHOW_HIDDEN_SHIPS)
        .map(|(_, json)| json)
        .collect();
    assert_eq!(saved, ["true", "false"]);
}

#[test]
fn a_ship_row_marks_the_name_it_was_found_by_and_not_its_tier() {
    let mut sidebar = Sidebar::open();

    sidebar.search("iowa");

    assert!(sidebar.listed(&ship_row_id(fixture::IOWA_INDEX)), "Iowa is kept");
    let marked = sidebar.marked();
    assert!(marked.contains(&"Iowa".to_string()), "{marked:?}");
    assert!(!marked.iter().any(|mark| mark.contains("IX")), "the tier is not marked: {marked:?}");
}

#[test]
fn a_part_row_marks_its_model_stem() {
    let mut sidebar = Sidebar::open();
    let mount =
        part_row_id(&VehicleParamIndex::new(fixture::IOWA_INDEX), &ShipPartKey::Mount(HardpointName::new("HP_AGM_1")));

    sidebar.search("agm034");

    assert!(sidebar.listed(&mount), "the search opens Iowa to its main battery");
    assert!(sidebar.marked().contains(&"AGM034".to_string()), "{:?}", sidebar.marked());
}

#[test]
fn a_model_row_and_a_variant_row_mark_their_stems_and_a_skin_tag_is_plain() {
    let mut sidebar = Sidebar::open();

    sidebar.search("lazarus");
    assert!(sidebar.marked().contains(&"Lazarus".to_string()), "{:?}", sidebar.marked());

    sidebar.search("halloween");
    assert!(sidebar.listed(&model_row_id(&fixture::path(fixture::IOWA_SKIN))), "the skin is listed under Iowa's hull");
    let marked = sidebar.marked();
    assert!(marked.contains(&"Halloween".to_string()), "{marked:?}");
    assert!(!marked.contains(&"Skin".to_string()), "the skin tag is drawn plain, not as a match");
}

#[test]
fn a_fortification_row_marks_its_name_or_the_field_it_was_found_by() {
    let mut sidebar = Sidebar::open();
    sidebar.state.adopt_fortifications(fixture::fortifications());
    let fort = building_row_id(GameParamId::from(fixture::FORT_OF_THE_ISLES));

    sidebar.search("isles");
    assert!(sidebar.listed(&fort), "the fort is kept");
    assert!(sidebar.marked().contains(&"Isles".to_string()), "{:?}", sidebar.marked());

    sidebar.search("pcbx001");
    let marked = sidebar.marked();
    assert!(!marked.iter().any(|mark| mark.contains("Isles")), "the name did not match: {marked:?}");
    assert!(marked.contains(&"PCBX001".to_string()), "the note names the match: {marked:?}");
}

#[test]
fn a_building_under_its_model_opens_the_building_and_the_model_row_the_model() {
    let mut sidebar = Sidebar::open();
    let battery = GameParamId::from(fixture::COASTAL_BATTERY);
    let fort_model = fixture::path(fixture::FORT);
    sidebar.state.adopt_model_index(index(fixture::model_buildings()));
    let fort_row = model_row_id(&fort_model);
    sidebar.browse(&["model-folder/Building", "model-folder/Building/Complex", fort_row.as_str()]);
    let battery_row = model_building_row_id(&fort_model, battery);
    assert!(sidebar.listed(&battery_row), "the building is listed under its model");

    sidebar.click_row(&battery_row);
    sidebar.enter_on(&battery_row);
    assert_eq!(sidebar.events(), [SidebarEvent::OpenBuilding(battery), SidebarEvent::OpenBuilding(battery)]);

    sidebar.inputs.open.set(Some(OpenTarget::Building(battery)));
    sidebar.settle();
    assert_eq!(sidebar.selected(), HashSet::from([battery_row]), "the open building's row is highlighted");

    let fort = GameParamId::from(fixture::FORT_OF_THE_ISLES);
    sidebar.browse(&["armor-fortifications", "armor-fortifications/Complex"]);
    sidebar.state.adopt_fortifications(fixture::fortifications());
    sidebar.inputs.open.set(Some(OpenTarget::Building(fort)));
    sidebar.settle();
    assert!(sidebar.listed(&model_building_row_id(&fort_model, fort)), "the fort is listed under its hull");
    assert_eq!(
        sidebar.selected(),
        HashSet::from([building_row_id(fort)]),
        "the Fortifications row is highlighted first"
    );

    sidebar.enter_on(&fort_row);
    assert_eq!(sidebar.events()[2..], [SidebarEvent::OpenModel { model: fort_model, from_ship: None }]);
}

#[test]
fn a_fortification_row_opens_its_building_by_click_and_by_enter() {
    let mut sidebar = Sidebar::open();
    sidebar.state.adopt_fortifications(fixture::fortifications());
    sidebar.browse(&["armor-fortifications", "armor-fortifications/Complex"]);
    let fort = GameParamId::from(fixture::FORT_OF_THE_ISLES);

    sidebar.click_row(&building_row_id(fort));
    sidebar.enter_on(&building_row_id(fort));

    assert_eq!(sidebar.events(), [SidebarEvent::OpenBuilding(fort), SidebarEvent::OpenBuilding(fort)]);
}

#[test]
fn a_fortification_row_and_its_folders_open_no_menu() {
    let mut sidebar = Sidebar::open();
    sidebar.state.adopt_fortifications(fixture::fortifications());
    sidebar.browse(&["armor-fortifications", "armor-fortifications/Complex"]);
    let fort = building_row_id(GameParamId::from(fixture::FORT_OF_THE_ISLES));

    for row in [RowId::from("armor-fortifications"), RowId::from("armor-fortifications/Complex"), fort.clone()] {
        assert_eq!(sidebar.menu_of(&row), Vec::<String>::new(), "{row} offers nothing");
        assert!(sidebar.window.by_role(Role::Menu).is_empty(), "so {row} opens no menu");
    }
    assert!(!sidebar.selected().contains(&fort), "nor selects the building's row");
    assert!(sidebar.events().is_empty(), "nor opens the building");
}

#[test]
fn enter_in_the_search_field_opens_the_first_match_and_down_moves_into_the_list() {
    let mut sidebar = Sidebar::open();

    sidebar.click("armor-sidebar-search");
    sidebar.window.type_text("yamato");
    sidebar.window.press_key(NamedKey::Enter, Modifiers::NONE);
    sidebar.settle();
    let yamato = ShipChoice { param_index: VehicleParamIndex::new("PJSB018"), display_name: "Yamato".to_string() };
    assert_eq!(
        sidebar.events(),
        [SidebarEvent::OpenShip(yamato)],
        "the search applies at once and its first row opens"
    );

    // A search naming only a model, typed and still waiting out its pause when Down is pressed.
    sidebar.state.search_text.set("lazarus".to_string());
    sidebar.state.search_changed("lazarus");
    sidebar.click("armor-sidebar-search");
    sidebar.window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    sidebar.settle();
    sidebar.window.press_key(NamedKey::Enter, Modifiers::NONE);
    sidebar.settle();

    let lazarus = fixture::path("content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model");
    assert_eq!(
        sidebar.events()[1..],
        [SidebarEvent::OpenModel { model: lazarus, from_ship: None }],
        "down moved into the list"
    );
}

#[test]
fn typing_a_search_filters_ships_and_models() {
    let mut sidebar = Sidebar::open();

    sidebar.click("armor-sidebar-search");
    sidebar.window.type_text("iowa");
    sidebar.settle();

    assert!(sidebar.listed(&ship_row_id(fixture::IOWA_INDEX)), "the ship is listed");
    assert!(sidebar.listed(&model_row_id(&fixture::path(fixture::IOWA))), "the model is listed");
    assert!(!sidebar.listed(&ship_row_id("PJSB018")), "a ship not matching is not");
    let yamato = fixture::path("content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model");
    assert!(!sidebar.listed(&model_row_id(&yamato)), "nor a model not matching");

    sidebar.window.press_key(NamedKey::Escape, Modifiers::NONE);
    sidebar.settle();
    assert_eq!(sidebar.state.search_text.get_untracked(), "", "Escape empties the search");
    assert!(!sidebar.state.query.get_untracked().is_active(), "and the listing is no longer searched");
}

#[test]
fn a_folder_closed_during_a_search_stays_closed_when_the_ship_parts_arrive() {
    let mut sidebar = Sidebar::open();
    let iowa = ship_row_id(fixture::IOWA_INDEX);
    sidebar.search("agm034");
    assert!(sidebar.listed(&iowa), "the search opens the ship found by its parts");

    let mut expanded = sidebar.state.expanded.get_untracked();
    expanded.remove("armor-nation-USA");
    sidebar.state.set_expanded(expanded);
    let catalog = fixture::catalog();
    let references = fixture::references(&fixture::ships());
    sidebar.state.adopt_ship_parts(fixture::ship_parts(&catalog, &references));
    sidebar.settle();

    assert!(sidebar.listed(&RowId::from("armor-nation-USA")), "the nation is listed");
    assert!(!sidebar.listed(&iowa), "and stays closed");
}

#[test]
fn a_ship_row_loads_the_ship_and_a_part_row_opens_the_part_model() {
    let mut sidebar = Sidebar::open();
    sidebar.browse(&["armor-nation-USA", "armor-class-USA-Battleship", "armor-part-PASB018-group-MainBattery"]);
    let iowa = ship_row_id(fixture::IOWA_INDEX);

    sidebar.click_row(&iowa);
    assert_eq!(sidebar.events(), [SidebarEvent::OpenShip(iowa_choice())]);

    let vehicle = VehicleParamIndex::new(fixture::IOWA_INDEX);
    let part = ShipPartKey::Mount(HardpointName::new("HP_AGM_1"));
    let part_row = part_row_id(&vehicle, &part);
    assert!(sidebar.listed(&part_row), "opening the ship lists its mounts");
    sidebar.click_row(&part_row);

    let from_ship = Some(ShipPartRef { vehicle_param_index: vehicle, part });
    assert_eq!(sidebar.events()[1..], [SidebarEvent::OpenModel { model: turret(), from_ship }]);
}

/// The game draws a class icon pointing up; the sidebar shows it pointing right, as the other
/// tabs do.
#[test]
fn a_class_icon_points_right() {
    rust_i18n::set_locale("en");
    let vfs = game_art::testing::upright_class_icon(Species::Battleship);
    let options = HeadlessOptions {
        drawing: Drawing::Device,
        ..HeadlessOptions::new(WindowSize { width: 400.0, height: 300.0 })
    };
    let mut sidebar = match Sidebar::try_mount(options, ThemeChoice::Dark, catalog_over(vfs), ModelsSource::Loading) {
        Ok(sidebar) => sidebar,
        Err(err) if game_art::testing::no_device(&err) => {
            eprintln!("skipped: no graphics adapter to draw the sidebar on");
            return;
        }
        Err(err) => panic!("the sidebar opens: {err}"),
    };
    sidebar.browse(&["armor-nation-USA"]);
    crate::interaction_tests::settle_in_real_time(&mut sidebar.window);

    game_art::testing::assert_points_right(&mut sidebar.window, "wt-armor-row__class");
}

/// The catalogue and model library of the install named by `WT_WOWS_DIR`.
fn real_catalog() -> (Arc<SidebarCatalog>, ModelsSource) {
    let Some(dir) = std::env::var_os("WT_WOWS_DIR").map(std::path::PathBuf::from) else {
        panic!("WT_WOWS_DIR names a game install");
    };
    let cache = wows_toolkit_replay_data::load::GameDataCache::new(dir.clone());
    let data = wows_toolkit_replay_data::load::load_current_build(&dir, &cache).expect("the build loads");
    let assets =
        wows_toolkit_armor::assets::load_ship_assets(data.vfs_with_assets(), data.assets_bin_bytes(), data.provider())
            .expect("the ship assets load");
    let models = wows_toolkit_armor::assets::ModelAssets::build(&assets).expect("the model library builds");
    let catalog = wows_toolkit_armor::catalog::ShipCatalog::build(assets.metadata());
    let loaded = SidebarCatalog { assets: Arc::new(assets), catalog: Arc::new(catalog) };

    (Arc::new(loaded), ModelsSource::Ready(models.library))
}

/// Runs frames in real time until the model catalogue is listed, so the work off the UI thread
/// lands.
fn settle_until_listed(sidebar: &mut Sidebar) {
    for _ in 0..600 {
        if sidebar.state.models.with_untracked(|models| matches!(models, super::state::ModelsState::Ready(_))) {
            break;
        }
        sidebar.window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(50));
    }
    for _ in 0..1200 {
        if sidebar.state.parts_listed() {
            break;
        }
        sidebar.window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(50));
    }
    crate::interaction_tests::settle_in_real_time(&mut sidebar.window);
}

/// The sidebar over a real install, browsing and searching, in both palettes. Writes
/// `target/zgui-shots/armor-sidebar-*.png`.
#[test]
#[ignore = "draws on the graphics device and reads a game install; run by hand to review the sidebar"]
fn the_sidebar_renders_the_real_catalogue_in_dark_and_light() {
    use landaire_ui::Appearance;

    rust_i18n::set_locale("en");
    let (loaded, models) = real_catalog();
    let size = WindowSize { width: 480.0, height: 900.0 };

    for (theme, appearance, name) in
        [(ThemeChoice::Dark, Appearance::Dark, "dark"), (ThemeChoice::Light, Appearance::Light, "light")]
    {
        let options =
            HeadlessOptions { drawing: Drawing::Device, desktop: Some(appearance), ..HeadlessOptions::new(size) };
        let mut sidebar = Sidebar::mount(options, theme, Arc::clone(&loaded), models.clone());
        settle_until_listed(&mut sidebar);

        sidebar.browse(&["armor-nation-USA", "armor-class-USA-Battleship", "model-folder/nation-usa"]);
        sidebar.inputs.open.set(Some(OpenTarget::Ship(VehicleParamIndex::new("PASB018"))));
        sidebar.settle();
        let path = crate::interaction_tests::save_shot(&mut sidebar.window, &format!("armor-sidebar-browse-{name}"));
        println!("{}", path.display());

        sidebar.click("armor-sidebar-search");
        sidebar.window.type_text("iowa");
        sidebar.settle();
        let path = crate::interaction_tests::save_shot(&mut sidebar.window, &format!("armor-sidebar-search-{name}"));
        println!("{}", path.display());

        sidebar.click("armor-sidebar-common");
        let path = crate::interaction_tests::save_shot(&mut sidebar.window, &format!("armor-sidebar-common-{name}"));
        println!("{}", path.display());
    }
}

//! The armor viewer's panels opened headlessly in the main window's contexts.
//!
//! The review renders read the install named by `WT_WOWS_DIR`.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_armor::assets::load_ship_assets;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wows_toolkit_viewmodel::armor::penetration::resolve_ship_shells;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_params::types::Species;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::IntoView;

use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::shell::DialogHostProps;
use crate::shell::Shell;
use crate::shell::ShellHarnessProps;
use crate::tabs::armor_viewer::panels::export::ExportRequest;
use crate::tabs::armor_viewer::panels::export::open_export_dialog;
use crate::tabs::armor_viewer::panels::incoming::IncomingFire;
use crate::tabs::armor_viewer::panels::legend;
use crate::tabs::armor_viewer::panels::legend::ArmorLegendProps;
use crate::tabs::armor_viewer::panels::legend::DEFAULT_POS;
use crate::tabs::armor_viewer::panels::legend::LegendState;
use crate::tabs::armor_viewer::panels::pen::PenCheckPanelProps;
use crate::tabs::armor_viewer::panels::pen::PenetrationState;
use crate::tabs::armor_viewer::panels::pen::PlateUnderPointer;
use crate::tabs::armor_viewer::panels::trajectory::TrajectoryPanelProps;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use wows_toolkit_armor::trajectory;
use wows_toolkit_armor::trajectory::Trajectory;
use wows_toolkit_armor::trajectory::TrajectoryHit;
use wows_toolkit_armor::viewport::types::Vec3;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::Degrees;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::ShipModelDistance;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 900.0, height: 800.0 };

/// What a mounted page hands the test to steer.
#[derive(Clone, Copy)]
struct Mounted {
    shell: Shell,
    legend: RwSignal<LegendState, LocalStorage>,
    saved: RwSignal<usize, LocalStorage>,
    pen: RwSignal<PenetrationState, LocalStorage>,
    viewport: ArmorViewportState,
}

type Caught = Rc<Cell<Option<Mounted>>>;

/// The panels side by side as the options rail holds them, with the legend over a viewer area.
#[component]
fn Page(caught: Caught, catalog: Option<Arc<ShipCatalog>>, assets: Option<Arc<ShipAssets>>) -> impl IntoView {
    let shell = Shell::current();
    let legend = RwSignal::new_local(LegendState::default());
    let saved = RwSignal::new_local(0usize);
    let pen = RwSignal::new_local(PenetrationState::default());
    let incoming = RwSignal::new_local(IncomingFire::default());
    let viewport = ArmorViewportState::new();
    caught.set(Some(Mounted { shell, legend, saved, pen, viewport }));
    let catalog = Signal::stored_local(catalog);
    let assets = Signal::stored_local(assets);

    view! {
        row(style = "width: 100%; height: 100%; gap: 12px; padding: 8px; background-color: var(--lui-color-bg)") {
            column(style = "width: 360px; gap: 16px") {
                PenCheckPanel(
                    pen = pen,
                    catalog = catalog,
                    assets = assets,
                    incoming = incoming,
                    playback_at = Signal::stored_local(None),
                    viewport = viewport
                )
                TrajectoryPanel(pen = pen, viewport = viewport)
            }
            box(style = "flex: 1; position: relative; background-color: var(--lui-color-panel)") {
                ArmorLegend(state = legend, on_saved_change = UnsyncCallback::new(move |()| saved.update(|saved| *saved += 1)))
            }
            DialogHost(dialogs = shell.dialogs)
        }
    }
}

fn open_with(
    theme: ThemeChoice,
    options: HeadlessOptions,
    catalog: Option<Arc<ShipCatalog>>,
    assets: Option<Arc<ShipAssets>>,
) -> (HeadlessWindow, Mounted) {
    let caught: Caught = Rc::new(Cell::new(None));
    let inner = Rc::clone(&caught);
    let mut window = HeadlessWindow::open(options, move || {
        let (caught, catalog, assets) = (Rc::clone(&inner), catalog.clone(), assets.clone());
        view! { ShellHarness(theme = theme) { Page(caught = caught, catalog = catalog, assets = assets) } }
    })
    .expect("the panels open headlessly");
    window.advance(Duration::from_millis(100));
    let mounted = caught.get().expect("the page is built");

    (window, mounted)
}

fn open() -> (HeadlessWindow, Mounted) {
    open_with(ThemeChoice::Dark, HeadlessOptions::new(SIZE), None, None)
}

fn shown(window: &HeadlessWindow, id: &str) -> bool {
    window.by_test_id(id).is_some()
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let found = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is on screen"));
    window.click(found).expect("the element takes a click");
    window.advance(Duration::from_millis(300));
}

fn text_of(window: &HeadlessWindow, class: &str) -> Vec<String> {
    window.texts_by_class(class)
}

fn shell(ammo_type: AmmoType, caliber: f32, he_pen_mm: Option<f32>) -> ShellInfo {
    ShellInfo {
        name: "PTEST".to_owned(),
        ammo_type,
        caliber: Millimeters::from(caliber),
        he_pen_mm,
        sap_pen_mm: None,
        alpha_damage: 0.0,
        muzzle_velocity: 800.0,
        mass_kg: 50.0,
        krupp: 2000.0,
        ricochet_angle: None,
        always_ricochet_angle: None,
        fuse_time: None,
        fuse_threshold: None,
        burn_prob: None,
        air_drag: 0.3,
        normalization: None,
        cap: true,
    }
}

fn ship(param_index: &str, display_name: &str) -> ComparisonShip {
    ComparisonShip {
        param_index: param_index.to_owned(),
        display_name: display_name.to_owned(),
        tier: 8,
        nation: "usa".to_owned(),
        species: Species::Cruiser,
        shells: vec![shell(AmmoType::AP, 203.0, None), shell(AmmoType::HE, 203.0, Some(40.0))],
    }
}

#[test]
fn the_legend_collapses_closes_and_is_dragged_saving_each() {
    let (mut window, mounted) = open();
    assert_eq!(text_of(&window, "wt-armor-legend__label").len(), 10, "one row per thickness band");

    click(&mut window, "armor-legend-collapse");
    assert!(mounted.legend.get_untracked().collapsed);
    assert!(text_of(&window, "wt-armor-legend__label").is_empty(), "a collapsed legend keeps its title only");
    assert_eq!(mounted.saved.get_untracked(), 1);

    let handle = window.by_test_id("armor-legend-drag-handle").expect("the title is the handle");
    let from = window.bounds(handle).expect("the handle is laid out");
    let to = zgui::geom::Point::new(
        zgui::geom::DevicePx(from.origin.x.0 + 60.0),
        zgui::geom::DevicePx(from.origin.y.0 + 40.0),
    );
    window.drag(handle, to, 4).expect("the handle drags");
    window.advance(Duration::from_millis(100));
    let moved = mounted.legend.get_untracked().pos;
    assert!(moved.x > DEFAULT_POS.x && moved.y > DEFAULT_POS.y, "the legend follows the pointer: {moved:?}");
    assert_eq!(mounted.saved.get_untracked(), 2, "the end of a drag is saved");

    click(&mut window, "armor-legend-close");
    assert!(!shown(&window, "armor-legend-panel"), "a closed legend is gone");
    assert_eq!(mounted.saved.get_untracked(), 3);
}

#[test]
fn dragging_the_legend_moves_it_without_building_it_again() {
    let (mut window, mounted) = open();
    let built = legend::builds_for_test();

    let handle = window.by_test_id("armor-legend-drag-handle").expect("the title is the handle");
    let from = window.bounds(handle).expect("the handle is laid out");
    let to = zgui::geom::Point::new(
        zgui::geom::DevicePx(from.origin.x.0 + 60.0),
        zgui::geom::DevicePx(from.origin.y.0 + 40.0),
    );
    window.drag(handle, to, 4).expect("the handle drags");
    window.advance(Duration::from_millis(100));

    assert_ne!(mounted.legend.get_untracked().pos, DEFAULT_POS, "the legend moved");
    assert_eq!(legend::builds_for_test(), built, "neither the panel nor its rows were built again");
}

#[test]
fn the_checker_reads_each_shell_against_the_plate_under_the_pointer() {
    let (mut window, mounted) = open();
    assert!(shown(&window, "armor-pen"));
    assert!(!shown(&window, "armor-pen-ships"), "nothing is compared yet");

    mounted.pen.update(|pen| {
        pen.add(ship("PA", "Alpha"));
        pen.add(ship("PB", "Bravo"));
    });
    window.advance(Duration::from_millis(100));
    let verdicts = text_of(&window, "wt-armor-pen__verdict");
    assert_eq!(verdicts.len(), 4, "a row per shell per ship: {verdicts:?}");
    assert!(verdicts.iter().all(|verdict| verdict == "hover a plate"), "{verdicts:?}");

    mounted.pen.update(|pen| {
        pen.plate = Some(PlateUnderPointer { zone: "Citadel".to_owned(), thickness: Millimeters::from(30.0) })
    });
    window.advance(Duration::from_millis(100));
    assert_eq!(text_of(&window, "wt-armor-pen__shell-pen"), ["-", "40mm", "-", "40mm"]);
    let he = window.by_attribute("data-tone", "ok");
    assert!(he.is_some(), "40 mm of HE gets through 30 mm");

    click(&mut window, "armor-pen-ifhe");
    assert!(mounted.pen.get_untracked().ifhe);
    assert_eq!(text_of(&window, "wt-armor-pen__shell-pen"), ["-", "50mm", "-", "50mm"], "IFHE adds a quarter");

    assert_eq!(mounted.pen.get_untracked().firing_ship.as_deref(), Some("PA"), "the first ship added fires");
    click(&mut window, "armor-pen-firing-1");
    assert_eq!(mounted.pen.get_untracked().firing_ship.as_deref(), Some("PB"));

    click(&mut window, "armor-pen-remove-1");
    assert_eq!(mounted.pen.get_untracked().ships.len(), 1);
    assert_eq!(mounted.pen.get_untracked().firing_ship, None, "dropping the firing ship stops it firing");

    click(&mut window, "armor-pen-clear");
    assert!(mounted.pen.get_untracked().ships.is_empty());
}

#[test]
fn the_trajectory_section_asks_for_a_ship_until_one_is_compared() {
    let (mut window, mounted) = open();
    assert!(shown(&window, "armor-trajectory-add-ship"), "with no ship the button leads to the checker");
    assert!(shown(&window, "armor-arc-none"), "nothing is cast yet");

    mounted.pen.update(|pen| pen.add(ship("PA", "Alpha")));
    window.advance(Duration::from_millis(100));
    assert!(!shown(&window, "armor-trajectory-add-ship"));
    assert!(shown(&window, "armor-trajectory-ship"));
    assert!(shown(&window, "armor-trajectory-shell"));
}

/// A shell through two plates, cast with no ship chosen.
fn cast_from(range: f32) -> Trajectory {
    let hit = |at: f32| TrajectoryHit {
        position: Vec3::new(at, 0.0, 0.0),
        thickness: Millimeters::from(32.0),
        zone: "Citadel".to_owned(),
        material: ArmorMaterial::parse("Belt").expect("Belt is a table key"),
        angle_from_normal: Degrees::from(10.0),
        distance_from_start: ShipModelDistance::from(at),
        normal: Vec3::x(),
    };
    trajectory::cast(vec![hit(0.0), hit(3.0)], Vec3::x(), None, 100.0, false, Km::new(range))
}

fn selected(window: &mut HeadlessWindow, id: &str) -> Option<bool> {
    let found = window.by_test_id(id)?;
    window.a11y(found)?.selected
}

/// Each arc has a marker. A click on one opens its editor, whose range slider fires that arc again
/// and no other; a right-click removes its arc; a click on the open arc's marker puts the editor
/// away.
#[test]
fn a_marker_selects_its_arc_for_editing_and_removes_it() {
    let (mut window, mounted) = open();
    assert!(!shown(&window, "armor-arc-markers"), "nothing is cast yet");

    let viewport = mounted.viewport;
    let first = viewport.add_arc(Vec3::x(), cast_from(10.0)).expect("the viewport takes an arc");
    viewport.add_arc(Vec3::x(), cast_from(12.0)).expect("the viewport takes an arc");
    window.advance(Duration::from_millis(100));
    assert_eq!(selected(&mut window, "armor-arc-marker-1"), Some(true), "a new arc opens for editing");
    assert_eq!(selected(&mut window, "armor-arc-marker-0"), Some(false));
    assert!(shown(&window, "armor-arc-editor"));

    click(&mut window, "armor-arc-marker-0");
    assert_eq!(selected(&mut window, "armor-arc-marker-0"), Some(true), "a marker selects its arc");
    assert_eq!(viewport.selected_arc().map(|(id, _)| id), Some(first));

    // The editor's slider is the last on the page; a press at its middle asks for 15 km.
    let slider = *window.by_role(Role::Slider).last().expect("the editor has a range slider");
    window.click(slider).expect("the slider takes a click");
    window.advance(Duration::from_millis(100));
    let ranges: Vec<f32> = viewport.arcs().iter().map(|arc| arc.range.value()).collect();
    assert!((ranges[0] - 15.0).abs() <= 1.0, "the slider fires the selected arc from its new range: {ranges:?}");
    assert_eq!(ranges[1], 12.0, "and leaves the other");

    let second = window.by_test_id("armor-arc-marker-1").expect("the second marker is on screen");
    window.right_click(second).expect("the marker takes a right-click");
    window.advance(Duration::from_millis(100));
    assert!(!shown(&window, "armor-arc-marker-1"), "a right-click removes its arc");
    assert_eq!(selected(&mut window, "armor-arc-marker-0"), Some(true), "and leaves the one open");

    click(&mut window, "armor-arc-marker-0");
    assert!(!shown(&window, "armor-arc-editor"), "the open arc's marker puts its editor away");
    assert!(shown(&window, "armor-arc-marker-0"), "without removing the arc");
}

/// The latest build of the install `WT_WOWS_DIR` names, loaded as the app loads it: with the
/// game's translations, which the catalog's display names come from.
fn real_assets() -> (Arc<ShipAssets>, Arc<ShipCatalog>, Arc<GameMetadataProvider>) {
    let wows_dir = PathBuf::from(std::env::var("WT_WOWS_DIR").expect("WT_WOWS_DIR names a game install"));
    let available = wowsunpack::game_data::list_available_builds(&wows_dir).expect("the install lists its builds");
    let build = *available.last().expect("the install has a build");
    let loaded = GameDataCache::new(wows_dir).get_or_load_build(build).expect("the build loads");
    let metadata = Arc::clone(loaded.provider());
    let assets =
        load_ship_assets(loaded.vfs_with_assets(), loaded.assets_bin_bytes(), &metadata).expect("the ship assets load");
    let catalog = ShipCatalog::build(&metadata);

    (Arc::new(assets), Arc::new(catalog), metadata)
}

/// The catalog's ship called `name`.
fn param_index_of(catalog: &ShipCatalog, name: &str) -> String {
    catalog
        .nations
        .iter()
        .flat_map(|nation| &nation.classes)
        .flat_map(|class| &class.ships)
        .find(|ship| ship.display_name == name)
        .unwrap_or_else(|| panic!("{name} is in the catalog"))
        .param_index
        .clone()
}

fn review_options(appearance: Appearance) -> HeadlessOptions {
    HeadlessOptions { drawing: Drawing::Device, desktop: Some(appearance), ..HeadlessOptions::new(SIZE) }
}

/// The checker comparing two real ships against a plate, the trajectory section and the legend,
/// then the export dialog on one of them, in both themes, for review.
#[test]
#[ignore = "draws on the graphics device; set WT_WOWS_DIR to review the panels"]
fn the_panels_render_in_dark_and_light() {
    let (assets, catalog, metadata) = real_assets();
    let des_moines = param_index_of(&catalog, "Des Moines");
    let yamato = param_index_of(&catalog, "Yamato");
    for (theme, appearance, name) in [
        (ThemeChoice::Dark, Appearance::Dark, "armor-panels-dark"),
        (ThemeChoice::Light, Appearance::Light, "armor-panels-light"),
    ] {
        let (mut window, mounted) =
            open_with(theme, review_options(appearance), Some(Arc::clone(&catalog)), Some(Arc::clone(&assets)));
        mounted.pen.update(|pen| {
            for param_index in [&des_moines, &yamato] {
                pen.add(resolve_ship_shells(&metadata, param_index).expect("the ship's shells resolve"));
            }
            pen.plate = Some(PlateUnderPointer { zone: "Citadel".to_owned(), thickness: Millimeters::from(32.0) });
        });
        window.advance(Duration::from_millis(300));
        println!("{}", save_shot(&mut window, name).display());

        let search = window.by_test_id("armor-pen-search").expect("the search is on screen");
        window.click(search).expect("the search takes a click");
        window.type_text("mont");
        window.advance(Duration::from_millis(300));
        assert!(shown(&window, "armor-pen-add-0"), "typing offers ships");
        println!("{}", save_shot(&mut window, &format!("{name}-search")).display());

        let request = ExportRequest::Ship {
            param_index: des_moines.clone(),
            display_name: "Des Moines".to_owned(),
            seed_hull: None,
        };
        open_export_dialog(mounted.shell, request, Arc::clone(&assets));
        for _ in 0..10 {
            settle_in_real_time(&mut window);
            if shown(&window, "armor-export-size") {
                break;
            }
        }
        assert!(shown(&window, "armor-export-dialog"), "the export dialog is open");
        assert!(shown(&window, "armor-export-contents-0"));
        assert!(shown(&window, "armor-export-res-0"));
        println!("{}", save_shot(&mut window, &format!("{name}-export")).display());
    }
}

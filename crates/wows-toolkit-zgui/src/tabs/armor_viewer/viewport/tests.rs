//! The viewport drawn on the graphics device: a real ship rendered, turned by a drag and picked
//! under the pointer, and a scene carried across a change of device.
//!
//! The tests that need a game install read it from `WT_WOWS_DIR`, and every test that draws skips
//! with a note when there is no graphics adapter.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::HeadlessError;
use landaire_ui::Scheme;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use rust_i18n::t;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::load_model::fixtures::armored_model_scene;
use wows_toolkit_armor::load_ship::ArmorThickness;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::SceneSubject;
use wows_toolkit_armor::load_ship::ThicknessSource;
use wows_toolkit_armor::load_ship::load_ship_armor_by_param;
use wows_toolkit_armor::model_refs::VehicleParamIndex;
use wows_toolkit_armor::plate_picking;
use wows_toolkit_armor::scene_load::LoadCancel;
use wows_toolkit_armor::viewport::device::ViewportDevice;
use wows_toolkit_armor::viewport::headless::HeadlessDevice;
use wows_toolkit_armor::viewport::headless::SRGB_TARGET;
use wows_toolkit_armor::viewport::renderer::GpuPipeline;
use wows_toolkit_armor::viewport::types::TargetSize;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_armor::viewport::types::ViewRect;
use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_data::AssetsBin;
use wowsunpack::game_data::build_game_vfs_for_build;
use wowsunpack::game_data::list_available_builds;
use wowsunpack::game_params::provider::GameMetadataProvider;
use zgui::component;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::platform::CursorStyle;
use zgui::prelude::Zoom;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use super::ArmorViewportProps;
use super::ArmorViewportState;
use super::OpeningLayer;
use super::SceneSource;
use super::host::DeviceStatus;
use super::model::ViewportModel;
use super::state::ViewCx;
use crate::services::Services;
use crate::tabs::armor_viewer::panels::incoming::IncomingFire;
use crate::tabs::armor_viewer::panels::incoming::IncomingLogProps;
use crate::tabs::armor_viewer::test_fixtures::plated_scene;
use crate::tabs::armor_viewer::test_fixtures::salvo_on_plate;
use crate::theme::SemanticTokensProps;

/// The size the viewport is reviewed at.
const VIEW: WindowSize = WindowSize { width: 1000.0, height: 640.0 };

/// Where the viewport sits in its window, in CSS pixels.
#[derive(Clone, Copy, Debug)]
struct ViewBox {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl ViewBox {
    /// The whole of a window [`VIEW`] in size.
    const FILL: Self = Self { left: 0.0, top: 0.0, width: VIEW.width, height: VIEW.height };
}

/// How long the pipeline may take to compile and the first frame to land, in real time.
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(60);

/// The zoom the tooltip placement is checked at.
const TIP_ZOOM: f32 = 1.5;

/// One CSS pixel, the gap between the pointer and the tooltip, in device pixels.
const GAP: f32 = TIP_ZOOM;

/// A ship from a real install, loaded once for every test in the process.
struct RealShip {
    assets: Arc<ShipAssets>,
    param_index: String,
    display_name: String,
    scene: Arc<LoadedShipArmor>,
}

/// Iowa from the install `WT_WOWS_DIR` names, its background stage run, loaded once for every
/// test in the process. `None` without an install.
static REAL_SHIP: LazyLock<Option<RealShip>> = LazyLock::new(load_real_ship);

fn real_ship() -> Option<&'static RealShip> {
    REAL_SHIP.as_ref()
}

fn load_real_ship() -> Option<RealShip> {
    let wows_dir = PathBuf::from(std::env::var_os("WT_WOWS_DIR")?);
    let build = *list_available_builds(&wows_dir).ok()?.last()?;
    let vfs = build_game_vfs_for_build(&wows_dir, build, AssetsBin::Overlay).expect("the game VFS builds");
    let metadata = Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("GameParams load"));
    let assets =
        Arc::new(ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata)).expect("the ship assets load"));
    let catalog = ShipCatalog::build(&metadata);
    let ship = catalog
        .nations
        .iter()
        .flat_map(|nation| &nation.classes)
        .flat_map(|class| &class.ships)
        .find(|ship| ship.display_name.contains("Iowa"))
        .expect("Iowa is in the catalog");
    let first = load_ship_armor_by_param(&assets, &ship.param_index, &ship.display_name, &LoadCancel::new())
        .expect("Iowa's armor loads");
    let cancel = LoadCancel::new();
    for job in first.rest {
        job.run(&first.scene, &cancel).expect("a background job of the load runs");
    }

    Some(RealShip {
        assets,
        param_index: ship.param_index.clone(),
        display_name: ship.display_name.clone(),
        scene: Arc::new(first.scene),
    })
}

/// The viewport at `place` in a themed window, handing its state out.
#[component]
fn Harness(scheme: Scheme, place: ViewBox, handed: Rc<Cell<Option<ArmorViewportState>>>) -> impl IntoView {
    Services::provide(None);
    let state = ArmorViewportState::new();
    handed.set(Some(state));
    let choice = Signal::stored_local(ThemeChoice::Fixed(scheme));
    let px = |value: f32| format!("{value}px");

    view! {
        ThemeRoot(choice = choice) {
            SemanticTokens(choice = Signal::stored_local(match scheme {
                Scheme::Light => wows_toolkit_viewmodel::settings::ThemeChoice::Light,
                _ => wows_toolkit_viewmodel::settings::ThemeChoice::Dark,
            }))
            box(
                style:position = "absolute",
                style:left = px(place.left),
                style:top = px(place.top),
                style:width = px(place.width),
                style:height = px(place.height),
                style:display = "flex",
                style:background-color = "var(--lui-color-panel)"
            ) {
                ArmorViewport(state = state)
            }
        }
    }
}

/// A window showing the viewport on the graphics device, or `None` with a note when this machine
/// has no adapter.
fn open(appearance: Appearance) -> Option<(HeadlessWindow, ArmorViewportState)> {
    open_in(HeadlessOptions { desktop: Some(appearance), ..HeadlessOptions::new(VIEW) }, ViewBox::FILL)
}

/// [`open`] with the window `options` describe and the viewport at `place` in it.
fn open_in(options: HeadlessOptions, place: ViewBox) -> Option<(HeadlessWindow, ArmorViewportState)> {
    let scheme = match options.desktop {
        Some(Appearance::Light) => Scheme::Light,
        _ => Scheme::Dark,
    };
    let handed = Rc::new(Cell::new(None));
    let given = Rc::clone(&handed);
    let options = HeadlessOptions { drawing: Drawing::Device, ..options };
    let opened = HeadlessWindow::open(options, move || {
        let handed = Rc::clone(&given);
        view! { Harness(scheme = scheme, place = place, handed = handed) }
    });
    match opened {
        Ok(window) => Some((window, handed.get().expect("the harness hands its state out"))),
        Err(HeadlessError::NoWindow | HeadlessError::App { source: zgui::runtime::AppError::GpuUnavailable(_) }) => {
            eprintln!("skipped: no graphics adapter to draw the viewport on");
            None
        }
        Err(error) => panic!("the window opens: {error}"),
    }
}

/// Shows the real ship in `state` and runs frames until it is drawn.
fn show_ship(window: &mut HeadlessWindow, state: ArmorViewportState, ship: &RealShip) {
    state.set_scene_source(SceneSource::Ship {
        assets: Arc::clone(&ship.assets),
        param_index: ship.param_index.clone(),
        display_name: ship.display_name.clone(),
    });
    state.show_armor(Arc::clone(&ship.scene), OpeningLayer::Armor);
    settle_until(window, || state.read_untracked(ViewportModel::scene_is_shown).unwrap_or(false));
    // The frame after the upload draws the meshes.
    window.advance(Duration::from_millis(100));
}

/// Runs frames, in real time as well as on the window's clock, until `done` holds: the pipeline
/// compiles on a thread of its own.
fn settle_until(window: &mut HeadlessWindow, done: impl Fn() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "the viewport is drawn within {FIRST_FRAME_WAIT:?}");
        window.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The share of the viewport's pixels unlike its top-left corner, which is open water.
fn drawn_share(window: &mut HeadlessWindow) -> f32 {
    let viewport = window.by_test_id("armor-viewport").expect("the viewport is in the document");
    let bounds = window.bounds(viewport).expect("the viewport has a box");
    let pixels = window.pixels().expect("the window is drawn on the device");
    let (left, top) = (bounds.origin.x.0 as i32 + 20, bounds.origin.y.0 as i32 + 20);
    let ground = pixels.rgba(left, top);
    let (width, height) = (bounds.size.width.0 as i32 - 40, bounds.size.height.0 as i32 - 40);
    let mut differing = 0;
    let mut total = 0;
    for y in (top..top + height).step_by(4) {
        for x in (left..left + width).step_by(4) {
            total += 1;
            let pixel = pixels.rgba(x, y);
            if pixel.iter().zip(ground).any(|(got, was)| got.abs_diff(was) > 12) {
                differing += 1;
            }
        }
    }
    differing as f32 / total.max(1) as f32
}

fn shot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"))
}

#[test]
fn a_real_ship_renders_in_dark_and_light() {
    let Some(ship) = real_ship() else {
        eprintln!("skipped: WT_WOWS_DIR names no game install");
        return;
    };
    for (appearance, name) in [(Appearance::Dark, "armor-viewport-dark"), (Appearance::Light, "armor-viewport-light")] {
        let Some((mut window, state)) = open(appearance) else { return };
        show_ship(&mut window, state, ship);

        let share = drawn_share(&mut window);
        assert!(share > 0.05, "{name}: the ship covers {share} of the viewport");
        assert!(
            window.by_test_id("armor-viewport-message").is_none(),
            "{name}: no message stands in the scene's place"
        );
        let path = shot_path(name);
        window.save_png(&path).expect("the window is written to a PNG");
        println!("{}", path.display());
    }
}

#[test]
fn a_screenshot_is_the_scene_at_the_viewport_size() {
    let Some(ship) = real_ship() else {
        eprintln!("skipped: WT_WOWS_DIR names no game install");
        return;
    };
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_ship(&mut window, state, ship);

    // Polled here between frames: the frames are what answer it.
    let mut screenshot = Box::pin(state.screenshot());
    let waker = futures::task::noop_waker();
    let mut poll = std::task::Context::from_waker(&waker);
    let started = std::time::Instant::now();
    let png = loop {
        if let std::task::Poll::Ready(result) = screenshot.as_mut().poll(&mut poll) {
            break result.expect("the screenshot is taken");
        }
        assert!(started.elapsed() < FIRST_FRAME_WAIT, "the screenshot lands");
        window.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(20));
    };
    let image = image::load_from_memory(&png).expect("the screenshot is a PNG");
    let (width, height) = state.read_untracked(|model| model.viewport_size()).expect("the state is alive");
    assert_eq!((image.width() as f32, image.height() as f32), (width.round(), height.round()));
}

#[test]
fn dragging_the_viewport_turns_the_camera() {
    let Some(ship) = real_ship() else {
        eprintln!("skipped: WT_WOWS_DIR names no game install");
        return;
    };
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_ship(&mut window, state, ship);
    let before = state.camera();
    let frame_before = window.pixels().expect("the window is drawn");

    let viewport = window.by_test_id("armor-viewport").expect("the viewport is in the document");
    let bounds = window.bounds(viewport).expect("the viewport has a box");
    // Clear of the gizmo in the top right corner.
    let to = Point::new(
        DevicePx(bounds.origin.x.0 + bounds.size.width.0 * 0.3),
        DevicePx(bounds.origin.y.0 + bounds.size.height.0 * 0.6),
    );
    window.drag(viewport, to, 8).expect("the drag is delivered");
    window.advance(Duration::from_millis(100));

    let after = state.camera();
    assert!((after.azimuth - before.azimuth).abs() > 0.05, "the drag orbits: {} to {}", before.azimuth, after.azimuth);
    let frame_after = window.pixels().expect("the window is drawn");
    assert!(frame_before != frame_after, "the scene is drawn again from the new angle");
}

#[test]
fn the_plate_under_the_pointer_is_picked_and_named() {
    let Some(ship) = real_ship() else {
        eprintln!("skipped: WT_WOWS_DIR names no game install");
        return;
    };
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_ship(&mut window, state, ship);

    // The ship is framed in the middle of the viewport, where the deck armour faces the camera.
    let picked = state
        .read_untracked(|model| {
            let rect = model.last_bounds?;
            let hit = model.viewport.pick(rect.center(), rect)?;
            plate_picking::tooltip_for_hit(&hit, &model.mesh_triangle_info).cloned()
        })
        .flatten();
    let plate = picked.expect("a plate is under the centre of the viewport");
    assert!(plate.thickness_mm > 0.0, "the plate has a thickness: {plate:?}");

    let viewport = window.by_test_id("armor-viewport").expect("the viewport is in the document");
    window.hover(viewport).expect("the pointer moves over the viewport");
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("armor-plate-tooltip").is_some(), "the plate's tooltip is shown");
    let hovered = state.read_untracked(|model| model.plate_hover().map(|(tooltip, _)| tooltip.zone)).flatten();
    assert_eq!(hovered.as_deref(), Some(plate.zone.as_str()));
}

/// The real ship in a window 600 by 400 CSS pixels zoomed to [`TIP_ZOOM`], with the viewport at
/// `place`. `None`, with a note, without an install or an adapter.
fn zoomed_ship(place: ViewBox) -> Option<HeadlessWindow> {
    let Some(ship) = real_ship() else {
        eprintln!("skipped: WT_WOWS_DIR names no game install");
        return None;
    };
    let zoom = Zoom::new(TIP_ZOOM).expect("a zoom in range");
    let size = WindowSize { width: 600.0 * TIP_ZOOM, height: 400.0 * TIP_ZOOM };
    let options = HeadlessOptions { zoom, desktop: Some(Appearance::Dark), ..HeadlessOptions::new(size) };
    let (mut window, state) = open_in(options, place)?;
    show_ship(&mut window, state, ship);

    Some(window)
}

/// Moves the pointer to `x`, `y` CSS pixels in the window and returns where it is in device
/// pixels with the plate tooltip's box.
fn plate_tooltip_at(window: &mut HeadlessWindow, x: f32, y: f32) -> (Point<DevicePx, Device>, Rect<DevicePx, Device>) {
    let pointer = Point::new(DevicePx(x * TIP_ZOOM), DevicePx(y * TIP_ZOOM));
    window.hover_at(pointer);
    window.advance(Duration::from_millis(100));
    let tooltip = window.by_test_id("armor-plate-tooltip").expect("a plate's tooltip is shown");

    (pointer, window.bounds(tooltip).expect("the tooltip has a box"))
}

/// Whether `actual` is `expected` device pixels, to the device pixel the tooltip is rounded onto.
fn near(actual: DevicePx, expected: f32) -> bool {
    (actual.0 - expected).abs() <= 0.5
}

/// The tooltip stands one CSS pixel right of and below the pointer and moves with it, in a zoomed
/// window whose viewport does not start at the window's corner.
#[test]
fn the_plate_tooltip_follows_the_pointer_in_an_offset_zoomed_viewport() {
    let Some(mut window) = zoomed_ship(ViewBox { left: 60.0, top: 40.0, width: 300.0, height: 200.0 }) else { return };

    for (x, y) in [(210.0, 140.0), (220.0, 146.0)] {
        let (pointer, tooltip) = plate_tooltip_at(&mut window, x, y);
        assert!(near(tooltip.left(), pointer.x.0 + GAP), "{tooltip:?} starts just right of {pointer:?}");
        assert!(near(tooltip.top(), pointer.y.0 + GAP), "{tooltip:?} starts just below {pointer:?}");
    }
}

/// Where the window leaves no room right of the pointer, the tooltip stands left of it instead,
/// and on neither axis does it cover the pointer or leave the window.
#[test]
fn the_plate_tooltip_crosses_the_pointer_at_the_window_edge() {
    let Some(mut window) = zoomed_ship(ViewBox { left: 300.0, top: 200.0, width: 300.0, height: 200.0 }) else {
        return;
    };

    let (pointer, tooltip) = plate_tooltip_at(&mut window, 450.0, 300.0);
    assert!(tooltip.size.width.0 > 150.0 * TIP_ZOOM, "the tooltip is wider than the room right of the pointer");
    assert!(near(tooltip.right(), pointer.x.0 - GAP), "{tooltip:?} ends just left of {pointer:?}");
    let below = near(tooltip.top(), pointer.y.0 + GAP);
    let above = near(tooltip.bottom(), pointer.y.0 - GAP);
    assert!(below || above, "{tooltip:?} stands just above or below {pointer:?}");
    assert!(tooltip.bottom().0 <= 400.0 * TIP_ZOOM, "{tooltip:?} stays inside the window");
}

/// A device of this machine's own for the model-level tests, or `None` with a note.
fn headless_device() -> Option<HeadlessDevice> {
    match HeadlessDevice::new(SRGB_TARGET) {
        Ok(device) => Some(device),
        Err(error) => {
            eprintln!("skipped: no graphics adapter ({error})");
            None
        }
    }
}

/// `device`'s pipeline as zgui's device generation `generation`.
fn scene_device(device: &HeadlessDevice, generation: u64) -> Rc<ViewportDevice<u64>> {
    let pipeline = GpuPipeline::new(&device.device, &device.queue, SRGB_TARGET);
    Rc::new(ViewportDevice::new(device.device.clone(), device.queue.clone(), generation, pipeline))
}

#[test]
fn a_new_device_generation_uploads_the_scene_again() {
    let Some(first) = headless_device() else { return };
    let size = TargetSize::new(160, 120).expect("a frame has area");
    let mut model = ViewportModel::new();
    model.last_bounds = Some(ViewRect::new(0.0, 0.0, 160.0, 120.0));
    let mut cx = ViewCx::for_test();

    let path = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
    let scene = Arc::new(armored_model_scene(path));
    let generation_one = scene_device(&first, 1);
    model.sync_gpu(DeviceStatus::Ready(Rc::clone(&generation_one)), &mut cx);
    model.show_armor(Arc::clone(&scene), OpeningLayer::Armor, &mut cx);
    let drawn_first = first.render_rgba(&model.viewport, size);
    let ground = drawn_first[..4].to_vec();
    assert!(
        drawn_first.as_chunks::<4>().0.iter().any(|pixel| pixel.as_slice() != ground.as_slice()),
        "the model is drawn"
    );

    // zgui retires the device: the registry drops it, and every mesh buffer made on it with it.
    drop(generation_one);
    assert!(model.scene().is_none(), "the viewport holds no retired device");
    model.sync_gpu(DeviceStatus::Waiting, &mut cx);
    assert!(!model.is_gpu_ready());

    let Some(second) = headless_device() else { return };
    let generation_two = scene_device(&second, 2);
    model.sync_gpu(DeviceStatus::Ready(Rc::clone(&generation_two)), &mut cx);
    let held = model.scene().expect("the viewport draws on the new device");
    assert!(held.is_generation(2));
    assert!(Arc::ptr_eq(model.current_armor.as_ref().expect("the scene is kept"), &scene));
    let drawn_second = second.render_rgba(&model.viewport, size);
    assert_eq!(drawn_first, drawn_second, "the scene is uploaded again and drawn the same");
}

#[test]
fn a_scene_handed_over_before_the_device_is_uploaded_when_it_arrives() {
    let Some(device) = headless_device() else { return };
    let mut model = ViewportModel::new();
    model.last_bounds = Some(ViewRect::new(0.0, 0.0, 160.0, 120.0));
    let mut cx = ViewCx::for_test();
    let path = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
    let scene = Arc::new(armored_model_scene(path));

    model.show_armor(Arc::clone(&scene), OpeningLayer::Armor, &mut cx);
    assert!(model.current_armor.is_none(), "nothing is uploaded without a device");
    assert!(model.pending_armor.is_some());

    let generation = scene_device(&device, 7);
    model.sync_gpu(DeviceStatus::Ready(Rc::clone(&generation)), &mut cx);
    assert!(model.pending_armor.is_none());
    assert!(Arc::ptr_eq(model.current_armor.as_ref().expect("the scene is uploaded"), &scene));
    let frame = device.render_rgba(&model.viewport, TargetSize::new(160, 120).expect("a frame has area"));
    let ground = frame[..4].to_vec();
    assert!(frame.as_chunks::<4>().0.iter().any(|pixel| pixel.as_slice() != ground.as_slice()), "the model is drawn");
}

fn turret() -> ModelPath {
    ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path")
}

/// [`armored_model_scene`] with its plate read as 25 mm from a ship that mounts the model.
fn known_thickness_scene() -> LoadedShipArmor {
    let mut scene = armored_model_scene(turret());
    for info in scene.meshes.iter_mut().flat_map(|mesh| &mut mesh.triangle_info) {
        info.thickness_mm = 25.0;
    }
    if let SceneSubject::Model(model) = &mut scene.subject {
        model.thickness = ArmorThickness::Ship(ThicknessSource {
            vehicle_param_index: VehicleParamIndex::new("PASC001".to_string()),
            display_name: "Iowa".to_string(),
            hp: HardpointName::new("HP_AGM_1"),
        });
    }

    scene
}

/// Shows `scene` in `state` and runs frames until it is drawn.
fn show_scene(window: &mut HeadlessWindow, state: ArmorViewportState, scene: LoadedShipArmor) {
    state.show_armor(Arc::new(scene), OpeningLayer::Armor);
    settle_until(window, || state.read_untracked(ViewportModel::scene_is_shown).unwrap_or(false));
    // The frame after the upload draws the meshes, and lays out the view picking reads.
    window.advance(Duration::from_millis(100));
}

/// Where the middle of the fixture's one plate is drawn, in window pixels: the viewport fills a
/// window of scale one at its corner.
fn plate_on_screen(state: ArmorViewportState) -> Point<DevicePx, Device> {
    let at = state
        .read_untracked(|model| {
            let rect = model.last_bounds?;
            let centroid = model.viewport.pos_to_world_space(Vec3::new(4.0 / 3.0, 2.0 / 3.0, 2.0));
            model.viewport.camera.project_to_screen(centroid, rect)
        })
        .flatten()
        .expect("the plate's middle is in view");

    Point::new(DevicePx(at.x), DevicePx(at.y))
}

/// A point of the viewport with no plate under it, clear of the gizmo in the top right corner.
fn empty_on_screen(state: ArmorViewportState) -> Point<DevicePx, Device> {
    let at = state
        .read_untracked(|model| {
            let rect = model.last_bounds?;
            [(30.0, 30.0), (30.0, rect.h - 30.0), (rect.w - 30.0, rect.h - 30.0)]
                .into_iter()
                .map(|(x, y)| Vec2::new(x, y))
                .find(|at| model.viewport.pick(*at, rect).is_none())
        })
        .flatten()
        .expect("a corner of the viewport shows no plate");

    Point::new(DevicePx(at.x), DevicePx(at.y))
}

fn cast_hint_text(window: &HeadlessWindow) -> String {
    window.texts_by_class("wt-armor-viewport__cast-hint").concat()
}

/// The hint invites a cast while the armor has no arc: a Shift-click on the plate casts one and
/// takes the hint down, and clearing the arcs puts it back.
#[test]
fn a_shift_click_on_the_hull_casts_and_the_hint_follows_the_arcs() {
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_scene(&mut window, state, known_thickness_scene());
    assert!(window.by_test_id("armor-viewport-cast-hint").is_some(), "an armored scene with no arc invites a cast");
    let said = cast_hint_text(&window);
    assert!(said.contains("Shift+click the hull to add an impact point"), "the hint names the gesture: {said}");
    let bounds = |id: &str| {
        let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
        window.bounds(element).expect("it is laid out")
    };
    assert!(
        bounds("armor-viewport-cast-hint").bottom() <= bounds("armor-length-readout").top(),
        "the hint sits above the length readout"
    );

    let plate = plate_on_screen(state);
    window.click_at_with(plate, Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert_eq!(state.arcs().len(), 1, "a Shift-click on the plate casts an arc");
    assert!(window.by_test_id("armor-viewport-cast-miss").is_none(), "a cast that lands says nothing");
    assert!(window.by_test_id("armor-viewport-cast-hint").is_none(), "the hint goes once an arc exists");

    state.clear_trajectories();
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("armor-viewport-cast-hint").is_some(), "and comes back when the arcs are cleared");
}

/// A Shift-click off the hull says nothing is there, for a few seconds, and casts nothing.
#[test]
fn a_shift_click_off_the_hull_says_why_nothing_was_cast() {
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_scene(&mut window, state, known_thickness_scene());

    window.click_at_with(empty_on_screen(state), Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert!(state.arcs().is_empty());
    assert!(window.by_test_id("armor-viewport-cast-miss").is_some(), "the miss is said");
    assert_eq!(cast_hint_text(&window), t!("ui.armor.cast.miss_nothing_under_pointer"), "in place of the hint");

    window.advance(Duration::from_secs(4));
    assert!(window.by_test_id("armor-viewport-cast-miss").is_none(), "the miss is said for a while only");
    let hint = window.by_test_id("armor-viewport-cast-hint").expect("and the hint returns");
    assert_eq!(window.attribute(hint, "data-miss"), None, "drawn as the hint, not the miss");
}

/// A Shift-click that lands takes down the miss a click before it said, at once.
#[test]
fn a_cast_that_lands_takes_down_the_last_miss() {
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_scene(&mut window, state, known_thickness_scene());
    window.click_at_with(empty_on_screen(state), Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("armor-viewport-cast-miss").is_some());

    window.click_at_with(plate_on_screen(state), Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert_eq!(state.arcs().len(), 1);
    assert!(window.by_test_id("armor-viewport-cast-miss").is_none(), "the miss is taken down with the cast");
}

/// A model no ship gives thickness for cannot be cast at: a Shift-click on its plate says so, and
/// nothing invites a cast there.
#[test]
fn a_shift_click_on_a_plate_of_unknown_thickness_says_so() {
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_scene(&mut window, state, armored_model_scene(turret()));
    assert!(window.by_test_id("armor-viewport-cast-hint").is_none(), "no cast is invited where none can land");

    window.click_at_with(plate_on_screen(state), Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert!(state.arcs().is_empty());
    assert_eq!(cast_hint_text(&window), t!("ui.armor.cast.miss_thickness_unknown"));
}

/// Holding Shift over a plate shows the crosshair, and only there.
#[test]
fn holding_shift_over_a_plate_shows_the_crosshair() {
    let Some((mut window, state)) = open(Appearance::Dark) else { return };
    show_scene(&mut window, state, known_thickness_scene());
    // A press focuses the viewport, which the keys go to; the middle button only pans.
    let viewport = window.by_test_id("armor-viewport").expect("the viewport is in the document");
    window.middle_click(viewport).expect("the viewport takes a press");

    window.hover_at(plate_on_screen(state));
    window.advance(Duration::from_millis(100));
    assert_ne!(window.cursor(), Some(CursorStyle::Crosshair), "not without Shift");
    window.hold_key(NamedKey::Shift, Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert_eq!(window.cursor(), Some(CursorStyle::Crosshair), "Shift over the plate aims");
    window.release_key(NamedKey::Shift, Modifiers::NONE);
    window.advance(Duration::from_millis(100));
    assert_ne!(window.cursor(), Some(CursorStyle::Crosshair), "letting go of Shift stops aiming");

    window.hover_at(empty_on_screen(state));
    window.hold_key(NamedKey::Shift, Modifiers::SHIFT);
    window.advance(Duration::from_millis(100));
    assert_ne!(window.cursor(), Some(CursorStyle::Crosshair), "off the hull there is nothing to aim at");
}

/// What [`LogHarness`] hands the test: the viewport and the incoming fire log beside it.
type LogHandles = Rc<Cell<Option<(ArmorViewportState, RwSignal<IncomingFire, LocalStorage>)>>>;

/// The viewport beside the incoming fire log, as the options rail holds it, handing both out.
#[component]
fn LogHarness(handed: LogHandles) -> impl IntoView {
    Services::provide(None);
    let state = ArmorViewportState::new();
    let incoming = RwSignal::new_local(IncomingFire::default());
    handed.set(Some((state, incoming)));
    let choice = Signal::stored_local(ThemeChoice::Fixed(Scheme::Dark));

    view! {
        ThemeRoot(choice = choice) {
            SemanticTokens(choice = Signal::stored_local(wows_toolkit_viewmodel::settings::ThemeChoice::Dark))
            row(style = "width: 100%; height: 100%; background-color: var(--lui-color-panel)") {
                box(style = "flex: 1; display: flex") {
                    ArmorViewport(state = state)
                }
                column(style = "width: 360px; padding: 8px") {
                    IncomingLog(incoming = incoming, playback_at = Signal::stored_local(None), viewport = state, on_seek = None)
                }
            }
        }
    }
}

/// The plated fixture in a viewport beside the incoming fire log, holding `shots` shells of one
/// salvo landed on its plate. `None` with a note when this machine has no adapter.
fn open_log(shots: usize) -> Option<(HeadlessWindow, ArmorViewportState)> {
    let handed = Rc::new(Cell::new(None));
    let given = Rc::clone(&handed);
    let options = HeadlessOptions { drawing: Drawing::Device, ..HeadlessOptions::new(VIEW) };
    let opened = HeadlessWindow::open(options, move || {
        let handed = Rc::clone(&given);
        view! { LogHarness(handed = handed) }
    });
    let mut window = match opened {
        Ok(window) => window,
        Err(HeadlessError::NoWindow | HeadlessError::App { source: zgui::runtime::AppError::GpuUnavailable(_) }) => {
            eprintln!("skipped: no graphics adapter to draw the viewport on");
            return None;
        }
        Err(error) => panic!("the window opens: {error}"),
    };
    let (state, incoming) = handed.take().expect("the harness hands its state out");

    show_scene(&mut window, state, plated_scene());
    let center = state
        .read_untracked(|model| model.current_armor.as_ref().map(|armor| armor.center()))
        .flatten()
        .expect("the scene is uploaded");
    let fire = salvo_on_plate(center, shots);
    state.compare_incoming(&fire.context.taken, &fire.context.shells);
    incoming.set(fire);
    window.advance(Duration::from_millis(100));

    Some((window, state))
}

/// How many shell paths the overlay draws.
fn paths_drawn(state: ArmorViewportState) -> usize {
    state.read_untracked(|model| model.overlay_scene(std::time::Instant::now()).arcs.len()).unwrap_or_default()
}

fn press(window: &mut HeadlessWindow, id: &str) {
    let found = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
    window.click(found).expect("it takes a click");
    window.advance(Duration::from_millis(100));
}

/// A press on a shell in the log casts it back through the hull: its path is drawn and the plates
/// it struck are listed. A second press puts both away.
#[test]
fn a_shell_picked_in_the_log_lists_its_plates_and_draws_its_path() {
    let Some((mut window, state)) = open_log(1) else { return };
    assert_eq!(paths_drawn(state), 0, "nothing is drawn before a pick");
    assert!(window.by_test_id("armor-incoming-detail").is_none());

    press(&mut window, "armor-incoming-shell-0-0");
    assert!(window.by_test_id("armor-incoming-detail").is_some(), "the picked shell's detail is shown");
    assert_eq!(window.texts_by_class("wt-armor-plates__thickness"), ["25 mm"], "with the plate it struck");
    assert!(window.by_test_id("armor-incoming-arrival").is_some(), "and how it arrived");
    assert_eq!(paths_drawn(state), 1, "its path is drawn over the hull");
    let row = window.by_test_id("armor-incoming-shell-0-0").expect("the row stays");
    assert_eq!(window.a11y(row).and_then(|row| row.selected), Some(true), "the row reads as picked");

    press(&mut window, "armor-incoming-shell-0-0");
    assert!(window.by_test_id("armor-incoming-detail").is_none(), "a second press puts the detail away");
    assert_eq!(paths_drawn(state), 0, "and the path");
}

/// A salvo opens to its shells when pressed, and every shell of it is drawn; a second press folds
/// it and puts the paths away.
#[test]
fn a_salvo_picked_in_the_log_opens_and_draws_every_shell() {
    let Some((mut window, state)) = open_log(2) else { return };
    assert!(window.by_test_id("armor-incoming-shell-0-0").is_none(), "a salvo of several shells opens folded");

    press(&mut window, "armor-incoming-salvo-0");
    assert!(window.by_test_id("armor-incoming-shell-0-1").is_some(), "the press opens the salvo");
    assert_eq!(paths_drawn(state), 2, "and draws each of its shells");
    assert!(window.by_test_id("armor-incoming-detail").is_none(), "a salvo has no single shell to detail");

    press(&mut window, "armor-incoming-shell-0-1");
    assert_eq!(paths_drawn(state), 1, "a shell picked inside it is drawn alone");
    assert!(window.by_test_id("armor-incoming-detail").is_some());

    press(&mut window, "armor-incoming-salvo-0");
    press(&mut window, "armor-incoming-salvo-0");
    assert_eq!(paths_drawn(state), 0, "pressing the picked salvo puts it down");
    assert!(window.by_test_id("armor-incoming-shell-0-0").is_none(), "and folds it");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the cast hint"]
fn the_cast_hint_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let Some((mut window, state)) = open(appearance) else { return };
        show_scene(&mut window, state, known_thickness_scene());
        assert!(window.by_test_id("armor-viewport-cast-hint").is_some());
        let path = shot_path(&format!("armor-viewport-cast-hint-{name}"));
        window.save_png(&path).expect("the window is written to a PNG");
        println!("{}", path.display());

        window.click_at_with(empty_on_screen(state), Modifiers::SHIFT);
        window.advance(Duration::from_millis(100));
        assert!(window.by_test_id("armor-viewport-cast-miss").is_some());
        let path = shot_path(&format!("armor-viewport-cast-miss-{name}"));
        window.save_png(&path).expect("the window is written to a PNG");
        println!("{}", path.display());
    }
}

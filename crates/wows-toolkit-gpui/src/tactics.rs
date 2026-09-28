//! A tactics board: a map with capture points on it, drawn away from any
//! battle.
//!
//! The egui app calls this the Tactics Board (`replay/minimap_view/tactics.rs`).
//! It is the same map the replay viewport draws, rasterised through the same
//! renderer, with the capture points of one of the ship's own game modes on it
//! rather than a battle's.

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_toolkit_viewmodel::tactics::naming;
use wows_toolkit_viewmodel::tactics::preset;
use wowsunpack::game_types::WorldPos;

use crate::replay_inspector::GameDataCache;

/// A board's walk of the replays recorded layouts nothing had before.
///
/// Raised to the app, which holds the copy a board opened later starts from.
pub struct LayoutsFound(pub CapLayoutDb);

/// One map the board can be set on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapChoice {
    /// The id the cap layouts are keyed by. `None` for a map found in the game's
    /// own art with no layout recorded for it yet, which can still be drawn on
    /// and which a later scan can give an id to.
    pub map_id: Option<u32>,
    /// The space name, such as `spaces/16_OC_bees_to_honey`.
    pub space: String,
    /// What the reader is shown.
    pub label: String,
}

/// One game mode the chosen map has a recorded layout for.
#[derive(Clone, Debug)]
pub struct ModeChoice {
    pub key: CapLayoutKey,
    pub label: String,
}

/// A capture point on the board.
///
/// Editable, unlike the layout it was seeded from: moving a cap and widening
/// its zone is the question the board exists to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardCapPoint {
    /// Its place in the alphabet: A, B, C.
    pub index: usize,
    /// Where it sits, in the world's own coordinates.
    pub world_x: f32,
    pub world_z: f32,
    /// How wide the zone is, in world units.
    pub radius: f32,
    /// Which team holds it at the start. `None` for a neutral one.
    pub team: Option<wows_replays::types::TeamId>,
    /// Whether it came from a recorded layout rather than from the reader.
    ///
    /// A frozen cap is where the game put it, so it is not dragged or deleted:
    /// the egui board holds its own the same way.
    pub frozen: bool,
}

impl BoardCapPoint {
    fn from_layout(point: &wows_replay_insights::cap_layout::CapPointLayout) -> Self {
        Self {
            index: point.index,
            world_x: point.position.x,
            world_z: point.position.z,
            radius: point.radius.value(),
            // The layout states a neutral cap as a negative team, which is an
            // absence rather than a team.
            team: (point.team_id >= 0).then(|| wows_replays::types::TeamId::new(point.team_id)),
            frozen: true,
        }
    }

    fn from_preset(saved: &preset::PresetCapPoint) -> Self {
        Self {
            index: saved.index,
            world_x: saved.world_x,
            world_z: saved.world_z,
            radius: saved.radius,
            team: (saved.team_id >= 0).then(|| wows_replays::types::TeamId::new(saved.team_id)),
            frozen: saved.frozen,
        }
    }

    fn to_preset(&self) -> preset::PresetCapPoint {
        preset::PresetCapPoint {
            index: self.index,
            world_x: self.world_x,
            world_z: self.world_z,
            radius: self.radius,
            // A cap nobody holds is stated as a negative team, which is the
            // form the egui board writes and reads.
            team_id: self.team.map(|team| team.raw()).unwrap_or(-1),
            frozen: self.frozen,
        }
    }

    /// The letter this cap is labelled with.
    fn letter(&self) -> String {
        char::from(b'A' + (self.index.min(25) as u8)).to_string()
    }

    /// What the renderer draws it as.
    fn command(&self, map: &wows_minimap_renderer::MapInfo) -> DrawCommand {
        let at = map.world_to_minimap(WorldPos::new(self.world_x, 0.0, self.world_z), MINIMAP_SIZE);
        DrawCommand::CapturePoint {
            pos: MinimapPos { x: at.x, y: at.y },
            radius: map.world_distance_to_minimap(self.radius, MINIMAP_SIZE).round() as i32,
            color: team_color(self.team),
            alpha: CAP_FILL_ALPHA,
            label: self.letter(),
            progress: 0.0,
            invader_color: None,
            // No flag: a board's capture points are the reader's to place, and
            // a base's flag belongs to the battle that had one.
            flag_icon: None,
        }
    }
}

/// How solid a capture zone's fill is drawn, as the battle draws its own.
const CAP_FILL_ALPHA: f32 = 0.25;

/// What a team's capture zone is coloured: the reader's own team, the enemy, and
/// one nobody holds.
///
/// The same two the renderer paints a friendly and an enemy marker
/// (`wt_collab_client::geometry`), so one side reads as one colour whether it is
/// a zone or a ship.
const FRIENDLY_COLOR: [u8; 3] = [76, 232, 170];
const ENEMY_COLOR: [u8; 3] = [254, 77, 42];
const NEUTRAL_COLOR: [u8; 3] = [255, 255, 255];

/// The board reads team zero as the reader's own, which is the team a replay
/// records the recording player on.
fn team_color(team: Option<wows_replays::types::TeamId>) -> [u8; 3] {
    match team.map(|team| team.raw()) {
        None => NEUTRAL_COLOR,
        Some(0) => FRIENDLY_COLOR,
        Some(_) => ENEMY_COLOR,
    }
}

/// [`RESIZE_BAND_PX`] in world units, at this map's scale.
///
/// A zone's edge is a line on screen; the press that grabs it is measured in the
/// world, so the tolerance has to cross over.
fn band_in_world(map: &wows_minimap_renderer::MapInfo) -> f32 {
    let per_unit = map.world_distance_to_minimap(1.0, MINIMAP_SIZE);
    if per_unit > f32::EPSILON { RESIZE_BAND_PX / per_unit } else { RESIZE_BAND_PX }
}

/// Every map the board can be set on.
///
/// The recorded layouts first, then whatever else the loaded build ships art
/// for: a map nobody has a replay of can still be drawn on, it simply starts
/// with no capture points.
pub fn maps(layouts: &CapLayoutDb, game_data: Option<&GameDataCache>) -> Vec<MapChoice> {
    let metadata = game_data.and_then(|data| data.newest_loaded()).map(|loaded| loaded.provider().clone());
    let metadata = metadata.as_deref();

    let mut maps: Vec<MapChoice> = layouts
        .maps()
        .into_iter()
        .map(|(map_id, space)| {
            let label = naming::map_label(&space, metadata);
            MapChoice { map_id: Some(map_id), space, label }
        })
        .collect();

    if let Some(loaded) = game_data.and_then(|data| data.newest_loaded()) {
        for space in drawable_spaces(loaded.vfs()) {
            if maps.iter().any(|map| map.space == space) {
                continue;
            }
            let label = naming::map_label(&space, metadata);
            maps.push(MapChoice { map_id: None, space, label });
        }
    }

    maps.sort_by(|a, b| a.label.cmp(&b.label));
    maps
}

/// The spaces the build ships a minimap for.
///
/// A dock scene is not a map anyone plays on, and a space with no minimap art
/// has nothing to draw, so neither is offered.
fn drawable_spaces(vfs: &wowsunpack::vfs::VfsPath) -> Vec<String> {
    let Ok(spaces) = vfs.join("spaces") else { return Vec::new() };
    let Ok(entries) = spaces.read_dir() else { return Vec::new() };
    entries
        .filter(|entry| entry.is_dir().unwrap_or(false))
        .filter_map(|entry| {
            let name = entry.filename();
            if name.starts_with("Dock") {
                return None;
            }
            let has_art = ["minimap.png", "minimap_water.png"]
                .into_iter()
                .any(|art| entry.join(art).map(|path| path.exists().unwrap_or(false)).unwrap_or(false));
            has_art.then(|| format!("spaces/{name}"))
        })
        .collect()
}

/// The modes a map has recorded layouts for, named apart where two read alike.
///
/// A map with no id has no recorded layout and so no modes, which is what a scan
/// can change.
pub fn modes(layouts: &CapLayoutDb, map_id: Option<u32>, game_data: Option<&GameDataCache>) -> Vec<ModeChoice> {
    let Some(map_id) = map_id else { return Vec::new() };
    let metadata = game_data.and_then(|data| data.newest_loaded()).map(|loaded| loaded.provider().clone());
    let found: Vec<CapLayout> = layouts.modes_for_map(map_id).into_iter().cloned().collect();
    let labels = naming::mode_labels(&found, metadata.as_deref());
    found.into_iter().zip(labels).map(|(layout, label)| ModeChoice { key: layout.key.clone(), label }).collect()
}

/// The capture points a mode puts on the map.
pub fn caps_of(layouts: &CapLayoutDb, key: &CapLayoutKey) -> Vec<BoardCapPoint> {
    layouts.get(key).map(|layout| layout.points.iter().map(BoardCapPoint::from_layout).collect()).unwrap_or_default()
}

/// What a drag on a capture point is doing to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapDrag {
    /// Picked out, and nothing more: a frozen cap is where the game put it.
    None,
    /// Moving it across the map.
    Move,
    /// Widening or narrowing its zone.
    Resize,
}

/// How near a zone's drawn edge a press has to land to widen it rather than move
/// it, in map pixels. A tolerance rather than a fraction of the radius: the
/// reader is aiming at a line on screen, and that line is the same thickness
/// whatever the zone is.
const RESIZE_BAND_PX: f32 = 8.0;

/// The smallest a zone can be made, in world units, so one cannot be shrunk to
/// nothing and lost. The egui board holds its own to the same floor.
const MIN_CAP_RADIUS: f32 = 0.5;

/// What a capture point added by hand starts as. A cap circle is about 5 km,
/// which is about 167 world units; the egui board starts one at 150.
const NEW_CAP_RADIUS: f32 = 150.0;

/// What a board draws in until the reader picks otherwise, matching the ink and
/// nib the replay viewport starts with.
const DEFAULT_INK: [u8; 4] = [0xff, 0xd7, 0x3a, 0xff];
const DEFAULT_NIB: f32 = 2.0;

/// The inks the board offers, which are the replay viewport's own.
const INKS: [[u8; 4]; 6] = [
    [0xff, 0xd7, 0x3a, 0xff],
    [0xe8, 0x73, 0x7b, 0xff],
    [0x6f, 0xd9, 0x8a, 0xff],
    [0x7f, 0xb4, 0xe8, 0xff],
    [0xe9, 0xe5, 0xdd, 0xff],
    [0x1a, 0x1a, 0x18, 0xff],
];

/// How wide the nib can be drawn, which is the span the replay viewport holds
/// its own to.
const MIN_NIB: f32 = 1.0;
const MAX_NIB: f32 = 8.0;

/// The tools the board offers, in the order the egui board takes them up.
///
/// The shapes are drawn hollow: a board is read through, and a filled one hides
/// the map it is about.
fn tools() -> Vec<(wt_collab_client::drawing::Tool, &'static str)> {
    use wt_collab_client::drawing::Tool;
    vec![
        (Tool::Freehand, "ui.renderer.annotations.freehand"),
        (Tool::Line, "ui.renderer.annotations.line"),
        (Tool::Arrow, "ui.renderer.annotations.arrow"),
        (Tool::Circle { filled: false }, "ui.renderer.annotations.circle"),
        (Tool::Rectangle { filled: false }, "ui.renderer.annotations.rectangle"),
        (Tool::Triangle { filled: false }, "ui.renderer.annotations.triangle"),
        (Tool::Measurement, "ui.renderer.annotations.measure"),
        (Tool::Eraser, "ui.renderer.annotations.eraser"),
    ]
}

/// Which range circles a placed ship shows.
///
/// Chosen once and carried by every ship placed afterwards: a reader comparing
/// two ships' detection ranges wants the same circles on both.
const RANGE_CIRCLES: [(RangeCircle, &str); 6] = [
    (RangeCircle::Detection, "ui.renderer.context.detection"),
    (RangeCircle::MainBattery, "ui.renderer.context.main_battery"),
    (RangeCircle::SecondaryBattery, "ui.renderer.context.secondary"),
    (RangeCircle::Torpedo, "ui.renderer.context.torpedo"),
    (RangeCircle::Radar, "ui.renderer.context.radar"),
    (RangeCircle::Hydro, "ui.renderer.context.hydro"),
];

/// One of the circles a placed ship can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeCircle {
    Detection,
    MainBattery,
    SecondaryBattery,
    Torpedo,
    Radar,
    Hydro,
}

impl RangeCircle {
    /// Whether this circle is on, in a filter.
    fn is_on(self, filter: &wt_collab_client::types::AnnotationRangeFilter) -> bool {
        match self {
            Self::Detection => filter.detection,
            Self::MainBattery => filter.main_battery,
            Self::SecondaryBattery => filter.secondary_battery,
            Self::Torpedo => filter.torpedo,
            Self::Radar => filter.radar,
            Self::Hydro => filter.hydro,
        }
    }

    /// Turns this circle on or off in a filter.
    fn set(self, filter: &mut wt_collab_client::types::AnnotationRangeFilter, on: bool) {
        match self {
            Self::Detection => filter.detection = on,
            Self::MainBattery => filter.main_battery = on,
            Self::SecondaryBattery => filter.secondary_battery = on,
            Self::Torpedo => filter.torpedo = on,
            Self::Radar => filter.radar = on,
            Self::Hydro => filter.hydro = on,
        }
    }
}

/// A ship waiting to be placed on the board.
#[derive(Clone, Debug)]
pub struct PlacedShip {
    /// The param the ranges are read from.
    param_id: u64,
    name: String,
    species: wowsunpack::game_params::types::Species,
    /// Whether it is on the reader's side, which is what colours the marker.
    friendly: bool,
}

/// How many ship matches the search offers at once.
const SHIP_MATCHES: usize = 10;

/// How far a walk of the replay directory has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub read: usize,
    /// How many there are to read. `None` until the directory has been walked,
    /// which is not the same as a directory holding none.
    pub total: Option<usize>,
}

/// The board itself.
pub struct TacticsBoard {
    focus_handle: FocusHandle,
    game_data: Option<GameDataCache>,
    layouts: CapLayoutDb,
    maps: Vec<MapChoice>,
    map: Option<MapChoice>,
    modes: Vec<ModeChoice>,
    mode: Option<CapLayoutKey>,
    caps: Vec<BoardCapPoint>,
    /// Which capture point the reader is working on, by its place in `caps`.
    selected: Option<usize>,
    /// Where the pointer is on the map, which is what the part-drawn shape is
    /// built against. `None` while it is off the map.
    pointer_at: Option<[f32; 2]>,
    /// The drag in progress, and what it is doing.
    dragging: Option<(usize, CapDrag)>,
    /// Whether the next click on the map places a capture point.
    adding: bool,
    /// The tool in hand and the shape it is part way through, which is the same
    /// state machine the replay viewport draws with.
    drawing: wt_collab_client::drawing::Drawing,
    /// What has been drawn on the board.
    annotations: Vec<wt_collab_client::types::Annotation>,
    /// The window this board is drawn in, remembered so the menu can bring it
    /// forward. `None` until it has been drawn once.
    window: Option<AnyWindowHandle>,
    /// Where the map was last painted, which is what a pointer position is
    /// read against. `None` until it has been painted once. Shared with the
    /// painter, which is the only thing that knows where the map landed.
    painted: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// What a saved board is called, as the reader is typing it.
    preset_name: Entity<gpui_kit::component::input::InputState>,
    /// Every board already saved, read when the window opens and after each
    /// save so the list says what is there.
    presets: Vec<String>,
    /// How far a walk of the replay directory has got, when one is running. It
    /// is what fills the mode picker for a machine with no layouts recorded.
    scanning: Option<ScanProgress>,
    /// Where the replays are, so the walk knows what to read.
    replays_dir: Option<std::path::PathBuf>,
    /// Set when the board goes away, which is what stops a walk part way
    /// through rather than leaving it reading a year of replays for a window
    /// nobody has open.
    scan_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The game version the ranges are read at. `None` before the install has
    /// been read, which draws them at the newest layout this build knows.
    version: Option<wowsunpack::data::Version>,
    /// What is typed into the ship search, and what it matched. Placing a ship
    /// means naming one first: an unnamed marker has no ranges to draw.
    ship_search: Entity<gpui_kit::component::input::InputState>,
    matched_ships: Vec<(wowsunpack::game_params::types::Species, crate::armor_viewer::catalog::ShipEntry)>,
    /// What is typed into the map search, so every map a build ships is
    /// reachable rather than only the first few.
    map_search: Entity<gpui_kit::component::input::InputState>,
    map_search_text: String,
    _map_search_subscription: Subscription,
    /// The ship the next placement is of, and whether it is on the reader's
    /// side. `None` until one is picked, which is what the Ship tool waits for.
    placing: Option<PlacedShip>,
    /// Every ship the build knows, built the first time one is looked up.
    ship_catalog: Option<std::rc::Rc<crate::armor_viewer::catalog::ShipCatalog>>,
    /// Which circles a placed ship shows. Every ship already on the board is
    /// given the same set, so two ships are compared on the same terms.
    range_filter: wt_collab_client::types::AnnotationRangeFilter,
    _ship_search_subscription: Subscription,
    /// The map as it was last rasterised. `None` until one is drawn, which is
    /// what the placeholder stands in for.
    drawn: Option<Arc<RenderImage>>,
    /// Whether the last attempt to draw the map found no art for it, which is
    /// what the board says rather than claiming to still be drawing.
    no_art: bool,
    /// Whether a rasterisation is in flight, so a burst of edits asks for one
    /// redraw rather than one each.
    rasterising: bool,
    /// Whether anything changed while one was in flight.
    stale: bool,
}

impl TacticsBoard {
    pub fn new(
        game_data: Option<GameDataCache>,
        layouts: CapLayoutDb,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let maps = maps(&layouts, game_data.as_ref());
        let preset_name = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder(t!("ui.tactics.preset_name").into_owned())
        });
        let ship_search = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder(t!("ui.renderer.annotations.ship_hint").into_owned())
        });
        let map_search = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx).placeholder(t!("ui.tactics.map_hint").into_owned())
        });
        let map_search_subscription = cx.subscribe(&map_search, |this, state, event, cx| {
            if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                this.map_search_text = state.read(cx).value().to_string();
                cx.notify();
            }
        });
        let ship_search_subscription = cx.subscribe(&ship_search, |this, state, event, cx| {
            if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                let typed = state.read(cx).value().to_string();
                this.matched_ships = this.matching_ships(&typed);
                cx.notify();
            }
        });
        Self {
            preset_name,
            presets: preset::list_preset_names(),
            scanning: None,
            replays_dir: None,
            scan_cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            version: None,
            ship_search,
            matched_ships: Vec::new(),
            map_search,
            map_search_text: String::new(),
            _map_search_subscription: map_search_subscription,
            placing: None,
            ship_catalog: None,
            range_filter: wt_collab_client::types::AnnotationRangeFilter::default(),
            _ship_search_subscription: ship_search_subscription,
            focus_handle: cx.focus_handle(),
            game_data,
            layouts,
            maps,
            map: None,
            modes: Vec::new(),
            mode: None,
            caps: Vec::new(),
            selected: None,
            pointer_at: None,
            dragging: None,
            adding: false,
            drawing: wt_collab_client::drawing::Drawing::new(DEFAULT_INK, DEFAULT_NIB),
            annotations: Vec::new(),
            window: None,
            painted: std::rc::Rc::new(std::cell::Cell::new(None)),
            drawn: None,
            no_art: false,
            rasterising: false,
            stale: false,
        }
    }

    /// Points the board at the replays a scan would read, and at the version
    /// its ranges are read at.
    pub fn set_install(
        &mut self,
        dir: Option<std::path::PathBuf>,
        version: Option<wowsunpack::data::Version>,
        cx: &mut Context<Self>,
    ) {
        self.replays_dir = dir;
        self.version = version;
        cx.notify();
    }

    /// Reads every replay for the capture layouts of the modes they were played
    /// in.
    ///
    /// The layouts are what the mode picker offers, and the only place they can
    /// be read from is a battle that used them. Each is written once: a machine
    /// with a year of replays pays for this walk once and never again.
    fn scan_replays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scanning.is_some() {
            return;
        }
        let Some(dir) = self.replays_dir.clone() else {
            crate::toast::warn(t!("ui.messages.wows_dir_not_set").into_owned(), window, cx);
            return;
        };
        let Some(loaded) = self.game_data.as_ref().and_then(|data| data.newest_loaded()) else {
            crate::toast::warn(t!("ui.tactics.scan_needs_game_data").into_owned(), window, cx);
            return;
        };
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let Some(runtime) = crate::runtime::runtime(cx) else { return };

        let provider = loaded.provider().clone();
        let constants = loaded.base_constants().clone();
        let known = self.layouts.clone();
        self.scan_cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled = std::sync::Arc::clone(&self.scan_cancelled);
        self.scanning = Some(ScanProgress { read: 0, total: None });
        cx.notify();

        // The walk reports itself as it goes rather than being polled: a ticker
        // would keep firing with nothing to say, and in the test executor it
        // would never stop.
        let (reports, mut progress) = futures::channel::mpsc::unbounded();

        cx.spawn_in(window, async move |this, cx| {
            let walk = cx.background_spawn(async move {
                let mut found = known;
                let files = replay_files(&dir);
                let total = files.len();
                let _ = reports.unbounded_send(ScanProgress { read: 0, total: Some(total) });
                let mut fresh = Vec::new();
                for (read, path) in files.iter().enumerate() {
                    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    if let Some(layout) = layout_of(path, provider.as_ref(), &constants, &found) {
                        let kept = layout.clone();
                        if found.insert(layout) {
                            fresh.push(kept);
                        }
                    }
                    let _ = reports.unbounded_send(ScanProgress { read: read + 1, total: Some(total) });
                }
                // Each new layout on its own rather than the whole cache: this
                // walk holds a copy taken when the board opened, and writing all
                // of it would put those rows back over anything written since.
                for layout in &fresh {
                    if let Err(err) = runtime.handle().block_on(CapLayoutDb::save_layout_to_db(&pool, layout)) {
                        tracing::warn!("tactics: a capture layout was not saved: {err}");
                    }
                }
                (found, fresh.len(), total)
            });

            let listen = {
                let this = this.clone();
                let mut cx = cx.clone();
                async move {
                    while let Some(step) = futures::StreamExt::next(&mut progress).await {
                        let open = this.update(&mut cx, |this, cx| {
                            // Only while this walk is the one running: a board
                            // whose walk was replaced is not reporting on it.
                            if this.scanning.is_none() {
                                return false;
                            }
                            this.scanning = Some(step);
                            cx.notify();
                            true
                        });
                        if !matches!(open, Ok(true)) {
                            break;
                        }
                    }
                }
            };

            let (found, ()) = futures::future::join(walk, listen).await;

            let _ = this.update_in(cx, |this, window, cx| {
                let (found, added, total) = found;
                this.scanning = None;
                this.layouts = found;
                // The map list grows with what the walk turned up, and the
                // modes of whichever map the board is on.
                this.maps = maps(&this.layouts, this.game_data.as_ref());
                if let Some(map) = this.map.as_mut() {
                    // A map the walk has just recorded a layout for now has an
                    // id, which is what its modes are looked up by: without
                    // this the scan that found them would show none.
                    if map.map_id.is_none() {
                        map.map_id =
                            this.maps.iter().find(|found| found.space == map.space).and_then(|found| found.map_id);
                    }
                    let map_id = map.map_id;
                    this.modes = modes(&this.layouts, map_id, this.game_data.as_ref());
                }
                crate::toast::info(t!("ui.tactics.scan_done", added = added, total = total).into_owned(), window, cx);
                // The app holds the copy a later board starts from, which would
                // otherwise not know what this walk turned up.
                if added > 0 {
                    cx.emit(LayoutsFound(this.layouts.clone()));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves the board under the name in the field.
    ///
    /// A board with no map is not a board: it names nothing to open again.
    fn save_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.preset_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            crate::toast::warn(t!("ui.tactics.preset_needs_a_name").into_owned(), window, cx);
            return;
        }
        let Some(map) = self.map.clone() else {
            crate::toast::warn(t!("ui.tactics.preset_needs_a_map").into_owned(), window, cx);
            return;
        };

        let saved = preset::TacticsPreset {
            name: name.clone(),
            map_name: map.space,
            // The file states a map with no recorded layout as the id zero,
            // which is the form the egui board writes and reads.
            map_id: map.map_id.unwrap_or(0),
            cap_points: self.caps.iter().map(BoardCapPoint::to_preset).collect(),
            annotations: self.annotations.iter().map(preset::PresetAnnotation::from_annotation).collect(),
        };
        match preset::save_preset(&saved) {
            Ok(()) => {
                self.presets = preset::list_preset_names();
                crate::toast::info(t!("ui.tactics.preset_saved", name = name).into_owned(), window, cx);
                cx.notify();
            }
            Err(why) => {
                crate::toast::failed(
                    t!("ui.tactics.preset_save_failed", error = why.to_string()).into_owned(),
                    window,
                    cx,
                );
            }
        }
    }

    /// Opens a saved board.
    fn load_preset(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let read = match preset::load_preset(name) {
            Ok(read) => read,
            Err(why) => {
                crate::toast::failed(
                    t!("ui.tactics.preset_load_failed", error = why.to_string()).into_owned(),
                    window,
                    cx,
                );
                return;
            }
        };

        // Matched by space name rather than by map id: a board saved from a map
        // nothing has a layout for carries the id zero, which names no map.
        let metadata =
            self.game_data.as_ref().and_then(|data| data.newest_loaded()).map(|loaded| loaded.provider().clone());
        let map = self.maps.iter().find(|map| map.space == read.map_name).cloned().unwrap_or_else(|| MapChoice {
            map_id: (read.map_id != 0).then_some(read.map_id),
            label: naming::map_label(&read.map_name, metadata.as_deref()),
            space: read.map_name.clone(),
        });
        self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        // The saved capture points stand, whatever mode the map has: they are
        // what the reader put there.
        self.mode = None;
        self.map = Some(map);
        self.caps = read.cap_points.iter().map(BoardCapPoint::from_preset).collect();
        self.annotations = read.annotations.iter().map(preset::PresetAnnotation::to_annotation).collect();
        self.selected = None;
        self.adding = false;
        self.redraw(cx);
    }

    /// Drops a saved board.
    fn delete_preset(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(why) = preset::delete_preset(name) {
            crate::toast::failed(
                t!("ui.tactics.preset_delete_failed", error = why.to_string()).into_owned(),
                window,
                cx,
            );
            return;
        }
        self.presets = preset::list_preset_names();
        cx.notify();
    }

    /// The maps the picker offers: the ones whose names match what has been
    /// typed, or the first few when nothing has been.
    ///
    /// Every map is reachable this way, which a capped list on its own is not.
    fn offered_maps(&self) -> Vec<MapChoice> {
        let typed = self.map_search_text.trim().to_lowercase();
        if typed.is_empty() {
            return self.maps.iter().take(MAPS_SHOWN).cloned().collect();
        }
        self.maps
            .iter()
            .filter(|map| map.label.to_lowercase().contains(&typed) || map.space.to_lowercase().contains(&typed))
            .take(MAPS_SHOWN)
            .cloned()
            .collect()
    }

    /// Where a window position falls on the map, in the map's own pixels.
    ///
    /// `None` before the map has been painted once, and for a position in the
    /// margin beside a map that does not fill its element.
    fn map_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.painted.get()?;
        let drawn = self.drawn.as_ref()?;
        let size = drawn.size(0);
        let (width, height) = (size.width.0 as f32, size.height.0 as f32);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        // Drawn to fit without stretching, so one scale covers both directions.
        let scale = (bounds.size.width.as_f32() / width).min(bounds.size.height.as_f32() / height);
        let left = bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - width * scale) / 2.0;
        let top = bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - height * scale) / 2.0;
        let (x, y) = ((position.x.as_f32() - left) / scale, (position.y.as_f32() - top) / scale);
        (x >= 0.0 && x < width && y >= 0.0 && y < height).then_some((x, y))
    }

    /// The map's own coordinate metadata, which is what turns a map pixel into
    /// a world position and back.
    fn map_info(&self) -> Option<wows_minimap_renderer::MapInfo> {
        let map = self.map.as_ref()?;
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        wows_minimap_renderer::assets::load_map_info(&map.space, loaded.vfs())
    }

    /// Where a window position falls in the world.
    ///
    /// Through the map's own inverse of what [`BoardCapPoint::command`] draws
    /// with, so a press lands on the zone it looks like it lands on.
    fn world_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let (x, y) = self.map_point(position)?;
        let map = self.map_info()?;
        let world = map.minimap_to_world_f32(x, y, MINIMAP_SIZE);
        Some((world.x, world.z))
    }

    /// Which capture point a press lands on, and what it would do to it.
    ///
    /// A press within a few pixels of a zone's drawn edge, inside or out, widens
    /// it; anywhere else inside moves it. The topmost one wins, which is the
    /// last drawn.
    fn cap_under(&self, position: Point<Pixels>) -> Option<(usize, CapDrag)> {
        let (x, z) = self.world_point(position)?;
        let map = self.map_info()?;
        // The band is a screen tolerance, so it is measured where the reader is
        // aiming: converted from pixels into the world at this map's scale.
        let band = band_in_world(&map);
        self.caps.iter().enumerate().rev().find_map(|(index, cap)| {
            let away = ((x - cap.world_x).powi(2) + (z - cap.world_z).powi(2)).sqrt();
            // A frozen cap is still pickable, so its figures can be read; it is
            // simply not dragged.
            if cap.frozen {
                return (away < cap.radius).then_some((index, CapDrag::None));
            }
            if (away - cap.radius).abs() <= band {
                return Some((index, CapDrag::Resize));
            }
            (away < cap.radius).then_some((index, CapDrag::Move))
        })
    }

    /// Whether a capture point is the reader's to move.
    ///
    /// Every one is: a board is a place to ask what a different layout would
    /// look like, and the recorded one is only where that starts.
    pub fn selected_cap(&self) -> Option<&BoardCapPoint> {
        self.selected.and_then(|at| self.caps.get(at))
    }

    /// Whether the next click places a capture point.
    pub fn adding(&self) -> bool {
        self.adding
    }

    /// Turns placing capture points on or off.
    pub fn set_adding(&mut self, adding: bool, cx: &mut Context<Self>) {
        self.adding = adding;
        cx.notify();
    }

    /// Places a capture point where the reader clicked.
    fn add_cap_at(&mut self, world: (f32, f32), cx: &mut Context<Self>) {
        // Lettered past the highest already there, so a cap taken off does not
        // hand its letter to the next one placed.
        let index = self.caps.iter().map(|cap| cap.index + 1).max().unwrap_or(0);
        self.caps.push(BoardCapPoint {
            index,
            world_x: world.0,
            world_z: world.1,
            radius: NEW_CAP_RADIUS,
            team: None,
            frozen: false,
        });
        self.selected = Some(index);
        self.adding = false;
        self.redraw(cx);
    }

    /// Drops the capture point the reader is working on.
    ///
    /// A frozen one stays: it is where the game put it, and the egui board holds
    /// its own the same way.
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.selected else { return };
        if self.caps.get(at).is_none_or(|cap| cap.frozen) {
            return;
        }
        self.selected = None;
        // The letters the others carry stand: a cap is lettered by what it was
        // called, and renumbering would rename the ones that stayed.
        self.caps.remove(at);
        self.redraw(cx);
    }

    /// Hands the selected capture point to the next team round: nobody, the
    /// reader's side, then the other.
    pub fn cycle_selected_team(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.selected else { return };
        let Some(cap) = self.caps.get_mut(at).filter(|cap| !cap.frozen) else { return };
        cap.team = match cap.team.map(|team| team.raw()) {
            None => Some(wows_replays::types::TeamId::new(0)),
            Some(0) => Some(wows_replays::types::TeamId::new(1)),
            Some(_) => None,
        };
        self.redraw(cx);
    }

    /// Takes every capture point off the board.
    pub fn clear_caps(&mut self, cx: &mut Context<Self>) {
        if self.caps.is_empty() {
            return;
        }
        self.caps.clear();
        self.selected = None;
        self.redraw(cx);
    }

    /// Whether a drawing tool is in hand, which is what takes the pointer away
    /// from the capture points.
    fn has_tool(&self) -> bool {
        *self.drawing.tool() != wt_collab_client::drawing::Tool::None
    }

    /// Takes up a tool, or puts it down again when it is already in hand.
    pub fn set_tool(&mut self, tool: wt_collab_client::drawing::Tool, cx: &mut Context<Self>) {
        let putting_down = *self.drawing.tool() == tool;
        let next = if putting_down { wt_collab_client::drawing::Tool::None } else { tool };
        self.drawing.set_tool(next);
        // A tool in hand is not also placing capture points.
        if !putting_down {
            self.adding = false;
            self.selected = None;
        }
        cx.notify();
    }

    /// Every ship the build knows, built the first time one is looked up.
    fn ships(&mut self) -> Option<std::rc::Rc<crate::armor_viewer::catalog::ShipCatalog>> {
        if let Some(catalog) = &self.ship_catalog {
            return Some(std::rc::Rc::clone(catalog));
        }
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        let catalog = std::rc::Rc::new(crate::armor_viewer::catalog::ShipCatalog::build(loaded.provider()));
        self.ship_catalog = Some(std::rc::Rc::clone(&catalog));
        Some(catalog)
    }

    /// The ships whose names match what has been typed.
    ///
    /// Empty until something is typed: every ship at once is not a choice.
    fn matching_ships(
        &mut self,
        typed: &str,
    ) -> Vec<(wowsunpack::game_params::types::Species, crate::armor_viewer::catalog::ShipEntry)> {
        let query = typed.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let Some(catalog) = self.ships() else { return Vec::new() };
        let mut found = Vec::new();
        for nation in &catalog.nations {
            for class in &nation.classes {
                for ship in &class.ships {
                    if ship.search_name.contains(&query) {
                        found.push((class.species, ship.clone()));
                        if found.len() >= SHIP_MATCHES {
                            return found;
                        }
                    }
                }
            }
        }
        found
    }

    /// Takes up the ship the reader picked, so the next click places it.
    fn pick_ship(
        &mut self,
        species: wowsunpack::game_params::types::Species,
        ship: &crate::armor_viewer::catalog::ShipEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use wowsunpack::game_params::types::GameParamProvider as _;

        let Some(loaded) = self.game_data.as_ref().and_then(|data| data.newest_loaded()) else { return };
        let Some(param) = loaded.provider().game_param_by_index(&ship.param_index) else { return };

        let friendly = self.placing.as_ref().map(|placed| placed.friendly).unwrap_or(true);
        self.placing =
            Some(PlacedShip { param_id: param.id().raw(), name: ship.display_name.clone(), species, friendly });
        self.drawing.set_tool(wt_collab_client::drawing::Tool::Ship {
            species: format!("{species:?}"),
            friendly,
            yaw: 0.0,
        });
        self.adding = false;
        self.selected = None;
        self.matched_ships.clear();
        self.ship_search.update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    /// Which side the ship being placed is on.
    pub fn set_placing_friendly(&mut self, friendly: bool, cx: &mut Context<Self>) {
        let Some(placed) = self.placing.as_mut() else { return };
        placed.friendly = friendly;
        let species = format!("{:?}", placed.species);
        self.drawing.set_tool(wt_collab_client::drawing::Tool::Ship { species, friendly, yaw: 0.0 });
        cx.notify();
    }

    /// Names the ship a placement just put down, which is what its ranges are
    /// read from.
    ///
    /// The tool draws a marker of a species; the identity behind it is what the
    /// reader picked, and only this side knows it.
    fn name_placed_ship(&mut self) {
        let Some(placed) = self.placing.clone() else { return };
        let Some(wt_collab_client::types::Annotation::Ship { config, .. }) = self.annotations.last_mut() else {
            return;
        };
        *config = Some(wt_collab_client::types::AnnotationShipConfig {
            param_id: placed.param_id,
            ship_name: placed.name,
            range_filter: self.range_filter.clone(),
            // Stock hull and no modifiers until the reader says otherwise,
            // which is where the egui chooser leaves it.
            ..Default::default()
        });
    }

    /// Turns one circle on or off, for the ships already placed and the ones to
    /// come.
    pub fn set_range_circle(&mut self, circle: RangeCircle, on: bool, cx: &mut Context<Self>) {
        circle.set(&mut self.range_filter, on);
        for annotation in &mut self.annotations {
            let wt_collab_client::types::Annotation::Ship { config: Some(config), .. } = annotation else { continue };
            circle.set(&mut config.range_filter, on);
        }
        self.redraw(cx);
    }

    /// Draws in a different ink from here on. What is already drawn keeps the
    /// ink it was drawn in.
    pub fn set_ink(&mut self, ink: [u8; 4], cx: &mut Context<Self>) {
        self.drawing.set_color(ink);
        cx.notify();
    }

    /// Widens or narrows the nib, within what the board draws with.
    pub fn step_nib(&mut self, by: f32, cx: &mut Context<Self>) {
        let stepped = (self.drawing.width() + by).clamp(MIN_NIB, MAX_NIB);
        self.drawing.set_width(stepped);
        cx.notify();
    }

    /// Takes everything drawn off the board, leaving the capture points.
    pub fn clear_annotations(&mut self, cx: &mut Context<Self>) {
        if self.annotations.is_empty() {
            return;
        }
        self.annotations.clear();
        self.redraw(cx);
    }

    /// Hands a pointer event to the tool and keeps what it drew.
    fn stroke(&mut self, stroke: wt_collab_client::drawing::Stroke, cx: &mut Context<Self>) {
        match self.drawing.handle(stroke, &self.annotations) {
            Some(wt_collab_client::drawing::Drawn::Added(annotation)) => {
                let placed_a_ship = matches!(annotation, wt_collab_client::types::Annotation::Ship { .. });
                self.annotations.push(annotation);
                if placed_a_ship {
                    self.name_placed_ship();
                }
            }
            // The index names a shape that was there when the stroke began, so
            // it is checked rather than trusted.
            Some(wt_collab_client::drawing::Drawn::Erased(index)) if index < self.annotations.len() => {
                self.annotations.remove(index);
            }
            _ => {}
        }
        self.redraw(cx);
    }

    /// The same, for a stroke that only moved the shape being built: the frame
    /// is redrawn by the pointer that moved it, not again here.
    fn stroke_without_redraw(&mut self, stroke: wt_collab_client::drawing::Stroke) {
        if let Some(wt_collab_client::drawing::Drawn::Added(annotation)) =
            self.drawing.handle(stroke, &self.annotations)
        {
            let placed_a_ship = matches!(annotation, wt_collab_client::types::Annotation::Ship { .. });
            self.annotations.push(annotation);
            if placed_a_ship {
                self.name_placed_ship();
            }
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        // A tool in hand takes the drag: a reader drawing a line is not asking
        // to move the capture point under it.
        if self.has_tool() {
            let Some(at) = self.map_point(event.position) else { return };
            self.stroke(wt_collab_client::drawing::Stroke::Began { at: [at.0, at.1] }, cx);
            return;
        }
        if self.adding {
            let Some(world) = self.world_point(event.position) else { return };
            self.add_cap_at(world, cx);
            return;
        }
        match self.cap_under(event.position) {
            Some((index, what)) => {
                self.selected = Some(index);
                self.dragging = Some((index, what));
            }
            // A click past every zone puts the selection down, which is what
            // takes the handles off the map.
            None => self.selected = None,
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // A drag released off the map never reports its release here, so a
        // pointer moving with nothing held has let go of whatever it had.
        if event.pressed_button.is_none() {
            self.release(cx);
        }
        let at = self.map_point(event.position);
        let moved_over = at.map(|(x, y)| [x, y]);
        if self.pointer_at != moved_over {
            self.pointer_at = moved_over;
            // The part-drawn shape follows the pointer, so the frame is only
            // worth redrawing while one is being drawn.
            if self.drawing.is_drawing() {
                self.redraw(cx);
            }
        }

        if self.drawing.is_drawing() {
            let Some(at) = at else { return };
            // Straight lines are not asked for here: the board has no modifier
            // on its pointer yet, and a freehand stroke is what the tool draws
            // without one.
            self.stroke_without_redraw(wt_collab_client::drawing::Stroke::Moved { at: [at.0, at.1], straight: false });
            return;
        }

        let Some((index, what)) = self.dragging else { return };
        let Some((x, z)) = self.world_point(event.position) else { return };
        let Some(cap) = self.caps.get_mut(index) else { return };
        match what {
            CapDrag::None => return,
            CapDrag::Move => {
                cap.world_x = x;
                cap.world_z = z;
            }
            CapDrag::Resize => {
                let away = ((x - cap.world_x).powi(2) + (z - cap.world_z).powi(2)).sqrt();
                cap.radius = away.max(MIN_CAP_RADIUS);
            }
        }
        self.redraw(cx);
    }

    /// Lets go of whatever the pointer had hold of.
    fn release(&mut self, cx: &mut Context<Self>) {
        let held = self.dragging.take().is_some();
        // A shape part way through is abandoned rather than finished somewhere
        // the reader did not put it.
        let drawing = self.drawing.is_drawing();
        if drawing {
            self.drawing.cancel();
        }
        if held || drawing {
            self.redraw(cx);
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.has_tool() {
            let Some(at) = self.map_point(event.position) else { return };
            let stroke = if self.drawing.is_drawing() {
                wt_collab_client::drawing::Stroke::Ended { at: [at.0, at.1] }
            } else {
                // A ship and the eraser are placed and used by a click, which
                // never begins a drag.
                wt_collab_client::drawing::Stroke::Clicked { at: [at.0, at.1] }
            };
            self.stroke(stroke, cx);
            return;
        }
        if self.dragging.take().is_some() {
            cx.notify();
        }
    }

    /// What the window is titled.
    ///
    /// The board rather than the board and its map: a window's title is set when
    /// it opens, which is before any map is chosen, and gpui has no way to
    /// change it afterwards. The map is named in the board's own strip.
    pub fn title() -> String {
        t!("ui.windows.tactics_board").into_owned()
    }

    /// Sets the board on a map, which replaces whatever was on it.
    pub fn set_map(&mut self, map: MapChoice, cx: &mut Context<Self>) {
        if self.map.as_ref() == Some(&map) {
            return;
        }
        self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        // The first mode the map has, so a board opens with capture points on
        // it rather than empty.
        self.mode = self.modes.first().map(|mode| mode.key.clone());
        self.caps = self.mode.as_ref().map(|key| caps_of(&self.layouts, key)).unwrap_or_default();
        self.map = Some(map);
        self.redraw(cx);
    }

    /// Sets the board on one of the chosen map's modes, or takes the mode off.
    ///
    /// Picking the mode already set puts it down, which is the egui board's own
    /// blank entry: an empty map to place capture points on from nothing. The
    /// layout's own caps come back by picking it again.
    pub fn set_mode(&mut self, key: CapLayoutKey, cx: &mut Context<Self>) {
        if self.mode.as_ref() == Some(&key) {
            self.mode = None;
            self.caps.clear();
            self.selected = None;
            self.redraw(cx);
            return;
        }
        self.caps = caps_of(&self.layouts, &key);
        self.selected = None;
        self.mode = Some(key);
        self.redraw(cx);
    }

    /// Rasterises the map with whatever is on it.
    ///
    /// One at a time: the raster costs a map-sized image, and a reader dragging
    /// a capture point would otherwise queue one per frame. A change made while
    /// one is running is drawn as soon as it lands.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        let Some(map) = self.map.clone() else {
            self.drawn = None;
            cx.notify();
            return;
        };
        let Some(game_data) = self.game_data.clone() else {
            // Nothing to draw with, but the toolbar still says what the board
            // holds, so it is redrawn even though the map is not.
            cx.notify();
            return;
        };
        // The version the ranges are read at: a ship's detection and gun ranges
        // are version-gated, so without one no range is drawn rather than one
        // read at a version nobody is playing.
        let version = self.version;
        if self.rasterising {
            self.stale = true;
            return;
        }
        self.rasterising = true;

        let caps = self.caps.clone();
        let mut annotations = self.annotations.clone();
        // The shape under the pointer is drawn the way the finished one will
        // be, so what the reader sees while dragging is what they get.
        if let Some(part_drawn) = self.pointer_at.and_then(|at| self.drawing.in_progress(at)) {
            annotations.push(part_drawn);
        }
        cx.spawn(async move |this, cx| {
            let drawn = cx
                .background_spawn(async move { rasterise(&map.space, &game_data, version, &caps, &annotations) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.rasterising = false;
                this.no_art = drawn.is_none();
                if let Some(drawn) = drawn {
                    this.drawn = Some(drawn);
                }
                cx.notify();
                if std::mem::take(&mut this.stale) {
                    this.redraw(cx);
                }
            });
        })
        .detach();
    }
}

/// Every replay under `dir`, in a stable order.
///
/// Symbolic links are not followed: a directory the reader picked may link back
/// into its own ancestry, which would walk for ever. A subdirectory that cannot
/// be read is skipped rather than losing the rest.
fn replay_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    let mut entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            found.extend(replay_files(&path));
            continue;
        }
        // The battle in progress has no results and no layout to read; it is
        // also being written to while this reads it.
        if path.extension().is_some_and(|ext| ext == "wowsreplay")
            && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
        {
            found.push(path);
        }
    }
    found
}

/// The capture layout `path` was played on, when it is one nothing has yet.
///
/// The replay's own metadata says which layout it used, so a battle on a layout
/// already recorded is not parsed at all: that is the difference between a walk
/// that takes seconds and one that takes an hour.
fn layout_of(
    path: &std::path::Path,
    provider: &wowsunpack::game_params::provider::GameMetadataProvider,
    constants: &wows_replays::game_constants::GameConstants,
    known: &CapLayoutDb,
) -> Option<wows_replay_insights::cap_layout::CapLayout> {
    let replay = wows_replays::ReplayFile::from_file(path).ok()?;
    let key = CapLayoutKey { map_id: replay.meta.mapId, scenario_config_id: replay.meta.scenarioConfigId };
    if known.contains(&key) {
        return None;
    }
    wows_replay_insights::cap_layout::extract_cap_layout_from_replay(path, provider, Some(constants))
}

/// Draws `space` with `caps` on it.
///
/// `None` when the loaded build ships no art for that map, which the board says
/// rather than claiming to still be drawing it.
fn rasterise(
    space: &str,
    game_data: &GameDataCache,
    version: Option<wowsunpack::data::Version>,
    caps: &[BoardCapPoint],
    annotations: &[wt_collab_client::types::Annotation],
) -> Option<Arc<RenderImage>> {
    let loaded = game_data.newest_loaded()?;
    let map = wows_minimap_renderer::assets::load_map_info(space, loaded.vfs())?;
    let mut commands: Vec<DrawCommand> = caps.iter().map(|cap| cap.command(&map)).collect();
    // Under the markers, so a ship is not buried under its own circles.
    commands.extend(ship_ranges(annotations, game_data, version, map.space_size as f32));
    // Over the zones, so a line drawn across one is not buried under it.
    commands.extend(annotations.iter().flat_map(wt_collab_client::geometry::annotation_commands));
    crate::minimap_preview::render_map(space, game_data, &commands)
}

/// The range circles the placed ships show.
///
/// Read from each ship's own params, so what is drawn is what that ship sees
/// and shoots rather than a figure typed in.
fn ship_ranges(
    annotations: &[wt_collab_client::types::Annotation],
    game_data: &GameDataCache,
    version: Option<wowsunpack::data::Version>,
    space_size: f32,
) -> Vec<DrawCommand> {
    use wowsunpack::game_params::types::GameParamProvider;

    // Without a version there is no saying what a ship's ranges are: they are
    // gated on it, and a circle drawn at the wrong one reads as a fact.
    let Some(version) = version else { return Vec::new() };
    let Some(loaded) = game_data.newest_loaded() else { return Vec::new() };
    let provider = loaded.provider();

    let mut circles = Vec::new();
    for annotation in annotations {
        let wt_collab_client::types::Annotation::Ship { config: Some(config), .. } = annotation else { continue };
        let Some(param) = GameParamProvider::game_param_by_id(provider.as_ref(), config.param_id.into()) else {
            continue;
        };
        let Some(vehicle) = param.vehicle() else { continue };
        let hull = (!config.hull_name.is_empty()).then_some(config.hull_name.as_str());
        let ranges = vehicle.resolve_ranges(Some(provider.as_ref()), hull, version);
        circles.extend(wt_collab_client::geometry::ship_range_commands(annotation, &ranges, space_size));
    }
    circles
}

impl EventEmitter<LayoutsFound> for TacticsBoard {}

impl Focusable for TacticsBoard {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TacticsBoard {
    /// The window this board is drawn in.
    pub fn window(&self) -> Option<AnyWindowHandle> {
        self.window
    }
}

impl Drop for TacticsBoard {
    fn drop(&mut self) {
        // A board that has gone is not still reading replays for a window
        // nobody has open.
        self.scan_cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Render for TacticsBoard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Taken at draw time because nothing else knows it, and the menu needs
        // it to bring this board forward.
        self.window = Some(window.window_handle());
        let border = cx.theme().border;
        v_flex().size_full().child(self.render_toolbar(cx)).child(div().h(px(1.)).bg(border)).child(self.render_map(cx))
    }
}

impl TacticsBoard {
    /// The map and mode pickers.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let chosen_map = self.map.clone();
        let chosen_mode = self.mode.clone();

        v_flex()
            .gap_1()
            .p_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.tactics.map").to_string()))
                    .child(
                        gpui_kit::component::input::Input::new(&self.map_search)
                            .id("tactics-map-search")
                            .small()
                            .w(px(140.)),
                    )
                    .child(h_flex().flex_wrap().gap_1().children(self.offered_maps().into_iter().enumerate().map(
                        |(index, map)| {
                            let chosen = chosen_map.as_ref() == Some(&map);
                            crate::ui::selectable(
                                ("tactics-map", index),
                                chosen,
                                Button::new(("tactics-map-button", index))
                                    .label(map.label.clone())
                                    .compact()
                                    .selected(chosen)
                                    .on_click({
                                        let board = board.clone();
                                        move |_event, _window, cx: &mut App| {
                                            let map = map.clone();
                                            board.update(cx, |board, cx| board.set_map(map, cx));
                                        }
                                    }),
                            )
                        },
                    ))),
            )
            .child(self.render_cap_tools(cx))
            .child(self.render_draw_tools(cx))
            .child(self.render_ship_picker(cx))
            .child(self.render_range_circles(cx))
            .child(self.render_presets(cx))
            .child(self.render_scan(cx))
            .when(!self.modes.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.tactics.mode").to_string()),
                        )
                        .child(h_flex().flex_wrap().gap_1().children(self.modes.iter().cloned().enumerate().map(
                            |(index, mode)| {
                                let chosen = chosen_mode.as_ref() == Some(&mode.key);
                                crate::ui::selectable(
                                    ("tactics-mode", index),
                                    chosen,
                                    Button::new(("tactics-mode-button", index))
                                        .label(mode.label.clone())
                                        .compact()
                                        .selected(chosen)
                                        .on_click({
                                            let board = board.clone();
                                            move |_event, _window, cx: &mut App| {
                                                let key = mode.key.clone();
                                                board.update(cx, |board, cx| board.set_mode(key, cx));
                                            }
                                        }),
                                )
                            },
                        ))),
                )
            })
    }

    /// Reading the replays for the modes they were played in.
    fn render_scan(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        match self.scanning {
            Some(progress) => h_flex()
                .gap_2()
                .items_center()
                .child(div().text_xs().text_color(crate::theme::text_dim()).child(match progress.total {
                    Some(total) => format!("{} / {total}", progress.read),
                    None => t!("ui.tactics.scan_running").into_owned(),
                }))
                .into_any_element(),
            None => Button::new("tactics-scan")
                .label(t!("ui.tactics.scan_replays").into_owned())
                .compact()
                .tooltip(t!("ui.tactics.scan_replays_tooltip").to_string())
                .on_click(move |_event, window, cx: &mut App| {
                    board.update(cx, |board, cx| board.scan_replays(window, cx));
                })
                .into_any_element(),
        }
    }

    /// What can be drawn on the board, and in what.
    fn render_draw_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let in_hand = self.drawing.tool().clone();
        let nib = self.drawing.width();
        let has_drawing = !self.annotations.is_empty();

        h_flex()
            .gap_1()
            .flex_wrap()
            .items_center()
            .children(tools().into_iter().enumerate().map(|(index, (tool, key))| {
                let board = board.clone();
                let chosen = in_hand == tool;
                crate::ui::selectable(
                    ("tactics-tool", index),
                    chosen,
                    Button::new(("tactics-tool-button", index))
                        .label(t!(key).into_owned())
                        .compact()
                        .selected(chosen)
                        .on_click(move |_event, _window, cx: &mut App| {
                            let tool = tool.clone();
                            board.update(cx, |board, cx| board.set_tool(tool, cx));
                        }),
                )
            }))
            .children(INKS.iter().enumerate().map(|(index, ink)| {
                let board = board.clone();
                let ink = *ink;
                let chosen = self.drawing.color() == ink;
                crate::ui::selectable(
                    ("tactics-ink", index),
                    chosen,
                    Button::new(("tactics-ink-button", index))
                        .compact()
                        .selected(chosen)
                        .child(div().size_3().rounded_full().bg(rgb(u32::from_be_bytes([0, ink[0], ink[1], ink[2]]))))
                        .on_click(move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.set_ink(ink, cx));
                        }),
                )
            }))
            .child({
                let board = board.clone();
                Button::new("tactics-nib-down").label("-").compact().disabled(nib <= MIN_NIB).on_click(
                    move |_event, _window, cx: &mut App| {
                        board.update(cx, |board, cx| board.step_nib(-1.0, cx));
                    },
                )
            })
            .child(div().text_xs().text_color(crate::theme::text_dim()).child(format!("{nib:.0}")))
            .child({
                let board = board.clone();
                Button::new("tactics-nib-up").label("+").compact().disabled(nib >= MAX_NIB).on_click(
                    move |_event, _window, cx: &mut App| {
                        board.update(cx, |board, cx| board.step_nib(1.0, cx));
                    },
                )
            })
            .when(has_drawing, |this| {
                this.child({
                    let board = board.clone();
                    Button::new("tactics-clear-drawing")
                        .label(t!("ui.tactics.clear_drawing").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.clear_annotations(cx));
                        })
                })
            })
    }

    /// Which range circles the placed ships show.
    fn render_range_circles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let filter = self.range_filter.clone();

        h_flex()
            .gap_1()
            .flex_wrap()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.renderer.context.ranges").to_string()),
            )
            .children(RANGE_CIRCLES.into_iter().enumerate().map(|(index, (circle, key))| {
                let board = board.clone();
                let on = circle.is_on(&filter);
                gpui_kit::component::checkbox::Checkbox::new(("tactics-range", index))
                    .label(t!(key).into_owned())
                    .checked(on)
                    .on_click(move |checked, _window, cx: &mut App| {
                        let checked = *checked;
                        board.update(cx, |board, cx| board.set_range_circle(circle, checked, cx));
                    })
            }))
    }

    /// Picking a ship to place, and which side it is on.
    fn render_ship_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let placing = self.placing.clone();

        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::input::Input::new(&self.ship_search)
                            .id("tactics-ship-search")
                            .small()
                            .w(px(180.)),
                    )
                    .when_some(placing, |this, placed| {
                        let friendly = placed.friendly;
                        this.child(
                            div()
                                .text_xs()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.tactics.placing", ship = placed.name.clone()).into_owned()),
                        )
                        .child({
                            let board = board.clone();
                            Button::new("tactics-ship-side")
                                .label(if friendly {
                                    t!("ui.tactics.side_friendly").into_owned()
                                } else {
                                    t!("ui.tactics.side_enemy").into_owned()
                                })
                                .compact()
                                .on_click(move |_event, _window, cx: &mut App| {
                                    board.update(cx, |board, cx| board.set_placing_friendly(!friendly, cx));
                                })
                        })
                    }),
            )
            .when(!self.matched_ships.is_empty(), |this| {
                this.child(h_flex().flex_wrap().gap_1().children(self.matched_ships.iter().cloned().enumerate().map(
                    |(index, (species, ship))| {
                        let board = board.clone();
                        Button::new(("tactics-ship-match", index)).label(ship.display_name.clone()).compact().on_click(
                            move |_event, window, cx: &mut App| {
                                let ship = ship.clone();
                                board.update(cx, |board, cx| board.pick_ship(species, &ship, window, cx));
                            },
                        )
                    },
                )))
            })
    }

    /// Saving the board, and opening one that was saved.
    fn render_presets(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();

        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::input::Input::new(&self.preset_name)
                            .id("tactics-preset-name")
                            .small()
                            .w(px(180.)),
                    )
                    .child({
                        let board = board.clone();
                        Button::new("tactics-preset-save")
                            .label(t!("ui.tactics.preset_save").into_owned())
                            .compact()
                            .on_click(move |_event, window, cx: &mut App| {
                                board.update(cx, |board, cx| board.save_preset(window, cx));
                            })
                    }),
            )
            .when(!self.presets.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.tactics.presets").to_string()),
                        )
                        .child(h_flex().flex_wrap().gap_1().children(self.presets.iter().cloned().enumerate().map(
                            |(index, name)| {
                                let opening = board.clone();
                                let dropping = board.clone();
                                h_flex()
                                    .gap_0p5()
                                    .items_center()
                                    .child({
                                        let name = name.clone();
                                        Button::new(("tactics-preset-open", index))
                                            .label(name.clone())
                                            .compact()
                                            .on_click(move |_event, window, cx: &mut App| {
                                                let name = name.clone();
                                                opening.update(cx, |board, cx| board.load_preset(&name, window, cx));
                                            })
                                    })
                                    .child(
                                        Button::new(("tactics-preset-drop", index))
                                            .label(t!("ui.tactics.preset_delete").into_owned())
                                            .compact()
                                            .on_click(move |_event, window, cx: &mut App| {
                                                let name = name.clone();
                                                dropping.update(cx, |board, cx| board.delete_preset(&name, window, cx));
                                            }),
                                    )
                            },
                        ))),
                )
            })
    }

    /// The map itself, or what is standing in its way.
    fn render_map(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(drawn) = self.drawn.clone() else {
            return div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .child(if self.map.is_none() {
                    t!("ui.tactics.pick_a_map").to_string()
                } else if self.no_art {
                    t!("ui.tactics.no_map_art").to_string()
                } else {
                    t!("ui.tactics.drawing").to_string()
                })
                .into_any_element();
        };

        div()
            .id("tactics-map")
            .flex_1()
            .min_h(px(0.))
            .relative()
            .child(img(drawn).size_full().object_fit(ObjectFit::Contain))
            // Where the map was painted, which is what a pointer position is
            // read against. Taken at paint time because nothing else knows it.
            .child(
                canvas(
                    {
                        let painted = std::rc::Rc::clone(&self.painted);
                        move |bounds, _window, _cx| painted.set(Some(bounds))
                    },
                    |_bounds, _state, _window, _cx| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .into_any_element()
    }

    /// What can be done to the capture points on the board.
    fn render_cap_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let adding = self.adding;
        let selected = self.selected_cap().cloned();
        let has_caps = !self.caps.is_empty();

        h_flex()
            .gap_1()
            .items_center()
            .child(crate::ui::selectable(
                "tactics-add-cap",
                adding,
                Button::new("tactics-add-cap-button")
                    .label(t!("ui.tactics.add_cap").into_owned())
                    .compact()
                    .selected(adding)
                    .on_click({
                        let board = board.clone();
                        move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| {
                                let adding = !board.adding();
                                board.set_adding(adding, cx);
                            });
                        }
                    }),
            ))
            .when_some(selected, |this, cap| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.tactics.selected_cap", letter = cap.letter()).into_owned()),
                )
                .child({
                    let board = board.clone();
                    Button::new("tactics-cap-team").label(t!("ui.tactics.cap_team").into_owned()).compact().on_click(
                        move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.cycle_selected_team(cx));
                        },
                    )
                })
                .child({
                    let board = board.clone();
                    Button::new("tactics-cap-delete")
                        .label(t!("ui.tactics.delete_cap").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.remove_selected(cx));
                        })
                })
            })
            .when(has_caps, |this| {
                this.child({
                    let board = board.clone();
                    Button::new("tactics-clear-caps")
                        .label(t!("ui.tactics.clear_caps").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.clear_caps(cx));
                        })
                })
            })
    }
}

/// How many maps the picker offers at once.
///
/// A build ships dozens, and the strip is for reaching one rather than reading
/// them all: the search narrows to what the reader typed, and this caps what is
/// offered before they have typed anything.
const MAPS_SHOWN: usize = 24;

#[cfg(test)]
mod tests {
    // Named rather than glob-imported: this module glob-imports gpui, whose own
    // `test` attribute would otherwise stand in for the one these want.
    use super::BoardCapPoint;
    use super::CapDrag;
    use super::ENEMY_COLOR;
    use super::FRIENDLY_COLOR;
    use super::NEUTRAL_COLOR;
    use super::team_color;

    fn cap(index: usize, x: f32, z: f32, radius: f32) -> BoardCapPoint {
        BoardCapPoint { index, world_x: x, world_z: z, radius, team: None, frozen: false }
    }

    /// A press inside a zone moves it; one near its edge widens it. The band is
    /// measured against that zone's own radius, so a wide zone has a wide band.
    #[test]
    fn a_press_near_the_edge_widens_rather_than_moves() {
        let zone = cap(0, 0.0, 0.0, 1000.0);
        let band = 8.0;
        let what = |away: f32| {
            if (away - zone.radius).abs() <= band {
                return Some(CapDrag::Resize);
            }
            (away < zone.radius).then_some(CapDrag::Move)
        };

        assert_eq!(what(0.0), Some(CapDrag::Move));
        assert_eq!(what(500.0), Some(CapDrag::Move));
        assert_eq!(what(995.0), Some(CapDrag::Resize), "just inside the edge widens it");
        assert_eq!(what(1005.0), Some(CapDrag::Resize), "and so does just outside");
        assert_eq!(what(1100.0), None, "a press well past the zone is not on it");
    }

    /// A cap taken off the board leaves the letters running in order, so what
    /// is left reads A, B rather than A, C.
    #[test]
    fn the_letters_close_up_when_one_is_removed() {
        let mut caps = vec![cap(0, 0.0, 0.0, 100.0), cap(1, 1.0, 1.0, 100.0), cap(2, 2.0, 2.0, 100.0)];
        caps.remove(1);
        for (index, cap) in caps.iter_mut().enumerate() {
            cap.index = index;
        }
        assert_eq!(caps.iter().map(BoardCapPoint::letter).collect::<Vec<_>>(), ["A", "B"]);
    }

    /// A zone reads by the team that holds it, and by nobody's colour when it
    /// is neutral.
    #[test]
    fn a_zone_is_coloured_by_who_holds_it() {
        assert_eq!(team_color(None), NEUTRAL_COLOR);
        assert_eq!(team_color(Some(wows_replays::types::TeamId::new(0))), FRIENDLY_COLOR);
        assert_eq!(team_color(Some(wows_replays::types::TeamId::new(1))), ENEMY_COLOR);
    }

    /// A layout's neutral cap is stated as a negative team, which is an absence
    /// rather than a team to colour by.
    #[test]
    fn a_layouts_negative_team_reads_as_nobody() {
        let point = wows_replay_insights::cap_layout::CapPointLayout {
            index: 0,
            position: wowsunpack::game_types::WorldPos2D { x: 1.0, z: 2.0 },
            radius: wowsunpack::game_params::types::BigWorldDistance::from(600.0),
            cp_type: wowsunpack::game_types::ControlPointType::Control,
            team_id: -1,
            initially_enabled: true,
        };
        assert_eq!(BoardCapPoint::from_layout(&point).team, None);

        let held = wows_replay_insights::cap_layout::CapPointLayout { team_id: 1, ..point };
        assert_eq!(BoardCapPoint::from_layout(&held).team, Some(wows_replays::types::TeamId::new(1)));
    }
}

#[cfg(test)]
mod coordinate_tests {
    // Named rather than glob-imported, for the reason the other test module
    // names its imports.
    use super::BoardCapPoint;
    use super::MINIMAP_SIZE;
    use super::RESIZE_BAND_PX;
    use super::band_in_world;
    use wows_minimap_renderer::MapInfo;

    fn map(space_size: i32) -> MapInfo {
        MapInfo { space_size }
    }

    /// A press has to land on the zone it looks like it lands on: the position a
    /// pointer is read at goes through the inverse of what the zone is drawn
    /// with, at the same output size.
    #[test]
    fn a_map_pixel_round_trips_to_the_world_and_back() {
        let map = map(1200);
        for at in [(0.0, 0.0), (100.0, 40.0), (383.5, 383.5), (767.0, 767.0)] {
            let world = map.minimap_to_world_f32(at.0, at.1, MINIMAP_SIZE);
            let back = map.world_to_minimap(world, MINIMAP_SIZE);
            assert!(
                (back.x - at.0).abs() < 0.01 && (back.y - at.1).abs() < 0.01,
                "{at:?} came back as ({}, {})",
                back.x,
                back.y
            );
        }
    }

    /// A zone drawn at the centre of the map is hit by a press at the centre of
    /// the map, which is the failure a wrong coordinate space produces.
    #[test]
    fn a_press_at_the_centre_lands_on_a_zone_drawn_there() {
        let map = map(1200);
        let centre = map.minimap_to_world_f32(MINIMAP_SIZE as f32 / 2.0, MINIMAP_SIZE as f32 / 2.0, MINIMAP_SIZE);
        let cap =
            BoardCapPoint { index: 0, world_x: centre.x, world_z: centre.z, radius: 150.0, team: None, frozen: false };

        let drawn =
            map.world_to_minimap(wowsunpack::game_types::WorldPos::new(cap.world_x, 0.0, cap.world_z), MINIMAP_SIZE);
        assert!((drawn.x - MINIMAP_SIZE as f32 / 2.0).abs() < 0.01);
        assert!((drawn.y - MINIMAP_SIZE as f32 / 2.0).abs() < 0.01);

        let pressed = map.minimap_to_world_f32(drawn.x, drawn.y, MINIMAP_SIZE);
        let away = ((pressed.x - cap.world_x).powi(2) + (pressed.z - cap.world_z).powi(2)).sqrt();
        assert!(away < 1.0, "the press is {away} units from the zone it was aimed at");
    }

    /// The resize tolerance is a screen distance, so it is the same handful of
    /// pixels on a small map and a large one.
    #[test]
    fn the_resize_band_is_the_same_on_screen_whatever_the_map() {
        for space in [800i32, 1200, 1600] {
            let map = map(space);
            let band = band_in_world(&map);
            let on_screen = map.world_distance_to_minimap(band, MINIMAP_SIZE);
            assert!(
                (on_screen - RESIZE_BAND_PX).abs() < 0.01,
                "a {space}-unit map gives a {on_screen}px band, not {RESIZE_BAND_PX}"
            );
        }
    }
}

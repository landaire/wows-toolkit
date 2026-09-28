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
    /// What a session calls this zone. Its place in the list is not that: a
    /// peer's list is its own, and a zone moved by one has to be the same zone
    /// on every board.
    pub id: crate::collab::CapPointId,
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
            id: crate::collab::CapPointId::fresh(),
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
            id: crate::collab::CapPointId::fresh(),
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

    /// This zone as a session states it.
    fn to_wire(&self) -> wt_collab_client::protocol::WireCapPoint {
        wt_collab_client::protocol::WireCapPoint {
            id: self.id.raw(),
            index: self.index as u32,
            world_x: self.world_x,
            world_z: self.world_z,
            radius: self.radius,
            // A zone nobody holds is stated as a negative team, which is the
            // form the wire and the egui board both use.
            team_id: self.team.map(|team| team.raw()).unwrap_or(-1),
            frozen: self.frozen,
        }
    }

    /// The same, read back.
    fn from_wire(wire: &wt_collab_client::protocol::WireCapPoint) -> Self {
        Self {
            id: crate::collab::CapPointId::new(wire.id),
            index: wire.index as usize,
            world_x: wire.world_x,
            world_z: wire.world_z,
            radius: wire.radius,
            team: (wire.team_id >= 0).then(|| wows_replays::types::TeamId::new(wire.team_id)),
            frozen: wire.frozen,
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

/// How wide a world unit is: the game measures in 30-metre units, so a zone
/// stated in kilometres is that many thirty-metre steps.
const WORLD_UNITS_PER_KM: f32 = 1000.0 / 30.0;

/// How far one press moves a zone's width, in kilometres.
const RADIUS_STEP_KM: f32 = 0.5;

/// A zone's width in the units it is talked about in.
fn radius_km(radius: f32) -> f32 {
    radius / WORLD_UNITS_PER_KM
}

/// The same shape, drawn wider and dimmer: what sits under a picked one to say
/// it is picked.
fn halo(annotation: &wt_collab_client::types::Annotation) -> wt_collab_client::types::Annotation {
    use wt_collab_client::types::Annotation;

    let mut halo = annotation.clone();
    let widened = |width: &mut f32| *width = (*width + HALO_WIDTH).max(HALO_WIDTH);
    let tinted = |color: &mut [u8; 4]| *color = HALO_INK;
    match &mut halo {
        // A ship marker is drawn from its species rather than stroked, so it
        // has no width to widen; it reads as picked by the handles on it.
        Annotation::Ship { .. } => {}
        Annotation::FreehandStroke { color, width, .. }
        | Annotation::Line { color, width, .. }
        | Annotation::Arrow { color, width, .. }
        | Annotation::Measurement { color, width, .. } => {
            tinted(color);
            widened(width);
        }
        Annotation::Circle { color, width, filled, .. }
        | Annotation::Rectangle { color, width, filled, .. }
        | Annotation::Triangle { color, width, filled, .. } => {
            tinted(color);
            widened(width);
            // Hollow, so the shape it is under is still read through it.
            *filled = false;
        }
    }
    halo
}

/// How much wider a picked shape's halo is drawn, and in what.
const HALO_WIDTH: f32 = 4.0;
const HALO_INK: [u8; 4] = [0xff, 0xd7, 0x3a, 0x80];

/// Where the drawn map landed in the element that painted it, and how much it
/// was scaled to fit.
///
/// The frame is drawn to fit without stretching, so it is letterboxed: its top
/// left corner is not the element's own. Held apart from the board so the two
/// directions cannot drift: a handle placed by one and pressed by the other has
/// to agree to the pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Letterbox {
    left: f32,
    top: f32,
    scale: f32,
    /// The frame's own size, which is what a point is held inside.
    frame: (f32, f32),
}

impl Letterbox {
    /// `None` for a frame with no extent, which nothing can be placed against.
    fn fitting(bounds: Bounds<Pixels>, frame: (f32, f32)) -> Option<Self> {
        if frame.0 <= 0.0 || frame.1 <= 0.0 {
            return None;
        }
        let scale = (bounds.size.width.as_f32() / frame.0).min(bounds.size.height.as_f32() / frame.1);
        Some(Self {
            left: bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - frame.0 * scale) / 2.0,
            top: bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - frame.1 * scale) / 2.0,
            scale,
            frame,
        })
    }

    /// An element position as a point in the frame. `None` for one in the
    /// margin beside a map that does not fill its element.
    fn to_frame(self, at: (f32, f32)) -> Option<(f32, f32)> {
        let (x, y) = ((at.0 - self.left) / self.scale, (at.1 - self.top) / self.scale);
        (x >= 0.0 && x < self.frame.0 && y >= 0.0 && y < self.frame.1).then_some((x, y))
    }

    /// A point in the frame as an element position. `None` for one the frame
    /// does not show, which has nowhere on screen to be.
    fn to_element(self, at: (f32, f32)) -> Option<(f32, f32)> {
        (at.0 >= 0.0 && at.0 < self.frame.0 && at.1 >= 0.0 && at.1 < self.frame.1)
            .then_some((self.left + at.0 * self.scale, self.top + at.1 * self.scale))
    }
}

/// How big a turning handle is drawn, how far above the shape it sits, and in
/// what. The egui board's own figures, in screen pixels at any zoom.
const HANDLE_RADIUS: Pixels = px(5.);
const HANDLE_DISTANCE: Pixels = px(25.);
const HANDLE_COLOR: u32 = 0xFFFF64;

/// How near a drawn shape a press has to land to pick it out, in map pixels.
/// A line is one pixel wide and nobody presses one exactly.
const SHAPE_REACH_PX: f32 = 10.0;

/// How much one notch of the wheel changes the zoom.
const ZOOM_PER_NOTCH: f32 = 0.004;

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

/// What is on the board, as an undo step holds it.
#[derive(Clone, Debug, PartialEq)]
struct BoardState {
    caps: Vec<BoardCapPoint>,
    /// With the ids the shapes are known by, so putting a step back is the same
    /// change to a session as it is to a board nobody else can see.
    annotations: Vec<wt_collab_client::drawing::Held>,
}

/// How many changes can be taken back.
///
/// A board is edited in small steps and the whole of what is on it is kept per
/// step, so the stack is held to a depth rather than growing with the session.
const HISTORY_DEPTH: usize = 50;

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
    /// This board's end of a collab session, which also holds what has been
    /// drawn on it when there is no session to hold it.
    collab: crate::collab::CollabLink,
    /// What this board is called in a session. A board is one window of several
    /// a session can be on, and every message about it carries this.
    board_id: crate::collab::BoardId,
    /// The art a peer sent for this board, for a map this build has none of.
    /// `None` for a board drawn from art this build ships.
    peer_art: Option<Arc<image::RgbImage>>,
    /// What that map measures, sent with the art. Without it a world position
    /// cannot be placed on the board at all, and this build has nothing to read
    /// it from for a map it does not ship.
    peer_map_info: Option<wows_minimap_renderer::MapInfo>,
    /// Who opened the board in the session. `None` for one nobody has shared,
    /// which is every board outside a session.
    owner: Option<crate::collab::UserId>,
    /// Which version of the session's capture points and shapes this board has
    /// taken, so a change made by a peer is noticed without comparing lists
    /// every tick.
    adopted_caps: Option<u64>,
    adopted_shapes: Option<u64>,
    /// Whether this board has told the session which map it is on. Announced
    /// once per map rather than per tick.
    announced: bool,
    /// The session's end of this board, held back until the session has been
    /// told the board exists.
    ///
    /// Anything sent for a board the session does not hold is dropped by the
    /// peer task, so the board draws on its own end until the announcement has
    /// gone over and only then starts speaking to the session.
    joining: Option<crate::collab::CollabLink>,
    /// Kept alive while a session is running, to notice what peers change.
    _collab_tick: Option<Task<()>>,
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
    /// Which part of the map is on screen: the whole of it until the reader
    /// zooms in, and then whatever they moved to.
    view: wows_minimap_renderer::viewport::MapViewport,
    /// The pan in progress, and where the pointer was when it last moved.
    panning: Option<Point<Pixels>>,
    /// What the board held before each change, so a change can be taken back.
    /// The map and the mode are not in it: those are what the board is set on
    /// rather than what is on it.
    history: Vec<BoardState>,
    /// What was taken back, so it can be put back again.
    undone: Vec<BoardState>,
    /// The drawn shapes the reader has picked out, which a drag then moves.
    picked: wt_collab_client::drawing::Selection,
    /// A move in progress: where it began, and what was on the board then.
    moving: Option<([f32; 2], Vec<wt_collab_client::types::Annotation>)>,
    /// A turn in progress: which shape, and how it stood before the handle was
    /// taken hold of. Held because a turn is read as a bearing from the shape's
    /// middle each time the pointer moves, not as a step from where it last was.
    turning: Option<(usize, wt_collab_client::types::Annotation)>,
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
            collab: crate::collab::CollabLink::default(),
            board_id: crate::collab::BoardId::fresh(),
            peer_art: None,
            peer_map_info: None,
            owner: None,
            adopted_caps: None,
            adopted_shapes: None,
            announced: false,
            joining: None,
            _collab_tick: None,
            window: None,
            painted: std::rc::Rc::new(std::cell::Cell::new(None)),
            drawn: None,
            no_art: false,
            view: wows_minimap_renderer::viewport::MapViewport::default(),
            panning: None,
            history: Vec::new(),
            undone: Vec::new(),
            picked: wt_collab_client::drawing::Selection::default(),
            moving: None,
            turning: None,
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
            annotations: self.shapes().iter().map(preset::PresetAnnotation::from_annotation).collect(),
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
        let caps = read.cap_points.iter().map(BoardCapPoint::from_preset).collect();
        self.set_caps(caps);
        self.replace_shapes(read.annotations.iter().map(preset::PresetAnnotation::to_annotation).collect());
        self.selected = None;
        self.adding = false;
        self.announce_again(cx);
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
        let (x, y) = self.drawn_point(position)?;
        // The frame shows the part of the map the window names, so a drawn
        // point is turned back into a map one before anything reads it.
        Some(self.view.to_map(x, y))
    }

    /// How the drawn map sits in the element it was painted in.
    ///
    /// `None` before it has been painted once, and for a frame with no extent,
    /// which nothing can be placed against either way.
    fn letterbox(&self) -> Option<Letterbox> {
        let bounds = self.painted.get()?;
        let size = self.drawn.as_ref()?.size(0);
        Letterbox::fitting(bounds, (size.width.0 as f32, size.height.0 as f32))
    }

    /// Where a window position falls in the drawn frame, before the window is
    /// undone. What a zoom about the pointer needs, since it re-anchors on a
    /// drawn point rather than a map one.
    fn drawn_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        self.letterbox()?.to_frame((position.x.as_f32(), position.y.as_f32()))
    }

    /// Where a map point lands in the element.
    ///
    /// The other way round from [`Self::map_point`], for a control that has to
    /// sit over the shape it belongs to. `None` for a point the window has
    /// scrolled off the map, which has nowhere on screen to be.
    fn element_point(&self, at: (f32, f32)) -> Option<(Pixels, Pixels)> {
        let (x, y) = self.letterbox()?.to_element((self.view.x(at.0), self.view.y(at.1)))?;
        Some((px(x), px(y)))
    }

    /// Where a shape's turning handle sits in the element, and where the line to
    /// it starts.
    ///
    /// Above the shape by a fixed number of pixels rather than a map distance,
    /// so the handle keeps its size and its reach at every zoom, as the egui
    /// board's does.
    fn rotation_handle(
        &self,
        annotation: &wt_collab_client::types::Annotation,
    ) -> Option<((Pixels, Pixels), (Pixels, Pixels))> {
        let [left, top, right, _] = wt_collab_client::drawing::annotation_bounds(annotation);
        let anchor = self.element_point(((left + right) / 2.0, top))?;
        Some(((anchor.0, anchor.1 - HANDLE_DISTANCE), anchor))
    }

    /// The shape whose turning handle is under `position`, if the pointer is on
    /// one.
    ///
    /// A handle belongs to a single picked shape that has a bearing at all:
    /// turning several at once about their own middles is not what one handle
    /// means, and a circle looks the same at every angle.
    fn handle_under(&self, position: Point<Pixels>) -> Option<(usize, wt_collab_client::types::Annotation)> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotation_at(index)?;
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let (handle, _) = self.rotation_handle(&annotation)?;
        let reach = HANDLE_RADIUS + px(8.);
        let away = (position.x - handle.0).as_f32().hypot((position.y - handle.1).as_f32());
        (away < reach.as_f32()).then_some((index, annotation))
    }

    /// The handle a picked shape is turned by, drawn over the map.
    ///
    /// An element rather than part of the frame: it is a control rather than
    /// something drawn on the map, so it keeps its size at every zoom and stays
    /// out of a saved preset.
    fn rotation_handle_overlay(&self) -> Option<AnyElement> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotation_at(index)?;
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let (handle, anchor) = self.rotation_handle(&annotation)?;
        Some(
            div()
                .absolute()
                .left(handle.0 - HANDLE_RADIUS)
                .top(handle.1 - HANDLE_RADIUS)
                .child(div().size(HANDLE_RADIUS * 2.0).rounded_full().bg(gpui_kit::rgb(HANDLE_COLOR)))
                // The stem back to the shape, so the handle reads as belonging
                // to it rather than floating over the map.
                .child(
                    div()
                        .absolute()
                        .left(HANDLE_RADIUS)
                        .top(HANDLE_RADIUS)
                        .w(px(1.))
                        .h(anchor.1 - handle.1)
                        .bg(gpui_kit::rgb(HANDLE_COLOR)),
                )
                .into_any_element(),
        )
    }

    /// Zooms the map about the pointer, so what is under it stays there.
    fn zoom_about(&mut self, notches: f32, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(at) = self.drawn_point(position) else { return };
        let zoom = self.view.zoom() * (1.0 + notches * ZOOM_PER_NOTCH);
        let view = self.view.zoomed_about(zoom, at);
        if view == self.view {
            return;
        }
        self.view = view;
        self.redraw(cx);
    }

    /// Puts the whole map back on screen.
    fn reset_view(&mut self, cx: &mut Context<Self>) {
        if self.view == wows_minimap_renderer::viewport::MapViewport::default() {
            return;
        }
        self.view = wows_minimap_renderer::viewport::MapViewport::default();
        self.redraw(cx);
    }

    /// The map's own coordinate metadata, which is what turns a map pixel into
    /// a world position and back.
    fn map_info(&self) -> Option<wows_minimap_renderer::MapInfo> {
        // What a peer sent first: a board shared from a build this one does not
        // have takes its measurements off the wire, because this build has none
        // for that map to read.
        if let Some(sent) = self.peer_map_info.clone() {
            return Some(sent);
        }
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

    /// The drawn shape under the pointer, if it is near enough one.
    ///
    /// A reach rather than a hit test: a line is one pixel wide and nobody can
    /// press one exactly, which is why the shared reader takes a distance.
    fn shape_under(&self, position: Point<Pixels>) -> Option<usize> {
        let at = self.map_point(position)?;
        wt_collab_client::drawing::nearest_within(&self.shapes(), [at.0, at.1], SHAPE_REACH_PX)
    }

    /// Erases the shapes the reader has picked out.
    pub fn erase_picked(&mut self, cx: &mut Context<Self>) {
        if self.picked.picked().is_empty() {
            return;
        }
        self.remember();
        let mut going: Vec<usize> = self.picked.picked().to_vec();
        // Highest first, so an index is not shifted out from under the next.
        going.sort_unstable_by(|a, b| b.cmp(a));
        for at in going {
            self.collab.erase_annotation(at);
        }
        self.picked.clear();
        self.redraw(cx);
    }

    /// Remembers what is on the board, before changing it.
    ///
    /// A change made after something was taken back is the new end of the line,
    /// so what was undone is dropped rather than redone into a board that has
    /// moved on.
    fn remember(&mut self) {
        self.history.push(self.board_state());
        if self.history.len() > HISTORY_DEPTH {
            self.history.remove(0);
        }
        self.undone.clear();
    }

    /// Whether there is a change to take back, and one to put back.
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Takes the last change back.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(was) = self.history.pop() else { return };
        self.undone.push(self.board_state());
        self.adopt(was, cx);
    }

    /// Puts back what was taken.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(again) = self.undone.pop() else { return };
        self.history.push(self.board_state());
        self.adopt(again, cx);
    }

    /// Puts the board back to a step.
    fn adopt(&mut self, state: BoardState, cx: &mut Context<Self>) {
        self.set_caps(state.caps);
        self.collab.restore(&state.annotations);
        // The selection named a place in a list that has just been replaced.
        self.selected = None;
        self.dragging = None;
        self.picked.clear();
        self.redraw(cx);
    }

    /// Hands this board its end of a session, or takes it away again.
    ///
    /// What was drawn stays on the board either way: a session starting under an
    /// open board hears about it, and one ending leaves it where the reader can
    /// still see it.
    pub fn set_collab(&mut self, link: crate::collab::CollabLink, cx: &mut Context<Self>) {
        self.announced = false;
        self.adopted_caps = None;
        self.adopted_shapes = None;
        if !link.is_active() {
            // A session ending. What it held is carried onto this board's own
            // end, where the reader can still see it.
            let carried = self.collab.annotations_held();
            self.joining = None;
            self.collab = crate::collab::CollabLink::default().on_board(self.board_id);
            self.collab.adopt(carried);
            self.follow_collab(cx);
            self.redraw(cx);
            return;
        }
        // Held back until the announcement has gone over, because the session
        // drops anything sent for a board it does not hold yet.
        self.joining = Some(link.on_board(self.board_id));
        self.follow_collab(cx);
        self.announce(cx);
    }

    /// Opens this board on one a peer already has, rather than on a map of this
    /// reader's choosing.
    ///
    /// The art comes with it: the peer who opened the board may be on a build
    /// this one does not have, and a board nobody can draw is not shared.
    pub fn adopt_session_board(&mut self, board: crate::collab::SessionBoard, cx: &mut Context<Self>) {
        self.board_id = board.board_id;
        self.owner = Some(board.owner_user_id);
        self.collab = self.collab.on_board(board.board_id);
        self.map = Some(MapChoice {
            map_id: (board.map.map_id > 0).then_some(board.map.map_id),
            space: board.map.space,
            label: board.map.label,
        });
        self.modes = modes(&self.layouts, self.map.as_ref().and_then(|map| map.map_id), self.game_data.as_ref());
        self.mode = None;
        // Already in the session by definition, so nothing is announced back.
        self.announced = true;
        self.peer_map_info = board.map.info.clone();
        self.peer_art = board.map.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(Arc::new(art.to_rgb8())),
            // Art that will not decode leaves the board drawn from this build's
            // own, which is the state a board with no art sent is in.
            Err(err) => {
                tracing::warn!("tactics board: the art a peer sent could not be read: {err}");
                None
            }
        });
        self.follow_collab(cx);
        self.follow_session(cx);
        self.redraw(cx);
    }

    /// Whether this board is one a peer opened rather than this reader.
    pub fn is_a_peers(&self) -> bool {
        self.owner.is_some_and(|owner| self.collab.my_user_id() != Some(owner))
    }

    /// Tells the session which map this board is on, then hands it everything
    /// already on the board.
    ///
    /// In that order, and through the one channel, because the session drops
    /// anything sent for a board it does not hold. Nothing happens without a map
    /// chosen: there is no board to announce until there is something to draw
    /// on, and what the reader draws meanwhile stays on this end until there is.
    fn announce(&mut self, cx: &mut Context<Self>) {
        if self.announced || self.joining.is_none() {
            return;
        }
        let (Some(map), Some(game_data)) = (self.map.clone(), self.game_data.clone()) else { return };
        self.announced = true;
        cx.spawn(async move |this, cx| {
            let space = map.space.clone();
            let read = game_data.clone();
            let art = cx
                .background_executor()
                .spawn(async move {
                    let loaded = read.newest_loaded()?;
                    let art = wows_minimap_renderer::assets::load_map_image(&space, loaded.vfs());
                    let info = wows_minimap_renderer::assets::load_map_info(&space, loaded.vfs());
                    Some((art.as_ref().and_then(encode_png), info))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(session) = this.joining.take() else { return };
                // Read before the swap: what the board holds is on its own end
                // until now.
                let carried = this.collab.annotations_held();
                let caps: Vec<wt_collab_client::protocol::WireCapPoint> =
                    this.caps.iter().map(BoardCapPoint::to_wire).collect();
                // Both absent where no build is open, which is a board a peer
                // can still draw from the map name if it has that build.
                let (art_png, info) = art.unwrap_or((None, None));
                session.announce_board(crate::collab::BoardMap {
                    space: map.space,
                    label: map.label,
                    // The wire states a map with no recorded layout as the id
                    // zero, which is the form the egui board sends and reads.
                    map_id: map.map_id.unwrap_or(0),
                    art_png,
                    info,
                });
                session.adopt(carried);
                for cap in caps {
                    session.set_cap(cap);
                }
                this.collab = session;
                this.follow_session(cx);
                this.redraw(cx);
            });
        })
        .detach();
    }

    /// Says the board is on a different map than the session was told.
    ///
    /// The announcement carries the map, so changing it means announcing again
    /// under the same board: every peer is drawing the map this one named.
    fn announce_again(&mut self, cx: &mut Context<Self>) {
        if !self.collab.is_active() && self.joining.is_none() {
            return;
        }
        if self.joining.is_none() {
            self.joining = Some(self.collab.clone());
        }
        self.announced = false;
        self.announce(cx);
    }

    /// Notices what peers change, while a session is running.
    fn follow_collab(&mut self, cx: &mut Context<Self>) {
        if !self.collab.is_active() {
            self._collab_tick = None;
            return;
        }
        if self._collab_tick.is_some() {
            return;
        }
        self._collab_tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(COLLAB_TICK).await;
                let running = this
                    .update(cx, |this, cx| {
                        if !this.collab.is_active() && this.joining.is_none() {
                            return false;
                        }
                        this.adopt_a_peers_board(cx);
                        this.follow_session(cx);
                        true
                    })
                    .unwrap_or(false);
                if !running {
                    return;
                }
            }
        }));
    }

    /// Opens this board on a peer's, where the reader has not chosen a map.
    ///
    /// Checked as the session is followed rather than once when it starts: a
    /// joiner is sent the boards the session is on after the handshake, so at
    /// the moment it connects there are none to adopt.
    fn adopt_a_peers_board(&mut self, cx: &mut Context<Self>) {
        if self.map.is_some() {
            return;
        }
        let mine = self.collab.my_user_id();
        let link = self.joining.clone().unwrap_or_else(|| self.collab.clone());
        let Some(board) = link.session_boards().into_iter().find(|board| Some(board.owner_user_id) != mine) else {
            return;
        };
        tracing::info!("tactics board: opening on a board shared in this session");
        self.adopt_session_board(board, cx);
    }

    /// Takes what the session holds that this board has not drawn yet.
    fn follow_session(&mut self, cx: &mut Context<Self>) {
        let Some(versions) = self.collab.board_versions() else { return };
        let mut changed = false;

        if self.adopted_caps != Some(versions.caps) {
            self.adopted_caps = Some(versions.caps);
            let mut caps: Vec<BoardCapPoint> = self.collab.board_caps().iter().map(BoardCapPoint::from_wire).collect();
            // By letter, so the board reads A to Z whichever order the session
            // happens to hold them in.
            caps.sort_by_key(|cap| cap.index);
            if caps != self.caps {
                self.caps = caps;
                // The selection named a place in a list that has been replaced.
                self.selected = None;
                self.dragging = None;
                changed = true;
            }
        }

        // The shapes are held by the link, so a bump only means this board is
        // drawing a list that has moved on.
        if self.adopted_shapes != Some(versions.shapes) {
            self.adopted_shapes = Some(versions.shapes);
            self.picked.retain_to(self.collab.annotation_count());
            changed = true;
        }

        if changed {
            self.redraw(cx);
        }
    }

    /// Puts `caps` on the board, telling the session what changed.
    ///
    /// By difference rather than by sending the lot: a session holds capture
    /// points by id, and re-sending an unchanged one would have every peer
    /// redraw its board for nothing.
    fn set_caps(&mut self, caps: Vec<BoardCapPoint>) {
        let was: Vec<crate::collab::CapPointId> = self.caps.iter().map(|cap| cap.id).collect();
        for gone in was.iter().filter(|id| !caps.iter().any(|cap| cap.id == **id)) {
            self.collab.remove_cap(*gone);
        }
        for cap in &caps {
            if !self.caps.iter().any(|held| held == cap) {
                self.collab.set_cap(cap.to_wire());
            }
        }
        self.caps = caps;
    }

    /// Says that one capture point changed, so every board in the session shows
    /// the same zone in the same place.
    fn report_cap(&self, index: usize) {
        if let Some(cap) = self.caps.get(index) {
            self.collab.set_cap(cap.to_wire());
        }
    }

    /// What is on the board, for an undo step to hold.
    fn board_state(&self) -> BoardState {
        BoardState { caps: self.caps.clone(), annotations: self.collab.annotations_held() }
    }

    /// The shapes drawn on the board.
    ///
    /// Held by the link rather than by the board: with a session it is the
    /// session's list, and without one the link's own, so the same code draws
    /// and edits either.
    pub fn shapes(&self) -> Vec<wt_collab_client::types::Annotation> {
        self.collab.annotations()
    }

    /// Puts `shapes` on the board in place of what is there, which is what
    /// opening a saved board does.
    fn replace_shapes(&mut self, shapes: Vec<wt_collab_client::types::Annotation>) {
        for at in (0..self.collab.annotation_count()).rev() {
            self.collab.erase_annotation(at);
        }
        for shape in shapes {
            self.collab.add_annotation(shape);
        }
        self.picked.clear();
    }

    /// Places a capture point where the reader clicked.
    fn add_cap_at(&mut self, world: (f32, f32), cx: &mut Context<Self>) {
        self.remember();
        // Lettered past the highest already there, so a cap taken off does not
        // hand its letter to the next one placed.
        let index = self.caps.iter().map(|cap| cap.index + 1).max().unwrap_or(0);
        self.caps.push(BoardCapPoint {
            id: crate::collab::CapPointId::fresh(),
            index,
            world_x: world.0,
            world_z: world.1,
            radius: NEW_CAP_RADIUS,
            team: None,
            frozen: false,
        });
        self.report_cap(self.caps.len() - 1);
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
        self.remember();
        self.selected = None;
        // The letters the others carry stand: a cap is lettered by what it was
        // called, and renumbering would rename the ones that stayed.
        let gone = self.caps.remove(at);
        self.collab.remove_cap(gone.id);
        self.redraw(cx);
    }

    /// Widens or narrows the selected zone by half a kilometre.
    ///
    /// Read and set in kilometres, which is how a cap circle is talked about;
    /// the model itself is in the world's own units.
    pub fn step_selected_radius(&mut self, by_km: f32, cx: &mut Context<Self>) {
        let Some(at) = self.selected else { return };
        if self.caps.get(at).is_none_or(|cap| cap.frozen) {
            return;
        }
        self.remember();
        let Some(cap) = self.caps.get_mut(at) else { return };
        cap.radius = (cap.radius + by_km * WORLD_UNITS_PER_KM).max(MIN_CAP_RADIUS);
        self.report_cap(at);
        self.redraw(cx);
    }

    /// Hands the selected capture point to the next team round: nobody, the
    /// reader's side, then the other.
    pub fn cycle_selected_team(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.selected else { return };
        if self.caps.get(at).is_none_or(|cap| cap.frozen) {
            return;
        }
        self.remember();
        let Some(cap) = self.caps.get_mut(at) else { return };
        cap.team = match cap.team.map(|team| team.raw()) {
            None => Some(wows_replays::types::TeamId::new(0)),
            Some(0) => Some(wows_replays::types::TeamId::new(1)),
            Some(_) => None,
        };
        self.report_cap(at);
        self.redraw(cx);
    }

    /// Takes every capture point off the board.
    pub fn clear_caps(&mut self, cx: &mut Context<Self>) {
        if self.caps.is_empty() {
            return;
        }
        self.remember();
        for gone in std::mem::take(&mut self.caps) {
            self.collab.remove_cap(gone.id);
        }
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
    fn add_shape(&mut self, mut annotation: wt_collab_client::types::Annotation) {
        // Named before it goes over rather than after: in a session the add is a
        // message, and the list it would be named in has not caught up yet.
        if let wt_collab_client::types::Annotation::Ship { config, .. } = &mut annotation
            && let Some(placed) = self.placing.clone()
        {
            *config = Some(wt_collab_client::types::AnnotationShipConfig {
                param_id: placed.param_id,
                ship_name: placed.name,
                range_filter: self.range_filter.clone(),
                // Stock hull and no modifiers until the reader says otherwise,
                // which is where the egui chooser leaves it.
                ..Default::default()
            });
        }
        self.collab.add_annotation(annotation);
    }

    /// Turns one circle on or off, for the ships already placed and the ones to
    /// come.
    pub fn set_range_circle(&mut self, circle: RangeCircle, on: bool, cx: &mut Context<Self>) {
        circle.set(&mut self.range_filter, on);
        for (at, mut annotation) in self.shapes().into_iter().enumerate() {
            let wt_collab_client::types::Annotation::Ship { config: Some(config), .. } = &mut annotation else {
                continue;
            };
            circle.set(&mut config.range_filter, on);
            self.collab.update_annotation(at, annotation);
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
        if self.collab.annotation_count() == 0 {
            return;
        }
        self.remember();
        self.replace_shapes(Vec::new());
        self.redraw(cx);
    }

    /// Hands a pointer event to the tool and keeps what it drew.
    fn stroke(&mut self, stroke: wt_collab_client::drawing::Stroke, cx: &mut Context<Self>) {
        match self.drawing.handle(stroke, &self.shapes()) {
            Some(wt_collab_client::drawing::Drawn::Added(annotation)) => self.add_shape(annotation),
            Some(wt_collab_client::drawing::Drawn::Erased(index)) => self.collab.erase_annotation(index),
            _ => {}
        }
        self.redraw(cx);
    }

    /// The same, for a stroke that only moved the shape being built: the frame
    /// is redrawn by the pointer that moved it, not again here.
    fn stroke_without_redraw(&mut self, stroke: wt_collab_client::drawing::Stroke) {
        if let Some(wt_collab_client::drawing::Drawn::Added(annotation)) = self.drawing.handle(stroke, &self.shapes()) {
            self.add_shape(annotation);
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
            self.remember();
            self.stroke(wt_collab_client::drawing::Stroke::Began { at: [at.0, at.1] }, cx);
            return;
        }
        if self.adding {
            let Some(world) = self.world_point(event.position) else { return };
            self.add_cap_at(world, cx);
            return;
        }
        // A double click puts the whole map back, which is how the replay
        // viewport is reset too.
        if event.click_count >= 2 {
            self.reset_view(cx);
            return;
        }
        // The handle takes the drag before anything else: it sits above the
        // shape, over map nobody is reaching for.
        if let Some((index, annotation)) = self.handle_under(event.position) {
            self.remember();
            self.turning = Some((index, annotation));
            return;
        }

        if let Some(at) = self.map_point(event.position) {
            // A shape already picked out is dragged as a whole, which is what
            // moving a line or a circle means.
            if !self.picked.is_empty()
                && self.shape_under(event.position).is_some_and(|found| self.picked.picked().contains(&found))
            {
                self.remember();
                self.moving = Some(([at.0, at.1], self.shapes()));
                return;
            }
            // Otherwise a press on one picks it out, and ctrl adds it to
            // whatever is already picked. The reach is the shared one, which is
            // what the replay viewport picks with too.
            let was: Vec<usize> = self.picked.picked().to_vec();
            self.picked.click(&self.shapes(), [at.0, at.1], event.modifiers.secondary());
            if self.picked.picked() != was.as_slice() {
                cx.notify();
                return;
            }
        }

        match self.cap_under(event.position) {
            Some((index, what)) => {
                self.selected = Some(index);
                if what != CapDrag::None {
                    // Taken as the drag begins: the pointer moves it many times
                    // and all of that is one change to take back.
                    self.remember();
                }
                self.dragging = Some((index, what));
            }
            // A drag past every zone moves the map, which is what a reader
            // zoomed in is reaching for; the click alone puts the selection
            // down, which takes the handles off the map.
            None => {
                self.selected = None;
                self.panning = Some(event.position);
            }
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

        if let Some((index, before)) = self.turning.clone() {
            let Some(at) = self.map_point(event.position) else { return };
            let [left, top, right, bottom] = wt_collab_client::drawing::annotation_bounds(&before);
            let middle = [(left + right) / 2.0, (top + bottom) / 2.0];
            let mut turned = before;
            wt_collab_client::drawing::rotate_annotation(
                &mut turned,
                wt_collab_client::drawing::bearing(middle, [at.0, at.1]),
            );
            self.collab.update_annotation(index, turned);
            self.redraw(cx);
            return;
        }

        if let Some((from, was)) = self.moving.clone() {
            let Some(at) = self.map_point(event.position) else { return };
            let delta = [at.0 - from[0], at.1 - from[1]];
            // Measured from where the drag began rather than from the last
            // position, so the shape does not creep as the pointer stutters.
            for index in self.picked.picked() {
                let Some(annotation) = was.get(*index) else { continue };
                let mut moved = annotation.clone();
                wt_collab_client::drawing::move_annotation(&mut moved, delta);
                self.collab.update_annotation(*index, moved);
            }
            self.redraw(cx);
            return;
        }

        if let Some(from) = self.panning {
            let delta = (event.position.x.as_f32() - from.x.as_f32(), event.position.y.as_f32() - from.y.as_f32());
            self.panning = Some(event.position);
            if delta.0 != 0.0 || delta.1 != 0.0 {
                self.view = self.view.dragged(delta);
                self.redraw(cx);
            }
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
        self.panning = None;
        self.moving = None;
        self.turning = None;
        // Said once, when it is let go: a drag moves a zone at pointer-event
        // rate, and every peer would redraw its board for each of them.
        if let Some((index, _)) = self.dragging {
            self.report_cap(index);
        }
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

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let notches = event.delta.pixel_delta(window.line_height()).y.as_f32();
        if notches != 0.0 {
            self.zoom_about(notches, event.position, cx);
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.panning = None;
        self.moving = None;
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
        let caps = self.mode.as_ref().map(|key| caps_of(&self.layouts, key)).unwrap_or_default();
        self.set_caps(caps);
        self.map = Some(map);
        // A board is announced with the map it is on, so a different map is a
        // different announcement under the same board.
        self.announce_again(cx);
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
            self.set_caps(Vec::new());
            self.selected = None;
            self.redraw(cx);
            return;
        }
        let caps = caps_of(&self.layouts, &key);
        self.set_caps(caps);
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

        let view = self.view;
        let art = self.peer_art.clone();
        let sent_info = self.peer_map_info.clone();
        let board_id = self.board_id;
        let caps = self.caps.clone();
        let shapes = self.shapes();
        let mut annotations = shapes.clone();
        // A shape the reader has picked out is drawn again underneath in a
        // wider stroke, which is what says it is the one a drag will move.
        let mut under: Vec<wt_collab_client::types::Annotation> =
            self.picked.picked().iter().filter_map(|at| shapes.get(*at)).map(halo).collect();
        under.append(&mut annotations);
        let mut annotations = under;
        // The shape under the pointer is drawn the way the finished one will
        // be, so what the reader sees while dragging is what they get.
        if let Some(part_drawn) = self.pointer_at.and_then(|at| self.drawing.in_progress(at)) {
            annotations.push(part_drawn);
        }
        cx.spawn(async move |this, cx| {
            let drawn = cx
                .background_spawn(async move {
                    rasterise(
                        Drawing {
                            space: &map.space,
                            art: art.as_ref(),
                            info: sent_info.as_ref(),
                            board_id: board_id.raw(),
                        },
                        &game_data,
                        version,
                        view,
                        &caps,
                        &annotations,
                    )
                })
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
/// What a board draws its map from: this build's art for a space, or art a peer
/// sent for a board this build has none for.
struct Drawing<'a> {
    space: &'a str,
    art: Option<&'a Arc<image::RgbImage>>,
    /// What the map measures, where it came off the wire with the art.
    info: Option<&'a wows_minimap_renderer::MapInfo>,
    /// Names the entry a renderer over peer art is kept under, since the art
    /// belongs to the board rather than to a map this build ships.
    board_id: u64,
}

fn rasterise(
    drawing: Drawing<'_>,
    game_data: &GameDataCache,
    version: Option<wowsunpack::data::Version>,
    view: wows_minimap_renderer::viewport::MapViewport,
    caps: &[BoardCapPoint],
    annotations: &[wt_collab_client::types::Annotation],
) -> Option<Arc<RenderImage>> {
    let loaded = game_data.newest_loaded()?;
    // The measurements a peer sent, where this build ships none for the map.
    let map = match drawing.info.cloned() {
        Some(sent) => sent,
        None => wows_minimap_renderer::assets::load_map_info(drawing.space, loaded.vfs())?,
    };
    let mut commands: Vec<DrawCommand> = caps.iter().map(|cap| cap.command(&map)).collect();
    // Under the markers, so a ship is not buried under its own circles.
    commands.extend(ship_ranges(annotations, game_data, version, map.space_size as f32));
    // Over the zones, so a line drawn across one is not buried under it.
    commands.extend(annotations.iter().flat_map(wt_collab_client::geometry::annotation_commands));
    match drawing.art {
        Some(art) => crate::minimap_preview::render_map_art(
            &format!("board-{}", drawing.board_id),
            art,
            game_data,
            view,
            &commands,
        ),
        None => crate::minimap_preview::render_map(drawing.space, game_data, view, &commands),
    }
}

/// A map image as a PNG, for sending to a peer.
///
/// `None` when it cannot be encoded, which leaves the board announced without
/// art rather than not announced at all.
fn encode_png(art: &image::RgbImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// How often a board looks at what the session holds.
const COLLAB_TICK: std::time::Duration = std::time::Duration::from_millis(200);

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
        // Nor is it still a board the session is on, which a peer would
        // otherwise be offered. Only this app's own: closing the window on a
        // board a peer shared would take it off their board too.
        if !self.is_a_peers() {
            self.collab.close_board();
        }
    }
}

impl TacticsBoard {
    /// The board's keyboard: the chords the egui board takes.
    ///
    /// Ctrl+Z takes a change back and Ctrl+Y or Ctrl+Shift+Z puts it back;
    /// Escape puts the tool down, and Delete erases the capture point picked
    /// out.
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.secondary() {
            match event.keystroke.key.as_str() {
                "z" if !modifiers.shift => self.undo(cx),
                "y" | "z" => self.redo(cx),
                _ => {}
            }
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => self.set_tool(wt_collab_client::drawing::Tool::None, cx),
            // What is picked out is what a delete is about: a drawn shape if
            // one is picked, and the capture point otherwise.
            "delete" | "backspace" if !self.picked.picked().is_empty() => self.erase_picked(cx),
            "delete" | "backspace" => self.remove_selected(cx),
            _ => {}
        }
    }
}

impl Render for TacticsBoard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Taken at draw time because nothing else knows it, and the menu needs
        // it to bring this board forward.
        self.window = Some(window.window_handle());
        let border = cx.theme().border;
        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .child(self.render_toolbar(cx))
            // Says why zones and shapes this reader did not put there are on the
            // board: it is a peer's, opened because the session is on it.
            .children(self.is_a_peers().then(|| {
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.tactics.a_peers_board").to_string())
            }))
            .child(div().h(px(1.)).bg(border))
            .child(self.render_map(cx))
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
        let has_drawing = self.collab.annotation_count() > 0;

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
            .children(self.rotation_handle_overlay())
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
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
                .child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.tactics.cap_radius", km = format!("{:.1}", radius_km(cap.radius))).into_owned()),
                )
                .child({
                    let board = board.clone();
                    Button::new("tactics-cap-narrow").label("-").compact().on_click(
                        move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.step_selected_radius(-RADIUS_STEP_KM, cx));
                        },
                    )
                })
                .child({
                    let board = board.clone();
                    Button::new("tactics-cap-widen").label("+").compact().on_click(
                        move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.step_selected_radius(RADIUS_STEP_KM, cx));
                        },
                    )
                })
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
            .child({
                let board = board.clone();
                Button::new("tactics-undo")
                    .label(t!("ui.renderer.annotations.undo").into_owned())
                    .compact()
                    .disabled(!self.can_undo())
                    .on_click(move |_event, _window, cx: &mut App| {
                        board.update(cx, |board, cx| board.undo(cx));
                    })
            })
            .child({
                let board = board.clone();
                Button::new("tactics-redo")
                    .label(t!("ui.renderer.annotations.redo").into_owned())
                    .compact()
                    .disabled(!self.can_redo())
                    .on_click(move |_event, _window, cx: &mut App| {
                        board.update(cx, |board, cx| board.redo(cx));
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
        BoardCapPoint {
            id: crate::collab::CapPointId::new(index as u64 + 1),
            index,
            world_x: x,
            world_z: z,
            radius,
            team: None,
            frozen: false,
        }
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

    /// A cap circle is talked about in kilometres, and the model holds it in the
    /// game's own thirty-metre units.
    #[test]
    fn a_zone_reads_in_kilometres() {
        // The default a placed cap starts at is about the 5 km a cap circle is.
        assert!((super::radius_km(super::NEW_CAP_RADIUS) - 4.5).abs() < 0.01);
        assert!((super::radius_km(super::WORLD_UNITS_PER_KM) - 1.0).abs() < 0.001);
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
        let cap = BoardCapPoint {
            id: crate::collab::CapPointId::new(1),
            index: 0,
            world_x: centre.x,
            world_z: centre.z,
            radius: 150.0,
            team: None,
            frozen: false,
        };

        let drawn =
            map.world_to_minimap(wowsunpack::game_types::WorldPos::new(cap.world_x, 0.0, cap.world_z), MINIMAP_SIZE);
        assert!((drawn.x - MINIMAP_SIZE as f32 / 2.0).abs() < 0.01);
        assert!((drawn.y - MINIMAP_SIZE as f32 / 2.0).abs() < 0.01);

        let pressed = map.minimap_to_world_f32(drawn.x, drawn.y, MINIMAP_SIZE);
        let away = ((pressed.x - cap.world_x).powi(2) + (pressed.z - cap.world_z).powi(2)).sqrt();
        assert!(away < 1.0, "the press is {away} units from the zone it was aimed at");
    }

    /// A press is read against the part of the map on screen, so a zoomed board
    /// still lands where it looks like it lands.
    #[test]
    fn a_press_is_read_through_the_window_the_map_is_drawn_in() {
        use wows_minimap_renderer::viewport::MapViewport;

        let whole = MapViewport::default();
        assert_eq!(whole.to_map(100.0, 200.0), (100.0, 200.0), "the whole map draws itself one to one");

        // Zoomed about the centre, a point drawn at the centre is still the
        // centre of the map.
        let middle = MINIMAP_SIZE as f32 / 2.0;
        let zoomed = whole.zoomed_about(whole.zoom() * 2.0, (middle, middle));
        let (x, y) = zoomed.to_map(middle, middle);
        assert!((x - middle).abs() < 0.01 && (y - middle).abs() < 0.01, "the centre moved to ({x}, {y})");

        // And a point off to one side is nearer the centre in map space than it
        // is on screen, which is what being zoomed in means.
        let (x, _y) = zoomed.to_map(middle + 100.0, middle);
        assert!(x > middle && x < middle + 100.0, "a point 100px right of centre reads as {x}");
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

#[cfg(test)]
mod letterbox_tests {
    use super::Letterbox;
    use gpui_kit::Bounds;
    use gpui_kit::Pixels;
    use gpui_kit::Point;
    use gpui_kit::Size;
    use gpui_kit::px;

    fn element(width: f32, height: f32) -> Bounds<Pixels> {
        Bounds { origin: Point { x: px(10.), y: px(20.) }, size: Size { width: px(width), height: px(height) } }
    }

    /// A frame narrower than its element is centred in it rather than stretched.
    #[test]
    fn a_frame_is_centred_in_the_element() {
        let fit = Letterbox::fitting(element(400., 200.), (100., 100.)).expect("a frame with extent");
        assert_eq!(fit.scale, 2.0, "held to the tighter direction");
        assert_eq!(fit.top, 20.0, "filling it top to bottom");
        assert_eq!(fit.left, 110.0, "and centred across it");
    }

    /// The two directions agree to the pixel, which is what lets a handle be
    /// placed by one and pressed by the other.
    #[test]
    fn the_two_directions_are_each_others_inverse() {
        let fit = Letterbox::fitting(element(400., 200.), (100., 100.)).expect("a frame with extent");
        for at in [(0.0, 0.0), (50.0, 50.0), (99.0, 1.0)] {
            let on_screen = fit.to_element(at).expect("a point the frame shows");
            let back = fit.to_frame(on_screen).expect("and it is over the map");
            assert!((back.0 - at.0).abs() < 1e-3 && (back.1 - at.1).abs() < 1e-3, "{at:?} -> {back:?}");
        }
    }

    /// A press in the margin beside the map is not on it, and a point the frame
    /// does not show has nowhere on screen to be.
    #[test]
    fn a_point_outside_is_refused_both_ways() {
        let fit = Letterbox::fitting(element(400., 200.), (100., 100.)).expect("a frame with extent");
        assert!(fit.to_frame((20.0, 100.0)).is_none(), "in the left margin");
        assert!(fit.to_element((-1.0, 50.0)).is_none(), "off the frame");
        assert!(fit.to_element((100.0, 50.0)).is_none(), "and past its far edge");
    }

    /// A frame with no extent cannot be placed against at all.
    #[test]
    fn a_frame_with_no_extent_is_refused() {
        assert!(Letterbox::fitting(element(400., 200.), (0., 100.)).is_none());
    }
}

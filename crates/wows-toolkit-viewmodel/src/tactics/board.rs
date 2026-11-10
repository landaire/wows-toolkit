//! The model a tactics board is drawn from: the maps and modes it offers, the
//! capture points on it, the tools it draws with, and where a pointer lands.
//!
//! Shared by every front end, so a press lands on the same zone and a board
//! reads the same capture layouts whichever app shows it.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_replay_insights::cap_layout::CapPointLayout;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::GameParamId;
use wows_replays::types::TeamId;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::WorldPos;
use wowsunpack::game_types::WorldPos2D;
use wowsunpack::vfs::VfsPath;
use wt_collab_protocol::drawing::Held;
use wt_collab_protocol::drawing::Tool;
use wt_collab_protocol::ids::CapPointId;
use wt_collab_protocol::protocol::WireCapPoint;
use wt_collab_protocol::types::Annotation;
use wt_collab_protocol::types::AnnotationRangeFilter;

use crate::ship_catalog::ShipCatalog;
use crate::ship_catalog::ShipEntry;
use crate::tactics::naming;
use crate::tactics::preset::PresetCapPoint;

/// How solid a capture zone's fill is drawn, as the battle draws its own.
pub const CAP_FILL_ALPHA: f32 = 0.25;

/// What a team's capture zone is coloured: the reader's own team, the enemy,
/// and one nobody holds.
///
/// The same two the renderer paints a friendly and an enemy marker
/// (`wt_collab_protocol::geometry`), so one side reads as one colour whether it
/// is a zone or a ship.
pub const FRIENDLY_COLOR: [u8; 3] = [76, 232, 170];
pub const ENEMY_COLOR: [u8; 3] = [254, 77, 42];
pub const NEUTRAL_COLOR: [u8; 3] = [255, 255, 255];

/// How wide a world unit is: the game measures in 30-metre units, so a zone
/// stated in kilometres is that many thirty-metre steps.
pub const WORLD_UNITS_PER_KM: f32 = 1000.0 / 30.0;

/// How far one press moves a zone's width, in kilometres.
pub const RADIUS_STEP_KM: f32 = 0.5;

/// How near a zone's drawn edge a press has to land to widen it rather than
/// move it, in map pixels. A tolerance rather than a fraction of the radius:
/// the reader is aiming at a line on screen, and that line is the same
/// thickness whatever the zone is.
pub const RESIZE_BAND_PX: f32 = 8.0;

/// The smallest a zone can be made, in world units, so one cannot be shrunk to
/// nothing and lost. The egui board holds its own to the same floor.
pub const MIN_CAP_RADIUS: f32 = 0.1 * WORLD_UNITS_PER_KM;
pub const MAX_CAP_RADIUS: f32 = 10.0 * WORLD_UNITS_PER_KM;

/// What a capture point added by hand starts as. A cap circle is about 5 km,
/// which is about 167 world units; the egui board starts one at 150.
pub const NEW_CAP_RADIUS: f32 = 150.0;

/// How near a drawn shape a press has to land to pick it out, in map pixels.
/// A line is one pixel wide and nobody presses one exactly.
pub const SHAPE_REACH_PX: f32 = 10.0;

/// How much one pixel of wheel travel changes the zoom.
pub const ZOOM_PER_NOTCH: f32 = 0.004;

/// What a board draws in until the reader picks otherwise, matching the ink and
/// nib the replay viewport starts with.
pub const DEFAULT_INK: [u8; 4] = [0xff, 0xd7, 0x3a, 0xff];
pub const DEFAULT_NIB: f32 = 2.0;

/// The inks the board offers, which are the replay viewport's own, with the
/// translation key each is named by.
pub const INKS: [([u8; 4], &str); 6] = [
    ([0xff, 0xd7, 0x3a, 0xff], "ui.tactics.ink_yellow"),
    ([0xe8, 0x73, 0x7b, 0xff], "ui.tactics.ink_red"),
    ([0x6f, 0xd9, 0x8a, 0xff], "ui.tactics.ink_green"),
    ([0x7f, 0xb4, 0xe8, 0xff], "ui.tactics.ink_blue"),
    ([0xe9, 0xe5, 0xdd, 0xff], "ui.tactics.ink_white"),
    ([0x1a, 0x1a, 0x18, 0xff], "ui.tactics.ink_black"),
];

/// How wide the nib can be drawn, which is the span the replay viewport holds
/// its own to.
pub const MIN_NIB: f32 = 1.0;
pub const MAX_NIB: f32 = 8.0;

/// How much wider a picked shape's halo is drawn, and in what.
pub const HALO_WIDTH: f32 = 4.0;
pub const HALO_INK: [u8; 4] = [0xff, 0xd7, 0x3a, 0x80];

/// How many changes can be taken back.
///
/// A board is edited in small steps and the whole of what is on it is kept per
/// step, so the stack is held to a depth rather than growing with the session.
pub const HISTORY_DEPTH: usize = 50;

/// How many ship matches the search offers at once.
pub const SHIP_MATCHES: usize = 10;

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

impl MapChoice {
    /// Whether a typed filter finds this map under `title`, the name the
    /// picker lists it by.
    pub fn matches(&self, title: &str, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        title.to_lowercase().contains(&query)
            || self.label.to_lowercase().contains(&query)
            || self.space.to_lowercase().contains(&query)
            || self.map_id.is_some_and(|id| id.to_string().contains(&query))
    }
}

/// What tells two maps apart: the id and the space, without the label.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapChoiceKey {
    pub map_id: Option<u32>,
    pub space: String,
}

impl From<&MapChoice> for MapChoiceKey {
    fn from(map: &MapChoice) -> Self {
        Self { map_id: map.map_id, space: map.space.clone() }
    }
}

/// One game mode the chosen map has a recorded layout for.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeChoice {
    pub key: CapLayoutKey,
    pub label: String,
}

/// Every map the board can be set on.
///
/// The recorded layouts first, then whatever else the loaded build ships art
/// for: a map nobody has a replay of can still be drawn on, it simply starts
/// with no capture points.
pub fn maps(layouts: &CapLayoutDb, metadata: Option<&GameMetadataProvider>, vfs: Option<&VfsPath>) -> Vec<MapChoice> {
    let mut maps: Vec<MapChoice> = layouts
        .maps()
        .into_iter()
        .map(|(map_id, space)| {
            let label = naming::map_label(&space, metadata);
            MapChoice { map_id: Some(map_id), space, label }
        })
        .collect();

    if let Some(vfs) = vfs {
        for space in drawable_spaces(vfs) {
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

/// What the picker lists each map as: its label, with its id or space after it
/// where two maps read alike.
pub fn map_titles(maps: &[MapChoice]) -> Vec<String> {
    let mut label_counts = HashMap::new();
    for map in maps {
        *label_counts.entry(map.label.to_lowercase()).or_insert(0usize) += 1;
    }

    maps.iter()
        .map(|map| {
            let alike = label_counts.get(&map.label.to_lowercase()).is_some_and(|count| *count > 1);
            if !alike {
                return map.label.clone();
            }
            format!("{} ({})", map.label, map.map_id.map_or_else(|| map.space.clone(), |id| id.to_string()))
        })
        .collect()
}

/// The one map drawn on `space`, if exactly one is.
pub fn unique_map_for_space(maps: &[MapChoice], space: &str) -> Option<MapChoice> {
    let mut matches = maps.iter().filter(|map| map.space == space);
    let map = matches.next()?.clone();

    matches.next().is_none().then_some(map)
}

/// The spaces the build ships a minimap for.
///
/// A dock scene is not a map anyone plays on, and a space with no minimap art
/// has nothing to draw, so neither is offered.
pub fn drawable_spaces(vfs: &VfsPath) -> Vec<String> {
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
/// A map with no id has no recorded layout and so no modes, which is what a
/// scan can change.
pub fn modes(layouts: &CapLayoutDb, map_id: Option<u32>, metadata: Option<&GameMetadataProvider>) -> Vec<ModeChoice> {
    let Some(map_id) = map_id else { return Vec::new() };
    let found: Vec<CapLayout> = layouts.modes_for_map(map_id).into_iter().cloned().collect();
    let labels = naming::mode_labels(&found, metadata);

    found.into_iter().zip(labels).map(|(layout, label)| ModeChoice { key: layout.key.clone(), label }).collect()
}

/// The capture points a mode puts on the map.
pub fn caps_of(layouts: &CapLayoutDb, key: &CapLayoutKey) -> Vec<BoardCapPoint> {
    layouts.get(key).map(|layout| layout.points.iter().map(BoardCapPoint::from_layout).collect()).unwrap_or_default()
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
    pub id: CapPointId,
    /// Its place in the alphabet: A, B, C.
    pub index: usize,
    /// Where it sits, in the world's own coordinates.
    pub world_x: f32,
    pub world_z: f32,
    /// How wide the zone is, in world units.
    pub radius: f32,
    /// Which team holds it at the start. `None` for a neutral one.
    pub team: Option<TeamId>,
    /// Whether it came from a recorded layout rather than from the reader.
    ///
    /// A frozen cap is where the game put it, so it is not dragged or deleted:
    /// the egui board holds its own the same way.
    pub frozen: bool,
}

impl BoardCapPoint {
    pub fn from_layout(point: &CapPointLayout) -> Self {
        Self {
            index: point.index,
            world_x: point.position.x,
            world_z: point.position.z,
            radius: point.radius.value(),
            // The layout states a neutral cap as a negative team, which is an
            // absence rather than a team.
            team: (point.team_id >= 0).then(|| TeamId::new(point.team_id)),
            frozen: true,
            id: CapPointId::fresh(),
        }
    }

    pub fn from_preset(saved: &PresetCapPoint) -> Self {
        Self {
            index: saved.index,
            world_x: saved.world_x,
            world_z: saved.world_z,
            radius: saved.radius,
            team: (saved.team_id >= 0).then(|| TeamId::new(saved.team_id)),
            frozen: saved.frozen,
            id: CapPointId::fresh(),
        }
    }

    pub fn to_preset(&self) -> PresetCapPoint {
        PresetCapPoint {
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
    pub fn to_wire(&self) -> WireCapPoint {
        WireCapPoint {
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
    pub fn from_wire(wire: &WireCapPoint) -> Self {
        Self {
            id: CapPointId::new(wire.id),
            index: wire.index as usize,
            world_x: wire.world_x,
            world_z: wire.world_z,
            radius: wire.radius,
            team: (wire.team_id >= 0).then(|| TeamId::new(wire.team_id)),
            frozen: wire.frozen,
        }
    }

    /// Whether `other` is the same zone in the same place, whatever id either
    /// carries.
    pub fn same_layout(&self, other: &Self) -> bool {
        self.index == other.index
            && self.world_x == other.world_x
            && self.world_z == other.world_z
            && self.radius == other.radius
            && self.team == other.team
            && self.frozen == other.frozen
    }

    /// The letter this cap is labelled with.
    pub fn letter(&self) -> String {
        char::from(b'A' + (self.index.min(25) as u8)).to_string()
    }

    /// What the renderer draws it as.
    pub fn command(&self, map: &MapInfo) -> DrawCommand {
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

/// The board reads team zero as the reader's own, which is the team a replay
/// records the recording player on.
pub fn team_color(team: Option<TeamId>) -> [u8; 3] {
    match team.map(|team| team.raw()) {
        None => NEUTRAL_COLOR,
        Some(0) => FRIENDLY_COLOR,
        Some(_) => ENEMY_COLOR,
    }
}

/// The team a capture point passes to next: nobody, the reader's side, then the
/// other.
pub fn next_team(team: Option<TeamId>) -> Option<TeamId> {
    match team.map(|team| team.raw()) {
        None => Some(TeamId::new(0)),
        Some(0) => Some(TeamId::new(1)),
        Some(_) => None,
    }
}

/// The translation key a team is named by.
pub fn team_label_key(team: Option<TeamId>) -> &'static str {
    match team.map(|team| team.raw()) {
        None => "ui.tactics.team_neutral",
        Some(0) => "ui.tactics.team_one",
        Some(_) => "ui.tactics.team_two",
    }
}

/// A zone's width in the units it is talked about in.
pub fn radius_km(radius: f32) -> f32 {
    radius / WORLD_UNITS_PER_KM
}

/// The letter the next capture point placed by hand takes: past the highest
/// already there, so a cap taken off does not hand its letter to the next one.
pub fn next_cap_index(caps: &[BoardCapPoint]) -> usize {
    caps.iter().map(|cap| cap.index + 1).max().unwrap_or(0)
}

/// The same shape, drawn wider and dimmer: what sits under a picked one to say
/// it is picked.
pub fn halo(annotation: &Annotation) -> Annotation {
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

/// [`RESIZE_BAND_PX`] in world units, at this map's scale.
///
/// A zone's edge is a line on screen; the press that grabs it is measured in
/// the world, so the tolerance has to cross over.
pub fn band_in_world(map: &MapInfo) -> f32 {
    let per_unit = map.world_distance_to_minimap(1.0, MINIMAP_SIZE);
    if per_unit > f32::EPSILON { RESIZE_BAND_PX / per_unit } else { RESIZE_BAND_PX }
}

/// What a drag on a capture point is doing to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapDrag {
    /// Picked out, and nothing more: a frozen cap is where the game put it.
    None,
    /// Moving it across the map.
    Move,
    /// Widening or narrowing its zone.
    Resize,
}

/// Which capture point a press at `world` lands on, and what it would do to it.
///
/// A press within a few pixels of a zone's drawn edge, inside or out, widens
/// it; anywhere else inside moves it. The topmost one wins, which is the last
/// drawn.
pub fn cap_under(caps: &[BoardCapPoint], world: WorldPos2D, map: &MapInfo) -> Option<(usize, CapDrag)> {
    let WorldPos2D { x, z } = world;
    // The band is a screen tolerance, so it is measured where the reader is
    // aiming: converted from pixels into the world at this map's scale.
    let band = band_in_world(map);

    caps.iter().enumerate().rev().find_map(|(index, cap)| {
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

/// A position in the element a frame is drawn into, in its own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementPos {
    pub x: f32,
    pub y: f32,
}

/// A position in a drawn frame, in the frame's own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePos {
    pub x: f32,
    pub y: f32,
}

/// A width and a height in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub width: f32,
    pub height: f32,
}

/// Where a drawn frame landed in the element that shows it, and how much it was
/// scaled to fit.
///
/// The frame is drawn to fit without stretching, so it is letterboxed: its top
/// left corner is not the element's own. Held in one place so the two
/// directions cannot drift: a handle placed by one and pressed by the other has
/// to agree to the pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    /// The frame's own size, which is what a point is held inside.
    pub frame: Extent,
}

impl Letterbox {
    /// The frame `frame` fitted into an element at `origin` of `size`. `None`
    /// for a frame or an element with no extent, which nothing can be placed
    /// against.
    pub fn fitting(origin: ElementPos, size: Extent, frame: Extent) -> Option<Self> {
        if frame.width <= 0.0 || frame.height <= 0.0 || size.width <= 0.0 || size.height <= 0.0 {
            return None;
        }
        let scale = (size.width / frame.width).min(size.height / frame.height);

        Some(Self {
            left: origin.x + (size.width - frame.width * scale) / 2.0,
            top: origin.y + (size.height - frame.height * scale) / 2.0,
            scale,
            frame,
        })
    }

    /// An element position as a point in the frame. `None` for one in the
    /// margin beside a frame that does not fill its element.
    pub fn to_frame(self, at: ElementPos) -> Option<FramePos> {
        let x = (at.x - self.left) / self.scale;
        let y = (at.y - self.top) / self.scale;

        self.holds(x, y).then_some(FramePos { x, y })
    }

    /// A point in the frame as an element position. `None` for one the frame
    /// does not show, which has nowhere on screen to be.
    pub fn to_element(self, at: FramePos) -> Option<ElementPos> {
        self.holds(at.x, at.y)
            .then_some(ElementPos { x: self.left + at.x * self.scale, y: self.top + at.y * self.scale })
    }

    fn holds(self, x: f32, y: f32) -> bool {
        x >= 0.0 && x < self.frame.width && y >= 0.0 && y < self.frame.height
    }
}

/// One of the tools the board offers.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardTool {
    pub tool: Tool,
    /// The translation key it is named by.
    pub label_key: &'static str,
    /// The key that takes it up with the shortcut modifier held.
    pub shortcut: &'static str,
}

/// The tools the board offers, in the order the egui board takes them up.
///
/// The shapes are drawn hollow: a board is read through, and a filled one hides
/// the map it is about.
pub fn tools() -> Vec<BoardTool> {
    let tool = |tool, label_key, shortcut| BoardTool { tool, label_key, shortcut };
    vec![
        tool(Tool::Freehand, "ui.renderer.annotations.tool_freehand", "2"),
        tool(Tool::Line, "ui.renderer.annotations.tool_line", "4"),
        tool(Tool::Arrow, "ui.renderer.annotations.tool_arrow", "1"),
        tool(Tool::Circle { filled: false }, "ui.renderer.annotations.tool_circle", "5"),
        tool(Tool::Rectangle { filled: false }, "ui.renderer.annotations.tool_rectangle", "6"),
        tool(Tool::Triangle { filled: false }, "ui.renderer.annotations.tool_triangle", "7"),
        tool(Tool::Measurement, "ui.renderer.annotations.tool_measurement", "M"),
        tool(Tool::Eraser, "ui.renderer.annotations.tool_eraser", "3"),
    ]
}

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

/// Which range circles a placed ship can show, with the translation key each
/// is named by.
///
/// Chosen once and carried by every ship placed afterwards: a reader comparing
/// two ships' detection ranges wants the same circles on both.
pub const RANGE_CIRCLES: [(RangeCircle, &str); 6] = [
    (RangeCircle::Detection, "ui.renderer.context.detection"),
    (RangeCircle::MainBattery, "ui.renderer.context.main_battery"),
    (RangeCircle::SecondaryBattery, "ui.renderer.context.secondary"),
    (RangeCircle::Torpedo, "ui.renderer.context.torpedo"),
    (RangeCircle::Radar, "ui.renderer.context.radar"),
    (RangeCircle::Hydro, "ui.renderer.context.hydro"),
];

impl RangeCircle {
    /// Whether this circle is on, in a filter.
    pub fn is_on(self, filter: &AnnotationRangeFilter) -> bool {
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
    pub fn set(self, filter: &mut AnnotationRangeFilter, on: bool) {
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
pub struct BoardState {
    pub caps: Vec<BoardCapPoint>,
    /// With the ids the shapes are known by, so putting a step back is the same
    /// change to a session as it is to a board nobody else can see.
    pub annotations: Vec<Held>,
}

/// A ship waiting to be placed on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedShip {
    /// The param the ranges are read from.
    pub param_id: GameParamId,
    pub name: String,
    pub species: Species,
    /// Whether it is on the reader's side, which is what colours the marker.
    pub friendly: bool,
}

impl PlacedShip {
    /// The tool that places this ship.
    pub fn tool(&self) -> Tool {
        Tool::Ship { species: format!("{:?}", self.species), friendly: self.friendly, yaw: 0.0 }
    }
}

/// The ships whose names match what has been typed, at most [`SHIP_MATCHES`].
///
/// Empty until something is typed: every ship at once is not a choice.
pub fn matching_ships(catalog: &ShipCatalog, typed: &str) -> Vec<(Species, ShipEntry)> {
    let query = unidecode::unidecode(typed.trim()).to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut found = Vec::new();
    for nation in &catalog.nations {
        for class in &nation.classes {
            for ship in &class.ships {
                if !ship.search_name.contains(&query) {
                    continue;
                }
                found.push((class.species, ship.clone()));
                if found.len() >= SHIP_MATCHES {
                    return found;
                }
            }
        }
    }
    found
}

/// The range circles the placed ships show.
///
/// Read from each ship's own params, so what is drawn is what that ship sees
/// and shoots rather than a figure typed in. Without a version there is no
/// saying what a ship's ranges are: they are gated on it, and a circle drawn at
/// the wrong one reads as a fact.
pub fn ship_ranges(
    annotations: &[Annotation],
    metadata: &GameMetadataProvider,
    version: Option<Version>,
    space_size: f32,
) -> Vec<DrawCommand> {
    let Some(version) = version else { return Vec::new() };

    let mut circles = Vec::new();
    for annotation in annotations {
        let Annotation::Ship { config: Some(config), .. } = annotation else { continue };
        let Some(param) = GameParamProvider::game_param_by_id(metadata, config.param_id.into()) else {
            continue;
        };
        let Some(vehicle) = param.vehicle() else { continue };
        let hull = (!config.hull_name.is_empty()).then_some(config.hull_name.as_str());
        let ranges = vehicle.resolve_ranges(Some(metadata), hull, version);
        circles.extend(wt_collab_protocol::geometry::ship_range_commands(annotation, &ranges, space_size));
    }
    circles
}

/// How far a walk of the replay directory has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub read: usize,
    /// How many there are to read. `None` until the directory has been walked,
    /// which is not the same as a directory holding none.
    pub total: Option<usize>,
}

/// What a walk of the replay directory found.
#[derive(Debug)]
pub struct ScanOutcome {
    /// Every layout known, the ones it started with included.
    pub found: CapLayoutDb,
    /// The layouts nothing had before, which are the ones to save.
    pub fresh: Vec<CapLayout>,
    /// How many replays there were to read.
    pub total: usize,
}

/// Reads every replay under `dir` for the capture layouts of the modes they
/// were played in.
///
/// The layouts are what a mode picker offers, and the only place they can be
/// read from is a battle that used them. `report` hears each step; `cancelled`
/// stops the walk part way through.
pub fn scan_layouts(
    dir: &Path,
    metadata: &GameMetadataProvider,
    constants: &GameConstants,
    known: CapLayoutDb,
    cancelled: &AtomicBool,
    mut report: impl FnMut(ScanProgress),
) -> ScanOutcome {
    let mut found = known;
    let files = replay_files(dir);
    let total = files.len();
    report(ScanProgress { read: 0, total: Some(total) });

    let mut fresh = Vec::new();
    for (read, path) in files.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        if let Some(layout) = layout_of(path, metadata, constants, &found) {
            let kept = layout.clone();
            if found.insert(layout) {
                fresh.push(kept);
            }
        }
        report(ScanProgress { read: read + 1, total: Some(total) });
    }

    ScanOutcome { found, fresh, total }
}

/// Every replay under `dir`, in a stable order.
///
/// Symbolic links are not followed: a directory the reader picked may link back
/// into its own ancestry, which would walk for ever. A subdirectory that cannot
/// be read is skipped rather than losing the rest.
pub fn replay_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());

        let mut subdirectories = Vec::new();
        for entry in entries {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                subdirectories.push(path);
                continue;
            }
            // The battle in progress has no results and no layout to read; it
            // is also being written to while this reads it.
            if path.extension().is_some_and(|ext| ext == "wowsreplay")
                && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
            {
                found.push(path);
            }
        }
        // Reversed onto the stack, so the walk visits them in name order.
        pending.extend(subdirectories.into_iter().rev());
    }

    found.sort();
    found
}

/// The capture layout `path` was played on, when it is one nothing has yet.
///
/// The replay's own metadata says which layout it used, so a battle on a layout
/// already recorded is not parsed at all: that is the difference between a walk
/// that takes seconds and one that takes an hour.
pub fn layout_of(
    path: &Path,
    metadata: &GameMetadataProvider,
    constants: &GameConstants,
    known: &CapLayoutDb,
) -> Option<CapLayout> {
    let replay = wows_replays::ReplayFile::from_file(path).ok()?;
    let key = CapLayoutKey { map_id: replay.meta.mapId, scenario_config_id: replay.meta.scenarioConfigId };
    if known.contains(&key) {
        return None;
    }

    wows_replay_insights::cap_layout::extract_cap_layout_from_replay(path, metadata, Some(constants))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(index: usize, x: f32, z: f32, radius: f32) -> BoardCapPoint {
        BoardCapPoint {
            id: CapPointId::new(index as u64 + 1),
            index,
            world_x: x,
            world_z: z,
            radius,
            team: None,
            frozen: false,
        }
    }

    fn map(space_size: i32) -> MapInfo {
        MapInfo { space_size }
    }

    /// A press inside a zone moves it; one near its edge widens it; one well
    /// past it is not on it.
    #[test]
    fn a_press_near_the_edge_widens_rather_than_moves() {
        let map = map(1200);
        let band = band_in_world(&map);
        let caps = [cap(0, 0.0, 0.0, 100.0)];

        let at = |x: f32| WorldPos2D { x, z: 0.0 };
        assert_eq!(cap_under(&caps, at(0.0), &map), Some((0, CapDrag::Move)));
        assert_eq!(cap_under(&caps, at(50.0), &map), Some((0, CapDrag::Move)));
        assert_eq!(cap_under(&caps, at(100.0 - band / 2.0), &map), Some((0, CapDrag::Resize)));
        assert_eq!(cap_under(&caps, at(100.0 + band / 2.0), &map), Some((0, CapDrag::Resize)));
        assert_eq!(cap_under(&caps, at(100.0 + band * 2.0), &map), None);
    }

    /// A frozen cap is picked but never dragged.
    #[test]
    fn a_frozen_cap_is_picked_but_not_dragged() {
        let map = map(1200);
        let caps = [BoardCapPoint { frozen: true, ..cap(0, 0.0, 0.0, 100.0) }];

        assert_eq!(cap_under(&caps, WorldPos2D { x: 99.0, z: 0.0 }, &map), Some((0, CapDrag::None)));
    }

    /// A cap circle is talked about in kilometres, and the model holds it in
    /// the game's own thirty-metre units.
    #[test]
    fn a_zone_reads_in_kilometres() {
        assert!((radius_km(NEW_CAP_RADIUS) - 4.5).abs() < 0.01);
        assert!((radius_km(WORLD_UNITS_PER_KM) - 1.0).abs() < 0.001);
    }

    /// A zone reads by the team that holds it, and by nobody's colour when it
    /// is neutral; the team cycles nobody, ours, theirs.
    #[test]
    fn a_zone_is_coloured_by_who_holds_it() {
        assert_eq!(team_color(None), NEUTRAL_COLOR);
        assert_eq!(team_color(Some(TeamId::new(0))), FRIENDLY_COLOR);
        assert_eq!(team_color(Some(TeamId::new(1))), ENEMY_COLOR);
        assert_eq!(next_team(None), Some(TeamId::new(0)));
        assert_eq!(next_team(Some(TeamId::new(0))), Some(TeamId::new(1)));
        assert_eq!(next_team(Some(TeamId::new(1))), None);
    }

    /// A layout's neutral cap is stated as a negative team, which is an absence
    /// rather than a team to colour by.
    #[test]
    fn a_layouts_negative_team_reads_as_nobody() {
        let point = CapPointLayout {
            index: 0,
            position: wowsunpack::game_types::WorldPos2D { x: 1.0, z: 2.0 },
            radius: wowsunpack::game_params::types::BigWorldDistance::from(600.0),
            cp_type: wowsunpack::game_types::ControlPointType::Control,
            team_id: -1,
            initially_enabled: true,
        };
        assert_eq!(BoardCapPoint::from_layout(&point).team, None);

        let held = CapPointLayout { team_id: 1, ..point };
        assert_eq!(BoardCapPoint::from_layout(&held).team, Some(TeamId::new(1)));
    }

    /// A placed cap is lettered past the highest already there.
    #[test]
    fn a_placed_cap_takes_the_next_letter() {
        let caps = [cap(0, 0.0, 0.0, 1.0), cap(2, 0.0, 0.0, 1.0)];

        assert_eq!(next_cap_index(&caps), 3);
        assert_eq!(next_cap_index(&[]), 0);
        assert_eq!(cap(3, 0.0, 0.0, 1.0).letter(), "D");
    }

    /// A zone drawn at the centre of the map is hit by a press at the centre
    /// of the map, which is the failure a wrong coordinate space produces.
    #[test]
    fn a_press_at_the_centre_lands_on_a_zone_drawn_there() {
        let map = map(1200);
        let middle = MINIMAP_SIZE as f32 / 2.0;
        let centre = map.minimap_to_world_f32(middle, middle, MINIMAP_SIZE);
        let zone = cap(0, centre.x, centre.z, 150.0);

        let drawn = zone.command(&map);
        let DrawCommand::CapturePoint { pos, .. } = drawn else { panic!("a zone draws as a capture point") };
        assert!((pos.x - middle).abs() < 0.01 && (pos.y - middle).abs() < 0.01);

        let pressed = map.minimap_to_world_f32(pos.x, pos.y, MINIMAP_SIZE);
        let pressed = WorldPos2D { x: pressed.x, z: pressed.z };
        assert_eq!(cap_under(std::slice::from_ref(&zone), pressed, &map).map(|hit| hit.0), Some(0));
    }

    /// The resize tolerance is a screen distance, so it is the same handful of
    /// pixels on a small map and a large one.
    #[test]
    fn the_resize_band_is_the_same_on_screen_whatever_the_map() {
        for space in [800i32, 1200, 1600] {
            let map = map(space);
            let on_screen = map.world_distance_to_minimap(band_in_world(&map), MINIMAP_SIZE);
            assert!((on_screen - RESIZE_BAND_PX).abs() < 0.01, "{space}: {on_screen}px");
        }
    }

    /// Two maps that read alike are told apart by id, or by space without one.
    #[test]
    fn maps_that_read_alike_are_named_apart() {
        let maps = [
            MapChoice { map_id: Some(7), space: "spaces/a".into(), label: "Ocean".into() },
            MapChoice { map_id: None, space: "spaces/b".into(), label: "ocean".into() },
            MapChoice { map_id: Some(9), space: "spaces/c".into(), label: "Atoll".into() },
        ];

        assert_eq!(map_titles(&maps), ["Ocean (7)", "ocean (spaces/b)", "Atoll"]);
        assert!(maps[0].matches("Ocean (7)", "7"));
        assert!(maps[1].matches("ocean (spaces/b)", "SPACES/B"));
        assert!(!maps[2].matches("Atoll", "ocean"));
        assert_eq!(unique_map_for_space(&maps, "spaces/b"), Some(maps[1].clone()));
    }

    /// A frame narrower than its element is centred in it rather than
    /// stretched, and the two directions are each other's inverse.
    #[test]
    fn a_frame_is_centred_and_round_trips() {
        let extent = |width, height| Extent { width, height };
        let fit = Letterbox::fitting(ElementPos { x: 10.0, y: 20.0 }, extent(400.0, 200.0), extent(100.0, 100.0))
            .expect("a frame with extent");
        assert_eq!((fit.scale, fit.top, fit.left), (2.0, 20.0, 110.0));

        for (x, y) in [(0.0, 0.0), (50.0, 50.0), (99.0, 1.0)] {
            let at = FramePos { x, y };
            let on_screen = fit.to_element(at).expect("a point the frame shows");
            let back = fit.to_frame(on_screen).expect("over the map");
            assert!((back.x - at.x).abs() < 1e-3 && (back.y - at.y).abs() < 1e-3, "{at:?} -> {back:?}");
        }
        assert!(fit.to_frame(ElementPos { x: 20.0, y: 100.0 }).is_none(), "in the left margin");
        assert!(fit.to_element(FramePos { x: 100.0, y: 50.0 }).is_none(), "past the far edge");
        assert!(Letterbox::fitting(ElementPos { x: 0.0, y: 0.0 }, extent(400.0, 200.0), extent(0.0, 100.0)).is_none());
    }

    /// The walk finds replays in nested directories in name order and skips
    /// the battle in progress.
    #[test]
    fn the_walk_lists_replays_in_order() {
        let dir = std::env::temp_dir().join(format!("wtv-board-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("b")).expect("a nested directory");
        for file in ["a.wowsreplay", "temp.wowsreplay", "notes.txt", "b/c.wowsreplay"] {
            std::fs::write(dir.join(file), b"").expect("a file");
        }

        let found = replay_files(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(found, [dir.join("a.wowsreplay"), dir.join("b/c.wowsreplay")]);
    }
}

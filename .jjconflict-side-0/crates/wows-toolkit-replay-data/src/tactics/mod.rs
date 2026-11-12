//! A tactics board: a map with capture points on it, drawn away from any battle.
//!
//! The egui app calls this the Tactics Board (`replay/minimap_view/tactics.rs`). It is the same
//! map the replay viewport draws, rasterised through the same renderer, with the capture points of
//! one of the map's own game modes on it rather than a battle's.
//!
//! [`board::Board`] is the whole board as a front end drives it; the rest is what it is built
//! from.

pub mod board;
pub mod render;
pub mod scan;
pub mod ships;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_replay_insights::cap_layout::CapPointLayout;
use wows_replays::types::TeamId;
use wows_toolkit_services::collab::CapPointId;
use wows_toolkit_viewmodel::tactics::naming;
use wows_toolkit_viewmodel::tactics::preset;
use wowsunpack::game_types::WorldPos;
use wt_collab_client::drawing::Tool;
use wt_collab_client::protocol::WireCapPoint;
use wt_collab_client::types::Annotation;
use wt_collab_client::types::AnnotationRangeFilter;

use crate::load::GameDataCache;

/// One map the board can be set on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapChoice {
    /// The id the cap layouts are keyed by. `None` for a map found in the game's own art with no
    /// layout recorded for it yet, which can still be drawn on and which a later scan can give an
    /// id to.
    pub map_id: Option<u32>,
    /// The space name, such as `spaces/16_OC_bees_to_honey`.
    pub space: String,
    /// What the reader is shown.
    pub label: String,
}

impl MapChoice {
    /// What tells this map from every other in a list: two builds can name one space alike and
    /// give it different ids.
    pub fn key(&self) -> MapChoiceKey {
        MapChoiceKey { map_id: self.map_id, space: self.space.clone() }
    }
}

/// What identifies a [`MapChoice`] in a picker.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapChoiceKey {
    pub map_id: Option<u32>,
    pub space: String,
}

/// A map as the picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEntry {
    pub choice: MapChoice,
    /// The label, with the id or space after it where two maps read alike.
    pub title: String,
}

impl MapEntry {
    /// Whether `query` finds this entry: by its title, its label, its space or its id.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self.choice.label.to_lowercase().contains(&query)
            || self.choice.space.to_lowercase().contains(&query)
            || self.choice.map_id.is_some_and(|id| id.to_string().contains(&query))
    }
}

/// One game mode the chosen map has a recorded layout for.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeChoice {
    pub key: CapLayoutKey,
    pub label: String,
}

/// The picker value of a mode.
pub fn mode_value(key: &CapLayoutKey) -> String {
    format!("{}:{}", key.map_id, key.scenario_config_id)
}

/// A capture point on the board.
///
/// Editable, unlike the layout it was seeded from: moving a cap and widening its zone is the
/// question the board exists to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardCapPoint {
    /// What a session calls this zone. Its place in the list is not that: a peer's list is its
    /// own, and a zone moved by one has to be the same zone on every board.
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
    /// A frozen cap is where the game put it, so it is not dragged or deleted: the egui board
    /// holds its own the same way.
    pub frozen: bool,
}

/// A team as the layout, the preset file and the wire state it: a negative team is nobody's.
fn team_from_raw(raw: i64) -> Option<TeamId> {
    (raw >= 0).then(|| TeamId::new(raw))
}

/// The other way: a cap nobody holds is stated as a negative team, which is the form the egui
/// board and the wire use.
fn team_to_raw(team: Option<TeamId>) -> i64 {
    team.map(|team| team.raw()).unwrap_or(-1)
}

impl BoardCapPoint {
    pub fn from_layout(point: &CapPointLayout) -> Self {
        Self {
            index: point.index,
            world_x: point.position.x,
            world_z: point.position.z,
            radius: point.radius.value(),
            team: team_from_raw(point.team_id),
            frozen: true,
            id: CapPointId::fresh(),
        }
    }

    pub fn from_preset(saved: &preset::PresetCapPoint) -> Self {
        Self {
            index: saved.index,
            world_x: saved.world_x,
            world_z: saved.world_z,
            radius: saved.radius,
            team: team_from_raw(saved.team_id),
            frozen: saved.frozen,
            id: CapPointId::fresh(),
        }
    }

    pub fn to_preset(&self) -> preset::PresetCapPoint {
        preset::PresetCapPoint {
            index: self.index,
            world_x: self.world_x,
            world_z: self.world_z,
            radius: self.radius,
            team_id: team_to_raw(self.team),
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
            team_id: team_to_raw(self.team),
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
            team: team_from_raw(wire.team_id),
            frozen: wire.frozen,
        }
    }

    /// The letter this cap is labelled with.
    pub fn letter(&self) -> String {
        char::from(b'A' + (self.index.min(25) as u8)).to_string()
    }

    /// Which way the team picker reads it.
    pub fn side(&self) -> CapSide {
        match self.team.map(|team| team.raw()) {
            None => CapSide::Neutral,
            Some(0) => CapSide::Friendly,
            Some(_) => CapSide::Enemy,
        }
    }

    /// What the renderer draws it as.
    pub fn command(&self, map: &wows_minimap_renderer::MapInfo) -> DrawCommand {
        let at = map.world_to_minimap(WorldPos::new(self.world_x, 0.0, self.world_z), MINIMAP_SIZE);
        DrawCommand::CapturePoint {
            pos: MinimapPos { x: at.x, y: at.y },
            radius: map.world_distance_to_minimap(self.radius, MINIMAP_SIZE).round() as i32,
            color: team_color(self.team),
            alpha: CAP_FILL_ALPHA,
            label: self.letter(),
            progress: 0.0,
            invader_color: None,
            // No flag: a board's capture points are the reader's to place, and a base's flag
            // belongs to the battle that had one.
            flag_icon: None,
        }
    }
}

/// Who holds a capture point, as the board names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapSide {
    Neutral,
    Friendly,
    Enemy,
}

impl CapSide {
    /// The translation key the board labels it with.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Neutral => "ui.tactics.team_neutral",
            Self::Friendly => "ui.tactics.team_one",
            Self::Enemy => "ui.tactics.team_two",
        }
    }
}

/// How solid a capture zone's fill is drawn, as the battle draws its own.
const CAP_FILL_ALPHA: f32 = 0.25;

/// What a team's capture zone is coloured: the reader's own team, the enemy, and one nobody holds.
///
/// The same two the renderer paints a friendly and an enemy marker (`wt_collab_client::geometry`),
/// so one side reads as one colour whether it is a zone or a ship.
pub const FRIENDLY_COLOR: [u8; 3] = [76, 232, 170];
pub const ENEMY_COLOR: [u8; 3] = [254, 77, 42];
pub const NEUTRAL_COLOR: [u8; 3] = [255, 255, 255];

/// The board reads team zero as the reader's own, which is the team a replay records the
/// recording player on.
pub fn team_color(team: Option<TeamId>) -> [u8; 3] {
    match team.map(|team| team.raw()) {
        None => NEUTRAL_COLOR,
        Some(0) => FRIENDLY_COLOR,
        Some(_) => ENEMY_COLOR,
    }
}

/// How wide a world unit is: the game measures in 30-metre units, so a zone stated in kilometres
/// is that many thirty-metre steps.
pub const WORLD_UNITS_PER_KM: f32 = 1000.0 / 30.0;

/// How far one press moves a zone's width, in kilometres.
pub const RADIUS_STEP_KM: f32 = 0.5;

/// A zone's width in the units it is talked about in.
pub fn radius_km(radius: f32) -> f32 {
    radius / WORLD_UNITS_PER_KM
}

/// The same shape, drawn wider and dimmer: what sits under a picked one to say it is picked.
pub fn halo(annotation: &Annotation) -> Annotation {
    let mut halo = annotation.clone();
    let widened = |width: &mut f32| *width = (*width + HALO_WIDTH).max(HALO_WIDTH);
    let tinted = |color: &mut [u8; 4]| *color = HALO_INK;
    match &mut halo {
        // A ship marker is drawn from its species rather than stroked, so it has no width to
        // widen; it reads as picked by the handles on it.
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

/// A position in the element the map is drawn in, in CSS pixels from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementPos {
    pub x: f32,
    pub y: f32,
}

/// An element's extent in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementSize {
    pub width: f32,
    pub height: f32,
}

/// Where the drawn map landed in the element that shows it, and how much it was scaled to fit.
///
/// The frame is drawn to fit without stretching, so it is letterboxed: its top left corner is not
/// the element's own. Held apart from the board so the two directions cannot drift: a handle
/// placed by one and pressed by the other has to agree to the pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    /// The frame's own size, which is what a point is held inside.
    pub frame: (f32, f32),
}

impl Letterbox {
    /// `None` for a frame with no extent, which nothing can be placed against.
    pub fn fitting(element: ElementSize, frame: (f32, f32)) -> Option<Self> {
        if frame.0 <= 0.0 || frame.1 <= 0.0 {
            return None;
        }
        let scale = (element.width / frame.0).min(element.height / frame.1);
        Some(Self {
            left: (element.width - frame.0 * scale) / 2.0,
            top: (element.height - frame.1 * scale) / 2.0,
            scale,
            frame,
        })
    }

    /// An element position as a point in the frame. `None` for one in the margin beside a map
    /// that does not fill its element.
    pub fn to_frame(self, at: ElementPos) -> Option<(f32, f32)> {
        let (x, y) = ((at.x - self.left) / self.scale, (at.y - self.top) / self.scale);
        (x >= 0.0 && x < self.frame.0 && y >= 0.0 && y < self.frame.1).then_some((x, y))
    }

    /// A point in the frame as an element position. `None` for one the frame does not show,
    /// which has nowhere on screen to be.
    pub fn to_element(self, at: (f32, f32)) -> Option<ElementPos> {
        (at.0 >= 0.0 && at.0 < self.frame.0 && at.1 >= 0.0 && at.1 < self.frame.1)
            .then_some(ElementPos { x: self.left + at.0 * self.scale, y: self.top + at.1 * self.scale })
    }
}

/// How big a turning handle is drawn and how far above the shape it sits. The egui board's own
/// figures, in screen pixels at any zoom.
pub const HANDLE_RADIUS: f32 = 5.0;
pub const HANDLE_DISTANCE: f32 = 25.0;
pub const HANDLE_COLOR: [u8; 3] = [0xff, 0xff, 0x64];

/// How near a drawn shape a press has to land to pick it out, in map pixels. A line is one pixel
/// wide and nobody presses one exactly.
const SHAPE_REACH_PX: f32 = 10.0;

/// How much one pixel of wheel travel changes the zoom.
pub const ZOOM_PER_NOTCH: f32 = 0.004;

/// [`RESIZE_BAND_PX`] in world units, at this map's scale.
///
/// A zone's edge is a line on screen; the press that grabs it is measured in the world, so the
/// tolerance has to cross over.
pub fn band_in_world(map: &wows_minimap_renderer::MapInfo) -> f32 {
    let per_unit = map.world_distance_to_minimap(1.0, MINIMAP_SIZE);
    if per_unit > f32::EPSILON { RESIZE_BAND_PX / per_unit } else { RESIZE_BAND_PX }
}

/// Every map the board can be set on.
///
/// The recorded layouts first, then whatever else the loaded build ships art for: a map nobody
/// has a replay of can still be drawn on, it simply starts with no capture points.
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

/// The maps as the picker lists them, with the id or space after a label two maps share.
pub fn map_entries(maps: &[MapChoice]) -> Vec<MapEntry> {
    let mut label_counts = HashMap::new();
    for map in maps {
        *label_counts.entry(map.label.to_lowercase()).or_insert(0usize) += 1;
    }
    maps.iter()
        .cloned()
        .map(|choice| {
            let disambiguate = label_counts.get(&choice.label.to_lowercase()).is_some_and(|count| *count > 1);
            let title = if disambiguate {
                format!(
                    "{} ({})",
                    choice.label,
                    choice.map_id.map_or_else(|| choice.space.clone(), |id| id.to_string())
                )
            } else {
                choice.label.clone()
            };
            MapEntry { choice, title }
        })
        .collect()
}

/// The one map listed for `space`. `None` when there is none, or more than one to choose from.
pub fn unique_map_for_space(maps: &[MapChoice], space: &str) -> Option<MapChoice> {
    let mut matches = maps.iter().filter(|map| map.space == space);
    let map = matches.next()?.clone();
    matches.next().is_none().then_some(map)
}

/// The spaces the build ships a minimap for.
///
/// A dock scene is not a map anyone plays on, and a space with no minimap art has nothing to draw,
/// so neither is offered.
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
/// A map with no id has no recorded layout and so no modes, which is what a scan can change.
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
pub enum CapDrag {
    /// Picked out, and nothing more: a frozen cap is where the game put it.
    None,
    /// Moving it across the map.
    Move,
    /// Widening or narrowing its zone.
    Resize,
}

/// How near a zone's drawn edge a press has to land to widen it rather than move it, in map
/// pixels. A tolerance rather than a fraction of the radius: the reader is aiming at a line on
/// screen, and that line is the same thickness whatever the zone is.
pub const RESIZE_BAND_PX: f32 = 8.0;

/// The smallest a zone can be made, in world units, so one cannot be shrunk to nothing and lost.
/// The egui board holds its own to the same floor.
pub const MIN_CAP_RADIUS: f32 = 0.1 * WORLD_UNITS_PER_KM;
pub const MAX_CAP_RADIUS: f32 = 10.0 * WORLD_UNITS_PER_KM;

/// What a capture point added by hand starts as. A cap circle is about 5 km, which is about 167
/// world units; the egui board starts one at 150.
pub const NEW_CAP_RADIUS: f32 = 150.0;

/// What a board draws in until the reader picks otherwise, matching the ink and nib the replay
/// viewport starts with.
pub const DEFAULT_INK: [u8; 4] = [0xff, 0xd7, 0x3a, 0xff];
pub const DEFAULT_NIB: f32 = 2.0;

/// The inks the board offers, which are the replay viewport's own, with their translation keys.
pub const INKS: [([u8; 4], &str); 6] = [
    ([0xff, 0xd7, 0x3a, 0xff], "ui.tactics.ink_yellow"),
    ([0xe8, 0x73, 0x7b, 0xff], "ui.tactics.ink_red"),
    ([0x6f, 0xd9, 0x8a, 0xff], "ui.tactics.ink_green"),
    ([0x7f, 0xb4, 0xe8, 0xff], "ui.tactics.ink_blue"),
    ([0xe9, 0xe5, 0xdd, 0xff], "ui.tactics.ink_white"),
    ([0x1a, 0x1a, 0x18, 0xff], "ui.tactics.ink_black"),
];

/// How wide the nib can be drawn, which is the span the replay viewport holds its own to.
pub const MIN_NIB: f32 = 1.0;
pub const MAX_NIB: f32 = 8.0;

/// A drawing tool the board offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardTool {
    Freehand,
    Line,
    Arrow,
    Circle,
    Rectangle,
    Triangle,
    Measurement,
    Eraser,
}

impl BoardTool {
    /// The tools in the order the egui board takes them up.
    pub const ALL: [Self; 8] = [
        Self::Freehand,
        Self::Line,
        Self::Arrow,
        Self::Circle,
        Self::Rectangle,
        Self::Triangle,
        Self::Measurement,
        Self::Eraser,
    ];

    /// What the drawing state machine takes up.
    ///
    /// The shapes are drawn hollow: a board is read through, and a filled one hides the map it is
    /// about.
    pub fn tool(self) -> Tool {
        match self {
            Self::Freehand => Tool::Freehand,
            Self::Line => Tool::Line,
            Self::Arrow => Tool::Arrow,
            Self::Circle => Tool::Circle { filled: false },
            Self::Rectangle => Tool::Rectangle { filled: false },
            Self::Triangle => Tool::Triangle { filled: false },
            Self::Measurement => Tool::Measurement,
            Self::Eraser => Tool::Eraser,
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Freehand => "ui.renderer.annotations.tool_freehand",
            Self::Line => "ui.renderer.annotations.tool_line",
            Self::Arrow => "ui.renderer.annotations.tool_arrow",
            Self::Circle => "ui.renderer.annotations.tool_circle",
            Self::Rectangle => "ui.renderer.annotations.tool_rectangle",
            Self::Triangle => "ui.renderer.annotations.tool_triangle",
            Self::Measurement => "ui.renderer.annotations.tool_measurement",
            Self::Eraser => "ui.renderer.annotations.tool_eraser",
        }
    }

    /// The key that takes it up with the shortcut modifier held.
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Freehand => "2",
            Self::Line => "4",
            Self::Arrow => "1",
            Self::Circle => "5",
            Self::Rectangle => "6",
            Self::Triangle => "7",
            Self::Measurement => "M",
            Self::Eraser => "3",
        }
    }

    /// The tool a shortcut key takes up, the key as typed in lower case.
    pub fn for_shortcut(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.shortcut().eq_ignore_ascii_case(key))
    }
}

/// The modifier the board's chords are held with, as it is named on this platform.
pub fn shortcut_modifier_name() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" }
}

/// Which range circles a placed ship shows, with their translation keys.
///
/// Chosen once and carried by every ship placed afterwards: a reader comparing two ships'
/// detection ranges wants the same circles on both.
pub const RANGE_CIRCLES: [(RangeCircle, &str); 6] = [
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

/// How far a walk of the replay directory has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub read: usize,
    /// How many there are to read. `None` until the directory has been walked, which is not the
    /// same as a directory holding none.
    pub total: Option<usize>,
}

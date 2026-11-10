//! What is on a tactics board and how input changes it.
//!
//! Positions arrive in the frame surface's device pixels; the board turns them
//! into map pixels through the letterbox the producer draws with, and into the
//! world through the map's own measurements.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use image::RgbImage;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_toolkit_viewmodel::ship_catalog::ShipCatalog;
use wows_toolkit_viewmodel::ship_catalog::ShipEntry;
use wows_toolkit_viewmodel::tactics::PresetAnnotation;
use wows_toolkit_viewmodel::tactics::TacticsPreset;
use wows_toolkit_viewmodel::tactics::board;
use wows_toolkit_viewmodel::tactics::board::BoardCapPoint;
use wows_toolkit_viewmodel::tactics::board::BoardState;
use wows_toolkit_viewmodel::tactics::board::CapDrag;
use wows_toolkit_viewmodel::tactics::board::Letterbox;
use wows_toolkit_viewmodel::tactics::board::MapChoice;
use wows_toolkit_viewmodel::tactics::board::MapChoiceKey;
use wows_toolkit_viewmodel::tactics::board::ModeChoice;
use wows_toolkit_viewmodel::tactics::board::PlacedShip;
use wows_toolkit_viewmodel::tactics::board::RangeCircle;
use wows_toolkit_viewmodel::tactics::board::ScanProgress;
use wows_toolkit_viewmodel::tactics::naming;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;
use wt_collab_client::drawing::Drawing;
use wt_collab_client::drawing::Drawn;
use wt_collab_client::drawing::Selection;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;
use wt_collab_client::ids::BoardId;
use wt_collab_client::ids::CapPointId;
use wt_collab_client::ids::UserId;
use wt_collab_client::link::CollabLink;
use wt_collab_client::link::SessionBoard;
use wt_collab_client::types::Annotation;
use wt_collab_client::types::AnnotationRangeFilter;
use wt_collab_client::types::AnnotationShipConfig;

use crate::frames::Modifiers;
use crate::frames::PixelPoint;
use crate::frames::PixelSize;
use crate::frames::PointerButton;
use crate::tactics::error::TacticsError;
use crate::tactics::game::BoardGame;

/// How big a turning handle is drawn, how far above the shape it sits, and how
/// far from it a press still takes it, in points at any zoom.
pub const HANDLE_RADIUS: f32 = 5.0;
pub const HANDLE_DISTANCE: f32 = 25.0;
const HANDLE_REACH: f32 = HANDLE_RADIUS + 8.0;

/// How many pixels of wheel travel one scroll step stands for, which is about
/// a line of text: the zoom per notch is stated per pixel.
const WHEEL_STEP_PX: f32 = 20.0;

/// The view the frame surface reports: its size in device pixels and how many
/// of them make a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub size: PixelSize,
    pub scale: f32,
}

/// What the map area shows in place of the map, or that it shows the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStatus {
    PickAMap,
    NoGameData,
    NoArt,
    Drawing,
    Shown,
}

/// A message for the reader about something the board did on its own time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Counts up, so the same text said twice is shown twice.
    pub id: u32,
    pub text: String,
}

/// What the session lets this end do. Held so a change is noticed once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Permissions {
    may_steer: bool,
    locked: bool,
}

/// One entry of the mode picker: no mode, or a recorded layout.
#[derive(Clone, Debug, PartialEq)]
pub enum ModeEntry {
    Blank,
    Layout(ModeChoice),
}

impl ModeEntry {
    /// What the picker and the commands name this entry by.
    pub fn value(&self) -> String {
        match self {
            Self::Blank => "blank".to_owned(),
            Self::Layout(mode) => format!("{}:{}", mode.key.map_id, mode.key.scenario_config_id),
        }
    }
}

/// What the producer needs to draw one frame, taken under the board's lock and
/// drawn outside it.
pub struct RasterJob {
    pub game: Arc<dyn BoardGame>,
    pub space: String,
    /// Art a peer sent, for a map this build has none of.
    pub art: Option<Arc<RgbImage>>,
    pub map_info: MapInfo,
    pub board_id: BoardId,
    pub view: MapViewport,
    pub caps: Vec<BoardCapPoint>,
    /// Halos first, then the shapes, then the part-drawn one.
    pub annotations: Vec<Annotation>,
    pub viewport: Viewport,
    /// Where the turning handle and the foot of its stem sit, in device pixels.
    pub handle: Option<((f32, f32), (f32, f32))>,
}

/// What a frame is drawn as.
pub enum Frame {
    /// Nothing to draw: the surface is cleared.
    Empty,
    Map(Box<RasterJob>),
}

/// What [`Board::begin_announce`] hands the work that encodes the map art.
pub struct Announcement {
    pub game: Arc<dyn BoardGame>,
    pub map: MapChoice,
}

/// A tactics board.
pub struct Board {
    game: Option<Arc<dyn BoardGame>>,
    replays_dir: Option<PathBuf>,
    layouts: CapLayoutDb,
    maps: Vec<MapChoice>,
    map: Option<MapChoice>,
    /// The chosen map's measurements, read once when it is chosen.
    map_info: Option<MapInfo>,
    modes: Vec<ModeChoice>,
    mode: Option<CapLayoutKey>,
    caps: Vec<BoardCapPoint>,
    /// Which capture point the reader is working on, by its place in `caps`.
    selected: Option<usize>,

    /// Where the pointer is on the map, which the part-drawn shape is built
    /// against. `None` while it is off the map.
    pointer_at: Option<[f32; 2]>,
    dragging: Option<(usize, CapDrag)>,
    /// Whether the next click on the map places a capture point.
    adding: bool,
    drawing: Drawing,
    picked: Selection,
    /// A move in progress: where it began, and what was on the board then.
    moving: Option<([f32; 2], Vec<Annotation>)>,
    /// A turn in progress: which shape, and how it stood before the handle
    /// was taken. Read as a bearing from the shape's middle each move.
    turning: Option<(usize, Annotation)>,
    /// The pan in progress, and where the pointer was when it last moved.
    panning: Option<PixelPoint>,
    /// Whether the primary button is held over the frame.
    pressed: bool,

    collab: CollabLink,
    board_id: BoardId,
    peer_art: Option<Arc<RgbImage>>,
    peer_map_info: Option<MapInfo>,
    /// Who opened the board in the session. `None` outside one.
    owner: Option<UserId>,
    adopted_caps: Option<u64>,
    adopted_shapes: Option<u64>,
    permissions: Option<Permissions>,
    /// Whether the session has been told which map this board is on.
    announced: bool,
    /// The session's end of this board, held back until the session has been
    /// told the board exists: it drops anything sent for a board it does not
    /// hold.
    joining: Option<CollabLink>,

    presets: Vec<String>,
    scanning: Option<ScanProgress>,
    scan_cancelled: Arc<AtomicBool>,

    placing: Option<PlacedShip>,
    ship_catalog: Option<Arc<ShipCatalog>>,
    ship_query: String,
    matched_ships: Vec<(Species, ShipEntry)>,
    range_filter: AnnotationRangeFilter,

    view: MapViewport,
    viewport: Option<Viewport>,
    history: Vec<BoardState>,
    undone: Vec<BoardState>,

    /// Whether the last frame drawn showed the map, and whether the last
    /// attempt found no art for it.
    shown: bool,
    no_art: bool,
    notice: Option<Notice>,
    next_notice: u32,

    /// Whether the frame needs drawing again, and whether what the toolbar
    /// shows changed.
    dirty: bool,
    changed: bool,
}

impl Board {
    pub fn new(game: Option<Arc<dyn BoardGame>>, layouts: CapLayoutDb, replays_dir: Option<PathBuf>) -> Self {
        let maps = board::maps(
            &layouts,
            game.as_deref().map(BoardGame::metadata),
            game.as_deref().map(BoardGame::vfs),
        );
        let board_id = BoardId::fresh();
        Self {
            game,
            replays_dir,
            layouts,
            maps,
            map: None,
            map_info: None,
            modes: Vec::new(),
            mode: None,
            caps: Vec::new(),
            selected: None,
            pointer_at: None,
            dragging: None,
            adding: false,
            drawing: Drawing::new(board::DEFAULT_INK, board::DEFAULT_NIB),
            picked: Selection::default(),
            moving: None,
            turning: None,
            panning: None,
            pressed: false,
            collab: CollabLink::default().on_board(board_id),
            board_id,
            peer_art: None,
            peer_map_info: None,
            owner: None,
            adopted_caps: None,
            adopted_shapes: None,
            permissions: None,
            announced: false,
            joining: None,
            presets: wows_toolkit_viewmodel::tactics::list_preset_names(),
            scanning: None,
            scan_cancelled: Arc::new(AtomicBool::new(false)),
            placing: None,
            ship_catalog: None,
            ship_query: String::new(),
            matched_ships: Vec::new(),
            range_filter: AnnotationRangeFilter::default(),
            view: MapViewport::default(),
            viewport: None,
            history: Vec::new(),
            undone: Vec::new(),
            shown: false,
            no_art: false,
            notice: None,
            next_notice: 1,
            dirty: true,
            changed: true,
        }
    }

    /// The same board emptied, keeping the game, the layouts, the replay
    /// directory and the frame's size: what reopening it starts from.
    pub fn reopened(&self) -> Self {
        let mut fresh = Self::new(self.game.clone(), self.layouts.clone(), self.replays_dir.clone());
        fresh.viewport = self.viewport;
        fresh
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Asks for the frame to be drawn again, which also changes what the
    /// toolbar reads.
    fn redraw(&mut self) {
        self.dirty = true;
        self.changed = true;
    }

    fn notify(&mut self) {
        self.changed = true;
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { id: self.next_notice, text: text.into() });
        self.next_notice = self.next_notice.wrapping_add(1);
        self.notify();
    }
}

/// Reading what is on the board, for the toolbar.
impl Board {
    pub fn maps(&self) -> &[MapChoice] {
        &self.maps
    }

    pub fn map(&self) -> Option<&MapChoice> {
        self.map.as_ref()
    }

    pub fn mode_entries(&self) -> Vec<ModeEntry> {
        std::iter::once(ModeEntry::Blank).chain(self.modes.iter().cloned().map(ModeEntry::Layout)).collect()
    }

    pub fn mode_entry(&self) -> ModeEntry {
        match &self.mode {
            None => ModeEntry::Blank,
            Some(key) => self
                .modes
                .iter()
                .find(|mode| mode.key == *key)
                .cloned()
                .map_or(ModeEntry::Blank, ModeEntry::Layout),
        }
    }

    pub fn caps(&self) -> &[BoardCapPoint] {
        &self.caps
    }

    pub fn selected_cap(&self) -> Option<&BoardCapPoint> {
        self.selected.and_then(|at| self.caps.get(at))
    }

    pub fn adding(&self) -> bool {
        self.adding
    }

    /// The board changed under someone else's hand and wants drawing.
    pub fn redraw_requested(&mut self) {
        self.dirty = true;
    }

    pub fn tool(&self) -> &Tool {
        self.drawing.tool()
    }

    pub fn ink(&self) -> [u8; 4] {
        self.drawing.color()
    }

    pub fn nib(&self) -> f32 {
        self.drawing.width()
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn may_steer(&self) -> bool {
        self.collab.may_steer()
    }

    pub fn locked(&self) -> bool {
        self.collab.annotations_locked()
    }

    pub fn shape_count(&self) -> usize {
        self.collab.annotation_count()
    }

    pub fn picked(&self) -> &[usize] {
        self.picked.picked()
    }

    pub fn presets(&self) -> &[String] {
        &self.presets
    }

    pub fn scanning(&self) -> Option<ScanProgress> {
        self.scanning
    }

    pub fn placing(&self) -> Option<&PlacedShip> {
        self.placing.as_ref()
    }

    pub fn ship_query(&self) -> &str {
        &self.ship_query
    }

    pub fn matched_ships(&self) -> &[(Species, ShipEntry)] {
        &self.matched_ships
    }

    pub fn range_filter(&self) -> &AnnotationRangeFilter {
        &self.range_filter
    }

    /// The range filters of the ships already placed.
    pub fn ship_filters(&self) -> Vec<AnnotationRangeFilter> {
        self.shapes()
            .into_iter()
            .filter_map(|annotation| match annotation {
                Annotation::Ship { config: Some(config), .. } => Some(config.range_filter),
                _ => None,
            })
            .collect()
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    #[cfg(test)]
    pub fn view(&self) -> MapViewport {
        self.view
    }

    pub fn viewport(&self) -> Option<Viewport> {
        self.viewport
    }

    pub fn has_map_info(&self) -> bool {
        self.current_map_info().is_some()
    }

    /// Whether this board is one a peer opened rather than this reader.
    pub fn is_a_peers(&self) -> bool {
        self.owner.is_some_and(|owner| self.collab.my_user_id() != Some(owner))
    }

    pub fn status(&self) -> MapStatus {
        if self.map.is_none() {
            return MapStatus::PickAMap;
        }
        if self.game.is_none() {
            return MapStatus::NoGameData;
        }
        if self.no_art {
            return MapStatus::NoArt;
        }
        if !self.shown {
            return MapStatus::Drawing;
        }
        MapStatus::Shown
    }

    /// Whether a drawing tool is in hand, which takes the pointer away from the
    /// capture points.
    pub fn has_tool(&self) -> bool {
        !self.locked() && *self.drawing.tool() != Tool::None
    }

    /// Capture points remain editable by peers until the host locks
    /// annotations.
    pub fn can_edit_caps(&self) -> bool {
        self.collab.may_steer() || !self.collab.annotations_locked()
    }

    /// The shapes drawn on the board, held by the link: the session's list in
    /// a session and the link's own outside one.
    pub fn shapes(&self) -> Vec<Annotation> {
        self.collab.annotations()
    }
}

/// Where a position on the frame lands.
impl Board {
    fn letterbox(&self) -> Option<Letterbox> {
        let viewport = self.viewport?;
        let side = MINIMAP_SIZE as f32;
        Letterbox::fitting(
            (0.0, 0.0),
            (viewport.size.width as f32, viewport.size.height as f32),
            (side, side),
        )
    }

    /// Where a frame position falls in the drawn map, before the zoom is
    /// undone. `None` before the map has been drawn, and in the margin beside
    /// it.
    fn drawn_point(&self, at: PixelPoint) -> Option<(f32, f32)> {
        if !self.shown {
            return None;
        }
        self.letterbox()?.to_frame((at.x, at.y))
    }

    /// Where a frame position falls on the map, in the map's own pixels.
    fn map_point(&self, at: PixelPoint) -> Option<(f32, f32)> {
        let (x, y) = self.drawn_point(at)?;
        Some(self.view.to_map(x, y))
    }

    /// Where a map point lands on the frame, `None` where the zoom has
    /// scrolled it off.
    fn frame_point(&self, at: (f32, f32)) -> Option<(f32, f32)> {
        self.letterbox()?.to_element((self.view.x(at.0), self.view.y(at.1)))
    }

    fn current_map_info(&self) -> Option<MapInfo> {
        self.peer_map_info.clone().or_else(|| self.map_info.clone())
    }

    /// Where a frame position falls in the world.
    fn world_point(&self, at: PixelPoint) -> Option<(f32, f32)> {
        let (x, y) = self.map_point(at)?;
        let map = self.current_map_info()?;
        let world = map.minimap_to_world_f32(x, y, MINIMAP_SIZE);
        Some((world.x, world.z))
    }

    fn cap_under(&self, at: PixelPoint) -> Option<(usize, CapDrag)> {
        board::cap_under(&self.caps, self.world_point(at)?, &self.current_map_info()?)
    }

    fn shape_under(&self, at: PixelPoint) -> Option<usize> {
        let at = self.map_point(at)?;
        wt_collab_client::drawing::nearest_within(&self.shapes(), [at.0, at.1], board::SHAPE_REACH_PX)
    }

    fn scale(&self) -> f32 {
        self.viewport.map_or(1.0, |viewport| viewport.scale)
    }

    /// Where a shape's turning handle sits on the frame, and the foot of its
    /// stem: a fixed distance above the shape on screen, so the handle keeps
    /// its size and reach at every zoom.
    fn rotation_handle(&self, annotation: &Annotation) -> Option<((f32, f32), (f32, f32))> {
        let [left, top, right, _] = wt_collab_client::drawing::annotation_bounds(annotation);
        let anchor = self.frame_point(((left + right) / 2.0, top))?;

        Some(((anchor.0, anchor.1 - HANDLE_DISTANCE * self.scale()), anchor))
    }

    /// The one picked shape that can be turned, with its handle.
    fn turnable(&self) -> Option<(usize, Annotation, ((f32, f32), (f32, f32)))> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotation_at(index)?;
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let handle = self.rotation_handle(&annotation)?;

        Some((index, annotation, handle))
    }

    fn handle_under(&self, at: PixelPoint) -> Option<(usize, Annotation)> {
        let (index, annotation, (handle, _)) = self.turnable()?;
        let away = (at.x - handle.0).hypot(at.y - handle.1);

        (away < HANDLE_REACH * self.scale()).then_some((index, annotation))
    }
}

/// Setting the board up.
impl Board {
    /// The frame's size changed.
    pub fn resize(&mut self, viewport: Viewport) {
        if self.viewport == Some(viewport) {
            return;
        }
        self.viewport = Some(viewport);
        self.redraw();
    }

    /// What the producer found drawing the last frame.
    pub fn drew(&mut self, shown: bool, no_art: bool) {
        if self.shown == shown && self.no_art == no_art {
            return;
        }
        self.shown = shown;
        self.no_art = no_art;
        self.notify();
    }

    /// Sets the board on a map, which replaces whatever was on it.
    pub fn set_map_value(&mut self, value: &str) -> Result<Option<Announcement>, TacticsError> {
        let titles = board::map_titles(&self.maps);
        let found = self
            .maps
            .iter()
            .zip(&titles)
            .find(|(map, _)| map_value(map) == value)
            .map(|(map, _)| map.clone())
            .ok_or_else(|| TacticsError::UnknownMap { value: value.to_owned() })?;

        Ok(self.set_map(found))
    }

    pub fn set_map(&mut self, map: MapChoice) -> Option<Announcement> {
        if !self.collab.may_steer() || self.map.as_ref() == Some(&map) {
            return None;
        }
        let metadata = self.game.as_deref().map(BoardGame::metadata);
        self.modes = board::modes(&self.layouts, map.map_id, metadata);
        // The first mode the map has, so a board opens with capture points on
        // it rather than empty.
        self.mode = self.modes.first().map(|mode| mode.key.clone());
        let caps = self.mode.as_ref().map(|key| board::caps_of(&self.layouts, key)).unwrap_or_default();
        self.set_caps(caps);
        self.selected = None;
        self.map_info = self
            .game
            .as_deref()
            .and_then(|game| wows_minimap_renderer::assets::load_map_info(&map.space, game.vfs()));
        self.map = Some(map);
        self.shown = false;
        self.no_art = false;
        self.view = MapViewport::default();
        self.redraw();

        // A board is announced with the map it is on, so a different map is a
        // different announcement under the same board.
        self.announce_again()
    }

    /// Sets the board on one of the map's modes, or takes the mode off.
    pub fn set_mode_value(&mut self, value: &str) -> Result<(), TacticsError> {
        let entry = self
            .mode_entries()
            .into_iter()
            .find(|entry| entry.value() == value)
            .ok_or_else(|| TacticsError::UnknownMode { value: value.to_owned() })?;
        match entry {
            ModeEntry::Blank => self.set_blank_mode(),
            ModeEntry::Layout(mode) if self.mode.as_ref() != Some(&mode.key) => self.set_mode(mode.key),
            // The mode already set: the picker confirmed it again.
            ModeEntry::Layout(_) => {}
        }
        Ok(())
    }

    fn set_mode(&mut self, key: CapLayoutKey) {
        if !self.collab.may_steer() {
            return;
        }
        let caps = board::caps_of(&self.layouts, &key);
        self.set_caps(caps);
        self.selected = None;
        self.mode = Some(key);
        self.redraw();
    }

    pub fn set_blank_mode(&mut self) {
        if !self.collab.may_steer() || (self.mode.is_none() && self.caps.is_empty()) {
            return;
        }
        self.mode = None;
        self.set_caps(Vec::new());
        self.selected = None;
        self.redraw();
    }

    /// Takes the layouts a walk found, growing the map list and the chosen
    /// map's modes.
    pub fn adopt_layouts(&mut self, layouts: CapLayoutDb) {
        self.layouts = layouts;
        let metadata = self.game.as_deref().map(BoardGame::metadata);
        self.maps = board::maps(&self.layouts, metadata, self.game.as_deref().map(BoardGame::vfs));
        if let Some(map) = self.map.as_mut() {
            // A map the walk has just recorded a layout for now has an id,
            // which is what its modes are looked up by.
            if map.map_id.is_none() {
                map.map_id = board::unique_map_for_space(&self.maps, &map.space).and_then(|found| found.map_id);
            }
            self.modes = board::modes(&self.layouts, map.map_id, metadata);
        }
        self.notify();
    }
}

/// Saved boards.
impl Board {
    /// Saves the board under `name`. A board with no map names nothing to open
    /// again.
    pub fn save_preset(&mut self, name: &str) -> Result<(), TacticsError> {
        if !self.collab.may_steer() {
            return Ok(());
        }
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(TacticsError::PresetNeedsName);
        }
        let Some(map) = self.map.clone() else { return Err(TacticsError::PresetNeedsMap) };

        let saved = TacticsPreset {
            name: name.clone(),
            map_name: map.space,
            // The file states a map with no recorded layout as the id zero,
            // which is the form the egui board writes and reads.
            map_id: map.map_id.unwrap_or(0),
            cap_points: self.caps.iter().map(BoardCapPoint::to_preset).collect(),
            annotations: self.shapes().iter().map(PresetAnnotation::from_annotation).collect(),
        };
        wows_toolkit_viewmodel::tactics::save_preset(&saved)
            .map_err(|source| TacticsError::PresetSave { name: name.clone(), source })?;
        self.presets = wows_toolkit_viewmodel::tactics::list_preset_names();
        self.say(format!("Saved {name}"));
        Ok(())
    }

    /// Opens a saved board.
    pub fn load_preset(&mut self, name: &str) -> Result<Option<Announcement>, TacticsError> {
        if !self.collab.may_steer() || self.collab.annotations_locked() {
            return Ok(None);
        }
        let read = wows_toolkit_viewmodel::tactics::load_preset(name)
            .map_err(|source| TacticsError::PresetLoad { name: name.to_owned(), source })?;

        // Matched by space name rather than by map id: a board saved from a map
        // nothing has a layout for carries the id zero, which names no map.
        let metadata = self.game.as_deref().map(BoardGame::metadata);
        let map_id = (read.map_id != 0).then_some(read.map_id);
        let matching = self
            .maps
            .iter()
            .find(|map| map.space == read.map_name && map.map_id == map_id)
            .cloned()
            .or_else(|| if map_id.is_none() { board::unique_map_for_space(&self.maps, &read.map_name) } else { None });
        let map = matching.unwrap_or_else(|| MapChoice {
            map_id,
            label: naming::map_label(&read.map_name, metadata),
            space: read.map_name.clone(),
        });
        if !self.maps.iter().any(|candidate| MapChoiceKey::from(candidate) == MapChoiceKey::from(&map)) {
            self.maps.push(map.clone());
        }
        self.modes = board::modes(&self.layouts, map.map_id, metadata);
        // The saved capture points stand, whatever mode the map has: they are
        // what the reader put there.
        self.mode = None;
        if self.map.as_ref() != Some(&map) {
            self.map_info = self
                .game
                .as_deref()
                .and_then(|game| wows_minimap_renderer::assets::load_map_info(&map.space, game.vfs()));
            self.shown = false;
            self.no_art = false;
        }
        self.map = Some(map);
        let caps = read.cap_points.iter().map(BoardCapPoint::from_preset).collect();
        self.set_caps(caps);
        self.replace_shapes(read.annotations.iter().map(PresetAnnotation::to_annotation).collect());
        self.selected = None;
        self.adding = false;
        self.redraw();

        Ok(self.announce_again())
    }

    pub fn delete_preset(&mut self, name: &str) -> Result<(), TacticsError> {
        if !self.collab.may_steer() {
            return Ok(());
        }
        wows_toolkit_viewmodel::tactics::delete_preset(name)
            .map_err(|source| TacticsError::PresetDelete { name: name.to_owned(), source })?;
        self.presets = wows_toolkit_viewmodel::tactics::list_preset_names();
        self.notify();
        Ok(())
    }

    /// Restores the selected mode's capture points and removes the drawing.
    pub fn reset_board(&mut self) {
        if !self.collab.may_steer() || self.collab.annotations_locked() {
            return;
        }
        let caps = self.mode.as_ref().map(|key| board::caps_of(&self.layouts, key)).unwrap_or_default();
        let caps_match = self.caps.len() == caps.len()
            && self.caps.iter().zip(&caps).all(|(current, default)| current.same_layout(default));
        if caps_match && self.collab.annotation_count() == 0 {
            return;
        }

        self.remember();
        self.set_caps(caps);
        self.replace_shapes(Vec::new());
        self.selected = None;
        self.adding = false;
        self.redraw();
    }
}

/// The replay walk.
impl Board {
    /// Starts a walk, handing back what it reads with. `None` when one is
    /// running already.
    pub fn begin_scan(&mut self) -> Result<Option<ScanStart>, TacticsError> {
        if self.scanning.is_some() {
            return Ok(None);
        }
        let dir = self.replays_dir.clone().ok_or(TacticsError::NoReplaysDir)?;
        let game = self.game.clone().ok_or(TacticsError::NoGameData)?;
        self.scan_cancelled = Arc::new(AtomicBool::new(false));
        self.scanning = Some(ScanProgress { read: 0, total: None });
        self.notify();

        Ok(Some(ScanStart { dir, game, known: self.layouts.clone(), cancelled: Arc::clone(&self.scan_cancelled) }))
    }

    /// A step of the walk. Ignored once the walk was replaced or stopped.
    pub fn scan_step(&mut self, cancelled: &Arc<AtomicBool>, step: ScanProgress) {
        if !Arc::ptr_eq(cancelled, &self.scan_cancelled) || self.scanning.is_none() {
            return;
        }
        self.scanning = Some(step);
        self.notify();
    }

    pub fn finish_scan(&mut self, cancelled: &Arc<AtomicBool>, found: CapLayoutDb, added: usize, total: usize) {
        if !Arc::ptr_eq(cancelled, &self.scan_cancelled) {
            return;
        }
        self.scanning = None;
        self.adopt_layouts(found);
        self.say(format!("Read {total} replays and found {added} new layouts"));
    }
}

/// What a replay walk reads with.
pub struct ScanStart {
    pub dir: PathBuf,
    pub game: Arc<dyn BoardGame>,
    pub known: CapLayoutDb,
    pub cancelled: Arc<AtomicBool>,
}

/// History.
impl Board {
    fn board_state(&self) -> BoardState {
        BoardState { caps: self.caps.clone(), annotations: self.collab.annotations_held() }
    }

    /// Remembers what is on the board, before changing it. A change after an
    /// undo is the new end of the line, so what was undone is dropped.
    fn remember(&mut self) {
        self.history.push(self.board_state());
        if self.history.len() > board::HISTORY_DEPTH {
            self.history.remove(0);
        }
        self.undone.clear();
    }

    pub fn undo(&mut self) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(was) = self.history.pop() else { return };
        self.undone.push(self.board_state());
        self.adopt(was);
    }

    pub fn redo(&mut self) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(again) = self.undone.pop() else { return };
        self.history.push(self.board_state());
        self.adopt(again);
    }

    fn adopt(&mut self, state: BoardState) {
        self.set_caps(state.caps);
        self.collab.restore(&state.annotations);
        // The selection named a place in a list that has just been replaced.
        self.selected = None;
        self.dragging = None;
        self.picked.clear();
        self.redraw();
    }
}

/// Capture points.
impl Board {
    /// Puts `caps` on the board, telling the session what changed by
    /// difference: re-sending an unchanged one would have every peer redraw.
    fn set_caps(&mut self, caps: Vec<BoardCapPoint>) {
        let was: Vec<CapPointId> = self.caps.iter().map(|cap| cap.id).collect();
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

    fn report_cap(&self, index: usize) {
        if let Some(cap) = self.caps.get(index) {
            self.collab.set_cap(cap.to_wire());
        }
    }

    pub fn set_adding(&mut self, adding: bool) {
        if !self.can_edit_caps() {
            return;
        }
        self.adding = adding;
        self.notify();
    }

    fn add_cap_at(&mut self, world: (f32, f32)) {
        if !self.can_edit_caps() {
            return;
        }
        self.remember();
        let index = board::next_cap_index(&self.caps);
        self.caps.push(BoardCapPoint {
            id: CapPointId::fresh(),
            index,
            world_x: world.0,
            world_z: world.1,
            radius: board::NEW_CAP_RADIUS,
            team: None,
            frozen: false,
        });
        self.report_cap(self.caps.len() - 1);
        self.selected = Some(self.caps.len() - 1);
        self.adding = false;
        self.redraw();
    }

    /// Drops the capture point the reader is working on. A frozen one stays:
    /// it is where the game put it.
    pub fn remove_selected(&mut self) {
        if !self.can_edit_caps() {
            return;
        }
        let Some(at) = self.selected else { return };
        if self.caps.get(at).is_none_or(|cap| cap.frozen) {
            return;
        }
        self.remember();
        self.selected = None;
        // The letters the others carry stand: renumbering would rename them.
        let gone = self.caps.remove(at);
        self.collab.remove_cap(gone.id);
        self.redraw();
    }

    /// Widens or narrows the selected zone, in kilometres.
    pub fn step_selected_radius(&mut self, by_km: f32) {
        if !self.can_edit_caps() {
            return;
        }
        let Some(at) = self.selected.filter(|at| *at < self.caps.len()) else { return };
        self.remember();
        let cap = &mut self.caps[at];
        cap.radius =
            (cap.radius + by_km * board::WORLD_UNITS_PER_KM).clamp(board::MIN_CAP_RADIUS, board::MAX_CAP_RADIUS);
        self.report_cap(at);
        self.redraw();
    }

    pub fn cycle_selected_team(&mut self) {
        if !self.can_edit_caps() {
            return;
        }
        let Some(at) = self.selected.filter(|at| *at < self.caps.len()) else { return };
        self.remember();
        let cap = &mut self.caps[at];
        cap.team = board::next_team(cap.team);
        self.report_cap(at);
        self.redraw();
    }

    /// Takes every capture point the reader added off the board.
    pub fn clear_caps(&mut self) {
        if !self.can_edit_caps() || !self.caps.iter().any(|cap| !cap.frozen) {
            return;
        }
        self.remember();
        let selected_id = self.selected_cap().map(|cap| cap.id);
        let removed: Vec<CapPointId> = self.caps.iter().filter(|cap| !cap.frozen).map(|cap| cap.id).collect();
        self.caps.retain(|cap| cap.frozen);
        for id in removed {
            self.collab.remove_cap(id);
        }
        self.selected = selected_id.and_then(|id| self.caps.iter().position(|cap| cap.id == id));
        self.redraw();
    }
}

/// Drawing.
impl Board {
    /// Takes up a tool, or puts it down again when it is already in hand.
    pub fn set_tool(&mut self, tool: Tool) {
        if self.collab.annotations_locked() {
            return;
        }
        let putting_down = *self.drawing.tool() == tool;
        let next = if putting_down { Tool::None } else { tool };
        self.drawing.set_tool(next);
        // A tool in hand is not also placing capture points.
        if !putting_down {
            self.adding = false;
            self.selected = None;
        }
        self.redraw();
    }

    /// Takes up a tool from its shortcut: never puts it down, and drops what
    /// was picked out.
    pub fn set_shortcut_tool(&mut self, tool: Tool) {
        if self.collab.annotations_locked() {
            return;
        }
        self.drawing.set_tool(tool);
        self.adding = false;
        self.selected = None;
        self.picked.clear();
        self.redraw();
    }

    pub fn set_ink(&mut self, ink: [u8; 4]) {
        if self.collab.annotations_locked() {
            return;
        }
        self.drawing.set_color(ink);
        self.notify();
    }

    pub fn step_nib(&mut self, by: f32) {
        if self.collab.annotations_locked() {
            return;
        }
        let stepped = (self.drawing.width() + by).clamp(board::MIN_NIB, board::MAX_NIB);
        self.drawing.set_width(stepped);
        self.notify();
    }

    pub fn clear_annotations(&mut self) {
        if self.collab.annotations_locked() || self.collab.annotation_count() == 0 {
            return;
        }
        self.remember();
        self.replace_shapes(Vec::new());
        self.redraw();
    }

    pub fn erase_picked(&mut self) {
        if self.collab.annotations_locked() || self.picked.is_empty() {
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
        self.redraw();
    }

    /// What a delete is about: a drawn shape if one is picked, the capture
    /// point otherwise.
    pub fn delete(&mut self) {
        if self.picked.is_empty() {
            self.remove_selected();
        } else {
            self.erase_picked();
        }
    }

    fn replace_shapes(&mut self, shapes: Vec<Annotation>) {
        self.collab.replace_annotations(shapes);
        self.drawing.cancel();
        self.picked.clear();
        self.panning = None;
        self.dragging = None;
        self.moving = None;
        self.turning = None;
    }

    /// Names the ship a placement just put down, before it goes over: in a
    /// session the add is a message, and the list it would be named in has
    /// not caught up yet.
    fn add_shape(&mut self, mut annotation: Annotation) {
        if let Annotation::Ship { config, .. } = &mut annotation
            && let Some(placed) = self.placing.clone()
        {
            *config = Some(AnnotationShipConfig {
                param_id: placed.param_id,
                ship_name: placed.name,
                range_filter: self.range_filter.clone(),
                // Stock hull and no modifiers, which is where the egui
                // chooser leaves it.
                ..Default::default()
            });
        }
        self.collab.add_annotation(annotation);
    }

    fn stroke(&mut self, stroke: Stroke) {
        if self.collab.annotations_locked() {
            return;
        }
        match self.drawing.handle(stroke, &self.shapes()) {
            Some(Drawn::Added(annotation)) => self.add_shape(annotation),
            Some(Drawn::Erased(index)) => self.collab.erase_annotation(index),
            None => {}
        }
        self.redraw();
    }
}

/// Ships and their ranges.
impl Board {
    fn ships(&mut self) -> Option<Arc<ShipCatalog>> {
        if let Some(catalog) = &self.ship_catalog {
            return Some(Arc::clone(catalog));
        }
        let game = self.game.as_deref()?;
        let catalog = Arc::new(ShipCatalog::build(game.metadata()));
        self.ship_catalog = Some(Arc::clone(&catalog));
        Some(catalog)
    }

    pub fn search_ships(&mut self, typed: &str) {
        self.ship_query = typed.to_owned();
        self.matched_ships = match self.ships() {
            Some(catalog) => board::matching_ships(&catalog, typed),
            None => Vec::new(),
        };
        self.notify();
    }

    /// Takes up the matched ship at `index`, so the next click places it.
    pub fn pick_ship(&mut self, index: u32) -> Result<(), TacticsError> {
        if self.collab.annotations_locked() {
            return Ok(());
        }
        let (species, ship) =
            self.matched_ships.get(index as usize).cloned().ok_or(TacticsError::UnknownShip { index })?;
        let game = self.game.clone().ok_or(TacticsError::NoGameData)?;
        let param = game.metadata().game_param_by_index(&ship.param_index).ok_or(TacticsError::UnknownShip { index })?;

        let friendly = self.placing.as_ref().is_none_or(|placed| placed.friendly);
        let placed = PlacedShip { param_id: param.id().raw(), name: ship.display_name, species, friendly };
        self.drawing.set_tool(placed.tool());
        self.placing = Some(placed);
        self.adding = false;
        self.selected = None;
        self.matched_ships.clear();
        self.ship_query.clear();
        self.redraw();
        Ok(())
    }

    pub fn set_placing_friendly(&mut self, friendly: bool) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(placed) = self.placing.as_mut() else { return };
        placed.friendly = friendly;
        let tool = placed.tool();
        self.drawing.set_tool(tool);
        self.notify();
    }

    /// Turns one circle on or off, for the ships placed and the ones to come.
    pub fn set_range_circle(&mut self, circle: RangeCircle, on: bool) {
        self.set_range_circles(&[circle], on);
    }

    pub fn set_all_range_circles(&mut self, on: bool) {
        let all: Vec<RangeCircle> = board::RANGE_CIRCLES.iter().map(|(circle, _)| *circle).collect();
        self.set_range_circles(&all, on);
    }

    fn set_range_circles(&mut self, circles: &[RangeCircle], on: bool) {
        if self.collab.annotations_locked() {
            return;
        }
        for circle in circles {
            circle.set(&mut self.range_filter, on);
        }
        for (at, mut annotation) in self.shapes().into_iter().enumerate() {
            let Annotation::Ship { config: Some(config), .. } = &mut annotation else { continue };
            for circle in circles {
                circle.set(&mut config.range_filter, on);
            }
            self.collab.update_annotation(at, annotation);
        }
        self.redraw();
    }
}

/// The view.
impl Board {
    fn zoom_about(&mut self, notches: f32, at: PixelPoint) {
        let Some(drawn) = self.drawn_point(at) else { return };
        let zoom = self.view.zoom() * (1.0 + notches * board::ZOOM_PER_NOTCH);
        let view = self.view.zoomed_about(zoom, drawn);
        if view == self.view {
            return;
        }
        self.view = view;
        self.redraw();
    }

    pub fn reset_view(&mut self) {
        if self.view == MapViewport::default() {
            return;
        }
        self.view = MapViewport::default();
        self.redraw();
    }
}

/// Pointer input, in the frame's device pixels.
impl Board {
    pub fn pointer_down(&mut self, at: PixelPoint, button: PointerButton, clicks: u32, modifiers: Modifiers) {
        match button {
            PointerButton::Primary => self.primary_down(at, clicks, modifiers),
            PointerButton::Secondary => self.secondary_down(at),
            // The other buttons do nothing on a board.
            PointerButton::Middle | PointerButton::Other(_) => {}
        }
    }

    fn primary_down(&mut self, at: PixelPoint, clicks: u32, modifiers: Modifiers) {
        self.pressed = true;

        // A tool in hand takes the drag: a reader drawing a line is not asking
        // to move the capture point under it.
        if self.has_tool() {
            let Some(point) = self.map_point(at) else { return };
            self.remember();
            self.stroke(Stroke::Began { at: [point.0, point.1] });
            return;
        }
        if self.adding && self.can_edit_caps() {
            let Some(world) = self.world_point(at) else { return };
            self.add_cap_at(world);
            return;
        }
        // A double click puts the whole map back, which is how the replay
        // viewport is reset too.
        if clicks >= 2 {
            self.reset_view();
            return;
        }
        // The handle takes the drag before anything else: it sits above the
        // shape, over map nobody is reaching for.
        if !self.collab.annotations_locked()
            && let Some((index, annotation)) = self.handle_under(at)
        {
            self.remember();
            self.turning = Some((index, annotation));
            return;
        }

        if !self.collab.annotations_locked()
            && let Some(point) = self.map_point(at)
        {
            // A shape already picked out is dragged as a whole.
            if !self.picked.is_empty() && self.shape_under(at).is_some_and(|found| self.picked.picked().contains(&found))
            {
                self.remember();
                self.moving = Some(([point.0, point.1], self.shapes()));
                return;
            }
            // Otherwise a press on one picks it out, and ctrl adds it to what
            // is already picked.
            let was: Vec<usize> = self.picked.picked().to_vec();
            self.picked.click(&self.shapes(), [point.0, point.1], modifiers.control || modifiers.meta);
            if self.picked.picked() != was.as_slice() {
                self.redraw();
                return;
            }
        }

        match self.can_edit_caps().then(|| self.cap_under(at)).flatten() {
            Some((index, what)) => {
                self.selected = Some(index);
                if what != CapDrag::None {
                    // Taken as the drag begins: all of the drag is one change.
                    self.remember();
                }
                self.dragging = Some((index, what));
            }
            // A drag past every zone moves the map; the click alone puts the
            // selection down.
            None => {
                self.selected = None;
                self.panning = Some(at);
            }
        }
        self.redraw();
    }

    /// The secondary button picks out the capture point under it, which is
    /// what the context menu then speaks of.
    fn secondary_down(&mut self, at: PixelPoint) {
        if let Some((index, _)) = self.cap_under(at)
            && self.selected != Some(index)
        {
            self.selected = Some(index);
            self.redraw();
        }
    }

    pub fn pointer_moved(&mut self, at: PixelPoint) {
        if self.collab.annotations_locked() {
            self.turning = None;
            self.moving = None;
            if self.drawing.is_drawing() {
                self.drawing.cancel();
            }
        }
        // A drag released off the frame never reports its release, so a
        // pointer moving with nothing held has let go of whatever it had.
        if !self.pressed {
            self.release();
        }

        let point = self.map_point(at);
        let moved_over = point.map(|(x, y)| [x, y]);
        if self.pointer_at != moved_over {
            self.pointer_at = moved_over;
            // The part-drawn shape follows the pointer.
            if self.drawing.is_drawing() {
                self.redraw();
            }
        }

        if self.drawing.is_drawing() {
            let Some(point) = point else { return };
            if !self.collab.annotations_locked()
                && let Some(Drawn::Added(annotation)) =
                    self.drawing.handle(Stroke::Moved { at: [point.0, point.1], straight: false }, &self.shapes())
            {
                self.add_shape(annotation);
            }
            return;
        }

        if !self.collab.annotations_locked()
            && let Some((index, before)) = self.turning.clone()
        {
            let Some(point) = point else { return };
            let [left, top, right, bottom] = wt_collab_client::drawing::annotation_bounds(&before);
            let middle = [(left + right) / 2.0, (top + bottom) / 2.0];
            let mut turned = before;
            wt_collab_client::drawing::rotate_annotation(
                &mut turned,
                wt_collab_client::drawing::bearing(middle, [point.0, point.1]),
            );
            self.collab.update_annotation(index, turned);
            self.redraw();
            return;
        }

        if !self.collab.annotations_locked()
            && let Some((from, was)) = self.moving.clone()
        {
            let Some(point) = point else { return };
            // Measured from where the drag began, so the shape does not creep
            // as the pointer stutters.
            let delta = [point.0 - from[0], point.1 - from[1]];
            for index in self.picked.picked() {
                let Some(annotation) = was.get(*index) else { continue };
                let mut moved = annotation.clone();
                wt_collab_client::drawing::move_annotation(&mut moved, delta);
                self.collab.update_annotation(*index, moved);
            }
            self.redraw();
            return;
        }

        if let Some(from) = self.panning {
            let delta = (at.x - from.x, at.y - from.y);
            self.panning = Some(at);
            // The view pans in drawn pixels, so the pointer's travel is
            // undone of the letterbox's scale first.
            let scale = self.letterbox().map_or(1.0, |letterbox| letterbox.scale);
            if delta != (0.0, 0.0) {
                self.view = self.view.dragged((delta.0 / scale, delta.1 / scale));
                self.redraw();
            }
            return;
        }

        if !self.can_edit_caps() {
            self.dragging = None;
            return;
        }
        let Some((index, what)) = self.dragging else { return };
        let Some((x, z)) = self.world_point(at) else { return };
        let Some(cap) = self.caps.get_mut(index) else { return };
        match what {
            CapDrag::None => return,
            CapDrag::Move => {
                cap.world_x = x;
                cap.world_z = z;
            }
            CapDrag::Resize => {
                let away = ((x - cap.world_x).powi(2) + (z - cap.world_z).powi(2)).sqrt();
                cap.radius = away.clamp(board::MIN_CAP_RADIUS, board::MAX_CAP_RADIUS);
            }
        }
        self.redraw();
    }

    pub fn pointer_up(&mut self, at: PixelPoint, button: PointerButton) {
        if button != PointerButton::Primary {
            return;
        }
        self.pressed = false;
        self.panning = None;
        self.moving = None;
        self.turning = None;
        if self.collab.annotations_locked() && self.drawing.is_drawing() {
            self.drawing.cancel();
        }
        if self.has_tool() {
            let Some(point) = self.map_point(at) else { return };
            let stroke = if self.drawing.is_drawing() {
                Stroke::Ended { at: [point.0, point.1] }
            } else {
                // A ship and the eraser are used by a click, which never
                // begins a drag.
                Stroke::Clicked { at: [point.0, point.1] }
            };
            self.stroke(stroke);
            return;
        }
        if self.finish_cap_drag() {
            self.redraw();
        }
    }

    pub fn pointer_left(&mut self) {
        if self.pointer_at.take().is_some() && self.drawing.is_drawing() {
            self.redraw();
        }
    }

    /// Wheel travel in scroll steps, positive down: down zooms out.
    pub fn wheel(&mut self, at: Option<PixelPoint>, dy: f32) {
        let Some(at) = at else { return };
        let notches = -dy * WHEEL_STEP_PX;
        if notches != 0.0 {
            self.zoom_about(notches, at);
        }
    }

    /// Lets go of whatever the pointer had hold of.
    fn release(&mut self) {
        self.panning = None;
        self.moving = None;
        self.turning = None;
        let held = self.finish_cap_drag();
        // A shape part way through is abandoned rather than finished somewhere
        // the reader did not put it.
        let drawing = self.drawing.is_drawing();
        if drawing {
            self.drawing.cancel();
        }
        if held || drawing {
            self.redraw();
        }
    }

    /// Says one completed cap drag to the session, once, when it is let go.
    fn finish_cap_drag(&mut self) -> bool {
        let allowed = self.can_edit_caps();
        let Some((index, _)) = self.dragging.take() else { return false };
        if allowed {
            self.report_cap(index);
        } else {
            self.selected = None;
            self.adopted_caps = None;
            self.follow_session();
        }
        true
    }

    /// Opens the context menu at a frame position: puts the tool down, stops
    /// placing capture points, and picks out the capture point under it.
    pub fn context_menu(&mut self, at: PixelPoint) {
        self.set_tool(Tool::None);
        self.set_adding(false);
        self.secondary_down(at);
        self.notify();
    }
}

/// Collab.
impl Board {
    /// Hands this board its end of a session, or takes it away again. What
    /// was drawn stays on the board either way.
    pub fn set_collab(&mut self, link: CollabLink) -> Option<Announcement> {
        self.announced = false;
        self.adopted_caps = None;
        self.adopted_shapes = None;
        if !link.is_active() {
            let carried = self.collab.annotations_held();
            self.joining = None;
            self.collab = CollabLink::default().on_board(self.board_id);
            self.collab.adopt(carried);
            self.redraw();
            return None;
        }
        // Held back until the announcement has gone over.
        self.joining = Some(link.on_board(self.board_id));
        self.begin_announce()
    }

    pub fn is_following(&self) -> bool {
        self.collab.is_active() || self.joining.is_some()
    }

    /// Opens this board on one a peer already has. The art comes with it: the
    /// peer may be on a build this one does not have.
    pub fn adopt_session_board(&mut self, shared: SessionBoard) {
        self.board_id = shared.board_id;
        self.owner = Some(shared.owner_user_id);
        let link = self.joining.take().unwrap_or_else(|| self.collab.clone());
        self.collab = link.on_board(shared.board_id);
        let map = MapChoice {
            map_id: (shared.map.map_id > 0).then_some(shared.map.map_id),
            space: shared.map.space,
            label: shared.map.label,
        };
        if !self.maps.iter().any(|candidate| MapChoiceKey::from(candidate) == MapChoiceKey::from(&map)) {
            self.maps.push(map.clone());
        }
        let metadata = self.game.as_deref().map(BoardGame::metadata);
        self.modes = board::modes(&self.layouts, map.map_id, metadata);
        self.mode = None;
        self.map_info = self
            .game
            .as_deref()
            .and_then(|game| wows_minimap_renderer::assets::load_map_info(&map.space, game.vfs()));
        self.map = Some(map);
        self.shown = false;
        self.no_art = false;
        // Already in the session by definition, so nothing is announced back.
        self.announced = true;
        self.peer_map_info = shared.map.info;
        self.peer_art = shared.map.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(Arc::new(art.to_rgb8())),
            // Art that will not decode leaves the board drawn from this
            // build's own, which is the state a board with no art sent is in.
            Err(err) => {
                log::warn!("tactics board: the art a peer sent could not be read: {err}");
                None
            }
        });
        self.follow_session();
        self.redraw();
    }

    /// Starts telling the session which map this board is on. `None` when
    /// there is nothing to announce: no session joining, no map, or no game
    /// to read the art from.
    fn begin_announce(&mut self) -> Option<Announcement> {
        if self.announced || self.joining.is_none() {
            return None;
        }
        let map = self.map.clone()?;
        let game = self.game.clone()?;
        self.announced = true;
        Some(Announcement { game, map })
    }

    fn announce_again(&mut self) -> Option<Announcement> {
        if !self.collab.is_active() && self.joining.is_none() {
            return None;
        }
        if self.joining.is_none() {
            self.joining = Some(self.collab.clone());
        }
        self.announced = false;
        self.begin_announce()
    }

    /// Finishes an announcement with the art read for it: the map first, then
    /// everything already on the board, through the one channel.
    pub fn finish_announce(&mut self, map: MapChoice, art_png: Option<Vec<u8>>, info: Option<MapInfo>) {
        let Some(session) = self.joining.take() else { return };
        let carried = self.collab.annotations_held();
        session.announce_board(wt_collab_client::link::BoardMap {
            space: map.space,
            label: map.label,
            // The wire states a map with no recorded layout as the id zero.
            map_id: map.map_id.unwrap_or(0),
            art_png,
            info,
        });
        session.adopt(carried);
        for cap in &self.caps {
            session.set_cap(cap.to_wire());
        }
        self.collab = session;
        self.follow_session();
        self.redraw();
    }

    /// Opens this board on a peer's, where the reader has not chosen a map.
    fn adopt_a_peers_board(&mut self) {
        if self.map.is_some() {
            return;
        }
        let mine = self.collab.my_user_id();
        let link = self.joining.clone().unwrap_or_else(|| self.collab.clone());
        let Some(shared) = link.session_boards().into_iter().find(|shared| Some(shared.owner_user_id) != mine) else {
            return;
        };
        self.adopt_session_board(shared);
    }

    /// Takes what the session holds that this board has not drawn yet.
    pub fn follow(&mut self) {
        if !self.is_following() {
            return;
        }
        self.adopt_a_peers_board();
        self.follow_session();
    }

    fn follow_session(&mut self) {
        let permissions = Permissions { may_steer: self.collab.may_steer(), locked: self.collab.annotations_locked() };
        if self.permissions != Some(permissions) {
            self.permissions = Some(permissions);
            if permissions.locked {
                self.drawing.cancel();
                self.turning = None;
                self.moving = None;
            }
            if permissions.locked && !permissions.may_steer {
                self.adding = false;
                self.dragging = None;
                self.adopted_caps = None;
            }
            self.notify();
        }
        let Some(versions) = self.collab.board_versions() else { return };
        let mut changed = false;

        if self.adopted_caps != Some(versions.caps) {
            self.adopted_caps = Some(versions.caps);
            let mut caps: Vec<BoardCapPoint> = self.collab.board_caps().iter().map(BoardCapPoint::from_wire).collect();
            // By letter, so the board reads A to Z whichever order the session
            // holds them in.
            caps.sort_by_key(|cap| cap.index);
            if caps != self.caps {
                self.caps = caps;
                self.selected = None;
                self.dragging = None;
                changed = true;
            }
        }

        if self.adopted_shapes != Some(versions.shapes) {
            self.adopted_shapes = Some(versions.shapes);
            self.picked.retain_to(self.collab.annotation_count());
            changed = true;
        }

        if changed {
            self.redraw();
        }
    }

    /// The board is going away: a walk stops, and a board this reader opened
    /// stops being one the session is on.
    pub fn close(&mut self) {
        self.scan_cancelled.store(true, Ordering::Relaxed);
        if !self.is_a_peers() {
            self.collab.close_board();
        }
    }
}

/// Drawing a frame.
impl Board {
    /// What the next frame is drawn from.
    pub fn frame(&self) -> Option<Frame> {
        let viewport = self.viewport?;
        let Some(map) = &self.map else { return Some(Frame::Empty) };
        let Some(game) = self.game.clone() else { return Some(Frame::Empty) };
        let Some(map_info) = self.current_map_info() else { return Some(Frame::Empty) };

        let shapes = self.shapes();
        // A picked shape is drawn again underneath in a wider stroke, which
        // says it is the one a drag will move.
        let mut annotations: Vec<Annotation> =
            self.picked.picked().iter().filter_map(|at| shapes.get(*at)).map(board::halo).collect();
        annotations.extend(shapes);
        // The shape under the pointer is drawn the way the finished one will
        // be, so what the reader sees while dragging is what they get.
        if let Some(part_drawn) = self.pointer_at.and_then(|at| self.drawing.in_progress(at)) {
            annotations.push(part_drawn);
        }

        Some(Frame::Map(Box::new(RasterJob {
            game,
            space: map.space.clone(),
            art: self.peer_art.clone(),
            map_info,
            board_id: self.board_id,
            view: self.view,
            caps: self.caps.clone(),
            annotations,
            viewport,
            handle: self.turnable().map(|(_, _, handle)| handle),
        })))
    }
}

/// What a map is named by in the picker and the commands.
pub fn map_value(map: &MapChoice) -> String {
    match map.map_id {
        Some(id) => format!("{}#{id}", map.space),
        None => map.space.clone(),
    }
}

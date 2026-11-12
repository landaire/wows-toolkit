//! The board as a front end drives it: what is on it, what the pointer and keyboard do to it, and
//! how it follows a collab session.
//!
//! The board does no work of its own off the thread it lives on. What it needs done elsewhere it
//! leaves as a request: a redraw ([`Board::take_redraw`]), the art a session is told about
//! ([`Board::take_announcement`]), and messages for the reader ([`Board::take_notices`]). The
//! front end drains them after each change and hands the results back.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_replays::types::TeamId;
use wows_toolkit_services::collab::BoardId;
use wows_toolkit_services::collab::BoardMap;
use wows_toolkit_services::collab::CapPointId;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::SessionBoard;
use wows_toolkit_services::collab::UserId;
use wows_toolkit_viewmodel::tactics::naming;
use wows_toolkit_viewmodel::tactics::preset;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::GameParamProvider as _;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::GameParamId;
use wowsunpack::game_types::WorldPos2D;
use wt_collab_client::drawing::Drawing;
use wt_collab_client::drawing::Drawn;
use wt_collab_client::drawing::Held;
use wt_collab_client::drawing::Selection;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;
use wt_collab_client::types::Annotation;
use wt_collab_client::types::AnnotationRangeFilter;
use wt_collab_client::types::AnnotationShipConfig;

use crate::load::GameDataCache;
use crate::tactics::BoardCapPoint;
use crate::tactics::CapDrag;
use crate::tactics::DEFAULT_INK;
use crate::tactics::DEFAULT_NIB;
use crate::tactics::ElementPos;
use crate::tactics::ElementSize;
use crate::tactics::HANDLE_DISTANCE;
use crate::tactics::HANDLE_RADIUS;
use crate::tactics::Letterbox;
use crate::tactics::MAX_CAP_RADIUS;
use crate::tactics::MAX_NIB;
use crate::tactics::MIN_CAP_RADIUS;
use crate::tactics::MIN_NIB;
use crate::tactics::MapChoice;
use crate::tactics::ModeChoice;
use crate::tactics::NEW_CAP_RADIUS;
use crate::tactics::RANGE_CIRCLES;
use crate::tactics::RangeCircle;
use crate::tactics::SHAPE_REACH_PX;
use crate::tactics::ScanProgress;
use crate::tactics::WORLD_UNITS_PER_KM;
use crate::tactics::ZOOM_PER_NOTCH;
use crate::tactics::band_in_world;
use crate::tactics::caps_of;
use crate::tactics::halo;
use crate::tactics::maps;
use crate::tactics::modes;
use crate::tactics::render::ArtRequest;
use crate::tactics::render::BoardArt;
use crate::tactics::render::FrameRequest;
use crate::tactics::scan::ScanJob;
use crate::tactics::scan::ScanOutcome;
use crate::tactics::ships::ShipChoice;
use crate::tactics::ships::ShipList;
use crate::tactics::unique_map_for_space;

/// How many changes can be taken back.
///
/// A board is edited in small steps and the whole of what is on it is kept per step, so the stack
/// is held to a depth rather than growing with the session.
const HISTORY_DEPTH: usize = 50;

/// What is on the board, as an undo step holds it.
#[derive(Clone, Debug, PartialEq)]
struct BoardState {
    caps: Vec<BoardCapPoint>,
    /// With the ids the shapes are known by, so putting a step back is the same change to a
    /// session as it is to a board nobody else can see.
    annotations: Vec<Held>,
}

/// A ship waiting to be placed on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedShip {
    /// The param the ranges are read from.
    param_id: GameParamId,
    pub name: String,
    species: Species,
    /// Whether it is on the reader's side, which is what colours the marker.
    pub friendly: bool,
}

/// A message for the reader, which the front end words and shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardNotice {
    PresetNeedsName,
    PresetNeedsMap,
    PresetSaved {
        name: String,
    },
    PresetSaveFailed {
        error: String,
    },
    PresetLoadFailed {
        error: String,
    },
    PresetDeleteFailed {
        error: String,
    },
    /// A scan was asked for with no game directory set.
    NoReplaysDir,
    /// A scan was asked for before a build was loaded.
    ScanNeedsGameData,
    ScanDone {
        added: usize,
        total: usize,
    },
}

/// What to do about the map's frame after a change.
pub enum Redraw {
    /// No map is chosen: show nothing.
    Clear,
    /// Draw this.
    Draw(Box<FrameRequest>),
}

/// The turning handle over a picked shape, in the element's own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotationHandle {
    /// The middle of the handle.
    pub handle: ElementPos,
    /// Where the stem from it meets the shape.
    pub anchor: ElementPos,
}

/// A key the board reads, as a front end names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardKey<'a> {
    /// A key that types `text`, already lower-cased.
    Text(&'a str),
    Escape,
    Delete,
    Backspace,
}

/// Which modifiers were held with a key or a press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Ctrl, or Cmd on macOS.
    pub secondary: bool,
    pub shift: bool,
}

/// The board itself.
pub struct Board {
    game_data: Option<GameDataCache>,
    layouts: CapLayoutDb,
    maps: Vec<MapChoice>,
    map: Option<MapChoice>,
    modes: Vec<ModeChoice>,
    mode: Option<CapLayoutKey>,
    caps: Vec<BoardCapPoint>,
    /// Which capture point the reader is working on, by its place in `caps`.
    selected: Option<usize>,
    /// Where the pointer is on the map, which is what the part-drawn shape is built against.
    /// `None` while it is off the map.
    pointer_at: Option<[f32; 2]>,
    /// The drag in progress, and what it is doing.
    dragging: Option<(usize, CapDrag)>,
    /// Whether the next click on the map places a capture point.
    adding: bool,
    /// The tool in hand and the shape it is part way through, which is the same state machine the
    /// replay viewport draws with.
    drawing: Drawing,
    /// This board's end of a collab session, which also holds what has been drawn on it when there
    /// is no session to hold it.
    collab: CollabLink,
    /// What this board is called in a session. A board is one window of several a session can be
    /// on, and every message about it carries this.
    board_id: BoardId,
    /// The art a peer sent for this board, for a map this build has none of.
    peer_art: Option<Arc<image::RgbImage>>,
    /// What that map measures, sent with the art. Without it a world position cannot be placed on
    /// the board at all, and this build has nothing to read it from for a map it does not ship.
    peer_map_info: Option<MapInfo>,
    /// Who opened the board in the session. `None` for one nobody has shared.
    owner: Option<UserId>,
    /// Which version of the session's capture points and shapes this board has taken, so a change
    /// made by a peer is noticed without comparing lists every tick.
    adopted_caps: Option<u64>,
    adopted_shapes: Option<u64>,
    /// The steering and lock state last seen, so a change to either is acted on once.
    permissions: Option<Permissions>,
    /// Whether this board has told the session which map it is on. Announced once per map rather
    /// than per tick.
    announced: bool,
    /// The session's end of this board, held back until the session has been told the board
    /// exists.
    ///
    /// Anything sent for a board the session does not hold is dropped by the peer task, so the
    /// board draws on its own end until the announcement has gone over and only then starts
    /// speaking to the session.
    joining: Option<CollabLink>,
    /// The art an announcement needs, until the front end takes it.
    pending_art: Option<ArtRequest>,
    /// The map an announcement is waiting for its art for.
    announcing: Option<MapChoice>,
    /// The element the map is shown in, and the frame shown in it, which is what a pointer
    /// position is read against. `None` until both are known.
    element: Option<ElementSize>,
    frame: Option<(f32, f32)>,
    /// Every board already saved, read when the board opens and after each save.
    presets: Vec<String>,
    /// How far a walk of the replay directory has got, when one is running.
    scanning: Option<ScanProgress>,
    /// Where the replays are, so the walk knows what to read.
    replays_dir: Option<PathBuf>,
    /// Set when the board goes away, which stops a walk part way through.
    scan_cancelled: Arc<AtomicBool>,
    /// The game version the ranges are read at. `None` before the install has been read, which
    /// draws no ranges.
    version: Option<Version>,
    /// The ship the next placement is of. `None` until one is picked.
    placing: Option<PlacedShip>,
    /// Every ship the build knows, built the first time one is looked up.
    ships: Option<Arc<ShipList>>,
    /// Which circles a placed ship shows. Every ship already on the board is given the same set.
    range_filter: AnnotationRangeFilter,
    /// Whether the last attempt to draw the map found no art for it.
    no_art: bool,
    /// Which part of the map is on screen.
    view: MapViewport,
    /// The pan in progress, and where the pointer was when it last moved.
    panning: Option<ElementPos>,
    /// What the board held before each change, so a change can be taken back. The map and the
    /// mode are not in it: those are what the board is set on rather than what is on it.
    history: Vec<BoardState>,
    /// What was taken back, so it can be put back again.
    undone: Vec<BoardState>,
    /// The drawn shapes the reader has picked out, which a drag then moves.
    picked: Selection,
    /// A move in progress: where it began, and what was on the board then.
    moving: Option<([f32; 2], Vec<Annotation>)>,
    /// A turn in progress: which shape, and how it stood before the handle was taken hold of.
    turning: Option<(usize, Annotation)>,
    redraw: bool,
    notices: Vec<BoardNotice>,
    /// Counts changes, so a view can tell the board moved without comparing it.
    revision: u64,
}

/// Whether this board may steer the session, and whether its annotations are locked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Permissions {
    may_steer: bool,
    locked: bool,
}

impl Board {
    pub fn new(game_data: Option<GameDataCache>, layouts: CapLayoutDb) -> Self {
        let maps = maps(&layouts, game_data.as_ref());
        let board_id = BoardId::fresh();
        Self {
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
            drawing: Drawing::new(DEFAULT_INK, DEFAULT_NIB),
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
            pending_art: None,
            announcing: None,
            element: None,
            frame: None,
            presets: preset::list_preset_names(),
            scanning: None,
            replays_dir: None,
            scan_cancelled: Arc::new(AtomicBool::new(false)),
            version: None,
            placing: None,
            ships: None,
            range_filter: AnnotationRangeFilter::default(),
            no_art: false,
            view: MapViewport::default(),
            panning: None,
            history: Vec::new(),
            undone: Vec::new(),
            picked: Selection::default(),
            moving: None,
            turning: None,
            redraw: false,
            notices: Vec::new(),
            revision: 0,
        }
    }

    /// Points the board at the replays a scan would read, and at the version its ranges are read
    /// at.
    pub fn set_install(&mut self, dir: Option<PathBuf>, version: Option<Version>) {
        self.replays_dir = dir;
        self.version = version;
        self.redraw();
    }

    /// Replaces the build the board is drawn from, as when the game data finishes loading after
    /// the board opened. The map list is rebuilt from it.
    pub fn set_game_data(&mut self, game_data: Option<GameDataCache>) {
        self.game_data = game_data;
        self.maps = maps(&self.layouts, self.game_data.as_ref());
        if let Some(map) = self.map.clone()
            && !self.maps.iter().any(|candidate| candidate.key() == map.key())
        {
            self.maps.push(map);
        }
        self.ships = None;
        // An announcement waits for the build to load.
        self.announce();
        self.redraw();
    }

    /// Takes layouts read after the board opened, such as the startup read of the store.
    pub fn set_layouts(&mut self, layouts: CapLayoutDb) {
        self.layouts = layouts;
        self.maps = maps(&self.layouts, self.game_data.as_ref());
        if let Some(map) = self.map.clone() {
            if !self.maps.iter().any(|candidate| candidate.key() == map.key()) {
                self.maps.push(map.clone());
            }
            self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        }
        self.changed();
    }

    /// Counts a change a view should follow.
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Asks for the map to be drawn again.
    fn redraw(&mut self) {
        self.redraw = true;
        self.changed();
    }

    /// Asks for the map to be drawn again, as when a change landed while the last drawing ran.
    pub fn request_redraw(&mut self) {
        self.redraw();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// What the map should show, when a change asked for it to be drawn again.
    ///
    /// `None` when nothing asked, and when there is no build to draw with: the toolbar still says
    /// what the board holds, but the map cannot be drawn.
    pub fn take_redraw(&mut self) -> Option<Redraw> {
        if !std::mem::take(&mut self.redraw) {
            return None;
        }
        let Some(map) = self.map.clone() else { return Some(Redraw::Clear) };
        let game_data = self.game_data.clone()?;
        let shapes = self.shapes();
        // A shape the reader has picked out is drawn again underneath in a wider stroke, which is
        // what says it is the one a drag will move.
        let mut annotations: Vec<Annotation> =
            self.picked.picked().iter().filter_map(|at| shapes.get(*at)).map(halo).collect();
        annotations.extend(shapes);
        // The shape under the pointer is drawn the way the finished one will be.
        if let Some(part_drawn) = self.pointer_at.and_then(|at| self.drawing.in_progress(at)) {
            annotations.push(part_drawn);
        }
        Some(Redraw::Draw(Box::new(FrameRequest {
            space: map.space,
            art: self.peer_art.clone(),
            info: self.peer_map_info.clone(),
            board_id: self.board_id.raw(),
            game_data,
            version: self.version,
            view: self.view,
            caps: self.caps.clone(),
            annotations,
        })))
    }

    /// Takes the size of the frame a rasterisation produced, or `None` for a map with no art.
    pub fn drawn(&mut self, frame: Option<(f32, f32)>) {
        let no_art = frame.is_none();
        if self.no_art != no_art {
            self.no_art = no_art;
            self.changed();
        }
        if frame.is_some() && self.frame != frame {
            self.frame = frame;
            self.changed();
        }
        if self.map.is_none() {
            self.frame = None;
        }
    }

    /// Takes the size of the element the map is shown in.
    pub fn set_element(&mut self, element: ElementSize) {
        if self.element != Some(element) {
            self.element = Some(element);
            self.changed();
        }
    }

    /// The messages raised since the last call.
    pub fn take_notices(&mut self) -> Vec<BoardNotice> {
        std::mem::take(&mut self.notices)
    }

    pub fn maps(&self) -> &[MapChoice] {
        &self.maps
    }

    pub fn map(&self) -> Option<&MapChoice> {
        self.map.as_ref()
    }

    pub fn modes(&self) -> &[ModeChoice] {
        &self.modes
    }

    pub fn mode(&self) -> Option<&CapLayoutKey> {
        self.mode.as_ref()
    }

    pub fn caps(&self) -> &[BoardCapPoint] {
        &self.caps
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

    pub fn range_filter(&self) -> &AnnotationRangeFilter {
        &self.range_filter
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

    pub fn no_art(&self) -> bool {
        self.no_art
    }

    pub fn has_frame(&self) -> bool {
        self.frame.is_some()
    }

    pub fn may_steer(&self) -> bool {
        self.collab.may_steer()
    }

    pub fn annotations_locked(&self) -> bool {
        self.collab.annotations_locked()
    }

    pub fn annotation_count(&self) -> usize {
        self.collab.annotation_count()
    }

    pub fn board_id(&self) -> BoardId {
        self.board_id
    }

    /// Whether a drawing tool is in hand, which is what takes the pointer away from the capture
    /// points.
    pub fn has_tool(&self) -> bool {
        !self.collab.annotations_locked() && *self.drawing.tool() != Tool::None
    }

    /// Capture points remain editable by peers until the host locks annotations.
    pub fn can_edit_caps(&self) -> bool {
        self.collab.may_steer() || !self.collab.annotations_locked()
    }

    /// Whether a map position can be turned into a world one, which placing a capture point needs.
    pub fn has_map_info(&self) -> bool {
        self.map_info().is_some()
    }

    /// The capture point the reader is working on.
    pub fn selected_cap(&self) -> Option<&BoardCapPoint> {
        self.selected.and_then(|at| self.caps.get(at))
    }

    /// Whether the next click places a capture point.
    pub fn adding(&self) -> bool {
        self.adding
    }

    /// Whether there is a change to take back, and one to put back.
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Whether this board is one a peer opened rather than this reader.
    pub fn is_a_peers(&self) -> bool {
        self.owner.is_some_and(|owner| self.collab.my_user_id() != Some(owner))
    }

    /// The shapes drawn on the board.
    ///
    /// Held by the link rather than by the board: with a session it is the session's list, and
    /// without one the link's own, so the same code draws and edits either.
    pub fn shapes(&self) -> Vec<Annotation> {
        self.collab.annotations()
    }

    /// The range filters of the ships placed on the board.
    pub fn ship_filters(&self) -> Vec<AnnotationRangeFilter> {
        self.shapes()
            .into_iter()
            .filter_map(|annotation| match annotation {
                Annotation::Ship { config: Some(config), .. } => Some(config.range_filter),
                _ => None,
            })
            .collect()
    }

    /// Saves the board under `name`.
    ///
    /// A board with no map is not a board: it names nothing to open again.
    pub fn save_preset(&mut self, name: &str) {
        if !self.collab.may_steer() {
            return;
        }
        let name = name.trim().to_owned();
        if name.is_empty() {
            self.notices.push(BoardNotice::PresetNeedsName);
            return;
        }
        let Some(map) = self.map.clone() else {
            self.notices.push(BoardNotice::PresetNeedsMap);
            return;
        };

        let saved = preset::TacticsPreset {
            name: name.clone(),
            map_name: map.space,
            // The file states a map with no recorded layout as the id zero, which is the form the
            // egui board writes and reads.
            map_id: map.map_id.unwrap_or(0),
            cap_points: self.caps.iter().map(BoardCapPoint::to_preset).collect(),
            annotations: self.shapes().iter().map(preset::PresetAnnotation::from_annotation).collect(),
        };
        match preset::save_preset(&saved) {
            Ok(()) => {
                self.presets = preset::list_preset_names();
                self.notices.push(BoardNotice::PresetSaved { name });
                self.changed();
            }
            Err(why) => self.notices.push(BoardNotice::PresetSaveFailed { error: why.to_string() }),
        }
    }

    /// Opens a saved board.
    pub fn load_preset(&mut self, name: &str) {
        if !self.collab.may_steer() || self.collab.annotations_locked() {
            return;
        }
        let read = match preset::load_preset(name) {
            Ok(read) => read,
            Err(why) => {
                self.notices.push(BoardNotice::PresetLoadFailed { error: why.to_string() });
                return;
            }
        };

        // Matched by space name rather than by map id: a board saved from a map nothing has a
        // layout for carries the id zero, which names no map.
        let metadata =
            self.game_data.as_ref().and_then(|data| data.newest_loaded()).map(|loaded| loaded.provider().clone());
        let map_id = (read.map_id != 0).then_some(read.map_id);
        let matching_map = self
            .maps
            .iter()
            .find(|map| map.space == read.map_name && map.map_id == map_id)
            .cloned()
            .or_else(|| if map_id.is_none() { unique_map_for_space(&self.maps, &read.map_name) } else { None });
        let map = matching_map.unwrap_or_else(|| MapChoice {
            map_id,
            label: naming::map_label(&read.map_name, metadata.as_deref()),
            space: read.map_name.clone(),
        });
        if !self.maps.iter().any(|candidate| candidate.key() == map.key()) {
            self.maps.push(map.clone());
        }
        self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        // The saved capture points stand, whatever mode the map has: they are what the reader put
        // there.
        self.mode = None;
        self.map = Some(map);
        let caps = read.cap_points.iter().map(BoardCapPoint::from_preset).collect();
        self.set_caps(caps);
        self.replace_shapes(read.annotations.iter().map(preset::PresetAnnotation::to_annotation).collect());
        self.selected = None;
        self.adding = false;
        self.peer_art = None;
        self.peer_map_info = None;
        self.forget_history();
        self.announce_again();
        self.redraw();
    }

    /// Restores the selected mode's capture points and removes annotations.
    pub fn reset_board(&mut self) {
        if !self.collab.may_steer() || self.collab.annotations_locked() {
            return;
        }

        let caps = self.mode.as_ref().map(|key| caps_of(&self.layouts, key)).unwrap_or_default();
        let caps_match = self.caps.len() == caps.len()
            && self.caps.iter().zip(&caps).all(|(current, default)| {
                current.index == default.index
                    && current.world_x == default.world_x
                    && current.world_z == default.world_z
                    && current.radius == default.radius
                    && current.team == default.team
                    && current.frozen == default.frozen
            });
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

    /// Drops a saved board.
    pub fn delete_preset(&mut self, name: &str) {
        if !self.collab.may_steer() {
            return;
        }
        if let Err(why) = preset::delete_preset(name) {
            self.notices.push(BoardNotice::PresetDeleteFailed { error: why.to_string() });
            return;
        }
        self.presets = preset::list_preset_names();
        self.changed();
    }

    /// Asks for a walk of the replays. `None`, with the reason raised as a notice, when one cannot
    /// run; `None` and nothing said when one is already running.
    pub fn begin_scan(&mut self) -> Option<ScanJob> {
        if self.scanning.is_some() {
            return None;
        }
        let Some(dir) = self.replays_dir.clone() else {
            self.notices.push(BoardNotice::NoReplaysDir);
            return None;
        };
        let Some(loaded) = self.game_data.as_ref().and_then(|data| data.newest_loaded()) else {
            self.notices.push(BoardNotice::ScanNeedsGameData);
            return None;
        };
        self.scan_cancelled = Arc::new(AtomicBool::new(false));
        self.scanning = Some(ScanProgress { read: 0, total: None });
        self.changed();
        Some(ScanJob {
            dir,
            provider: loaded.provider().clone(),
            constants: loaded.base_constants().clone(),
            known: self.layouts.clone(),
            cancelled: Arc::clone(&self.scan_cancelled),
        })
    }

    /// Takes a report from the walk that is running.
    pub fn scan_progress(&mut self, step: ScanProgress) {
        // Only while a walk is the one running: a board whose walk ended is not reporting on it.
        if self.scanning.is_none() {
            return;
        }
        self.scanning = Some(step);
        self.changed();
    }

    /// Takes what a walk found. The layouts, when it found any nothing had before, for the copy a
    /// board opened later starts from.
    pub fn finish_scan(&mut self, outcome: ScanOutcome) -> Option<CapLayoutDb> {
        let ScanOutcome { found, fresh, total } = outcome;
        let added = fresh.len();
        self.scanning = None;
        self.layouts = found;
        // The map list grows with what the walk turned up, and the modes of whichever map the
        // board is on.
        self.maps = maps(&self.layouts, self.game_data.as_ref());
        if let Some(map) = self.map.as_mut() {
            // A map the walk has just recorded a layout for now has an id, which is what its modes
            // are looked up by: without this the scan that found them would show none.
            if map.map_id.is_none() {
                map.map_id = unique_map_for_space(&self.maps, &map.space).and_then(|found| found.map_id);
            }
            let map_id = map.map_id;
            self.modes = modes(&self.layouts, map_id, self.game_data.as_ref());
        }
        if let Some(map) = self.map.clone()
            && !self.maps.iter().any(|candidate| candidate.key() == map.key())
        {
            self.maps.push(map);
        }
        self.notices.push(BoardNotice::ScanDone { added, total });
        self.changed();
        (added > 0).then(|| self.layouts.clone())
    }

    /// Where an element position falls in the drawn frame.
    fn drawn_point(&self, at: ElementPos) -> Option<(f32, f32)> {
        self.letterbox()?.to_frame(at)
    }

    /// How the drawn map sits in the element it is shown in.
    fn letterbox(&self) -> Option<Letterbox> {
        Letterbox::fitting(self.element?, self.frame?)
    }

    /// Where an element position falls on the map, in the map's own pixels. `None` before the map
    /// has been drawn, and for a position in the margin beside it.
    fn map_point(&self, at: ElementPos) -> Option<(f32, f32)> {
        let (x, y) = self.drawn_point(at)?;
        // The frame shows the part of the map the view names, so a drawn point is turned back
        // into a map one before anything reads it.
        Some(self.view.to_map(x, y))
    }

    /// Where a map point lands in the element. `None` for a point the view has scrolled off.
    fn element_point(&self, at: (f32, f32)) -> Option<ElementPos> {
        self.letterbox()?.to_element((self.view.x(at.0), self.view.y(at.1)))
    }

    /// Where a shape's turning handle sits, and where the line to it starts.
    ///
    /// Above the shape by a fixed number of pixels rather than a map distance, so the handle keeps
    /// its size and its reach at every zoom, as the egui board's does.
    fn handle_of(&self, annotation: &Annotation) -> Option<RotationHandle> {
        let [left, top, right, _] = wt_collab_client::drawing::annotation_bounds(annotation);
        let anchor = self.element_point(((left + right) / 2.0, top))?;
        Some(RotationHandle { handle: ElementPos { x: anchor.x, y: anchor.y - HANDLE_DISTANCE }, anchor })
    }

    /// The handle a picked shape is turned by.
    ///
    /// A handle belongs to a single picked shape that has a bearing at all: turning several at
    /// once about their own middles is not what one handle means, and a circle looks the same at
    /// every angle.
    pub fn rotation_handle(&self) -> Option<RotationHandle> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotation_at(index)?;
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        self.handle_of(&annotation)
    }

    /// The shape whose turning handle is under `at`, if the pointer is on one.
    fn handle_under(&self, at: ElementPos) -> Option<(usize, Annotation)> {
        let index = self.picked.single()?;
        let annotation = self.collab.annotation_at(index)?;
        if !wt_collab_client::drawing::can_rotate(&annotation) {
            return None;
        }
        let handle = self.handle_of(&annotation)?.handle;
        let reach = HANDLE_RADIUS + 8.0;
        let away = (at.x - handle.x).hypot(at.y - handle.y);
        (away < reach).then_some((index, annotation))
    }

    /// Zooms the map about the pointer, so what is under it stays there.
    pub fn zoom_about(&mut self, notches: f32, at: ElementPos) {
        let Some(drawn) = self.drawn_point(at) else { return };
        let zoom = self.view.zoom() * (1.0 + notches * ZOOM_PER_NOTCH);
        let view = self.view.zoomed_about(zoom, drawn);
        if view == self.view {
            return;
        }
        self.view = view;
        self.redraw();
    }

    /// Puts the whole map back on screen.
    pub fn reset_view(&mut self) {
        if self.view == MapViewport::default() {
            return;
        }
        self.view = MapViewport::default();
        self.redraw();
    }

    /// The map's own coordinate metadata, which is what turns a map pixel into a world position
    /// and back.
    fn map_info(&self) -> Option<MapInfo> {
        // What a peer sent first: a board shared from a build this one does not have takes its
        // measurements off the wire, because this build has none for that map to read.
        if let Some(sent) = self.peer_map_info.clone() {
            return Some(sent);
        }
        let map = self.map.as_ref()?;
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        wows_minimap_renderer::assets::load_map_info(&map.space, loaded.vfs())
    }

    /// Where an element position falls in the world.
    ///
    /// Through the map's own inverse of what [`BoardCapPoint::command`] draws with, so a press
    /// lands on the zone it looks like it lands on.
    fn world_point(&self, at: ElementPos) -> Option<WorldPos2D> {
        let (x, y) = self.map_point(at)?;
        let map = self.map_info()?;
        let world = map.minimap_to_world_f32(x, y, MINIMAP_SIZE);
        Some(WorldPos2D { x: world.x, z: world.z })
    }

    /// Which capture point a press lands on, and what it would do to it.
    ///
    /// A press within a few pixels of a zone's drawn edge, inside or out, widens it; anywhere else
    /// inside moves it. The topmost one wins, which is the last drawn.
    fn cap_under(&self, at: ElementPos) -> Option<(usize, CapDrag)> {
        let world = self.world_point(at)?;
        let map = self.map_info()?;
        let band = band_in_world(&map);
        self.caps
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, cap)| cap_drag_at(cap, world, band).map(|what| (index, what)))
    }

    /// Turns placing capture points on or off.
    pub fn set_adding(&mut self, adding: bool) {
        if !self.can_edit_caps() {
            return;
        }
        self.adding = adding;
        self.changed();
    }

    /// The drawn shape under the pointer, if it is near enough one.
    fn shape_under(&self, at: ElementPos) -> Option<usize> {
        let at = self.map_point(at)?;
        wt_collab_client::drawing::nearest_within(&self.shapes(), [at.0, at.1], SHAPE_REACH_PX)
    }

    /// Erases the shapes the reader has picked out.
    pub fn erase_picked(&mut self) {
        if self.collab.annotations_locked() || self.picked.picked().is_empty() {
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

    /// Remembers what is on the board, before changing it.
    ///
    /// A change made after something was taken back is the new end of the line, so what was undone
    /// is dropped rather than redone into a board that has moved on.
    fn remember(&mut self) {
        self.history.push(self.board_state());
        if self.history.len() > HISTORY_DEPTH {
            self.history.remove(0);
        }
        self.undone.clear();
    }

    /// Drops every step to take back or put back, for a change that replaces the board's map or
    /// mode: a step holds capture points and shapes without the map they were on.
    fn forget_history(&mut self) {
        self.history.clear();
        self.undone.clear();
    }

    /// Takes the last change back.
    pub fn undo(&mut self) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(was) = self.history.pop() else { return };
        self.undone.push(self.board_state());
        self.adopt(was);
    }

    /// Puts back what was taken.
    pub fn redo(&mut self) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(again) = self.undone.pop() else { return };
        self.history.push(self.board_state());
        self.adopt(again);
    }

    /// Puts the board back to a step.
    fn adopt(&mut self, state: BoardState) {
        self.set_caps(state.caps);
        self.collab.restore(&state.annotations);
        // The selection named a place in a list that has just been replaced.
        self.selected = None;
        self.dragging = None;
        self.picked.clear();
        self.redraw();
    }

    /// Hands this board its end of a session, or takes it away again.
    ///
    /// What was drawn stays on the board either way: a session starting under an open board hears
    /// about it, and one ending leaves it where the reader can still see it.
    pub fn set_collab(&mut self, link: CollabLink) {
        self.announced = false;
        self.adopted_caps = None;
        self.adopted_shapes = None;
        if !link.is_active() {
            let carried = self.collab.annotations_held();
            self.joining = None;
            // A peer's board stays on screen as this reader's own, under an id of its own, so
            // closing it later does not close the peer's.
            if self.owner.take().is_some() {
                self.board_id = BoardId::fresh();
            }
            self.collab = CollabLink::default().on_board(self.board_id);
            self.collab.adopt(carried);
            self.redraw();
            return;
        }
        // Held back until the announcement has gone over, because the session drops anything sent
        // for a board it does not hold yet.
        self.joining = Some(link.on_board(self.board_id));
        self.announce();
        self.changed();
    }

    /// Whether a session is running for this board, which is what the front end's tick follows.
    pub fn in_session(&self) -> bool {
        self.collab.is_active() || self.joining.is_some()
    }

    /// Opens this board on one a peer already has, rather than on a map of this reader's choosing.
    ///
    /// The art comes with it: the peer who opened the board may be on a build this one does not
    /// have, and a board nobody can draw is not shared.
    pub fn adopt_session_board(&mut self, board: SessionBoard) {
        self.board_id = board.board_id;
        self.owner = Some(board.owner_user_id);
        let link = self.joining.take().unwrap_or_else(|| self.collab.clone());
        self.collab = link.on_board(board.board_id);
        let map = MapChoice {
            map_id: (board.map.map_id > 0).then_some(board.map.map_id),
            space: board.map.space,
            label: board.map.label,
        };
        if !self.maps.iter().any(|candidate| candidate.key() == map.key()) {
            self.maps.push(map.clone());
        }
        self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        self.map = Some(map);
        self.mode = None;
        // Already in the session by definition, so nothing is announced back.
        self.announced = true;
        self.peer_map_info = board.map.info.clone();
        self.peer_art = board.map.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(Arc::new(art.to_rgb8())),
            // Art that will not decode leaves the board drawn from this build's own, which is the
            // state a board with no art sent is in.
            Err(err) => {
                tracing::warn!("tactics board: the art a peer sent could not be read: {err}");
                None
            }
        });
        self.follow_session();
        self.redraw();
    }

    /// Tells the session which map this board is on, then hands it everything already on the
    /// board.
    ///
    /// In that order, and through the one channel, because the session drops anything sent for a
    /// board it does not hold. Nothing happens without a map chosen.
    fn announce(&mut self) {
        if self.announced || self.joining.is_none() {
            return;
        }
        let (Some(map), Some(game_data)) = (self.map.clone(), self.game_data.clone()) else { return };
        // Held until the build has loaded: art read before then is absent, and a peer without the
        // map could not draw the board.
        if game_data.newest_loaded().is_none() {
            return;
        }
        self.announced = true;
        let request = ArtRequest { space: map.space.clone(), game_data };
        self.pending_art = Some(request);
        self.announcing = Some(map);
    }

    /// The art an announcement is waiting for, to be read off the UI thread and handed to
    /// [`Self::finish_announce`].
    pub fn take_announcement(&mut self) -> Option<ArtRequest> {
        self.pending_art.take()
    }

    /// Announces the board with the art read for it.
    pub fn finish_announce(&mut self, art: BoardArt) {
        // Read for a map the board has since left, which a later request is reading for.
        if self.announcing.as_ref().is_none_or(|map| map.space != art.space) {
            return;
        }
        let Some(map) = self.announcing.take() else { return };
        let Some(session) = self.joining.take() else { return };
        // Read before the swap: what the board holds is on its own end until now.
        let carried = self.collab.annotations_held();
        let caps: Vec<_> = self.caps.iter().map(BoardCapPoint::to_wire).collect();
        session.announce_board(BoardMap {
            space: map.space,
            label: map.label,
            // The wire states a map with no recorded layout as the id zero, which is the form the
            // egui board sends and reads.
            map_id: map.map_id.unwrap_or(0),
            art_png: art.png,
            info: art.info,
        });
        session.adopt(carried);
        for cap in caps {
            session.set_cap(cap);
        }
        self.collab = session;
        self.follow_session();
        self.redraw();
    }

    /// Says the board is on a different map than the session was told.
    fn announce_again(&mut self) {
        if !self.collab.is_active() && self.joining.is_none() {
            return;
        }
        if self.joining.is_none() {
            self.joining = Some(self.collab.clone());
        }
        self.announced = false;
        self.announce();
    }

    /// Follows the session once: opens a peer's board where the reader has chosen none, and takes
    /// what the session holds that this board has not drawn yet.
    pub fn tick(&mut self) {
        if !self.in_session() {
            return;
        }
        self.adopt_a_peers_board();
        self.follow_session();
    }

    /// Opens this board on a peer's, where the reader has not chosen a map.
    ///
    /// Checked as the session is followed rather than once when it starts: a joiner is sent the
    /// boards the session is on after the handshake, so at the moment it connects there are none.
    fn adopt_a_peers_board(&mut self) {
        if self.map.is_some() {
            return;
        }
        let mine = self.collab.my_user_id();
        let link = self.joining.clone().unwrap_or_else(|| self.collab.clone());
        let Some(board) = link.session_boards().into_iter().find(|board| Some(board.owner_user_id) != mine) else {
            return;
        };
        tracing::info!("tactics board: opening on a board shared in this session");
        self.adopt_session_board(board);
    }

    /// Takes what the session holds that this board has not drawn yet.
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
            self.changed();
        }
        let Some(versions) = self.collab.board_versions() else { return };
        let mut changed = false;

        if self.adopted_caps != Some(versions.caps) {
            self.adopted_caps = Some(versions.caps);
            let mut caps: Vec<BoardCapPoint> = self.collab.board_caps().iter().map(BoardCapPoint::from_wire).collect();
            // By letter, so the board reads A to Z whichever order the session holds them in.
            caps.sort_by_key(|cap| cap.index);
            if caps != self.caps {
                self.caps = caps;
                // The selection named a place in a list that has been replaced.
                self.selected = None;
                self.dragging = None;
                changed = true;
            }
        }

        // The shapes are held by the link, so a bump only means this board is drawing a list that
        // has moved on.
        if self.adopted_shapes != Some(versions.shapes) {
            self.adopted_shapes = Some(versions.shapes);
            self.picked.retain_to(self.collab.annotation_count());
            changed = true;
        }

        if changed {
            self.redraw();
        }
    }

    /// Puts `caps` on the board, telling the session what changed.
    ///
    /// By difference rather than by sending the lot: a session holds capture points by id, and
    /// re-sending an unchanged one would have every peer redraw its board for nothing.
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

    /// Says that one capture point changed, so every board in the session shows the same zone.
    fn report_cap(&self, index: usize) {
        if let Some(cap) = self.caps.get(index) {
            self.collab.set_cap(cap.to_wire());
        }
    }

    /// What is on the board, for an undo step to hold.
    fn board_state(&self) -> BoardState {
        BoardState { caps: self.caps.clone(), annotations: self.collab.annotations_held() }
    }

    /// Puts `shapes` on the board in place of what is there.
    fn replace_shapes(&mut self, shapes: Vec<Annotation>) {
        self.collab.replace_annotations(shapes);
        self.drawing.cancel();
        self.picked.clear();
        self.panning = None;
        self.dragging = None;
        self.moving = None;
        self.turning = None;
    }

    /// Places a capture point where the reader clicked.
    fn add_cap_at(&mut self, world: WorldPos2D) {
        if !self.can_edit_caps() {
            return;
        }
        self.remember();
        // Lettered past the highest already there, so a cap taken off does not hand its letter to
        // the next one placed.
        let index = self.caps.iter().map(|cap| cap.index + 1).max().unwrap_or(0);
        self.caps.push(BoardCapPoint {
            id: CapPointId::fresh(),
            index,
            world_x: world.x,
            world_z: world.z,
            radius: NEW_CAP_RADIUS,
            team: None,
            frozen: false,
        });
        self.report_cap(self.caps.len() - 1);
        self.selected = Some(self.caps.len() - 1);
        self.adding = false;
        self.redraw();
    }

    /// Drops the capture point the reader is working on. A frozen one stays.
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
        // The letters the others carry stand: renumbering would rename the ones that stayed.
        let gone = self.caps.remove(at);
        self.collab.remove_cap(gone.id);
        self.redraw();
    }

    /// Widens or narrows the selected zone, in kilometres.
    pub fn step_selected_radius(&mut self, by_km: f32) {
        if !self.can_edit_caps() {
            return;
        }
        let Some(at) = self.selected else { return };
        if self.caps.get(at).is_none() {
            return;
        }
        self.remember();
        let Some(cap) = self.caps.get_mut(at) else { return };
        cap.radius = (cap.radius + by_km * WORLD_UNITS_PER_KM).clamp(MIN_CAP_RADIUS, MAX_CAP_RADIUS);
        self.report_cap(at);
        self.redraw();
    }

    /// Hands the selected capture point to the next team round: nobody, the reader's side, then
    /// the other.
    pub fn cycle_selected_team(&mut self) {
        if !self.can_edit_caps() {
            return;
        }
        let Some(at) = self.selected else { return };
        if self.caps.get(at).is_none() {
            return;
        }
        self.remember();
        let Some(cap) = self.caps.get_mut(at) else { return };
        cap.team = match cap.team.map(|team| team.raw()) {
            None => Some(TeamId::new(0)),
            Some(0) => Some(TeamId::new(1)),
            Some(_) => None,
        };
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
        let removed: Vec<_> = self.caps.iter().filter(|cap| !cap.frozen).map(|cap| cap.id).collect();
        self.caps.retain(|cap| cap.frozen);
        for id in removed {
            self.collab.remove_cap(id);
        }
        self.selected = selected_id.and_then(|id| self.caps.iter().position(|cap| cap.id == id));
        self.redraw();
    }

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
        self.changed();
    }

    /// Takes up a tool by its chord, whatever is in hand.
    fn set_shortcut_tool(&mut self, tool: Tool) {
        if self.collab.annotations_locked() {
            return;
        }
        self.drawing.set_tool(tool);
        self.adding = false;
        self.selected = None;
        self.picked = Selection::default();
        self.changed();
    }

    /// The ships whose names match `query`, best first.
    pub fn matching_ships(&mut self, query: &FilterQuery) -> Vec<ShipChoice> {
        if !query.is_active() {
            return Vec::new();
        }
        let Some(ships) = self.ship_list() else { return Vec::new() };

        ships.matching(query)
    }

    /// Every ship the build knows, built the first time one is looked up.
    fn ship_list(&mut self) -> Option<Arc<ShipList>> {
        if let Some(ships) = &self.ships {
            return Some(Arc::clone(ships));
        }
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        let ships = Arc::new(ShipList::build(loaded.provider()));
        self.ships = Some(Arc::clone(&ships));
        Some(ships)
    }

    /// Takes up the ship the reader picked, so the next click places it.
    pub fn pick_ship(&mut self, ship: &ShipChoice) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(loaded) = self.game_data.as_ref().and_then(|data| data.newest_loaded()) else { return };
        let Some(param) = loaded.provider().game_param_by_index(&ship.param_index) else { return };

        let friendly = self.placing.as_ref().map(|placed| placed.friendly).unwrap_or(true);
        self.placing =
            Some(PlacedShip { param_id: param.id(), name: ship.display_name.clone(), species: ship.species, friendly });
        self.drawing.set_tool(Tool::Ship { species: format!("{:?}", ship.species), friendly, yaw: 0.0 });
        self.adding = false;
        self.selected = None;
        self.changed();
    }

    /// Which side the ship being placed is on.
    pub fn set_placing_friendly(&mut self, friendly: bool) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(placed) = self.placing.as_mut() else { return };
        placed.friendly = friendly;
        let species = format!("{:?}", placed.species);
        self.drawing.set_tool(Tool::Ship { species, friendly, yaw: 0.0 });
        self.changed();
    }

    /// Names the ship a placement just put down, which is what its ranges are read from.
    fn add_shape(&mut self, mut annotation: Annotation) {
        // Named before it goes over rather than after: in a session the add is a message, and the
        // list it would be named in has not caught up yet.
        if let Annotation::Ship { config, .. } = &mut annotation
            && let Some(placed) = self.placing.clone()
        {
            // Stock hull and no modifiers, which is where the egui chooser leaves it.
            *config = Some(AnnotationShipConfig {
                range_filter: self.range_filter.clone(),
                ..AnnotationShipConfig::new(placed.param_id, placed.name)
            });
        }
        self.collab.add_annotation(annotation);
    }

    /// Turns circles on or off, for the ships already placed and the ones to come.
    pub fn set_range_circles(&mut self, circles: &[RangeCircle], on: bool) {
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

    /// Every circle on or off.
    pub fn set_all_range_circles(&mut self, on: bool) {
        let all = RANGE_CIRCLES.map(|(circle, _)| circle);
        self.set_range_circles(&all, on);
    }

    /// Draws in a different ink from here on. What is already drawn keeps its ink.
    pub fn set_ink(&mut self, ink: [u8; 4]) {
        if self.collab.annotations_locked() {
            return;
        }
        self.drawing.set_color(ink);
        self.changed();
    }

    /// Widens or narrows the nib, within what the board draws with.
    pub fn step_nib(&mut self, by: f32) {
        if self.collab.annotations_locked() {
            return;
        }
        let stepped = (self.drawing.width() + by).clamp(MIN_NIB, MAX_NIB);
        self.drawing.set_width(stepped);
        self.changed();
    }

    /// Takes everything drawn off the board, leaving the capture points.
    pub fn clear_annotations(&mut self) {
        if self.collab.annotations_locked() || self.collab.annotation_count() == 0 {
            return;
        }
        self.remember();
        self.replace_shapes(Vec::new());
        self.redraw();
    }

    /// Hands a pointer event to the tool and keeps what it drew.
    fn stroke(&mut self, stroke: Stroke) {
        if self.collab.annotations_locked() {
            return;
        }
        match self.drawing.handle(stroke, &self.shapes()) {
            Some(Drawn::Added(annotation)) => self.add_shape(annotation),
            Some(Drawn::Erased(index)) => self.collab.erase_annotation(index),
            _ => {}
        }
        self.redraw();
    }

    /// A primary press on the map.
    pub fn pointer_down(&mut self, at: ElementPos, held: Modifiers) {
        // A tool in hand takes the drag: a reader drawing a line is not asking to move the
        // capture point under it.
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
        // The handle takes the drag before anything else: it sits above the shape, over map nobody
        // is reaching for.
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
            if !self.picked.is_empty()
                && self.shape_under(at).is_some_and(|found| self.picked.picked().contains(&found))
            {
                self.remember();
                self.moving = Some(([point.0, point.1], self.shapes()));
                return;
            }
            // Otherwise a press on one picks it out, and the modifier adds it to whatever is
            // already picked.
            let was: Vec<usize> = self.picked.picked().to_vec();
            self.picked.click(&self.shapes(), [point.0, point.1], held.secondary);
            if self.picked.picked() != was.as_slice() {
                self.redraw();
                return;
            }
        }

        match self.can_edit_caps().then(|| self.cap_under(at)).flatten() {
            Some((index, what)) => {
                self.selected = Some(index);
                if what != CapDrag::None {
                    // Taken as the drag begins: the pointer moves it many times and all of that is
                    // one change to take back.
                    self.remember();
                }
                self.dragging = Some((index, what));
            }
            // A drag past every zone moves the map; the click alone puts the selection down.
            None => {
                self.selected = None;
                self.panning = Some(at);
            }
        }
        self.changed();
    }

    /// A double press on the map puts the whole map back, which is how the replay viewport is
    /// reset too. Not while a tool or placement takes the press.
    pub fn double_click(&mut self) {
        if self.has_tool() || self.adding {
            return;
        }
        self.panning = None;
        self.reset_view();
    }

    /// A secondary press, which picks out the capture point under it for the menu it opens.
    pub fn context_down(&mut self, at: ElementPos) {
        if let Some((index, _)) = self.cap_under(at)
            && self.selected != Some(index)
        {
            self.selected = Some(index);
            self.changed();
        }
    }

    /// What opening the map's menu does to the board: the tool and placement are put down.
    pub fn menu_opened(&mut self) {
        if *self.drawing.tool() != Tool::None {
            self.set_tool(Tool::None);
        }
        self.set_adding(false);
    }

    /// The pointer moved over the map. `pressed` is whether a button is held.
    pub fn pointer_move(&mut self, at: ElementPos, pressed: bool) {
        if self.collab.annotations_locked() {
            self.turning = None;
            self.moving = None;
            if self.drawing.is_drawing() {
                self.drawing.cancel();
            }
        }
        // A drag released off the map never reports its release here, so a pointer moving with
        // nothing held has let go of whatever it had.
        if !pressed {
            self.release();
        }
        let point = self.map_point(at);
        let moved_over = point.map(|(x, y)| [x, y]);
        if self.pointer_at != moved_over {
            self.pointer_at = moved_over;
            // The part-drawn shape follows the pointer, so the frame is only worth redrawing while
            // one is being drawn.
            if self.drawing.is_drawing() {
                self.redraw();
            }
        }

        if self.drawing.is_drawing() {
            let Some(point) = point else { return };
            if self.collab.annotations_locked() {
                return;
            }
            if let Some(Drawn::Added(annotation)) =
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
            && let Some((from, was)) = self.moving.as_ref()
        {
            let Some(point) = point else { return };
            let delta = [point.0 - from[0], point.1 - from[1]];
            // Measured from where the drag began rather than from the last position, so the shape
            // does not creep as the pointer stutters.
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
            if delta.0 != 0.0 || delta.1 != 0.0 {
                // The view pans in frame pixels, and the element shows the frame scaled.
                let scale = self.letterbox().map_or(1.0, |fit| fit.scale);
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
        let Some(world) = self.world_point(at) else { return };
        let Some(cap) = self.caps.get_mut(index) else { return };
        match what {
            CapDrag::None => return,
            CapDrag::Move => {
                cap.world_x = world.x;
                cap.world_z = world.z;
            }
            CapDrag::Resize => {
                let away = ((world.x - cap.world_x).powi(2) + (world.z - cap.world_z).powi(2)).sqrt();
                cap.radius = away.clamp(MIN_CAP_RADIUS, MAX_CAP_RADIUS);
            }
        }
        self.redraw();
    }

    /// The pointer left the map: the part-drawn shape stops following it.
    pub fn pointer_left(&mut self) {
        if self.pointer_at.take().is_some() && self.drawing.is_drawing() {
            self.redraw();
        }
    }

    /// Lets go of whatever the pointer had hold of.
    pub fn release(&mut self) {
        self.panning = None;
        self.moving = None;
        self.turning = None;
        let held = self.finish_cap_drag();
        // A shape part way through is abandoned rather than finished somewhere the reader did not
        // put it.
        let drawing = self.drawing.is_drawing();
        if drawing {
            self.drawing.cancel();
        }
        if held || drawing {
            self.redraw();
        }
    }

    /// Sends one completed cap drag, or restores it after a lock change.
    fn finish_cap_drag(&mut self) -> bool {
        // Said once, when it is let go: a drag moves a zone at pointer-event rate, and every peer
        // would redraw its board for each of them.
        let cap_edit_allowed = self.can_edit_caps();
        let Some((index, _)) = self.dragging.take() else { return false };
        if cap_edit_allowed {
            self.report_cap(index);
        } else {
            self.selected = None;
            self.adopted_caps = None;
            self.follow_session();
        }
        true
    }

    /// A primary release on the map.
    pub fn pointer_up(&mut self, at: ElementPos) {
        self.panning = None;
        self.moving = None;
        self.turning = None;
        if self.collab.annotations_locked() && self.drawing.is_drawing() {
            self.drawing.cancel();
        }
        if self.has_tool() {
            // Released off the map: the shape part way through is abandoned.
            let Some(point) = self.map_point(at) else {
                self.release();
                return;
            };
            let stroke = if self.drawing.is_drawing() {
                Stroke::Ended { at: [point.0, point.1] }
            } else {
                // A ship and the eraser are placed and used by a click, which never begins a drag.
                Stroke::Clicked { at: [point.0, point.1] }
            };
            self.stroke(stroke);
            return;
        }
        if self.finish_cap_drag() {
            self.changed();
        }
    }

    /// The board's keyboard: the chords the egui board takes.
    ///
    /// The modifier with Z takes a change back and with Y or Shift+Z puts it back; with 1..7 and M
    /// it selects drawing tools. Escape puts the tool down, `[` and `]` step the nib, and Delete
    /// erases what is picked out. `true` when the key was one of these.
    pub fn key(&mut self, key: BoardKey<'_>, held: Modifiers) -> bool {
        if held.secondary {
            let BoardKey::Text(text) = key else { return false };
            match text {
                "z" if !held.shift => self.undo(),
                "y" | "z" => self.redo(),
                other => match crate::tactics::BoardTool::for_shortcut(other) {
                    Some(tool) => self.set_shortcut_tool(tool.tool()),
                    None => return false,
                },
            }
            return true;
        }
        match key {
            BoardKey::Escape => self.set_tool(Tool::None),
            BoardKey::Text("[") if self.has_tool() => self.step_nib(-1.0),
            BoardKey::Text("]") if self.has_tool() => self.step_nib(1.0),
            // What is picked out is what a delete is about: a drawn shape if one is picked, and the
            // capture point otherwise.
            BoardKey::Delete | BoardKey::Backspace if !self.picked.picked().is_empty() => self.erase_picked(),
            BoardKey::Delete | BoardKey::Backspace => self.remove_selected(),
            _ => return false,
        }
        true
    }

    /// Sets the board on a map, which replaces whatever was on it.
    pub fn set_map(&mut self, map: MapChoice) {
        if !self.collab.may_steer() || self.map.as_ref() == Some(&map) {
            return;
        }
        self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        // The first mode the map has, so a board opens with capture points on it rather than
        // empty.
        self.mode = self.modes.first().map(|mode| mode.key.clone());
        let caps = self.mode.as_ref().map(|key| caps_of(&self.layouts, key)).unwrap_or_default();
        self.set_caps(caps);
        self.selected = None;
        self.map = Some(map);
        // A peer's art and measurements belong to the map that peer sent.
        self.peer_art = None;
        self.peer_map_info = None;
        self.forget_history();
        // A board is announced with the map it is on, so a different map is a different
        // announcement under the same board.
        self.announce_again();
        self.redraw();
    }

    /// Sets the board on one of the chosen map's modes.
    ///
    /// Picking the mode already set puts it down, which is the egui board's own blank entry: an
    /// empty map to place capture points on from nothing.
    pub fn set_mode(&mut self, key: CapLayoutKey) {
        if !self.collab.may_steer() {
            return;
        }
        if self.mode.as_ref() == Some(&key) {
            self.set_blank_mode();
            return;
        }
        let caps = caps_of(&self.layouts, &key);
        self.set_caps(caps);
        self.selected = None;
        self.mode = Some(key);
        self.forget_history();
        self.redraw();
    }

    /// Clears the selected layout and its capture points.
    pub fn set_blank_mode(&mut self) {
        if !self.collab.may_steer() || (self.mode.is_none() && self.caps.is_empty()) {
            return;
        }
        self.mode = None;
        self.forget_history();
        self.set_caps(Vec::new());
        self.selected = None;
        self.redraw();
    }

    /// The board is going away: a walk stops, and the session is told the board has closed.
    ///
    /// Only this app's own: closing the window on a board a peer shared would take it off their
    /// board too.
    pub fn close(&mut self) {
        self.scan_cancelled.store(true, Ordering::Relaxed);
        if !self.is_a_peers() {
            self.collab.close_board();
        }
    }
}

/// What a press at `world` does to `cap`, given how near its edge counts as on it.
///
/// A frozen cap is still pickable, so its figures can be read; it is simply not dragged.
pub fn cap_drag_at(cap: &BoardCapPoint, world: WorldPos2D, band: f32) -> Option<CapDrag> {
    let away = ((world.x - cap.world_x).powi(2) + (world.z - cap.world_z).powi(2)).sqrt();
    if cap.frozen {
        return (away < cap.radius).then_some(CapDrag::None);
    }
    if (away - cap.radius).abs() <= band {
        return Some(CapDrag::Resize);
    }
    (away < cap.radius).then_some(CapDrag::Move)
}

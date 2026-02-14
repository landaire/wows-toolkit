use crate::icon_str;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use egui::Color32;
use egui::CornerRadius;
use egui::FontId;
use egui::Pos2;
use egui::Rect;
use egui::Shape;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;
use egui::mutex::Mutex;

use rootcause::report;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::draw_command::{DrawCommand, ShipConfigCircleKind};
use wows_minimap_renderer::map_data::MapInfo;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::decoder::Consumable;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::recognized::Recognized;

use egui_taffy::AsTuiBuilder as _;
use egui_taffy::TuiBuilderLogic as _;
use egui_taffy::taffy;
use egui_taffy::taffy::prelude::auto;
use egui_taffy::taffy::prelude::length;

use crate::icons;
use crate::settings::SavedRenderOptions;
use crate::wows_data::SharedWoWsData;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Approximate number of frame snapshots per second of game time.
/// Controls the granularity of seeking in the replay.
const SNAPSHOTS_PER_SECOND: f32 = 1.5;
const ICON_SIZE: f32 = assets::ICON_SIZE as f32;
const PLAYBACK_SPEEDS: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];

// ─── Zoom/Pan State ─────────────────────────────────────────────────────────

/// Overlay controls visibility state. Persists across frames.
struct OverlayState {
    /// Last time the mouse moved or a control was interacted with (ctx.input time).
    last_activity: f64,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self { last_activity: 0.0 }
    }
}

/// Zoom and pan state for the replay viewport. Persists across frames.
struct ViewportZoomPan {
    /// Zoom level. 1.0 = no zoom (fit to window). Range: [1.0, 10.0].
    zoom: f32,
    /// Pan offset in zoomed-minimap-pixel space.
    /// (0,0) = top-left corner of the map is at the top-left of the viewport.
    pan: Vec2,
}

impl Default for ViewportZoomPan {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO }
    }
}

/// Encapsulates coordinate transforms for a single frame of viewport rendering.
/// Handles both window-fit scaling and zoom/pan for the map region.
struct MapTransform {
    /// Top-left of the allocated painter rect in screen space.
    origin: Pos2,
    /// Uniform scale from logical canvas pixels to screen pixels.
    window_scale: f32,
    /// Zoom level (1.0 = no zoom).
    zoom: f32,
    /// Pan offset in zoomed-minimap-pixel space.
    pan: Vec2,
    /// HUD height in logical pixels.
    hud_height: f32,
    /// Logical canvas width (768).
    canvas_width: f32,
}

impl MapTransform {
    /// Convert a MinimapPos (in [0..768] space) to screen Pos2.
    /// Applies zoom and pan, then window scale. Used for all map elements.
    fn minimap_to_screen(&self, pos: &MinimapPos) -> Pos2 {
        let zoomed_x = pos.x as f32 * self.zoom - self.pan.x;
        let zoomed_y = pos.y as f32 * self.zoom - self.pan.y;
        Pos2::new(
            self.origin.x + zoomed_x * self.window_scale,
            self.origin.y + (self.hud_height + zoomed_y) * self.window_scale,
        )
    }

    /// Scale a distance (e.g., radius, icon size) from minimap space to screen space.
    /// Scales with both zoom and window_scale.
    fn scale_distance(&self, d: f32) -> f32 {
        d * self.zoom * self.window_scale
    }

    /// Scale a stroke width. Scales with window_scale only (not zoom),
    /// keeping lines readable at all zoom levels.
    fn scale_stroke(&self, width: f32) -> f32 {
        width * self.window_scale
    }

    /// Position for HUD elements (ScoreBar, Timer, KillFeed).
    /// These scale with the window but NOT with zoom/pan.
    fn hud_pos(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.origin.x + x * self.window_scale, self.origin.y + y * self.window_scale)
    }

    /// The HUD-scaled canvas width in screen pixels.
    fn screen_canvas_width(&self) -> f32 {
        self.canvas_width * self.window_scale
    }

    /// Convert a screen Pos2 to minimap logical coords (inverse of minimap_to_screen).
    fn screen_to_minimap(&self, screen_pos: Pos2) -> Vec2 {
        let sx = (screen_pos.x - self.origin.x) / self.window_scale;
        let sy = (screen_pos.y - self.origin.y) / self.window_scale - self.hud_height;
        Vec2::new((sx + self.pan.x) / self.zoom, (sy + self.pan.y) / self.zoom)
    }
}

// ─── Annotation / Painting State ─────────────────────────────────────────────

const SHIP_SPECIES: [&str; 5] = ["Destroyer", "Cruiser", "Battleship", "AirCarrier", "Submarine"];
const FRIENDLY_COLOR: Color32 = Color32::from_rgb(76, 232, 170);
const ENEMY_COLOR: Color32 = Color32::from_rgb(254, 77, 42);

/// A single annotation placed on the map.
#[derive(Clone)]
enum Annotation {
    Ship { pos: Vec2, yaw: f32, species: String, friendly: bool },
    FreehandStroke { points: Vec<Vec2>, color: Color32, width: f32 },
    Line { start: Vec2, end: Vec2, color: Color32, width: f32 },
    Circle { center: Vec2, radius: f32, color: Color32, width: f32, filled: bool },
    Rectangle { center: Vec2, half_size: Vec2, rotation: f32, color: Color32, width: f32, filled: bool },
    Triangle { center: Vec2, radius: f32, rotation: f32, color: Color32, width: f32, filled: bool },
}

/// Active drawing/placement tool.
#[derive(Clone)]
enum PaintTool {
    None,
    PlacingShip { species: String, friendly: bool, yaw: f32 },
    Freehand { current_stroke: Option<Vec<Vec2>> },
    Eraser,
    DrawingLine { start: Option<Vec2> },
    DrawingCircle { filled: bool, center: Option<Vec2> },
    DrawingRect { filled: bool, center: Option<Vec2> },
    DrawingTriangle { filled: bool, center: Option<Vec2> },
}

/// Persistent annotation layer state.
struct AnnotationState {
    annotations: Vec<Annotation>,
    undo_stack: Vec<Vec<Annotation>>,
    active_tool: PaintTool,
    paint_color: Color32,
    stroke_width: f32,
    selected_index: Option<usize>,
    show_context_menu: bool,
    context_menu_pos: Pos2,
    dragging_rotation: bool,
    /// Ships whose trails are explicitly hidden (by player name).
    trail_hidden_ships: HashSet<String>,
    /// Player name of ship nearest to right-click position (for context menu options).
    context_menu_ship: Option<String>,
    /// Per-ship range overrides. Maps player_name -> [det, mb, sec, radar, hydro].
    /// When a ship is in this map, these flags override the global range toggles.
    ship_range_overrides: HashMap<String, [bool; 5]>,
}

impl Default for AnnotationState {
    fn default() -> Self {
        Self {
            annotations: Vec::new(),
            undo_stack: Vec::new(),
            active_tool: PaintTool::None,
            paint_color: Color32::YELLOW,
            stroke_width: 2.0,
            selected_index: None,
            show_context_menu: false,
            context_menu_pos: Pos2::ZERO,
            dragging_rotation: false,
            trail_hidden_ships: HashSet::new(),
            context_menu_ship: None,
            ship_range_overrides: HashMap::new(),
        }
    }
}

impl AnnotationState {
    /// Save current annotations as an undo snapshot.
    fn save_undo(&mut self) {
        self.undo_stack.push(self.annotations.clone());
        // Cap stack size
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the last undo snapshot, restoring annotations.
    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.annotations = prev;
            self.selected_index = None;
        }
    }
}

// ─── Asset Cache ─────────────────────────────────────────────────────────────

/// RGBA image data: (pixels, width, height).
type RgbaAsset = (Vec<u8>, u32, u32);

/// Cached assets shared across renderer instances. Lives in TabState.
/// Ship and plane icons are game-global; map data is per-map.
#[derive(Default)]
pub struct RendererAssetCache {
    ship_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    plane_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    consumable_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    death_cause_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    powerup_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    maps: HashMap<String, CachedMapData>,
}

struct CachedMapData {
    image: Option<Arc<RgbaAsset>>,
    info: Option<MapInfo>,
}

impl RendererAssetCache {
    fn get_or_load_ship_icons(
        &mut self,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.ship_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_ship_icons(file_tree, pkg_loader);
        let converted: HashMap<String, RgbaAsset> = raw
            .into_iter()
            .map(|(k, img)| {
                let (w, h) = (img.width(), img.height());
                (k, (img.into_raw(), w, h))
            })
            .collect();
        let arc = Arc::new(converted);
        self.ship_icons = Some(Arc::clone(&arc));
        arc
    }

    fn get_or_load_plane_icons(
        &mut self,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.plane_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_plane_icons(file_tree, pkg_loader);
        let converted: HashMap<String, RgbaAsset> = raw
            .into_iter()
            .map(|(k, img)| {
                let (w, h) = (img.width(), img.height());
                (k, (img.into_raw(), w, h))
            })
            .collect();
        let arc = Arc::new(converted);
        self.plane_icons = Some(Arc::clone(&arc));
        arc
    }

    fn get_or_load_consumable_icons(
        &mut self,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.consumable_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_consumable_icons(file_tree, pkg_loader);
        let converted: HashMap<String, RgbaAsset> = raw
            .into_iter()
            .map(|(k, img)| {
                let (w, h) = (img.width(), img.height());
                (k, (img.into_raw(), w, h))
            })
            .collect();
        let arc = Arc::new(converted);
        self.consumable_icons = Some(Arc::clone(&arc));
        arc
    }

    fn get_or_load_death_cause_icons(
        &mut self,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.death_cause_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_death_cause_icons(file_tree, pkg_loader, 16);
        let converted: HashMap<String, RgbaAsset> = raw
            .into_iter()
            .map(|(k, img)| {
                let (w, h) = (img.width(), img.height());
                (k, (img.into_raw(), w, h))
            })
            .collect();
        let arc = Arc::new(converted);
        self.death_cause_icons = Some(Arc::clone(&arc));
        arc
    }

    fn get_or_load_powerup_icons(
        &mut self,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.powerup_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_powerup_icons(file_tree, pkg_loader, 16);
        let converted: HashMap<String, RgbaAsset> = raw
            .into_iter()
            .map(|(k, img)| {
                let (w, h) = (img.width(), img.height());
                (k, (img.into_raw(), w, h))
            })
            .collect();
        let arc = Arc::new(converted);
        self.powerup_icons = Some(Arc::clone(&arc));
        arc
    }

    fn get_or_load_map(
        &mut self,
        map_name: &str,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> (Option<Arc<RgbaAsset>>, Option<MapInfo>) {
        if let Some(cached) = self.maps.get(map_name) {
            return (cached.image.clone(), cached.info.clone());
        }
        let map_image = assets::load_map_image(map_name, file_tree, pkg_loader).map(|img| {
            let rgba = image::DynamicImage::ImageRgb8(img).into_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            Arc::new((rgba.into_raw(), w, h))
        });
        let map_info = assets::load_map_info(map_name, file_tree, pkg_loader);
        self.maps.insert(map_name.to_string(), CachedMapData { image: map_image.clone(), info: map_info.clone() });
        (map_image, map_info)
    }
}

// ─── RenderOptions conversion ────────────────────────────────────────────────

pub fn render_options_from_saved(saved: &SavedRenderOptions) -> RenderOptions {
    RenderOptions {
        show_hp_bars: saved.show_hp_bars,
        show_tracers: saved.show_tracers,
        show_torpedoes: saved.show_torpedoes,
        show_planes: saved.show_planes,
        show_smoke: saved.show_smoke,
        show_score: saved.show_score,
        show_timer: saved.show_timer,
        show_kill_feed: saved.show_kill_feed,
        show_player_names: saved.show_player_names,
        show_ship_names: saved.show_ship_names,
        show_capture_points: saved.show_capture_points,
        show_buildings: saved.show_buildings,
        show_turret_direction: saved.show_turret_direction,
        show_consumables: saved.show_consumables,
        show_armament: saved.show_armament,
        show_trails: saved.show_trails,
        show_dead_trails: saved.show_dead_trails,
        show_speed_trails: saved.show_speed_trails,
        show_ship_config: saved.show_ship_config,
        show_dead_ship_names: saved.show_dead_ship_names,
        show_battle_result: saved.show_battle_result,
        show_buffs: saved.show_buffs,
        show_chat: saved.show_chat,
        show_advantage: saved.show_advantage,
        show_score_timer: saved.show_score_timer,
    }
}

fn saved_from_render_options(opts: &RenderOptions) -> SavedRenderOptions {
    SavedRenderOptions {
        show_hp_bars: opts.show_hp_bars,
        show_tracers: opts.show_tracers,
        show_torpedoes: opts.show_torpedoes,
        show_planes: opts.show_planes,
        show_smoke: opts.show_smoke,
        show_score: opts.show_score,
        show_timer: opts.show_timer,
        show_kill_feed: opts.show_kill_feed,
        show_player_names: opts.show_player_names,
        show_ship_names: opts.show_ship_names,
        show_capture_points: opts.show_capture_points,
        show_buildings: opts.show_buildings,
        show_turret_direction: opts.show_turret_direction,
        show_consumables: opts.show_consumables,
        show_dead_ships: false,
        show_dead_ship_names: opts.show_dead_ship_names,
        show_armament: opts.show_armament,
        show_trails: opts.show_trails,
        show_dead_trails: opts.show_dead_trails,
        show_speed_trails: opts.show_speed_trails,
        show_battle_result: opts.show_battle_result,
        show_buffs: opts.show_buffs,
        show_ship_config: opts.show_ship_config,
        show_chat: opts.show_chat,
        show_advantage: opts.show_advantage,
        show_score_timer: opts.show_score_timer,
    }
}

// ─── Commands & Shared State ─────────────────────────────────────────────────

/// Commands sent from the UI thread to the background playback thread.
pub enum PlaybackCommand {
    Play,
    Pause,
    Seek(f32),
    SetSpeed(f32),
    Stop,
}

/// A single frame's rendering data, shared from background to UI thread.
pub struct PlaybackFrame {
    pub commands: Vec<DrawCommand>,
    pub clock_seconds: f32,
    pub frame_index: usize,
    pub total_frames: usize,
    pub game_duration: f32,
}

/// Raw RGBA asset data loaded on the background thread.
/// Uses Arc to share cached data across renderer instances.
pub struct ReplayRendererAssets {
    pub map_image: Option<Arc<RgbaAsset>>,
    pub ship_icons: Arc<HashMap<String, RgbaAsset>>,
    pub plane_icons: Arc<HashMap<String, RgbaAsset>>,
    pub consumable_icons: Arc<HashMap<String, RgbaAsset>>,
    pub death_cause_icons: Arc<HashMap<String, RgbaAsset>>,
    pub powerup_icons: Arc<HashMap<String, RgbaAsset>>,
}

/// egui TextureHandles created on the UI thread.
struct RendererTextures {
    map_texture: Option<TextureHandle>,
    ship_icons: HashMap<String, TextureHandle>,
    /// Gold outline textures for detected-teammate highlight, keyed by the same variant keys as ship_icons.
    ship_icon_outlines: HashMap<String, TextureHandle>,
    plane_icons: HashMap<String, TextureHandle>,
    consumable_icons: HashMap<String, TextureHandle>,
    death_cause_icons: HashMap<String, TextureHandle>,
    powerup_icons: HashMap<String, TextureHandle>,
}

/// Status of the background renderer.
pub enum RendererStatus {
    Loading,
    Ready,
    Error(String),
}

/// State shared between the UI and background threads.
pub struct SharedRendererState {
    pub status: RendererStatus,
    pub frame: Option<PlaybackFrame>,
    pub assets: Option<ReplayRendererAssets>,
    pub playing: bool,
    pub speed: f32,
    pub options: RenderOptions,
    pub show_dead_ships: bool,
    /// Viewport egui context, set by the UI thread on first draw.
    /// Used by the background thread to request repaints after frame updates.
    pub viewport_ctx: Option<egui::Context>,
    /// Pre-parsed timeline events for the entire replay.
    pub(crate) timeline_events: Option<Vec<TimelineEvent>>,
    /// Absolute game clock at which the battle started (after pre-battle countdown).
    /// Used to convert between absolute clock (used by seeking) and elapsed time (used by timeline).
    pub battle_start: f32,
}

/// The cloneable viewport handle stored in TabState.
pub struct ReplayRendererViewer {
    pub title: Arc<String>,
    pub open: Arc<AtomicBool>,
    shared_state: Arc<Mutex<SharedRendererState>>,
    command_tx: mpsc::Sender<PlaybackCommand>,
    textures: Arc<Mutex<Option<RendererTextures>>>,
    /// When set, the main app loop should save these as default render options.
    pub pending_defaults_save: Arc<Mutex<Option<SavedRenderOptions>>>,
    /// Toast notifications for this renderer viewport.
    toasts: crate::tab_state::SharedToasts,
    /// Whether a video export is currently in progress.
    video_exporting: Arc<AtomicBool>,
    /// Data needed for video export (cloned from launch params).
    video_export_data: Arc<VideoExportData>,
    /// Zoom and pan state for the viewport. Persists across frames.
    zoom_pan: Arc<Mutex<ViewportZoomPan>>,
    /// Overlay controls visibility state.
    overlay_state: Arc<Mutex<OverlayState>>,
    /// Annotation/painting layer state.
    annotation_state: Arc<Mutex<AnnotationState>>,
}

/// Data retained for video export. Cloned once at launch time.
struct VideoExportData {
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
    replay_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
}

// ─── Launch ──────────────────────────────────────────────────────────────────

/// Create and launch a replay renderer in a background thread.
///
/// Returns a `ReplayRendererViewer` that can be drawn from the UI thread.
///
/// The `asset_cache` is shared across renderer instances to avoid reloading
/// ship/plane icons and map images from the game files on each launch.
pub fn launch_replay_renderer(
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
    replay_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    saved_options: &SavedRenderOptions,
) -> ReplayRendererViewer {
    let initial_options = render_options_from_saved(saved_options);
    let (command_tx, command_rx) = mpsc::channel();
    let shared_state = Arc::new(Mutex::new(SharedRendererState {
        status: RendererStatus::Loading,
        frame: None,
        assets: None,
        playing: false,
        speed: 20.0,
        options: initial_options,
        show_dead_ships: saved_options.show_dead_ships,
        viewport_ctx: None,
        timeline_events: None,
        battle_start: 0.0,
    }));

    let title = Arc::new(format!("Replay Renderer - {replay_name}"));

    let video_export_data = Arc::new(VideoExportData {
        raw_meta: raw_meta.clone(),
        packet_data: packet_data.clone(),
        map_name: map_name.clone(),
        replay_name,
        game_duration,
        wows_data: wows_data.clone(),
        asset_cache: Arc::clone(&asset_cache),
    });

    let viewer = ReplayRendererViewer {
        title,
        open: Arc::new(AtomicBool::new(true)),
        shared_state: Arc::clone(&shared_state),
        command_tx,
        textures: Arc::new(Mutex::new(None)),
        pending_defaults_save: Arc::new(Mutex::new(None)),
        toasts: Arc::new(parking_lot::Mutex::new(egui_notify::Toasts::default())),
        video_exporting: Arc::new(AtomicBool::new(false)),
        video_export_data,
        zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
        overlay_state: Arc::new(Mutex::new(OverlayState::default())),
        annotation_state: Arc::new(Mutex::new(AnnotationState::default())),
    };

    let open = Arc::clone(&viewer.open);

    std::thread::spawn(move || {
        playback_thread(
            raw_meta,
            packet_data,
            map_name,
            game_duration,
            wows_data,
            asset_cache,
            shared_state,
            command_rx,
            open,
        );
    });

    viewer
}

// ─── Background Thread ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn playback_thread(
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    shared_state: Arc<Mutex<SharedRendererState>>,
    command_rx: mpsc::Receiver<PlaybackCommand>,
    open: Arc<AtomicBool>,
) {
    // 1. Get file tree, pkg loader, game metadata, and game constants from the app
    let (file_tree, pkg_loader, game_metadata, game_constants) = {
        let data = wows_data.read();
        let gm = match data.game_metadata.clone() {
            Some(gm) => gm,
            None => {
                shared_state.lock().status = RendererStatus::Error("Game metadata not loaded".to_string());
                return;
            }
        };
        (data.file_tree.clone(), Arc::clone(&data.pkg_loader), gm, Arc::clone(&data.game_constants))
    };

    // 2. Load visual assets (cached across renderer instances)
    let map_info = {
        let mut cache = asset_cache.lock();
        let ship_icons = cache.get_or_load_ship_icons(&file_tree, &pkg_loader);
        let plane_icons = cache.get_or_load_plane_icons(&file_tree, &pkg_loader);
        let consumable_icons = cache.get_or_load_consumable_icons(&file_tree, &pkg_loader);
        let death_cause_icons = cache.get_or_load_death_cause_icons(&file_tree, &pkg_loader);
        let powerup_icons = cache.get_or_load_powerup_icons(&file_tree, &pkg_loader);
        let (map_image, map_info) = cache.get_or_load_map(&map_name, &file_tree, &pkg_loader);

        shared_state.lock().assets = Some(ReplayRendererAssets {
            map_image,
            ship_icons,
            plane_icons,
            consumable_icons,
            death_cause_icons,
            powerup_icons,
        });

        map_info
    };
    // Drop references to file_tree/pkg_loader early — no longer needed
    drop(file_tree);
    drop(pkg_loader);

    // 3. Parse replay file
    let replay_file = match ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone()) {
        Ok(rf) => rf,
        Err(e) => {
            shared_state.lock().status = RendererStatus::Error(format!("Failed to parse replay: {:?}", e));
            return;
        }
    };

    // 4. Create controller and renderer
    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut controller = BattleController::new(&replay_file.meta, &*game_metadata, Some(&game_constants));
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    let mut renderer = MinimapRenderer::new(map_info.clone(), &game_metadata, version, RenderOptions::default());

    // Parse all packets, tracking frame boundaries
    let frame_duration = 1.0 / SNAPSHOTS_PER_SECOND;
    let estimated_frames = (game_duration * SNAPSHOTS_PER_SECOND) as usize + 1;

    // Pre-parse: build a mapping of packet offsets to clock times
    // so we can efficiently seek by re-parsing
    let mut frame_snapshots: Vec<FrameSnapshot> = Vec::with_capacity(estimated_frames);
    let mut last_rendered_frame: i64 = -1;
    let mut prev_clock = GameClock(0.0);

    let full_packet_data = &replay_file.packet_data;
    let mut remaining = &full_packet_data[..];

    while !remaining.is_empty() {
        let offset_before = full_packet_data.len() - remaining.len();
        match parser.parse_packet(remaining) {
            Ok((rest, packet)) => {
                if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                    renderer.populate_players(&controller);
                    renderer.update_squadron_info(&controller);
                    renderer.update_ship_abilities(&controller);

                    let target_frame = (prev_clock.seconds() / frame_duration) as i64;
                    while last_rendered_frame < target_frame {
                        last_rendered_frame += 1;
                        let commands = renderer.draw_frame(&controller);
                        frame_snapshots
                            .push(FrameSnapshot { packet_offset: offset_before, clock: prev_clock.seconds() });

                        // Store the first frame immediately
                        if frame_snapshots.len() == 1 {
                            let mut state = shared_state.lock();
                            state.frame = Some(PlaybackFrame {
                                commands,
                                clock_seconds: prev_clock.seconds(),
                                frame_index: 0,
                                total_frames: estimated_frames,
                                game_duration,
                            });
                        }
                    }
                    prev_clock = packet.clock;
                } else if prev_clock.seconds() == 0.0 {
                    prev_clock = packet.clock;
                }

                controller.process(&packet);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    // Final tick
    if prev_clock.seconds() > 0.0 {
        renderer.populate_players(&controller);
        renderer.update_squadron_info(&controller);
        renderer.update_ship_abilities(&controller);
        let target_frame = (prev_clock.seconds() / frame_duration) as i64;
        while last_rendered_frame < target_frame {
            last_rendered_frame += 1;
            frame_snapshots.push(FrameSnapshot { packet_offset: full_packet_data.len(), clock: prev_clock.seconds() });
        }
    }
    controller.finish();

    let actual_total_frames = frame_snapshots.len();
    let actual_game_duration = frame_snapshots.last().map(|s| s.clock).unwrap_or(game_duration);

    // 4. Event extraction pass — second full parse for timeline events
    let (timeline_events, battle_start) = ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone())
        .ok()
        .map(|event_replay| extract_timeline_events(&event_replay, &game_metadata, Some(&game_constants)))
        .unwrap_or_default();
    {
        let mut state = shared_state.lock();
        state.timeline_events = Some(timeline_events);
        state.battle_start = battle_start;
    }

    // Mark as ready
    shared_state.lock().status = RendererStatus::Ready;

    // 5. Playback loop — respond to UI commands
    //
    // We keep a "live" ReplayFile + BattleController + MinimapRenderer that
    // represent the game state at the current frame. This lets us re-draw with
    // different RenderOptions without re-parsing the replay.
    //
    // For seeking or advancing, we re-parse from the beginning to the target
    // frame (rebuilding the live state). For SetOptions, we just update the
    // renderer options and call draw_frame() again — no re-parsing needed.
    let mut current_frame: usize = 0;
    let mut playing = false;
    let mut speed: f32 = 20.0;
    let mut last_advance = std::time::Instant::now();

    // Rebuild live state at frame 0 — drop the initial-parse objects first
    drop(controller);
    drop(renderer);
    drop(replay_file);
    // `replay_file` from the initial parse is no longer needed — create a fresh one
    // that the live controller will borrow from for the duration of the playback loop.
    let mut live_replay = match ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone()) {
        Ok(rf) => rf,
        Err(_) => return,
    };
    let mut live_controller = BattleController::new(&live_replay.meta, &*game_metadata, Some(&game_constants));
    let initial_opts = shared_state.lock().options.clone();
    let mut live_renderer = MinimapRenderer::new(map_info.clone(), &game_metadata, version, initial_opts);

    // Parse live state up to frame 0 so it matches the initially displayed frame
    if !frame_snapshots.is_empty() {
        parse_to_clock(
            &live_replay,
            &game_metadata,
            &mut live_controller,
            &mut live_renderer,
            frame_snapshots[0].clock,
            frame_duration,
        );
    }

    /// Helper: parse replay packets up to `target_clock`, feeding them into
    /// the given controller and renderer.
    fn parse_to_clock(
        replay_file: &ReplayFile,
        game_metadata: &GameMetadataProvider,
        controller: &mut BattleController<'_, '_, GameMetadataProvider>,
        renderer: &mut MinimapRenderer<'_>,
        target_clock: f32,
        frame_duration: f32,
    ) {
        let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
        let mut remaining = &replay_file.packet_data[..];
        let mut prev_clock = GameClock(0.0);

        while !remaining.is_empty() {
            match parser.parse_packet(remaining) {
                Ok((rest, packet)) => {
                    if packet.clock.seconds() > target_clock + frame_duration {
                        break;
                    }
                    // Stop if clock resets to 0 after game started — those are post-game packets
                    if prev_clock.seconds() > 0.0 && packet.clock.seconds() == 0.0 {
                        break;
                    }
                    if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                        renderer.populate_players(controller);
                        renderer.update_squadron_info(controller);
                        renderer.update_ship_abilities(controller);
                        let dead_ships = controller.dead_ships();
                        let minimap_positions = controller.minimap_positions();
                        renderer.record_positions(controller, prev_clock, |eid| {
                            // Skip dead ships
                            if let Some(dead) = dead_ships.get(eid) {
                                if prev_clock >= dead.clock {
                                    return false;
                                }
                            }
                            // Only record detected ships (visible on minimap)
                            minimap_positions.get(eid).map(|mm| mm.visible).unwrap_or(false)
                        });
                    }
                    prev_clock = packet.clock;
                    controller.process(&packet);
                    remaining = rest;
                }
                Err(_) => break,
            }
        }

        renderer.populate_players(controller);
        renderer.update_squadron_info(controller);
        renderer.update_ship_abilities(controller);
    }

    // Request a repaint of the viewport from the background thread.
    // Uses the egui Context stored by the UI thread on first draw.
    let request_repaint = |state: &Arc<Mutex<SharedRendererState>>| {
        if let Some(ref ctx) = state.lock().viewport_ctx {
            ctx.request_repaint();
        }
    };

    /// Rebuild live_replay/live_controller/live_renderer from scratch,
    /// parsing up to `$target_clock`. The macro is needed because Rust's
    /// borrow checker won't allow passing `&mut live_replay` and
    /// `&mut live_controller` (which borrows from `live_replay`) to the same function.
    macro_rules! rebuild_live_state {
        ($target_clock:expr) => {{
            let mut new_replay = match ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone()) {
                Ok(rf) => rf,
                Err(_) => continue,
            };
            std::mem::swap(&mut live_replay, &mut new_replay);
            // old replay is now in new_replay and will be dropped at end of block
            live_controller = BattleController::new(&live_replay.meta, &*game_metadata, Some(&game_constants));
            let current_opts = shared_state.lock().options.clone();
            live_renderer = MinimapRenderer::new(map_info.clone(), &*game_metadata, version, current_opts);
            parse_to_clock(
                &live_replay,
                &game_metadata,
                &mut live_controller,
                &mut live_renderer,
                $target_clock,
                frame_duration,
            );
        }};
    }

    loop {
        if !open.load(Ordering::Relaxed) {
            break;
        }

        // Process all pending commands
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                PlaybackCommand::Play => {
                    playing = true;
                    last_advance = std::time::Instant::now();
                }
                PlaybackCommand::Pause => {
                    playing = false;
                }
                PlaybackCommand::Seek(time) => {
                    // Find first frame with clock >= target time
                    let target = frame_snapshots
                        .iter()
                        .position(|s| s.clock >= time)
                        .unwrap_or(actual_total_frames.saturating_sub(1));
                    current_frame = target;
                    let target_clock =
                        frame_snapshots.get(current_frame).map(|s| s.clock).unwrap_or(actual_game_duration);

                    rebuild_live_state!(target_clock);

                    live_renderer.options = shared_state.lock().options.clone();
                    let commands = live_renderer.draw_frame(&live_controller);
                    shared_state.lock().frame = Some(PlaybackFrame {
                        commands,
                        clock_seconds: target_clock,
                        frame_index: current_frame,
                        total_frames: actual_total_frames,
                        game_duration: actual_game_duration,
                    });
                    request_repaint(&shared_state);
                }
                PlaybackCommand::SetSpeed(s) => {
                    speed = s;
                }
                PlaybackCommand::Stop => {
                    return;
                }
            }
        }

        if playing && actual_total_frames > 0 {
            let now = std::time::Instant::now();
            let dt = now.duration_since(last_advance).as_secs_f32();
            let base_fps = actual_total_frames as f32 / actual_game_duration.max(1.0);
            let frames_to_advance = dt * base_fps * speed;

            if frames_to_advance >= 1.0 {
                current_frame = (current_frame + frames_to_advance as usize).min(actual_total_frames - 1);
                last_advance = now;

                if current_frame >= actual_total_frames - 1 {
                    playing = false;
                    shared_state.lock().playing = false;
                }

                let target_clock = if current_frame < frame_snapshots.len() {
                    frame_snapshots[current_frame].clock
                } else {
                    actual_game_duration
                };

                rebuild_live_state!(target_clock);

                live_renderer.options = shared_state.lock().options.clone();
                let commands = live_renderer.draw_frame(&live_controller);
                shared_state.lock().frame = Some(PlaybackFrame {
                    commands,
                    clock_seconds: target_clock,
                    frame_index: current_frame,
                    total_frames: actual_total_frames,
                    game_duration: actual_game_duration,
                });
                request_repaint(&shared_state);
            }
        }

        // When paused, check if options changed and re-render if so
        // (armament/trail toggling requires backend to re-emit draw commands)
        if !playing {
            let new_opts = shared_state.lock().options.clone();
            if live_renderer.options.show_armament != new_opts.show_armament
                || live_renderer.options.show_trails != new_opts.show_trails
                || live_renderer.options.show_dead_trails != new_opts.show_dead_trails
                || live_renderer.options.show_speed_trails != new_opts.show_speed_trails
                || live_renderer.options.show_player_names != new_opts.show_player_names
                || live_renderer.options.show_ship_names != new_opts.show_ship_names
                || live_renderer.options.show_ship_config != new_opts.show_ship_config
                || live_renderer.options.show_chat != new_opts.show_chat
                || live_renderer.options.show_advantage != new_opts.show_advantage
                || live_renderer.options.show_score_timer != new_opts.show_score_timer
            {
                live_renderer.options = new_opts;
                let commands = live_renderer.draw_frame(&live_controller);
                shared_state.lock().frame = Some(PlaybackFrame {
                    commands,
                    clock_seconds: frame_snapshots.get(current_frame).map(|s| s.clock).unwrap_or(actual_game_duration),
                    frame_index: current_frame,
                    total_frames: actual_total_frames,
                    game_duration: actual_game_duration,
                });
                request_repaint(&shared_state);
            }
        }

        // Sleep to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(if playing { 8 } else { 16 }));
    }
}

struct FrameSnapshot {
    #[allow(dead_code)]
    packet_offset: usize,
    clock: f32,
}

// ─── Event Timeline ──────────────────────────────────────────────────────────

pub(crate) enum TimelineEventKind {
    HealthLost {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
        percent_lost: f32,
        old_hp: f32,
        new_hp: f32,
        max_hp: f32,
    },
    Death {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
        killer_ship: String,
        killer_player: String,
    },
    CapContested {
        cap_label: String,
        owner_is_friendly: bool,
    },
    CapFlipped {
        cap_label: String,
        capturer_is_friendly: bool,
    },
    CapBeingCaptured {
        cap_label: String,
        capturer_is_friendly: bool,
    },
    RadarUsed {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
    },
    AdvantageChanged {
        label: String,
        is_friendly: bool,
    },
    Disconnected {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
    },
}

pub(crate) struct TimelineEvent {
    clock: f32,
    kind: TimelineEventKind,
}

fn event_color(is_friendly: bool) -> Color32 {
    if is_friendly { FRIENDLY_COLOR } else { ENEMY_COLOR }
}

fn format_timeline_event(event: &TimelineEvent) -> String {
    let mins = event.clock as u32 / 60;
    let secs = event.clock as u32 % 60;
    let time = format!("{:02}:{:02}", mins, secs);
    let desc = match &event.kind {
        TimelineEventKind::HealthLost { ship_name, player_name, percent_lost, old_hp, new_hp, max_hp, .. } => {
            format!(
                "{} ({}) -{}% HP ({:.0}/{:.0} -> {:.0}/{:.0})",
                ship_name,
                player_name,
                (percent_lost * 100.0) as u32,
                old_hp,
                max_hp,
                new_hp,
                max_hp
            )
        }
        TimelineEventKind::Death { ship_name, player_name, killer_ship, killer_player, .. } => {
            if killer_ship.is_empty() {
                format!("{} ({}) destroyed", ship_name, player_name)
            } else {
                format!("{} ({}) destroyed by {} ({})", ship_name, player_name, killer_ship, killer_player)
            }
        }
        TimelineEventKind::CapContested { cap_label, .. } => format!("{} contested", cap_label),
        TimelineEventKind::CapFlipped { cap_label, .. } => format!("{} captured", cap_label),
        TimelineEventKind::CapBeingCaptured { cap_label, .. } => format!("{} being captured", cap_label),
        TimelineEventKind::RadarUsed { ship_name, player_name, .. } => {
            format!("{} ({}) used radar", ship_name, player_name)
        }
        TimelineEventKind::AdvantageChanged { label, .. } => label.clone(),
        TimelineEventKind::Disconnected { ship_name, player_name, .. } => {
            format!("{} ({}) disconnected", ship_name, player_name)
        }
    };
    format!("[{}] {}", time, desc)
}

/// Parse the entire replay and extract significant game events for the timeline.
/// Returns `(events, battle_start)` where `battle_start` is the absolute game clock
/// at which the battle started. Event clocks are adjusted to elapsed time.
fn extract_timeline_events(
    replay_file: &ReplayFile,
    game_metadata: &GameMetadataProvider,
    game_constants: Option<&GameConstants>,
) -> (Vec<TimelineEvent>, f32) {
    let mut events = Vec::new();
    let mut controller = BattleController::new(&replay_file.meta, game_metadata, game_constants);
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());

    // Player info lookups (populated once players are available)
    let mut ship_names: HashMap<EntityId, String> = HashMap::new();
    let mut player_names: HashMap<EntityId, String> = HashMap::new();
    let mut is_friendly: HashMap<EntityId, bool> = HashMap::new();
    let mut viewer_team_id: Option<i64> = None;
    let mut players_populated = false;

    // Health tracking: entity → (window_start_clock, health_at_window_start)
    let mut health_windows: HashMap<EntityId, (f32, f32)> = HashMap::new();

    // Kill tracking
    let mut last_kill_count: usize = 0;

    // Cap tracking: cap_index → (previous has_invaders, previous team_id)
    let mut cap_prev_contested: HashMap<usize, bool> = HashMap::new();
    let mut cap_prev_team: HashMap<usize, i64> = HashMap::new();
    let mut cap_prev_invader_team: HashMap<usize, i64> = HashMap::new();

    // Radar tracking: entity → number of radar activations seen so far
    let mut radar_counts: HashMap<EntityId, usize> = HashMap::new();

    // Advantage tracking
    use wows_minimap_renderer::advantage::{self, ScoringParams, TeamAdvantage, TeamState};
    let mut prev_advantage: TeamAdvantage = TeamAdvantage::Even;
    let mut advantage_check_clock: f32 = 0.0;

    let mut remaining = &replay_file.packet_data[..];
    let mut prev_clock = GameClock(0.0);

    while !remaining.is_empty() {
        match parser.parse_packet(remaining) {
            Ok((rest, packet)) => {
                if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                    // Populate player info on first tick where players are available
                    if !players_populated {
                        let players = controller.player_entities();
                        if !players.is_empty() {
                            for (entity_id, player) in players {
                                let ship_name = game_metadata
                                    .localized_name_from_param(player.vehicle())
                                    .map(|s| s.to_string())
                                    .unwrap_or_default();
                                ship_names.insert(*entity_id, ship_name);
                                player_names.insert(*entity_id, player.initial_state().username().to_string());

                                let relation = player.relation();
                                let friendly = relation.is_self() || relation.is_ally();
                                is_friendly.insert(*entity_id, friendly);

                                if relation.is_self() {
                                    viewer_team_id = Some(player.initial_state().team_id());
                                }
                            }
                            players_populated = true;
                        }
                    }

                    let clock = prev_clock.seconds();

                    // --- Health loss detection ---
                    for (entity_id, entity) in controller.entities_by_id() {
                        if let Some(vehicle_rc) = entity.vehicle_ref() {
                            let vehicle = vehicle_rc.borrow();
                            let props = vehicle.props();
                            let current_health = props.health();
                            let max_health = props.max_health();

                            if max_health <= 0.0 {
                                continue;
                            }

                            if let Some((window_start, health_at_start)) = health_windows.get_mut(entity_id) {
                                if clock - *window_start >= 3.0 {
                                    let loss = (*health_at_start - current_health) / max_health;
                                    if loss > 0.25 {
                                        let sname = ship_names.get(entity_id).cloned().unwrap_or_default();
                                        let pname = player_names.get(entity_id).cloned().unwrap_or_default();
                                        let friendly = is_friendly.get(entity_id).copied().unwrap_or(false);
                                        events.push(TimelineEvent {
                                            clock,
                                            kind: TimelineEventKind::HealthLost {
                                                ship_name: sname,
                                                player_name: pname,
                                                is_friendly: friendly,
                                                percent_lost: loss,
                                                old_hp: *health_at_start,
                                                new_hp: current_health,
                                                max_hp: max_health,
                                            },
                                        });
                                    }
                                    *window_start = clock;
                                    *health_at_start = current_health;
                                }
                            } else if props.is_alive() {
                                health_windows.insert(*entity_id, (clock, current_health));
                            }
                        }
                    }

                    // --- Death detection ---
                    let kills = controller.kills();
                    if kills.len() > last_kill_count {
                        for kill in &kills[last_kill_count..] {
                            let victim_ship = ship_names.get(&kill.victim).cloned().unwrap_or_default();
                            let victim_player = player_names.get(&kill.victim).cloned().unwrap_or_default();
                            let friendly = is_friendly.get(&kill.victim).copied().unwrap_or(false);
                            let killer_ship = ship_names.get(&kill.killer).cloned().unwrap_or_default();
                            let killer_player = player_names.get(&kill.killer).cloned().unwrap_or_default();
                            events.push(TimelineEvent {
                                clock: kill.clock.seconds(),
                                kind: TimelineEventKind::Death {
                                    ship_name: victim_ship,
                                    player_name: victim_player,
                                    is_friendly: friendly,
                                    killer_ship,
                                    killer_player,
                                },
                            });
                        }
                        last_kill_count = kills.len();
                    }

                    // --- Capture point events ---
                    let viewer_team = viewer_team_id.unwrap_or(0);
                    for cap in controller.capture_points() {
                        let cap_idx = cap.index;

                        let cap_label = if cap.control_point_type == 5 {
                            "Flag".to_string()
                        } else {
                            ((b'A' + cap_idx as u8) as char).to_string()
                        };

                        // Cap contested: both_inside transitions false → true
                        let prev_contested = cap_prev_contested.get(&cap_idx).copied().unwrap_or(false);
                        if cap.both_inside && !prev_contested {
                            events.push(TimelineEvent {
                                clock,
                                kind: TimelineEventKind::CapContested {
                                    cap_label: cap_label.clone(),
                                    owner_is_friendly: cap.team_id == viewer_team,
                                },
                            });
                        }
                        cap_prev_contested.insert(cap_idx, cap.both_inside);

                        // Cap being captured (uncontested): invader_team transitions from
                        // no-invader (<0) to a valid team (>=0), while not contested
                        let prev_invader = cap_prev_invader_team.get(&cap_idx).copied().unwrap_or(-1);
                        if cap.invader_team >= 0 && prev_invader < 0 && !cap.both_inside {
                            events.push(TimelineEvent {
                                clock,
                                kind: TimelineEventKind::CapBeingCaptured {
                                    cap_label: cap_label.clone(),
                                    capturer_is_friendly: cap.invader_team == viewer_team,
                                },
                            });
                        }
                        cap_prev_invader_team.insert(cap_idx, cap.invader_team);

                        // Cap flipped: team_id changes
                        if let Some(&prev_team) = cap_prev_team.get(&cap_idx)
                            && cap.team_id != prev_team
                            && cap.team_id >= 0
                        {
                            events.push(TimelineEvent {
                                clock,
                                kind: TimelineEventKind::CapFlipped {
                                    cap_label,
                                    capturer_is_friendly: cap.team_id == viewer_team,
                                },
                            });
                        }
                        cap_prev_team.insert(cap_idx, cap.team_id);
                    }

                    // --- Radar activation detection ---
                    for (entity_id, consumables) in controller.active_consumables() {
                        let radar_count =
                            consumables.iter().filter(|c| c.consumable == Recognized::Known(Consumable::Radar)).count();
                        let prev_count = radar_counts.get(entity_id).copied().unwrap_or(0);
                        if radar_count > prev_count {
                            let sname = ship_names.get(entity_id).cloned().unwrap_or_default();
                            let pname = player_names.get(entity_id).cloned().unwrap_or_default();
                            let friendly = is_friendly.get(entity_id).copied().unwrap_or(false);
                            events.push(TimelineEvent {
                                clock,
                                kind: TimelineEventKind::RadarUsed {
                                    ship_name: sname,
                                    player_name: pname,
                                    is_friendly: friendly,
                                },
                            });
                        }
                        radar_counts.insert(*entity_id, radar_count);
                    }

                    // --- Advantage change detection (check every ~3 seconds) ---
                    if clock - advantage_check_clock >= 3.0 && players_populated {
                        advantage_check_clock = clock;

                        let viewer_team = viewer_team_id.unwrap_or(0);
                        let swap = viewer_team == 1;
                        let players = controller.player_entities();
                        let entities = controller.entities_by_id();

                        let mut teams = [
                            TeamState {
                                score: 0,
                                uncontested_caps: 0,
                                total_hp: 0.0,
                                max_hp: 0.0,
                                ships_alive: 0,
                                ships_total: 0,
                                ships_known: 0,
                            },
                            TeamState {
                                score: 0,
                                uncontested_caps: 0,
                                total_hp: 0.0,
                                max_hp: 0.0,
                                ships_alive: 0,
                                ships_total: 0,
                                ships_known: 0,
                            },
                        ];

                        let scores = controller.team_scores();
                        if scores.len() >= 2 {
                            teams[0].score = scores[0].score;
                            teams[1].score = scores[1].score;
                        }

                        for cp in controller.capture_points() {
                            if !cp.is_enabled || cp.has_invaders {
                                continue;
                            }
                            if cp.team_id == 0 {
                                teams[0].uncontested_caps += 1;
                            } else if cp.team_id == 1 {
                                teams[1].uncontested_caps += 1;
                            }
                        }

                        for (entity_id, player) in players {
                            let team = player.initial_state().team_id() as usize;
                            if team > 1 {
                                continue;
                            }
                            teams[team].ships_total += 1;
                            if let Some(entity) = entities.get(entity_id) {
                                if let Some(vehicle) = entity.vehicle_ref() {
                                    let v = vehicle.borrow();
                                    let props = v.props();
                                    teams[team].ships_known += 1;
                                    teams[team].max_hp += props.max_health();
                                    if props.is_alive() {
                                        teams[team].ships_alive += 1;
                                        teams[team].total_hp += props.health();
                                    }
                                }
                            }
                        }

                        let scoring = controller
                            .scoring_rules()
                            .map(|r| ScoringParams {
                                team_win_score: r.team_win_score,
                                hold_reward: r.hold_reward,
                                hold_period: r.hold_period,
                            })
                            .unwrap_or(ScoringParams { team_win_score: 1000, hold_reward: 3, hold_period: 5.0 });

                        let result =
                            advantage::calculate_advantage(&teams[0], &teams[1], &scoring, controller.time_left());

                        // Swap so Team0 = friendly
                        let current = if swap {
                            match result.advantage {
                                TeamAdvantage::Team0(level) => TeamAdvantage::Team1(level),
                                TeamAdvantage::Team1(level) => TeamAdvantage::Team0(level),
                                other => other,
                            }
                        } else {
                            result.advantage
                        };

                        if current != prev_advantage {
                            let level_label = |adv: &TeamAdvantage| -> Option<(&str, bool)> {
                                match adv {
                                    TeamAdvantage::Team0(level) => Some((level.label(), true)),
                                    TeamAdvantage::Team1(level) => Some((level.label(), false)),
                                    TeamAdvantage::Even => None,
                                }
                            };

                            let label = match (level_label(&prev_advantage), level_label(&current)) {
                                // Gained advantage from even
                                (None, Some((new_label, _))) => {
                                    format!("{} advantage gained", new_label)
                                }
                                // Lost advantage to even
                                (Some((old_label, _)), None) => {
                                    format!("{} advantage lost", old_label)
                                }
                                // Same team, level changed
                                (Some((old_label, old_friendly)), Some((new_label, new_friendly)))
                                    if old_friendly == new_friendly =>
                                {
                                    let old_val = match &prev_advantage {
                                        TeamAdvantage::Team0(l) | TeamAdvantage::Team1(l) => Some(*l),
                                        _ => None,
                                    };
                                    let new_val = match &current {
                                        TeamAdvantage::Team0(l) | TeamAdvantage::Team1(l) => Some(*l),
                                        _ => None,
                                    };
                                    // Compare by discriminant order (Absolute=0 > Strong=1 > Moderate=2 > Weak=3)
                                    if let (Some(o), Some(n)) = (old_val, new_val) {
                                        if (n as u8) < (o as u8) {
                                            format!("{} advantage gained", new_label)
                                        } else {
                                            format!("Dropped to {} advantage", new_label)
                                        }
                                    } else {
                                        format!("{} advantage", new_label)
                                    }
                                }
                                // Advantage flipped teams
                                (Some(_), Some((new_label, _))) => {
                                    format!("{} advantage gained", new_label)
                                }
                                _ => String::new(),
                            };

                            if !label.is_empty() {
                                let is_friendly = match &current {
                                    TeamAdvantage::Team0(_) => true,
                                    TeamAdvantage::Team1(_) => false,
                                    TeamAdvantage::Even => match &prev_advantage {
                                        TeamAdvantage::Team1(_) => true, // enemy lost advantage = good for us
                                        _ => false,
                                    },
                                };
                                events.push(TimelineEvent {
                                    clock,
                                    kind: TimelineEventKind::AdvantageChanged { label, is_friendly },
                                });
                            }
                            prev_advantage = current;
                        }
                    }

                    prev_clock = packet.clock;
                } else if prev_clock.seconds() == 0.0 {
                    prev_clock = packet.clock;
                }

                controller.process(&packet);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    controller.finish();

    // --- Disconnect events (non-death) ---
    {
        use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
        for (entity_id, player) in controller.player_entities() {
            for info in player.connection_change_info() {
                if info.event_kind() == ConnectionChangeKind::Disconnected && !info.had_death_event() {
                    let sname = ship_names.get(entity_id).cloned().unwrap_or_default();
                    let pname = player_names.get(entity_id).cloned().unwrap_or_default();
                    let friendly = is_friendly.get(entity_id).copied().unwrap_or(false);
                    events.push(TimelineEvent {
                        clock: info.at_game_duration().as_secs_f32(),
                        kind: TimelineEventKind::Disconnected {
                            ship_name: sname,
                            player_name: pname,
                            is_friendly: friendly,
                        },
                    });
                }
            }
        }
    }

    // Translate event times from absolute game clock to elapsed time since battle start
    let battle_start = controller.battle_start_clock().map(|c| c.seconds()).unwrap_or(0.0);
    for event in &mut events {
        event.clock = controller.game_clock_to_elapsed(event.clock);
    }

    // Sort events by clock time
    events.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap_or(std::cmp::Ordering::Equal));
    (events, battle_start)
}

// ─── Video Export ────────────────────────────────────────────────────────────

/// Spawn a background thread that renders the replay to an MP4 video file
/// using the software renderer (`ImageTarget`) and `VideoEncoder`.
#[allow(clippy::too_many_arguments)]
fn save_as_video(
    output_path: String,
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
    game_duration: f32,
    options: RenderOptions,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    toasts: crate::tab_state::SharedToasts,
    video_exporting: Arc<AtomicBool>,
) {
    video_exporting.store(true, Ordering::Relaxed);
    toasts.lock().info("Exporting video...");

    std::thread::spawn(move || {
        let result = render_video_blocking(
            &output_path,
            &raw_meta,
            &packet_data,
            &map_name,
            game_duration,
            options,
            &wows_data,
            &asset_cache,
        );

        match result {
            Ok(()) => {
                toasts.lock().success(format!("Video saved to {}", output_path));
            }
            Err(e) => {
                toasts.lock().error(format!("Video export failed: {}", e));
            }
        }
        video_exporting.store(false, Ordering::Relaxed);
    });
}

/// Spawn a background thread that renders the replay to a temporary MP4 file,
/// then copies it to the clipboard.
fn render_video_to_clipboard(
    file_name: String,
    export_data: Arc<VideoExportData>,
    options: RenderOptions,
    toasts: crate::tab_state::SharedToasts,
    video_exporting: Arc<AtomicBool>,
) {
    video_exporting.store(true, Ordering::Relaxed);
    toasts.lock().info("Rendering video to clipboard...");

    std::thread::spawn(move || {
        let temp_dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => {
                toasts.lock().error(format!("Failed to create temp dir: {}", e));
                video_exporting.store(false, Ordering::Relaxed);
                return;
            }
        };
        let output_path = temp_dir.path().join(&file_name);
        let output_str = output_path.to_string_lossy().to_string();

        let result = render_video_blocking(
            &output_str,
            &export_data.raw_meta,
            &export_data.packet_data,
            &export_data.map_name,
            export_data.game_duration,
            options,
            &export_data.wows_data,
            &export_data.asset_cache,
        );

        match result {
            Ok(()) => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set().file_list(&[output_path]);
                    // Leak the tempdir so the file persists until the OS cleans it up
                    // or the process exits — clipboard consumers need the file to exist.
                    std::mem::forget(temp_dir);
                    toasts.lock().success("Video copied to clipboard");
                } else {
                    toasts.lock().error("Failed to open clipboard");
                }
            }
            Err(e) => {
                toasts.lock().error(format!("Video export failed: {}", e));
            }
        }
        video_exporting.store(false, Ordering::Relaxed);
    });
}

/// Blocking implementation of the video export.
#[allow(clippy::too_many_arguments)]
fn render_video_blocking(
    output_path: &str,
    raw_meta: &[u8],
    packet_data: &[u8],
    map_name: &str,
    game_duration: f32,
    options: RenderOptions,
    wows_data: &SharedWoWsData,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
) -> rootcause::Result<()> {
    use wows_minimap_renderer::drawing::ImageTarget;
    use wows_minimap_renderer::video::VideoEncoder;

    // Get game metadata and load assets for the software renderer
    let (file_tree, pkg_loader, game_metadata, game_constants) = {
        let data = wows_data.read();
        let gm = data.game_metadata.clone().ok_or_else(|| report!("Game metadata not loaded"))?;
        (data.file_tree.clone(), Arc::clone(&data.pkg_loader), gm, Arc::clone(&data.game_constants))
    };

    // Load assets — reuse cached raw RGBA data and convert to image types
    let (
        map_image_rgb,
        ship_icons_rgba,
        plane_icons_rgba,
        consumable_icons_rgba,
        death_cause_icons,
        powerup_icons,
        map_info,
    ) = {
        let mut cache = asset_cache.lock();
        let ship_raw = cache.get_or_load_ship_icons(&file_tree, &pkg_loader);
        let plane_raw = cache.get_or_load_plane_icons(&file_tree, &pkg_loader);
        let consumable_raw = cache.get_or_load_consumable_icons(&file_tree, &pkg_loader);
        let death_cause_raw = cache.get_or_load_death_cause_icons(&file_tree, &pkg_loader);
        let powerup_raw = cache.get_or_load_powerup_icons(&file_tree, &pkg_loader);
        let (map_raw, map_info) = cache.get_or_load_map(map_name, &file_tree, &pkg_loader);

        // Convert cached RGBA bytes back to image types for ImageTarget
        let ship_icons: HashMap<String, image::RgbaImage> = ship_raw
            .iter()
            .map(|(k, (data, w, h))| (k.clone(), image::RgbaImage::from_raw(*w, *h, data.clone()).unwrap()))
            .collect();

        let plane_icons: HashMap<String, image::RgbaImage> = plane_raw
            .iter()
            .map(|(k, (data, w, h))| (k.clone(), image::RgbaImage::from_raw(*w, *h, data.clone()).unwrap()))
            .collect();

        let consumable_icons: HashMap<String, image::RgbaImage> = consumable_raw
            .iter()
            .map(|(k, (data, w, h))| (k.clone(), image::RgbaImage::from_raw(*w, *h, data.clone()).unwrap()))
            .collect();

        let map_image = map_raw.as_ref().and_then(|arc| {
            let (data, w, h) = &**arc;
            // Cached data is RGBA, convert to RGB for ImageTarget
            let rgba = image::RgbaImage::from_raw(*w, *h, data.clone())?;
            Some(image::DynamicImage::ImageRgba8(rgba).into_rgb8())
        });

        let death_cause_icons: HashMap<String, image::RgbaImage> = death_cause_raw
            .iter()
            .map(|(k, (data, w, h))| (k.clone(), image::RgbaImage::from_raw(*w, *h, data.clone()).unwrap()))
            .collect();

        let powerup_icons: HashMap<String, image::RgbaImage> = powerup_raw
            .iter()
            .map(|(k, (data, w, h))| (k.clone(), image::RgbaImage::from_raw(*w, *h, data.clone()).unwrap()))
            .collect();

        (map_image, ship_icons, plane_icons, consumable_icons, death_cause_icons, powerup_icons, map_info)
    };

    // Build replay parser components
    let replay_file = ReplayFile::from_decrypted_parts(raw_meta.to_vec(), packet_data.to_vec())
        .map_err(|e| report!("Failed to parse replay: {:?}", e))?;

    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut controller = BattleController::new(&replay_file.meta, &*game_metadata, Some(&game_constants));
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    let mut renderer = MinimapRenderer::new(map_info, &game_metadata, version, options);
    let mut target = ImageTarget::new(
        map_image_rgb,
        ship_icons_rgba,
        plane_icons_rgba,
        consumable_icons_rgba,
        death_cause_icons,
        powerup_icons,
    );
    let mut encoder = VideoEncoder::new(output_path, None, game_duration);

    // Parse all packets, advancing the encoder at each clock tick
    let mut remaining = &replay_file.packet_data[..];
    let mut prev_clock = GameClock(0.0);

    while !remaining.is_empty() {
        match parser.parse_packet(remaining) {
            Ok((rest, packet)) => {
                if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                    renderer.populate_players(&controller);
                    renderer.update_squadron_info(&controller);
                    renderer.update_ship_abilities(&controller);
                    encoder.advance_clock(prev_clock, &controller, &mut renderer, &mut target);
                }
                prev_clock = packet.clock;
                controller.process(&packet);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    controller.finish();
    renderer.populate_players(&controller);
    renderer.update_squadron_info(&controller);
    renderer.update_ship_abilities(&controller);
    encoder.finish(&controller, &mut renderer, &mut target).map_err(|e| report!("{e}"))?;

    Ok(())
}

// ─── DrawCommand → epaint conversion ─────────────────────────────────────────

fn color_from_rgb(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn color_from_rgba(rgb: [u8; 3], alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], (alpha * 255.0) as u8)
}

/// Build a rotated textured quad mesh for a ship/plane icon.
fn make_rotated_icon_mesh(texture_id: egui::TextureId, center: Pos2, icon_size: f32, yaw: f32, tint: Color32) -> Shape {
    let half = icon_size / 2.0;
    // ImageTarget uses inverse rotation (dest→src) with:
    //   cos_r = sin(yaw), sin_r = cos(yaw)
    //   src_x =  dx*cos_r + dy*sin_r
    //   src_y = -dx*sin_r + dy*cos_r
    // For forward vertex rotation we need the transpose (negate sin terms):
    //   dst_x = dx*cos_r - dy*sin_r
    //   dst_y = dx*sin_r + dy*cos_r
    let cos_r = yaw.sin();
    let sin_r = yaw.cos();

    let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
    let uvs = [egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)];

    let mut mesh = egui::Mesh::with_texture(texture_id);
    for (&(dx, dy), &uv) in corners.iter().zip(uvs.iter()) {
        let rx = dx * cos_r - dy * sin_r + center.x;
        let ry = dx * sin_r + dy * cos_r + center.y;
        mesh.vertices.push(egui::epaint::Vertex { pos: egui::pos2(rx, ry), uv, color: tint });
    }
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    Shape::Mesh(mesh.into())
}

/// Build an unrotated textured quad mesh for a plane icon.
fn make_icon_mesh(texture_id: egui::TextureId, center: Pos2, w: f32, h: f32) -> Shape {
    let half_w = w / 2.0;
    let half_h = h / 2.0;
    let rect = Rect::from_min_max(
        Pos2::new(center.x - half_w, center.y - half_h),
        Pos2::new(center.x + half_w, center.y + half_h),
    );
    let mut mesh = egui::Mesh::with_texture(texture_id);
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
    Shape::Mesh(mesh.into())
}

/// Draw player name and/or ship name labels centered above an icon.
/// `scale` controls font and offset sizing (1.0 at default 768px canvas).
/// `armament_color` is applied to ship_name first if shown, otherwise player_name.
fn draw_ship_labels(
    ctx: &egui::Context,
    center: Pos2,
    scale: f32,
    player_name: Option<&str>,
    ship_name: Option<&str>,
    armament_color: Option<Color32>,
    shapes: &mut Vec<Shape>,
) {
    let label_font = FontId::proportional(10.0 * scale);
    let line_height = 12.0 * scale;
    let label_color = Color32::WHITE;
    let shadow_color = Color32::from_rgba_unmultiplied(0, 0, 0, 180);
    let shadow_offset = (1.0 * scale).min(2.0);

    let line_count = player_name.is_some() as i32 + ship_name.is_some() as i32;
    if line_count == 0 {
        return;
    }

    // Armament color goes on ship_name if shown, else on player_name
    let (pn_color, sn_color) = if ship_name.is_some() {
        (label_color, armament_color.unwrap_or(label_color))
    } else {
        (armament_color.unwrap_or(label_color), label_color)
    };

    // Position lines above the icon
    let base_y = center.y - 14.0 * scale - line_count as f32 * line_height;
    let mut cur_y = base_y;

    if let Some(name) = player_name {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), pn_color));
        let text_w = galley.size().x;
        let tx = center.x - text_w / 2.0;
        let shadow_galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx + shadow_offset, cur_y + shadow_offset), shadow_galley, shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx, cur_y), galley, pn_color));
        cur_y += line_height;
    }

    if let Some(name) = ship_name {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), sn_color));
        let text_w = galley.size().x;
        let tx = center.x - text_w / 2.0;
        let shadow_galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx + shadow_offset, cur_y + shadow_offset), shadow_galley, shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx, cur_y), galley, sn_color));
    }
}

/// Check whether a DrawCommand should be drawn given the current RenderOptions.
/// This runs on the UI thread so option changes are instant (no cross-thread round-trip).
fn should_draw_command(cmd: &DrawCommand, opts: &RenderOptions, show_dead_ships: bool) -> bool {
    match cmd {
        DrawCommand::ShotTracer { .. } => opts.show_tracers,
        DrawCommand::Torpedo { .. } => opts.show_torpedoes,
        DrawCommand::Smoke { .. } => opts.show_smoke,
        DrawCommand::Ship { .. } => true, // ships always drawn; name visibility handled below
        DrawCommand::HealthBar { .. } => opts.show_hp_bars,
        DrawCommand::DeadShip { .. } => show_dead_ships,
        DrawCommand::Plane { .. } => opts.show_planes,
        DrawCommand::ScoreBar { .. } => opts.show_score,
        DrawCommand::Timer { .. } => opts.show_timer,
        DrawCommand::PreBattleCountdown { .. } => opts.show_timer,
        DrawCommand::KillFeed { .. } => opts.show_kill_feed,
        DrawCommand::CapturePoint { .. } => opts.show_capture_points,
        DrawCommand::Building { .. } => opts.show_buildings,
        DrawCommand::TurretDirection { .. } => opts.show_turret_direction,
        DrawCommand::ConsumableRadius { .. } => opts.show_consumables,
        DrawCommand::PatrolRadius { .. } => opts.show_planes,
        DrawCommand::ConsumableIcons { .. } => opts.show_consumables,
        DrawCommand::PositionTrail { .. } => opts.show_trails || opts.show_speed_trails,
        DrawCommand::ShipConfigCircle { .. } => opts.show_ship_config,
        DrawCommand::BuffZone { .. } => opts.show_capture_points,
        DrawCommand::TeamBuffs { .. } => opts.show_buffs,
        DrawCommand::BattleResultOverlay { .. } => opts.show_battle_result,
        DrawCommand::ChatOverlay { .. } => opts.show_chat,
        DrawCommand::TeamAdvantage { .. } => opts.show_advantage,
    }
}

/// Distance from a point to the nearest part of an annotation (in minimap logical coords).
/// Returns 0 if the point is inside the shape.
fn annotation_distance(ann: &Annotation, point: Vec2) -> f32 {
    match ann {
        Annotation::Ship { pos, .. } => (*pos - point).length(),
        Annotation::FreehandStroke { points, .. } => {
            points.windows(2).map(|seg| point_to_segment_dist(point, seg[0], seg[1])).fold(f32::MAX, f32::min)
        }
        Annotation::Line { start, end, .. } => point_to_segment_dist(point, *start, *end),
        Annotation::Circle { center, radius, .. } => {
            let dist_from_center = (point - *center).length();
            if dist_from_center <= *radius {
                0.0 // inside the circle
            } else {
                dist_from_center - *radius
            }
        }
        Annotation::Rectangle { center, half_size, rotation, .. } => {
            // Transform point into the rectangle's local coordinate space
            let dp = point - *center;
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let local = Vec2::new(dp.x * cos_r + dp.y * sin_r, -dp.x * sin_r + dp.y * cos_r);
            let dx = (local.x.abs() - half_size.x).max(0.0);
            let dy = (local.y.abs() - half_size.y).max(0.0);
            (dx * dx + dy * dy).sqrt()
        }
        Annotation::Triangle { center, radius, rotation, .. } => {
            // Check if inside: use distance from center vs circumradius as approximation
            let dist = (point - *center).length();
            // Inradius of equilateral triangle = radius / 2
            let inradius = *radius * 0.5;
            if dist <= inradius {
                0.0
            } else {
                // Distance to nearest edge
                let verts: Vec<Vec2> = (0..3)
                    .map(|i| {
                        let angle = *rotation + i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                        *center + Vec2::new(radius * angle.cos(), radius * angle.sin())
                    })
                    .collect();
                let mut min_dist = f32::MAX;
                for i in 0..3 {
                    let d = point_to_segment_dist(point, verts[i], verts[(i + 1) % 3]);
                    if d < min_dist {
                        min_dist = d;
                    }
                }
                min_dist
            }
        }
    }
}

/// Distance from a point to a line segment.
fn point_to_segment_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_sq();
    if len_sq < 0.001 {
        return ap.length();
    }
    let t = (ap.x * ab.x + ap.y * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let closest = a + ab * t;
    (p - closest).length()
}

/// Short display name for ship species (used in context menu buttons).
fn ship_short_name(species: &str) -> &str {
    match species {
        "Destroyer" => "DD",
        "Cruiser" => "CA",
        "Battleship" => "BB",
        "AirCarrier" => "CV",
        "Submarine" => "SS",
        _ => species,
    }
}

/// Helper to convert a minimap Vec2 position to screen Pos2 via MapTransform.
fn minimap_vec2_to_screen(pos: Vec2, transform: &MapTransform) -> Pos2 {
    transform.minimap_to_screen(&MinimapPos { x: pos.x as i32, y: pos.y as i32 })
}

/// Render a single annotation onto the map painter.
fn render_annotation(ann: &Annotation, transform: &MapTransform, textures: &RendererTextures, painter: &egui::Painter) {
    match ann {
        Annotation::Ship { pos, yaw, species, friendly } => {
            let screen_pos = minimap_vec2_to_screen(*pos, transform);
            let icon_size = transform.scale_distance(ICON_SIZE);
            let tint = if *friendly { FRIENDLY_COLOR } else { ENEMY_COLOR };
            // Draw outline ring to distinguish from replay ships
            let ring_radius = icon_size * 0.6;
            painter.add(Shape::circle_stroke(screen_pos, ring_radius, Stroke::new(1.5, tint)));
            if let Some(tex) = textures.ship_icons.get(species.as_str()) {
                painter.add(make_rotated_icon_mesh(tex.id(), screen_pos, icon_size, *yaw, tint));
            } else {
                painter.add(Shape::circle_filled(screen_pos, icon_size / 2.0, tint));
            }
        }
        Annotation::FreehandStroke { points, color, width } => {
            let stroke_w = transform.scale_stroke(*width);
            for pair in points.windows(2) {
                let a = minimap_vec2_to_screen(pair[0], transform);
                let b = minimap_vec2_to_screen(pair[1], transform);
                painter.add(Shape::LineSegment { points: [a, b], stroke: Stroke::new(stroke_w, *color) });
            }
        }
        Annotation::Line { start, end, color, width } => {
            let a = minimap_vec2_to_screen(*start, transform);
            let b = minimap_vec2_to_screen(*end, transform);
            painter.add(Shape::LineSegment {
                points: [a, b],
                stroke: Stroke::new(transform.scale_stroke(*width), *color),
            });
        }
        Annotation::Circle { center, radius, color, width, filled } => {
            let c = minimap_vec2_to_screen(*center, transform);
            let r = transform.scale_distance(*radius);
            if *filled {
                painter.add(Shape::circle_filled(c, r, *color));
            } else {
                painter.add(Shape::circle_stroke(c, r, Stroke::new(transform.scale_stroke(*width), *color)));
            }
        }
        Annotation::Rectangle { center, half_size, rotation, color, width, filled } => {
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let corners_local = [
                Vec2::new(-half_size.x, -half_size.y),
                Vec2::new(half_size.x, -half_size.y),
                Vec2::new(half_size.x, half_size.y),
                Vec2::new(-half_size.x, half_size.y),
            ];
            let screen_corners: Vec<Pos2> = corners_local
                .iter()
                .map(|c| {
                    let rotated = Vec2::new(c.x * cos_r - c.y * sin_r, c.x * sin_r + c.y * cos_r);
                    minimap_vec2_to_screen(*center + rotated, transform)
                })
                .collect();
            if *filled {
                painter.add(Shape::convex_polygon(screen_corners, *color, Stroke::NONE));
            } else {
                let stroke = Stroke::new(transform.scale_stroke(*width), *color);
                painter.add(egui::epaint::PathShape::closed_line(screen_corners, stroke));
            }
        }
        Annotation::Triangle { center, radius, rotation, color, width, filled } => {
            let screen_verts: Vec<Pos2> = (0..3)
                .map(|i| {
                    let angle = *rotation + i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                    let dx = radius * angle.cos();
                    let dy = radius * angle.sin();
                    minimap_vec2_to_screen(*center + Vec2::new(dx, dy), transform)
                })
                .collect();
            if *filled {
                painter.add(Shape::convex_polygon(screen_verts, *color, Stroke::NONE));
            } else {
                let stroke = Stroke::new(transform.scale_stroke(*width), *color);
                painter.add(egui::epaint::PathShape::closed_line(screen_verts, stroke));
            }
        }
    }
}

/// Render a preview of the active tool at the cursor position.
fn render_tool_preview(
    tool: &PaintTool,
    minimap_pos: Vec2,
    color: Color32,
    stroke_width: f32,
    transform: &MapTransform,
    textures: &RendererTextures,
    painter: &egui::Painter,
) {
    let ghost_alpha = 128u8;
    match tool {
        PaintTool::PlacingShip { species, friendly, yaw } => {
            let screen_pos = minimap_vec2_to_screen(minimap_pos, transform);
            let icon_size = transform.scale_distance(ICON_SIZE);
            let base = if *friendly { FRIENDLY_COLOR } else { ENEMY_COLOR };
            let tint = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), ghost_alpha);
            if let Some(tex) = textures.ship_icons.get(species.as_str()) {
                painter.add(make_rotated_icon_mesh(tex.id(), screen_pos, icon_size, *yaw, tint));
            } else {
                painter.add(Shape::circle_filled(screen_pos, icon_size / 2.0, tint));
            }
        }
        PaintTool::Freehand { current_stroke } => {
            if let Some(points) = current_stroke {
                let sw = transform.scale_stroke(stroke_width);
                for pair in points.windows(2) {
                    let a = minimap_vec2_to_screen(pair[0], transform);
                    let b = minimap_vec2_to_screen(pair[1], transform);
                    painter.add(Shape::LineSegment { points: [a, b], stroke: Stroke::new(sw, color) });
                }
            }
            // Draw stroke-width circle at cursor
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.0, color)));
        }
        PaintTool::Eraser => {
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = transform.scale_distance(15.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.5, Color32::from_rgb(255, 100, 100))));
        }
        PaintTool::DrawingLine { start, .. } => {
            // Stroke-width circle at cursor
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.0, color)));
            if let Some(s) = start {
                let a = minimap_vec2_to_screen(*s, transform);
                let b = minimap_vec2_to_screen(minimap_pos, transform);
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                painter.add(Shape::LineSegment {
                    points: [a, b],
                    stroke: Stroke::new(transform.scale_stroke(stroke_width), ghost_color),
                });
            }
        }
        PaintTool::DrawingCircle { center: origin, filled, .. } => {
            // Stroke-width circle at cursor
            let cursor_screen = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(cursor_screen, r, Stroke::new(1.0, color)));
            if let Some(org) = origin {
                // Circle from drag origin to cursor (origin and cursor are opposite edges)
                let mid = (*org + minimap_pos) / 2.0;
                let radius = (minimap_pos - *org).length() / 2.0;
                let c = minimap_vec2_to_screen(mid, transform);
                let r = transform.scale_distance(radius);
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                if *filled {
                    painter.add(Shape::circle_filled(c, r, ghost_color));
                } else {
                    painter.add(Shape::circle_stroke(
                        c,
                        r,
                        Stroke::new(transform.scale_stroke(stroke_width), ghost_color),
                    ));
                }
            }
        }
        PaintTool::DrawingRect { center: origin, filled, .. } => {
            // Stroke-width circle at cursor
            let cursor_screen = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(cursor_screen, r, Stroke::new(1.0, color)));
            if let Some(org) = origin {
                // Rect from drag origin corner to cursor corner
                let min = Vec2::new(org.x.min(minimap_pos.x), org.y.min(minimap_pos.y));
                let max = Vec2::new(org.x.max(minimap_pos.x), org.y.max(minimap_pos.y));
                let corners: Vec<Pos2> = [
                    Vec2::new(min.x, min.y),
                    Vec2::new(max.x, min.y),
                    Vec2::new(max.x, max.y),
                    Vec2::new(min.x, max.y),
                ]
                .iter()
                .map(|p| minimap_vec2_to_screen(*p, transform))
                .collect();
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                if *filled {
                    painter.add(Shape::convex_polygon(corners, ghost_color, Stroke::NONE));
                } else {
                    let stroke = Stroke::new(transform.scale_stroke(stroke_width), ghost_color);
                    painter.add(egui::epaint::PathShape::closed_line(corners, stroke));
                }
            }
        }
        PaintTool::DrawingTriangle { center, filled, .. } => {
            // Stroke-width circle at cursor
            let cursor_screen = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(cursor_screen, r, Stroke::new(1.0, color)));
            if let Some(ctr) = center {
                let radius = (minimap_pos - *ctr).length();
                let verts: Vec<Pos2> = (0..3)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                        let dx = radius * angle.cos();
                        let dy = radius * angle.sin();
                        minimap_vec2_to_screen(*ctr + Vec2::new(dx, dy), transform)
                    })
                    .collect();
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                if *filled {
                    painter.add(Shape::convex_polygon(verts, ghost_color, Stroke::NONE));
                } else {
                    let stroke = Stroke::new(transform.scale_stroke(stroke_width), ghost_color);
                    painter.add(egui::epaint::PathShape::closed_line(verts, stroke));
                }
            }
        }
        PaintTool::None => {}
    }
}

/// Render a selection highlight around an annotation (corner brackets + rotation handle).
fn render_selection_highlight(ann: &Annotation, transform: &MapTransform, painter: &egui::Painter) {
    let highlight_stroke = Stroke::new(1.5, Color32::from_rgb(255, 255, 100));
    let margin = 8.0; // extra pixels around the bounding box

    let screen_rect = annotation_screen_bounds(ann, transform);
    let expanded = screen_rect.expand(margin);

    // Draw corner brackets instead of full rectangle for a cleaner look
    let corners = [expanded.left_top(), expanded.right_top(), expanded.right_bottom(), expanded.left_bottom()];
    let bracket_len = 8.0f32.min(expanded.width() / 3.0).min(expanded.height() / 3.0);
    for i in 0..4 {
        let c = corners[i];
        let next = corners[(i + 1) % 4];
        let prev = corners[(i + 3) % 4];
        let to_next = (next - c).normalized() * bracket_len;
        let to_prev = (prev - c).normalized() * bracket_len;
        painter.add(Shape::LineSegment { points: [c, c + to_next], stroke: highlight_stroke });
        painter.add(Shape::LineSegment { points: [c, c + to_prev], stroke: highlight_stroke });
    }

    // Draw rotation handle for rotatable annotations
    let has_rotation =
        matches!(ann, Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. });
    if has_rotation {
        let (handle_pos, anchor) = rotation_handle_pos(ann, transform);
        let thin_stroke = Stroke::new(1.0, Color32::from_rgb(255, 255, 100));
        painter.add(Shape::LineSegment { points: [anchor, handle_pos], stroke: thin_stroke });
        painter.add(Shape::circle_filled(handle_pos, ROTATION_HANDLE_RADIUS, Color32::from_rgb(255, 255, 100)));
    }
}

const ROTATION_HANDLE_RADIUS: f32 = 5.0;
const ROTATION_HANDLE_DISTANCE: f32 = 25.0;

/// Get the screen position of the rotation handle and its anchor point on the bounding box.
fn rotation_handle_pos(ann: &Annotation, transform: &MapTransform) -> (Pos2, Pos2) {
    let bounds = annotation_screen_bounds(ann, transform);
    let anchor = Pos2::new(bounds.center().x, bounds.top());
    let handle = Pos2::new(anchor.x, anchor.y - ROTATION_HANDLE_DISTANCE);
    (handle, anchor)
}

/// Compute the screen-space bounding rect for an annotation.
fn annotation_screen_bounds(ann: &Annotation, transform: &MapTransform) -> Rect {
    match ann {
        Annotation::Ship { pos, .. } => {
            let c = minimap_vec2_to_screen(*pos, transform);
            let half = transform.scale_distance(ICON_SIZE) / 2.0;
            Rect::from_center_size(c, egui::vec2(half * 2.0, half * 2.0))
        }
        Annotation::FreehandStroke { points, .. } => {
            let screen_pts: Vec<Pos2> = points.iter().map(|p| minimap_vec2_to_screen(*p, transform)).collect();
            let mut rect = Rect::from_min_max(screen_pts[0], screen_pts[0]);
            for p in &screen_pts[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
        Annotation::Line { start, end, .. } => {
            let a = minimap_vec2_to_screen(*start, transform);
            let b = minimap_vec2_to_screen(*end, transform);
            Rect::from_two_pos(a, b)
        }
        Annotation::Circle { center, radius, .. } => {
            let c = minimap_vec2_to_screen(*center, transform);
            let r = transform.scale_distance(*radius);
            Rect::from_center_size(c, egui::vec2(r * 2.0, r * 2.0))
        }
        Annotation::Rectangle { center, half_size, rotation, .. } => {
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let corners_local = [
                Vec2::new(-half_size.x, -half_size.y),
                Vec2::new(half_size.x, -half_size.y),
                Vec2::new(half_size.x, half_size.y),
                Vec2::new(-half_size.x, half_size.y),
            ];
            let screen_corners: Vec<Pos2> = corners_local
                .iter()
                .map(|c| {
                    let rotated = Vec2::new(c.x * cos_r - c.y * sin_r, c.x * sin_r + c.y * cos_r);
                    minimap_vec2_to_screen(*center + rotated, transform)
                })
                .collect();
            let mut rect = Rect::from_min_max(screen_corners[0], screen_corners[0]);
            for p in &screen_corners[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
        Annotation::Triangle { center, radius, rotation, .. } => {
            let screen_verts: Vec<Pos2> = (0..3)
                .map(|i| {
                    let angle = *rotation + i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                    let dx = radius * angle.cos();
                    let dy = radius * angle.sin();
                    minimap_vec2_to_screen(*center + Vec2::new(dx, dy), transform)
                })
                .collect();
            let mut rect = Rect::from_min_max(screen_verts[0], screen_verts[0]);
            for p in &screen_verts[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
    }
}

/// Convert a single DrawCommand into epaint shapes.
/// Uses `MapTransform` for all coordinate mapping. `opts` filters name labels.
fn draw_command_to_shapes(
    cmd: &DrawCommand,
    transform: &MapTransform,
    textures: &RendererTextures,
    ctx: &egui::Context,
    opts: &RenderOptions,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let ws = transform.window_scale;

    match cmd {
        DrawCommand::ShotTracer { from, to, color } => {
            let p1 = transform.minimap_to_screen(from);
            let p2 = transform.minimap_to_screen(to);
            shapes.push(Shape::LineSegment {
                points: [p1, p2],
                stroke: Stroke::new(transform.scale_stroke(1.0), color_from_rgb(*color)),
            });
        }

        DrawCommand::Torpedo { pos, color } => {
            let center = transform.minimap_to_screen(pos);
            shapes.push(Shape::circle_filled(center, transform.scale_distance(2.0), color_from_rgb(*color)));
        }

        DrawCommand::Smoke { pos, radius, color, alpha } => {
            let center = transform.minimap_to_screen(pos);
            shapes.push(Shape::circle_filled(
                center,
                transform.scale_distance(*radius as f32),
                color_from_rgba(*color, *alpha),
            ));
        }

        DrawCommand::Ship {
            pos,
            yaw,
            species,
            color,
            visibility,
            opacity,
            is_self,
            player_name,
            ship_name,
            is_detected_teammate,
            name_color,
        } => {
            let center = transform.minimap_to_screen(pos);
            let icon_size = transform.scale_distance(ICON_SIZE);

            if let Some(sp) = species {
                let variant_key = match (*visibility, *is_self) {
                    (wows_minimap_renderer::ShipVisibility::Visible, true) => format!("{}_self", sp),
                    (wows_minimap_renderer::ShipVisibility::Visible, false) => sp.clone(),
                    (wows_minimap_renderer::ShipVisibility::MinimapOnly, _) => {
                        format!("{}_invisible", sp)
                    }
                    (wows_minimap_renderer::ShipVisibility::Undetected, _) => {
                        format!("{}_invisible", sp)
                    }
                };

                // Gold icon-shaped outline for detected teammates (drawn before icon)
                if *is_detected_teammate {
                    let outline_tex =
                        textures.ship_icon_outlines.get(&variant_key).or_else(|| textures.ship_icon_outlines.get(sp));
                    if let Some(otex) = outline_tex {
                        shapes.push(make_rotated_icon_mesh(otex.id(), center, icon_size, *yaw, Color32::WHITE));
                    }
                }

                let texture = textures.ship_icons.get(&variant_key).or_else(|| textures.ship_icons.get(sp));

                if let Some(tex) = texture {
                    let tint = if let Some(c) = color {
                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (*opacity * 255.0) as u8)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, (*opacity * 255.0) as u8)
                    };
                    shapes.push(make_rotated_icon_mesh(tex.id(), center, icon_size, *yaw, tint));
                } else {
                    let c = color.map(|c| color_from_rgba(c, *opacity)).unwrap_or(Color32::from_rgba_unmultiplied(
                        128,
                        128,
                        128,
                        (*opacity * 255.0) as u8,
                    ));
                    shapes.push(Shape::circle_filled(center, transform.scale_distance(5.0), c));
                }
            }
            let pname = if opts.show_player_names { player_name.as_deref() } else { None };
            let sname = if opts.show_ship_names { ship_name.as_deref() } else { None };
            let pn_color =
                if opts.show_armament { name_color.map(|c| Color32::from_rgb(c[0], c[1], c[2])) } else { None };
            draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, pn_color, &mut shapes);
        }

        DrawCommand::HealthBar { pos, fraction, fill_color, background_color, background_alpha } => {
            let bar_w = transform.scale_distance(20.0);
            let bar_h = transform.scale_distance(3.0);
            let center = transform.minimap_to_screen(pos);
            let bar_x = center.x - bar_w / 2.0;
            let bar_y = center.y + transform.scale_distance(10.0);

            let bg_rect = Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(bar_w, bar_h));
            shapes.push(Shape::rect_filled(
                bg_rect,
                CornerRadius::ZERO,
                color_from_rgba(*background_color, *background_alpha),
            ));

            let fill_w = fraction.clamp(0.0, 1.0) * bar_w;
            if fill_w > 0.0 {
                let fill_rect = Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(fill_w, bar_h));
                shapes.push(Shape::rect_filled(fill_rect, CornerRadius::ZERO, color_from_rgb(*fill_color)));
            }
        }

        DrawCommand::DeadShip { pos, yaw, species, color, is_self, player_name, ship_name } => {
            let center = transform.minimap_to_screen(pos);
            let icon_size = transform.scale_distance(ICON_SIZE);
            if let Some(sp) = species {
                let variant_key = if *is_self { format!("{}_dead_self", sp) } else { format!("{}_dead", sp) };

                let texture = textures.ship_icons.get(&variant_key).or_else(|| textures.ship_icons.get(sp));

                if let Some(tex) = texture {
                    let tint = if let Some(c) = color { Color32::from_rgb(c[0], c[1], c[2]) } else { Color32::WHITE };
                    shapes.push(make_rotated_icon_mesh(tex.id(), center, icon_size, *yaw, tint));
                } else {
                    let s = transform.scale_distance(6.0);
                    let stroke = Stroke::new(transform.scale_stroke(2.0), Color32::RED);
                    shapes.push(Shape::LineSegment {
                        points: [Pos2::new(center.x - s, center.y - s), Pos2::new(center.x + s, center.y + s)],
                        stroke,
                    });
                    shapes.push(Shape::LineSegment {
                        points: [Pos2::new(center.x + s, center.y - s), Pos2::new(center.x - s, center.y + s)],
                        stroke,
                    });
                }
            }
            if opts.show_dead_ship_names {
                let pname = if opts.show_player_names { player_name.as_deref() } else { None };
                let sname = if opts.show_ship_names { ship_name.as_deref() } else { None };
                draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, None, &mut shapes);
            }
        }

        DrawCommand::Plane { pos, icon_key } => {
            let center = transform.minimap_to_screen(pos);
            if let Some(tex) = textures.plane_icons.get(icon_key) {
                let size = tex.size();
                let w = transform.scale_distance(size[0] as f32);
                let h = transform.scale_distance(size[1] as f32);
                shapes.push(make_icon_mesh(tex.id(), center, w, h));
            } else {
                shapes.push(Shape::circle_filled(center, transform.scale_distance(3.0), Color32::YELLOW));
            }
        }

        DrawCommand::ScoreBar {
            team0,
            team1,
            team0_color,
            team1_color,
            max_score,
            team0_timer,
            team1_timer,
            advantage_label,
            advantage_team,
        } => {
            let canvas_w = transform.screen_canvas_width();
            let bar_height = 20.0 * ws;
            let max_score = *max_score as f32;
            let half = canvas_w / 2.0;
            let center_gap = 2.0 * ws;

            let bar_origin = transform.hud_pos(0.0, 0.0);

            // Dark background
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(bar_origin, Vec2::new(canvas_w, bar_height)),
                CornerRadius::ZERO,
                Color32::from_rgba_unmultiplied(30, 30, 30, 204),
            ));

            // Team 0 progress: grows from left edge toward center
            let t0_frac = (*team0 as f32 / max_score).clamp(0.0, 1.0);
            let t0_width = t0_frac * (half - center_gap);
            if t0_width > 0.0 {
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(bar_origin, Vec2::new(t0_width, bar_height)),
                    CornerRadius::ZERO,
                    color_from_rgb(*team0_color),
                ));
            }

            // Team 1 progress: grows from right edge toward center
            let t1_frac = (*team1 as f32 / max_score).clamp(0.0, 1.0);
            let t1_width = t1_frac * (half - center_gap);
            if t1_width > 0.0 {
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(
                        Pos2::new(bar_origin.x + canvas_w - t1_width, bar_origin.y),
                        Vec2::new(t1_width, bar_height),
                    ),
                    CornerRadius::ZERO,
                    color_from_rgb(*team1_color),
                ));
            }

            let score_font = FontId::proportional(14.0 * ws);
            let timer_font = FontId::proportional(12.0 * ws);
            let adv_font = FontId::proportional(11.0 * ws);
            let t0_text = format!("{}", team0);
            let t1_text = format!("{}", team1);
            let timer_color = Color32::from_rgb(200, 200, 200);
            let pill_color = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
            let pill_pad_x = 4.0 * ws;
            let pill_pad_y = 1.0 * ws;
            let pill_rounding = CornerRadius::same((3.0 * ws) as u8);

            // ── Measure all team 0 elements ──
            let t0_score_g = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text.clone(), score_font.clone(), Color32::WHITE));
            let t0_score_w = t0_score_g.size().x;
            let t0_score_h = t0_score_g.size().y;
            drop(t0_score_g);

            let t0_timer_w = team0_timer.as_ref().map(|t| {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(t.clone(), timer_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                w
            });

            let t0_adv_w = if *advantage_team == 0 && !advantage_label.is_empty() {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                Some(w)
            } else {
                None
            };

            // Total width for team 0 pill
            let mut t0_total_w = t0_score_w;
            if let Some(tw) = t0_timer_w {
                t0_total_w += 4.0 * ws + tw;
            }
            if let Some(aw) = t0_adv_w {
                t0_total_w += 6.0 * ws + aw;
            }

            // Draw team 0 pill + text
            let t0_pill_x = bar_origin.x + 8.0 * ws - pill_pad_x;
            let t0_pill_y = bar_origin.y + 2.0 * ws - pill_pad_y;
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(
                    Pos2::new(t0_pill_x, t0_pill_y),
                    Vec2::new(t0_total_w + pill_pad_x * 2.0, t0_score_h + pill_pad_y * 2.0),
                ),
                pill_rounding,
                pill_color,
            ));

            let mut t0_cursor = bar_origin.x + 8.0 * ws;
            let t0_score_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text, score_font.clone(), Color32::WHITE));
            shapes.push(Shape::galley(Pos2::new(t0_cursor, bar_origin.y + 2.0 * ws), t0_score_galley, Color32::WHITE));
            t0_cursor += t0_score_w;

            if let Some(timer) = team0_timer {
                t0_cursor += 4.0 * ws;
                let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font.clone(), timer_color));
                let tw = tg.size().x;
                shapes.push(Shape::galley(Pos2::new(t0_cursor, bar_origin.y + 3.0 * ws), tg, timer_color));
                t0_cursor += tw;
            }

            let _t0_end_x = t0_cursor;

            if let Some(_) = t0_adv_w {
                t0_cursor += 6.0 * ws;
                let ag = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                shapes.push(Shape::galley(Pos2::new(t0_cursor, bar_origin.y + 4.0 * ws), ag, Color32::WHITE));
            }

            // ── Measure all team 1 elements ──
            let t1_score_g = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text.clone(), score_font.clone(), Color32::WHITE));
            let t1_score_w = t1_score_g.size().x;
            let t1_score_h = t1_score_g.size().y;
            drop(t1_score_g);

            let t1_timer_w = team1_timer.as_ref().map(|t| {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(t.clone(), timer_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                w
            });

            let t1_adv_w = if *advantage_team == 1 && !advantage_label.is_empty() {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                Some(w)
            } else {
                None
            };

            // Total width for team 1 pill
            let mut t1_total_w = t1_score_w;
            if let Some(tw) = t1_timer_w {
                t1_total_w += 4.0 * ws + tw;
            }
            if let Some(aw) = t1_adv_w {
                t1_total_w += 6.0 * ws + aw;
            }

            // Draw team 1 pill + text (right-aligned)
            let t1_pill_x = bar_origin.x + canvas_w - 8.0 * ws - t1_total_w - pill_pad_x;
            let t1_pill_y = bar_origin.y + 2.0 * ws - pill_pad_y;
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(
                    Pos2::new(t1_pill_x, t1_pill_y),
                    Vec2::new(t1_total_w + pill_pad_x * 2.0, t1_score_h + pill_pad_y * 2.0),
                ),
                pill_rounding,
                pill_color,
            ));

            // Lay out team 1 elements right-to-left
            let mut t1_cursor = bar_origin.x + canvas_w - 8.0 * ws;

            // Score (rightmost)
            t1_cursor -= t1_score_w;
            let t1_score_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text, score_font, Color32::WHITE));
            shapes.push(Shape::galley(Pos2::new(t1_cursor, bar_origin.y + 2.0 * ws), t1_score_galley, Color32::WHITE));
            let _t1_score_x = t1_cursor;

            // Timer (left of score)
            if let Some(timer) = team1_timer {
                let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font, timer_color));
                let tw = tg.size().x;
                t1_cursor -= 4.0 * ws + tw;
                shapes.push(Shape::galley(Pos2::new(t1_cursor, bar_origin.y + 3.0 * ws), tg, timer_color));
            }

            let _t1_start_x = t1_cursor;

            // Advantage (leftmost, if team 1)
            if let Some(aw) = t1_adv_w {
                t1_cursor -= 6.0 * ws + aw;
                let ag = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font, Color32::WHITE));
                shapes.push(Shape::galley(Pos2::new(t1_cursor, bar_origin.y + 4.0 * ws), ag, Color32::WHITE));
            }
        }

        DrawCommand::Timer { time_remaining, elapsed } => {
            let canvas_w = transform.screen_canvas_width();
            let main_font = FontId::proportional(16.0 * ws);
            let pill_color = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
            let pill_pad_x = 4.0 * ws;
            let pill_pad_y = 1.0 * ws;
            let pill_rounding = CornerRadius::same((3.0 * ws) as u8);

            if let Some(remaining) = time_remaining {
                // Measure both lines to size a single pill
                let r = (*remaining).max(0) as u32;
                let remaining_text = format!("{:02}:{:02}", r / 60, r % 60);
                let small_font = FontId::proportional(11.0 * ws);
                let e = elapsed.max(0.0) as u32;
                let elapsed_text = format!("+{:02}:{:02}", e / 60, e % 60);
                let gray = Color32::from_rgb(180, 180, 180);

                let rg = ctx.fonts_mut(|f| f.layout_no_wrap(remaining_text, main_font, Color32::WHITE));
                let r_w = rg.size().x;
                let r_h = rg.size().y;
                let eg = ctx.fonts_mut(|f| f.layout_no_wrap(elapsed_text, small_font, gray));
                let e_w = eg.size().x;
                let e_h = eg.size().y;

                let pill_w = r_w.max(e_w);
                let pill_h = r_h + e_h;
                let pos = transform.hud_pos(0.0, 2.0);
                let pill_x = pos.x + canvas_w / 2.0 - pill_w / 2.0 - pill_pad_x;
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(
                        Pos2::new(pill_x, pos.y - pill_pad_y),
                        Vec2::new(pill_w + pill_pad_x * 2.0, pill_h + pill_pad_y * 2.0),
                    ),
                    pill_rounding,
                    pill_color,
                ));

                let r_x = pos.x + canvas_w / 2.0 - r_w / 2.0;
                shapes.push(Shape::galley(Pos2::new(r_x, pos.y), rg, Color32::WHITE));

                let e_x = pos.x + canvas_w / 2.0 - e_w / 2.0;
                shapes.push(Shape::galley(Pos2::new(e_x, pos.y + r_h), eg, gray));
            } else {
                // Fallback: just show elapsed time centered
                let e = elapsed.max(0.0) as u32;
                let text = format!("{:02}:{:02}", e / 60, e % 60);
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text, main_font, Color32::WHITE));
                let text_w = galley.size().x;
                let text_h = galley.size().y;
                let pos = transform.hud_pos(0.0, 2.0);
                let x = pos.x + canvas_w / 2.0 - text_w / 2.0;
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(
                        Pos2::new(x - pill_pad_x, pos.y - pill_pad_y),
                        Vec2::new(text_w + pill_pad_x * 2.0, text_h + pill_pad_y * 2.0),
                    ),
                    pill_rounding,
                    pill_color,
                ));
                shapes.push(Shape::galley(Pos2::new(x, pos.y), galley, Color32::WHITE));
            }
        }

        DrawCommand::PreBattleCountdown { seconds } => {
            // Reuse the BattleResultOverlay rendering with gold color and subtitle
            let overlay = DrawCommand::BattleResultOverlay {
                text: format!("{}", seconds),
                subtitle: Some("BATTLE STARTS IN".to_string()),
                color: [255, 200, 50],
            };
            shapes.extend(draw_command_to_shapes(&overlay, transform, textures, ctx, opts));
        }

        DrawCommand::TeamAdvantage { .. } => {
            // Rendering handled by ScoreBar; this command is kept for tooltip interaction only
        }

        DrawCommand::KillFeed { entries } => {
            use wows_replays::analyzer::decoder::DeathCause;

            let canvas_w = transform.screen_canvas_width();
            let name_font = FontId::proportional(12.0 * ws);
            let line_h = 20.0 * ws;
            let icon_size = ICON_SIZE * ws;
            let cause_icon_size = icon_size;
            let gap = 2.0 * ws;
            let right_margin = 4.0 * ws;
            let start = transform.hud_pos(0.0, 22.0);

            for (i, entry) in entries.iter().take(5).enumerate() {
                let y = start.y + i as f32 * line_h;

                let killer_color = color_from_rgb(entry.killer_color);
                let victim_color = color_from_rgb(entry.victim_color);

                let cause_key = match entry.cause.known() {
                    Some(DeathCause::Artillery | DeathCause::ApShell | DeathCause::HeShell | DeathCause::CsShell) => {
                        "main_caliber"
                    }
                    Some(DeathCause::Secondaries) => "atba",
                    Some(DeathCause::Torpedo | DeathCause::AerialTorpedo) => "torpedo",
                    Some(DeathCause::Fire) => "burning",
                    Some(DeathCause::Flooding) => "flood",
                    Some(DeathCause::DiveBomber) => "bomb",
                    Some(DeathCause::SkipBombs) => "skip",
                    Some(DeathCause::AerialRocket) => "rocket",
                    Some(DeathCause::Detonation) => "detonate",
                    Some(DeathCause::Ramming) => "ram",
                    Some(DeathCause::DepthCharge | DeathCause::AerialDepthCharge) => "depthbomb",
                    Some(DeathCause::Missile) => "missile",
                    _ => "main_caliber",
                };

                // Measure text segments
                let killer_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.killer_name.clone(), name_font.clone(), killer_color));
                let victim_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.victim_name.clone(), name_font.clone(), victim_color));
                let killer_name_w = killer_galley.size().x;
                let victim_name_w = victim_galley.size().x;

                let ship_font = name_font.clone();
                let killer_ship = entry.killer_ship_name.as_deref().unwrap_or("");
                let victim_ship = entry.victim_ship_name.as_deref().unwrap_or("");
                let killer_ship_galley = if !killer_ship.is_empty() {
                    Some(ctx.fonts_mut(|f| f.layout_no_wrap(killer_ship.to_string(), ship_font.clone(), killer_color)))
                } else {
                    None
                };
                let victim_ship_galley = if !victim_ship.is_empty() {
                    Some(ctx.fonts_mut(|f| f.layout_no_wrap(victim_ship.to_string(), ship_font.clone(), victim_color)))
                } else {
                    None
                };
                let killer_ship_w = killer_ship_galley.as_ref().map_or(0.0, |g| g.size().x);
                let victim_ship_w = victim_ship_galley.as_ref().map_or(0.0, |g| g.size().x);

                let has_cause_icon = textures.death_cause_icons.contains_key(cause_key);
                let cause_w = if has_cause_icon { cause_icon_size } else { 0.0 };

                let has_killer_icon =
                    entry.killer_species.as_ref().map_or(false, |sp| textures.ship_icons.contains_key(sp.as_str()));
                let has_victim_icon =
                    entry.victim_species.as_ref().map_or(false, |sp| textures.ship_icons.contains_key(sp.as_str()));

                // Total width: killer_name [gap icon gap] killer_ship gap cause gap victim_name [gap icon gap] victim_ship
                let mut total_w = killer_name_w;
                if has_killer_icon {
                    total_w += gap + icon_size + gap;
                } else if killer_ship_w > 0.0 {
                    total_w += gap;
                }
                if killer_ship_w > 0.0 {
                    total_w += killer_ship_w;
                }
                total_w += gap * 2.0 + cause_w + gap * 2.0;
                total_w += victim_name_w;
                if has_victim_icon {
                    total_w += gap + icon_size + gap;
                } else if victim_ship_w > 0.0 {
                    total_w += gap;
                }
                if victim_ship_w > 0.0 {
                    total_w += victim_ship_w;
                }

                // Semi-transparent background
                let bg_x = start.x + canvas_w - total_w - right_margin * 2.0;
                let bg_rect =
                    Rect::from_min_size(Pos2::new(bg_x, y - 1.0 * ws), Vec2::new(total_w + right_margin * 2.0, line_h));
                shapes.push(Shape::rect_filled(bg_rect, CornerRadius::ZERO, Color32::from_black_alpha(128)));

                let mut x = start.x + canvas_w - total_w - right_margin;
                // Vertically center icons with the text
                let row_rect = killer_galley.rows.first().map(|r| r.rect()).unwrap_or(egui::Rect::ZERO);
                let icon_center_y = y + row_rect.center().y;

                // Killer name
                shapes.push(Shape::galley(Pos2::new(x, y), killer_galley, Color32::TRANSPARENT));
                x += killer_name_w;

                // Killer ship icon (facing left: -90° from north)
                if has_killer_icon {
                    x += gap;
                    let sp = entry.killer_species.as_ref().unwrap();
                    if let Some(tex) = textures.ship_icons.get(sp.as_str()) {
                        let tint =
                            Color32::from_rgb(entry.killer_color[0], entry.killer_color[1], entry.killer_color[2]);
                        shapes.push(make_rotated_icon_mesh(
                            tex.id(),
                            Pos2::new(x + icon_size / 2.0, icon_center_y),
                            icon_size,
                            std::f32::consts::PI,
                            tint,
                        ));
                    }
                    x += icon_size + gap;
                } else if killer_ship_w > 0.0 {
                    x += gap;
                }

                // Killer ship name
                if let Some(galley) = killer_ship_galley {
                    shapes.push(Shape::galley(Pos2::new(x, y), galley, Color32::TRANSPARENT));
                    x += killer_ship_w;
                }

                // Death cause icon
                x += gap * 2.0;
                if let Some(tex) = textures.death_cause_icons.get(cause_key) {
                    let half = cause_icon_size / 2.0;
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_max(
                        Pos2::new(x, icon_center_y - half),
                        Pos2::new(x + cause_icon_size, icon_center_y + half),
                    );
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));
                }
                x += cause_w + gap * 2.0;

                // Victim name
                shapes.push(Shape::galley(Pos2::new(x, y), victim_galley, Color32::TRANSPARENT));
                x += victim_name_w;

                // Victim ship icon (facing right: +90° from north)
                if has_victim_icon {
                    x += gap;
                    let sp = entry.victim_species.as_ref().unwrap();
                    if let Some(tex) = textures.ship_icons.get(sp.as_str()) {
                        let tint =
                            Color32::from_rgb(entry.victim_color[0], entry.victim_color[1], entry.victim_color[2]);
                        shapes.push(make_rotated_icon_mesh(
                            tex.id(),
                            Pos2::new(x + icon_size / 2.0, icon_center_y),
                            icon_size,
                            0.0,
                            tint,
                        ));
                    }
                    x += icon_size + gap;
                } else if victim_ship_w > 0.0 {
                    x += gap;
                }

                // Victim ship name
                if let Some(galley) = victim_ship_galley {
                    shapes.push(Shape::galley(Pos2::new(x, y), galley, Color32::TRANSPARENT));
                }
            }
        }

        DrawCommand::CapturePoint { pos, radius, color, alpha, label, progress, invader_color } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);

            shapes.push(Shape::circle_filled(center, r, color_from_rgba(*color, *alpha)));

            if *progress > 0.001
                && let Some(inv_color) = invader_color
            {
                let fill_alpha = (*alpha + 0.10).min(1.0);
                let sweep = *progress * std::f32::consts::TAU;
                let segments = 64;
                let start_angle = -std::f32::consts::FRAC_PI_2;
                let pie_color = color_from_rgba(*inv_color, fill_alpha);

                let mut mesh = egui::Mesh::default();
                mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: egui::pos2(0.0, 0.0), color: pie_color });
                let step_count = ((segments as f32 * (*progress)).ceil() as usize).max(1);
                let angle_step = sweep / step_count as f32;
                for i in 0..=step_count {
                    let angle = start_angle + i as f32 * angle_step;
                    let px = center.x + r * angle.cos();
                    let py = center.y + r * angle.sin();
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: egui::pos2(px, py),
                        uv: egui::pos2(0.0, 0.0),
                        color: pie_color,
                    });
                    if i > 0 {
                        let vi = mesh.vertices.len() as u32;
                        mesh.indices.extend_from_slice(&[0, vi - 2, vi - 1]);
                    }
                }
                shapes.push(Shape::Mesh(mesh.into()));
            }

            let outline_color = if *progress > 0.001 {
                invader_color.map(color_from_rgb).unwrap_or_else(|| color_from_rgb(*color))
            } else {
                color_from_rgb(*color)
            };
            shapes.push(Shape::circle_stroke(center, r, Stroke::new(transform.scale_stroke(1.5), outline_color)));

            if !label.is_empty() {
                let font = FontId::proportional(11.0 * ws);
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.clone(), font, Color32::WHITE));
                let text_w = galley.size().x;
                let text_h = galley.size().y;
                shapes.push(Shape::galley(
                    Pos2::new(center.x - text_w / 2.0, center.y - text_h / 2.0),
                    galley,
                    Color32::WHITE,
                ));
            }
        }

        DrawCommand::Building { pos, color, is_alive } => {
            let center = transform.minimap_to_screen(pos);
            let r = if *is_alive { transform.scale_distance(2.0) } else { transform.scale_distance(1.5) };
            shapes.push(Shape::circle_filled(center, r, color_from_rgb(*color)));
        }

        DrawCommand::TurretDirection { pos, yaw, color, length } => {
            let start = transform.minimap_to_screen(pos);
            // yaw is screen-space: 0 = east, PI/2 = north
            let dx = *length as f32 * yaw.cos();
            let dy = -*length as f32 * yaw.sin();
            let end = Pos2::new(start.x + transform.scale_distance(dx), start.y + transform.scale_distance(dy));
            let stroke_width = transform.scale_stroke(1.5);
            let c = color_from_rgb(*color);
            let line_color = Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), 180);
            shapes.push(Shape::line_segment([start, end], Stroke::new(stroke_width, line_color)));
        }

        DrawCommand::ConsumableRadius { pos, radius_px, color, alpha } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius_px as f32);
            let fill_color = color_from_rgba(*color, *alpha);
            shapes.push(Shape::circle_filled(center, r, fill_color));
            let outline_color = color_from_rgba(*color, 0.5);
            let stroke_w = transform.scale_stroke(2.0);
            shapes.push(Shape::circle_stroke(center, r, Stroke::new(stroke_w, outline_color)));
        }

        DrawCommand::PatrolRadius { pos, radius_px, color, alpha } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius_px as f32);
            let fill_color = color_from_rgba(*color, *alpha);
            shapes.push(Shape::circle_filled(center, r, fill_color));
        }

        DrawCommand::ConsumableIcons { pos, icon_keys, has_hp_bar, .. } => {
            let center = transform.minimap_to_screen(pos);
            // Position below HP bar (10 bar top + 3 bar height + 11 half-icon + 2 gap = 26)
            // or below the ship icon if no HP bar (10 + 11 half-icon + 2 gap = 23)
            let base_offset = if *has_hp_bar { 26.0 } else { 23.0 };
            let icon_y = center.y + transform.scale_distance(base_offset);
            let icon_size = transform.scale_distance(16.0);
            let gap = transform.scale_distance(1.0);
            let count = icon_keys.len() as f32;
            let total_width = count * icon_size + (count - 1.0) * gap;
            let start_x = center.x - total_width / 2.0 + icon_size / 2.0;
            for (i, icon_key) in icon_keys.iter().enumerate() {
                let icon_x = start_x + i as f32 * (icon_size + gap);
                if let Some(tex) = textures.consumable_icons.get(icon_key) {
                    let half = icon_size / 2.0;
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_max(
                        Pos2::new(icon_x - half, icon_y - half),
                        Pos2::new(icon_x + half, icon_y + half),
                    );
                    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
                    shapes.push(Shape::Mesh(mesh.into()));
                }
            }
        }

        DrawCommand::PositionTrail { points, .. } => {
            let dot_radius = transform.scale_distance(1.5);
            for (pos, color) in points {
                let center = transform.minimap_to_screen(pos);
                shapes.push(Shape::circle_filled(center, dot_radius, color_from_rgb(*color)));
            }
        }

        DrawCommand::ShipConfigCircle { pos, radius_px, color, alpha, dashed, label, .. } => {
            let center = transform.minimap_to_screen(pos);
            let screen_radius = transform.scale_distance(*radius_px);
            let circle_color = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], (alpha * 255.0) as u8);
            let stroke = Stroke::new(1.5, circle_color);

            if *dashed {
                // Dashed circle: draw as series of arcs
                let segments = 48;
                let gap_ratio = 0.4;
                for i in 0..segments {
                    let t0 = i as f32 / segments as f32 * std::f32::consts::TAU;
                    let t1 = (i as f32 + 1.0 - gap_ratio) / segments as f32 * std::f32::consts::TAU;
                    let steps = 4;
                    let points: Vec<Pos2> = (0..=steps)
                        .map(|s| {
                            let t = t0 + (t1 - t0) * s as f32 / steps as f32;
                            Pos2::new(center.x + screen_radius * t.cos(), center.y + screen_radius * t.sin())
                        })
                        .collect();
                    shapes.push(Shape::line(points, stroke));
                }
            } else {
                shapes.push(Shape::circle_stroke(center, screen_radius, stroke));
            }

            // Draw label near the top of the circle
            if let Some(text) = label {
                let label_pos = Pos2::new(center.x, center.y - screen_radius - 4.0);
                let galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), egui::FontId::proportional(10.0), circle_color));
                let text_width = galley.size().x;
                shapes.push(Shape::galley(
                    Pos2::new(label_pos.x - text_width / 2.0, label_pos.y - galley.size().y),
                    galley,
                    Color32::TRANSPARENT,
                ));
            }
        }

        DrawCommand::BuffZone { pos, radius, color, alpha, marker_name } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);

            // Filled circle
            shapes.push(Shape::circle_filled(center, r, color_from_rgba(*color, *alpha)));
            // Border ring
            shapes.push(Shape::circle_stroke(
                center,
                r,
                Stroke::new(transform.scale_stroke(1.5), color_from_rgba(*color, 0.6)),
            ));

            // Powerup icon centered on zone
            if let Some(name) = marker_name {
                if let Some(tex) = textures.powerup_icons.get(name.as_str()) {
                    let icon_size = transform.scale_distance(16.0);
                    let half = icon_size / 2.0;
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_max(
                        Pos2::new(center.x - half, center.y - half),
                        Pos2::new(center.x + half, center.y + half),
                    );
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));
                }
            }
        }

        DrawCommand::BattleResultOverlay { text, subtitle, color } => {
            let canvas_w = transform.screen_canvas_width();
            let canvas_h = (transform.canvas_width + transform.hud_height) * transform.window_scale;
            let center_x = transform.origin.x + canvas_w / 2.0;
            let center_y = transform.origin.y + canvas_h / 2.0;

            // Main text: 1/8 of canvas width as font size
            let font_size = canvas_w / 8.0;
            let main_font = FontId::proportional(font_size);
            let main_galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), main_font, Color32::WHITE));
            let main_w = main_galley.size().x;
            let main_h = main_galley.size().y;

            // Subtitle: 1/4 of main font size
            let sub_galley = subtitle.as_ref().map(|s| {
                let sub_font = FontId::proportional(font_size / 4.0);
                ctx.fonts_mut(|f| f.layout_no_wrap(s.clone(), sub_font, Color32::from_gray(200)))
            });
            let sub_h = sub_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
            let gap = if subtitle.is_some() { 8.0 * ws } else { 0.0 };
            let total_h = main_h + gap + sub_h;

            // Centered position for main text
            let text_x = center_x - main_w / 2.0;
            let text_y = center_y - total_h / 2.0;

            // Text glow layers matching video renderer approach:
            // dark shadows for contrast, then colored glow, then white text
            let offsets: &[(f32, f32)] =
                &[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)];
            let glow_layers: &[(f32, [u8; 3], f32)] = &[
                (6.0, [0, 0, 0], 0.15),
                (4.0, [0, 0, 0], 0.25),
                (3.0, *color, 0.30),
                (2.0, *color, 0.50),
                (1.0, *color, 0.70),
            ];

            for &(dist, c, opacity) in glow_layers {
                let layer_color = Color32::from_rgba_premultiplied(
                    (c[0] as f32 * opacity) as u8,
                    (c[1] as f32 * opacity) as u8,
                    (c[2] as f32 * opacity) as u8,
                    (255.0 * opacity) as u8,
                );
                let glow_font = FontId::proportional(font_size);
                for &(dx, dy) in offsets {
                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), glow_font.clone(), layer_color));
                    shapes.push(Shape::galley(
                        Pos2::new(text_x + dx * dist, text_y + dy * dist),
                        galley,
                        Color32::TRANSPARENT,
                    ));
                }
            }

            // Main white text on top
            shapes.push(Shape::galley(Pos2::new(text_x, text_y), main_galley, Color32::TRANSPARENT));

            // Subtitle
            if let Some(sub_galley) = sub_galley {
                let sub_w = sub_galley.size().x;
                let sub_x = center_x - sub_w / 2.0;
                let sub_y = text_y + main_h + gap;

                // Subtitle dark outline
                let sub_font = FontId::proportional(font_size / 4.0);
                for &(dx, dy) in offsets {
                    let outline = ctx.fonts_mut(|f| {
                        f.layout_no_wrap(
                            subtitle.as_ref().unwrap().clone(),
                            sub_font.clone(),
                            Color32::from_rgba_premultiplied(0, 0, 0, 180),
                        )
                    });
                    shapes.push(Shape::galley(
                        Pos2::new(sub_x + dx * 2.0, sub_y + dy * 2.0),
                        outline,
                        Color32::TRANSPARENT,
                    ));
                }

                shapes.push(Shape::galley(Pos2::new(sub_x, sub_y), sub_galley, Color32::TRANSPARENT));
            }
        }

        DrawCommand::TeamBuffs { friendly_buffs, enemy_buffs } => {
            let canvas_w = transform.screen_canvas_width();
            let icon_size = 16.0 * ws;
            let gap = 2.0 * ws;
            let buff_y = transform.hud_pos(0.0, 22.0).y;
            let origin_x = transform.hud_pos(0.0, 0.0).x;

            // Friendly buffs: left side
            let mut x = origin_x + 4.0 * ws;
            for (marker, count) in friendly_buffs {
                if let Some(tex) = textures.powerup_icons.get(marker.as_str()) {
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_size(Pos2::new(x, buff_y), Vec2::new(icon_size, icon_size));
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));

                    if *count > 1 {
                        let label = format!("{}", count);
                        let font = FontId::proportional(10.0 * ws);
                        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label, font, Color32::WHITE));
                        let tw = galley.size().x;
                        shapes.push(Shape::galley(
                            Pos2::new(x + icon_size, buff_y + 4.0 * ws),
                            galley,
                            Color32::TRANSPARENT,
                        ));
                        x += icon_size + tw + gap;
                    } else {
                        x += icon_size + gap;
                    }
                }
            }

            // Enemy buffs: right side
            let mut x = origin_x + canvas_w - 4.0 * ws;
            for (marker, count) in enemy_buffs {
                if let Some(tex) = textures.powerup_icons.get(marker.as_str()) {
                    if *count > 1 {
                        let label = format!("{}", count);
                        let font = FontId::proportional(10.0 * ws);
                        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label, font, Color32::WHITE));
                        let tw = galley.size().x;
                        x -= tw;
                        shapes.push(Shape::galley(Pos2::new(x, buff_y + 4.0 * ws), galley, Color32::TRANSPARENT));
                        x -= icon_size;
                    } else {
                        x -= icon_size;
                    }

                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_size(Pos2::new(x, buff_y), Vec2::new(icon_size, icon_size));
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));
                    x -= gap;
                }
            }
        }

        DrawCommand::ChatOverlay { entries } => {
            let canvas_w = transform.screen_canvas_width();
            let canvas_h = (transform.canvas_width + transform.hud_height) * transform.window_scale;
            let header_font = FontId::proportional(11.0 * ws);
            let msg_font = FontId::proportional(11.0 * ws);
            let line_h = 14.0 * ws;
            let icon_size = 12.0 * ws;
            let padding = 6.0 * ws;
            let entry_gap = 6.0 * ws;

            // Chat box: left side, vertically centered, 25% of canvas width
            let box_w = canvas_w * 0.25;
            let box_x = transform.origin.x + 4.0 * ws;
            let inner_w = box_w - padding * 2.0;

            struct ChatLayout {
                /// Line 1: "[CLAN] PlayerName" — clan portion in clan color, rest in team color
                clan_galley: Option<std::sync::Arc<egui::Galley>>,
                name_galley: std::sync::Arc<egui::Galley>,
                /// Line 2: ship icon + ship name
                ship_icon_species: Option<String>,
                ship_name_galley: Option<std::sync::Arc<egui::Galley>>,
                /// Line 3+: word-wrapped message
                msg_galleys: Vec<std::sync::Arc<egui::Galley>>,
                opacity: f32,
                team_color: [u8; 3],
            }

            let mut layouts = Vec::new();
            let mut total_h = padding; // top padding
            for entry in entries {
                let opacity = entry.opacity;
                let alpha = (opacity * 255.0) as u8;
                let team_color = entry.team_color;
                let team_c = Color32::from_rgba_unmultiplied(team_color[0], team_color[1], team_color[2], alpha);

                // Line 1: clan tag + player name
                let clan_galley =
                    if !entry.clan_tag.is_empty() {
                        let clan_c = if let Some(cc) = entry.clan_color {
                            Color32::from_rgba_unmultiplied(cc[0], cc[1], cc[2], alpha)
                        } else {
                            team_c
                        };
                        Some(ctx.fonts_mut(|f| {
                            f.layout_no_wrap(format!("[{}] ", entry.clan_tag), header_font.clone(), clan_c)
                        }))
                    } else {
                        None
                    };
                let name_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.player_name.clone(), header_font.clone(), team_c));

                // Line 2: ship icon + ship name (optional)
                let ship_name_galley = entry
                    .ship_name
                    .as_ref()
                    .map(|sn| ctx.fonts_mut(|f| f.layout_no_wrap(sn.clone(), header_font.clone(), team_c)));
                let has_ship_line = ship_name_galley.is_some();

                // Message lines (word-wrapped)
                let msg_color = Color32::from_rgba_unmultiplied(
                    entry.message_color[0],
                    entry.message_color[1],
                    entry.message_color[2],
                    alpha,
                );
                let msg_galleys = ctx.fonts_mut(|f| {
                    let job =
                        egui::text::LayoutJob::simple(entry.message.clone(), msg_font.clone(), msg_color, inner_w);
                    let galley = f.layout_job(job);
                    vec![galley]
                });

                let msg_lines: usize = msg_galleys.iter().map(|g| g.rows.len().max(1)).sum();
                let line_count = 1 + has_ship_line as usize + msg_lines;
                total_h += line_count as f32 * line_h + entry_gap;

                layouts.push(ChatLayout {
                    clan_galley,
                    name_galley,
                    ship_icon_species: entry.ship_species.clone(),
                    ship_name_galley,
                    msg_galleys,
                    opacity,
                    team_color,
                });
            }

            if layouts.is_empty() {
                // nothing to draw
            } else {
                total_h += padding; // bottom padding
                let box_y = transform.origin.y + canvas_h / 2.0 - total_h / 2.0;

                // Semi-translucent background
                let bg_rect = Rect::from_min_size(Pos2::new(box_x, box_y), Vec2::new(box_w, total_h));
                shapes.push(Shape::rect_filled(bg_rect, CornerRadius::same(3), Color32::from_black_alpha(90)));

                let mut y = box_y + padding;
                for layout in &layouts {
                    let alpha = (layout.opacity * 255.0) as u8;
                    let x = box_x + padding;

                    // Line 1: [CLAN] PlayerName
                    let mut nx = x;
                    if let Some(ref cg) = layout.clan_galley {
                        shapes.push(Shape::galley(Pos2::new(nx, y), cg.clone(), Color32::TRANSPARENT));
                        nx += cg.size().x;
                    }
                    shapes.push(Shape::galley(Pos2::new(nx, y), layout.name_galley.clone(), Color32::TRANSPARENT));
                    y += line_h;

                    // Line 2: ship icon + ship name
                    if let Some(ref sng) = layout.ship_name_galley {
                        let mut sx = x;
                        if let Some(ref species) = layout.ship_icon_species {
                            if let Some(tex) = textures.ship_icons.get(species.as_str()) {
                                let tc = layout.team_color;
                                let tint = Color32::from_rgba_unmultiplied(tc[0], tc[1], tc[2], alpha);
                                // Vertically center icon with the text on this line
                                let icon_center_y = y + sng.size().y / 2.0;
                                shapes.push(make_rotated_icon_mesh(
                                    tex.id(),
                                    Pos2::new(sx + icon_size / 2.0, icon_center_y),
                                    icon_size,
                                    0.0,
                                    tint,
                                ));
                            }
                            sx += icon_size + 2.0 * ws;
                        }
                        shapes.push(Shape::galley(Pos2::new(sx, y), sng.clone(), Color32::TRANSPARENT));
                        y += line_h;
                    }

                    // Message text (word-wrapped)
                    for galley in &layout.msg_galleys {
                        shapes.push(Shape::galley(Pos2::new(x, y), galley.clone(), Color32::TRANSPARENT));
                        y += galley.rows.len().max(1) as f32 * line_h;
                    }

                    y += entry_gap;
                }
            }
        }
    }

    shapes
}

// ─── Texture Upload ──────────────────────────────────────────────────────────

/// Generate an outline RGBA image from a source icon's alpha channel.
/// The outline is `thickness` pixels wide around opaque regions (alpha > 128).
/// Returns (rgba_data, width, height) with the same dimensions as the input.
fn generate_icon_outline(data: &[u8], w: u32, h: u32, thickness: i32) -> Vec<u8> {
    let iw = w as i32;
    let ih = h as i32;
    let mut out = vec![0u8; (w * h * 4) as usize];

    for y in 0..ih {
        for x in 0..iw {
            let idx = (y * iw + x) as usize;
            let self_alpha = data[idx * 4 + 3];
            if self_alpha > 128 {
                // Inside the icon — leave transparent (icon itself will be drawn on top)
                continue;
            }

            // Check if any neighbor within `thickness` is opaque
            let mut has_opaque_neighbor = false;
            'outer: for ny in (y - thickness).max(0)..=(y + thickness).min(ih - 1) {
                for nx in (x - thickness).max(0)..=(x + thickness).min(iw - 1) {
                    let ni = (ny * iw + nx) as usize;
                    if data[ni * 4 + 3] > 128 {
                        has_opaque_neighbor = true;
                        break 'outer;
                    }
                }
            }

            if has_opaque_neighbor {
                let oi = idx * 4;
                out[oi] = 255; // R (gold)
                out[oi + 1] = 215; // G
                out[oi + 2] = 0; // B
                out[oi + 3] = 230; // A
            }
        }
    }

    out
}

fn upload_textures(ctx: &egui::Context, assets: &ReplayRendererAssets) -> RendererTextures {
    let map_texture = assets.map_image.as_ref().map(|asset| {
        let (ref data, w, h) = **asset;
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], data);
        ctx.load_texture("replay_map", image, egui::TextureOptions::LINEAR)
    });

    let mut ship_icons: HashMap<String, TextureHandle> = HashMap::new();
    let mut ship_icon_outlines: HashMap<String, TextureHandle> = HashMap::new();
    for (key, (data, w, h)) in assets.ship_icons.iter() {
        let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
        let handle = ctx.load_texture(format!("ship_{}", key), image, egui::TextureOptions::LINEAR);
        ship_icons.insert(key.clone(), handle);

        let outline_data = generate_icon_outline(data, *w, *h, 2);
        let outline_image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], &outline_data);
        let outline_handle =
            ctx.load_texture(format!("ship_outline_{}", key), outline_image, egui::TextureOptions::LINEAR);
        ship_icon_outlines.insert(key.clone(), outline_handle);
    }

    let plane_icons: HashMap<String, TextureHandle> = assets
        .plane_icons
        .iter()
        .map(|(key, (data, w, h))| {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
            let handle = ctx.load_texture(format!("plane_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let consumable_icons: HashMap<String, TextureHandle> = assets
        .consumable_icons
        .iter()
        .map(|(key, (data, w, h))| {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
            let handle = ctx.load_texture(format!("consumable_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let death_cause_icons: HashMap<String, TextureHandle> = assets
        .death_cause_icons
        .iter()
        .map(|(key, (data, w, h))| {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
            let handle = ctx.load_texture(format!("death_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let powerup_icons: HashMap<String, TextureHandle> = assets
        .powerup_icons
        .iter()
        .map(|(key, (data, w, h))| {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
            let handle = ctx.load_texture(format!("powerup_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    RendererTextures {
        map_texture,
        ship_icons,
        ship_icon_outlines,
        plane_icons,
        consumable_icons,
        death_cause_icons,
        powerup_icons,
    }
}

// ─── Viewport Rendering ─────────────────────────────────────────────────────

impl ReplayRendererViewer {
    pub fn draw(&self, ctx: &egui::Context) {
        // Store the parent context so the background thread can request repaints.
        // Deferred viewports repaint as part of their parent's paint cycle.
        {
            let mut state = self.shared_state.lock();
            if state.viewport_ctx.is_none() {
                state.viewport_ctx = Some(ctx.clone());
            }
        }

        let shared_state = self.shared_state.clone();
        let command_tx = self.command_tx.clone();
        let window_open = self.open.clone();
        let textures_arc = self.textures.clone();
        let pending_save = self.pending_defaults_save.clone();
        let toasts = self.toasts.clone();
        let video_exporting = self.video_exporting.clone();
        let video_export_data = self.video_export_data.clone();
        let zoom_pan_arc = self.zoom_pan.clone();
        let overlay_state_arc = self.overlay_state.clone();
        let annotation_arc = self.annotation_state.clone();

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(&*self.title),
            egui::ViewportBuilder::default()
                .with_title(&*self.title)
                .with_inner_size([800.0, 900.0])
                .with_min_inner_size([400.0, 450.0]),
            move |ctx, _class| {
                if !window_open.load(Ordering::Relaxed) {
                    return;
                }

                let state = shared_state.lock();
                let status_is_loading = matches!(state.status, RendererStatus::Loading);
                let status_error = match &state.status {
                    RendererStatus::Error(e) => Some(e.clone()),
                    _ => None,
                };
                let has_assets = state.assets.is_some();
                let playing = state.playing;
                let speed = state.speed;
                let options = state.options.clone();
                let show_dead_ships = state.show_dead_ships;
                let battle_start = state.battle_start;
                let frame_data =
                    state.frame.as_ref().map(|f| (f.frame_index, f.total_frames, f.clock_seconds, f.game_duration));
                drop(state);

                // Upload textures on first ready frame
                {
                    let mut tex_guard = textures_arc.lock();
                    if tex_guard.is_none() && has_assets {
                        let state = shared_state.lock();
                        if let Some(assets) = &state.assets {
                            *tex_guard = Some(upload_textures(ctx, assets));
                        }
                    }
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    if status_is_loading {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                            ui.label("Loading replay data...");
                        });
                        ctx.request_repaint();
                        return;
                    }

                    if let Some(err) = status_error {
                        ui.colored_label(Color32::RED, format!("Error: {}", err));
                        return;
                    }

                    // Canvas area — fill all available space.
                    // window_scale maps logical canvas pixels to screen pixels.
                    // We use the full available rect so the viewport expands when
                    // the window is resized (showing more map area when zoomed).
                    let logical_canvas = Vec2::new(MINIMAP_SIZE as f32, CANVAS_HEIGHT as f32);
                    let available = ui.available_size();
                    let scale_x = available.x / logical_canvas.x;
                    let scale_y = available.y / logical_canvas.y;
                    let fit_scale = scale_x.min(scale_y);
                    let fill_scale = scale_x.max(scale_y);
                    // Smoothly blend from fit (full canvas visible, centered) at zoom 1.0
                    // to fill (no empty borders) by zoom 2.0.
                    let current_zoom = zoom_pan_arc.lock().zoom;
                    let t = ((current_zoom - 1.0) / 1.0).clamp(0.0, 1.0);
                    let window_scale = (fit_scale + t * (fill_scale - fit_scale)).max(0.1);
                    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
                    // Center the scaled canvas within the available rect
                    let scaled_canvas = logical_canvas * window_scale;
                    let offset_x = ((available.x - scaled_canvas.x) / 2.0).max(0.0);
                    let offset_y = ((available.y - scaled_canvas.y) / 2.0).max(0.0);
                    let origin = response.rect.min + Vec2::new(offset_x, offset_y);

                    // Zoom/pan input handling
                    let mut zp = zoom_pan_arc.lock();
                    let mut zoom_changed = false;
                    let tool_active = !matches!(annotation_arc.lock().active_tool, PaintTool::None);

                    // Scroll-wheel: zoom (normal) or rotate (when placing ship/rect/tri)
                    if response.hovered() {
                        let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
                        if scroll_delta != 0.0 {
                            let mut ann = annotation_arc.lock();
                            let scroll_used_by_tool = match &mut ann.active_tool {
                                PaintTool::PlacingShip { yaw, .. } => {
                                    *yaw += scroll_delta * 0.005;
                                    true
                                }
                                PaintTool::DrawingRect { .. } | PaintTool::DrawingTriangle { .. } => {
                                    // Rotation handled during placement in Phase 4
                                    false
                                }
                                _ => false,
                            };
                            drop(ann);

                            if !scroll_used_by_tool {
                                let zoom_speed = 0.01;
                                let old_zoom = zp.zoom;
                                let new_zoom = (old_zoom * (1.0 + scroll_delta * zoom_speed)).clamp(1.0, 10.0);

                                if new_zoom != old_zoom {
                                    if let Some(cursor) = response.hover_pos() {
                                        let local_x = (cursor.x - origin.x) / window_scale;
                                        let local_y = (cursor.y - origin.y) / window_scale - HUD_HEIGHT as f32;
                                        let minimap_x = (local_x + zp.pan.x) / old_zoom;
                                        let minimap_y = (local_y + zp.pan.y) / old_zoom;
                                        zp.pan.x = minimap_x * new_zoom - local_x;
                                        zp.pan.y = minimap_y * new_zoom - local_y;
                                    }
                                    zp.zoom = new_zoom;
                                    zoom_changed = true;
                                }
                            }
                        }
                    }

                    // Drag-to-pan: middle always pans, left only when no tool and no selection
                    let has_selection = annotation_arc.lock().selected_index.is_some();
                    if response.dragged_by(egui::PointerButton::Middle)
                        || (!tool_active && !has_selection && response.dragged_by(egui::PointerButton::Primary))
                    {
                        let drag = response.drag_delta();
                        zp.pan.x -= drag.x / window_scale;
                        zp.pan.y -= drag.y / window_scale;
                        zoom_changed = true;
                    }

                    // Double-click to reset zoom
                    if response.double_clicked() {
                        zp.zoom = 1.0;
                        zp.pan = Vec2::ZERO;
                        zoom_changed = true;
                    }

                    // Clamp pan so the map can't scroll past its edges.
                    // Visible area in logical space: use the smaller of available vs scaled_canvas
                    // (when window < canvas, available constrains; when window > canvas, scaled_canvas constrains)
                    let visible_w = available.x.min(scaled_canvas.x) / window_scale;
                    let visible_h =
                        (available.y.min(scaled_canvas.y) - HUD_HEIGHT as f32 * window_scale) / window_scale;
                    let map_zoomed = MINIMAP_SIZE as f32 * zp.zoom;
                    let max_pan_x = (map_zoomed - visible_w).max(0.0);
                    let max_pan_y = (map_zoomed - visible_h).max(0.0);
                    zp.pan.x = zp.pan.x.clamp(0.0, max_pan_x);
                    zp.pan.y = zp.pan.y.clamp(0.0, max_pan_y);

                    // Build transform for this frame
                    let transform = MapTransform {
                        origin,
                        window_scale,
                        zoom: zp.zoom,
                        pan: zp.pan,
                        hud_height: HUD_HEIGHT as f32,
                        canvas_width: MINIMAP_SIZE as f32,
                    };
                    let current_zoom = zp.zoom;
                    drop(zp);

                    // Cursor icon based on tool / zoom state
                    if response.hovered() {
                        let cursor = {
                            let ann = annotation_arc.lock();
                            match &ann.active_tool {
                                PaintTool::PlacingShip { .. } => Some(egui::CursorIcon::Cell),
                                PaintTool::Freehand { .. }
                                | PaintTool::Eraser
                                | PaintTool::DrawingLine { .. }
                                | PaintTool::DrawingCircle { .. }
                                | PaintTool::DrawingRect { .. }
                                | PaintTool::DrawingTriangle { .. } => Some(egui::CursorIcon::None),
                                PaintTool::None => {
                                    if let Some(sel) = ann.selected_index {
                                        if ann.dragging_rotation {
                                            Some(egui::CursorIcon::Grabbing)
                                        } else if sel < ann.annotations.len() {
                                            // Check if hovering the rotation handle
                                            let has_rot = matches!(
                                                ann.annotations[sel],
                                                Annotation::Ship { .. }
                                                    | Annotation::Rectangle { .. }
                                                    | Annotation::Triangle { .. }
                                            );
                                            let on_handle = has_rot
                                                && response.hover_pos().is_some_and(|hp| {
                                                    let (handle, _) =
                                                        rotation_handle_pos(&ann.annotations[sel], &transform);
                                                    (hp - handle).length() < ROTATION_HANDLE_RADIUS + 8.0
                                                });
                                            if on_handle {
                                                Some(egui::CursorIcon::Alias)
                                            } else {
                                                Some(egui::CursorIcon::Grab)
                                            }
                                        } else {
                                            Some(egui::CursorIcon::Grab)
                                        }
                                    } else {
                                        // Check if hovering over a ship (show context menu cursor)
                                        let near_ship = response.hover_pos().is_some_and(|hp| {
                                            let state = shared_state.lock();
                                            if let Some(ref frame) = state.frame {
                                                frame.commands.iter().any(|cmd| {
                                                    if let DrawCommand::Ship { pos, player_name: Some(_), .. } = cmd {
                                                        let sp = transform.minimap_to_screen(pos);
                                                        hp.distance(sp) < 30.0
                                                    } else {
                                                        false
                                                    }
                                                })
                                            } else {
                                                false
                                            }
                                        });
                                        if near_ship {
                                            Some(egui::CursorIcon::PointingHand)
                                        } else {
                                            None // fall through to zoom cursor
                                        }
                                    }
                                }
                            }
                        };
                        if let Some(c) = cursor {
                            if response.dragged() && c == egui::CursorIcon::Grab {
                                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                            } else {
                                ctx.set_cursor_icon(c);
                            }
                        } else if current_zoom > 1.01 {
                            if response.dragged() {
                                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                            } else {
                                ctx.set_cursor_icon(egui::CursorIcon::Grab);
                            }
                        }
                    }

                    // Request repaint if zoom/pan changed while paused
                    if zoom_changed && !playing {
                        ctx.request_repaint();
                    }

                    // Draw dark background
                    painter.rect_filled(response.rect, CornerRadius::ZERO, Color32::from_rgb(20, 25, 35));

                    // Clipped painter for map-region content (below HUD)
                    let hud_screen_height = HUD_HEIGHT as f32 * window_scale;
                    let map_clip = Rect::from_min_max(
                        Pos2::new(origin.x, origin.y + hud_screen_height),
                        Pos2::new(origin.x + scaled_canvas.x, origin.y + scaled_canvas.y),
                    );
                    let map_painter = painter.with_clip_rect(map_clip);

                    let tex_guard = textures_arc.lock();
                    if let Some(ref textures) = *tex_guard {
                        // Draw map texture (clipped to map region)
                        if let Some(ref map_tex) = textures.map_texture {
                            let map_tl = transform.minimap_to_screen(&MinimapPos { x: 0, y: 0 });
                            let map_br = transform
                                .minimap_to_screen(&MinimapPos { x: MINIMAP_SIZE as i32, y: MINIMAP_SIZE as i32 });
                            let map_rect = Rect::from_min_max(map_tl, map_br);
                            let mut mesh = egui::Mesh::with_texture(map_tex.id());
                            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                            mesh.add_rect_with_uv(map_rect, uv, Color32::WHITE);
                            map_painter.add(Shape::Mesh(mesh.into()));
                        }

                        // Draw grid overlay (A-J / 1-10)
                        {
                            let cell_logical = MINIMAP_SIZE as f32 / 10.0;
                            let grid_color = Color32::from_rgba_unmultiplied(255, 255, 255, 64);
                            let grid_stroke = Stroke::new(transform.scale_stroke(1.0), grid_color);

                            for i in 1..10 {
                                let offset = (i as f32 * cell_logical) as i32;
                                let top = transform.minimap_to_screen(&MinimapPos { x: offset, y: 0 });
                                let bottom =
                                    transform.minimap_to_screen(&MinimapPos { x: offset, y: MINIMAP_SIZE as i32 });
                                map_painter.add(Shape::LineSegment { points: [top, bottom], stroke: grid_stroke });

                                let left = transform.minimap_to_screen(&MinimapPos { x: 0, y: offset });
                                let right =
                                    transform.minimap_to_screen(&MinimapPos { x: MINIMAP_SIZE as i32, y: offset });
                                map_painter.add(Shape::LineSegment { points: [left, right], stroke: grid_stroke });
                            }

                            let label_font = FontId::proportional(11.0 * window_scale);
                            let label_color = Color32::from_rgba_unmultiplied(255, 255, 255, 180);
                            for i in 0..10 {
                                // Numbers 1-10 across the top
                                let num_label = format!("{}", i + 1);
                                let galley =
                                    ctx.fonts_mut(|f| f.layout_no_wrap(num_label, label_font.clone(), label_color));
                                let text_w = galley.size().x;
                                let cell_center_x = (i as f32 * cell_logical + cell_logical / 2.0) as i32;
                                let pos = transform.minimap_to_screen(&MinimapPos { x: cell_center_x, y: 2 });
                                map_painter.add(Shape::galley(
                                    Pos2::new(pos.x - text_w / 2.0, pos.y),
                                    galley,
                                    label_color,
                                ));

                                // Letters A-J down the left
                                let letter = (b'A' + i as u8) as char;
                                let galley = ctx.fonts_mut(|f| {
                                    f.layout_no_wrap(letter.to_string(), label_font.clone(), label_color)
                                });
                                let text_h = galley.size().y;
                                let cell_center_y = (i as f32 * cell_logical + cell_logical / 2.0) as i32;
                                let pos = transform.minimap_to_screen(&MinimapPos { x: 3, y: cell_center_y });
                                map_painter.add(Shape::galley(
                                    Pos2::new(pos.x, pos.y - text_h / 2.0),
                                    galley,
                                    label_color,
                                ));
                            }
                        }

                        // Draw current frame's commands, filtered by UI-local options
                        let (trail_hidden_ships, ship_range_overrides) = {
                            let ann = annotation_arc.lock();
                            (ann.trail_hidden_ships.clone(), ann.ship_range_overrides.clone())
                        };
                        let state = shared_state.lock();
                        if let Some(ref frame) = state.frame {
                            // Collect alive ship names for filtering config circles
                            let alive_ships: HashSet<&str> = frame
                                .commands
                                .iter()
                                .filter_map(|cmd| {
                                    if let DrawCommand::Ship { player_name: Some(name), .. } = cmd {
                                        Some(name.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            // Separate HUD and map commands so HUD draws on unclipped painter
                            for cmd in &frame.commands {
                                if !should_draw_command(cmd, &options, show_dead_ships) {
                                    continue;
                                }
                                // Apply per-ship trail filter
                                if let DrawCommand::PositionTrail { player_name, .. } = cmd {
                                    if let Some(name) = player_name {
                                        if trail_hidden_ships.contains(name) {
                                            continue;
                                        }
                                    }
                                }
                                // Apply per-ship config circle filter (only show if explicitly enabled via right-click, never for dead ships)
                                if let DrawCommand::ShipConfigCircle { player_name, kind, .. } = cmd {
                                    if !alive_ships.contains(player_name.as_str()) {
                                        continue;
                                    }
                                    let enabled = if let Some(overrides) = ship_range_overrides.get(player_name) {
                                        let kind_idx = match kind {
                                            ShipConfigCircleKind::Detection => 0,
                                            ShipConfigCircleKind::MainBattery => 1,
                                            ShipConfigCircleKind::SecondaryBattery => 2,
                                            ShipConfigCircleKind::Radar => 3,
                                            ShipConfigCircleKind::Hydro => 4,
                                        };
                                        overrides[kind_idx]
                                    } else {
                                        false // hidden by default; must enable per-ship via right-click
                                    };
                                    if !enabled {
                                        continue;
                                    }
                                }
                                let is_hud = matches!(
                                    cmd,
                                    DrawCommand::ScoreBar { .. }
                                        | DrawCommand::Timer { .. }
                                        | DrawCommand::PreBattleCountdown { .. }
                                        | DrawCommand::KillFeed { .. }
                                        | DrawCommand::TeamBuffs { .. }
                                        | DrawCommand::BattleResultOverlay { .. }
                                        | DrawCommand::ChatOverlay { .. }
                                        | DrawCommand::TeamAdvantage { .. }
                                );
                                let cmd_shapes = draw_command_to_shapes(cmd, &transform, textures, ctx, &options);
                                let target_painter = if is_hud { &painter } else { &map_painter };
                                for shape in cmd_shapes {
                                    target_painter.add(shape);
                                }
                            }

                            // Hover tooltip for TeamAdvantage
                            let ws = transform.window_scale;
                            // Find ScoreBar to compute advantage label position
                            let score_bar_info = frame.commands.iter().find_map(|cmd| {
                                if let DrawCommand::ScoreBar { team0, team1, team0_timer, team1_timer, advantage_team, .. } = cmd {
                                    Some((*team0, *team1, team0_timer.clone(), team1_timer.clone(), *advantage_team))
                                } else {
                                    None
                                }
                            });
                            for cmd in &frame.commands {
                                if let DrawCommand::TeamAdvantage { label, color, breakdown } = cmd {
                                    if label.is_empty() {
                                        break;
                                    }
                                    let canvas_w = transform.screen_canvas_width();
                                    let bar_height = 20.0 * ws;
                                    let bar_origin = transform.hud_pos(0.0, 0.0);

                                    // Recompute cursor positions matching ScoreBar rendering
                                    let score_font = FontId::proportional(14.0 * ws);
                                    let timer_font = FontId::proportional(12.0 * ws);
                                    let adv_font = FontId::proportional(11.0 * ws);
                                    let adv_color = color_from_rgb(*color);
                                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.clone(), adv_font, adv_color));
                                    let text_w = galley.size().x;
                                    let text_h = galley.size().y;

                                    let (t0_end_x, t1_start_x) = if let Some((t0_score, t1_score, ref t0_timer, ref t1_timer, _)) = score_bar_info {
                                        let t0_text = format!("{}", t0_score);
                                        let t0g = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text, score_font.clone(), Color32::WHITE));
                                        let mut t0_end = bar_origin.x + 8.0 * ws + t0g.size().x;
                                        if let Some(timer) = t0_timer {
                                            let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font.clone(), Color32::WHITE));
                                            t0_end = t0_end + 4.0 * ws + tg.size().x;
                                        }
                                        let t1_text = format!("{}", t1_score);
                                        let t1g = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text, score_font, Color32::WHITE));
                                        let mut t1_start = bar_origin.x + canvas_w - t1g.size().x - 8.0 * ws;
                                        if let Some(timer) = t1_timer {
                                            let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font, Color32::WHITE));
                                            t1_start = t1_start - tg.size().x - 4.0 * ws;
                                        }
                                        (t0_end, t1_start)
                                    } else {
                                        let half = canvas_w / 2.0;
                                        (bar_origin.x + half, bar_origin.x + half)
                                    };

                                    let adv_team = score_bar_info.as_ref().map(|s| s.4).unwrap_or(-1);
                                    let x = if adv_team == 0 {
                                        t0_end_x + 6.0 * ws
                                    } else {
                                        t1_start_x - text_w - 6.0 * ws
                                    };
                                    let y = bar_origin.y + (bar_height - text_h) / 2.0;
                                    let label_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(text_w, text_h));
                                    let resp = ui.interact(
                                        label_rect,
                                        egui::Id::new("advantage_tooltip"),
                                        egui::Sense::hover(),
                                    );
                                    resp.on_hover_ui(|ui| {
                                        let bd = breakdown;
                                        let fmt_contrib = |val: f64| -> String {
                                            if val > 0.0 {
                                                format!("+{:.1}", val)
                                            } else if val < 0.0 {
                                                format!("{:.1}", val)
                                            } else {
                                                "0".to_string()
                                            }
                                        };
                                        ui.label(egui::RichText::new("Advantage Breakdown").strong());
                                        ui.separator();
                                        if bd.team_eliminated {
                                            ui.label("A team has been eliminated");
                                        } else {
                                            egui::Grid::new("adv_grid").num_columns(2).show(ui, |ui| {
                                                if bd.time_to_win != 0.0 {
                                                    ui.label("Time to Win");
                                                    ui.label(fmt_contrib(bd.time_to_win));
                                                    ui.end_row();
                                                }
                                                if bd.score_gap != 0.0 {
                                                    ui.label("Score Gap");
                                                    ui.label(fmt_contrib(bd.score_gap));
                                                    ui.end_row();
                                                }
                                                if bd.projection != 0.0 {
                                                    ui.label("Score Projection");
                                                    ui.label(fmt_contrib(bd.projection));
                                                    ui.end_row();
                                                }
                                                if bd.cap_control != 0.0 {
                                                    ui.label("Cap Control");
                                                    ui.label(fmt_contrib(bd.cap_control));
                                                    ui.end_row();
                                                }
                                                if bd.hp != 0.0 {
                                                    ui.label("HP Advantage");
                                                    ui.label(fmt_contrib(bd.hp));
                                                    ui.end_row();
                                                }
                                                if bd.ship_count != 0.0 {
                                                    ui.label("Ship Count");
                                                    ui.label(fmt_contrib(bd.ship_count));
                                                    ui.end_row();
                                                }
                                                ui.separator();
                                                ui.separator();
                                                ui.end_row();
                                                ui.label(egui::RichText::new("Total").strong());
                                                ui.label(egui::RichText::new(fmt_contrib(bd.total)).strong());
                                                ui.end_row();
                                            });
                                            if !bd.hp_data_reliable {
                                                ui.small("HP/ship data incomplete — only score factors shown");
                                            }
                                        }
                                    });
                                    break;
                                }
                            }
                        }
                        drop(state);

                        // ─── Render annotations on map ───────────────────────────
                        {
                            let ann_state = annotation_arc.lock();
                            for ann in &ann_state.annotations {
                                render_annotation(ann, &transform, textures, &map_painter);
                            }
                            // Draw selection highlight
                            if let Some(sel) = ann_state.selected_index
                                && sel < ann_state.annotations.len()
                            {
                                render_selection_highlight(&ann_state.annotations[sel], &transform, &map_painter);
                            }
                            // Render tool preview (ghost shape at cursor)
                            if let Some(cursor_pos) = response.hover_pos() {
                                let minimap_pos = transform.screen_to_minimap(cursor_pos);
                                render_tool_preview(
                                    &ann_state.active_tool,
                                    minimap_pos,
                                    ann_state.paint_color,
                                    ann_state.stroke_width,
                                    &transform,
                                    textures,
                                    &map_painter,
                                );
                            }
                        }
                    }
                    drop(tex_guard);

                    // ─── Active tool indicator ───────────────────────────────────
                    {
                        let ann = annotation_arc.lock();
                        let tool_label = match &ann.active_tool {
                            PaintTool::None => None,
                            PaintTool::PlacingShip { species, friendly, .. } => {
                                let team = if *friendly { "Friendly" } else { "Enemy" };
                                Some(format!("Placing {} {}", team, ship_short_name(species)))
                            }
                            PaintTool::Freehand { .. } => Some("Freehand".into()),
                            PaintTool::Eraser => Some("Eraser".into()),
                            PaintTool::DrawingLine { .. } => Some("Line".into()),
                            PaintTool::DrawingCircle { .. } => Some("Circle".into()),
                            PaintTool::DrawingRect { .. } => Some("Rectangle".into()),
                            PaintTool::DrawingTriangle { .. } => Some("Triangle".into()),
                        };
                        if let Some(label) = tool_label {
                            let text_pos = Pos2::new(response.rect.left() + 8.0, response.rect.top() + 8.0);
                            painter.text(
                                text_pos,
                                egui::Align2::LEFT_TOP,
                                format!("{} (right-click to cancel)", label),
                                FontId::proportional(13.0),
                                Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                            );
                        }
                    }

                    // ─── Annotation input handling ────────────────────────────────
                    {
                        let mut ann = annotation_arc.lock();
                        let tool_active = !matches!(ann.active_tool, PaintTool::None);

                        // When a tool is active, clear any selection
                        if tool_active {
                            ann.selected_index = None;
                        }

                        // Right-click: open context menu or cancel tool
                        if response.secondary_clicked() {
                            if tool_active {
                                ann.active_tool = PaintTool::None;
                            } else {
                                let click_pos = response.interact_pointer_pos().unwrap_or(response.rect.center());
                                ann.show_context_menu = true;
                                ann.context_menu_pos = click_pos;

                                // Detect nearest ship to right-click position
                                ann.context_menu_ship = None;
                                let state = shared_state.lock();
                                if let Some(ref frame) = state.frame {
                                    let mut best_dist = 30.0_f32; // max click distance in screen px
                                    for cmd in &frame.commands {
                                        if let DrawCommand::Ship { pos, player_name: Some(name), .. } = cmd {
                                            let screen_pos = transform.minimap_to_screen(pos);
                                            let dist = click_pos.distance(screen_pos);
                                            if dist < best_dist {
                                                best_dist = dist;
                                                ann.context_menu_ship = Some(name.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Escape key: cancel tool or deselect
                        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                            if tool_active {
                                ann.active_tool = PaintTool::None;
                            } else {
                                ann.selected_index = None;
                            }
                        }

                        // Delete/Backspace to delete selected annotation
                        if !tool_active
                            && let Some(sel) = ann.selected_index
                            && sel < ann.annotations.len()
                            && ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                        {
                            ann.save_undo();
                            ann.annotations.remove(sel);
                            ann.selected_index = None;
                        }

                        // [ and ] to adjust stroke width when a tool is active
                        if tool_active {
                            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                                ann.stroke_width = (ann.stroke_width - 1.0).clamp(1.0, 8.0);
                            }
                            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                                ann.stroke_width = (ann.stroke_width + 1.0).clamp(1.0, 8.0);
                            }
                        }

                        // Left-click/drag tool actions
                        let cursor_minimap = response.hover_pos().map(|p| transform.screen_to_minimap(p));

                        // Copy these before the match to avoid borrowing ann while matching on active_tool
                        let paint_color = ann.paint_color;
                        let stroke_width = ann.stroke_width;
                        let mut new_annotation: Option<Annotation> = None;
                        let mut erase_idx: Option<usize> = None;

                        match &mut ann.active_tool {
                            PaintTool::None => {
                                // Check if drag started on the rotation handle
                                if response.drag_started_by(egui::PointerButton::Primary)
                                    && let Some(sel) = ann.selected_index
                                    && sel < ann.annotations.len()
                                {
                                    let has_rot = matches!(
                                        ann.annotations[sel],
                                        Annotation::Ship { .. }
                                            | Annotation::Rectangle { .. }
                                            | Annotation::Triangle { .. }
                                    );
                                    if has_rot && let Some(drag_origin) = response.interact_pointer_pos() {
                                        let (handle, _) = rotation_handle_pos(&ann.annotations[sel], &transform);
                                        if (drag_origin - handle).length() < ROTATION_HANDLE_RADIUS + 8.0 {
                                            ann.dragging_rotation = true;
                                        }
                                    }
                                }

                                // Handle rotation drag
                                if ann.dragging_rotation
                                    && response.dragged_by(egui::PointerButton::Primary)
                                    && let Some(sel) = ann.selected_index
                                    && sel < ann.annotations.len()
                                    && let Some(cursor_screen) = response.hover_pos()
                                {
                                    let center_screen =
                                        annotation_screen_bounds(&ann.annotations[sel], &transform).center();
                                    let angle = -(cursor_screen.x - center_screen.x)
                                        .atan2(-(cursor_screen.y - center_screen.y));
                                    match &mut ann.annotations[sel] {
                                        Annotation::Ship { yaw, .. } => *yaw = angle,
                                        Annotation::Rectangle { rotation, .. } => *rotation = angle,
                                        Annotation::Triangle { rotation, .. } => *rotation = angle,
                                        _ => {}
                                    }
                                }

                                // Stop rotation drag
                                if response.drag_stopped_by(egui::PointerButton::Primary) {
                                    ann.dragging_rotation = false;
                                }

                                // Click to select/deselect annotations
                                if response.clicked()
                                    && let Some(click_pos) = cursor_minimap
                                {
                                    let threshold = 15.0;
                                    let mut closest_idx = None;
                                    let mut closest_dist = f32::MAX;
                                    for (i, a) in ann.annotations.iter().enumerate() {
                                        let d = annotation_distance(a, click_pos);
                                        if d < closest_dist {
                                            closest_dist = d;
                                            closest_idx = Some(i);
                                        }
                                    }
                                    if closest_dist < threshold {
                                        ann.selected_index = closest_idx;
                                    } else {
                                        ann.selected_index = None;
                                    }
                                }
                                // Drag to move selected annotation (only if not rotating)
                                if !ann.dragging_rotation
                                    && response.dragged_by(egui::PointerButton::Primary)
                                    && let Some(sel) = ann.selected_index
                                    && sel < ann.annotations.len()
                                {
                                    let delta = response.drag_delta();
                                    // Convert screen delta to minimap delta
                                    let minimap_delta = Vec2::new(
                                        delta.x / (transform.window_scale * transform.zoom),
                                        delta.y / (transform.window_scale * transform.zoom),
                                    );
                                    match &mut ann.annotations[sel] {
                                        Annotation::Ship { pos, .. } => *pos += minimap_delta,
                                        Annotation::FreehandStroke { points, .. } => {
                                            for p in points.iter_mut() {
                                                *p += minimap_delta;
                                            }
                                        }
                                        Annotation::Line { start, end, .. } => {
                                            *start += minimap_delta;
                                            *end += minimap_delta;
                                        }
                                        Annotation::Circle { center, .. } => *center += minimap_delta,
                                        Annotation::Rectangle { center, .. } => *center += minimap_delta,
                                        Annotation::Triangle { center, .. } => *center += minimap_delta,
                                    }
                                }
                            }

                            PaintTool::PlacingShip { species, friendly, yaw } => {
                                if response.clicked()
                                    && let Some(pos) = cursor_minimap
                                {
                                    new_annotation = Some(Annotation::Ship {
                                        pos,
                                        yaw: *yaw,
                                        species: species.clone(),
                                        friendly: *friendly,
                                    });
                                }
                                ctx.request_repaint();
                            }

                            PaintTool::Freehand { current_stroke } => {
                                if response.dragged_by(egui::PointerButton::Primary)
                                    && let Some(pos) = cursor_minimap
                                {
                                    current_stroke.get_or_insert_with(Vec::new).push(pos);
                                }
                                if response.drag_stopped_by(egui::PointerButton::Primary)
                                    && let Some(points) = current_stroke.take()
                                    && points.len() >= 2
                                {
                                    new_annotation = Some(Annotation::FreehandStroke {
                                        points,
                                        color: paint_color,
                                        width: stroke_width,
                                    });
                                }
                                ctx.request_repaint();
                            }

                            PaintTool::Eraser => {
                                if response.clicked()
                                    && let Some(click_pos) = cursor_minimap
                                {
                                    let threshold = 15.0;
                                    let mut closest_idx = None;
                                    let mut closest_dist = f32::MAX;
                                    for (i, a) in ann.annotations.iter().enumerate() {
                                        let d = annotation_distance(a, click_pos);
                                        if d < closest_dist {
                                            closest_dist = d;
                                            closest_idx = Some(i);
                                        }
                                    }
                                    if closest_dist < threshold {
                                        erase_idx = closest_idx;
                                    }
                                }
                                ctx.request_repaint();
                            }

                            PaintTool::DrawingLine { start } => {
                                if response.clicked()
                                    && let Some(pos) = cursor_minimap
                                {
                                    if let Some(s) = *start {
                                        new_annotation = Some(Annotation::Line {
                                            start: s,
                                            end: pos,
                                            color: paint_color,
                                            width: stroke_width,
                                        });
                                        *start = None;
                                    } else {
                                        *start = Some(pos);
                                    }
                                }
                                ctx.request_repaint();
                            }

                            PaintTool::DrawingCircle { filled, center } => {
                                // Drag to draw: press sets one edge, release sets opposite
                                if response.drag_started_by(egui::PointerButton::Primary)
                                    && let Some(pos) = cursor_minimap
                                {
                                    *center = Some(pos);
                                }
                                if response.drag_stopped_by(egui::PointerButton::Primary)
                                    && let Some(origin) = *center
                                {
                                    if let Some(pos) = cursor_minimap {
                                        let mid = (origin + pos) / 2.0;
                                        let radius = (pos - origin).length() / 2.0;
                                        if radius > 1.0 {
                                            new_annotation = Some(Annotation::Circle {
                                                center: mid,
                                                radius,
                                                color: paint_color,
                                                width: stroke_width,
                                                filled: *filled,
                                            });
                                        }
                                    }
                                    *center = None;
                                }
                                ctx.request_repaint();
                            }

                            PaintTool::DrawingRect { filled, center } => {
                                // Drag to draw: press sets one corner, release sets opposite
                                if response.drag_started_by(egui::PointerButton::Primary)
                                    && let Some(pos) = cursor_minimap
                                {
                                    *center = Some(pos);
                                }
                                if response.drag_stopped_by(egui::PointerButton::Primary)
                                    && let Some(origin) = *center
                                {
                                    if let Some(pos) = cursor_minimap {
                                        let mid = (origin + pos) / 2.0;
                                        let half = ((pos - origin) / 2.0).abs();
                                        if half.x > 1.0 && half.y > 1.0 {
                                            new_annotation = Some(Annotation::Rectangle {
                                                center: mid,
                                                half_size: half,
                                                rotation: 0.0,
                                                color: paint_color,
                                                width: stroke_width,
                                                filled: *filled,
                                            });
                                        }
                                    }
                                    *center = None;
                                }
                                ctx.request_repaint();
                            }

                            PaintTool::DrawingTriangle { filled, center } => {
                                if response.clicked()
                                    && let Some(pos) = cursor_minimap
                                {
                                    if let Some(ctr) = *center {
                                        let radius = (pos - ctr).length();
                                        new_annotation = Some(Annotation::Triangle {
                                            center: ctr,
                                            radius,
                                            rotation: 0.0,
                                            color: paint_color,
                                            width: stroke_width,
                                            filled: *filled,
                                        });
                                        *center = None;
                                    } else {
                                        *center = Some(pos);
                                    }
                                }
                                ctx.request_repaint();
                            }
                        }

                        // Apply deferred mutations after the match (borrow of active_tool is released)
                        if new_annotation.is_some() || erase_idx.is_some() {
                            ann.save_undo();
                        }
                        if let Some(a) = new_annotation {
                            ann.annotations.push(a);
                        }
                        if let Some(idx) = erase_idx {
                            ann.annotations.remove(idx);
                        }

                        // Ctrl+Z to undo
                        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
                            ann.undo();
                        }
                    }

                    // ─── Playback keyboard shortcuts ──────────────────────────────
                    {
                        // Space: toggle play/pause
                        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                            if playing {
                                let _ = command_tx.send(PlaybackCommand::Pause);
                                shared_state.lock().playing = false;
                            } else {
                                let _ = command_tx.send(PlaybackCommand::Play);
                                shared_state.lock().playing = true;
                            }
                        }

                        // Up/Down arrows: change playback speed
                        {
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                                let current = shared_state.lock().speed;
                                if let Some(&next) = PLAYBACK_SPEEDS.iter().find(|&&s| s > current + 0.1) {
                                    let _ = command_tx.send(PlaybackCommand::SetSpeed(next));
                                    shared_state.lock().speed = next;
                                    toasts.lock().info(format!("{:.0}x", next));
                                }
                            }
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                                let current = shared_state.lock().speed;
                                if let Some(&next) = PLAYBACK_SPEEDS.iter().rev().find(|&&s| s < current - 0.1) {
                                    let _ = command_tx.send(PlaybackCommand::SetSpeed(next));
                                    shared_state.lock().speed = next;
                                    toasts.lock().info(format!("{:.0}x", next));
                                }
                            }
                        }

                        if let Some((_frame_idx, _total_frames, clock_secs, game_dur)) = frame_data {
                            // Left/Right arrows: seek +/-10 seconds
                            let shift = ctx.input(|i| i.modifiers.shift);
                            if !shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                                let target = (clock_secs - 10.0).max(0.0);
                                let _ = command_tx.send(PlaybackCommand::Seek(target));
                            }
                            if !shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                                let target = (clock_secs + 10.0).min(game_dur);
                                let _ = command_tx.send(PlaybackCommand::Seek(target));
                            }

                            // Shift+Left/Right: skip to prev/next timeline event
                            let elapsed = (clock_secs - battle_start).max(0.0);
                            if shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                                let state = shared_state.lock();
                                if let Some(ref events) = state.timeline_events {
                                    if let Some(event) = events.iter().rev().find(|e| e.clock < elapsed - 0.5) {
                                        let seek_clock = event.clock + battle_start;
                                        let desc = format_timeline_event(event);
                                        drop(state);
                                        let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                        toasts.lock().info(desc);
                                    }
                                }
                            }
                            if shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                                let state = shared_state.lock();
                                if let Some(ref events) = state.timeline_events {
                                    if let Some(event) = events.iter().find(|e| e.clock > elapsed) {
                                        let seek_clock = event.clock + battle_start;
                                        let desc = format_timeline_event(event);
                                        drop(state);
                                        let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                        toasts.lock().info(desc);
                                    }
                                }
                            }
                        }
                    }

                    // ─── Context menu (egui::Area at cursor) ─────────────────────
                    {
                        let show_menu = annotation_arc.lock().show_context_menu;
                        if show_menu {
                            let menu_pos = annotation_arc.lock().context_menu_pos;
                            let menu_resp = egui::Area::new(ui.id().with("paint_context_menu"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(menu_pos)
                                .interactable(true)
                                .show(ctx, |ui| {
                                    let frame = egui::Frame::NONE
                                        .fill(Color32::from_gray(30))
                                        .corner_radius(CornerRadius::same(6))
                                        .inner_margin(egui::Margin::same(8))
                                        .stroke(Stroke::new(1.0, Color32::from_gray(80)));
                                    frame.show(ui, |ui| {
                                        ui.set_min_width(200.0);
                                        let mut ann = annotation_arc.lock();
                                        let tex_guard = textures_arc.lock();

                                        // ── Friendly ships row ──
                                        ui.label(egui::RichText::new("Friendly Ships").color(FRIENDLY_COLOR).small());
                                        ui.horizontal(|ui| {
                                            for species in &SHIP_SPECIES {
                                                let clicked = if let Some(ref textures) = *tex_guard {
                                                    if let Some(tex) = textures.ship_icons.get(*species) {
                                                        let img = egui::Image::new(egui::load::SizedTexture::new(
                                                            tex.id(),
                                                            egui::vec2(24.0, 24.0),
                                                        ))
                                                        .rotate(std::f32::consts::FRAC_PI_2, egui::vec2(0.5, 0.5))
                                                        .tint(FRIENDLY_COLOR);
                                                        ui.add(egui::Button::image(img))
                                                            .on_hover_text(format!(
                                                                "Friendly {}",
                                                                ship_short_name(species)
                                                            ))
                                                            .clicked()
                                                    } else {
                                                        ui.button(
                                                            egui::RichText::new(ship_short_name(species))
                                                                .color(FRIENDLY_COLOR),
                                                        )
                                                        .clicked()
                                                    }
                                                } else {
                                                    ui.button(
                                                        egui::RichText::new(ship_short_name(species))
                                                            .color(FRIENDLY_COLOR),
                                                    )
                                                    .clicked()
                                                };
                                                if clicked {
                                                    ann.active_tool = PaintTool::PlacingShip {
                                                        species: species.to_string(),
                                                        friendly: true,
                                                        yaw: 0.0,
                                                    };
                                                    ann.show_context_menu = false;
                                                }
                                            }
                                        });

                                        // ── Enemy ships row ──
                                        ui.label(egui::RichText::new("Enemy Ships").color(ENEMY_COLOR).small());
                                        ui.horizontal(|ui| {
                                            for species in &SHIP_SPECIES {
                                                let clicked = if let Some(ref textures) = *tex_guard {
                                                    if let Some(tex) = textures.ship_icons.get(*species) {
                                                        let img = egui::Image::new(egui::load::SizedTexture::new(
                                                            tex.id(),
                                                            egui::vec2(24.0, 24.0),
                                                        ))
                                                        .rotate(std::f32::consts::FRAC_PI_2, egui::vec2(0.5, 0.5))
                                                        .tint(ENEMY_COLOR);
                                                        ui.add(egui::Button::image(img))
                                                            .on_hover_text(format!(
                                                                "Enemy {}",
                                                                ship_short_name(species)
                                                            ))
                                                            .clicked()
                                                    } else {
                                                        ui.button(
                                                            egui::RichText::new(ship_short_name(species))
                                                                .color(ENEMY_COLOR),
                                                        )
                                                        .clicked()
                                                    }
                                                } else {
                                                    ui.button(
                                                        egui::RichText::new(ship_short_name(species))
                                                            .color(ENEMY_COLOR),
                                                    )
                                                    .clicked()
                                                };
                                                if clicked {
                                                    ann.active_tool = PaintTool::PlacingShip {
                                                        species: species.to_string(),
                                                        friendly: false,
                                                        yaw: 0.0,
                                                    };
                                                    ann.show_context_menu = false;
                                                }
                                            }
                                        });
                                        drop(tex_guard);

                                        ui.separator();

                                        // ── Drawing tools row ──
                                        ui.label(egui::RichText::new("Drawing Tools").small());
                                        ui.horizontal(|ui| {
                                            if ui.button(icons::PAINT_BRUSH).on_hover_text("Freehand").clicked() {
                                                ann.active_tool = PaintTool::Freehand { current_stroke: None };
                                                ann.show_context_menu = false;
                                            }
                                            if ui.button(icons::ERASER).on_hover_text("Eraser").clicked() {
                                                ann.active_tool = PaintTool::Eraser;
                                                ann.show_context_menu = false;
                                            }
                                            if ui.button(icons::LINE_SEGMENT).on_hover_text("Line").clicked() {
                                                ann.active_tool = PaintTool::DrawingLine { start: None };
                                                ann.show_context_menu = false;
                                            }
                                            if ui.button(icons::CIRCLE).on_hover_text("Circle").clicked() {
                                                ann.active_tool =
                                                    PaintTool::DrawingCircle { filled: false, center: None };
                                                ann.show_context_menu = false;
                                            }
                                            if ui.button(icons::SQUARE).on_hover_text("Rectangle").clicked() {
                                                ann.active_tool =
                                                    PaintTool::DrawingRect { filled: false, center: None };
                                                ann.show_context_menu = false;
                                            }
                                            if ui.button(icons::TRIANGLE).on_hover_text("Triangle").clicked() {
                                                ann.active_tool =
                                                    PaintTool::DrawingTriangle { filled: false, center: None };
                                                ann.show_context_menu = false;
                                            }
                                        });

                                        ui.separator();

                                        // ── Color presets + custom picker + stroke width ──
                                        const PRESET_COLORS: &[(Color32, &str)] = &[
                                            (Color32::WHITE, "White"),
                                            (Color32::from_rgb(160, 160, 160), "Gray"),
                                            (Color32::from_rgb(230, 50, 50), "Red"),
                                            (Color32::from_rgb(240, 140, 30), "Orange"),
                                            (Color32::from_rgb(240, 230, 50), "Yellow"),
                                            (Color32::from_rgb(50, 200, 50), "Green"),
                                            (Color32::from_rgb(50, 120, 230), "Blue"),
                                            (Color32::from_rgb(180, 60, 230), "Purple"),
                                            (Color32::from_rgb(255, 130, 180), "Pink"),
                                        ];
                                        ui.horizontal(|ui| {
                                            let swatch_size = egui::vec2(16.0, 16.0);

                                            // Custom color picker first, with white outline
                                            egui::color_picker::color_edit_button_srgba(
                                                ui,
                                                &mut ann.paint_color,
                                                egui::color_picker::Alpha::Opaque,
                                            );
                                            let picker_rect = ui.min_rect();
                                            ui.painter().rect_stroke(
                                                picker_rect,
                                                CornerRadius::same(2),
                                                Stroke::new(1.5, Color32::WHITE),
                                                egui::StrokeKind::Outside,
                                            );

                                            ui.add_space(4.0);

                                            // Preset color swatches
                                            for &(color, name) in PRESET_COLORS {
                                                let selected = ann.paint_color == color;
                                                let (rect, resp) =
                                                    ui.allocate_exact_size(swatch_size, egui::Sense::click());
                                                ui.painter().rect_filled(rect, CornerRadius::same(3), color);
                                                if selected {
                                                    ui.painter().rect_stroke(
                                                        rect,
                                                        CornerRadius::same(3),
                                                        Stroke::new(2.0, Color32::WHITE),
                                                        egui::StrokeKind::Outside,
                                                    );
                                                }
                                                if resp.on_hover_text(name).clicked() {
                                                    ann.paint_color = color;
                                                }
                                            }
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Width:");
                                            ui.add(egui::Slider::new(&mut ann.stroke_width, 1.0..=8.0).max_decimals(1));
                                        });

                                        // ── Ship-specific options (shown when right-clicking a ship) ──
                                        if let Some(ref ship_name) = ann.context_menu_ship.clone() {
                                            ui.separator();
                                            ui.label(egui::RichText::new(ship_name.as_str()).small());

                                            // Per-ship trail toggle
                                            let mut show_trail = !ann.trail_hidden_ships.contains(ship_name);
                                            if ui.checkbox(&mut show_trail, "Show Trail").changed() {
                                                if show_trail {
                                                    ann.trail_hidden_ships.remove(ship_name);
                                                    // If global trails are off, turn them on
                                                    if !shared_state.lock().options.show_trails {
                                                        shared_state.lock().options.show_trails = true;
                                                    }
                                                } else {
                                                    ann.trail_hidden_ships.insert(ship_name.clone());
                                                    // If all trails are now hidden, turn off global
                                                    let state = shared_state.lock();
                                                    if let Some(ref frame) = state.frame {
                                                        let all_hidden = frame.commands.iter().all(|cmd| {
                                                            if let DrawCommand::PositionTrail {
                                                                player_name: Some(name),
                                                                ..
                                                            } = cmd
                                                            {
                                                                ann.trail_hidden_ships.contains(name)
                                                            } else {
                                                                true
                                                            }
                                                        });
                                                        if all_hidden && state.options.show_trails {
                                                            drop(state);
                                                            shared_state.lock().options.show_trails = false;
                                                        }
                                                    }
                                                }
                                                ctx.request_repaint();
                                            }

                                            // Disable all other trails
                                            if ui.button("Disable All Other Trails").clicked() {
                                                let state = shared_state.lock();
                                                if let Some(ref frame) = state.frame {
                                                    for cmd in &frame.commands {
                                                        if let DrawCommand::PositionTrail {
                                                            player_name: Some(name),
                                                            ..
                                                        } = cmd
                                                        {
                                                            if name != ship_name {
                                                                ann.trail_hidden_ships.insert(name.clone());
                                                            }
                                                        }
                                                    }
                                                }
                                                ann.trail_hidden_ships.remove(ship_name);
                                                if !state.options.show_trails {
                                                    drop(state);
                                                    shared_state.lock().options.show_trails = true;
                                                }
                                                ann.show_context_menu = false;
                                                ctx.request_repaint();
                                            }

                                            // Per-type range toggles for this ship
                                            let mut flags =
                                                ann.ship_range_overrides.get(ship_name).copied().unwrap_or([false; 5]);
                                            ui.label(egui::RichText::new("Ranges").small());
                                            let mut range_changed = false;
                                            range_changed |= ui.checkbox(&mut flags[0], "Detection").changed();
                                            range_changed |= ui.checkbox(&mut flags[1], "Main Battery").changed();
                                            range_changed |= ui.checkbox(&mut flags[2], "Secondary").changed();
                                            range_changed |= ui.checkbox(&mut flags[3], "Radar").changed();
                                            range_changed |= ui.checkbox(&mut flags[4], "Hydro").changed();
                                            let any_on = flags.iter().any(|&f| f);
                                            let all_on = flags == [true; 5];
                                            if !all_on && ui.button("Enable All").clicked() {
                                                flags = [true; 5];
                                                range_changed = true;
                                            } else if all_on && ui.button("Disable All").clicked() {
                                                flags = [false; 5];
                                                range_changed = true;
                                            }
                                            if range_changed {
                                                if flags == [false; 5] {
                                                    ann.ship_range_overrides.remove(ship_name);
                                                } else {
                                                    ann.ship_range_overrides.insert(ship_name.clone(), flags);
                                                }
                                                // Auto-enable global when turning on any range
                                                if flags.iter().any(|&f| f) {
                                                    let mut state = shared_state.lock();
                                                    if !state.options.show_ship_config {
                                                        state.options.show_ship_config = true;
                                                    }
                                                }
                                                // Auto-disable global when no ship has any range enabled
                                                if ann.ship_range_overrides.is_empty() {
                                                    let mut state = shared_state.lock();
                                                    if state.options.show_ship_config {
                                                        state.options.show_ship_config = false;
                                                    }
                                                }
                                                ctx.request_repaint();
                                            }

                                            // Disable all other ships' ranges
                                            if any_on && ui.button("Disable All Other Ranges").clicked() {
                                                let keys: Vec<String> = ann
                                                    .ship_range_overrides
                                                    .keys()
                                                    .filter(|k| k.as_str() != ship_name)
                                                    .cloned()
                                                    .collect();
                                                for k in keys {
                                                    ann.ship_range_overrides.remove(&k);
                                                }
                                                ann.show_context_menu = false;
                                                ctx.request_repaint();
                                            }

                                            // Enable ranges for all alive ships
                                            if ui.button("Enable All Ships' Ranges").clicked() {
                                                let state = shared_state.lock();
                                                if let Some(ref frame) = state.frame {
                                                    for cmd in &frame.commands {
                                                        if let DrawCommand::Ship { player_name: Some(name), .. } = cmd {
                                                            ann.ship_range_overrides
                                                                .entry(name.clone())
                                                                .or_insert([true; 5]);
                                                            // Set all flags to true for ships that already have entries
                                                            if let Some(f) = ann.ship_range_overrides.get_mut(name) {
                                                                *f = [true; 5];
                                                            }
                                                        }
                                                    }
                                                }
                                                if !state.options.show_ship_config {
                                                    drop(state);
                                                    shared_state.lock().options.show_ship_config = true;
                                                }
                                                ann.show_context_menu = false;
                                                ctx.request_repaint();
                                            }
                                        }
                                        // ── Clear all ──
                                        if !ann.annotations.is_empty() {
                                            ui.separator();
                                            if ui
                                                .button(
                                                    egui::RichText::new(icon_str!(icons::TRASH, "Clear All"))
                                                        .color(Color32::from_rgb(255, 100, 100)),
                                                )
                                                .clicked()
                                            {
                                                ann.save_undo();
                                                ann.annotations.clear();
                                                ann.show_context_menu = false;
                                            }
                                        }
                                    });
                                });

                            // Close menu on click outside (but not if a sub-popup like color picker is open)
                            let menu_rect = menu_resp.response.rect;
                            let any_popup = ctx.is_popup_open();
                            let clicked_outside = !any_popup
                                && ctx.input(|i| {
                                    i.pointer.any_click()
                                        && i.pointer.interact_pos().is_some_and(|p| !menu_rect.contains(p))
                                });
                            if clicked_outside {
                                annotation_arc.lock().show_context_menu = false;
                            }
                        }
                    }

                    // ─── Selection edit popup ─────────────────────────────────────
                    {
                        let ann = annotation_arc.lock();
                        let sel_info = ann.selected_index.and_then(|idx| {
                            if idx < ann.annotations.len() {
                                let bounds = annotation_screen_bounds(&ann.annotations[idx], &transform);
                                Some((idx, bounds))
                            } else {
                                None
                            }
                        });
                        drop(ann);

                        if let Some((sel_idx, bounds)) = sel_info {
                            // Position popup to the right of the annotation, or below if near edge
                            let popup_pos = Pos2::new(bounds.right() + 8.0, bounds.center().y);
                            egui::Area::new(ui.id().with("annotation_edit_popup"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(popup_pos)
                                .interactable(true)
                                .show(ctx, |ui| {
                                    let frame = egui::Frame::NONE
                                        .fill(Color32::from_gray(30))
                                        .corner_radius(CornerRadius::same(6))
                                        .inner_margin(egui::Margin::same(6))
                                        .stroke(Stroke::new(1.0, Color32::from_gray(80)));
                                    frame.show(ui, |ui| {
                                        let mut ann = annotation_arc.lock();
                                        if sel_idx >= ann.annotations.len() {
                                            return;
                                        }

                                        // Size slider (for circle, rect, triangle)
                                        let has_size = matches!(
                                            ann.annotations[sel_idx],
                                            Annotation::Circle { .. }
                                                | Annotation::Rectangle { .. }
                                                | Annotation::Triangle { .. }
                                        );
                                        if has_size {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("Size").small());
                                                let mut size = match &ann.annotations[sel_idx] {
                                                    Annotation::Circle { radius, .. } => *radius,
                                                    Annotation::Rectangle { half_size, .. } => {
                                                        (half_size.x + half_size.y) / 2.0
                                                    }
                                                    Annotation::Triangle { radius, .. } => *radius,
                                                    _ => 0.0,
                                                };
                                                let old = size;
                                                ui.add(egui::DragValue::new(&mut size).speed(1.0).range(5.0..=500.0));
                                                if size != old && size > 0.0 {
                                                    match &mut ann.annotations[sel_idx] {
                                                        Annotation::Circle { radius, .. } => *radius = size,
                                                        Annotation::Rectangle { half_size, .. } => {
                                                            let ratio = if old > 0.0 { size / old } else { 1.0 };
                                                            *half_size *= ratio;
                                                        }
                                                        Annotation::Triangle { radius, .. } => *radius = size,
                                                        _ => {}
                                                    }
                                                }
                                            });
                                        }

                                        // Color picker (for non-ship annotations)
                                        let is_ship = matches!(ann.annotations[sel_idx], Annotation::Ship { .. });
                                        if !is_ship {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("Color").small());
                                                let color_ref = match &mut ann.annotations[sel_idx] {
                                                    Annotation::FreehandStroke { color, .. } => color,
                                                    Annotation::Line { color, .. } => color,
                                                    Annotation::Circle { color, .. } => color,
                                                    Annotation::Rectangle { color, .. } => color,
                                                    Annotation::Triangle { color, .. } => color,
                                                    _ => unreachable!(),
                                                };
                                                egui::color_picker::color_edit_button_srgba(
                                                    ui,
                                                    color_ref,
                                                    egui::color_picker::Alpha::Opaque,
                                                );
                                            });
                                        }

                                        // Filled toggle (for circle, rect, triangle)
                                        let has_filled = matches!(
                                            ann.annotations[sel_idx],
                                            Annotation::Circle { .. }
                                                | Annotation::Rectangle { .. }
                                                | Annotation::Triangle { .. }
                                        );
                                        if has_filled {
                                            let filled_ref = match &mut ann.annotations[sel_idx] {
                                                Annotation::Circle { filled, .. } => filled,
                                                Annotation::Rectangle { filled, .. } => filled,
                                                Annotation::Triangle { filled, .. } => filled,
                                                _ => unreachable!(),
                                            };
                                            ui.checkbox(filled_ref, egui::RichText::new("Filled").small());
                                        }

                                        // Team toggle (for ships)
                                        if is_ship
                                            && let Annotation::Ship { friendly, .. } = &mut ann.annotations[sel_idx]
                                        {
                                            let (label, color) = if *friendly {
                                                ("Friendly", FRIENDLY_COLOR)
                                            } else {
                                                ("Enemy  ", ENEMY_COLOR)
                                            };
                                            let btn =
                                                egui::Button::new(egui::RichText::new(label).color(color).small())
                                                    .min_size(egui::vec2(60.0, 0.0));
                                            if ui.add(btn).clicked() {
                                                *friendly = !*friendly;
                                            }
                                        }

                                        // Delete button
                                        if ui
                                            .button(
                                                egui::RichText::new(icons::TRASH)
                                                    .color(Color32::from_rgb(255, 100, 100)),
                                            )
                                            .on_hover_text("Delete")
                                            .clicked()
                                        {
                                            ann.save_undo();
                                            ann.annotations.remove(sel_idx);
                                            ann.selected_index = None;
                                        }
                                    });
                                });
                        }
                    }

                    // ─── Overlay controls (video-player style) ───────────────────

                    // Track mouse activity for fade
                    let now = ctx.input(|i| i.time);
                    let any_mouse_activity =
                        ctx.input(|i| i.pointer.velocity().length() > 0.5 || i.pointer.any_click());
                    {
                        let mut ov = overlay_state_arc.lock();
                        if any_mouse_activity {
                            ov.last_activity = now;
                        }
                    }

                    // Check if any popup is open (keeps overlay visible, e.g. settings or speed)
                    let any_popup_open = egui::Popup::is_any_open(ctx);

                    // Compute overlay opacity
                    let elapsed = now - overlay_state_arc.lock().last_activity;
                    let overlay_rect = ctx.memory(|mem| mem.area_rect(ui.id().with("controls_overlay")));
                    let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                    let overlay_hovered = overlay_rect.is_some_and(|r| hover_pos.is_some_and(|p| r.contains(p)));
                    let opacity = if overlay_hovered || any_popup_open || elapsed < 2.0 {
                        1.0_f32
                    } else if elapsed < 2.5 {
                        (1.0 - ((elapsed - 2.0) / 0.5) as f32).max(0.0)
                    } else {
                        0.0
                    };

                    // Request repaint during fade animation
                    if opacity > 0.0 && opacity < 1.0 {
                        ctx.request_repaint();
                    }

                    // Only render overlay when visible (so it doesn't block canvas input when hidden)
                    if opacity > 0.0 {
                        let bg_alpha = (180.0 * opacity) as u8;
                        let text_alpha = (255.0 * opacity) as u8;

                        egui::Area::new(ui.id().with("controls_overlay"))
                            .order(egui::Order::Foreground)
                            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
                            .interactable(true)
                            .show(ctx, |ui| {
                                // Apply faded text color
                                ui.visuals_mut().override_text_color =
                                    Some(Color32::from_rgba_unmultiplied(255, 255, 255, text_alpha));
                                ui.visuals_mut().widgets.inactive.bg_fill =
                                    Color32::from_rgba_unmultiplied(60, 60, 60, bg_alpha);
                                ui.visuals_mut().widgets.hovered.bg_fill =
                                    Color32::from_rgba_unmultiplied(80, 80, 80, bg_alpha);

                                let frame = egui::Frame::NONE
                                    .fill(Color32::from_black_alpha(bg_alpha))
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(egui::Margin::same(8));
                                frame.show(ui, |ui| {
                                    let overlay_width = (response.rect.width() - 32.0).max(200.0);
                                    ui.set_width(overlay_width);

                                    // Prevent egui text layout from wrapping to minimal width
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                                    let row_style = taffy::Style {
                                        display: taffy::Display::Flex,
                                        flex_direction: taffy::FlexDirection::Row,
                                        align_items: Some(taffy::AlignItems::Center),
                                        gap: length(4.0),
                                        size: taffy::Size { width: taffy::prelude::percent(1.0), height: auto() },
                                        ..Default::default()
                                    };
                                    let grow_style = taffy::Style {
                                        flex_grow: 1.0,
                                        flex_shrink: 1.0,
                                        min_size: taffy::Size { width: length(60.0), height: auto() },
                                        ..Default::default()
                                    };
                                    let fixed_style = taffy::Style { flex_shrink: 0.0, ..Default::default() };

                                    let mut settings_btn_opt: Option<egui::Response> = None;
                                    let mut timeline_btn_opt: Option<egui::Response> = None;

                                    egui_taffy::tui(ui, ui.id().with("overlay_tui"))
                                        .reserve_available_width()
                                        .style(row_style)
                                        .show(|tui| {
                                            // Jump to start
                                            {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::SKIP_BACK));
                                                if btn.on_hover_text("Jump to start").clicked() {
                                                    let _ = command_tx.send(PlaybackCommand::Seek(0.0));
                                                }
                                            }

                                            // Skip to previous event
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let elapsed = (clock_secs - battle_start).max(0.0);
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::REWIND));
                                                if btn.on_hover_text("Previous event (Shift+Left)").clicked() {
                                                    let state = shared_state.lock();
                                                    if let Some(ref events) = state.timeline_events {
                                                        if let Some(event) =
                                                            events.iter().rev().find(|e| e.clock < elapsed - 0.5)
                                                        {
                                                            let seek_clock = event.clock + battle_start;
                                                            let desc = format_timeline_event(event);
                                                            drop(state);
                                                            let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                                            toasts.lock().info(desc);
                                                        }
                                                    }
                                                }
                                            }

                                            // Back 10 seconds
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::CLOCK_COUNTER_CLOCKWISE));
                                                if btn.on_hover_text("Back 10s (Left)").clicked() {
                                                    let target = (clock_secs - 10.0).max(0.0);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(target));
                                                }
                                            }

                                            // Play/Pause
                                            if playing {
                                                if tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::PAUSE))
                                                    .on_hover_text("Pause (Space)")
                                                    .clicked()
                                                {
                                                    let _ = command_tx.send(PlaybackCommand::Pause);
                                                    shared_state.lock().playing = false;
                                                }
                                            } else if tui
                                                .tui()
                                                .style(fixed_style.clone())
                                                .ui_add(egui::Button::new(icons::PLAY))
                                                .on_hover_text("Play (Space)")
                                                .clicked()
                                            {
                                                let _ = command_tx.send(PlaybackCommand::Play);
                                                shared_state.lock().playing = true;
                                            }

                                            // Forward 10 seconds
                                            if let Some((_fi, _tf, clock_secs, game_dur)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::CLOCK_CLOCKWISE));
                                                if btn.on_hover_text("Forward 10s (Right)").clicked() {
                                                    let target = (clock_secs + 10.0).min(game_dur);
                                                    let _ = command_tx.send(PlaybackCommand::Seek(target));
                                                }
                                            }

                                            // Skip to next event
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::FAST_FORWARD));
                                                if btn.on_hover_text("Next event (Shift+Right)").clicked() {
                                                    let elapsed = (clock_secs - battle_start).max(0.0);
                                                    let state = shared_state.lock();
                                                    if let Some(ref events) = state.timeline_events {
                                                        if let Some(event) =
                                                            events.iter().find(|e| e.clock > elapsed)
                                                        {
                                                            let seek_clock = event.clock + battle_start;
                                                            let desc = format_timeline_event(event);
                                                            drop(state);
                                                            let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                                            toasts.lock().info(desc);
                                                        }
                                                    }
                                                }
                                            }

                                            // Jump to end
                                            if let Some((_fi, _tf, _clock_secs, game_dur)) = frame_data {
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::SKIP_FORWARD));
                                                if btn.on_hover_text("Jump to end").clicked() {
                                                    let _ = command_tx.send(PlaybackCommand::Seek(game_dur));
                                                }
                                            }

                                            // Seek slider (flex_grow: 1.0 — fills remaining space)
                                            if let Some((_frame_idx, _total_frames, clock_secs, game_dur)) = frame_data
                                            {
                                                let mut seek_time = clock_secs;
                                                let mut seek_changed = false;
                                                tui.tui().style(grow_style.clone()).ui(|ui| {
                                                    ui.spacing_mut().slider_width = ui.available_width();
                                                    let slider = egui::Slider::new(&mut seek_time, 0.0..=game_dur)
                                                        .show_value(false);
                                                    seek_changed = ui.add(slider).changed();
                                                });
                                                if seek_changed {
                                                    let _ = command_tx.send(PlaybackCommand::Seek(seek_time));
                                                }

                                                let elapsed_secs = (clock_secs - battle_start).max(0.0) as u32;
                                                let mins = elapsed_secs / 60;
                                                let secs = elapsed_secs % 60;
                                                tui.tui()
                                                    .style(fixed_style.clone())
                                                    .label(format!("{:02}:{:02}", mins, secs));
                                            }

                                            // Speed selector
                                            let mut current_speed = speed;
                                            tui.tui().style(fixed_style.clone()).ui(|ui| {
                                                egui::ComboBox::from_id_salt("overlay_speed")
                                                    .selected_text(format!("{:.0}x", current_speed))
                                                    .width(60.0)
                                                    .show_ui(ui, |ui| {
                                                        for s in PLAYBACK_SPEEDS {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut current_speed,
                                                                    s,
                                                                    format!("{:.0}x", s),
                                                                )
                                                                .changed()
                                                            {
                                                                let _ = command_tx.send(PlaybackCommand::SetSpeed(s));
                                                                shared_state.lock().speed = s;
                                                            }
                                                        }
                                                    });
                                            });

                                            tui.tui()
                                                .style(fixed_style.clone())
                                                .ui_add(egui_taffy::widgets::TaffySeparator::default());

                                            // Settings button
                                            let btn = tui.tui().style(fixed_style.clone()).ui_add(egui::Button::new(
                                                egui::RichText::new(icons::GEAR_FINE).size(18.0),
                                            ));
                                            settings_btn_opt = Some(btn);

                                            // Timeline button
                                            let btn = tui.tui().style(fixed_style.clone()).ui_add(egui::Button::new(
                                                egui::RichText::new(icons::LIST_BULLETS).size(18.0),
                                            ));
                                            timeline_btn_opt = Some(btn);

                                            // Save as Video button
                                            {
                                                let is_exporting = video_exporting.load(Ordering::Relaxed);
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .enabled_ui(!is_exporting)
                                                    .ui_add(egui::Button::new(
                                                        egui::RichText::new(icons::FLOPPY_DISK).size(18.0),
                                                    ));
                                                if btn.on_hover_text("Save as Video").clicked() {
                                                    let opts = options.clone();
                                                    let default_name = format!("{}.mp4", video_export_data.replay_name);
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_file_name(&default_name)
                                                        .add_filter("MP4 Video", &["mp4"])
                                                        .save_file()
                                                    {
                                                        save_as_video(
                                                            path.to_string_lossy().to_string(),
                                                            video_export_data.raw_meta.clone(),
                                                            video_export_data.packet_data.clone(),
                                                            video_export_data.map_name.clone(),
                                                            video_export_data.game_duration,
                                                            opts,
                                                            video_export_data.wows_data.clone(),
                                                            Arc::clone(&video_export_data.asset_cache),
                                                            Arc::clone(&toasts),
                                                            Arc::clone(&video_exporting),
                                                        );
                                                    }
                                                }
                                            }

                                            // Render Video to Clipboard button
                                            {
                                                let is_exporting = video_exporting.load(Ordering::Relaxed);
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .enabled_ui(!is_exporting)
                                                    .ui_add(egui::Button::new(
                                                        egui::RichText::new(icons::CLIPBOARD).size(18.0),
                                                    ));
                                                if btn.on_hover_text("Render Video to Clipboard").clicked() {
                                                    let opts = options.clone();
                                                    let file_name = format!("{}.mp4", video_export_data.replay_name);
                                                    let export_data = Arc::clone(&video_export_data);
                                                    let toasts2 = Arc::clone(&toasts);
                                                    let exporting = Arc::clone(&video_exporting);
                                                    render_video_to_clipboard(
                                                        file_name,
                                                        export_data,
                                                        opts,
                                                        toasts2,
                                                        exporting,
                                                    );
                                                }
                                            }

                                            tui.tui()
                                                .style(fixed_style.clone())
                                                .ui_add(egui_taffy::widgets::TaffySeparator::default());

                                            // Zoom slider
                                            {
                                                tui.tui().style(fixed_style.clone()).label(icons::MAGNIFYING_GLASS);

                                                let mut zp = zoom_pan_arc.lock();
                                                let mut zoom_val = zp.zoom;
                                                let slider = egui::Slider::new(&mut zoom_val, 1.0..=10.0_f32)
                                                    .logarithmic(true)
                                                    .max_decimals(1)
                                                    .suffix("x");
                                                if tui.tui().style(fixed_style.clone()).ui_add(slider).changed() {
                                                    let old_zoom = zp.zoom;
                                                    let center_x = zp.pan.x + MINIMAP_SIZE as f32 / 2.0;
                                                    let center_y = zp.pan.y + MINIMAP_SIZE as f32 / 2.0;
                                                    let minimap_cx = center_x / old_zoom;
                                                    let minimap_cy = center_y / old_zoom;
                                                    zp.pan.x = minimap_cx * zoom_val - MINIMAP_SIZE as f32 / 2.0;
                                                    zp.pan.y = minimap_cy * zoom_val - MINIMAP_SIZE as f32 / 2.0;
                                                    zp.zoom = zoom_val;
                                                    zp.pan.x = zp.pan.x.max(0.0);
                                                    zp.pan.y = zp.pan.y.max(0.0);
                                                }
                                                if tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new("Reset"))
                                                    .clicked()
                                                {
                                                    zp.zoom = 1.0;
                                                    zp.pan = Vec2::ZERO;
                                                }
                                            }
                                        });

                                    let settings_btn = settings_btn_opt.unwrap();

                                    // Settings popup (inside frame, outside horizontal)
                                    egui::Popup::from_toggle_button_response(&settings_btn)
                                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                        .show(|ui| {
                                            ui.set_min_width(180.0);
                                            let mut opts = options.clone();
                                            let mut show_dead = show_dead_ships;
                                            let mut changed = false;

                                            // ── Ship Settings ──
                                            ui.label(egui::RichText::new("Ship Settings").small().strong());
                                            ui.indent("ship_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_armament, "Armament").changed();
                                                changed |= ui.checkbox(&mut show_dead, "Dead Ships").changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_dead_ship_names, "Dead Ship Names")
                                                    .changed();
                                                changed |= ui.checkbox(&mut opts.show_hp_bars, "HP Bars").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_player_names, "Player Names").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_ship_config, "Ship Ranges").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_ship_names, "Ship Names").changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_turret_direction, "Turret Direction")
                                                    .changed();
                                            });

                                            // ── Trail Settings ──
                                            ui.label(egui::RichText::new("Trail Settings").small().strong());
                                            ui.indent("trail_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_trails, "Trails").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_speed_trails, "Speed Trails").changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_dead_trails, "Dead Ship Trails")
                                                    .changed();
                                            });

                                            // ── Map Settings ──
                                            ui.label(egui::RichText::new("Map Settings").small().strong());
                                            ui.indent("map_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_buildings, "Buildings").changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_capture_points, "Capture Points")
                                                    .changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_consumables, "Consumables").changed();
                                                changed |= ui.checkbox(&mut opts.show_planes, "Planes").changed();
                                                changed |= ui.checkbox(&mut opts.show_smoke, "Smoke").changed();
                                                changed |= ui.checkbox(&mut opts.show_torpedoes, "Torpedoes").changed();
                                                changed |= ui.checkbox(&mut opts.show_tracers, "Tracers").changed();
                                            });

                                            // ── HUD Settings ──
                                            ui.label(egui::RichText::new("HUD Settings").small().strong());
                                            ui.indent("hud_settings", |ui| {
                                                changed |= ui
                                                    .checkbox(&mut opts.show_battle_result, "Battle Result")
                                                    .changed();
                                                changed |= ui.checkbox(&mut opts.show_buffs, "Buff Counters").changed();
                                                changed |= ui.checkbox(&mut opts.show_chat, "Chat").changed();
                                                changed |= ui.checkbox(&mut opts.show_kill_feed, "Kill Feed").changed();
                                                changed |= ui.checkbox(&mut opts.show_score, "Score").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_score_timer, "Score Timers").changed();
                                                changed |=
                                                    ui.checkbox(&mut opts.show_advantage, "Team Advantage").changed();
                                                changed |= ui.checkbox(&mut opts.show_timer, "Timer").changed();
                                            });

                                            if changed {
                                                let mut state = shared_state.lock();
                                                state.options = opts.clone();
                                                state.show_dead_ships = show_dead;
                                            }

                                            ui.separator();
                                            if ui.button("Save Defaults").clicked() {
                                                let mut saved = saved_from_render_options(&opts);
                                                saved.show_dead_ships = show_dead;
                                                *pending_save.lock() = Some(saved);
                                            }
                                        });

                                    // Timeline popup
                                    let timeline_btn = timeline_btn_opt.unwrap();
                                    egui::Popup::from_toggle_button_response(&timeline_btn)
                                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                        .frame(
                                            egui::Frame::popup(ui.style())
                                                .fill(ui.style().visuals.window_fill.gamma_multiply(0.5)),
                                        )
                                        .show(|ui| {
                                            ui.set_width(280.0);
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("Event Timeline").strong());
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Center),
                                                    |ui| {
                                                        let state = shared_state.lock();
                                                        if let Some(events) = &state.timeline_events {
                                                            if ui.small_button("Copy").clicked() {
                                                                let text: String = events
                                                                    .iter()
                                                                    .map(|e| format_timeline_event(e))
                                                                    .collect::<Vec<_>>()
                                                                    .join("\n");
                                                                ui.ctx().copy_text(text);
                                                            }
                                                        }
                                                    },
                                                );
                                            });
                                            ui.separator();

                                            let state = shared_state.lock();
                                            if let Some(events) = &state.timeline_events {
                                                if events.is_empty() {
                                                    ui.label("No significant events detected.");
                                                } else {
                                                    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                                                        ui.set_width(ui.available_width());
                                                        for event in events {
                                                            let mins = event.clock as u32 / 60;
                                                            let secs = event.clock as u32 % 60;
                                                            let timestamp = format!("{:02}:{:02}", mins, secs);

                                                            let row = ui.horizontal(|ui| {
                                                                let mut clicked = ui.small_button(&timestamp).clicked();

                                                                let (color, text, hover) = match &event.kind {
                                                                    TimelineEventKind::HealthLost {
                                                                        ship_name, player_name, is_friendly,
                                                                        percent_lost, old_hp, new_hp, max_hp,
                                                                    } => {
                                                                        let pct = (percent_lost * 100.0) as u32;
                                                                        (
                                                                            event_color(*is_friendly),
                                                                            format!("{} -{}% HP", ship_name, pct),
                                                                            format!("{} ({})\n{:.0}/{:.0} -> {:.0}/{:.0} HP",
                                                                                ship_name, player_name, old_hp, max_hp, new_hp, max_hp),
                                                                        )
                                                                    }
                                                                    TimelineEventKind::Death {
                                                                        ship_name, player_name, is_friendly,
                                                                        killer_ship, killer_player,
                                                                    } => {
                                                                        let hover = if killer_ship.is_empty() {
                                                                            format!("{} ({})", ship_name, player_name)
                                                                        } else {
                                                                            format!("{} ({})\nKilled by {} ({})",
                                                                                ship_name, player_name, killer_ship, killer_player)
                                                                        };
                                                                        (
                                                                            event_color(*is_friendly),
                                                                            format!("{} destroyed", ship_name),
                                                                            hover,
                                                                        )
                                                                    }
                                                                    TimelineEventKind::CapContested {
                                                                        cap_label, owner_is_friendly,
                                                                    } => (
                                                                        event_color(*owner_is_friendly),
                                                                        format!("{} contested", cap_label),
                                                                        String::new(),
                                                                    ),
                                                                    TimelineEventKind::CapFlipped {
                                                                        cap_label, capturer_is_friendly,
                                                                    } => (
                                                                        event_color(*capturer_is_friendly),
                                                                        format!("{} captured", cap_label),
                                                                        String::new(),
                                                                    ),
                                                                    TimelineEventKind::CapBeingCaptured {
                                                                        cap_label, capturer_is_friendly,
                                                                    } => (
                                                                        event_color(*capturer_is_friendly),
                                                                        format!("{} being captured", cap_label),
                                                                        String::new(),
                                                                    ),
                                                                    TimelineEventKind::RadarUsed {
                                                                        ship_name, player_name, is_friendly,
                                                                    } => (
                                                                        event_color(*is_friendly),
                                                                        format!("{} used radar", ship_name),
                                                                        format!("{} ({})", ship_name, player_name),
                                                                    ),
                                                                    TimelineEventKind::AdvantageChanged {
                                                                        label, is_friendly,
                                                                    } => (event_color(*is_friendly), label.clone(), String::new()),
                                                                    TimelineEventKind::Disconnected {
                                                                        ship_name, player_name, is_friendly,
                                                                    } => (
                                                                        event_color(*is_friendly),
                                                                        format!("{} disconnected", ship_name),
                                                                        format!("{} ({})", ship_name, player_name),
                                                                    ),
                                                                };

                                                                let label_resp = ui.add(
                                                                    egui::Label::new(
                                                                        egui::RichText::new(text).color(color),
                                                                    )
                                                                    .selectable(false)
                                                                    .sense(egui::Sense::click()),
                                                                );
                                                                if !hover.is_empty() {
                                                                    label_resp.clone().on_hover_text(&hover);
                                                                }
                                                                if label_resp.hovered() {
                                                                    ui.ctx().set_cursor_icon(
                                                                        egui::CursorIcon::PointingHand,
                                                                    );
                                                                }
                                                                clicked |= label_resp.clicked();
                                                                clicked
                                                            });
                                                            if row.inner {
                                                                let _ =
                                                                    command_tx.send(PlaybackCommand::Seek(event.clock + battle_start));
                                                            }
                                                        }
                                                    });
                                                }
                                            } else {
                                                ui.spinner();
                                                ui.label("Parsing events...");
                                            }
                                        });
                                });
                            });
                    }

                    toasts.lock().show(ctx);
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    window_open.store(false, Ordering::Relaxed);
                    let _ = command_tx.send(PlaybackCommand::Stop);
                    ctx.request_repaint();
                } else if playing {
                    ctx.request_repaint();
                }
            },
        );
    }
}

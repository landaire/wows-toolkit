use crate::icon_str;
use std::collections::HashMap;
use std::collections::HashSet;
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
use parking_lot::Mutex;

use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::GameFonts;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::RenderProgress;
use wows_minimap_renderer::RenderStage;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;
use wows_minimap_renderer::map_data::MapInfo;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::WorldPos;
use wowsunpack::vfs::VfsPath;

use egui_taffy::AsTuiBuilder as _;
use egui_taffy::TuiBuilderLogic as _;
use egui_taffy::taffy;
use egui_taffy::taffy::prelude::auto;
use egui_taffy::taffy::prelude::length;

use crate::collab::SessionStatus;
use crate::collab::peer::FrameBroadcast;
use crate::icons;
use crate::settings::SavedRenderOptions;
use crate::wows_data::SharedWoWsData;

use crate::controls::CommandGroup;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Approximate number of frame snapshots per second of game time.
/// Controls the granularity of seeking in the replay.
const SNAPSHOTS_PER_SECOND: f32 = 1.5;
const ICON_SIZE: f32 = assets::ICON_SIZE as f32;
const PLAYBACK_SPEEDS: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];

/// Font ID using the game font family (Warhelios Bold + CJK fallbacks).
/// Falls back to default proportional font if game fonts haven't been registered yet.
fn game_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("GameFont".into()))
}

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

pub(super) const SHIP_SPECIES: [&str; 5] = ["Destroyer", "Cruiser", "Battleship", "AirCarrier", "Submarine"];
pub(super) const FRIENDLY_COLOR: Color32 = Color32::from_rgb(76, 232, 170);
pub(super) const ENEMY_COLOR: Color32 = Color32::from_rgb(254, 77, 42);

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

/// Snapshot of annotation state for undo/redo.
#[derive(Clone)]
struct AnnotationSnapshot {
    annotations: Vec<Annotation>,
    ids: Vec<u64>,
    owners: Vec<u64>,
}

/// Persistent annotation layer state.
struct AnnotationState {
    annotations: Vec<Annotation>,
    /// Unique ID for each annotation (parallel to `annotations`).
    annotation_ids: Vec<u64>,
    undo_stack: Vec<AnnotationSnapshot>,
    active_tool: PaintTool,
    paint_color: Color32,
    stroke_width: f32,
    selected_index: Option<usize>,
    show_context_menu: bool,
    context_menu_pos: Pos2,
    dragging_rotation: bool,
    /// Ships whose trails are explicitly hidden (by player name).
    trail_hidden_ships: HashSet<String>,
    /// Ship nearest to right-click position (entity_id, player_name) for context menu options.
    context_menu_ship: Option<(EntityId, String)>,
    /// Per-ship range overrides keyed by entity ID.
    /// When a ship is in this map, these flags override the global range toggles.
    ship_range_overrides: HashMap<EntityId, ShipConfigFilter>,
    /// Initial self range filter from saved settings, applied once self entity ID is known.
    pending_self_range_filter: Option<ShipConfigFilter>,
    /// Owner user_ids for each annotation (parallel to `annotations`), received from collab sync.
    annotation_owners: Vec<u64>,
}

impl Default for AnnotationState {
    fn default() -> Self {
        Self {
            annotations: Vec::new(),
            annotation_ids: Vec::new(),
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
            pending_self_range_filter: None,
            annotation_owners: Vec::new(),
        }
    }
}

impl AnnotationState {
    /// Save current annotations as an undo snapshot.
    fn save_undo(&mut self) {
        self.undo_stack.push(AnnotationSnapshot {
            annotations: self.annotations.clone(),
            ids: self.annotation_ids.clone(),
            owners: self.annotation_owners.clone(),
        });
        // Cap stack size
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the last undo snapshot, restoring annotations.
    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.annotations = prev.annotations;
            self.annotation_ids = prev.ids;
            self.annotation_owners = prev.owners;
            self.selected_index = None;
        }
    }
}

// ─── Collab annotation conversion ────────────────────────────────────────────

/// Convert a collab wire annotation (primitive arrays) to the local annotation (egui types).
fn collab_annotation_to_local(ca: crate::collab::types::Annotation) -> Annotation {
    use crate::collab::types as ct;
    match ca {
        ct::Annotation::Ship { pos, yaw, species, friendly } => {
            Annotation::Ship { pos: Vec2::new(pos[0], pos[1]), yaw, species, friendly }
        }
        ct::Annotation::FreehandStroke { points, color, width } => Annotation::FreehandStroke {
            points: points.into_iter().map(|p| Vec2::new(p[0], p[1])).collect(),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        ct::Annotation::Line { start, end, color, width } => Annotation::Line {
            start: Vec2::new(start[0], start[1]),
            end: Vec2::new(end[0], end[1]),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        ct::Annotation::Circle { center, radius, color, width, filled } => Annotation::Circle {
            center: Vec2::new(center[0], center[1]),
            radius,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        ct::Annotation::Rectangle { center, half_size, rotation, color, width, filled } => Annotation::Rectangle {
            center: Vec2::new(center[0], center[1]),
            half_size: Vec2::new(half_size[0], half_size[1]),
            rotation,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        ct::Annotation::Triangle { center, radius, rotation, color, width, filled } => Annotation::Triangle {
            center: Vec2::new(center[0], center[1]),
            radius,
            rotation,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
    }
}

/// Convert a local annotation (egui types) to collab wire annotation (primitive arrays).
#[allow(dead_code)]
fn local_annotation_to_collab(a: &Annotation) -> crate::collab::types::Annotation {
    use crate::collab::types as ct;
    match a {
        Annotation::Ship { pos, yaw, species, friendly } => {
            ct::Annotation::Ship { pos: [pos.x, pos.y], yaw: *yaw, species: species.clone(), friendly: *friendly }
        }
        Annotation::FreehandStroke { points, color, width } => ct::Annotation::FreehandStroke {
            points: points.iter().map(|p| [p.x, p.y]).collect(),
            color: color.to_array(),
            width: *width,
        },
        Annotation::Line { start, end, color, width } => ct::Annotation::Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
            color: color.to_array(),
            width: *width,
        },
        Annotation::Circle { center, radius, color, width, filled } => ct::Annotation::Circle {
            center: [center.x, center.y],
            radius: *radius,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Rectangle { center, half_size, rotation, color, width, filled } => ct::Annotation::Rectangle {
            center: [center.x, center.y],
            half_size: [half_size.x, half_size.y],
            rotation: *rotation,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Triangle { center, radius, rotation, color, width, filled } => ct::Annotation::Triangle {
            center: [center.x, center.y],
            radius: *radius,
            rotation: *rotation,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
    }
}

/// Send a `SetAnnotation` event for the annotation at `idx` via the collab channel.
fn send_annotation_update(shared_state: &SharedRendererState, ann: &AnnotationState, idx: usize) {
    if let Some(ref tx) = shared_state.collab_local_tx {
        let _ = tx.send(crate::collab::peer::LocalEvent::Annotation(
            crate::collab::peer::LocalAnnotationEvent::Set {
                id: ann.annotation_ids[idx],
                annotation: local_annotation_to_collab(&ann.annotations[idx]),
                owner: ann.annotation_owners.get(idx).copied().unwrap_or(0),
            },
        ));
    }
}

/// Send a `RemoveAnnotation` event for the given annotation ID via the collab channel.
fn send_annotation_remove(shared_state: &SharedRendererState, id: u64) {
    if let Some(ref tx) = shared_state.collab_local_tx {
        let _ = tx.send(crate::collab::peer::LocalEvent::Annotation(
            crate::collab::peer::LocalAnnotationEvent::Remove { id },
        ));
    }
}

/// Send a `ClearAnnotations` event via the collab channel.
fn send_annotation_clear(shared_state: &SharedRendererState) {
    if let Some(ref tx) = shared_state.collab_local_tx {
        let _ = tx.send(crate::collab::peer::LocalEvent::Annotation(
            crate::collab::peer::LocalAnnotationEvent::Clear,
        ));
    }
}

/// Send a full annotation sync (used after undo to broadcast the complete state).
fn send_annotation_full_sync(shared_state: &SharedRendererState, ann: &AnnotationState) {
    if let Some(ref tx) = shared_state.collab_command_tx {
        let collab_anns: Vec<_> = ann.annotations.iter().map(local_annotation_to_collab).collect();
        let _ = tx.send(crate::collab::SessionCommand::SyncAnnotations {
            annotations: collab_anns,
            owners: ann.annotation_owners.clone(),
            ids: ann.annotation_ids.clone(),
        });
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
    game_fonts: Option<GameFonts>,
    maps: HashMap<String, CachedMapData>,
}

struct CachedMapData {
    image: Option<Arc<RgbaAsset>>,
    info: Option<MapInfo>,
}

impl RendererAssetCache {
    fn get_or_load_ship_icons(&mut self, vfs: &VfsPath) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.ship_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_ship_icons(vfs);
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

    fn get_or_load_plane_icons(&mut self, vfs: &VfsPath) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.plane_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_plane_icons(vfs);
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

    fn get_or_load_consumable_icons(&mut self, vfs: &VfsPath) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.consumable_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_consumable_icons(vfs);
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

    fn get_or_load_death_cause_icons(&mut self, vfs: &VfsPath) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.death_cause_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_death_cause_icons(vfs, 16);
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

    fn get_or_load_powerup_icons(&mut self, vfs: &VfsPath) -> Arc<HashMap<String, RgbaAsset>> {
        if let Some(ref cached) = self.powerup_icons {
            return Arc::clone(cached);
        }
        let raw = assets::load_powerup_icons(vfs, 16);
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

    fn get_or_load_game_fonts(&mut self, vfs: &VfsPath) -> GameFonts {
        if let Some(ref cached) = self.game_fonts {
            return cached.clone();
        }
        let fonts = assets::load_game_fonts(vfs);
        self.game_fonts = Some(fonts.clone());
        fonts
    }

    fn get_or_load_map(&mut self, map_name: &str, vfs: &VfsPath) -> (Option<Arc<RgbaAsset>>, Option<MapInfo>) {
        if let Some(cached) = self.maps.get(map_name) {
            return (cached.image.clone(), cached.info.clone());
        }
        let map_image = assets::load_map_image(map_name, vfs).map(|img| {
            let rgba = image::DynamicImage::ImageRgb8(img).into_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            Arc::new((rgba.into_raw(), w, h))
        });
        let map_info = assets::load_map_info(map_name, vfs);
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
        show_weather: saved.show_buildings, // TODO: add show_weather to SavedRenderOptions
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
        // UI does its own per-ship filtering in the draw loop, so emit all circles
        ship_config_visibility: ShipConfigVisibility::Filtered(Arc::new(|_| Some(ShipConfigFilter::all_enabled()))),
    }
}

/// Send the current per-ship trail hidden set through the collab channel (if connected).
fn broadcast_trail_overrides(trail_hidden: &HashSet<String>, shared_state: &Arc<Mutex<SharedRendererState>>) {
    let state = shared_state.lock();
    if let Some(ref tx) = state.collab_local_tx {
        let data: Vec<_> = trail_hidden.iter().cloned().collect();
        let _ = tx.send(crate::collab::peer::LocalEvent::TrailOverrides(data));
    }
}

/// Send the current per-ship range overrides through the collab channel (if connected).
fn broadcast_range_overrides(
    overrides: &HashMap<EntityId, ShipConfigFilter>,
    shared_state: &Arc<Mutex<SharedRendererState>>,
) {
    let state = shared_state.lock();
    if let Some(ref tx) = state.collab_local_tx {
        let data: Vec<_> = overrides.iter().map(|(k, v)| (*k, *v)).collect();
        let _ = tx.send(crate::collab::peer::LocalEvent::RangeOverrides(data));
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
        // Range filter flags are persisted from annotation state at the call site
        show_self_detection_range: false,
        show_self_main_battery_range: false,
        show_self_secondary_range: false,
        show_self_torpedo_range: false,
        show_self_radar_range: false,
        show_self_hydro_range: false,
        show_chat: opts.show_chat,
        show_advantage: opts.show_advantage,
        show_score_timer: opts.show_score_timer,
        prefer_cpu_encoder: false, // Not part of RenderOptions; set by caller
    }
}

// ─── Commands & Shared State ─────────────────────────────────────────────────

/// Commands sent from the UI thread to the background playback thread.
pub enum PlaybackCommand {
    Play,
    Pause,
    Seek(GameClock),
    SetSpeed(f32),
    Stop,
}

/// A single frame's rendering data, shared from background to UI thread.
pub struct PlaybackFrame {
    pub replay_id: u64,
    pub commands: Vec<DrawCommand>,
    pub clock: GameClock,
    pub frame_index: usize,
    pub total_frames: usize,
    pub game_duration: f32,
}

// ─── Realtime Armor Bridge ───────────────────────────────────────────────────

/// A salvo event extracted from the replay for the realtime armor viewer.
#[derive(Clone, Debug)]
pub struct ReplaySalvoEvent {
    pub clock: GameClock,
    /// Estimated time the shells reach the target (fire time + flight time).
    pub estimated_impact_clock: GameClock,
    pub target_entity_id: EntityId,
    pub attacker_entity_id: EntityId,
    pub params_id: wowsunpack::game_types::GameParamId,
    pub shots: Vec<ReplayShotData>,
    /// Target ship's yaw (radians) at the time this salvo was created.
    pub target_ship_yaw: f32,
    /// Target ship's world position at the time this salvo was created.
    pub target_ship_position: WorldPos,
}

/// Per-shell origin/target in world space (BigWorld coordinates).
#[derive(Clone, Debug)]
pub struct ReplayShotData {
    pub origin: WorldPos,
    pub target: WorldPos,
}

/// Player info snapshot captured from BattleController for the armor viewer.
#[derive(Clone, Debug)]
pub struct ReplayPlayerInfo {
    pub entity_id: EntityId,
    pub username: String,
    pub team_id: i64,
    pub vehicle: Arc<wowsunpack::game_params::types::Param>,
    pub ship_display_name: String,
    pub is_friendly: bool,
    /// Equipped hull GameParamId from the replay's ShipConfig.
    pub hull_param_id: Option<wowsunpack::game_types::GameParamId>,
}

/// Shared bridge between replay thread and realtime armor viewer windows.
pub struct RealtimeArmorBridge {
    pub players: Vec<ReplayPlayerInfo>,
    pub salvos: Vec<ReplaySalvoEvent>,
    /// Resolved shot hits from ShotKills packets, matched to originating salvos.
    pub shot_hits: Vec<ResolvedShotHit>,
    pub last_clock: GameClock,
    /// The entity this bridge tracks (the ship whose armor viewer is open).
    pub target_entity_id: EntityId,
    /// Incremented each time salvos are cleared (seek/rebuild). Consumers use
    /// this to detect that their cursor into `salvos` is stale.
    pub generation: u64,
    /// Pre-computed shot timeline for this target ship (entire replay).
    /// Set after the shot extraction pass completes.
    pub shot_timeline: Option<Arc<ShipShotTimeline>>,
}

impl RealtimeArmorBridge {
    pub fn new(target_entity_id: EntityId) -> Self {
        Self {
            players: Vec::new(),
            salvos: Vec::new(),
            shot_hits: Vec::new(),
            last_clock: GameClock(0.0),
            target_entity_id,
            generation: 0,
            shot_timeline: None,
        }
    }

    pub fn clear_salvos(&mut self) {
        self.salvos.clear();
        self.shot_hits.clear();
        self.last_clock = GameClock(0.0);
        self.generation += 1;
    }
}

/// A request from the context menu to open a realtime armor viewer.
pub struct ArmorViewerRequest {
    pub target_entity_id: EntityId,
    pub target_ship_name: String,
    pub bridge: Arc<Mutex<RealtimeArmorBridge>>,
    /// Sender for playback commands (seek, etc.) back to the replay thread.
    pub command_tx: mpsc::Sender<PlaybackCommand>,
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
    pub battle_start: GameClock,
    /// Actual game duration from the last packet's clock (may differ from metadata duration).
    pub actual_game_duration: Option<f32>,
    /// The replay owner's player name (from replay metadata).
    pub self_player_name: Option<String>,
    /// The replay owner's entity ID (resolved from draw commands).
    pub self_entity_id: Option<EntityId>,
    /// Game fonts loaded from game files, set by the background thread.
    pub game_fonts: Option<GameFonts>,
    /// Whether game fonts have been registered with the egui context.
    pub game_fonts_registered: bool,
    /// Active armor bridges for realtime armor viewer windows.
    pub armor_bridges: Vec<Arc<Mutex<RealtimeArmorBridge>>>,
    /// Pending requests to open realtime armor viewer windows (consumed by app.rs).
    pub pending_armor_viewers: Vec<ArmorViewerRequest>,
    /// Pre-computed shot timelines per target ship (entire replay).
    /// Set after the shot extraction pass completes.
    pub shot_timelines: Option<HashMap<EntityId, Arc<ShipShotTimeline>>>,
    /// Parsed replay/spectator keybinding groups from `commands.scheme.xml`.
    pub replay_controls: Option<Vec<CommandGroup>>,
    /// When a collab session is active, frames are cloned and sent here for broadcast.
    pub session_frame_tx: Option<std::sync::mpsc::SyncSender<FrameBroadcast>>,
    /// Game client version string from replay metadata (e.g. "0,13,7,0").
    pub game_version: Option<String>,
    /// Assigned collab replay ID when wired to a session.
    pub collab_replay_id: Option<u64>,
    /// True once a ReplayOpened command has been sent for this renderer.
    pub session_announced: bool,
    /// Version of render options last applied from the collab session.
    pub applied_render_options_version: u64,
    /// Version of annotation sync last applied from the collab session.
    pub applied_annotation_sync_version: u64,
    /// Version of per-ship range overrides last applied from the collab session.
    pub applied_range_override_version: u64,
    /// Version of per-ship trail overrides last applied from the collab session.
    pub applied_trail_override_version: u64,
    /// Reference to the collab session state (set by app.rs when wired to a session).
    pub collab_session_state: Option<Arc<Mutex<crate::collab::SessionState>>>,
    /// Channel to send local UI events (cursors, annotations, pings, etc.) to the collab peer task.
    pub collab_local_tx: Option<std::sync::mpsc::Sender<crate::collab::peer::LocalEvent>>,
    /// Channel to send session commands (e.g. ReplayOpened) directly from the
    /// background thread, avoiding cross-window repaint issues.
    pub collab_command_tx: Option<std::sync::mpsc::Sender<crate::collab::SessionCommand>>,
    /// Replay name for collab announcements (set once at creation).
    pub collab_replay_name: Option<String>,
    /// Map space size in BigWorld units (from MapInfo), used for px→km conversion.
    pub map_space_size: Option<f32>,
}

/// The cloneable viewport handle stored in TabState.
/// What kind of video export action is pending behind the GPU warning dialog.
enum PendingVideoExport {
    /// Save to a user-chosen file path.
    SaveToFile { output_path: String, options: RenderOptions, prefer_cpu: bool, actual_game_duration: Option<f32> },
    /// Render to a temporary file and copy to clipboard.
    CopyToClipboard { options: RenderOptions, prefer_cpu: bool, actual_game_duration: Option<f32> },
}

/// State for the GPU encoder warning dialog.
struct GpuEncoderWarning {
    /// The pending export action to execute if the user clicks "Ok".
    pending_action: PendingVideoExport,
    /// Whether the "Don't show this again" checkbox is checked.
    dont_show_again: bool,
}

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
    /// Progress of the current video export, updated by the background thread.
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    /// Data needed for video export (cloned from launch params).
    /// `None` for client viewers that don't have their own replay data.
    video_export_data: Option<Arc<VideoExportData>>,
    /// Zoom and pan state for the viewport. Persists across frames.
    zoom_pan: Arc<Mutex<ViewportZoomPan>>,
    /// Overlay controls visibility state.
    overlay_state: Arc<Mutex<OverlayState>>,
    /// Annotation/painting layer state.
    annotation_state: Arc<Mutex<AnnotationState>>,
    /// Shared flag for "suppress GPU encoder warning" (persisted in Settings).
    pub suppress_gpu_warning: Arc<AtomicBool>,
    /// Active GPU encoder warning dialog, if any.
    gpu_encoder_warning: Arc<Mutex<Option<GpuEncoderWarning>>>,
    /// User preference: prefer CPU (software) encoder for video export.
    prefer_cpu_encoder: Arc<AtomicBool>,
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
#[allow(clippy::too_many_arguments)]
pub fn launch_replay_renderer(
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
    replay_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    saved_options: &SavedRenderOptions,
    suppress_gpu_warning: Arc<AtomicBool>,
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
        battle_start: GameClock(0.0),
        actual_game_duration: None,
        self_player_name: None,
        self_entity_id: None,
        game_fonts: None,
        game_fonts_registered: false,
        armor_bridges: Vec::new(),
        pending_armor_viewers: Vec::new(),
        shot_timelines: None,
        replay_controls: None,
        session_frame_tx: None,
        game_version: None,
        collab_replay_id: None,
        session_announced: false,
        applied_render_options_version: 0,
        applied_annotation_sync_version: 0,
        applied_range_override_version: 0,
        applied_trail_override_version: 0,
        collab_session_state: None,
        collab_local_tx: None,
        collab_command_tx: None,
        collab_replay_name: Some(replay_name.clone()),
        map_space_size: None,
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
        video_export_progress: Arc::new(Mutex::new(None)),
        video_export_data: Some(video_export_data),
        zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
        overlay_state: Arc::new(Mutex::new(OverlayState::default())),
        annotation_state: Arc::new(Mutex::new({
            let mut ann = AnnotationState::default();
            let filter = saved_options.self_range_filter();
            if filter.any_enabled() {
                ann.pending_self_range_filter = Some(filter);
            }
            ann
        })),
        suppress_gpu_warning,
        gpu_encoder_warning: Arc::new(Mutex::new(None)),
        prefer_cpu_encoder: Arc::new(AtomicBool::new(saved_options.prefer_cpu_encoder)),
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

/// Create a lightweight client viewer for a collaborative session.
///
/// The client doesn't have its own replay data — it receives rendered frames
/// from the host. The map image is decoded from the PNG sent in SessionInfo.
/// Ship icons, fonts, and other assets are loaded from the local game data.
pub fn launch_client_renderer(
    replay_name: String,
    map_image_png: Vec<u8>,
    game_version: String,
    saved_options: &SavedRenderOptions,
    suppress_gpu_warning: Arc<AtomicBool>,
    wows_data: Option<&SharedWoWsData>,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
) -> ReplayRendererViewer {
    let initial_options = render_options_from_saved(saved_options);
    let (_command_tx, _command_rx) = mpsc::channel();

    // Decode PNG to RGBA
    let map_image = image::load_from_memory(&map_image_png).ok().map(|img| {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Arc::new((rgba.into_raw(), w, h) as RgbaAsset)
    });

    // Load icons and fonts from VFS via the shared asset cache.
    let (ship_icons, plane_icons, consumable_icons, death_cause_icons, powerup_icons, game_fonts) =
        if let Some(vfs) = wows_data.map(|d| d.read().vfs.clone()) {
            let mut cache = asset_cache.lock();
            let si = cache.get_or_load_ship_icons(&vfs);
            let pi = cache.get_or_load_plane_icons(&vfs);
            let ci = cache.get_or_load_consumable_icons(&vfs);
            let di = cache.get_or_load_death_cause_icons(&vfs);
            let pwi = cache.get_or_load_powerup_icons(&vfs);
            let gf = cache.get_or_load_game_fonts(&vfs);
            (si, pi, ci, di, pwi, Some(gf))
        } else {
            (
                Arc::new(HashMap::new()),
                Arc::new(HashMap::new()),
                Arc::new(HashMap::new()),
                Arc::new(HashMap::new()),
                Arc::new(HashMap::new()),
                None,
            )
        };

    let shared_state = Arc::new(Mutex::new(SharedRendererState {
        status: RendererStatus::Loading,
        frame: None,
        assets: Some(ReplayRendererAssets {
            map_image,
            ship_icons,
            plane_icons,
            consumable_icons,
            death_cause_icons,
            powerup_icons,
        }),
        playing: false,
        speed: 1.0,
        options: initial_options,
        show_dead_ships: saved_options.show_dead_ships,
        viewport_ctx: None,
        timeline_events: None,
        battle_start: GameClock(0.0),
        actual_game_duration: None,
        self_player_name: None,
        self_entity_id: None,
        game_fonts,
        game_fonts_registered: false,
        armor_bridges: Vec::new(),
        pending_armor_viewers: Vec::new(),
        shot_timelines: None,
        replay_controls: None,
        session_frame_tx: None,
        game_version: Some(game_version),
        collab_replay_id: None,
        session_announced: false,
        applied_render_options_version: 0,
        applied_annotation_sync_version: 0,
        applied_range_override_version: 0,
        applied_trail_override_version: 0,
        collab_session_state: None,
        collab_local_tx: None,
        collab_command_tx: None,
        collab_replay_name: None,
        map_space_size: None,
    }));

    let title = Arc::new(format!("Collab Viewer - {replay_name}"));

    ReplayRendererViewer {
        title,
        open: Arc::new(AtomicBool::new(true)),
        shared_state: Arc::clone(&shared_state),
        command_tx: _command_tx,
        textures: Arc::new(Mutex::new(None)),
        pending_defaults_save: Arc::new(Mutex::new(None)),
        toasts: Arc::new(parking_lot::Mutex::new(egui_notify::Toasts::default())),
        video_exporting: Arc::new(AtomicBool::new(false)),
        video_export_progress: Arc::new(Mutex::new(None)),
        video_export_data: None,
        zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
        overlay_state: Arc::new(Mutex::new(OverlayState::default())),
        annotation_state: Arc::new(Mutex::new({
            let mut ann = AnnotationState::default();
            let filter = saved_options.self_range_filter();
            if filter.any_enabled() {
                ann.pending_self_range_filter = Some(filter);
            }
            ann
        })),
        suppress_gpu_warning,
        gpu_encoder_warning: Arc::new(Mutex::new(None)),
        prefer_cpu_encoder: Arc::new(AtomicBool::new(false)),
    }
}

mod playback;
use playback::playback_thread;

mod timeline;
pub use timeline::HealthSnapshot;
pub use timeline::PreExtractedHit;
pub use timeline::ShipShotTimeline;
pub use timeline::ShotCountHints;
pub(crate) use timeline::TimelineEvent;
pub(crate) use timeline::TimelineEventKind;
pub(crate) use timeline::event_color;
pub(crate) use timeline::format_timeline_event;

mod video_export;
use video_export::execute_video_export;

mod shapes;
use shapes::*;

mod textures;
use textures::upload_textures;

// ─── Viewport Rendering ─────────────────────────────────────────────────────

impl ReplayRendererViewer {
    /// Access the shared renderer state (for polling pending requests, etc.).
    pub fn shared_state(&self) -> &Arc<Mutex<SharedRendererState>> {
        &self.shared_state
    }

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
        let video_export_progress = self.video_export_progress.clone();
        let video_export_data = self.video_export_data.clone();
        let zoom_pan_arc = self.zoom_pan.clone();
        let overlay_state_arc = self.overlay_state.clone();
        let annotation_arc = self.annotation_state.clone();
        let suppress_gpu_warning = self.suppress_gpu_warning.clone();
        let gpu_encoder_warning = self.gpu_encoder_warning.clone();
        let prefer_cpu_encoder = self.prefer_cpu_encoder.clone();
        let parent_ctx = ctx.clone();

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(&*self.title),
            egui::ViewportBuilder::default()
                .with_title(&*self.title)
                .with_inner_size([800.0, 900.0])
                .with_min_inner_size([400.0, 450.0]),
            move |ctx, _class| {
                if !window_open.load(Ordering::Relaxed) || crate::app::mitigate_wgpu_mem_leak(ctx) {
                    return;
                }

                let mut repaint = false;

                let mut state = shared_state.lock();

                // Register game fonts with egui on the first frame.
                // set_fonts() doesn't take effect until the next frame, so we
                // track whether we just registered to avoid using them too early.
                let mut fonts_just_registered = false;
                if !state.game_fonts_registered {
                    let mut font_defs = egui::FontDefinitions::default();
                    egui_phosphor::add_to_fonts(&mut font_defs, egui_phosphor::Variant::Regular);

                    if let Some(ref fonts) = state.game_fonts {
                        font_defs.font_data.insert(
                            "game_font_primary".to_owned(),
                            egui::FontData::from_owned(fonts.primary_bytes.clone()).into(),
                        );
                        let mut family_fonts = vec!["game_font_primary".to_owned()];
                        let fallback_names = ["game_font_ko", "game_font_jp", "game_font_cn"];
                        for (i, bytes) in fonts.fallback_bytes.iter().enumerate() {
                            let name = fallback_names.get(i).unwrap_or(&"game_font_fallback").to_string();
                            font_defs.font_data.insert(
                                name.clone(),
                                egui::FontData::from_owned(bytes.clone()).into(),
                            );
                            family_fonts.push(name);
                        }
                        font_defs.families.insert(
                            egui::FontFamily::Name("GameFont".into()),
                            family_fonts,
                        );
                    } else {
                        let proportional = font_defs
                            .families
                            .get(&egui::FontFamily::Proportional)
                            .cloned()
                            .unwrap_or_default();
                        font_defs.families.insert(
                            egui::FontFamily::Name("GameFont".into()),
                            proportional,
                        );
                    }

                    ctx.set_fonts(font_defs);
                    state.game_fonts_registered = true;
                    fonts_just_registered = true;
                }

                // For client renderers: transition Loading→Ready once fonts are
                // effective (registered on a prior frame) and a frame has arrived.
                if matches!(state.status, RendererStatus::Loading)
                    && state.frame.is_some()
                    && !fonts_just_registered
                {
                    tracing::debug!("Renderer: Loading→Ready (fonts effective, frame available)");
                    state.status = RendererStatus::Ready;
                }

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
                let actual_game_duration = state.actual_game_duration;
                let frame_data =
                    state.frame.as_ref().map(|f| (f.frame_index, f.total_frames, f.clock, f.game_duration));
                // Resolve self entity ID from draw commands (once)
                let self_entity_id = if state.self_entity_id.is_some() {
                    state.self_entity_id
                } else if let Some(ref frame) = state.frame {
                    let eid = frame.commands.iter().find_map(|cmd| {
                        if let DrawCommand::Ship { entity_id, is_self: true, .. } = cmd {
                            Some(*entity_id)
                        } else {
                            None
                        }
                    });
                    if eid.is_some() {
                        state.self_entity_id = eid;
                    }
                    eid
                } else {
                    None
                };

                drop(state);

                // Apply pending render options / annotation sync from collab session (version-based).
                // Session lifecycle events (Started, Ended, etc.) are polled by app.rs.
                {
                    let mut state = shared_state.lock();
                    if let Some(ref session_state_arc) = state.collab_session_state.clone() {
                        let s = session_state_arc.lock();
                            if s.render_options_version > state.applied_render_options_version {
                                if let Some(ref opts) = s.current_render_options {
                                    state.options.show_hp_bars = opts.show_hp_bars;
                                    state.options.show_tracers = opts.show_tracers;
                                    state.options.show_torpedoes = opts.show_torpedoes;
                                    state.options.show_planes = opts.show_planes;
                                    state.options.show_smoke = opts.show_smoke;
                                    state.options.show_score = opts.show_score;
                                    state.options.show_timer = opts.show_timer;
                                    state.options.show_kill_feed = opts.show_kill_feed;
                                    state.options.show_player_names = opts.show_player_names;
                                    state.options.show_ship_names = opts.show_ship_names;
                                    state.options.show_capture_points = opts.show_capture_points;
                                    state.options.show_buildings = opts.show_buildings;
                                    state.options.show_turret_direction = opts.show_turret_direction;
                                    state.options.show_consumables = opts.show_consumables;
                                    state.options.show_armament = opts.show_armament;
                                    state.options.show_trails = opts.show_trails;
                                    state.options.show_dead_trails = opts.show_dead_trails;
                                    state.options.show_speed_trails = opts.show_speed_trails;
                                    state.options.show_ship_config = opts.show_ship_config;
                                    state.options.show_dead_ship_names = opts.show_dead_ship_names;
                                    state.options.show_battle_result = opts.show_battle_result;
                                    state.options.show_buffs = opts.show_buffs;
                                    state.options.show_chat = opts.show_chat;
                                    state.options.show_advantage = opts.show_advantage;
                                    state.options.show_score_timer = opts.show_score_timer;
                                    state.show_dead_ships = opts.show_dead_ships;
                                }
                                state.applied_render_options_version = s.render_options_version;
                            }
                            if s.annotation_sync_version > state.applied_annotation_sync_version {
                                if let Some(ref sync) = s.current_annotation_sync {
                                    let mut ann = annotation_arc.lock();
                                    ann.annotations = sync.annotations.iter().cloned().map(collab_annotation_to_local).collect();
                                    ann.annotation_owners = sync.owners.clone();
                                    ann.annotation_ids = sync.ids.clone();
                                }
                                state.applied_annotation_sync_version = s.annotation_sync_version;
                            }
                            if s.range_override_version > state.applied_range_override_version {
                                if let Some(ref overrides) = s.current_range_overrides {
                                    let mut ann = annotation_arc.lock();
                                    ann.ship_range_overrides.clear();
                                    for &(eid, filter) in overrides {
                                        ann.ship_range_overrides.insert(eid, filter);
                                    }
                                    // Enable show_ship_config if any overrides are present
                                    if !ann.ship_range_overrides.is_empty() && !state.options.show_ship_config {
                                        state.options.show_ship_config = true;
                                    }
                                }
                                state.applied_range_override_version = s.range_override_version;
                            }
                            if s.trail_override_version > state.applied_trail_override_version {
                                if let Some(ref hidden) = s.current_trail_hidden {
                                    let mut ann = annotation_arc.lock();
                                    ann.trail_hidden_ships.clear();
                                    for name in hidden {
                                        ann.trail_hidden_ships.insert(name.clone());
                                    }
                                    // Enable show_trails if there are still some visible trails
                                    if !ann.trail_hidden_ships.is_empty() && !state.options.show_trails {
                                        state.options.show_trails = true;
                                    }
                                }
                                state.applied_trail_override_version = s.trail_override_version;
                            }
                        }
                }

                // Apply pending self range filter once self entity ID is known
                if let Some(self_eid) = self_entity_id {
                    let mut ann = annotation_arc.lock();
                    if let Some(filter) = ann.pending_self_range_filter.take() {
                        ann.ship_range_overrides.insert(self_eid, filter);
                        // Ensure show_ship_config is enabled
                        let mut state = shared_state.lock();
                        if !state.options.show_ship_config {
                            state.options.show_ship_config = true;
                        }
                    }
                }

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
                        repaint = true;
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

                            let label_font = game_font(11.0 * window_scale);
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
                            // Collect alive ship entity IDs for filtering config circles
                            let alive_ships: HashSet<EntityId> = frame
                                .commands
                                .iter()
                                .filter_map(|cmd| {
                                    if let DrawCommand::Ship { entity_id, .. } = cmd {
                                        Some(*entity_id)
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            // Separate HUD and map commands so HUD draws on unclipped painter
                            let mut placed_labels: Vec<Rect> = Vec::new();
                            for cmd in &frame.commands {
                                if !should_draw_command(cmd, &options, show_dead_ships) {
                                    continue;
                                }
                                // Apply per-ship trail filter
                                if let DrawCommand::PositionTrail { player_name, .. } = cmd
                                    && let Some(name) = player_name
                                        && trail_hidden_ships.contains(name) {
                                            continue;
                                        }
                                // Apply per-ship config circle filter (only show if explicitly enabled via right-click, never for dead ships)
                                if let DrawCommand::ShipConfigCircle { entity_id, kind, .. } = cmd {
                                    if !alive_ships.contains(entity_id) {
                                        continue;
                                    }
                                    let enabled = if let Some(filter) = ship_range_overrides.get(entity_id) {
                                        filter.is_enabled(kind)
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
                                let cmd_shapes = draw_command_to_shapes(cmd, &transform, textures, ctx, &options, &mut placed_labels);
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
                                    let bar_height = HUD_HEIGHT as f32 * ws;
                                    let bar_origin = transform.hud_pos(0.0, 0.0);

                                    // Recompute cursor positions matching ScoreBar rendering
                                    let score_font = game_font(14.0 * ws);
                                    let timer_font = game_font(12.0 * ws);
                                    let adv_font = game_font(11.0 * ws);
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
                                        let fmt_contrib = |val: (f32, f32)| -> String {
                                            let diff = val.0 - val.1;
                                            if diff > 0.0 {
                                                format!("+{:.1}", diff)
                                            } else if diff < 0.0 {
                                                format!("{:.1}", diff)
                                            } else {
                                                "0".to_string()
                                            }
                                        };
                                        let is_nonzero = |val: (f32, f32)| val.0 != 0.0 || val.1 != 0.0;
                                        ui.label(egui::RichText::new("Advantage Breakdown").strong());
                                        ui.separator();
                                        if bd.team_eliminated {
                                            ui.label("A team has been eliminated");
                                        } else {
                                            egui::Grid::new("adv_grid").num_columns(2).show(ui, |ui| {
                                                if is_nonzero(bd.score_projection) {
                                                    ui.label("Score Projection");
                                                    ui.label(fmt_contrib(bd.score_projection));
                                                    ui.end_row();
                                                }
                                                if is_nonzero(bd.fleet_power) {
                                                    ui.label("Fleet Power");
                                                    ui.label(fmt_contrib(bd.fleet_power));
                                                    ui.end_row();
                                                }
                                                if is_nonzero(bd.strategic_threat) {
                                                    ui.label("Strategic Threat");
                                                    ui.label(fmt_contrib(bd.strategic_threat));
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

                        // ─── Render remote cursors (collab session) ──────────
                        let collab_ss = shared_state.lock().collab_session_state.clone();
                        if let Some(ref ss_arc) = collab_ss {
                            let s = ss_arc.lock();
                            let now = std::time::Instant::now();
                            for cursor in &s.cursors {
                                // Skip our own cursor.
                                if cursor.user_id == s.my_user_id {
                                    continue;
                                }
                                if let Some(pos) = cursor.pos {
                                    let age = now.duration_since(cursor.last_update).as_secs_f32();
                                    if age > 5.0 {
                                        continue; // fully faded
                                    }
                                    let alpha = if age > 3.0 {
                                        ((5.0 - age) / 2.0 * 255.0) as u8
                                    } else {
                                        255
                                    };
                                    let [r, g, b] = cursor.color;
                                    let color = Color32::from_rgba_unmultiplied(r, g, b, alpha);

                                    let screen_pos = transform.minimap_to_screen(&MinimapPos { x: pos[0] as i32, y: pos[1] as i32 });

                                    // Draw cursor arrow (small triangle pointing up-left)
                                    let size = 10.0;
                                    let points = vec![
                                        screen_pos,
                                        screen_pos + Vec2::new(0.0, size * 1.5),
                                        screen_pos + Vec2::new(size * 0.6, size * 1.1),
                                    ];
                                    map_painter.add(egui::Shape::convex_polygon(
                                        points,
                                        color,
                                        egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, alpha)),
                                    ));

                                    // Draw name label
                                    let label_pos = screen_pos + Vec2::new(size * 0.8, size * 0.5);
                                    let galley = map_painter.layout_no_wrap(
                                        cursor.name.clone(),
                                        egui::FontId::proportional(11.0),
                                        color,
                                    );
                                    // Background for readability
                                    let label_rect = egui::Rect::from_min_size(
                                        label_pos - Vec2::new(2.0, 1.0),
                                        galley.size() + Vec2::new(4.0, 2.0),
                                    );
                                    map_painter.rect_filled(
                                        label_rect,
                                        2.0,
                                        Color32::from_rgba_unmultiplied(0, 0, 0, alpha / 2),
                                    );
                                    map_painter.galley(label_pos, galley, Color32::PLACEHOLDER);
                                }
                            }

                            // ─── Render map pings (ripple effects) ──────────────
                            let mut has_active_pings = false;
                            for ping in &s.pings {
                                let age = now.duration_since(ping.time).as_secs_f32();
                                if age > 1.0 {
                                    continue;
                                }
                                has_active_pings = true;
                                let max_r = transform.scale_distance(40.0);
                                let r = age * max_r;
                                let alpha = ((1.0 - age) * 200.0) as u8;
                                let [pr, pg, pb] = ping.color;
                                let ping_color = Color32::from_rgba_unmultiplied(pr, pg, pb, alpha);
                                let screen_pos = transform.minimap_to_screen(&MinimapPos { x: ping.pos[0] as i32, y: ping.pos[1] as i32 });
                                map_painter.add(egui::Shape::circle_stroke(screen_pos, r, egui::Stroke::new(2.0, ping_color)));
                                map_painter.add(egui::Shape::circle_stroke(screen_pos, r * 0.6, egui::Stroke::new(1.5, ping_color)));
                            }
                            if has_active_pings {
                                repaint = true;
                            }
                            drop(s);
                            // Clean up expired pings
                            if has_active_pings {
                                let mut s = ss_arc.lock();
                                s.pings.retain(|p| now.duration_since(p.time).as_secs_f32() < 1.0);
                            }
                        }
                    }

                    // ─── Send local cursor to collab session ────────────────
                    {
                        let cursor_pos = response.hover_pos().map(|p| {
                            let mp = transform.screen_to_minimap(p);
                            [mp.x, mp.y]
                        });
                        if let Some(ref tx) = shared_state.lock().collab_local_tx {
                            let _ = tx.send(crate::collab::peer::LocalEvent::CursorPosition(cursor_pos));
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
                                        if let DrawCommand::Ship { pos, entity_id, player_name: Some(name), .. } = cmd {
                                            let screen_pos = transform.minimap_to_screen(pos);
                                            let dist = click_pos.distance(screen_pos);
                                            if dist < best_dist {
                                                best_dist = dist;
                                                ann.context_menu_ship = Some((*entity_id, name.clone()));
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
                            let id = ann.annotation_ids[sel];
                            ann.annotations.remove(sel);
                            ann.annotation_ids.remove(sel);
                            ann.annotation_owners.remove(sel);
                            ann.selected_index = None;
                            send_annotation_remove(&shared_state.lock(), id);
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

                                // Stop rotation drag — sync final rotation to collab
                                if ann.dragging_rotation && response.drag_stopped_by(egui::PointerButton::Primary) {
                                    ann.dragging_rotation = false;
                                    if let Some(sel) = ann.selected_index {
                                        send_annotation_update(&shared_state.lock(), &ann, sel);
                                    }
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
                                        // Click on empty space → send map ping
                                        let state = shared_state.lock();
                                        if let Some(ref tx) = state.collab_local_tx {
                                            let _ = tx.send(crate::collab::peer::LocalEvent::Ping([click_pos.x, click_pos.y]));
                                        }
                                        // Also push a local ping so the sender sees their own ripple
                                        if let Some(ref ss_arc) = state.collab_session_state {
                                            let mut ss = ss_arc.lock();
                                            let my_id = ss.my_user_id;
                                            let color = ss.cursors.iter().find(|c| c.user_id == my_id).map(|c| c.color).unwrap_or([255, 255, 0]);
                                            ss.pings.push(crate::collab::PeerPing {
                                                user_id: my_id,
                                                color,
                                                pos: [click_pos.x, click_pos.y],
                                                time: std::time::Instant::now(),
                                            });
                                        }
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
                                // Sync moved annotation on drag release
                                if !ann.dragging_rotation
                                    && response.drag_stopped_by(egui::PointerButton::Primary)
                                    && let Some(sel) = ann.selected_index
                                    && sel < ann.annotations.len()
                                {
                                    send_annotation_update(&shared_state.lock(), &ann, sel);
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
                            }

                            PaintTool::DrawingCircle { filled, center } => {
                                // Drag to draw: click = center, drag outward = radius
                                if response.drag_started_by(egui::PointerButton::Primary)
                                    && let Some(pos) = cursor_minimap
                                {
                                    *center = Some(pos);
                                }
                                if response.drag_stopped_by(egui::PointerButton::Primary)
                                    && let Some(origin) = *center
                                {
                                    if let Some(pos) = cursor_minimap {
                                        let radius = (pos - origin).length();
                                        if radius > 1.0 {
                                            new_annotation = Some(Annotation::Circle {
                                                center: origin,
                                                radius,
                                                color: paint_color,
                                                width: stroke_width,
                                                filled: *filled,
                                            });
                                        }
                                    }
                                    *center = None;
                                }
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
                            }
                        }

                        // Apply deferred mutations after the match (borrow of active_tool is released)
                        if new_annotation.is_some() || erase_idx.is_some() {
                            ann.save_undo();
                        }
                        if let Some(a) = new_annotation {
                            let id: u64 = rand::random();
                            let my_user_id = shared_state.lock().collab_session_state.as_ref()
                                .map(|ss| ss.lock().my_user_id).unwrap_or(0);
                            ann.annotations.push(a);
                            ann.annotation_ids.push(id);
                            ann.annotation_owners.push(my_user_id);
                            let state = shared_state.lock();
                            send_annotation_update(&state, &ann, ann.annotations.len() - 1);
                        }
                        if let Some(idx) = erase_idx {
                            let id = ann.annotation_ids[idx];
                            ann.annotations.remove(idx);
                            ann.annotation_ids.remove(idx);
                            ann.annotation_owners.remove(idx);
                            send_annotation_remove(&shared_state.lock(), id);
                        }

                        // Ctrl+Z to undo
                        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
                            ann.undo();
                            send_annotation_full_sync(&shared_state.lock(), &ann);
                        }

                        if response.clicked() {
                            repaint = true;
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
                                let target = (clock_secs - 10.0).max(GameClock(0.0));
                                let _ = command_tx.send(PlaybackCommand::Seek(target));
                            }
                            if !shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                                let target = (clock_secs + 10.0).min(GameClock(game_dur));
                                let _ = command_tx.send(PlaybackCommand::Seek(target));
                            }

                            // Shift+Left/Right: skip to prev/next timeline event
                            let elapsed = clock_secs.to_elapsed(battle_start);
                            if shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                                let state = shared_state.lock();
                                if let Some(ref events) = state.timeline_events
                                    && let Some(event) = events.iter().rev().find(|e| e.clock < elapsed - 0.5) {
                                        let seek_clock = event.clock.to_absolute(battle_start);
                                        let desc = format_timeline_event(event);
                                        drop(state);
                                        let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                        toasts.lock().info(desc);
                                    }
                            }
                            if shift && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                                let state = shared_state.lock();
                                if let Some(ref events) = state.timeline_events
                                    && let Some(event) = events.iter().find(|e| e.clock > elapsed) {
                                        let seek_clock = event.clock.to_absolute(battle_start);
                                        let desc = format_timeline_event(event);
                                        drop(state);
                                        let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                        toasts.lock().info(desc);
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
                                        if let Some((ship_eid, ref ship_name)) = ann.context_menu_ship.clone() {
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
                                                broadcast_trail_overrides(&ann.trail_hidden_ships, &shared_state);
                                                repaint = true;
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
                                                            && name != ship_name {
                                                                ann.trail_hidden_ships.insert(name.clone());
                                                            }
                                                    }
                                                }
                                                ann.trail_hidden_ships.remove(ship_name);
                                                if !state.options.show_trails {
                                                    drop(state);
                                                    shared_state.lock().options.show_trails = true;
                                                }
                                                broadcast_trail_overrides(&ann.trail_hidden_ships, &shared_state);
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }

                                            // Per-type range toggles for this ship
                                            let mut filter =
                                                ann.ship_range_overrides.get(&ship_eid).copied().unwrap_or_default();
                                            ui.label(egui::RichText::new("Ranges").small());
                                            let mut range_changed = false;
                                            range_changed |= ui.checkbox(&mut filter.detection, "Detection").changed();
                                            range_changed |= ui.checkbox(&mut filter.main_battery, "Main Battery").changed();
                                            range_changed |= ui.checkbox(&mut filter.secondary_battery, "Secondary").changed();
                                            range_changed |= ui.checkbox(&mut filter.torpedo, "Torpedo").changed();
                                            range_changed |= ui.checkbox(&mut filter.radar, "Radar").changed();
                                            range_changed |= ui.checkbox(&mut filter.hydro, "Hydro").changed();
                                            let all_on = filter == ShipConfigFilter::all_enabled();
                                            if !all_on && ui.button("Enable All").clicked() {
                                                filter = ShipConfigFilter::all_enabled();
                                                range_changed = true;
                                            } else if all_on && ui.button("Disable All").clicked() {
                                                filter = ShipConfigFilter::default();
                                                range_changed = true;
                                            }
                                            if range_changed {
                                                if !filter.any_enabled() {
                                                    ann.ship_range_overrides.remove(&ship_eid);
                                                } else {
                                                    ann.ship_range_overrides.insert(ship_eid, filter);
                                                }
                                                // Auto-enable global when turning on any range
                                                if filter.any_enabled() {
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
                                                broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);
                                                repaint = true;
                                            }

                                            // Disable all other ships' ranges
                                            if ui.button("Disable All Other Ranges").clicked() {
                                                let keys: Vec<EntityId> = ann
                                                    .ship_range_overrides
                                                    .keys()
                                                    .filter(|k| **k != ship_eid)
                                                    .copied()
                                                    .collect();
                                                for k in keys {
                                                    ann.ship_range_overrides.remove(&k);
                                                }
                                                broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }

                                            // Enable ranges for all alive ships
                                            if ui.button("Enable All Ships' Ranges").clicked() {
                                                let state = shared_state.lock();
                                                if let Some(ref frame) = state.frame {
                                                    for cmd in &frame.commands {
                                                        if let DrawCommand::Ship { entity_id, .. } = cmd {
                                                            ann.ship_range_overrides
                                                                .insert(*entity_id, ShipConfigFilter::all_enabled());
                                                        }
                                                    }
                                                }
                                                if !state.options.show_ship_config {
                                                    drop(state);
                                                    shared_state.lock().options.show_ship_config = true;
                                                }
                                                broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);
                                                ann.show_context_menu = false;
                                                repaint = true;
                                            }

                                            // ── Realtime Armor Viewer ──
                                            ui.separator();
                                            if ui.button(icon_str!(icons::SHIELD, "Show Realtime Armor")).clicked() {
                                                let mut new_bridge = RealtimeArmorBridge::new(ship_eid);
                                                let mut state = shared_state.lock();
                                                // Attach pre-computed shot timeline if available
                                                if let Some(ref timelines) = state.shot_timelines {
                                                    new_bridge.shot_timeline = timelines.get(&ship_eid).cloned();
                                                }
                                                let bridge = Arc::new(Mutex::new(new_bridge));
                                                state.armor_bridges.push(bridge.clone());
                                                state.pending_armor_viewers.push(ArmorViewerRequest {
                                                    target_entity_id: ship_eid,
                                                    target_ship_name: ship_name.clone(),
                                                    bridge,
                                                    command_tx: command_tx.clone(),
                                                });
                                                // Seek to current position to populate the new bridge
                                                let current_clock = state
                                                    .frame
                                                    .as_ref()
                                                    .map(|f| f.clock)
                                                    .unwrap_or(GameClock(0.0));
                                                drop(state);
                                                let _ = command_tx.send(PlaybackCommand::Seek(current_clock));
                                                ann.show_context_menu = false;
                                                repaint = true;
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
                                                ann.annotation_ids.clear();
                                                ann.annotation_owners.clear();
                                                ann.show_context_menu = false;
                                                send_annotation_clear(&shared_state.lock());
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
                                                let is_circle = matches!(&ann.annotations[sel_idx], Annotation::Circle { .. });
                                                let map_space = shared_state.lock().map_space_size;
                                                let use_km = is_circle && map_space.is_some();
                                                let mut size = match &ann.annotations[sel_idx] {
                                                    Annotation::Circle { radius, .. } => *radius,
                                                    Annotation::Rectangle { half_size, .. } => {
                                                        (half_size.x + half_size.y) / 2.0
                                                    }
                                                    Annotation::Triangle { radius, .. } => *radius,
                                                    _ => 0.0,
                                                };
                                                let old = size;
                                                if use_km {
                                                    let space_size = map_space.unwrap();
                                                    let mut km = size / 768.0 * space_size * 30.0 / 1000.0;
                                                    let old_km = km;
                                                    ui.add(egui::DragValue::new(&mut km).speed(0.1).range(0.1..=20.0).fixed_decimals(1).suffix(" km"));
                                                    if km != old_km {
                                                        size = km * 1000.0 / 30.0 / space_size * 768.0;
                                                    }
                                                } else {
                                                    ui.add(egui::DragValue::new(&mut size).speed(1.0).range(5.0..=500.0));
                                                }
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
                                                    send_annotation_update(&shared_state.lock(), &ann, sel_idx);
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
                                                let old_color = *color_ref;
                                                egui::color_picker::color_edit_button_srgba(
                                                    ui,
                                                    color_ref,
                                                    egui::color_picker::Alpha::Opaque,
                                                );
                                                if *color_ref != old_color {
                                                    send_annotation_update(&shared_state.lock(), &ann, sel_idx);
                                                }
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
                                            let old_filled = *filled_ref;
                                            ui.checkbox(filled_ref, egui::RichText::new("Filled").small());
                                            if *filled_ref != old_filled {
                                                send_annotation_update(&shared_state.lock(), &ann, sel_idx);
                                            }
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
                                                send_annotation_update(&shared_state.lock(), &ann, sel_idx);
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
                                            let id = ann.annotation_ids[sel_idx];
                                            ann.annotations.remove(sel_idx);
                                            ann.annotation_ids.remove(sel_idx);
                                            ann.annotation_owners.remove(sel_idx);
                                            ann.selected_index = None;
                                            send_annotation_remove(&shared_state.lock(), id);
                                        }
                                    });
                                });
                        }
                    }

                    // ─── Overlay controls (video-player style) ───────────────────

                    // Video export progress bar overlay
                    if video_exporting.load(Ordering::Relaxed) {
                        let progress_text = if let Some(p) = video_export_progress.lock().clone() {
                            let pct = if p.total > 0 { p.current as f32 / p.total as f32 } else { 0.0 };
                            let label = match p.stage {
                                RenderStage::Encoding => "Encoding",
                                RenderStage::Muxing => "Muxing",
                            };
                            Some((pct, format!("{} ({}/{})", label, p.current, p.total)))
                        } else {
                            None
                        };

                        egui::Area::new(ui.id().with("video_export_progress"))
                            .order(egui::Order::Foreground)
                            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
                            .interactable(false)
                            .show(ctx, |ui| {
                                egui::Frame::new()
                                    .fill(Color32::from_rgba_unmultiplied(30, 30, 30, 200))
                                    .corner_radius(CornerRadius::same(4))
                                    .inner_margin(egui::Margin::symmetric(12, 6))
                                    .show(ui, |ui| {
                                        ui.set_width(300.0);
                                        if let Some((pct, label)) = progress_text {
                                            ui.add(egui::ProgressBar::new(pct).text(label));
                                        } else {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label("Preparing export...");
                                            });
                                        }
                                    });
                            });
                        repaint = true;
                    }

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
                        repaint = true;
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
                                                    let _ = command_tx.send(PlaybackCommand::Seek(GameClock(0.0)));
                                                }
                                            }

                                            // Skip to previous event
                                            if let Some((_fi, _tf, clock_secs, _gd)) = frame_data {
                                                let elapsed = clock_secs.to_elapsed(battle_start);
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .ui_add(egui::Button::new(icons::REWIND));
                                                if btn.on_hover_text("Previous event (Shift+Left)").clicked() {
                                                    let state = shared_state.lock();
                                                    if let Some(ref events) = state.timeline_events
                                                        && let Some(event) =
                                                            events.iter().rev().find(|e| e.clock < elapsed - 0.5)
                                                        {
                                                            let seek_clock = event.clock.to_absolute(battle_start);
                                                            let desc = format_timeline_event(event);
                                                            drop(state);
                                                            let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                                            toasts.lock().info(desc);
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
                                                    let target = (clock_secs - 10.0).max(GameClock(0.0));
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
                                                    let target = (clock_secs + 10.0).min(GameClock(game_dur));
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
                                                    let elapsed = clock_secs.to_elapsed(battle_start);
                                                    let state = shared_state.lock();
                                                    if let Some(ref events) = state.timeline_events
                                                        && let Some(event) =
                                                            events.iter().find(|e| e.clock > elapsed)
                                                        {
                                                            let seek_clock = event.clock.to_absolute(battle_start);
                                                            let desc = format_timeline_event(event);
                                                            drop(state);
                                                            let _ = command_tx.send(PlaybackCommand::Seek(seek_clock));
                                                            toasts.lock().info(desc);
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
                                                    let _ = command_tx.send(PlaybackCommand::Seek(GameClock(game_dur)));
                                                }
                                            }

                                            // Seek slider (flex_grow: 1.0 — fills remaining space)
                                            if let Some((_frame_idx, _total_frames, clock_secs, game_dur)) = frame_data
                                            {
                                                let mut seek_time = clock_secs.seconds();
                                                let mut seek_changed = false;
                                                tui.tui().style(grow_style.clone()).ui(|ui| {
                                                    ui.spacing_mut().slider_width = ui.available_width();
                                                    let slider = egui::Slider::new(&mut seek_time, 0.0..=game_dur)
                                                        .show_value(false);
                                                    seek_changed = ui.add(slider).changed();
                                                });
                                                if seek_changed {
                                                    let _ = command_tx.send(PlaybackCommand::Seek(GameClock(seek_time)));
                                                }

                                                let elapsed_secs = clock_secs.to_elapsed(battle_start).seconds().max(0.0) as u32;
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


                                            // Save as Video / Clipboard buttons — hidden for client
                                            // viewers and while a collab session is active.
                                            let session_is_active = shared_state.lock().collab_session_state.as_ref()
                                                .map(|ss| matches!(ss.lock().status, SessionStatus::Active | SessionStatus::Starting))
                                                .unwrap_or(false);
                                            if !session_is_active
                                            && let Some(ref video_export_data) = video_export_data {
                                            {
                                                let is_exporting = video_exporting.load(Ordering::Relaxed);
                                                let has_warning = gpu_encoder_warning.lock().is_some();
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .enabled_ui(!is_exporting && !has_warning)
                                                    .ui_add(egui::Button::new(
                                                        egui::RichText::new(icons::FLOPPY_DISK).size(18.0),
                                                    ));
                                                if btn.on_hover_text("Save as Video").clicked() {
                                                    let mut opts = options.clone();
                                                    // Apply per-ship range overrides for video export
                                                    let overrides = annotation_arc.lock().ship_range_overrides.clone();
                                                    if !overrides.is_empty() {
                                                        opts.ship_config_visibility = ShipConfigVisibility::Filtered(Arc::new(move |eid| {
                                                            overrides.get(&eid).copied()
                                                        }));
                                                    }
                                                    let default_name = format!("{}.mp4", video_export_data.replay_name);
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_file_name(&default_name)
                                                        .add_filter("MP4 Video", &["mp4"])
                                                        .save_file()
                                                    {
                                                        let status = wows_minimap_renderer::check_encoder();
                                                        let prefer_cpu = prefer_cpu_encoder.load(Ordering::Relaxed) || !status.gpu_available;
                                                        let action = PendingVideoExport::SaveToFile {
                                                            output_path: path.to_string_lossy().to_string(),
                                                            options: opts,
                                                            prefer_cpu,
                                                            actual_game_duration,
                                                        };
                                                        if prefer_cpu || status.gpu_available || suppress_gpu_warning.load(Ordering::Relaxed) {
                                                            execute_video_export(action, video_export_data, &toasts, &video_exporting, &video_export_progress);
                                                        } else {
                                                            *gpu_encoder_warning.lock() = Some(GpuEncoderWarning {
                                                                pending_action: action,
                                                                dont_show_again: false,
                                                            });
                                                        }
                                                    }
                                                }
                                            }

                                            // Render Video to Clipboard button
                                            {
                                                let is_exporting = video_exporting.load(Ordering::Relaxed);
                                                let has_warning = gpu_encoder_warning.lock().is_some();
                                                let btn = tui
                                                    .tui()
                                                    .style(fixed_style.clone())
                                                    .enabled_ui(!is_exporting && !has_warning)
                                                    .ui_add(egui::Button::new(
                                                        egui::RichText::new(icons::CLIPBOARD).size(18.0),
                                                    ));
                                                if btn.on_hover_text("Render Video to Clipboard").clicked() {
                                                    let mut opts = options.clone();
                                                    // Apply per-ship range overrides for video export
                                                    let overrides = annotation_arc.lock().ship_range_overrides.clone();
                                                    if !overrides.is_empty() {
                                                        opts.ship_config_visibility = ShipConfigVisibility::Filtered(Arc::new(move |eid| {
                                                            overrides.get(&eid).copied()
                                                        }));
                                                    }
                                                    let status = wows_minimap_renderer::check_encoder();
                                                    let prefer_cpu = prefer_cpu_encoder.load(Ordering::Relaxed) || !status.gpu_available;
                                                    let action = PendingVideoExport::CopyToClipboard { options: opts, prefer_cpu, actual_game_duration };
                                                    if prefer_cpu || status.gpu_available || suppress_gpu_warning.load(Ordering::Relaxed) {
                                                        execute_video_export(action, video_export_data, &toasts, &video_exporting, &video_export_progress);
                                                    } else {
                                                        *gpu_encoder_warning.lock() = Some(GpuEncoderWarning {
                                                            pending_action: action,
                                                            dont_show_again: false,
                                                        });
                                                    }
                                                }
                                            }
                                            } // end if video_export_data
                                             // end if !session_is_active

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
                                            let scroll_out = egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
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

                                                // Self ship range toggles
                                                if let Some(self_eid) = self_entity_id {
                                                    let mut filter = annotation_arc.lock().ship_range_overrides
                                                        .get(&self_eid).copied().unwrap_or_default();
                                                    let mut self_changed = false;
                                                    ui.indent("self_ranges", |ui| {
                                                        ui.label(egui::RichText::new("Self Ship Ranges").small());
                                                        self_changed |= ui.checkbox(&mut filter.detection, "Detection").changed();
                                                        self_changed |= ui.checkbox(&mut filter.main_battery, "Main Battery").changed();
                                                        self_changed |= ui.checkbox(&mut filter.secondary_battery, "Secondary").changed();
                                                        self_changed |= ui.checkbox(&mut filter.torpedo, "Torpedo").changed();
                                                        self_changed |= ui.checkbox(&mut filter.radar, "Radar").changed();
                                                        self_changed |= ui.checkbox(&mut filter.hydro, "Hydro").changed();
                                                    });
                                                    if self_changed {
                                                        let mut ann = annotation_arc.lock();
                                                        if !filter.any_enabled() {
                                                            ann.ship_range_overrides.remove(&self_eid);
                                                        } else {
                                                            ann.ship_range_overrides.insert(self_eid, filter);
                                                        }
                                                        // Auto-enable global show_ship_config when any range is on
                                                        if filter.any_enabled() && !opts.show_ship_config {
                                                            opts.show_ship_config = true;
                                                            changed = true;
                                                        }
                                                        // Auto-disable global when no ship has any range enabled
                                                        if ann.ship_range_overrides.is_empty() && opts.show_ship_config {
                                                            opts.show_ship_config = false;
                                                            changed = true;
                                                        }
                                                        broadcast_range_overrides(&ann.ship_range_overrides, &shared_state);

                                                        repaint = true;
                                                    }
                                                }

                                                changed |=
                                                    ui.checkbox(&mut opts.show_ship_names, "Ship Names").changed();
                                                changed |= ui
                                                    .checkbox(&mut opts.show_turret_direction, "Turret Direction")
                                                    .changed();
                                            });

                                            // ── Trail Settings ──
                                            ui.label(egui::RichText::new("Trail Settings").small().strong());
                                            ui.indent("trail_settings", |ui| {
                                                changed |= ui.checkbox(&mut opts.show_trails, "Heat Trail").changed();
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

                                            // ── Export Settings ──
                                            ui.label(egui::RichText::new("Export Settings").small().strong());
                                            ui.indent("export_settings", |ui| {
                                                let mut cpu = prefer_cpu_encoder.load(Ordering::Relaxed);
                                                if ui.checkbox(&mut cpu, "Prefer CPU Encoder").on_hover_text("Use software (CPU) encoder instead of GPU for video export").changed() {
                                                    prefer_cpu_encoder.store(cpu, Ordering::Relaxed);
                                                }
                                            });

                                            if changed {
                                                let mut state = shared_state.lock();
                                                // Broadcast diffs to collab peers if connected.
                                                if let Some(ref tx) = state.collab_local_tx {
                                                    use crate::collab::protocol::CollabRenderOptions;
                                                    let old = CollabRenderOptions::from_render_options(&state.options, state.show_dead_ships);
                                                    let new = CollabRenderOptions::from_render_options(&opts, show_dead);
                                                    for (field, value) in old.diff(&new) {
                                                        let _ = tx.send(crate::collab::peer::LocalEvent::DisplayToggle(field, value));
                                                    }
                                                }
                                                state.options = opts.clone();
                                                state.show_dead_ships = show_dead;
                                            }
                                            (opts, show_dead)
                                            });

                                            let (opts, show_dead) = scroll_out.inner;
                                            ui.separator();
                                            if ui.button("Save Defaults").clicked() {
                                                let mut saved = saved_from_render_options(&opts);
                                                saved.show_dead_ships = show_dead;
                                                saved.prefer_cpu_encoder = prefer_cpu_encoder.load(Ordering::Relaxed);
                                                // Persist self range flags from annotation overrides
                                                if let Some(self_eid) = self_entity_id {
                                                    let ann = annotation_arc.lock();
                                                    let filter = ann.ship_range_overrides
                                                        .get(&self_eid).copied().unwrap_or_default();
                                                    saved.set_self_range_filter(&filter);
                                                }
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
                                                        if let Some(events) = &state.timeline_events
                                                            && ui.small_button("Copy").clicked() {
                                                                let text: String = events
                                                                    .iter()
                                                                    .map(format_timeline_event)
                                                                    .collect::<Vec<_>>()
                                                                    .join("\n");
                                                                ui.ctx().copy_text(text);
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
                                                            let mins = event.clock.seconds() as u32 / 60;
                                                            let secs = event.clock.seconds() as u32 % 60;
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
                                                                    command_tx.send(PlaybackCommand::Seek(event.clock.to_absolute(battle_start)));
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

                    // GPU encoder warning dialog
                    if gpu_encoder_warning.lock().is_some() {
                        let mut close_dialog = false;
                        let mut proceed = false;

                        egui::Window::new("GPU Video Encoder Unavailable")
                            .collapsible(false)
                            .resizable(false)
                            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                            .show(ctx, |ui| {
                                ui.label(
                                    "Could not find a supported GPU video encoder. \
                                     Video export will fall back to CPU encoding, \
                                     which will be significantly slower."
                                );
                                ui.add_space(8.0);
                                let mut warning = gpu_encoder_warning.lock();
                                if let Some(w) = warning.as_mut() {
                                    ui.checkbox(&mut w.dont_show_again, "Don't show this again");
                                }
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Ok").clicked() {
                                        proceed = true;
                                        close_dialog = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        close_dialog = true;
                                    }
                                });
                            });

                        if close_dialog {
                            let warning = gpu_encoder_warning.lock().take();
                            if let Some(w) = warning {
                                if w.dont_show_again {
                                    suppress_gpu_warning.store(true, Ordering::Relaxed);
                                }
                                if proceed
                                    && let Some(ref video_export_data) = video_export_data {
                                        execute_video_export(w.pending_action, video_export_data, &toasts, &video_exporting, &video_export_progress);
                                    }
                            }
                        }
                    }



                    toasts.lock().show(ctx);
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    window_open.store(false, Ordering::Relaxed);
                    let _ = command_tx.send(PlaybackCommand::Stop);
                    ctx.request_repaint();
                } else if status_is_loading {
                    // Keep the viewport alive while loading so it notices the
                    // Loading→Ready transition. Only repaint this viewport —
                    // do NOT wake the parent, which causes event-loop starvation.
                    ctx.request_repaint();
                } else if playing || repaint
                    || shared_state.lock().collab_session_state.is_some()
                {
                    // Repaint both this viewport AND the parent so sibling
                    // viewports (e.g. armor viewer) also update in realtime.
                    ctx.request_repaint();
                    parent_ctx.request_repaint();
                }
            },
        );
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use egui::mutex::Mutex;
use egui::{Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke, TextureHandle, Vec2};

use minimap_renderer::assets;
use minimap_renderer::draw_command::DrawCommand;
use minimap_renderer::map_data::MapInfo;
use minimap_renderer::renderer::{MinimapRenderer, RenderOptions};
use minimap_renderer::{CANVAS_HEIGHT, HUD_HEIGHT, MINIMAP_SIZE, MinimapPos};

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::settings::SavedRenderOptions;
use crate::wows_data::SharedWoWsData;

// ─── Constants ───────────────────────────────────────────────────────────────

const TOTAL_FRAMES: usize = 1800;
const FPS: f64 = 30.0;
const ICON_SIZE: f32 = 24.0;

// ─── Zoom/Pan State ─────────────────────────────────────────────────────────

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
}

// ─── Asset Cache ─────────────────────────────────────────────────────────────

/// RGBA image data: (pixels, width, height).
type RgbaAsset = (Vec<u8>, u32, u32);

/// Cached assets shared across renderer instances. Lives in TabState.
/// Ship and plane icons are game-global; map data is per-map.
pub struct RendererAssetCache {
    ship_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    plane_icons: Option<Arc<HashMap<String, RgbaAsset>>>,
    maps: HashMap<String, CachedMapData>,
}

struct CachedMapData {
    image: Option<Arc<RgbaAsset>>,
    info: Option<MapInfo>,
}

impl Default for RendererAssetCache {
    fn default() -> Self {
        Self { ship_icons: None, plane_icons: None, maps: HashMap::new() }
    }
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
    }
}

// ─── Commands & Shared State ─────────────────────────────────────────────────

/// Commands sent from the UI thread to the background playback thread.
pub enum PlaybackCommand {
    Play,
    Pause,
    Seek(usize),
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
}

/// egui TextureHandles created on the UI thread.
struct RendererTextures {
    map_texture: Option<TextureHandle>,
    ship_icons: HashMap<String, TextureHandle>,
    plane_icons: HashMap<String, TextureHandle>,
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
    /// Viewport egui context, set by the UI thread on first draw.
    /// Used by the background thread to request repaints after frame updates.
    pub viewport_ctx: Option<egui::Context>,
}

/// The cloneable viewport handle stored in TabState.
#[derive(Clone)]
pub struct ReplayRendererViewer {
    pub title: Arc<String>,
    pub open: Arc<AtomicBool>,
    shared_state: Arc<Mutex<SharedRendererState>>,
    command_tx: mpsc::Sender<PlaybackCommand>,
    textures: Arc<Mutex<Option<RendererTextures>>>,
    /// When set, the main app loop should save these as default render options.
    pub pending_defaults_save: Arc<Mutex<Option<SavedRenderOptions>>>,
    /// Timed status message shown in the viewport (message, expiry time).
    status_message: Arc<Mutex<Option<(String, std::time::Instant)>>>,
    /// Whether a video export is currently in progress.
    video_exporting: Arc<AtomicBool>,
    /// Data needed for video export (cloned from launch params).
    video_export_data: Arc<VideoExportData>,
    /// Zoom and pan state for the viewport. Persists across frames.
    zoom_pan: Arc<Mutex<ViewportZoomPan>>,
}

/// Data retained for video export. Cloned once at launch time.
struct VideoExportData {
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
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
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    initial_options: RenderOptions,
) -> ReplayRendererViewer {
    let (command_tx, command_rx) = mpsc::channel();
    let shared_state = Arc::new(Mutex::new(SharedRendererState {
        status: RendererStatus::Loading,
        frame: None,
        assets: None,
        playing: false,
        speed: 1.0,
        options: initial_options,
        viewport_ctx: None,
    }));

    let video_export_data = Arc::new(VideoExportData {
        raw_meta: raw_meta.clone(),
        packet_data: packet_data.clone(),
        map_name: map_name.clone(),
        game_duration,
        wows_data: wows_data.clone(),
        asset_cache: Arc::clone(&asset_cache),
    });

    let viewer = ReplayRendererViewer {
        title: Arc::new("Replay Renderer".to_string()),
        open: Arc::new(AtomicBool::new(true)),
        shared_state: Arc::clone(&shared_state),
        command_tx,
        textures: Arc::new(Mutex::new(None)),
        pending_defaults_save: Arc::new(Mutex::new(None)),
        status_message: Arc::new(Mutex::new(None)),
        video_exporting: Arc::new(AtomicBool::new(false)),
        video_export_data,
        zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
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
    // 1. Get file tree, pkg loader, and game metadata from the app
    let (file_tree, pkg_loader, game_metadata) = {
        let data = wows_data.read();
        let gm = match data.game_metadata.clone() {
            Some(gm) => gm,
            None => {
                shared_state.lock().status = RendererStatus::Error("Game metadata not loaded".to_string());
                return;
            }
        };
        (data.file_tree.clone(), Arc::clone(&data.pkg_loader), gm)
    };

    // 2. Load visual assets (cached across renderer instances)
    let map_info = {
        let mut cache = asset_cache.lock();
        let ship_icons = cache.get_or_load_ship_icons(&file_tree, &pkg_loader);
        let plane_icons = cache.get_or_load_plane_icons(&file_tree, &pkg_loader);
        let (map_image, map_info) = cache.get_or_load_map(&map_name, &file_tree, &pkg_loader);

        shared_state.lock().assets = Some(ReplayRendererAssets { map_image, ship_icons, plane_icons });

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
    let mut controller = BattleController::new(&replay_file.meta, &*game_metadata);
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    let mut renderer = MinimapRenderer::new(map_info.clone(), &*game_metadata, RenderOptions::default());

    // Parse all packets, tracking frame boundaries
    let frame_duration = if game_duration > 0.0 { game_duration / TOTAL_FRAMES as f32 } else { 1.0 / FPS as f32 };

    // Pre-parse: build a mapping of packet offsets to clock times
    // so we can efficiently seek by re-parsing
    let mut frame_snapshots: Vec<FrameSnapshot> = Vec::with_capacity(TOTAL_FRAMES);
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

                    let target_frame = (prev_clock.seconds() / frame_duration) as i64;
                    while last_rendered_frame < target_frame && last_rendered_frame < TOTAL_FRAMES as i64 - 1 {
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
                                total_frames: TOTAL_FRAMES,
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
        let target_frame = (prev_clock.seconds() / frame_duration) as i64;
        while last_rendered_frame < target_frame && last_rendered_frame < TOTAL_FRAMES as i64 - 1 {
            last_rendered_frame += 1;
            frame_snapshots.push(FrameSnapshot { packet_offset: full_packet_data.len(), clock: prev_clock.seconds() });
        }
    }
    controller.finish();

    let actual_total_frames = frame_snapshots.len();

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
    let mut speed: f32 = 1.0;
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
    let mut live_controller = BattleController::new(&live_replay.meta, &*game_metadata);
    let mut live_renderer = MinimapRenderer::new(map_info.clone(), &*game_metadata, RenderOptions::default());

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
                    if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                        renderer.populate_players(controller);
                        renderer.update_squadron_info(controller);
                    }
                    if prev_clock.seconds() == 0.0 {
                        prev_clock = packet.clock;
                    } else {
                        prev_clock = packet.clock;
                    }
                    controller.process(&packet);
                    remaining = rest;
                }
                Err(_) => break,
            }
        }

        renderer.populate_players(controller);
        renderer.update_squadron_info(controller);
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
            live_controller = BattleController::new(&live_replay.meta, &*game_metadata);
            live_renderer = MinimapRenderer::new(map_info.clone(), &*game_metadata, RenderOptions::default());
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
                PlaybackCommand::Seek(frame) => {
                    let target = frame.min(actual_total_frames.saturating_sub(1));
                    current_frame = target;
                    let target_clock = if current_frame < frame_snapshots.len() {
                        frame_snapshots[current_frame].clock
                    } else {
                        game_duration
                    };

                    rebuild_live_state!(target_clock);

                    let commands = live_renderer.draw_frame(&live_controller);
                    shared_state.lock().frame = Some(PlaybackFrame {
                        commands,
                        clock_seconds: target_clock,
                        frame_index: current_frame,
                        total_frames: actual_total_frames,
                        game_duration,
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
            let frames_to_advance = dt * FPS as f32 * speed;

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
                    game_duration
                };

                rebuild_live_state!(target_clock);

                let commands = live_renderer.draw_frame(&live_controller);
                shared_state.lock().frame = Some(PlaybackFrame {
                    commands,
                    clock_seconds: target_clock,
                    frame_index: current_frame,
                    total_frames: actual_total_frames,
                    game_duration,
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

// ─── Video Export ────────────────────────────────────────────────────────────

/// Spawn a background thread that renders the replay to an MP4 video file
/// using the software renderer (`ImageTarget`) and `VideoEncoder`.
fn save_as_video(
    output_path: String,
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    map_name: String,
    game_duration: f32,
    options: RenderOptions,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    status_message: Arc<Mutex<Option<(String, std::time::Instant)>>>,
    video_exporting: Arc<AtomicBool>,
) {
    video_exporting.store(true, Ordering::Relaxed);
    *status_message.lock() =
        Some(("Exporting video...".to_string(), std::time::Instant::now() + std::time::Duration::from_secs(3600)));

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
                *status_message.lock() = Some((
                    format!("Video saved to {}", output_path),
                    std::time::Instant::now() + std::time::Duration::from_secs(8),
                ));
            }
            Err(e) => {
                *status_message.lock() = Some((
                    format!("Video export failed: {}", e),
                    std::time::Instant::now() + std::time::Duration::from_secs(10),
                ));
            }
        }
        video_exporting.store(false, Ordering::Relaxed);
    });
}

/// Blocking implementation of the video export.
fn render_video_blocking(
    output_path: &str,
    raw_meta: &[u8],
    packet_data: &[u8],
    map_name: &str,
    game_duration: f32,
    options: RenderOptions,
    wows_data: &SharedWoWsData,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
) -> anyhow::Result<()> {
    use minimap_renderer::drawing::ImageTarget;
    use minimap_renderer::video::VideoEncoder;

    // Get game metadata and load assets for the software renderer
    let (file_tree, pkg_loader, game_metadata) = {
        let data = wows_data.read();
        let gm = data.game_metadata.clone().ok_or_else(|| anyhow::anyhow!("Game metadata not loaded"))?;
        (data.file_tree.clone(), Arc::clone(&data.pkg_loader), gm)
    };

    // Load assets — reuse cached raw RGBA data and convert to image types
    let (map_image_rgb, ship_icons_rgba, plane_icons_rgba, map_info) = {
        let mut cache = asset_cache.lock();
        let ship_raw = cache.get_or_load_ship_icons(&file_tree, &pkg_loader);
        let plane_raw = cache.get_or_load_plane_icons(&file_tree, &pkg_loader);
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

        let map_image = map_raw.as_ref().and_then(|arc| {
            let (data, w, h) = &**arc;
            // Cached data is RGBA, convert to RGB for ImageTarget
            let rgba = image::RgbaImage::from_raw(*w, *h, data.clone())?;
            Some(image::DynamicImage::ImageRgba8(rgba).into_rgb8())
        });

        (map_image, ship_icons, plane_icons, map_info)
    };

    // Build replay parser components
    let replay_file = ReplayFile::from_decrypted_parts(raw_meta.to_vec(), packet_data.to_vec())
        .map_err(|e| anyhow::anyhow!("Failed to parse replay: {:?}", e))?;

    let mut controller = BattleController::new(&replay_file.meta, &*game_metadata);
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    let mut renderer = MinimapRenderer::new(map_info, &*game_metadata, options);
    let mut target = ImageTarget::new(map_image_rgb, ship_icons_rgba, plane_icons_rgba);
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
                    encoder.advance_clock(prev_clock, &controller, &mut renderer, &mut target);
                }
                if prev_clock.seconds() == 0.0 {
                    prev_clock = packet.clock;
                } else {
                    prev_clock = packet.clock;
                }
                controller.process(&packet);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    controller.finish();
    renderer.populate_players(&controller);
    renderer.update_squadron_info(&controller);
    encoder.finish(&controller, &mut renderer, &mut target)?;

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
fn draw_ship_labels(
    ctx: &egui::Context,
    center: Pos2,
    scale: f32,
    player_name: Option<&str>,
    ship_name: Option<&str>,
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

    // Position lines above the icon
    let base_y = center.y - 14.0 * scale - line_count as f32 * line_height;
    let mut cur_y = base_y;

    if let Some(name) = player_name {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), label_color));
        let text_w = galley.size().x;
        let tx = center.x - text_w / 2.0;
        let shadow_galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx + shadow_offset, cur_y + shadow_offset), shadow_galley, shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx, cur_y), galley, label_color));
        cur_y += line_height;
    }

    if let Some(name) = ship_name {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), label_color));
        let text_w = galley.size().x;
        let tx = center.x - text_w / 2.0;
        let shadow_galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx + shadow_offset, cur_y + shadow_offset), shadow_galley, shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx, cur_y), galley, label_color));
    }
}

/// Check whether a DrawCommand should be drawn given the current RenderOptions.
/// This runs on the UI thread so option changes are instant (no cross-thread round-trip).
fn should_draw_command(cmd: &DrawCommand, opts: &RenderOptions) -> bool {
    match cmd {
        DrawCommand::ShotTracer { .. } => opts.show_tracers,
        DrawCommand::Torpedo { .. } => opts.show_torpedoes,
        DrawCommand::Smoke { .. } => opts.show_smoke,
        DrawCommand::Ship { .. } => true, // ships always drawn; name visibility handled below
        DrawCommand::HealthBar { .. } => opts.show_hp_bars,
        DrawCommand::DeadShip { .. } => true,
        DrawCommand::Plane { .. } => opts.show_planes,
        DrawCommand::ScoreBar { .. } => opts.show_score,
        DrawCommand::Timer { .. } => opts.show_timer,
        DrawCommand::KillFeed { .. } => opts.show_kill_feed,
        DrawCommand::CapturePoint { .. } => opts.show_capture_points,
        DrawCommand::Building { .. } => opts.show_buildings,
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

        DrawCommand::Ship { pos, yaw, species, color, visibility, opacity, is_self, player_name, ship_name } => {
            let center = transform.minimap_to_screen(pos);
            let icon_size = transform.scale_distance(ICON_SIZE);
            if let Some(sp) = species {
                let variant_key = match (*visibility, *is_self) {
                    (minimap_renderer::ShipVisibility::Visible, true) => format!("{}_self", sp),
                    (minimap_renderer::ShipVisibility::Visible, false) => sp.clone(),
                    (minimap_renderer::ShipVisibility::MinimapOnly, _) => {
                        format!("{}_last_visible", sp)
                    }
                    (minimap_renderer::ShipVisibility::Undetected, _) => {
                        format!("{}_invisible", sp)
                    }
                };

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
            draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, &mut shapes);
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
            let pname = if opts.show_player_names { player_name.as_deref() } else { None };
            let sname = if opts.show_ship_names { ship_name.as_deref() } else { None };
            draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, &mut shapes);
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

        DrawCommand::ScoreBar { team0, team1, team0_color, team1_color } => {
            let canvas_w = transform.screen_canvas_width();
            let total = (*team0 + *team1).max(1) as f32;
            let team0_width = (*team0 as f32 / total) * canvas_w;
            let bar_height = 20.0 * ws;

            let bar_origin = transform.hud_pos(0.0, 0.0);
            if team0_width > 0.0 {
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(bar_origin, Vec2::new(team0_width, bar_height)),
                    CornerRadius::ZERO,
                    color_from_rgb(*team0_color),
                ));
            }
            if team0_width < canvas_w {
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(
                        Pos2::new(bar_origin.x + team0_width, bar_origin.y),
                        Vec2::new(canvas_w - team0_width, bar_height),
                    ),
                    CornerRadius::ZERO,
                    color_from_rgb(*team1_color),
                ));
            }

            let font = FontId::proportional(14.0 * ws);
            let t0_text = format!("{}", team0);
            let t1_text = format!("{}", team1);

            let t0_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text, font.clone(), Color32::WHITE));
            shapes.push(Shape::galley(
                Pos2::new(bar_origin.x + 5.0 * ws, bar_origin.y + 3.0 * ws),
                t0_galley,
                Color32::WHITE,
            ));

            let t1_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text, font, Color32::WHITE));
            let t1_w = t1_galley.size().x;
            shapes.push(Shape::galley(
                Pos2::new(bar_origin.x + canvas_w - t1_w - 5.0 * ws, bar_origin.y + 3.0 * ws),
                t1_galley,
                Color32::WHITE,
            ));
        }

        DrawCommand::Timer { seconds } => {
            let canvas_w = transform.screen_canvas_width();
            let total_secs = seconds.max(0.0) as u32;
            let minutes = total_secs / 60;
            let secs = total_secs % 60;
            let text = format!("{:02}:{:02}", minutes, secs);

            let font = FontId::proportional(16.0 * ws);
            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text, font, Color32::WHITE));
            let text_w = galley.size().x;
            let pos = transform.hud_pos(0.0, 3.0);
            shapes.push(Shape::galley(Pos2::new(pos.x + canvas_w / 2.0 - text_w / 2.0, pos.y), galley, Color32::WHITE));
        }

        DrawCommand::KillFeed { entries } => {
            let canvas_w = transform.screen_canvas_width();
            let font = FontId::proportional(11.0 * ws);
            let line_h = 14.0 * ws;
            let start = transform.hud_pos(0.0, 25.0);
            let mut y = start.y;
            for (killer, victim) in entries.iter().take(5) {
                let text = format!("{} > {}", killer, victim);
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), Color32::WHITE));
                let text_w = galley.size().x;
                shapes.push(Shape::galley(
                    Pos2::new(start.x + canvas_w - text_w - 5.0 * ws, y),
                    galley,
                    Color32::WHITE,
                ));
                y += line_h;
            }
        }

        DrawCommand::CapturePoint { pos, radius, color, alpha, label, progress, invader_color } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);

            shapes.push(Shape::circle_filled(center, r, color_from_rgba(*color, *alpha)));

            if *progress > 0.001 {
                if let Some(inv_color) = invader_color {
                    let fill_alpha = (*alpha + 0.10).min(1.0);
                    let sweep = *progress * std::f32::consts::TAU;
                    let segments = 64;
                    let start_angle = -std::f32::consts::FRAC_PI_2;
                    let pie_color = color_from_rgba(*inv_color, fill_alpha);

                    let mut mesh = egui::Mesh::default();
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: center,
                        uv: egui::pos2(0.0, 0.0),
                        color: pie_color,
                    });
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
            }

            let outline_color = if *progress > 0.001 {
                invader_color.map(|c| color_from_rgb(c)).unwrap_or_else(|| color_from_rgb(*color))
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
    }

    shapes
}

// ─── Texture Upload ──────────────────────────────────────────────────────────

fn upload_textures(ctx: &egui::Context, assets: &ReplayRendererAssets) -> RendererTextures {
    let map_texture = assets.map_image.as_ref().map(|asset| {
        let (ref data, w, h) = **asset;
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], data);
        ctx.load_texture("replay_map", image, egui::TextureOptions::LINEAR)
    });

    let ship_icons: HashMap<String, TextureHandle> = assets
        .ship_icons
        .iter()
        .map(|(key, (data, w, h))| {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
            let handle = ctx.load_texture(format!("ship_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let plane_icons: HashMap<String, TextureHandle> = assets
        .plane_icons
        .iter()
        .map(|(key, (data, w, h))| {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
            let handle = ctx.load_texture(format!("plane_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    RendererTextures { map_texture, ship_icons, plane_icons }
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
        let status_message = self.status_message.clone();
        let video_exporting = self.video_exporting.clone();
        let video_export_data = self.video_export_data.clone();
        let zoom_pan_arc = self.zoom_pan.clone();

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(&*self.title),
            egui::ViewportBuilder::default()
                .with_title(&*self.title)
                .with_inner_size([800.0, 900.0])
                .with_min_inner_size([400.0, 450.0]),
            move |ctx, _class| {
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

                    // Controls bar
                    let controls_resp = ui.horizontal(|ui| {
                        if playing {
                            if ui.button("\u{23F8} Pause").clicked() {
                                let _ = command_tx.send(PlaybackCommand::Pause);
                                shared_state.lock().playing = false;
                            }
                        } else {
                            if ui.button("\u{25B6} Play").clicked() {
                                let _ = command_tx.send(PlaybackCommand::Play);
                                shared_state.lock().playing = true;
                            }
                        }

                        if let Some((frame_idx, total_frames, clock_secs, _game_dur)) = frame_data {
                            let mut seek_frame = frame_idx as f32;
                            let slider =
                                egui::Slider::new(&mut seek_frame, 0.0..=(total_frames.saturating_sub(1)) as f32)
                                    .show_value(false);
                            if ui.add(slider).changed() {
                                let _ = command_tx.send(PlaybackCommand::Seek(seek_frame as usize));
                            }

                            let total_secs = clock_secs as u32;
                            let mins = total_secs / 60;
                            let secs = total_secs % 60;
                            ui.label(format!("{:02}:{:02}", mins, secs));
                        }

                        let mut current_speed = speed;
                        egui::ComboBox::from_id_salt("speed")
                            .selected_text(format!("{:.1}x", current_speed))
                            .width(60.0)
                            .show_ui(ui, |ui| {
                                for s in [0.25, 0.5, 1.0, 2.0, 4.0, 8.0] {
                                    if ui.selectable_value(&mut current_speed, s, format!("{:.1}x", s)).changed() {
                                        let _ = command_tx.send(PlaybackCommand::SetSpeed(s));
                                        shared_state.lock().speed = s;
                                    }
                                }
                            });

                        ui.separator();

                        // Settings popup button (response returned from closure)
                        let settings_btn_resp = ui.button("\u{2699} Settings");

                        // Save as Video button
                        {
                            let is_exporting = video_exporting.load(Ordering::Relaxed);
                            if ui.add_enabled(!is_exporting, egui::Button::new("Save as Video")).clicked() {
                                let opts = options.clone();
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name("replay.mp4")
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
                                        Arc::clone(&status_message),
                                        Arc::clone(&video_exporting),
                                    );
                                }
                            }
                        }

                        ui.separator();

                        // Zoom slider
                        {
                            let mut zp = zoom_pan_arc.lock();
                            let mut zoom_val = zp.zoom;
                            ui.label("Zoom:");
                            let slider = egui::Slider::new(&mut zoom_val, 1.0..=10.0_f32)
                                .logarithmic(true)
                                .max_decimals(1)
                                .suffix("x");
                            if ui.add(slider).changed() {
                                // Zoom toward center of visible area
                                let old_zoom = zp.zoom;
                                let center_x = zp.pan.x + MINIMAP_SIZE as f32 / 2.0;
                                let center_y = zp.pan.y + MINIMAP_SIZE as f32 / 2.0;
                                let minimap_cx = center_x / old_zoom;
                                let minimap_cy = center_y / old_zoom;
                                zp.pan.x = minimap_cx * zoom_val - MINIMAP_SIZE as f32 / 2.0;
                                zp.pan.y = minimap_cy * zoom_val - MINIMAP_SIZE as f32 / 2.0;
                                zp.zoom = zoom_val;
                                // Rough clamp; authoritative clamp happens below after available_size is known
                                zp.pan.x = zp.pan.x.max(0.0);
                                zp.pan.y = zp.pan.y.max(0.0);
                            }
                            if ui.button("Reset").clicked() {
                                zp.zoom = 1.0;
                                zp.pan = Vec2::ZERO;
                            }
                        }

                        settings_btn_resp
                    });
                    let settings_btn_resp = controls_resp.inner;

                    // Settings popup (rendered outside the horizontal bar)
                    egui::Popup::from_toggle_button_response(&settings_btn_resp)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_min_width(180.0);
                            let mut opts = options.clone();
                            let mut changed = false;

                            // Alphabetical order
                            changed |= ui.checkbox(&mut opts.show_buildings, "Buildings").changed();
                            changed |= ui.checkbox(&mut opts.show_capture_points, "Capture Points").changed();
                            changed |= ui.checkbox(&mut opts.show_hp_bars, "HP Bars").changed();
                            changed |= ui.checkbox(&mut opts.show_kill_feed, "Kill Feed").changed();
                            changed |= ui.checkbox(&mut opts.show_planes, "Planes").changed();
                            changed |= ui.checkbox(&mut opts.show_player_names, "Player Names").changed();
                            changed |= ui.checkbox(&mut opts.show_score, "Score").changed();
                            changed |= ui.checkbox(&mut opts.show_ship_names, "Ship Names").changed();
                            changed |= ui.checkbox(&mut opts.show_smoke, "Smoke").changed();
                            changed |= ui.checkbox(&mut opts.show_timer, "Timer").changed();
                            changed |= ui.checkbox(&mut opts.show_torpedoes, "Torpedoes").changed();
                            changed |= ui.checkbox(&mut opts.show_tracers, "Tracers").changed();

                            if changed {
                                shared_state.lock().options = opts.clone();
                            }

                            ui.separator();
                            if ui.button("Save Defaults").clicked() {
                                *pending_save.lock() = Some(saved_from_render_options(&opts));
                            }
                        });

                    // Canvas area — fill all available space.
                    // window_scale maps logical canvas pixels to screen pixels.
                    // We use the full available rect so the viewport expands when
                    // the window is resized (showing more map area when zoomed).
                    let logical_canvas = Vec2::new(MINIMAP_SIZE as f32, CANVAS_HEIGHT as f32);
                    let available = ui.available_size();
                    let window_scale = (available.x / logical_canvas.x).min(available.y / logical_canvas.y).max(0.1);
                    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
                    let origin = response.rect.min;

                    // Zoom/pan input handling
                    let mut zp = zoom_pan_arc.lock();
                    let mut zoom_changed = false;

                    // Scroll-wheel zoom (cursor-centered)
                    if response.hovered() {
                        let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
                        if scroll_delta != 0.0 {
                            let zoom_speed = 0.002;
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

                    // Drag-to-pan
                    if response.dragged_by(egui::PointerButton::Primary)
                        || response.dragged_by(egui::PointerButton::Middle)
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
                    // Visible area in zoomed-minimap-pixel space:
                    let visible_w = available.x / window_scale;
                    let visible_h = (available.y - HUD_HEIGHT as f32 * window_scale) / window_scale;
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

                    // Grab cursor when zoomed in
                    if current_zoom > 1.01 {
                        if response.dragged() {
                            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                        } else if response.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }

                    // Request repaint if zoom/pan changed while paused
                    if zoom_changed && !playing {
                        ctx.request_repaint();
                    }

                    // Draw dark background
                    painter.rect_filled(response.rect, CornerRadius::ZERO, Color32::from_rgb(20, 25, 35));

                    // Clipped painter for map-region content (below HUD, full painter width/height)
                    let hud_screen_height = HUD_HEIGHT as f32 * window_scale;
                    let map_clip = Rect::from_min_max(
                        Pos2::new(response.rect.min.x, response.rect.min.y + hud_screen_height),
                        response.rect.max,
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
                        let state = shared_state.lock();
                        if let Some(ref frame) = state.frame {
                            // Separate HUD and map commands so HUD draws on unclipped painter
                            for cmd in &frame.commands {
                                if !should_draw_command(cmd, &options) {
                                    continue;
                                }
                                let is_hud = matches!(
                                    cmd,
                                    DrawCommand::ScoreBar { .. }
                                        | DrawCommand::Timer { .. }
                                        | DrawCommand::KillFeed { .. }
                                );
                                let cmd_shapes = draw_command_to_shapes(cmd, &transform, textures, ctx, &options);
                                let target_painter = if is_hud { &painter } else { &map_painter };
                                for shape in cmd_shapes {
                                    target_painter.add(shape);
                                }
                            }
                        }
                        drop(state);
                    }
                    drop(tex_guard);

                    // Status toast (bottom-left)
                    {
                        let mut msg_guard = status_message.lock();
                        if let Some((ref text, expiry)) = *msg_guard {
                            if std::time::Instant::now() < expiry {
                                ui.separator();
                                ui.label(text.as_str());
                                ctx.request_repaint_after_secs(1.0);
                            } else {
                                *msg_guard = None;
                            }
                        }
                    }
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    window_open.store(false, Ordering::Relaxed);
                    let _ = command_tx.send(PlaybackCommand::Stop);
                } else if playing {
                    ctx.request_repaint();
                }
            },
        );
    }
}

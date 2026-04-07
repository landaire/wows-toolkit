use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use rootcause::report;
use wows_minimap_renderer::RenderProgress;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use super::PendingVideoExport;
use super::RendererAssetCache;
use super::VideoExportData;
use crate::data::wows_data::SharedWoWsData;

/// Execute a pending video export action.
pub(super) fn execute_video_export(
    action: PendingVideoExport,
    video_export_data: &Arc<VideoExportData>,
    toasts: &crate::tab_state::SharedToasts,
    video_exporting: &Arc<AtomicBool>,
    video_export_progress: &Arc<Mutex<Option<RenderProgress>>>,
) {
    // Clear any stale progress from a previous export
    *video_export_progress.lock() = None;

    match action {
        PendingVideoExport::SaveToFile { output_path, options, prefer_cpu, actual_game_duration } => {
            save_as_video(
                output_path,
                video_export_data.raw_meta.clone(),
                video_export_data.packet_data.clone(),
                video_export_data.map_name.clone(),
                video_export_data.game_duration,
                options,
                video_export_data.wows_data.clone(),
                Arc::clone(&video_export_data.asset_cache),
                Arc::clone(toasts),
                Arc::clone(video_exporting),
                Arc::clone(video_export_progress),
                prefer_cpu,
                actual_game_duration,
            );
        }
        PendingVideoExport::CopyToClipboard { options, prefer_cpu, actual_game_duration } => {
            let file_name = format!("{}.mp4", video_export_data.replay_name);
            render_video_to_clipboard(
                file_name,
                Arc::clone(video_export_data),
                options,
                Arc::clone(toasts),
                Arc::clone(video_exporting),
                Arc::clone(video_export_progress),
                prefer_cpu,
                actual_game_duration,
            );
        }
    }
}

/// Spawn a background thread that renders the replay to an MP4 video file
/// using the software renderer (`ImageTarget`) and `VideoEncoder`.
#[allow(clippy::too_many_arguments)]
pub(super) fn save_as_video(
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
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    prefer_cpu: bool,
    actual_game_duration: Option<f32>,
) {
    video_exporting.store(true, Ordering::Relaxed);

    crate::util::thread::spawn_logged("video-export", move || {
        let result = render_video_blocking(
            &output_path,
            &raw_meta,
            &packet_data,
            &map_name,
            game_duration,
            options,
            &wows_data,
            &asset_cache,
            &video_export_progress,
            prefer_cpu,
            actual_game_duration,
        );

        match result {
            Ok(()) => {
                toasts.lock().success(format!("Video saved to {}", output_path));
            }
            Err(e) => {
                toasts.lock().error(format!("Video export failed: {}", e));
            }
        }
        *video_export_progress.lock() = None;
        video_exporting.store(false, Ordering::Relaxed);
    });
}

/// Spawn a background thread that renders the replay to a temporary MP4 file,
/// then copies it to the clipboard.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_video_to_clipboard(
    file_name: String,
    export_data: Arc<VideoExportData>,
    options: RenderOptions,
    toasts: crate::tab_state::SharedToasts,
    video_exporting: Arc<AtomicBool>,
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    prefer_cpu: bool,
    actual_game_duration: Option<f32>,
) {
    video_exporting.store(true, Ordering::Relaxed);

    crate::util::thread::spawn_logged("video-export-images", move || {
        let temp_dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => {
                toasts.lock().error(format!("Failed to create temp dir: {e}"));
                *video_export_progress.lock() = None;
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
            &video_export_progress,
            prefer_cpu,
            actual_game_duration,
        );

        match result {
            Ok(()) => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set().file_list(&[output_path]);
                    // Persist the temp dir so the file remains for clipboard consumers.
                    // The OS will clean it up on reboot.
                    let _ = temp_dir.keep();
                    toasts.lock().success("Video copied to clipboard");
                } else {
                    toasts.lock().error("Failed to open clipboard");
                }
            }
            Err(e) => {
                toasts.lock().error(format!("Video export failed: {}", e));
            }
        }
        *video_export_progress.lock() = None;
        video_exporting.store(false, Ordering::Relaxed);
    });
}

/// Information about a single replay to be rendered in a batch.
pub struct BatchReplayInfo {
    pub raw_meta: Vec<u8>,
    pub packet_data: Vec<u8>,
    pub map_name: String,
    pub replay_name: String,
    pub game_duration: f32,
    pub wows_data: SharedWoWsData,
}

/// Shared helper: render a list of replays sequentially, updating progress.
/// Returns (succeeded_count, failed_count, output_paths).
fn render_batch(
    replays: &[BatchReplayInfo],
    output_dir: &std::path::Path,
    options: &RenderOptions,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    progress: &Arc<Mutex<crate::task::BatchVideoExportProgress>>,
    prefer_cpu: bool,
) -> (usize, usize, Vec<std::path::PathBuf>) {
    let mut succeeded_paths = Vec::new();
    let mut failed = 0usize;
    let mut completed_frames: u64 = 0;

    for (i, replay) in replays.iter().enumerate() {
        {
            let mut p = progress.lock();
            p.completed_frames = completed_frames;
            p.current_index = i;
            p.current_name = replay.replay_name.clone();
        }

        let output_path = output_dir.join(format!("{}.mp4", replay.replay_name));
        let output_str = output_path.to_string_lossy().to_string();

        let frames_before = completed_frames;
        let per_replay_progress: Arc<Mutex<Option<RenderProgress>>> = Arc::new(Mutex::new(None));
        let stop_flag = Arc::new(AtomicBool::new(false));

        let progress_thread = {
            let progress = Arc::clone(progress);
            let per_replay_progress = Arc::clone(&per_replay_progress);
            let stop_flag = Arc::clone(&stop_flag);
            std::thread::spawn(move || {
                while !stop_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if let Some(ref p) = *per_replay_progress.lock() {
                        progress.lock().completed_frames = frames_before + p.current;
                    }
                }
            })
        };

        let result = render_video_blocking(
            &output_str,
            &replay.raw_meta,
            &replay.packet_data,
            &replay.map_name,
            replay.game_duration,
            options.clone(),
            &replay.wows_data,
            asset_cache,
            &per_replay_progress,
            prefer_cpu,
            None,
        );

        let estimated_frames = (replay.game_duration * 7.0) as u64;
        completed_frames += estimated_frames;

        stop_flag.store(true, Ordering::Relaxed);
        let _ = progress_thread.join();

        match result {
            Ok(()) => succeeded_paths.push(output_path),
            Err(e) => {
                tracing::error!("Batch render failed for '{}': {}", replay.replay_name, e);
                failed += 1;
            }
        }
    }

    (succeeded_paths.len(), failed, succeeded_paths)
}

/// Spawn a background thread that renders multiple replays to video files in a folder.
/// Returns a `BackgroundTask` to plug into the global status bar.
pub fn batch_render_to_folder(
    output_dir: std::path::PathBuf,
    replays: Vec<BatchReplayInfo>,
    options: RenderOptions,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    toasts: crate::tab_state::SharedToasts,
    prefer_cpu: bool,
) -> crate::task::BackgroundTask {
    let total_frames: u64 = replays.iter().map(|r| (r.game_duration * 7.0) as u64).sum();
    let total_replays = replays.len();
    let progress = Arc::new(Mutex::new(crate::task::BatchVideoExportProgress {
        total_frames,
        completed_frames: 0,
        current_index: 0,
        total_replays,
        current_name: String::new(),
    }));

    let (tx, rx) = std::sync::mpsc::channel();

    let progress_clone = Arc::clone(&progress);
    crate::util::thread::spawn_logged("batch-video-export", move || {
        let (succeeded, failed, _) =
            render_batch(&replays, &output_dir, &options, &asset_cache, &progress_clone, prefer_cpu);

        if failed == 0 {
            toasts.lock().success(format!("Batch render complete: {} videos saved", succeeded));
        } else {
            toasts.lock().warning(format!("Batch render: {} succeeded, {} failed", succeeded, failed));
        }
        let _ = tx.send(Ok(crate::task::BackgroundTaskCompletion::NoReceiver));
    });

    crate::task::BackgroundTask {
        receiver: Some(rx),
        kind: crate::task::BackgroundTaskKind::BatchVideoExport { progress },
    }
}

/// Spawn a background thread that renders multiple replays to a temp directory,
/// then copies all output files to the clipboard.
/// Returns a `BackgroundTask` to plug into the global status bar.
pub fn batch_render_to_clipboard(
    replays: Vec<BatchReplayInfo>,
    options: RenderOptions,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    toasts: crate::tab_state::SharedToasts,
    prefer_cpu: bool,
) -> crate::task::BackgroundTask {
    let total_frames: u64 = replays.iter().map(|r| (r.game_duration * 7.0) as u64).sum();
    let total_replays = replays.len();
    let progress = Arc::new(Mutex::new(crate::task::BatchVideoExportProgress {
        total_frames,
        completed_frames: 0,
        current_index: 0,
        total_replays,
        current_name: String::new(),
    }));

    let (tx, rx) = std::sync::mpsc::channel();

    let progress_clone = Arc::clone(&progress);
    crate::util::thread::spawn_logged("batch-video-clipboard", move || {
        let temp_dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => {
                toasts.lock().error(format!("Failed to create temp dir: {e}"));
                let _ = tx.send(Ok(crate::task::BackgroundTaskCompletion::NoReceiver));
                return;
            }
        };

        let (succeeded, failed, paths) =
            render_batch(&replays, temp_dir.path(), &options, &asset_cache, &progress_clone, prefer_cpu);

        if !paths.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
            let _ = clipboard.set().file_list(&refs);
            let _ = temp_dir.keep();
        }

        if failed == 0 {
            toasts.lock().success(format!("{} videos copied to clipboard", succeeded));
        } else {
            toasts.lock().warning(format!("Batch render: {} copied to clipboard, {} failed", succeeded, failed));
        }
        let _ = tx.send(Ok(crate::task::BackgroundTaskCompletion::NoReceiver));
    });

    crate::task::BackgroundTask {
        receiver: Some(rx),
        kind: crate::task::BackgroundTaskKind::BatchVideoExport { progress },
    }
}

/// Blocking implementation of the video export.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_video_blocking(
    output_path: &str,
    raw_meta: &[u8],
    packet_data: &[u8],
    map_name: &str,
    game_duration: f32,
    options: RenderOptions,
    wows_data: &SharedWoWsData,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    progress: &Arc<Mutex<Option<RenderProgress>>>,
    prefer_cpu: bool,
    actual_game_duration: Option<f32>,
) -> rootcause::Result<()> {
    use wows_minimap_renderer::drawing::ImageTarget;
    use wows_minimap_renderer::video::VideoEncoder;

    // Get game metadata and load assets for the software renderer
    let (vfs, game_metadata, game_constants) = {
        let data = wows_data.read();
        let gm = data.game_metadata.clone().ok_or_else(|| report!("Game metadata not loaded"))?;
        (data.vfs.clone(), gm, Arc::clone(&data.game_constants))
    };

    // Load assets — reuse cached raw RGBA data and convert to image types
    let (
        map_image_rgb,
        ship_icons_rgba,
        plane_icons_rgba,
        building_icons_rgba,
        consumable_icons_rgba,
        death_cause_icons,
        powerup_icons,
        map_info,
        game_fonts,
    ) = {
        let mut cache = asset_cache.lock();
        let ship_raw = cache.get_or_load_ship_icons(&vfs);
        let plane_raw = cache.get_or_load_plane_icons(&vfs);
        let building_raw = cache.get_or_load_building_icons(&vfs);
        let consumable_raw = cache.get_or_load_consumable_icons(&vfs);
        let death_cause_raw = cache.get_or_load_death_cause_icons(&vfs);
        let powerup_raw = cache.get_or_load_powerup_icons(&vfs);
        let game_fonts = cache.get_or_load_game_fonts(&vfs);
        let (map_raw, map_info) = cache.get_or_load_map(map_name, &vfs);

        // Convert cached RGBA bytes back to image types for ImageTarget
        let to_rgba = |a: &super::RgbaAsset| image::RgbaImage::from_raw(a.width, a.height, a.data.clone()).unwrap();
        let ship_icons: HashMap<String, image::RgbaImage> =
            ship_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();
        let plane_icons: HashMap<String, image::RgbaImage> =
            plane_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();
        let building_icons: HashMap<String, image::RgbaImage> =
            building_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();
        let consumable_icons: HashMap<String, image::RgbaImage> =
            consumable_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();

        let map_image = map_raw.as_ref().and_then(|arc| {
            // Cached data is RGBA, convert to RGB for ImageTarget
            let rgba = image::RgbaImage::from_raw(arc.width, arc.height, arc.data.clone())?;
            Some(image::DynamicImage::ImageRgba8(rgba).into_rgb8())
        });

        let death_cause_icons: HashMap<String, image::RgbaImage> =
            death_cause_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();
        let powerup_icons: HashMap<String, image::RgbaImage> =
            powerup_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();

        (
            map_image,
            ship_icons,
            plane_icons,
            building_icons,
            consumable_icons,
            death_cause_icons,
            powerup_icons,
            map_info,
            game_fonts,
        )
    };

    // Build replay parser components
    let replay_file = ReplayFile::from_decrypted_parts(raw_meta.to_vec(), packet_data.to_vec())
        .map_err(|e| report!("Failed to parse replay: {:?}", e))?;

    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut controller = BattleController::new(&replay_file.meta, &*game_metadata, Some(&game_constants));
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    // Load self player's ship silhouette for the stats panel
    let self_silhouette = replay_file.meta.vehicles.iter().find(|v| v.relation == 0).and_then(|v| {
        use wowsunpack::game_params::types::GameParamProvider;
        let param = GameParamProvider::game_param_by_id(&*game_metadata, v.shipId)?;
        let path = format!("gui/ships_silhouettes/{}.png", param.index());
        let img = wows_minimap_renderer::assets::load_packed_image(&path, &vfs)?;
        let mut rgba = img.into_rgba8();
        // Normalize to white pixels with original alpha for correct tint multiplication.
        for px in rgba.pixels_mut() {
            px[0] = 255;
            px[1] = 255;
            px[2] = 255;
        }
        Some(rgba)
    });

    let mut renderer = MinimapRenderer::new(map_info, &game_metadata, version, options.clone());
    renderer.set_fonts(game_fonts.clone());
    if let Some(ref sil) = self_silhouette {
        renderer.set_self_silhouette(sil.clone());
    }
    let mut target = ImageTarget::with_stats_panel(
        map_image_rgb,
        game_fonts,
        ship_icons_rgba,
        plane_icons_rgba,
        building_icons_rgba,
        consumable_icons_rgba,
        death_cause_icons,
        powerup_icons,
        options.show_stats_panel,
    );
    target.set_text_resolver(std::sync::Arc::new(crate::LocalizedTextResolver));
    let (cw, ch) = target.canvas_size();
    let mut encoder = VideoEncoder::new(Some(output_path), None, false, game_duration, cw, ch);
    encoder.set_prefer_cpu(prefer_cpu);
    if let Some(duration) = actual_game_duration {
        encoder.set_battle_duration(GameClock(duration));
    }
    {
        let progress = Arc::clone(progress);
        encoder.set_progress_callback(move |p| {
            *progress.lock() = Some(p);
        });
    }

    // Parse all packets, advancing the encoder at each clock tick
    let mut remaining = &replay_file.packet_data[..];
    let mut prev_clock = GameClock(0.0);

    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => {
                if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                    renderer.populate_players(&controller);
                    renderer.update_squadron_info(&controller);
                    renderer.update_ship_abilities(&controller);
                    encoder.advance_clock(prev_clock, &controller, &mut renderer, &mut target);
                }
                prev_clock = packet.clock;
                controller.process(&packet);
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

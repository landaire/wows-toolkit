use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use rootcause::report;
use wows_minimap_renderer::RenderProgress;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_battle_world::merged::MergedReplays;
use wows_replays::ReplayFile;
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
        PendingVideoExport::SaveToFile {
            output_path,
            options,
            prefer_cpu,
            codec,
            actual_game_duration,
            encoder_config,
            include_pre_battle,
        } => {
            save_as_video(
                output_path,
                video_export_data.raw_meta.clone(),
                video_export_data.packet_data.clone(),
                video_export_data.alt_replays.clone(),
                video_export_data.map_name.clone(),
                video_export_data.game_duration,
                options,
                video_export_data.wows_data.clone(),
                Arc::clone(&video_export_data.asset_cache),
                Arc::clone(toasts),
                Arc::clone(video_exporting),
                Arc::clone(video_export_progress),
                prefer_cpu,
                codec,
                actual_game_duration,
                encoder_config,
                include_pre_battle,
            );
        }
        PendingVideoExport::CopyToClipboard {
            options,
            prefer_cpu,
            codec,
            actual_game_duration,
            encoder_config,
            include_pre_battle,
        } => {
            let file_name = format!("{}.mp4", video_export_data.replay_name);
            render_video_to_clipboard(
                file_name,
                Arc::clone(video_export_data),
                options,
                Arc::clone(toasts),
                Arc::clone(video_exporting),
                Arc::clone(video_export_progress),
                prefer_cpu,
                codec,
                actual_game_duration,
                encoder_config,
                include_pre_battle,
            );
        }
    }
}

/// Rendu headless SYNCHRONE d'un replay vers un fichier MP4 (envoi auto Discord).
/// Reutilise `render_video_blocking` avec un cache d'assets neuf et un bitrate
/// calcule pour tenir sous `target_size_mib`.
pub fn render_replay_to_file(
    output_path: &str,
    raw_meta: &[u8],
    packet_data: &[u8],
    map_name: &str,
    game_duration: f32,
    options: RenderOptions,
    wows_data: &SharedWoWsData,
    target_size_mib: u32,
    speed: f32,
    actual_secs: f32,
) -> rootcause::Result<()> {
    let asset_cache = Arc::new(parking_lot::Mutex::new(RendererAssetCache::default()));
    let progress = Arc::new(Mutex::new(None));
    // Vitesse choisie (x5/x10/x15/x20) -> duree de sortie (scaling du temps).
    let speed = if speed > 0.0 { speed as f64 } else { 20.0 };
    let game_secs = if game_duration > 1.0 { game_duration as f64 } else { 60.0 };
    let output_duration = (game_secs / speed).max(1.0);
    // IMPORTANT : la vraie longueur de la video = duree REELLE de la partie / vitesse
    // (pas la limite theorique). On cale le bitrate la-dessus pour une bonne
    // qualite tout en restant sous target_size_mib.
    let actual = if actual_secs > 1.0 { actual_secs as f64 } else { game_secs };
    let video_len = (actual / speed).max(1.0);
    let bits = (target_size_mib as f64) * 1024.0 * 1024.0 * 8.0 * 0.92;
    let bitrate = (bits / video_len).clamp(400_000.0, 16_000_000.0) as u32;
    let encoder_config = wows_minimap_renderer::EncoderConfig {
        target_bitrate_bps: Some(bitrate),
        max_bitrate_bps: None,
        av1_quantizer: None,
    };
    render_video_blocking(
        output_path,
        raw_meta,
        packet_data,
        &[],
        map_name,
        game_duration,
        options,
        wows_data,
        &asset_cache,
        &progress,
        false,
        Some(wows_minimap_renderer::VideoCodec::H264),
        None,
        encoder_config,
        false,
        Some(output_duration),
    )
}

/// Spawn a background thread that renders the replay to an MP4 video file
/// using the software renderer (`ImageTarget`) and `VideoEncoder`.
#[allow(clippy::too_many_arguments)]
pub(super) fn save_as_video(
    output_path: String,
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    alt_replays: Vec<super::AltReplayBytes>,
    map_name: String,
    game_duration: f32,
    options: RenderOptions,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    toasts: crate::tab_state::SharedToasts,
    video_exporting: Arc<AtomicBool>,
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    prefer_cpu: bool,
    codec: Option<wows_minimap_renderer::VideoCodec>,
    actual_game_duration: Option<f32>,
    encoder_config: wows_minimap_renderer::EncoderConfig,
    include_pre_battle: bool,
) {
    video_exporting.store(true, Ordering::Relaxed);

    crate::util::thread::spawn_logged("video-export", move || {
        let result = render_video_blocking(
            &output_path,
            &raw_meta,
            &packet_data,
            &alt_replays,
            &map_name,
            game_duration,
            options,
            &wows_data,
            &asset_cache,
            &video_export_progress,
            prefer_cpu,
            codec,
            actual_game_duration,
            encoder_config,
            include_pre_battle,
            None,
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
    codec: Option<wows_minimap_renderer::VideoCodec>,
    actual_game_duration: Option<f32>,
    encoder_config: wows_minimap_renderer::EncoderConfig,
    include_pre_battle: bool,
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
            &export_data.alt_replays,
            &export_data.map_name,
            export_data.game_duration,
            options,
            &export_data.wows_data,
            &export_data.asset_cache,
            &video_export_progress,
            prefer_cpu,
            codec,
            actual_game_duration,
            encoder_config,
            include_pre_battle,
            None,
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

/// Encoding/output preferences shared by the batch render entry points.
#[derive(Clone, Copy)]
pub struct BatchEncodeOptions {
    pub prefer_cpu: bool,
    pub codec: Option<wows_minimap_renderer::VideoCodec>,
    pub include_pre_battle: bool,
}

/// Shared helper: render a list of replays sequentially, updating progress.
/// Returns (succeeded_count, failed_count, output_paths).
fn render_batch(
    replays: &[BatchReplayInfo],
    output_dir: &std::path::Path,
    options: &RenderOptions,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    progress: &Arc<Mutex<crate::task::BatchVideoExportProgress>>,
    encode: &BatchEncodeOptions,
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
            &[], // batch rendering doesn't propagate per-replay merge state
            &replay.map_name,
            replay.game_duration,
            options.clone(),
            &replay.wows_data,
            asset_cache,
            &per_replay_progress,
            encode.prefer_cpu,
            encode.codec,
            None,
            wows_minimap_renderer::EncoderConfig::default(),
            encode.include_pre_battle,
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
    encode: BatchEncodeOptions,
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
            render_batch(&replays, &output_dir, &options, &asset_cache, &progress_clone, &encode);

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
    encode: BatchEncodeOptions,
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
            render_batch(&replays, temp_dir.path(), &options, &asset_cache, &progress_clone, &encode);

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
    alt_replays: &[super::AltReplayBytes],
    map_name: &str,
    game_duration: f32,
    options: RenderOptions,
    wows_data: &SharedWoWsData,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    progress: &Arc<Mutex<Option<RenderProgress>>>,
    prefer_cpu: bool,
    codec: Option<wows_minimap_renderer::VideoCodec>,
    actual_game_duration: Option<f32>,
    encoder_config: wows_minimap_renderer::EncoderConfig,
    include_pre_battle: bool,
    output_duration_secs: Option<f64>,
) -> rootcause::Result<()> {
    use wows_minimap_renderer::drawing::ImageTarget;
    use wows_minimap_renderer::video::VideoEncoder;

    // Get game metadata and load assets for the software renderer
    let (vfs, version, game_metadata, game_constants, dump_dir) = {
        let data = wows_data.read();
        let gm = data.game_metadata.clone().ok_or_else(|| report!("Game metadata not loaded"))?;
        (data.vfs.clone(), data.version().copied(), gm, Arc::clone(&data.game_constants), data.dump_dir.clone())
    };
    let version = version.as_ref();
    let dump_dir = dump_dir.as_deref();

    // Load assets — reuse cached raw RGBA data and convert to image types
    let (
        map_image_rgb,
        ship_icons_rgba,
        plane_icons_rgba,
        building_icons_rgba,
        consumable_icons_rgba,
        ribbon_icons_rgba,
        subribbon_icons_rgba,
        death_cause_icons,
        powerup_icons,
        map_info,
        game_fonts,
    ) = {
        let mut cache = asset_cache.lock();
        let ship_raw = cache.get_or_load_ship_icons(&vfs, version, dump_dir);
        let plane_raw = cache.get_or_load_plane_icons(&vfs, version, dump_dir);
        let building_raw = cache.get_or_load_building_icons(&vfs, version, dump_dir);
        let consumable_raw = cache.get_or_load_consumable_icons(&vfs, version, dump_dir);
        let ribbon_raw = cache.get_or_load_ribbon_icons(&vfs, version, dump_dir);
        let subribbon_raw = cache.get_or_load_subribbon_icons(&vfs, version, dump_dir);
        let death_cause_raw = cache.get_or_load_death_cause_icons(&vfs, version, dump_dir);
        let powerup_raw = cache.get_or_load_powerup_icons(&vfs, version, dump_dir);
        let game_fonts = cache.get_or_load_game_fonts(&vfs, version, dump_dir);
        let (map_raw, map_info) = cache.get_or_load_map(map_name, &vfs, version);

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
        let ribbon_icons: HashMap<String, image::RgbaImage> =
            ribbon_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();
        let subribbon_icons: HashMap<String, image::RgbaImage> =
            subribbon_raw.iter().map(|(k, a)| (k.clone(), to_rgba(a))).collect();

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
            ribbon_icons,
            subribbon_icons,
            death_cause_icons,
            powerup_icons,
            map_info,
            game_fonts,
        )
    };

    // Build replay parser components
    let replay_file = ReplayFile::from_decrypted_parts(raw_meta.to_vec(), packet_data.to_vec())
        .map_err(|e| report!("Failed to parse replay: {:?}", e))?;
    let alt_replay_files: Vec<ReplayFile> = alt_replays
        .iter()
        .map(|a| ReplayFile::from_decrypted_parts(a.raw_meta.clone(), a.packet_data.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| report!("Failed to parse merge replay: {:?}", e))?;

    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
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
    renderer.set_merged_perspectives(!alt_replay_files.is_empty());
    if let Some(ref sil) = self_silhouette {
        renderer.set_self_silhouette(sil.clone());
    }
    let mut target = ImageTarget::with_side_panel(
        map_image_rgb,
        game_fonts,
        ship_icons_rgba,
        plane_icons_rgba,
        building_icons_rgba,
        consumable_icons_rgba,
        ribbon_icons_rgba,
        subribbon_icons_rgba,
        death_cause_icons,
        powerup_icons,
        wows_minimap_renderer::drawing::SidePanelLayout::from_options(&options),
    );
    target.set_text_resolver(std::sync::Arc::new(crate::LocalizedTextResolver));
    let (cw, ch) = target.canvas_size();
    let mut encoder = VideoEncoder::new(Some(output_path), None, false, game_duration, cw, ch);
    if let Some(d) = output_duration_secs {
        encoder.set_output_duration(d);
    }
    encoder.set_prefer_cpu(prefer_cpu);
    encoder.set_codec(match codec {
        Some(c) => wows_minimap_renderer::video::CodecChoice::Explicit(c),
        None => wows_minimap_renderer::video::CodecChoice::Auto,
    });
    encoder.set_encoder_config(encoder_config);
    {
        let progress = Arc::clone(progress);
        encoder.set_progress_callback(move |p| {
            *progress.lock() = Some(p);
        });
    }

    // Drive the parse via MergedReplays so a single code path handles both the
    // standalone case (zero alt replays, single primary) and the merged case
    // (alt perspectives feed packets into the same controller through the
    // routing filter).
    let mut session = MergedReplays::new(
        game_metadata.entity_specs(),
        &*game_metadata,
        &game_constants,
        version,
        &replay_file,
        &alt_replay_files,
    )
    .map_err(|e| report!("{e}"))?;
    let vehicle_facts = session.vehicle_facts().clone();
    {
        let mut primary_with_alts: Vec<&ReplayFile> = vec![&replay_file];
        primary_with_alts.extend(alt_replay_files.iter());
        let damage_events = wows_battle_world::merged::gather_damage_events(
            &*game_metadata,
            &game_constants,
            version,
            game_metadata.entity_specs(),
            &primary_with_alts,
        );
        renderer.set_damage_events(damage_events);
    }
    renderer.set_vehicle_facts(vehicle_facts.clone());
    wows_replay_insights::build::seed_consumable_inventories_from_facts(
        session.world_mut(),
        &vehicle_facts,
        &*game_metadata,
        version,
    );
    renderer.set_position_timeline(session.position_timeline());
    let salvo_flight_times = wows_battle_world::scan::scan_salvo_flight_times(
        &replay_file.meta,
        &*game_metadata,
        &game_constants,
        version,
        &replay_file,
    );
    renderer.set_salvo_flight_times(std::sync::Arc::new(salvo_flight_times));

    // By default the export starts at battle start, skipping the pre-battle
    // spawn and countdown. The battle-start clock comes from the scanned
    // session; falling back to clock 0 renders the full replay.
    let render_start =
        if include_pre_battle { GameClock(0.0) } else { session.battle_start_clock().unwrap_or(GameClock(0.0)) };
    encoder.set_render_start(render_start);
    if let Some(duration) = actual_game_duration {
        encoder.set_battle_duration(GameClock(duration));
    }

    let mut prev_render_clock = GameClock(0.0);
    while let Some(safe_clock) = session.step().map_err(|e| report!("{e}"))? {
        if safe_clock.0 > prev_render_clock.0 {
            let view = session.world_mut().view();
            renderer.populate_players(&view);
            renderer.update_squadron_info(&view);
            renderer.update_ship_abilities(&view);
            encoder.advance_clock(prev_render_clock, &view, &mut renderer, &mut target);
            prev_render_clock = safe_clock;
        }
    }

    let total = session.total_duration();
    if total.0 > prev_render_clock.0 {
        let view = session.world_mut().view();
        renderer.populate_players(&view);
        renderer.update_squadron_info(&view);
        renderer.update_ship_abilities(&view);
        encoder.advance_clock(total, &view, &mut renderer, &mut target);
    }

    session.finish();
    let mut world = session.into_world();
    {
        let view = world.view();
        renderer.populate_players(&view);
        renderer.update_squadron_info(&view);
        renderer.update_ship_abilities(&view);
        encoder.finish(&view, &mut renderer, &mut target).map_err(|e| report!("{e}"))?;
    }

    Ok(())
}

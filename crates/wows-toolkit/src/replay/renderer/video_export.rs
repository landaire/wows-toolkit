use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use rootcause::report;
use wows_minimap_renderer::ExportSettings;
use wows_minimap_renderer::RenderProgress;
use wows_minimap_renderer::SavedRender;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::ShipConfigFilter;
use wows_minimap_renderer::ShipConfigVisibility;
use wows_minimap_renderer::battle_scans::BattleScans;
use wows_minimap_renderer::encoder::EncoderStatus;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;
use wows_minimap_renderer::timing::ExportWindow;

use wows_battle_world::merged::MergedReplays;
use wows_replays::ReplayFile;
use wows_replays::types::GameClock;
use wows_toolkit_viewmodel::video_name::video_file_stem;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use super::PendingVideoExport;
use super::RendererAssetCache;
use super::VideoExportData;
use crate::data::wows_data::BuildDataCache;
use crate::data::wows_data::SharedBuildData;

/// What an export draws: the layers, and whether sunk ships are among them.
#[derive(Clone, Debug)]
pub struct DrawnLayers {
    pub options: RenderOptions,
    pub show_dead_ships: bool,
}

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
        PendingVideoExport::SaveToFile { output_path, layers, export, encoder_config } => {
            save_as_video(
                output_path,
                video_export_data.raw_meta.clone(),
                video_export_data.packet_data.clone(),
                video_export_data.alt_replays.clone(),
                video_export_data.map_name.clone(),
                layers,
                video_export_data.wows_data.clone(),
                Arc::clone(&video_export_data.asset_cache),
                Arc::clone(toasts),
                Arc::clone(video_exporting),
                Arc::clone(video_export_progress),
                export,
                encoder_config,
            );
        }
        PendingVideoExport::CopyToClipboard { layers, export, encoder_config } => {
            let file_name = format!("{}.mp4", video_export_data.video_file_stem);
            render_video_to_clipboard(
                file_name,
                Arc::clone(video_export_data),
                layers,
                Arc::clone(toasts),
                Arc::clone(video_exporting),
                Arc::clone(video_export_progress),
                export,
                encoder_config,
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
    alt_replays: Vec<super::AltReplayBytes>,
    map_name: String,
    layers: DrawnLayers,
    wows_data: SharedBuildData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    toasts: crate::tab_state::SharedToasts,
    video_exporting: Arc<AtomicBool>,
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    export: ExportSettings,
    encoder_config: wows_minimap_renderer::EncoderConfig,
) {
    video_exporting.store(true, Ordering::Relaxed);

    crate::util::thread::spawn_logged("video-export", move || {
        let result = render_video_blocking(
            &output_path,
            &raw_meta,
            &packet_data,
            &alt_replays,
            &map_name,
            layers,
            &wows_data,
            &asset_cache,
            &video_export_progress,
            export,
            encoder_config,
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
    layers: DrawnLayers,
    toasts: crate::tab_state::SharedToasts,
    video_exporting: Arc<AtomicBool>,
    video_export_progress: Arc<Mutex<Option<RenderProgress>>>,
    export: ExportSettings,
    encoder_config: wows_minimap_renderer::EncoderConfig,
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
            layers,
            &export_data.wows_data,
            &export_data.asset_cache,
            &video_export_progress,
            export,
            encoder_config,
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

/// Everything a render needs from one replay file, as read by
/// [`replay_render_input`].
pub struct ReplayRenderInput {
    pub raw_meta: Vec<u8>,
    pub packet_data: Vec<u8>,
    pub map_name: String,
    pub replay_name: String,
    pub video_file_stem: String,
    pub game_duration: f32,
    pub wows_data: SharedBuildData,
}

/// Read one replay off disk and pair it with the game data for the build it was
/// recorded on. `None` when the file cannot be read, its version string is
/// unreadable, or the toolkit does not have that build's data.
///
/// Reading is decrypt plus inflate of the whole packet stream. A batch runs this
/// on its render thread, one replay at a time, rather than where the batch is
/// requested.
pub fn replay_render_input(path: &std::path::Path, build_cache: &BuildDataCache) -> Option<ReplayRenderInput> {
    let replay_file = match ReplayFile::from_file(path) {
        Ok(replay_file) => replay_file,
        Err(error) => {
            tracing::warn!("could not read replay {} for rendering: {error:?}", path.display());
            return None;
        }
    };
    let Some(replay_version) = Version::try_from_client_exe(&replay_file.meta.clientVersionFromExe) else {
        tracing::warn!(
            "replay {} reports an unreadable client version {:?} - skipping",
            path.display(),
            replay_file.meta.clientVersionFromExe
        );
        return None;
    };
    let Some(wows_data) = build_cache.resolve(&replay_version) else {
        tracing::warn!("No data for version {} - skipping replay '{}'", replay_version.to_path(), path.display());
        return None;
    };

    let map_name = replay_file.meta.mapName.clone();
    let translated_map = match wows_data.read().game_metadata.as_ref() {
        Some(metadata) => wowsunpack::game_params::translations::translate_map_name(&map_name, metadata.as_ref()),
        None => map_name.clone(),
    };
    let base = format!("{} - {}", replay_file.meta.playerName, translated_map);
    let source_stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
    let replay_name = match &source_stem {
        Some(stem) => format!("{} - {}", base, stem),
        None => base,
    };
    let video_file_stem = video_file_stem(&replay_file.meta, &translated_map, source_stem.as_deref());

    let game_duration = replay_file.meta.duration as f32;
    let (_, raw_meta, packet_data) = replay_file.into_parts();
    Some(ReplayRenderInput {
        raw_meta: raw_meta.into_bytes(),
        packet_data,
        map_name,
        replay_name,
        video_file_stem,
        game_duration,
        wows_data,
    })
}

/// Everything a batch render needs beyond the replay list and destination.
pub struct BatchRenderContext {
    pub build_cache: BuildDataCache,
    pub layers: DrawnLayers,
    pub asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    pub toasts: crate::tab_state::SharedToasts,
    pub export: ExportSettings,
    pub egui_ctx: egui::Context,
}

/// What a batch draws and encodes: the saved renderer defaults, read through
/// [`SavedRenderOptions::render`] as a viewer opening reads them, with the CPU preference
/// resolved against what this machine can encode.
///
/// A batch has no per-ship choices, which are made by hand in a viewer and not saved. The saved
/// self ranges go on the recording player's ship, as a viewer opens with them; without them the
/// range layer is every ship's.
pub fn batch_render_settings(saved: &SavedRenderOptions, status: &EncoderStatus) -> (DrawnLayers, ExportSettings) {
    let SavedRender { mut options, show_dead_ships, self_ranges, mut export } = saved.render(RenderOptions::default());
    match self_ranges {
        Some(ranges) => {
            options.show_ship_config = true;
            options.ship_config_visibility = ShipConfigVisibility::SelfOnly(ranges);
        }
        None => {
            options.ship_config_visibility =
                ShipConfigVisibility::Filtered(Arc::new(|_| Some(ShipConfigFilter::all_enabled())));
        }
    }
    export.prefer_cpu = super::resolve_prefer_cpu(export.prefer_cpu, export.codec, status);

    (DrawnLayers { options, show_dead_ships }, export)
}

/// Shared helper: render a list of replays sequentially, updating progress.
///
/// Each replay is read off disk here, immediately before it is rendered, so the
/// caller never pays for the reads and only one replay's packet stream is
/// resident at a time. A replay that cannot be read counts as a failure.
///
/// Returns (succeeded_count, failed_count, output_paths).
fn render_batch(
    paths: &[std::path::PathBuf],
    output_dir: &std::path::Path,
    ctx: &BatchRenderContext,
    progress: &Arc<Mutex<crate::task::BatchVideoExportProgress>>,
) -> (usize, usize, Vec<std::path::PathBuf>) {
    let mut succeeded_paths = Vec::new();
    let mut failed = 0usize;

    for (i, path) in paths.iter().enumerate() {
        {
            let mut p = progress.lock();
            p.current_index = i;
            p.current_frames = 0;
            p.current_total_frames = None;
            p.current_name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        }

        let Some(replay) = replay_render_input(path, &ctx.build_cache) else {
            failed += 1;
            continue;
        };

        {
            let mut p = progress.lock();
            p.current_name = replay.replay_name.clone();
        }
        ctx.egui_ctx.request_repaint();

        let output_path = output_dir.join(format!("{}.mp4", replay.video_file_stem));
        let output_str = output_path.to_string_lossy().to_string();

        let per_replay_progress: Arc<Mutex<Option<RenderProgress>>> = Arc::new(Mutex::new(None));
        let stop_flag = Arc::new(AtomicBool::new(false));

        let progress_thread = {
            let progress = Arc::clone(progress);
            let per_replay_progress = Arc::clone(&per_replay_progress);
            let stop_flag = Arc::clone(&stop_flag);
            let egui_ctx = ctx.egui_ctx.clone();
            std::thread::spawn(move || {
                while !stop_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if let Some(ref p) = *per_replay_progress.lock() {
                        let mut batch = progress.lock();
                        batch.current_frames = p.current;
                        batch.current_total_frames = Some(p.total);
                        // Deferred so the sampler does not pin the repaint rate.
                        egui_ctx.request_repaint_after(std::time::Duration::from_millis(100));
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
            ctx.layers.clone(),
            &replay.wows_data,
            &ctx.asset_cache,
            &per_replay_progress,
            ctx.export,
            wows_minimap_renderer::EncoderConfig::default(),
        );

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
    paths: Vec<std::path::PathBuf>,
    ctx: BatchRenderContext,
) -> crate::task::BackgroundTask {
    let progress = Arc::new(Mutex::new(crate::task::BatchVideoExportProgress::for_batch(paths.len())));

    let (tx, rx) = crate::task::completion_channel();

    let progress_clone = Arc::clone(&progress);
    crate::util::thread::spawn_logged("batch-video-export", move || {
        let (succeeded, failed, _) = render_batch(&paths, &output_dir, &ctx, &progress_clone);

        if failed == 0 {
            ctx.toasts.lock().success(format!("Batch render complete: {} videos saved", succeeded));
        } else {
            ctx.toasts.lock().warning(format!("Batch render: {} succeeded, {} failed", succeeded, failed));
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
    paths: Vec<std::path::PathBuf>,
    ctx: BatchRenderContext,
) -> crate::task::BackgroundTask {
    let progress = Arc::new(Mutex::new(crate::task::BatchVideoExportProgress::for_batch(paths.len())));

    let (tx, rx) = crate::task::completion_channel();

    let progress_clone = Arc::clone(&progress);
    crate::util::thread::spawn_logged("batch-video-clipboard", move || {
        let temp_dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => {
                ctx.toasts.lock().error(format!("Failed to create temp dir: {e}"));
                let _ = tx.send(Ok(crate::task::BackgroundTaskCompletion::NoReceiver));
                return;
            }
        };

        let (succeeded, failed, rendered) = render_batch(&paths, temp_dir.path(), &ctx, &progress_clone);

        if !rendered.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let refs: Vec<&std::path::Path> = rendered.iter().map(|p| p.as_path()).collect();
            let _ = clipboard.set().file_list(&refs);
            let _ = temp_dir.keep();
        }

        if failed == 0 {
            ctx.toasts.lock().success(format!("{} videos copied to clipboard", succeeded));
        } else {
            ctx.toasts.lock().warning(format!("Batch render: {} copied to clipboard, {} failed", succeeded, failed));
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
    layers: DrawnLayers,
    wows_data: &SharedBuildData,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    progress: &Arc<Mutex<Option<RenderProgress>>>,
    export: ExportSettings,
    encoder_config: wows_minimap_renderer::EncoderConfig,
) -> rootcause::Result<()> {
    use wows_minimap_renderer::drawing::ImageTarget;
    use wows_minimap_renderer::video::VideoEncoder;

    let DrawnLayers { options, show_dead_ships } = layers;
    let ExportSettings { prefer_cpu, include_pre_battle, codec, timing } = export;

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
    BattleScans::gather(&*game_metadata, &game_constants, version, &replay_file, &alt_replay_files)
        .install(&mut renderer, &session);
    let vehicle_facts = session.vehicle_facts().clone();
    wows_replay_insights::build::seed_consumable_inventories_from_facts(
        session.world_mut(),
        &vehicle_facts,
        &*game_metadata,
        version,
    );

    // By default the export starts at battle start, skipping the pre-battle
    // spawn and countdown. The battle-start clock comes from the scanned
    // session; falling back to clock 0 renders the full replay.
    let render_start =
        if include_pre_battle { GameClock(0.0) } else { session.battle_start_clock().unwrap_or(GameClock(0.0)) };
    let Some(window) = ExportWindow::new(render_start, session.total_duration()) else {
        return Err(report!(
            "the replay ends at {} before the video would start at {render_start}",
            session.total_duration()
        ));
    };

    let (cw, ch) = target.canvas_size();
    let mut encoder = VideoEncoder::new(Some(output_path), None, false, timing, window, cw, ch);
    encoder.set_prefer_cpu(prefer_cpu);
    encoder.set_codec(match codec {
        Some(c) => wows_minimap_renderer::video::CodecChoice::Explicit(c),
        None => wows_minimap_renderer::video::CodecChoice::Auto,
    });
    encoder.set_encoder_config(encoder_config);
    encoder.set_show_dead_ships(show_dead_ships);
    {
        let progress = Arc::clone(progress);
        encoder.set_progress_callback(move |p| {
            *progress.lock() = Some(p);
        });
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

#[cfg(test)]
mod tests {
    use wows_minimap_renderer::ExportSettings;
    use wows_minimap_renderer::SavedRenderOptions;
    use wows_minimap_renderer::ShipConfigFilter;
    use wows_minimap_renderer::VideoCodec;
    use wows_minimap_renderer::encoder::CodecSupport;
    use wows_minimap_renderer::encoder::EncoderStatus;
    use wows_replays::types::EntityId;

    use super::batch_render_settings;

    /// A machine whose GPU encodes H.264 and nothing else.
    fn h264_gpu() -> EncoderStatus {
        let mut status = EncoderStatus {
            gpu_adapter_name: Some("test".into()),
            gpu_error: None,
            gpu_codecs: Default::default(),
            cpu_codecs: Default::default(),
        };
        status.gpu_codecs.insert(VideoCodec::H264, CodecSupport::Supported);
        status.cpu_codecs.insert(VideoCodec::Av1, CodecSupport::Supported);

        status
    }

    /// A batch from the listing draws and encodes what the renderer saved: a layer and sunk ships
    /// turned off stay off, the saved self ranges are the recording player's alone, and the
    /// export settings come through, a codec the GPU cannot encode falling back to software.
    #[test]
    fn a_batch_renders_with_the_saved_renderer_defaults() {
        let saved = SavedRenderOptions {
            show_torpedoes: false,
            show_dead_ships: false,
            show_self_detection_range: true,
            include_pre_battle: true,
            video_codec: Some(VideoCodec::Av1),
            ..SavedRenderOptions::default()
        };

        let (layers, export) = batch_render_settings(&saved, &h264_gpu());

        assert!(!layers.options.show_torpedoes, "a layer saved off is not filmed");
        assert!(!layers.show_dead_ships, "nor are sunk ships");
        assert!(layers.options.show_ship_config, "the saved self ranges are drawn");
        let visibility = &layers.options.ship_config_visibility;
        let detection_only = ShipConfigFilter { detection: true, ..ShipConfigFilter::default() };
        assert_eq!(visibility.filter_for(true, EntityId::from(1u32)), Some(detection_only), "on the recording ship");
        assert_eq!(visibility.filter_for(false, EntityId::from(2u32)), None, "and nobody else's");
        let expected = ExportSettings {
            prefer_cpu: true,
            include_pre_battle: true,
            codec: Some(VideoCodec::Av1),
            timing: saved.export_timing,
        };
        assert_eq!(export, expected);

        let every_ship = SavedRenderOptions { show_ship_config: true, ..SavedRenderOptions::default() };
        let (layers, export) = batch_render_settings(&every_ship, &h264_gpu());
        let visibility = &layers.options.ship_config_visibility;
        assert_eq!(
            visibility.filter_for(false, EntityId::from(2u32)),
            Some(ShipConfigFilter::all_enabled()),
            "without self ranges the range layer is every ship's"
        );
        assert!(!export.prefer_cpu, "the GPU encodes the codec it would pick");
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use parking_lot::Mutex;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::collab::peer::FrameBroadcast;
use crate::data::wows_data::SharedWoWsData;

use super::PlaybackCommand;
use super::PlaybackFrame;
use super::RealtimeArmorBridge;
use super::RendererAssetCache;
use super::RendererStatus;
use super::ReplayPlayerInfo;
use super::ReplayRendererAssets;
use super::SNAPSHOTS_PER_SECOND;
use super::SharedRendererState;
use super::timeline::ShipShotTimeline;
use super::timeline::extract_all_shots;
use super::timeline::extract_timeline_events;
use crate::util::controls::parse_commands_scheme;

#[allow(clippy::too_many_arguments)]
pub(super) fn playback_thread(
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
    // 1. Get VFS, game metadata, and game constants from the app
    let (vfs, game_metadata, game_constants) = {
        let data = wows_data.read();
        let gm = match data.game_metadata.clone() {
            Some(gm) => gm,
            None => {
                shared_state.lock().status = RendererStatus::Error("Game metadata not loaded".to_string());
                return;
            }
        };
        (data.vfs.clone(), gm, Arc::clone(&data.game_constants))
    };

    // 2. Load visual assets (cached across renderer instances)
    let (map_info, game_fonts, map_image_for_announce) = {
        let mut cache = asset_cache.lock();
        let ship_icons = cache.get_or_load_ship_icons(&vfs);
        let plane_icons = cache.get_or_load_plane_icons(&vfs);
        let consumable_icons = cache.get_or_load_consumable_icons(&vfs);
        let death_cause_icons = cache.get_or_load_death_cause_icons(&vfs);
        let powerup_icons = cache.get_or_load_powerup_icons(&vfs);
        let game_fonts = cache.get_or_load_game_fonts(&vfs);
        let (map_image, map_info) = cache.get_or_load_map(&map_name, &vfs);

        let map_image_for_announce = map_image.clone();
        shared_state.lock().assets = Some(ReplayRendererAssets {
            map_image,
            ship_icons,
            plane_icons,
            consumable_icons,
            death_cause_icons,
            powerup_icons,
        });

        shared_state.lock().map_space_size = map_info.as_ref().map(|m| m.space_size as f32);

        (map_info, game_fonts, map_image_for_announce)
    };

    // Announce ReplayOpened to collab peers directly from the background thread.
    // This avoids depending on the parent window repainting (cross-window
    // request_repaint is unreliable on Windows).
    {
        let mut state = shared_state.lock();
        if !state.session_announced
            && let (Some(replay_id), Some(ref tx)) = (state.collab_replay_id, state.collab_command_tx.clone())
        {
            let map_png = map_image_for_announce
                .as_ref()
                .map(|img: &Arc<super::RgbaAsset>| {
                    let mut buf = Vec::new();
                    if let Some(image) = image::RgbaImage::from_raw(img.width, img.height, img.data.clone()) {
                        let mut cursor = std::io::Cursor::new(&mut buf);
                        let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                    }
                    buf
                })
                .unwrap_or_default();
            let game_version = state.game_version.clone().unwrap_or_default();
            let replay_name = state.collab_replay_name.clone().unwrap_or_default();
            let collab_map_name = state.collab_map_name.clone().unwrap_or_default();
            let display_name = {
                let wd = wows_data.read();
                if let Some(ref gm) = wd.game_metadata {
                    wowsunpack::game_params::translations::translate_map_name(&collab_map_name, gm.as_ref())
                } else {
                    let bare = collab_map_name.strip_prefix("spaces/").unwrap_or(&collab_map_name);
                    let stripped = bare.find('_').map(|i| &bare[i + 1..]).unwrap_or(bare);
                    stripped.replace('_', " ")
                }
            };
            let _ = tx.send(crate::collab::SessionCommand::ReplayOpened {
                replay_id,
                replay_name,
                map_image_png: map_png,
                game_version,
                map_name: collab_map_name,
                display_name,
            });
            state.session_announced = true;
        }
    }

    // Store game fonts in shared state so the UI thread can register them with egui
    shared_state.lock().game_fonts = Some(game_fonts.clone());

    // Load replay/spectator keybindings from commands.scheme.xml
    {
        let path = "system/data/commands.scheme.xml";
        let mut buf = Vec::new();
        if let Ok(mut file) = vfs.join(path).and_then(|p| p.open_file()) {
            use std::io::Read;
            if file.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                let groups = parse_commands_scheme(&buf);
                if !groups.is_empty() {
                    shared_state.lock().replay_controls = Some(groups);
                }
            }
        }
    }

    // Drop VFS early — no longer needed
    drop(vfs);

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
    controller.set_track_shots(false); // No shot data needed for frame building
    let mut parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    let mut renderer = MinimapRenderer::new(map_info.clone(), &game_metadata, version, RenderOptions::default());
    renderer.set_fonts(game_fonts.clone());

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
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => {
                if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                    renderer.populate_players(&controller);
                    renderer.update_squadron_info(&controller);
                    renderer.update_ship_abilities(&controller);

                    let target_frame = (prev_clock.seconds() / frame_duration) as i64;
                    while last_rendered_frame < target_frame {
                        last_rendered_frame += 1;
                        let commands = renderer.draw_frame(&controller);
                        frame_snapshots.push(FrameSnapshot { packet_offset: offset_before, clock: prev_clock });

                        // Store the first frame immediately (and broadcast if session is active).
                        if frame_snapshots.len() == 1 {
                            let mut s = shared_state.lock();
                            if let Some(ref tx) = s.session_frame_tx {
                                let replay_id = s.collab_replay_id.unwrap_or(0);
                                tracing::debug!(
                                    "First frame: broadcasting via session_frame_tx (replay_id={replay_id})"
                                );
                                let _ = tx.try_send(FrameBroadcast {
                                    replay_id,
                                    clock: prev_clock.0,
                                    frame_index: 0,
                                    total_frames: estimated_frames as u32,
                                    game_duration,
                                    commands: commands.clone(),
                                });
                            } else {
                                tracing::debug!("First frame: session_frame_tx not wired yet, stored locally only");
                            }
                            s.frame = Some(PlaybackFrame {
                                replay_id: 0,
                                commands,
                                clock: prev_clock,
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
            frame_snapshots.push(FrameSnapshot { packet_offset: full_packet_data.len(), clock: prev_clock });
        }
    }
    controller.finish();

    let actual_total_frames = frame_snapshots.len();
    let actual_game_duration = frame_snapshots.last().map(|s| s.clock.seconds()).unwrap_or(game_duration);

    // 4. Event extraction pass — second full parse for timeline events + shot counting + health
    let timeline_result = ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone())
        .ok()
        .map(|event_replay| extract_timeline_events(&event_replay, &game_metadata, Some(&game_constants)));
    let (timeline_events, battle_start, shot_counts, health_histories) = match timeline_result {
        Some(r) => (r.events, r.battle_start, r.shot_counts, r.health_histories),
        None => (Vec::new(), GameClock(0.0), HashMap::new(), HashMap::new()),
    };
    {
        let mut state = shared_state.lock();
        state.timeline_events = Some(timeline_events);
        state.battle_start = battle_start;
        state.actual_game_duration = Some(actual_game_duration);
        state.self_player_name = Some(replay_file.meta.playerName.clone());
    }

    // 4b. Shot extraction pass — third full parse, pre-allocated from counts
    let shot_timelines = extract_all_shots(
        &raw_meta,
        &packet_data,
        &game_metadata,
        Some(&game_constants),
        &shot_counts,
        health_histories,
    );
    {
        let mut state = shared_state.lock();
        let timeline_map: HashMap<EntityId, Arc<ShipShotTimeline>> =
            shot_timelines.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        // Push timelines to any already-open armor bridges
        for bridge in &state.armor_bridges {
            let mut b = bridge.lock();
            if let Some(tl) = timeline_map.get(&b.target_entity_id) {
                b.shot_timeline = Some(tl.clone());
            }
        }
        state.shot_timelines = Some(timeline_map);
    }

    // Mark as ready and store game version for collab sessions
    {
        let mut state = shared_state.lock();
        state.status = RendererStatus::Ready;
        state.game_version = Some(replay_file.meta.clientVersionFromExe.clone());
        // Wake the UI so the viewport transitions out of the loading spinner.
        if let Some(ref ctx) = state.viewport_ctx {
            ctx.request_repaint();
        }
    }

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
    live_renderer.set_fonts(game_fonts.clone());

    // Tracks how many entries in `controller.shot_hits()` we've already pushed
    // to bridges. Reset to 0 whenever the controller is rebuilt.
    let mut hit_cursor: usize = 0;

    // Persistent parser + incremental parse tracking.
    // These allow forward playback to continue parsing from where we left off
    // instead of rebuilding from scratch every frame.
    let mut live_parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
    let mut live_offset: usize = 0;
    let mut live_clock = GameClock(0.0);

    // Parse live state up to frame 0 so it matches the initially displayed frame
    if !frame_snapshots.is_empty() {
        let armor_bridges = shared_state.lock().armor_bridges.clone();
        let mut staging = init_bridge_staging(&armor_bridges);
        (live_offset, live_clock) = parse_to_clock(
            &mut live_parser,
            &packet_data,
            live_offset,
            live_clock,
            &mut live_controller,
            &mut live_renderer,
            frame_snapshots[0].clock,
            frame_duration,
            &mut staging,
            &mut hit_cursor,
        );
        populate_bridge_players(&live_controller, &game_metadata, &armor_bridges);
        finalize_bridge_staging(&armor_bridges, staging, true);
    }

    /// Per-bridge staging area used during `parse_to_clock` to collect shot hits
    /// without holding the bridge lock. Swapped into the bridge atomically at the end.
    struct BridgeStaging {
        target_entity_id: EntityId,
        shot_hits: Vec<ResolvedShotHit>,
        last_clock: GameClock,
    }

    /// Initialize staging vecs from the current set of bridges (reads target_entity_id only).
    fn init_bridge_staging(bridges: &[Arc<Mutex<RealtimeArmorBridge>>]) -> Vec<BridgeStaging> {
        bridges
            .iter()
            .map(|b| {
                let locked = b.lock();
                BridgeStaging {
                    target_entity_id: locked.target_entity_id,
                    shot_hits: Vec::new(),
                    last_clock: GameClock(0.0),
                }
            })
            .collect()
    }

    /// Finalize staging: merge accumulated shot hits into bridges.
    ///
    /// When `replace` is true (full rebuild), the bridge shot_hits vec is replaced
    /// wholesale and generation is bumped. When false (incremental parse),
    /// new hits are appended and generation is only bumped if there are new
    /// hits, so the viewer doesn't see a spurious reset on every frame.
    fn finalize_bridge_staging(
        bridges: &[Arc<Mutex<RealtimeArmorBridge>>],
        staging: Vec<BridgeStaging>,
        replace: bool,
    ) {
        for (bridge, staged) in bridges.iter().zip(staging) {
            let mut b = bridge.lock();
            let has_new = !staged.shot_hits.is_empty();
            if replace {
                b.shot_hits = staged.shot_hits;
                b.generation += 1;
            } else if has_new {
                b.shot_hits.extend(staged.shot_hits);
                b.generation += 1;
            }
            b.last_clock = staged.last_clock;
        }
    }

    /// Extract new shot hits from the controller and push them into staging vecs.
    /// `hit_cursor` tracks how many hits have already been processed.
    fn push_shot_hits_to_staging(
        controller: &BattleController<'_, '_, GameMetadataProvider>,
        staging: &mut [BridgeStaging],
        hit_cursor: &mut usize,
    ) {
        if staging.is_empty() {
            return;
        }

        let all_hits = controller.shot_hits();
        let new_count = all_hits.len();

        // The controller clears shot_hits each clock tick, so reset the cursor
        // whenever the list shrinks.
        if new_count < *hit_cursor {
            *hit_cursor = 0;
        }
        if new_count <= *hit_cursor {
            return;
        }

        let new_hits = &all_hits[*hit_cursor..];
        *hit_cursor = new_count;

        let mut total_matched = 0u32;
        for hit in new_hits {
            for s in staging.iter_mut() {
                if hit.victim_entity_id == s.target_entity_id {
                    s.shot_hits.push(hit.clone());
                    total_matched += 1;
                }
            }
        }
        if total_matched > 0 {
            tracing::debug!(
                "push_shot_hits_to_staging: {} new hits, {} matched staging targets",
                new_hits.len(),
                total_matched,
            );
        }
    }

    /// Populate player info on all armor bridges from the controller.
    fn populate_bridge_players(
        controller: &BattleController<'_, '_, GameMetadataProvider>,
        game_metadata: &GameMetadataProvider,
        bridges: &[Arc<Mutex<RealtimeArmorBridge>>],
    ) {
        if bridges.is_empty() {
            return;
        }
        let players = controller.player_entities();
        if players.is_empty() {
            return;
        }

        let player_infos: Vec<ReplayPlayerInfo> = players
            .iter()
            .map(|(eid, player)| {
                let display_name = game_metadata
                    .localized_name_from_param(player.vehicle())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let team_id = player.initial_state().team_id();
                let hull_param_id = player.vehicle_entity().map(|ve| ve.props().ship_config().hull());
                ReplayPlayerInfo {
                    entity_id: *eid,
                    username: player.initial_state().username().to_string(),
                    team_id,
                    vehicle: Arc::new(player.vehicle().clone()),
                    ship_display_name: display_name,
                    hull_param_id,
                }
            })
            .collect();

        for bridge in bridges {
            let mut b = bridge.lock();
            if b.players.is_empty() {
                b.players = player_infos.clone();
            }
        }
    }

    /// Helper: parse replay packets up to `target_clock`, feeding them into
    /// the given controller and renderer.
    ///
    /// Supports incremental parsing: starts from `start_offset` into
    /// `packet_data` and uses the provided `prev_clock` as the initial
    /// clock value.  Returns `(new_offset, last_clock)` so the caller can
    /// resume from where we left off on the next frame.
    fn parse_to_clock(
        parser: &mut wows_replays::packet2::Parser<'_>,
        packet_data: &[u8],
        start_offset: usize,
        mut prev_clock: GameClock,
        controller: &mut BattleController<'_, '_, GameMetadataProvider>,
        renderer: &mut MinimapRenderer<'_>,
        target_clock: GameClock,
        frame_duration: f32,
        staging: &mut [BridgeStaging],
        hit_cursor: &mut usize,
    ) -> (usize, GameClock) {
        let mut remaining = &packet_data[start_offset..];

        while !remaining.is_empty() {
            match parser.parse_packet(&mut remaining) {
                Ok(packet) => {
                    if packet.clock > target_clock + frame_duration {
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
                            if let Some(dead) = dead_ships.get(eid)
                                && prev_clock >= dead.clock
                            {
                                return false;
                            }
                            // Only record detected ships (visible on minimap)
                            minimap_positions.get(eid).map(|mm| mm.visible).unwrap_or(false)
                        });
                    }
                    prev_clock = packet.clock;
                    controller.process(&packet);
                    push_shot_hits_to_staging(controller, staging, hit_cursor);
                }
                Err(_) => break,
            }
        }

        renderer.populate_players(controller);
        renderer.update_squadron_info(controller);
        renderer.update_ship_abilities(controller);

        let new_offset = packet_data.len() - remaining.len();
        (new_offset, prev_clock)
    }

    // Request a repaint of the viewport from the background thread.
    // Uses the egui Context stored by the UI thread on first draw.
    let request_repaint = |state: &Arc<Mutex<SharedRendererState>>| {
        if let Some(ref ctx) = state.lock().viewport_ctx {
            ctx.request_repaint();
        }
    };

    // Set a new playback frame on shared state and broadcast to collab session if active.
    let set_frame = |state: &Arc<Mutex<SharedRendererState>>,
                     commands: Vec<DrawCommand>,
                     clock: GameClock,
                     frame_index: usize,
                     total_frames: usize,
                     game_duration: f32| {
        let mut s = state.lock();
        // Clone commands for collab broadcast before moving into the frame.
        if let Some(ref tx) = s.session_frame_tx {
            let replay_id = s.collab_replay_id.unwrap_or(0);
            let _ = tx.try_send(FrameBroadcast {
                replay_id,
                clock: clock.0,
                frame_index: frame_index as u32,
                total_frames: total_frames as u32,
                game_duration,
                commands: commands.clone(),
            });
        }
        s.frame = Some(PlaybackFrame { replay_id: 0, commands, clock, frame_index, total_frames, game_duration });
    };

    /// Rebuild live_replay/live_controller/live_renderer/live_parser from
    /// scratch, parsing up to `$target_clock`.  Only needed for backward
    /// seeks — forward playback uses incremental parsing instead.
    ///
    /// The macro is needed because Rust's borrow checker won't allow passing
    /// `&mut live_replay` and `&mut live_controller` (which borrows from
    /// `live_replay`) to the same function.
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
            live_renderer.set_fonts(game_fonts.clone());
            // Reset parser and tracking state for full re-parse
            live_parser = wows_replays::packet2::Parser::new(game_metadata.entity_specs());
            hit_cursor = 0;
            // Rebuild bridge shot hits via staging (no intermediate empty state)
            let armor_bridges = shared_state.lock().armor_bridges.clone();
            let mut staging = init_bridge_staging(&armor_bridges);
            (live_offset, live_clock) = parse_to_clock(
                &mut live_parser,
                &packet_data,
                0,
                GameClock(0.0),
                &mut live_controller,
                &mut live_renderer,
                $target_clock,
                frame_duration,
                &mut staging,
                &mut hit_cursor,
            );
            populate_bridge_players(&live_controller, &game_metadata, &armor_bridges);
            finalize_bridge_staging(&armor_bridges, staging, true);
        }};
    }

    let mut last_bridge_count: usize = shared_state.lock().armor_bridges.len();

    loop {
        if !open.load(Ordering::Relaxed) {
            break;
        }

        // Detect new armor bridges — rebuild from scratch so their shot hits are re-parsed.
        {
            let current_bridge_count = shared_state.lock().armor_bridges.len();
            if current_bridge_count != last_bridge_count {
                tracing::debug!(
                    "Replay renderer: armor bridge count changed {} -> {}, rebuilding",
                    last_bridge_count,
                    current_bridge_count,
                );
                last_bridge_count = current_bridge_count;
                let target_clock =
                    frame_snapshots.get(current_frame).map(|s| s.clock).unwrap_or(GameClock(actual_game_duration));
                rebuild_live_state!(target_clock);
                live_renderer.options = shared_state.lock().options.clone();
                let commands = live_renderer.draw_frame(&live_controller);
                set_frame(
                    &shared_state,
                    commands,
                    target_clock,
                    current_frame,
                    actual_total_frames,
                    actual_game_duration,
                );
                request_repaint(&shared_state);
            }
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
                        frame_snapshots.get(current_frame).map(|s| s.clock).unwrap_or(GameClock(actual_game_duration));

                    if target_clock < live_clock {
                        // Seeking backward — must rebuild from scratch
                        rebuild_live_state!(target_clock);
                    } else {
                        // Seeking forward — continue parsing incrementally
                        let armor_bridges = shared_state.lock().armor_bridges.clone();
                        let mut staging = init_bridge_staging(&armor_bridges);
                        (live_offset, live_clock) = parse_to_clock(
                            &mut live_parser,
                            &packet_data,
                            live_offset,
                            live_clock,
                            &mut live_controller,
                            &mut live_renderer,
                            target_clock,
                            frame_duration,
                            &mut staging,
                            &mut hit_cursor,
                        );
                        populate_bridge_players(&live_controller, &game_metadata, &armor_bridges);
                        finalize_bridge_staging(&armor_bridges, staging, false);
                    }

                    live_renderer.options = shared_state.lock().options.clone();
                    let commands = live_renderer.draw_frame(&live_controller);
                    set_frame(
                        &shared_state,
                        commands,
                        target_clock,
                        current_frame,
                        actual_total_frames,
                        actual_game_duration,
                    );
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
                    GameClock(actual_game_duration)
                };

                // Forward playback — always incremental, never rebuild
                let armor_bridges = shared_state.lock().armor_bridges.clone();
                let mut staging = init_bridge_staging(&armor_bridges);
                (live_offset, live_clock) = parse_to_clock(
                    &mut live_parser,
                    &packet_data,
                    live_offset,
                    live_clock,
                    &mut live_controller,
                    &mut live_renderer,
                    target_clock,
                    frame_duration,
                    &mut staging,
                    &mut hit_cursor,
                );
                populate_bridge_players(&live_controller, &game_metadata, &armor_bridges);
                finalize_bridge_staging(&armor_bridges, staging, false);

                live_renderer.options = shared_state.lock().options.clone();
                let commands = live_renderer.draw_frame(&live_controller);
                set_frame(
                    &shared_state,
                    commands,
                    target_clock,
                    current_frame,
                    actual_total_frames,
                    actual_game_duration,
                );
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
                let clock =
                    frame_snapshots.get(current_frame).map(|s| s.clock).unwrap_or(GameClock(actual_game_duration));
                set_frame(&shared_state, commands, clock, current_frame, actual_total_frames, actual_game_duration);
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
    clock: GameClock,
}

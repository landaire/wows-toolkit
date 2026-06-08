use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use parking_lot::Mutex;

use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::merged::MergedReplays;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;

use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;

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
use super::timeline::extract_timeline_and_shots;
use crate::util::controls::parse_commands_scheme;

#[allow(clippy::too_many_arguments)]
pub(super) fn playback_thread(
    raw_meta: Vec<u8>,
    packet_data: Vec<u8>,
    alt_replays: Vec<crate::replay::renderer::AltReplayBytes>,
    map_name: String,
    game_duration: f32,
    wows_data: SharedWoWsData,
    asset_cache: Arc<parking_lot::Mutex<RendererAssetCache>>,
    shared_state: Arc<Mutex<SharedRendererState>>,
    command_rx: mpsc::Receiver<PlaybackCommand>,
    open: Arc<AtomicBool>,
) {
    // 1. Get VFS, game metadata, and game constants from the app
    let (vfs, version, game_metadata, game_constants, dump_dir) = {
        let data = wows_data.read();
        let gm = match data.game_metadata.clone() {
            Some(gm) => gm,
            None => {
                shared_state.lock().status = RendererStatus::Error("Game metadata not loaded".to_string());
                return;
            }
        };
        (data.vfs.clone(), data.version().copied(), gm, Arc::clone(&data.game_constants), data.dump_dir.clone())
    };
    let version = version.as_ref();
    let dump_dir = dump_dir.as_deref();

    // 2. Load visual assets (cached across renderer instances). Icons auto-borrow
    // from the newest dump for old replays that ship none; map and fonts use the
    // replay's own VFS.
    let (map_info, game_fonts, map_image_for_announce) = {
        let mut cache = asset_cache.lock();
        let ship_icons = cache.get_or_load_ship_icons(&vfs, version, dump_dir);
        let plane_icons = cache.get_or_load_plane_icons(&vfs, version, dump_dir);
        let building_icons = cache.get_or_load_building_icons(&vfs, version, dump_dir);
        let consumable_icons = cache.get_or_load_consumable_icons(&vfs, version, dump_dir);
        let death_cause_icons = cache.get_or_load_death_cause_icons(&vfs, version, dump_dir);
        let powerup_icons = cache.get_or_load_powerup_icons(&vfs, version, dump_dir);
        let crew_skill_icons = cache.get_or_load_crew_skill_icons(&vfs, version, dump_dir);
        let modernization_icons = cache.get_or_load_modernization_icons(&vfs, version, dump_dir);
        let signal_flag_icons = cache.get_or_load_signal_flag_icons(&vfs, version, dump_dir);
        let game_fonts = cache.get_or_load_game_fonts(&vfs, version, dump_dir);
        let (map_image, map_info) = cache.get_or_load_map(&map_name, &vfs, version);

        let map_image_for_announce = map_image.clone();
        shared_state.lock().assets = Some(ReplayRendererAssets {
            map_image,
            ship_icons,
            plane_icons,
            building_icons,
            consumable_icons,
            death_cause_icons,
            powerup_icons,
            crew_skill_icons,
            modernization_icons,
            signal_flag_icons,
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

    // Load self player's ship silhouette from VFS before dropping it.
    // Parse raw_meta JSON to find self player (relation == 0) and their shipId,
    // then look up the vehicle index in GameParams to find the silhouette PNG.
    let self_silhouette = (|| -> Option<image::RgbaImage> {
        let meta: wows_replays::ReplayMeta = serde_json::from_slice(&raw_meta).ok()?;
        let self_vehicle = meta.vehicles.iter().find(|v| v.relation == 0)?;
        let param = GameParamProvider::game_param_by_id(&*game_metadata, self_vehicle.shipId)?;
        let index = param.index();
        let buf = wowsunpack::game_assets::GuiAsset::ShipSilhouette(index).read(&vfs, version)?;
        let img = image::load_from_memory(&buf).ok()?;
        let mut rgba = img.into_rgba8();
        // Normalize to white pixels with original alpha — source silhouettes are dark,
        // and tint multiplication needs white (255) to produce the desired tint color.
        for px in rgba.pixels_mut() {
            px[0] = 255;
            px[1] = 255;
            px[2] = 255;
        }
        Some(rgba)
    })();

    // Drop VFS early — no longer needed
    drop(vfs);

    // 3. Parse replay files (primary + alt perspectives)
    let replay_file = match ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone()) {
        Ok(rf) => rf,
        Err(e) => {
            shared_state.lock().status = RendererStatus::Error(format!("Failed to parse replay: {:?}", e));
            return;
        }
    };
    let alt_replay_files: Vec<ReplayFile> = match alt_replays
        .iter()
        .map(|a| ReplayFile::from_decrypted_parts(a.raw_meta.clone(), a.packet_data.clone()))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(e) => {
            shared_state.lock().status = RendererStatus::Error(format!("Failed to parse merge replay: {:?}", e));
            return;
        }
    };

    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

    // 4. Build the initial merged session (drives the frame-snapshot pass)
    // and the renderer.
    let mut renderer = MinimapRenderer::new(map_info.clone(), &game_metadata, version, RenderOptions::default());
    renderer.set_fonts(game_fonts.clone());
    renderer.set_merged_perspectives(!alt_replay_files.is_empty());
    if let Some(ref sil) = self_silhouette {
        renderer.set_self_silhouette(sil.clone());
        // Store raw silhouette for the UI thread to convert to an egui TextureHandle
        shared_state.lock().self_silhouette_raw = Some((sil.width(), sil.height(), sil.as_raw().clone()));
    }

    let mut session = match MergedReplays::new(
        game_metadata.entity_specs(),
        &*game_metadata,
        &game_constants,
        version,
        &replay_file,
        &alt_replay_files,
    ) {
        Ok(s) => s,
        Err(e) => {
            shared_state.lock().status = RendererStatus::Error(format!("{e}"));
            return;
        }
    };
    session.world_mut().set_shot_tracking(ShotTracking::Untracked); // No shot data needed for frame building

    // Parse all packets, tracking frame boundaries
    let frame_duration = 1.0 / SNAPSHOTS_PER_SECOND;
    let estimated_frames = (game_duration * SNAPSHOTS_PER_SECOND) as usize + 1;

    // Frame snapshots: clock per rendered frame. Unlike single-replay's
    // (packet_offset, clock) seek hints, merged sessions seek by rebuilding
    // from scratch and stepping to the target clock, so we only need the
    // clock value here.
    let mut frame_snapshots: Vec<FrameSnapshot> = Vec::with_capacity(estimated_frames);
    let mut last_rendered_frame: i64 = -1;
    let mut prev_clock = GameClock(0.0);

    loop {
        let step = match session.step() {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                tracing::error!("merge step failed during initial parse: {e}");
                break;
            }
        };
        if step.0 > prev_clock.0 {
            {
                let view = session.world_mut().view();
                renderer.populate_players(&view);
                renderer.update_squadron_info(&view);
                renderer.update_ship_abilities(&view);
            }

            let target_frame = (prev_clock.seconds() / frame_duration) as i64;
            while last_rendered_frame < target_frame {
                last_rendered_frame += 1;
                let view = session.world_mut().view();
                let commands = renderer.draw_frame(&view);
                frame_snapshots.push(FrameSnapshot { clock: prev_clock });

                // Store the first frame immediately (and broadcast if session is active).
                if frame_snapshots.len() == 1 {
                    let mut s = shared_state.lock();
                    if let Some(ref tx) = s.session_frame_tx {
                        let replay_id = s.collab_replay_id.unwrap_or(0);
                        tracing::debug!("First frame: broadcasting via session_frame_tx (replay_id={replay_id})");
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
            prev_clock = step;
        }
    }

    // Final tick
    if prev_clock.seconds() > 0.0 {
        let view = session.world_mut().view();
        renderer.populate_players(&view);
        renderer.update_squadron_info(&view);
        renderer.update_ship_abilities(&view);
        let target_frame = (prev_clock.seconds() / frame_duration) as i64;
        while last_rendered_frame < target_frame {
            last_rendered_frame += 1;
            frame_snapshots.push(FrameSnapshot { clock: prev_clock });
        }
    }
    let vehicle_facts = session.vehicle_facts().clone();
    let damage_events = {
        let mut all = vec![&replay_file];
        all.extend(alt_replay_files.iter());
        wows_battle_world::merged::gather_damage_events(
            &*game_metadata,
            &game_constants,
            version,
            game_metadata.entity_specs(),
            &all,
        )
    };

    // One-shot build snapshot from the lookahead pass. The merged session
    // here has stepped through every packet in every replay (primary plus
    // alts), so its controller's `entities_by_id` contains every player
    // ever spotted from any perspective in this input set. In single-replay
    // mode that still catches every enemy the primary spotted; in
    // merged-replay mode it catches both teams' loadouts. Builds are
    // invariant during a battle, so this snapshot stays valid for the
    // session's entire lifetime — later live refreshes only fill in the
    // (rare) gaps the lookahead missed.
    let initial_builds = snapshot_player_builds(session.world_mut(), &game_metadata, version, |_| true);
    if !initial_builds.is_empty() {
        let mut s = shared_state.lock();
        for (eid, display) in initial_builds {
            s.player_builds.insert(eid, display);
        }
    }

    session.finish();

    let actual_total_frames = frame_snapshots.len();
    let actual_game_duration = frame_snapshots.last().map(|s| s.clock.seconds()).unwrap_or(game_duration);

    // 4. Combined timeline + shot extraction — single full parse produces both.
    let (timeline_events, battle_start, battle_end, shot_timelines) =
        match ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone()) {
            Ok(event_replay) => {
                let (tr, shots) = extract_timeline_and_shots(&event_replay, &game_metadata, Some(&game_constants));
                (tr.events, tr.battle_start, tr.battle_end, shots)
            }
            Err(_) => (Vec::new(), GameClock(0.0), None, HashMap::new()),
        };
    {
        let mut state = shared_state.lock();
        state.timeline_events = Some(timeline_events);
        state.battle_start = battle_start;
        state.battle_end = battle_end;
        state.actual_game_duration = Some(actual_game_duration);
        state.self_player_name = Some(replay_file.meta.playerName.clone());
    }
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
    drop(session);
    drop(renderer);
    drop(replay_file);
    drop(alt_replay_files);
    // Recreate primary + alt ReplayFiles for the live session. These are
    // long-lived for the duration of the playback loop; the live MergedReplays
    // borrows from them.
    let live_replay = match ReplayFile::from_decrypted_parts(raw_meta.clone(), packet_data.clone()) {
        Ok(rf) => rf,
        Err(_) => return,
    };
    let live_alt_files: Vec<ReplayFile> = match alt_replays
        .iter()
        .map(|a| ReplayFile::from_decrypted_parts(a.raw_meta.clone(), a.packet_data.clone()))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(_) => return,
    };
    let initial_opts = shared_state.lock().options.clone();
    let mut live_renderer = MinimapRenderer::new(map_info.clone(), &game_metadata, version, initial_opts);
    live_renderer.set_fonts(game_fonts.clone());
    live_renderer.set_merged_perspectives(!live_alt_files.is_empty());
    live_renderer.set_vehicle_facts(vehicle_facts.clone());
    live_renderer.set_damage_events(damage_events.clone());
    if let Some(ref sil) = self_silhouette {
        live_renderer.set_self_silhouette(sil.clone());
    }

    // Tracks how many entries in `controller.shot_hits()` we've already pushed
    // to bridges. Reset to 0 whenever the live session is rebuilt.
    let mut hit_cursor: usize = 0;
    let mut live_clock = GameClock(0.0);

    let live_salvo_flight_times = Arc::new(wows_battle_world::scan::scan_salvo_flight_times(
        &live_replay.meta,
        &*game_metadata,
        &game_constants,
        version,
        &live_replay,
    ));

    let mut live_session = match MergedReplays::new(
        game_metadata.entity_specs(),
        &*game_metadata,
        &game_constants,
        version,
        &live_replay,
        &live_alt_files,
    ) {
        Ok(s) => s,
        Err(_) => return,
    };
    live_renderer.set_position_timeline(live_session.position_timeline());
    live_renderer.set_salvo_flight_times(Arc::clone(&live_salvo_flight_times));

    // Capture the set of teams for which we have a recording-player replay.
    // The build popover gates enemy-team visibility on this: an enemy build
    // is only revealed when we have packet-level data from someone on that
    // team. Invariant for the session's lifetime (rebuilds reuse the same
    // replay set).
    let teams_with_replays: std::collections::HashSet<i64> =
        live_session.self_teams().iter().filter_map(|t| t.map(|tid| tid.raw())).collect();
    shared_state.lock().teams_with_replays = teams_with_replays;

    // Pull out the cancellation flag once so step callers below don't have to
    // re-lock shared_state every time.
    let cancel_step = Arc::clone(&shared_state.lock().cancel_step);

    // Parse live state up to frame 0 so it matches the initially displayed frame
    if !frame_snapshots.is_empty() {
        let armor_bridges = shared_state.lock().armor_bridges.clone();
        let mut staging = init_bridge_staging(&armor_bridges);
        let outcome = step_session_to_clock(
            &mut live_session,
            &mut live_renderer,
            live_clock,
            frame_snapshots[0].clock,
            frame_duration,
            &mut staging,
            &mut hit_cursor,
            &cancel_step,
        );
        live_clock = outcome.final_clock;
        populate_bridge_players(live_session.world_mut(), &game_metadata, &armor_bridges);
        finalize_bridge_staging(&armor_bridges, staging, true);
        wows_replay_insights::build::seed_consumable_inventories_from_facts(
            live_session.world_mut(),
            &vehicle_facts,
            &*game_metadata,
            version,
        );
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

    /// Extract new shot hits from the world and push them into staging vecs.
    /// `hit_cursor` tracks how many hits have already been processed per packet.
    fn push_shot_hits_to_staging(
        world: &BattleWorld<'_, '_, GameMetadataProvider>,
        staging: &mut [BridgeStaging],
        hit_cursor: &mut usize,
    ) {
        if staging.is_empty() {
            return;
        }

        let all_hits = world.shot_hits();
        let new_count = all_hits.len();

        // shot_hits is cleared each packet, so reset the cursor whenever
        // the list shrinks (new packet started).
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
        world: &mut BattleWorld<'_, '_, GameMetadataProvider>,
        game_metadata: &GameMetadataProvider,
        bridges: &[Arc<Mutex<RealtimeArmorBridge>>],
    ) {
        if bridges.is_empty() {
            return;
        }
        // Snapshot player list to release the &self borrow before calling vehicle_props (&mut self).
        let player_snapshot: Vec<(EntityId, wows_replays::Rc<wows_replays::analyzer::battle_controller::Player>)> =
            world.player_entities().iter().map(|(eid, p)| (*eid, wows_replays::Rc::clone(p))).collect();
        if player_snapshot.is_empty() {
            return;
        }
        let all_props = world.vehicle_props_all();

        let player_infos: Vec<ReplayPlayerInfo> = player_snapshot
            .iter()
            .map(|(eid, player)| {
                let display_name = game_metadata.localized_name_from_param(player.vehicle()).unwrap_or_default();
                let team_id = player.initial_state().team_id();
                let hull_param_id = all_props.get(eid).and_then(|props| props.ship_config().hull());
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

    /// Outcome of a [`step_session_to_clock`] call.
    struct StepOutcome {
        final_clock: GameClock,
        /// `true` if the loop bailed because `cancel` was set, meaning the
        /// session is parked at an intermediate clock. Callers should NOT
        /// publish this state to the viewport — render output reflects a
        /// stale seek the user has already moved past.
        cancelled: bool,
    }

    /// Drive `session` forward via [`MergedReplays::step`] until its safe
    /// clock reaches `target_clock`. Feeds the rendered state into
    /// `renderer` at each clock boundary and harvests new shot hits into
    /// the per-bridge staging vecs.
    ///
    /// Checks `cancel` on every step and returns early if it's been set.
    /// The flag is reset to `false` before returning so subsequent calls
    /// start with a clean slate. The returned [`StepOutcome::cancelled`]
    /// flag lets the caller decide whether to publish a frame.
    fn step_session_to_clock(
        session: &mut MergedReplays<'_, '_, '_, GameMetadataProvider>,
        renderer: &mut MinimapRenderer<'_>,
        mut prev_clock: GameClock,
        target_clock: GameClock,
        frame_duration: f32,
        staging: &mut [BridgeStaging],
        hit_cursor: &mut usize,
        cancel: &AtomicBool,
    ) -> StepOutcome {
        // Discard any stale cancel signal from before this call. The UI sets
        // `cancel = true` preemptively when it sends a Seek so an in-progress
        // step bails; if no step is running, that signal sits around and
        // would cause the *next* step to bail on its first iteration without
        // doing any work.
        cancel.store(false, Ordering::Relaxed);

        let mut cancelled = false;
        loop {
            if cancel.swap(false, Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let step = match session.step() {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("merge step failed: {e}");
                    break;
                }
            };
            if step.0 > target_clock.0 + frame_duration {
                break;
            }
            // Stop if clock resets to 0 after game started — those are post-game packets
            if prev_clock.seconds() > 0.0 && step.seconds() == 0.0 {
                break;
            }
            if step.0 > prev_clock.0 && prev_clock.seconds() > 0.0 {
                let view = session.world_mut().view();
                renderer.populate_players(&view);
                renderer.update_squadron_info(&view);
                renderer.update_ship_abilities(&view);
                let dead_ships = view.dead_ships();
                let minimap_positions = view.minimap_positions();
                renderer.record_positions(&view, prev_clock, |eid| {
                    if let Some(dead) = dead_ships.get(eid)
                        && prev_clock >= dead.clock
                    {
                        return false;
                    }
                    minimap_positions.get(eid).map(|mm| mm.visible).unwrap_or(false)
                });
            }
            prev_clock = step;
            push_shot_hits_to_staging(session.world(), staging, hit_cursor);
        }

        // Always leave the flag cleared on exit so a stale "true" set
        // between the last check and now doesn't cancel the next step.
        cancel.store(false, Ordering::Relaxed);

        if !cancelled {
            let view = session.world_mut().view();
            renderer.populate_players(&view);
            renderer.update_squadron_info(&view);
            renderer.update_ship_abilities(&view);
        }

        StepOutcome { final_clock: prev_clock, cancelled }
    }

    // Request a repaint of the viewport from the background thread.
    // Uses the egui Context stored by the UI thread on first draw.
    let request_repaint = |state: &Arc<Mutex<SharedRendererState>>| {
        if let Some(ref ctx) = state.lock().viewport_ctx {
            ctx.request_repaint();
        }
    };

    // Refresh the per-player build snapshot for the roster hover popover.
    // Builds are invariant during a battle, so each call only ADDS entries
    // the cache hasn't seen before. That way builds captured during the
    // initial lookahead pass (where the merged controller sees every player
    // any perspective ever spotted, including enemies in non-merge mode)
    // stay available even after the live controller has dropped an entity
    // (death, smoke screen removal).
    //
    // The resolver's `from_player` constructor only works on a finalized
    // BattleReport (it reads `Player.vehicle_entity`, which is populated at
    // battle end), so we look up the live VehicleEntity through
    // `entities_by_id` and call `ResolvedBuild::from_ids` directly.
    let refresh_player_builds =
        |state: &Arc<Mutex<SharedRendererState>>, world: &mut BattleWorld<'_, '_, GameMetadataProvider>| {
            let new_entries = snapshot_player_builds(world, &game_metadata, version, |eid| {
                !state.lock().player_builds.contains_key(&eid)
            });
            if new_entries.is_empty() {
                return;
            }
            let mut s = state.lock();
            for (eid, display) in new_entries {
                s.player_builds.insert(eid, display);
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

    /// Rebuild the live session from scratch and step it forward to
    /// `$target_clock`. Only needed for backward seeks; forward playback
    /// reuses the existing session and advances it incrementally.
    ///
    /// Evaluates to `true` if the step ran to completion, `false` if it
    /// was cancelled (caller should skip publishing a frame in that case).
    /// When cancelled, bridge staging is discarded so the previous bridge
    /// state stays visible instead of flashing to a partial rebuild.
    ///
    /// The macro form sidesteps the borrow checker around dropping and
    /// rebuilding `live_session` (which borrows from `live_replay` /
    /// `live_alt_files`).
    macro_rules! rebuild_live_state {
        ($target_clock:expr) => {{
            let current_opts = shared_state.lock().options.clone();
            live_renderer = MinimapRenderer::new(map_info.clone(), &*game_metadata, version, current_opts);
            live_renderer.set_fonts(game_fonts.clone());
            live_renderer.set_merged_perspectives(!live_alt_files.is_empty());
            live_renderer.set_vehicle_facts(vehicle_facts.clone());
            live_renderer.set_damage_events(damage_events.clone());
            if let Some(ref sil) = self_silhouette {
                live_renderer.set_self_silhouette(sil.clone());
            }
            hit_cursor = 0;
            live_clock = GameClock(0.0);
            live_session = match MergedReplays::new(
                game_metadata.entity_specs(),
                &*game_metadata,
                &game_constants,
                version,
                &live_replay,
                &live_alt_files,
            ) {
                Ok(s) => s,
                Err(_) => continue,
            };
            live_renderer.set_position_timeline(live_session.position_timeline());
            live_renderer.set_salvo_flight_times(Arc::clone(&live_salvo_flight_times));
            // Rebuild bridge shot hits via staging (no intermediate empty state)
            let armor_bridges = shared_state.lock().armor_bridges.clone();
            let mut staging = init_bridge_staging(&armor_bridges);
            let outcome = step_session_to_clock(
                &mut live_session,
                &mut live_renderer,
                live_clock,
                $target_clock,
                frame_duration,
                &mut staging,
                &mut hit_cursor,
                &cancel_step,
            );
            live_clock = outcome.final_clock;
            // Seed inventories whether or not the step was cancelled. The
            // facts cache is pre-computed and doesn't depend on which packets
            // the controller has processed; skipping the seed on cancel left
            // the freshly-built controller with empty inventories, which
            // surfaced as transient "no inventory" rows on rapid seek scrubs
            // when the cancelled-rebuild controller stayed in play.
            wows_replay_insights::build::seed_consumable_inventories_from_facts(
                live_session.world_mut(),
                &vehicle_facts,
                &*game_metadata,
                version,
            );
            if !outcome.cancelled {
                populate_bridge_players(live_session.world_mut(), &game_metadata, &armor_bridges);
                finalize_bridge_staging(&armor_bridges, staging, true);
            }
            !outcome.cancelled
        }};
    }

    // Overwrite the placeholder first frame from the initial parse pass with a
    // freshly drawn one from live_renderer. The initial pass renders before
    // vehicle_facts exists, so its rosters carry no HP or consumables; redraw
    // here with the cache installed and inventories seeded.
    if !frame_snapshots.is_empty() {
        live_renderer.options = shared_state.lock().options.clone();
        let first_clock = frame_snapshots[0].clock;
        live_renderer.set_render_clock(first_clock);
        let inventory_count = live_session.world_mut().consumable_inventories().len();
        let player_count = live_session.world().player_entities().len();
        let commands = {
            let view = live_session.world_mut().view();
            live_renderer.draw_frame(&view)
        };
        tracing::info!(clock = first_clock.0, inventory_count, player_count, "initial first-frame redraw");
        refresh_player_builds(&shared_state, live_session.world_mut());
        set_frame(&shared_state, commands, first_clock, 0, actual_total_frames, actual_game_duration);
        request_repaint(&shared_state);
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
                let completed = rebuild_live_state!(target_clock);
                if completed {
                    live_renderer.options = shared_state.lock().options.clone();
                    live_renderer.set_render_clock(target_clock);
                    let commands = {
                        let view = live_session.world_mut().view();
                        live_renderer.draw_frame(&view)
                    };
                    refresh_player_builds(&shared_state, live_session.world_mut());
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
        }

        // Drain pending commands and coalesce Seeks: when the user is dragging
        // the slider, the UI fires many Seeks in rapid succession. The
        // intermediate targets are not interesting — only the final position
        // is — so we collapse them into a single seek to the most recent
        // requested clock. Other command kinds are processed in order.
        let mut pending: Vec<PlaybackCommand> = Vec::new();
        while let Ok(cmd) = command_rx.try_recv() {
            pending.push(cmd);
        }
        let last_seek_idx = pending.iter().rposition(|c| matches!(c, PlaybackCommand::Seek(_)));
        for (i, cmd) in pending.into_iter().enumerate() {
            if matches!(cmd, PlaybackCommand::Seek(_)) && Some(i) != last_seek_idx {
                continue;
            }
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

                    let completed = if target_clock < live_clock {
                        // Seeking backward — must rebuild from scratch
                        rebuild_live_state!(target_clock)
                    } else {
                        // Seeking forward — keep stepping the session
                        let armor_bridges = shared_state.lock().armor_bridges.clone();
                        let mut staging = init_bridge_staging(&armor_bridges);
                        let outcome = step_session_to_clock(
                            &mut live_session,
                            &mut live_renderer,
                            live_clock,
                            target_clock,
                            frame_duration,
                            &mut staging,
                            &mut hit_cursor,
                            &cancel_step,
                        );
                        live_clock = outcome.final_clock;
                        if !outcome.cancelled {
                            populate_bridge_players(live_session.world_mut(), &game_metadata, &armor_bridges);
                            finalize_bridge_staging(&armor_bridges, staging, false);
                        }
                        !outcome.cancelled
                    };

                    // Skip publishing a frame if the step was cancelled: the
                    // session is parked at an intermediate clock the user has
                    // already moved past, and a fresher Seek is on its way.
                    if completed {
                        live_renderer.options = shared_state.lock().options.clone();
                        live_renderer.set_render_clock(target_clock);
                        let commands = {
                            let view = live_session.world_mut().view();
                            live_renderer.draw_frame(&view)
                        };
                        refresh_player_builds(&shared_state, live_session.world_mut());
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
            // Real elapsed time drives the clock so playback renders at the
            // display rate, not the coarse seek-snapshot rate (which made 1x a
            // slideshow). Cap dt so a stall doesn't fast-forward the match.
            let dt = now.duration_since(last_advance).as_secs_f32().min(0.25);
            last_advance = now;

            let target = (live_clock.0 + dt * speed).min(actual_game_duration);
            let target_clock = GameClock(target);
            if target >= actual_game_duration {
                playing = false;
                shared_state.lock().playing = false;
            }

            // Step the world precisely to the render clock (tiny slack) so ship
            // state advances at the packet rate instead of jumping at the
            // snapshot rate.
            let armor_bridges = shared_state.lock().armor_bridges.clone();
            let mut staging = init_bridge_staging(&armor_bridges);
            let outcome = step_session_to_clock(
                &mut live_session,
                &mut live_renderer,
                live_clock,
                target_clock,
                1.0 / 60.0,
                &mut staging,
                &mut hit_cursor,
                &cancel_step,
            );
            live_clock = target_clock;

            // Keep the scrubber index in sync with the continuous clock.
            let frac = target / actual_game_duration.max(1.0);
            current_frame = ((frac * (actual_total_frames - 1) as f32) as usize).min(actual_total_frames - 1);

            if !outcome.cancelled {
                populate_bridge_players(live_session.world_mut(), &game_metadata, &armor_bridges);
                finalize_bridge_staging(&armor_bridges, staging, false);

                live_renderer.options = shared_state.lock().options.clone();
                live_renderer.set_render_clock(target_clock);
                let commands = {
                    let view = live_session.world_mut().view();
                    live_renderer.draw_frame(&view)
                };
                refresh_player_builds(&shared_state, live_session.world_mut());
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
            if live_renderer.options != new_opts {
                live_renderer.options = new_opts;
                let clock =
                    frame_snapshots.get(current_frame).map(|s| s.clock).unwrap_or(GameClock(actual_game_duration));
                live_renderer.set_render_clock(clock);
                let commands = {
                    let view = live_session.world_mut().view();
                    live_renderer.draw_frame(&view)
                };
                refresh_player_builds(&shared_state, live_session.world_mut());
                set_frame(&shared_state, commands, clock, current_frame, actual_total_frames, actual_game_duration);
                request_repaint(&shared_state);
            }
        }

        // Pace the playback loop at ~30 fps (one render per iteration); idle
        // slower when paused.
        std::thread::sleep(std::time::Duration::from_millis(if playing { 33 } else { 16 }));
    }
}

struct FrameSnapshot {
    clock: GameClock,
}

/// Translate a `ResolvedBuild` into the display struct the hover popover
/// consumes. Looks up localized names/descriptions for the captain, every
/// learned skill, each modernization, and each signal flag. Skill icon keys
/// are derived from `internal_name` via snake_case conversion so they match
/// the filenames in `gui/crew_commander/skills/`.
fn build_display_from_resolved(
    build: &wows_replay_insights::build::ResolvedBuild,
    metadata: &GameMetadataProvider,
    version: &Version,
) -> super::PlayerBuildDisplay {
    use wowsunpack::data::ResourceLoader;

    let captain_name = build.captain.as_deref().and_then(|c| metadata.localized_name_from_param(c));

    let crew = build.captain.as_deref().and_then(|c| c.data().crew_ref());
    let learned: std::collections::HashSet<wowsunpack::game_params::types::CrewSkillType> =
        build.skills.iter().map(|s| wowsunpack::game_params::types::CrewSkillType::from(*s)).collect();

    let skill_rows: Vec<super::SkillRow> =
        wowsunpack::game_params::skill_grid_data::build_skill_grid(crew, &learned, build.species, version.build, metadata, version)
            .into_iter()
            .map(|row| super::SkillRow {
                tier: row.point_cost.map(|c| c.get()),
                skills: row
                    .skills
                    .into_iter()
                    .map(|s| super::SkillDisplay {
                        icon_key: wowsunpack::game_assets::crew_skill_icon_slug(&s.internal_name),
                        // Fall back to the internal name when a translation is absent.
                        name: s.name.unwrap_or_else(|| s.internal_name.to_string()),
                        description: s.description.unwrap_or_default(),
                        tier: s.point_cost.map(|c| c.get()),
                        learned: s.learned,
                    })
                    .collect(),
            })
            .collect();

    let upgrades = build.upgrades.iter().map(|p| equipment_display_for_param(p, metadata)).collect();
    // `config.exteriors()` returns every exterior the player has mounted:
    // signal flags, permoflages, ensigns, camos, skins, boosters. The popover
    // has a single Signals row aimed at combat flags, which GameParams tags
    // as `typeinfo.species == "Flags"`. Drop the rest so cosmetics don't show
    // up as broken icons.
    let signals = build
        .signals
        .iter()
        .filter(|p| matches!(p.species().and_then(|r| r.known()), Some(wowsunpack::game_params::types::Species::Flags)))
        .map(|p| equipment_display_for_param(p, metadata))
        .collect();
    super::PlayerBuildDisplay { captain_name, skill_rows, upgrades, signals }
}

/// Snapshot every player whose entity is currently tracked by `world`,
/// returning a list of (entity_id, display) pairs for which the caller's
/// `accept` predicate returned true. Used both for the one-shot lookahead
/// snapshot (where the merged world has seen every spotted player)
/// and for the per-frame additive refresh on the live world.
fn snapshot_player_builds<F: FnMut(EntityId) -> bool>(
    world: &mut BattleWorld<'_, '_, GameMetadataProvider>,
    metadata: &GameMetadataProvider,
    version: Version,
    mut accept: F,
) -> Vec<(EntityId, Arc<super::PlayerBuildDisplay>)> {
    // Snapshot players first to release the &self borrow before calling vehicle_props_all (&mut self).
    let players: Vec<(EntityId, wows_replays::Rc<wows_replays::analyzer::battle_controller::Player>)> =
        world.player_entities().iter().map(|(id, p)| (*id, wows_replays::Rc::clone(p))).collect();
    let all_props = world.vehicle_props_all();

    let mut out = Vec::new();
    for (entity_id, player) in &players {
        let entity_id = *entity_id;
        if !accept(entity_id) {
            continue;
        }
        let Some(props) = all_props.get(&entity_id) else {
            continue;
        };
        let Some(species) = player.vehicle().species().and_then(|s| s.known()).copied() else {
            continue;
        };
        let config = props.ship_config();
        let crew = props.crew_modifiers_compact_params();
        let captain_id = if crew.params_id().raw() != 0 { Some(crew.params_id()) } else { None };
        let skills = crew.learned_skills().for_species(&species);
        let Some(build) = wows_replay_insights::build::ResolvedBuild::from_ids(
            config.ship_params_id(),
            config.units(),
            config.modernization(),
            captain_id,
            skills,
            config.exteriors(),
            config.abilities(),
            species,
            version,
            metadata,
        ) else {
            continue;
        };
        out.push((entity_id, Arc::new(build_display_from_resolved(&build, metadata, &version))));
    }
    out
}

fn equipment_display_for_param(
    param: &wowsunpack::game_params::types::Param,
    metadata: &GameMetadataProvider,
) -> super::EquipmentDisplay {
    let (name, description) = wowsunpack::game_params::translations::translate_exterior(param, metadata);
    super::EquipmentDisplay {
        icon_key: param.name().to_string(),
        name: name.unwrap_or_else(|| param.name().to_string()),
        description: description.unwrap_or_default(),
    }
}

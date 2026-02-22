//! Realtime Armor Viewer — a secondary viewport window driven by replay salvo data.
//!
//! Opened from the replay renderer context menu ("Show in Armor Viewer").
//! Shows the 3D armor model of a specific ship and visualizes incoming shell
//! trajectories as the replay plays.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use egui::mutex::Mutex;

use tracing::warn;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::{AmmoType, Meters, ShellInfo};
use wowsunpack::game_types::{GameParamId, WorldPos};

use crate::armor_viewer::constants::*;
use crate::armor_viewer::state::{ArmorPane, LoadedShipArmor, StoredTrajectory};
use crate::icon_str;
use crate::icons;
use crate::replay_renderer::{RealtimeArmorBridge, ReplayPlayerInfo};
use crate::viewport_3d::GpuPipeline;

/// A realtime armor viewer window spawned from the replay renderer.
pub struct RealtimeArmorViewer {
    pub title: Arc<String>,
    pub open: Arc<AtomicBool>,

    /// Bridge shared with the replay background thread.
    bridge: Arc<Mutex<RealtimeArmorBridge>>,

    /// The target ship's entity ID in the replay.
    target_entity_id: wows_replays::types::EntityId,

    /// 3D viewport pane (reuses the existing ArmorPane infrastructure).
    pane: ArmorPane,

    /// GPU pipeline for 3D rendering.
    gpu_pipeline: Arc<GpuPipeline>,

    /// Ship assets for loading armor models.
    ship_assets: Arc<ShipAssets>,

    /// wgpu render state.
    render_state: eframe::egui_wgpu::RenderState,

    /// Enemy players (potential attackers).
    enemy_players: Vec<ReplayPlayerInfo>,

    /// Selected attacker entity ID. `None` = show all enemies.
    selected_attacker: Option<wows_replays::types::EntityId>,

    /// Last seen bridge generation. When this changes, the bridge was rebuilt
    /// (seek/rebuild) and we need to reset our cursor and reprocess.
    bridge_generation: u64,

    /// Cached shell info per projectile params_id.
    shell_cache: HashMap<GameParamId, Option<ShellInfo>>,

    /// Whether the ship model has been loaded.
    ship_loaded: bool,

    /// Whether players have been populated from the bridge.
    players_populated: bool,

    /// Per-salvo info for the side panel.
    salvo_log: Vec<SalvoLogEntry>,

    /// Currently selected salvo (by trajectory_id) for detail panel.
    selected_salvo_id: Option<u64>,

    /// Target ship's vehicle param (for loading).
    target_vehicle: Arc<wowsunpack::game_params::types::Param>,

    /// How many shot hits we've consumed from the bridge.
    processed_hit_count: usize,

    /// Set by any method that changes visible state. Checked and cleared by
    /// the viewport closure to decide whether to request a repaint.
    needs_repaint: bool,
}

/// Log entry for a single salvo displayed in the side panel.
struct SalvoLogEntry {
    clock: f32,
    /// Estimated clock time when shells reach the target.
    estimated_impact_clock: f32,
    attacker_name: String,
    shell_name: String,
    ammo_type: AmmoType,
    shell_count: usize,
    range: Meters,
    trajectory_id: Option<u64>,
    shell_info: Option<ShellInfo>,
}

impl RealtimeArmorViewer {
    /// Create a new realtime armor viewer.
    ///
    /// `target_player` must be the player info for the ship being viewed.
    /// `ship_assets` and `gpu_pipeline` are shared from the armor viewer state.
    pub fn new(
        target_player: &ReplayPlayerInfo,
        bridge: Arc<Mutex<RealtimeArmorBridge>>,
        ship_assets: Arc<ShipAssets>,
        gpu_pipeline: Arc<GpuPipeline>,
        render_state: eframe::egui_wgpu::RenderState,
    ) -> Self {
        let title =
            Arc::new(format!("Armor Viewer — {} ({})", target_player.username, target_player.ship_display_name));

        let mut pane = ArmorPane::empty(0);
        pane.show_plate_edges = true;
        pane.armor_opacity = 1.0;
        pane.trajectory_mode = true;

        Self {
            title,
            open: Arc::new(AtomicBool::new(true)),
            bridge,
            target_entity_id: target_player.entity_id,
            pane,
            gpu_pipeline,
            ship_assets,
            render_state,
            enemy_players: Vec::new(),
            selected_attacker: None,
            bridge_generation: 0,
            shell_cache: HashMap::new(),
            ship_loaded: false,
            players_populated: false,
            salvo_log: Vec::new(),
            selected_salvo_id: None,
            target_vehicle: target_player.vehicle.clone(),
            processed_hit_count: 0,
            needs_repaint: true,
        }
    }

    /// Load the target ship's armor model (called once, on first frame).
    fn start_ship_load(&mut self) {
        let vehicle = self.target_vehicle.clone();
        let ship_assets = self.ship_assets.clone();
        let display_name = {
            let b = self.bridge.lock();
            b.players
                .iter()
                .find(|p| p.entity_id == self.target_entity_id)
                .map(|p| p.ship_display_name.clone())
                .unwrap_or_default()
        };

        self.pane.selected_ship = Some(vehicle.index().to_string());
        self.pane.loading = true;

        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = (|| {
                use wowsunpack::game_params::types::GameParamProvider;
                let param = ship_assets.metadata().game_param_by_index(vehicle.index());
                let v =
                    param.as_ref().and_then(|p| p.vehicle().cloned()).ok_or_else(|| "No vehicle found".to_string())?;
                let draft_meters = param.as_ref().and_then(|p| {
                    p.vehicle()
                        .and_then(|v| v.hull_upgrades())
                        .and_then(|upgrades| upgrades.values().next())
                        .and_then(|config| config.draft())
                        .map(|m| m.value())
                });

                let options =
                    wowsunpack::export::ship::ShipExportOptions { lod: 0, hull: None, textures: false, damaged: false };
                let ctx = ship_assets.load_ship_from_vehicle(&v, &options).map_err(|e| format!("{e:?}"))?;
                let meshes = ctx.interactive_armor_meshes().map_err(|e| format!("{e:?}"))?;

                let mut min = [f32::MAX; 3];
                let mut max = [f32::MIN; 3];
                for mesh in &meshes {
                    for pos in &mesh.positions {
                        let p = if let Some(t) = &mesh.transform {
                            crate::ui::armor_viewer::transform_point(t, *pos)
                        } else {
                            *pos
                        };
                        for i in 0..3 {
                            min[i] = min[i].min(p[i]);
                            max[i] = max[i].max(p[i]);
                        }
                    }
                }

                // Build zone info
                let mut zone_parts_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
                    std::collections::HashMap::new();
                let mut zone_part_plates_map: std::collections::HashMap<
                    String,
                    std::collections::HashMap<String, std::collections::BTreeSet<i32>>,
                > = std::collections::HashMap::new();
                for mesh in &meshes {
                    for info in &mesh.triangle_info {
                        zone_parts_map.entry(info.zone.clone()).or_default().insert(info.material_name.clone());
                        let thickness_key = (info.thickness_mm * 10.0).round() as i32;
                        zone_part_plates_map
                            .entry(info.zone.clone())
                            .or_default()
                            .entry(info.material_name.clone())
                            .or_default()
                            .insert(thickness_key);
                    }
                }
                let mut zone_parts: Vec<(String, Vec<String>)> = zone_parts_map
                    .into_iter()
                    .map(|(zone, parts)| {
                        let mut parts: Vec<String> = parts.into_iter().collect();
                        parts.sort();
                        (zone, parts)
                    })
                    .collect();
                zone_parts.sort_by(|a, b| a.0.cmp(&b.0));
                let zone_part_plates: Vec<(String, Vec<(String, Vec<i32>)>)> = zone_parts
                    .iter()
                    .map(|(zone, parts)| {
                        let parts_with_plates: Vec<(String, Vec<i32>)> = parts
                            .iter()
                            .map(|part| {
                                let plates = zone_part_plates_map
                                    .get(zone)
                                    .and_then(|m| m.get(part))
                                    .map(|s| s.iter().copied().collect())
                                    .unwrap_or_default();
                                (part.clone(), plates)
                            })
                            .collect();
                        (zone.clone(), parts_with_plates)
                    })
                    .collect();
                let zones: Vec<String> = zone_parts.iter().map(|(z, _)| z.clone()).collect();

                let hull_meshes = ctx.interactive_hull_meshes().map_err(|e| format!("{e:?}"))?;
                for mesh in &hull_meshes {
                    for pos in &mesh.positions {
                        let p = if let Some(t) = &mesh.transform {
                            crate::ui::armor_viewer::transform_point(t, *pos)
                        } else {
                            *pos
                        };
                        for i in 0..3 {
                            min[i] = min[i].min(p[i]);
                            max[i] = max[i].max(p[i]);
                        }
                    }
                }
                let hull_part_groups = crate::ui::armor_viewer::build_hull_part_groups(&hull_meshes);

                Ok(LoadedShipArmor {
                    display_name,
                    meshes,
                    bounds: (min, max),
                    zones,
                    zone_parts,
                    zone_part_plates,
                    hull_meshes,
                    hull_part_groups,
                    draft_meters,
                    splash_data: None,
                    hit_locations: None,
                })
            })();
            let _ = tx.send(result);
        });

        self.pane.load_receiver = Some(rx);
    }

    /// Detect bridge generation changes (seek/rebuild) and clear+reprocess if needed.
    fn check_bridge_generation(&mut self) {
        let bridge = self.bridge.lock();

        if bridge.generation != self.bridge_generation {
            let old_gen = self.bridge_generation;
            self.bridge_generation = bridge.generation;
            tracing::debug!(
                "RealtimeArmorViewer: generation changed {old_gen} -> {} | bridge hits={} cursor={} clock={:.1}",
                bridge.generation,
                bridge.shot_hits.len(),
                self.processed_hit_count,
                bridge.last_clock,
            );
            if bridge.shot_hits.len() < self.processed_hit_count {
                tracing::debug!("RealtimeArmorViewer: seek backward detected, clearing and reprocessing");
                self.processed_hit_count = 0;
                drop(bridge);
                self.clear_and_reprocess();
                self.needs_repaint = true;
            }
        }
    }

    /// Process new shot hits from the bridge (server-authoritative impact data).
    ///
    /// Each `ResolvedShotHit` contains the actual world-space impact position and
    /// (optionally) terminal ballistics info (velocity, impact angle, detonator state).
    /// We use this to create trajectories with accurate impact positioning instead of
    /// relying solely on ballistic simulation.
    fn process_new_shot_hits(&mut self) {
        use crate::viewport_3d::camera::{normalize, scale, sub};

        let bridge = self.bridge.lock();

        let new_count = bridge.shot_hits.len();
        if new_count <= self.processed_hit_count {
            return;
        }

        let new_hits = bridge.shot_hits[self.processed_hit_count..].to_vec();
        let players = bridge.players.clone();
        drop(bridge);

        for hit in &new_hits {
            self.processed_hit_count += 1;

            // Filter by selected attacker
            if let Some(ref sel) = self.selected_attacker {
                if hit.hit.owner_id != *sel {
                    continue;
                }
            } else {
                // "All Enemies" mode: skip friendly fire
                let attacker_friendly =
                    players.iter().find(|p| p.entity_id == hit.hit.owner_id).map(|p| p.is_friendly).unwrap_or(false);
                if attacker_friendly {
                    continue;
                }
            }

            // Resolve shell info from the matched salvo's params_id
            let shell_info = hit.salvo.as_ref().map(|s| s.params_id).and_then(|pid| {
                self.shell_cache
                    .entry(pid)
                    .or_insert_with(|| self.ship_assets.metadata().resolve_shell_from_param_id(pid))
                    .clone()
            });

            let Some(shell) = shell_info else {
                continue;
            };

            // Victim ship position and yaw come directly from the ResolvedShotHit
            // (captured at impact time by the controller).
            let ship_yaw = hit.victim_yaw;
            let ship_world_pos = hit.victim_position;
            // Shot origins come directly from the married salvo
            let salvo_shots: Vec<_> = hit.salvo.as_ref().map(|s| s.shots.clone()).unwrap_or_default();

            // Use the actual impact position from the server
            let impact_pos = hit.hit.position;
            let neg_yaw_cos = (-ship_yaw).cos();
            let neg_yaw_sin = (-ship_yaw).sin();

            // Determine shell direction: prefer terminal ballistics if available,
            // otherwise compute from salvo origin → actual impact position.
            let shell_dir = if let Some(ref tb) = hit.hit.terminal_ballistics {
                let vel = tb.velocity;
                let speed = (vel.x * vel.x + vel.y * vel.y + vel.z * vel.z).sqrt();
                if speed < 1.0 {
                    continue;
                }
                let world_dir = [vel.x / speed, vel.y / speed, vel.z / speed];
                let local_dir_x = world_dir[0] * neg_yaw_cos - world_dir[2] * neg_yaw_sin;
                let local_dir_z = world_dir[0] * neg_yaw_sin + world_dir[2] * neg_yaw_cos;
                normalize([local_dir_z, world_dir[1], local_dir_x])
            } else if !salvo_shots.is_empty() {
                // Compute direction from average shot origin to actual impact position
                let n = salvo_shots.len() as f32;
                let avg_origin: WorldPos = salvo_shots.iter().map(|s| s.origin).sum::<WorldPos>() / n;
                let dx = impact_pos.x - avg_origin.x;
                let dz = impact_pos.z - avg_origin.z;
                let azimuth = dz.atan2(dx);
                let relative_angle = azimuth - ship_yaw;
                let approach_xz = [relative_angle.sin(), 0.0_f32, relative_angle.cos()];

                // Solve ballistics for vertical angle
                let range: Meters = avg_origin.distance_xz(&impact_pos);
                let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(&shell);
                let impact_result = crate::armor_viewer::ballistics::solve_for_range(&params, range);
                if let Some(ref imp) = impact_result {
                    let horiz_angle = imp.impact_angle_horizontal as f32;
                    let cos_h = horiz_angle.cos();
                    let sin_h = horiz_angle.sin();
                    normalize([approach_xz[0] * cos_h, -sin_h, approach_xz[2] * cos_h])
                } else {
                    normalize([approach_xz[0], 0.0, approach_xz[2]])
                }
            } else {
                continue; // No way to determine shell direction
            };

            let model_center = self
                .pane
                .loaded_armor
                .as_ref()
                .map(|a| {
                    [
                        (a.bounds.0[0] + a.bounds.1[0]) * 0.5,
                        (a.bounds.0[1] + a.bounds.1[1]) * 0.5,
                        (a.bounds.0[2] + a.bounds.1[2]) * 0.5,
                    ]
                })
                .unwrap_or([0.0; 3]);

            // Transform impact position from world space to model space
            let world_offset_x = impact_pos.x - ship_world_pos.x;
            let world_offset_z = impact_pos.z - ship_world_pos.z;
            let unrotated_x = world_offset_x * neg_yaw_cos - world_offset_z * neg_yaw_sin;
            let unrotated_z = world_offset_x * neg_yaw_sin + world_offset_z * neg_yaw_cos;
            let rotated_x = unrotated_z; // model_x = world_z
            let rotated_z = unrotated_x; // model_z = world_x

            let (clamped_x, clamped_z) = if let Some(ref armor) = self.pane.loaded_armor {
                let cx = (model_center[0] + rotated_x).clamp(armor.bounds.0[0], armor.bounds.1[0]);
                let cz = (model_center[2] + rotated_z).clamp(armor.bounds.0[2], armor.bounds.1[2]);
                (cx, cz)
            } else {
                (model_center[0] + rotated_x, model_center[2] + rotated_z)
            };
            let ray_through = [clamped_x, 0.0, clamped_z];

            // Cast ray from far behind the through-point along shell_dir
            let ray_origin = sub(ray_through, scale(shell_dir, 100.0));
            let all_hits = self.pane.viewport.pick_all_ray(ray_origin, shell_dir);

            tracing::info!(
                "RealtimeArmorViewer: shot_hit ray — shell={} dir=[{:.3},{:.3},{:.3}] impact=({:.1},{:.1},{:.1}) hits={} has_tb={}",
                shell.name,
                shell_dir[0],
                shell_dir[1],
                shell_dir[2],
                impact_pos.x,
                impact_pos.y,
                impact_pos.z,
                all_hits.len(),
                hit.hit.terminal_ballistics.is_some(),
            );

            if all_hits.is_empty() {
                // Log but skip — no armor hit by this ray
                let attacker_name = players
                    .iter()
                    .find(|p| p.entity_id == hit.hit.owner_id)
                    .map(|p| p.username.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                self.salvo_log.push(SalvoLogEntry {
                    clock: hit.clock.seconds(),
                    estimated_impact_clock: hit.clock.seconds(),
                    attacker_name,
                    shell_name: shell.name.clone(),
                    ammo_type: shell.ammo_type.clone(),
                    shell_count: 1,
                    range: Meters::new(0.0),
                    trajectory_id: None,
                    shell_info: Some(shell.clone()),
                });
                continue;
            }

            // Build trajectory hits
            let mut traj_hits = Vec::new();
            let first_dist = all_hits.first().map(|h| h.0.distance).unwrap_or(0.0);
            for (armor_hit, normal) in &all_hits {
                let tooltip = self
                    .pane
                    .mesh_triangle_info
                    .iter()
                    .find(|(id, _)| *id == armor_hit.mesh_id)
                    .and_then(|(_, infos)| infos.get(armor_hit.triangle_index));

                if let Some(info) = tooltip {
                    let angle = crate::armor_viewer::penetration::impact_angle_deg(&shell_dir, normal);
                    traj_hits.push(crate::armor_viewer::penetration::TrajectoryHit {
                        position: armor_hit.world_position,
                        thickness_mm: info.thickness_mm,
                        zone: info.zone.clone(),
                        material: info.material_name.clone(),
                        angle_deg: angle,
                        distance_from_start: armor_hit.distance - first_dist,
                    });
                }
            }

            // Build ImpactResult: use terminal ballistics if available, otherwise simulate.
            let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(&shell);
            let impact = if let Some(ref tb) = hit.hit.terminal_ballistics {
                Some(crate::armor_viewer::ballistics::ImpactResult::from_terminal_velocity(
                    &params,
                    tb.velocity.x as f64,
                    tb.velocity.y as f64,
                    tb.velocity.z as f64,
                ))
            } else if !salvo_shots.is_empty() {
                let n = salvo_shots.len() as f32;
                let avg_origin: WorldPos = salvo_shots.iter().map(|s| s.origin).sum::<WorldPos>() / n;
                let range: Meters = avg_origin.distance_xz(&impact_pos);
                crate::armor_viewer::ballistics::solve_for_range(&params, range)
            } else {
                None
            };

            let mut detonation_points = Vec::new();
            let mut last_visible_hit: Option<usize> = None;
            if shell.ammo_type == AmmoType::AP {
                if let Some(ref imp) = impact {
                    let sim = crate::armor_viewer::penetration::simulate_shell_through_hits(
                        &params, imp, &traj_hits, &shell_dir,
                    );
                    if let Some(det) = sim.detonation {
                        detonation_points.push(crate::armor_viewer::penetration::DetonationMarker {
                            position: det.position,
                            ship_index: 0,
                        });
                    }
                    let shell_stop = match (sim.detonated_at, sim.stopped_at) {
                        (Some(d), Some(s)) => Some(d.min(s)),
                        (Some(d), None) => Some(d),
                        (None, Some(s)) => Some(s),
                        (None, None) => None,
                    };
                    if let Some(idx) = shell_stop {
                        last_visible_hit = Some(last_visible_hit.map_or(idx, |prev: usize| prev.min(idx)));
                    }
                }
            }

            // Build arc from approach direction
            let approach_xz = [shell_dir[0], 0.0_f32, shell_dir[2]];
            let approach_len = (approach_xz[0] * approach_xz[0] + approach_xz[2] * approach_xz[2]).sqrt();
            let approach_xz = if approach_len > 0.001 {
                [approach_xz[0] / approach_len, 0.0, approach_xz[2] / approach_len]
            } else {
                [1.0, 0.0, 0.0]
            };
            let model_extent = self
                .pane
                .loaded_armor
                .as_ref()
                .map(|a| {
                    let dx = a.bounds.1[0] - a.bounds.0[0];
                    let dz = a.bounds.1[2] - a.bounds.0[2];
                    dx.max(dz)
                })
                .unwrap_or(10.0);
            let arc_horiz_extent = model_extent * 2.0;
            let first_hit_pos = traj_hits.first().map(|h| h.position).unwrap_or(model_center);

            let mut ship_arcs = Vec::new();
            if let Some(ref imp) = impact {
                let (arc_2d, height_ratio) =
                    crate::armor_viewer::ballistics::simulate_arc_points(&params, imp.launch_angle, 60);
                let arc_height_extent = arc_horiz_extent * (height_ratio as f32).max(0.02);
                let arc_points_3d: Vec<[f32; 3]> = arc_2d
                    .iter()
                    .map(|(xf, yf)| {
                        let xf = *xf as f32;
                        let yf = *yf as f32;
                        let along = (1.0 - xf) * arc_horiz_extent;
                        [
                            first_hit_pos[0] - approach_xz[0] * along,
                            first_hit_pos[1] + yf * arc_height_extent,
                            first_hit_pos[2] - approach_xz[2] * along,
                        ]
                    })
                    .collect();
                ship_arcs.push(crate::armor_viewer::penetration::ShipArc {
                    ship_index: 0,
                    arc_points_3d,
                    ballistic_impact: Some(imp.clone()),
                });
            }

            let total_armor: f32 = traj_hits.iter().map(|h| h.thickness_mm).sum();
            let traj_id = self.pane.next_trajectory_id;
            self.pane.next_trajectory_id += 1;

            let result = crate::armor_viewer::penetration::TrajectoryResult {
                origin: ray_origin,
                direction: shell_dir,
                hits: traj_hits,
                total_armor_mm: total_armor,
                ship_arcs,
                detonation_points,
            };

            // Pick trajectory color based on attacker
            let attacker_color_idx =
                self.enemy_players.iter().position(|p| p.entity_id == hit.hit.owner_id).unwrap_or(0);
            let traj_color = TRAJECTORY_PALETTE[attacker_color_idx % TRAJECTORY_PALETTE.len()];

            let cam_dist = self.pane.viewport.camera.distance;
            let mesh_id = crate::ui::armor_viewer::upload_trajectory_visualization(
                &mut self.pane.viewport,
                &result,
                &self.render_state.device,
                traj_color,
                last_visible_hit,
                cam_dist,
                self.pane.marker_opacity,
            );

            self.pane.trajectories.push(StoredTrajectory {
                meta: crate::armor_viewer::penetration::TrajectoryMeta {
                    id: traj_id,
                    color_index: attacker_color_idx % TRAJECTORY_PALETTE.len(),
                    range: Meters::new(0.0).to_km(),
                },
                result,
                mesh_id: Some(mesh_id),
                last_visible_hit,
                marker_cam_dist: cam_dist,
                show_plates_active: false,
                show_zones_active: false,
            });

            // Log entry
            let attacker_name = players
                .iter()
                .find(|p| p.entity_id == hit.hit.owner_id)
                .map(|p| p.username.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            self.salvo_log.push(SalvoLogEntry {
                clock: hit.clock.seconds(),
                estimated_impact_clock: hit.clock.seconds(),
                attacker_name,
                shell_name: shell.name.clone(),
                ammo_type: shell.ammo_type.clone(),
                shell_count: 1,
                range: Meters::new(0.0),
                trajectory_id: Some(traj_id),
                shell_info: Some(shell.clone()),
            });

            self.needs_repaint = true;
        }
    }

    /// Tick state: load ship, process salvos. Called before rendering.
    fn tick(&mut self) {
        // Populate enemy players from bridge on first availability
        if !self.players_populated {
            let bridge = self.bridge.lock();
            if !bridge.players.is_empty() {
                self.enemy_players = bridge.players.iter().filter(|p| !p.is_friendly).cloned().collect();
                self.players_populated = true;
                self.needs_repaint = true;
            }
        }

        // Start ship load on first frame
        if !self.ship_loaded && self.pane.load_receiver.is_none() && !self.pane.loading {
            self.start_ship_load();
            self.needs_repaint = true;
        }

        // Check if ship load completed
        if let Some(ref rx) = self.pane.load_receiver {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(armor) => {
                        tracing::debug!(
                            "RealtimeArmorViewer: ship loaded — bounds min=[{:.1},{:.1},{:.1}] max=[{:.1},{:.1},{:.1}]",
                            armor.bounds.0[0],
                            armor.bounds.0[1],
                            armor.bounds.0[2],
                            armor.bounds.1[0],
                            armor.bounds.1[1],
                            armor.bounds.1[2],
                        );
                        crate::ui::armor_viewer::init_armor_viewport(&mut self.pane, &armor, &self.render_state.device);
                        self.pane.loaded_armor = Some(armor);
                        self.pane.loading = false;
                        self.ship_loaded = true;
                    }
                    Err(e) => {
                        tracing::error!("Failed to load ship armor: {e}");
                        self.pane.loading = false;
                    }
                }
                self.pane.load_receiver = None;
                warn!("Ship loaded! we need repaint");
                self.needs_repaint = true;
            }
        }

        // Process shot hits if ship is loaded
        if self.ship_loaded {
            self.check_bridge_generation();
            self.process_new_shot_hits();
            self.expire_old_salvos();
        } else {
            // Log occasionally that we're waiting for ship load
            static TICK_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
            if n % 100 == 0 {
                tracing::debug!("RealtimeArmorViewer: tick #{n} — ship not loaded yet (loading={})", self.pane.loading);
            }
        }

        if self.needs_repaint {
            warn!("needs repaint");
        }
    }

    /// Sliding window duration in seconds. Trajectories older than this
    /// (relative to the current replay clock) are removed.
    const SLIDING_WINDOW_SECS: f32 = 5.0;

    /// Remove trajectories and log entries older than the sliding window.
    fn expire_old_salvos(&mut self) {
        let current_clock = self.bridge.lock().last_clock;
        let cutoff = current_clock - Self::SLIDING_WINDOW_SECS;

        // Find trajectory IDs to remove
        let expired_ids: Vec<Option<u64>> =
            self.salvo_log.iter().filter(|e| e.estimated_impact_clock < cutoff).map(|e| e.trajectory_id).collect();

        if expired_ids.is_empty() {
            return;
        }

        // Remove expired log entries
        self.salvo_log.retain(|e| e.estimated_impact_clock >= cutoff);

        // Remove expired trajectories and their GPU meshes
        let mut removed_any = false;
        for expired_traj_id in expired_ids.into_iter().flatten() {
            if let Some(pos) = self.pane.trajectories.iter().position(|t| t.meta.id == expired_traj_id) {
                let traj = self.pane.trajectories.remove(pos);
                if let Some(mesh_id) = traj.mesh_id {
                    self.pane.viewport.remove_mesh(mesh_id);
                }
                removed_any = true;
            }
        }

        if removed_any {
            self.pane.viewport.mark_dirty();
            self.needs_repaint = true;
        }
    }
}

/// Draw a realtime armor viewer as a deferred secondary viewport.
/// Takes `Arc<Mutex<RealtimeArmorViewer>>` so the closure can be `'static`.
pub fn draw_realtime_armor_viewer(viewer: &Arc<Mutex<RealtimeArmorViewer>>, ctx: &egui::Context) {
    let (title, open) = {
        let v = viewer.lock();
        if !v.open.load(Ordering::Relaxed) {
            return;
        }
        (v.title.clone(), v.open.clone())
    };

    let viewport_id = egui::ViewportId::from_hash_of(&*title);
    let viewer_clone = viewer.clone();
    let window_open = open.clone();
    let parent_ctx = ctx.clone();

    // Tick state (process new salvos, load ship, etc.)
    // Must happen on the main context so it runs even when the viewport isn't focused.
    {
        let mut v = viewer.lock();
        v.tick();

        if v.needs_repaint {
            ctx.request_repaint_of(viewport_id);
        }
    }

    ctx.show_viewport_deferred(
        viewport_id,
        egui::ViewportBuilder::default()
            .with_title(&*title)
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([600.0, 400.0]),
        move |ctx, _class| {
            if !window_open.load(Ordering::Relaxed) || crate::app::mitigate_wgpu_mem_leak(ctx) {
                return;
            }

            // Handle window close
            if ctx.input(|i| i.viewport().close_requested()) {
                window_open.store(false, Ordering::Relaxed);
                return;
            }

            {
                let mut viewer = viewer_clone.lock();
                egui::CentralPanel::default().show(ctx, |ui| {
                    viewer.draw_content(ui);
                });

                // Repaint both this viewport AND the parent so sibling viewports
                // (e.g. replay renderer) also update while this window has focus.
                if std::mem::take(&mut viewer.needs_repaint) {
                    ctx.request_repaint();
                    parent_ctx.request_repaint();
                }
            }
        },
    );
}

impl RealtimeArmorViewer {
    /// Draw the main content: 3D viewport + side panel.
    fn draw_content(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::right("rtav_side_panel").default_width(250.0).min_width(200.0).show_inside(ui, |ui| {
            self.draw_side_panel(ui);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.draw_viewport(ui);
        });
    }

    /// Draw the 3D armor viewport with toolbar and plate interaction.
    fn draw_viewport(&mut self, ui: &mut egui::Ui) {
        if self.pane.loading {
            ui.centered_and_justified(|ui| {
                ui.spinner();
                ui.label("Loading ship armor...");
            });
            return;
        }

        if !self.ship_loaded {
            ui.centered_and_justified(|ui| {
                ui.label("Waiting for ship data...");
            });
            return;
        }

        let mut zone_changed = false;
        let translate_part = |name: &str| -> String { name.to_string() };

        // Undo/redo keyboard shortcuts
        {
            let wants_undo = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift);
            let wants_redo = ui.input(|i| {
                i.modifiers.command
                    && (i.key_pressed(egui::Key::R) || (i.key_pressed(egui::Key::Z) && i.modifiers.shift))
            });
            if wants_undo {
                let current = crate::armor_viewer::state::VisibilitySnapshot {
                    part_visibility: self.pane.part_visibility.clone(),
                    plate_visibility: self.pane.plate_visibility.clone(),
                };
                if let Some(prev) = self.pane.undo_stack.undo(current) {
                    self.pane.part_visibility = prev.part_visibility;
                    self.pane.plate_visibility = prev.plate_visibility;
                    zone_changed = true;
                }
            } else if wants_redo {
                let current = crate::armor_viewer::state::VisibilitySnapshot {
                    part_visibility: self.pane.part_visibility.clone(),
                    plate_visibility: self.pane.plate_visibility.clone(),
                };
                if let Some(next) = self.pane.undo_stack.redo(current) {
                    self.pane.part_visibility = next.part_visibility;
                    self.pane.plate_visibility = next.plate_visibility;
                    zone_changed = true;
                }
            }
        }

        // Toolbar
        let prev_marker_opacity = self.pane.marker_opacity;
        if let Some(armor) = self.pane.loaded_armor.take() {
            if !armor.zone_parts.is_empty() {
                ui.horizontal(|ui| {
                    // Armor Zones button
                    let armor_btn =
                        ui.button(icon_str!(icons::SHIELD, "Armor")).on_hover_text("Toggle armor zone visibility");
                    egui::Popup::from_toggle_button_response(&armor_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            if crate::ui::armor_viewer::draw_armor_visibility_popover(
                                ui,
                                &mut self.pane,
                                &armor,
                                &translate_part,
                            ) {
                                zone_changed = true;
                            }
                        });

                    // Display settings button
                    let display_btn =
                        ui.button(icon_str!(icons::GEAR_FINE, "Display")).on_hover_text("Display settings");
                    egui::Popup::from_toggle_button_response(&display_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            if crate::ui::armor_viewer::draw_display_settings_popover(ui, &mut self.pane, &armor) {
                                zone_changed = true;
                            }
                            if !self.pane.trajectories.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label("Marker Opacity");
                                    ui.add(
                                        egui::Slider::new(&mut self.pane.marker_opacity, 0.0..=1.0).fixed_decimals(2),
                                    );
                                });
                            }
                        });
                });
                ui.separator();
            }
            self.pane.loaded_armor = Some(armor);
        }

        // Viewport rendering
        let available_size = ui.available_size();
        let pixel_size = (
            (available_size.x * ui.ctx().pixels_per_point()) as u32,
            (available_size.y * ui.ctx().pixels_per_point()) as u32,
        );

        if let Some(tex_id) = self.pane.viewport.render(&self.render_state, &self.gpu_pipeline, pixel_size) {
            let response = ui.add(
                egui::Image::new(egui::load::SizedTexture::new(tex_id, available_size))
                    .sense(egui::Sense::click_and_drag()),
            );

            let bounds = self.pane.loaded_armor.as_ref().map(|a| a.bounds);
            if self.pane.viewport.handle_input(&response, bounds) {
                self.needs_repaint = true;
            }

            // Plate interaction: hover tooltip, click-to-hide, right-click context menu, highlight
            if crate::ui::armor_viewer::handle_plate_interaction(
                ui,
                &response,
                &mut self.pane,
                &self.render_state.device,
                &translate_part,
                true,  // allow click-to-toggle (no trajectory mode conflict in realtime viewer)
                &[],   // no comparison ships
                false, // no IFHE
            ) {
                zone_changed = true;
            }
        }

        // Re-upload armor and trajectories if visibility changed
        let marker_opacity_changed = (self.pane.marker_opacity - prev_marker_opacity).abs() > 0.001;
        if zone_changed {
            if let Some(armor) = self.pane.loaded_armor.take() {
                crate::ui::armor_viewer::upload_armor_to_viewport(&mut self.pane, &armor, &self.render_state.device);
                self.pane.loaded_armor = Some(armor);
            }
            // Re-upload trajectory meshes (viewport.clear() destroyed them)
            let cam_dist = self.pane.viewport.camera.distance;
            for traj in &mut self.pane.trajectories {
                let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                traj.mesh_id = Some(crate::ui::armor_viewer::upload_trajectory_visualization(
                    &mut self.pane.viewport,
                    &traj.result,
                    &self.render_state.device,
                    color,
                    traj.last_visible_hit,
                    cam_dist,
                    self.pane.marker_opacity,
                ));
                traj.marker_cam_dist = cam_dist;
            }
            self.needs_repaint = true;
        } else if marker_opacity_changed && !self.pane.trajectories.is_empty() {
            let cam_dist = self.pane.viewport.camera.distance;
            for traj in &mut self.pane.trajectories {
                if let Some(old_mid) = traj.mesh_id.take() {
                    self.pane.viewport.remove_mesh(old_mid);
                }
                let color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                traj.mesh_id = Some(crate::ui::armor_viewer::upload_trajectory_visualization(
                    &mut self.pane.viewport,
                    &traj.result,
                    &self.render_state.device,
                    color,
                    traj.last_visible_hit,
                    cam_dist,
                    self.pane.marker_opacity,
                ));
                traj.marker_cam_dist = cam_dist;
            }
            self.needs_repaint = true;
        }
    }

    /// Draw the side panel with attacker selector and salvo log.
    fn draw_side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Incoming Fire");
        ui.separator();

        // Attacker selector
        ui.label(egui::RichText::new("Attacker Filter").strong());
        let current_label = if let Some(ref sel) = self.selected_attacker {
            self.enemy_players
                .iter()
                .find(|p| p.entity_id == *sel)
                .map(|p| format!("{} ({})", p.username, p.ship_display_name))
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            "All Enemies".to_string()
        };

        let mut attacker_changed = false;
        egui::ComboBox::from_id_salt("attacker_selector").selected_text(&current_label).show_ui(ui, |ui| {
            if ui.selectable_value(&mut self.selected_attacker, None, "All Enemies").changed() {
                attacker_changed = true;
            }
            for player in &self.enemy_players {
                let label = format!("{} ({})", player.username, player.ship_display_name);
                if ui.selectable_value(&mut self.selected_attacker, Some(player.entity_id), &label).changed() {
                    attacker_changed = true;
                }
            }
        });
        if attacker_changed {
            self.clear_and_reprocess();
            self.needs_repaint = true;
        }

        ui.separator();

        // Stats
        ui.label(egui::RichText::new(format!("{} salvos tracked", self.salvo_log.len())).small());

        ui.separator();

        // Salvo log — clickable entries
        let mut clicked_id: Option<Option<u64>> = None;
        egui::ScrollArea::vertical()
            .id_salt("salvo_log_scroll")
            .auto_shrink([false; 2])
            .max_height(ui.available_height() * 0.4)
            .show(ui, |ui| {
                for entry in &self.salvo_log {
                    let is_selected = entry.trajectory_id.is_some() && self.selected_salvo_id == entry.trajectory_id;
                    let frame = egui::Frame::group(ui.style()).fill(if is_selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        ui.visuals().widgets.noninteractive.bg_fill
                    });
                    let resp = frame.show(ui, |ui| {
                        let time_min = (entry.clock / 60.0).floor() as i32;
                        let time_sec = (entry.clock % 60.0) as i32;
                        ui.label(
                            egui::RichText::new(format!(
                                "{}:{:02} — {} {}",
                                time_min, time_sec, entry.shell_count, entry.shell_name
                            ))
                            .strong()
                            .small(),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "  {} • {} • {:.1}km",
                                entry.attacker_name,
                                entry.ammo_type.display_name(),
                                entry.range.to_km().value(),
                            ))
                            .small(),
                        );
                        if entry.trajectory_id.is_none() {
                            ui.label(egui::RichText::new("  (no armor hit)").small().weak());
                        }
                    });
                    if entry.trajectory_id.is_some() && resp.response.interact(egui::Sense::click()).clicked() {
                        if is_selected {
                            clicked_id = Some(None); // deselect
                        } else {
                            clicked_id = Some(entry.trajectory_id);
                        }
                    }
                }
            });

        if let Some(new_sel) = clicked_id {
            self.selected_salvo_id = new_sel;
            self.needs_repaint = true;
        }

        // Detail panel for selected salvo
        if let Some(sel_id) = self.selected_salvo_id {
            ui.separator();
            self.draw_salvo_detail(ui, sel_id);
        }
    }

    /// Draw the plate-by-plate detail panel for a selected salvo.
    fn draw_salvo_detail(&self, ui: &mut egui::Ui, trajectory_id: u64) {
        use crate::armor_viewer::penetration::{PlateOutcome, enclosing_zone};

        // Find the matching log entry and trajectory
        let entry = self.salvo_log.iter().find(|e| e.trajectory_id == Some(trajectory_id));
        let traj = self.pane.trajectories.iter().find(|t| t.meta.id == trajectory_id);

        let (Some(entry), Some(traj)) = (entry, traj) else {
            ui.label(egui::RichText::new("Salvo data no longer available").small().weak());
            return;
        };

        let result = &traj.result;

        // Header
        ui.label(egui::RichText::new(&entry.shell_name).strong());

        // Impact stats from the ballistic arc
        if let Some(arc) = result.ship_arcs.first() {
            if let Some(ref impact) = arc.ballistic_impact {
                ui.label(
                    egui::RichText::new(format!(
                        "v={:.0} m/s  t={:.1}s  fall={:.1}°",
                        impact.impact_velocity,
                        impact.time_to_target,
                        impact.impact_angle_horizontal.to_degrees(),
                    ))
                    .small(),
                );
            }
        }

        // Re-run shell simulation for outcome and per-plate results
        let sim = entry.shell_info.as_ref().and_then(|shell| {
            if shell.ammo_type != AmmoType::AP {
                return None;
            }
            let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
            let impact = crate::armor_viewer::ballistics::solve_for_range(&params, entry.range);
            impact.map(|imp| {
                crate::armor_viewer::penetration::simulate_shell_through_hits(
                    &params,
                    &imp,
                    &result.hits,
                    &result.direction,
                )
            })
        });

        // HE/SAP outcome for non-AP shells
        let he_sap_outcome = entry.shell_info.as_ref().and_then(|shell| match shell.ammo_type {
            AmmoType::HE => {
                let pen = shell.he_pen_mm.unwrap_or(0.0);
                result.hits.first().map(|hit| {
                    if pen >= hit.thickness_mm {
                        (
                            egui::Color32::from_rgb(255, 140, 40),
                            format!("HE detonates — {:.0}mm pen vs {:.0}mm", pen, hit.thickness_mm),
                        )
                    } else {
                        (egui::Color32::RED, format!("HE shatter — {:.0}mm pen < {:.0}mm", pen, hit.thickness_mm))
                    }
                })
            }
            AmmoType::SAP => {
                let pen = shell.sap_pen_mm.unwrap_or(0.0);
                result.hits.first().map(|hit| {
                    if pen >= hit.thickness_mm {
                        (
                            egui::Color32::from_rgb(255, 140, 40),
                            format!("SAP pen — {:.0}mm vs {:.0}mm", pen, hit.thickness_mm),
                        )
                    } else {
                        (egui::Color32::RED, format!("SAP shatter — {:.0}mm pen < {:.0}mm", pen, hit.thickness_mm))
                    }
                })
            }
            _ => None,
        });

        // Shell outcome badge
        if let Some(ref sim) = sim {
            let (color, text) = if let Some(det_idx) = sim.detonated_at {
                let zone = enclosing_zone(&result.hits, det_idx);
                (egui::Color32::from_rgb(255, 140, 40), format!("Detonation inside {zone}"))
            } else if let Some(stop_idx) = sim.stopped_at {
                let plate_desc = result
                    .hits
                    .get(stop_idx)
                    .map(|h| format!("{:.0}mm {}", h.thickness_mm, h.zone))
                    .unwrap_or_default();
                match sim.plates.get(stop_idx).map(|p| &p.outcome) {
                    Some(PlateOutcome::Ricochet) => (egui::Color32::RED, format!("Ricochet @ {plate_desc}")),
                    Some(PlateOutcome::Shatter) => (egui::Color32::RED, format!("Shatter @ {plate_desc}")),
                    _ => (egui::Color32::RED, format!("Stopped @ {plate_desc}")),
                }
            } else if sim.detonation.is_some() {
                (egui::Color32::YELLOW, "Overpen".to_string())
            } else {
                (egui::Color32::YELLOW, "Overpen (fuse never armed)".to_string())
            };
            ui.label(egui::RichText::new(&text).strong().small().color(color));
        } else if let Some((color, text)) = he_sap_outcome {
            ui.label(egui::RichText::new(&text).strong().small().color(color));
        }

        ui.separator();

        // Plate-by-plate breakdown
        let last_visible = traj.last_visible_hit;

        egui::ScrollArea::vertical().id_salt("plate_detail_scroll").auto_shrink([false; 2]).show(ui, |ui| {
            for (i, hit) in result.hits.iter().enumerate() {
                let is_post_detonation = last_visible.map_or(false, |lv| i > lv);

                // Check if detonation happens at this plate
                let detonation_here =
                    sim.as_ref().and_then(|s| if s.detonated_at == Some(i) { s.detonation.as_ref() } else { None });

                if is_post_detonation && detonation_here.is_none() {
                    continue;
                }

                let plate_color = if is_post_detonation {
                    egui::Color32::GRAY
                } else if hit.angle_deg < 30.0 {
                    egui::Color32::from_rgb(80, 220, 80)
                } else if hit.angle_deg < 45.0 {
                    egui::Color32::from_rgb(220, 180, 50)
                } else {
                    egui::Color32::from_rgb(220, 80, 80)
                };

                // Plate header
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("#{}", i + 1)).small().color(egui::Color32::GRAY));
                    ui.label(
                        egui::RichText::new(format!("{:.0}mm", hit.thickness_mm)).strong().small().color(plate_color),
                    );
                    ui.label(egui::RichText::new(format!("{:.1}°", hit.angle_deg)).small().color(plate_color));
                });
                ui.label(
                    egui::RichText::new(format!("  {} / {}", hit.zone, hit.material))
                        .small()
                        .color(egui::Color32::GRAY),
                );

                // Per-plate penetration outcome (AP)
                if !is_post_detonation {
                    if let Some(ref sim) = sim {
                        if let Some(plate) = sim.plates.get(i) {
                            let (icon, detail_color, detail) = match plate.outcome {
                                PlateOutcome::Overmatch => (
                                    ">>",
                                    egui::Color32::from_rgb(80, 220, 80),
                                    format!(
                                        "overmatch — {:.0}mm pen, v={:.0} m/s",
                                        plate.raw_pen_before_mm, plate.velocity_before
                                    ),
                                ),
                                PlateOutcome::Penetrate => (
                                    ">>",
                                    egui::Color32::from_rgb(80, 220, 80),
                                    format!(
                                        "{:.0}/{:.0}mm eff — v={:.0} m/s",
                                        plate.raw_pen_before_mm, plate.effective_thickness_mm, plate.velocity_before
                                    ),
                                ),
                                PlateOutcome::Ricochet => {
                                    ("X", egui::Color32::RED, format!("ricochet @ {:.1}°", hit.angle_deg))
                                }
                                PlateOutcome::Shatter => (
                                    "X",
                                    egui::Color32::RED,
                                    format!(
                                        "shatter — {:.0} < {:.0}mm eff",
                                        plate.raw_pen_before_mm, plate.effective_thickness_mm
                                    ),
                                ),
                            };
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(icon).small().color(detail_color));
                                let mut label = detail;
                                if plate.fuse_armed_here {
                                    label.push_str(" [fuse armed]");
                                }
                                ui.label(egui::RichText::new(label).small().color(detail_color));
                            });
                        }
                    }
                }

                // Detonation marker
                if let Some(det) = detonation_here {
                    let zone = enclosing_zone(&result.hits, i);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "** detonates inside {} — {:.1}m after plate #{}",
                                zone,
                                det.travel_distance,
                                i + 1,
                            ))
                            .small()
                            .color(egui::Color32::from_rgb(255, 140, 40)),
                        );
                    });
                }

                ui.add_space(2.0);
            }

            if result.hits.is_empty() {
                ui.label(egui::RichText::new("No armor plates hit").small().weak());
            }
        });
    }

    /// Clear trajectories and reprocess all salvos (used when attacker filter changes).
    fn clear_and_reprocess(&mut self) {
        // Remove GPU meshes for all trajectories
        for traj in &self.pane.trajectories {
            if let Some(mesh_id) = traj.mesh_id {
                self.pane.viewport.remove_mesh(mesh_id);
            }
        }
        self.pane.trajectories.clear();
        self.pane.viewport.mark_dirty();
        self.salvo_log.clear();
        self.processed_hit_count = 0;
        // Next tick() will call process_new_shot_hits() to reprocess.
    }
}

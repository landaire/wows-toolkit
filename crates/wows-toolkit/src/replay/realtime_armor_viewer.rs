use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use parking_lot::Mutex;

use tracing::trace;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_types::EntityId;
use wowsunpack::game_types::GameClock;
use wowsunpack::game_types::GameParamId;
use wowsunpack::game_types::ShellHitType;
use wowsunpack::game_types::ShotId;
use wowsunpack::game_types::WorldPos;
use wowsunpack::recognized::Recognized;

extern crate nalgebra as na;
use na::Rotation3;

use crate::armor_viewer::constants::*;
use crate::armor_viewer::penetration::ComparisonVerdict;
use crate::armor_viewer::penetration::ExitDivergence;
use crate::armor_viewer::penetration::ServerOutcome;
use crate::armor_viewer::penetration::ServerVsSimComparison;
use crate::armor_viewer::state::ArmorPane;
use crate::armor_viewer::state::SidebarHighlightKey;
use crate::armor_viewer::state::StoredTrajectory;
use crate::icons;
use crate::replay::renderer::RealtimeArmorBridge;
use crate::replay::renderer::ReplayPlayerInfo;
use crate::viewport_3d::GpuPipeline;
use crate::viewport_3d::Vec3;
use rust_i18n::t;

/// A realtime armor viewer window spawned from the replay renderer.
pub struct RealtimeArmorViewer {
    pub title: Arc<String>,
    pub open: Arc<AtomicBool>,

    /// Bridge shared with the replay background thread.
    bridge: Arc<Mutex<RealtimeArmorBridge>>,

    /// The target ship's entity ID in the replay.
    target_entity_id: wows_replays::types::EntityId,

    /// The target ship's team ID (used to determine which players are attackers).
    target_team_id: i64,

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

    /// Salvo groups for the side panel (shells grouped by salvo firing event).
    salvo_groups: Vec<SalvoGroup>,

    /// Fast lookup: SalvoKey -> index in `salvo_groups`.
    salvo_group_index: HashMap<SalvoKey, usize>,

    /// Counter for `SalvoKey::Unmatched` (monotonically increasing).
    unmatched_counter: u64,

    /// Current selection state (salvo group or individual shell).
    selection: Option<SalvoSelection>,

    /// Set when selection changes; triggers trajectory mesh re-upload in draw_viewport.
    selection_dirty: bool,

    /// Target ship's vehicle param (for loading).
    target_vehicle: Arc<wowsunpack::game_params::types::Param>,

    /// Whether to show secondary armament hits (off by default).
    show_secondaries: bool,

    /// Cached lookup: is a given projectile params_id a main battery shell?
    /// Built at player-populate time by resolving each ship's main_battery_ammo names to param IDs.
    is_main_battery: HashMap<GameParamId, bool>,

    /// How many shot hits we've consumed from the bridge.
    processed_hit_count: usize,

    /// Set by any method that changes visible state. Checked and cleared by
    /// the viewport closure to decide whether to request a repaint.
    needs_repaint: bool,

    /// Sender for playback commands (seek) back to the replay thread.
    command_tx: Option<std::sync::mpsc::Sender<crate::replay::renderer::PlaybackCommand>>,

    /// Pre-computed shot timeline for this target ship (entire replay).
    shot_timeline: Option<Arc<crate::replay::renderer::ShipShotTimeline>>,

    /// Whether the pre-computed timeline has been ingested into salvo_groups.
    timeline_ingested: bool,

    /// Auto-scroll the salvo log to the current clock position.
    auto_scroll: bool,

    /// Last clock we auto-scrolled to (avoids redundant scrolls).
    last_auto_scroll_clock: GameClock,
}

/// Identifies a salvo firing event. Shells with `salvo: None` get unique unmatched keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SalvoKey {
    /// Grouped by who fired and which turret salvo (salvo_id from the game).
    Matched {
        owner_id: EntityId,
        salvo_id: u32,
    },
    Unmatched(u64),
}

/// Per-shell data within a [`SalvoGroup`].
#[allow(dead_code)]
struct ShellEntry {
    shot_id: ShotId,
    clock: GameClock,
    range: Meters,
    /// Links to `StoredTrajectory.meta.id`. `None` if no armor was hit.
    trajectory_id: Option<u64>,
    /// Server vs simulation comparison (AP only).
    comparison: Option<ServerVsSimComparison>,
    server_outcome: ServerOutcome,
    /// Victim ship roll at impact time (radians). Used to set viewport model roll on selection.
    victim_roll: f32,
    /// Victim ship yaw at impact time (radians). Used to set viewport model yaw on selection.
    victim_yaw: f32,
    /// Server-reported impact angle against armor (radians), from terminal ballistics.
    server_material_angle: Option<f32>,
}

/// All shells from one salvo firing event, grouped together.
#[allow(dead_code)]
struct SalvoGroup {
    key: SalvoKey,
    attacker_name: String,
    shell_name: String,
    ammo_type: AmmoType,
    shell_info: Option<ShellInfo>,
    /// Earliest shell clock (display ordering).
    first_clock: GameClock,
    /// Latest shell clock (expiration).
    latest_clock: GameClock,
    shells: Vec<ShellEntry>,
}

/// Two-level selection state for the salvo log.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SalvoSelection {
    /// Salvo header clicked — all shells in this group are highlighted.
    Group(SalvoKey),
    /// Specific shell clicked — that shell is emphasized, siblings moderate, rest dimmed.
    Shell { group_key: SalvoKey, trajectory_id: u64 },
}

/// Grouped hits for a single shell (entry + optional exit).
struct ShellHitGroup {
    /// Primary hit (the non-ExitOverpenetration entry).
    entry: ResolvedShotHit,
    /// Exit point (ExitOverpenetration), if present.
    exit: Option<ResolvedShotHit>,
    /// Server outcome from the entry hit.
    server_outcome: ServerOutcome,
}

/// Output from [`RealtimeArmorViewer::simulate_and_upload_trajectory`].
struct TrajectorySimResult {
    traj_id: u64,
    comparison: Option<ServerVsSimComparison>,
    traj_hits: Vec<crate::armor_viewer::penetration::TrajectoryHit>,
    firing_range: Meters,
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
        command_tx: Option<std::sync::mpsc::Sender<crate::replay::renderer::PlaybackCommand>>,
    ) -> Self {
        let title =
            Arc::new(format!("Armor Viewer — {} ({})", target_player.username, target_player.ship_display_name));

        // Resolve the replay's equipped hull to a hull upgrade key name
        let selected_hull = target_player.hull_param_id.and_then(|hull_id| {
            use wowsunpack::game_params::types::GameParamProvider;
            let hull_param = ship_assets.metadata().game_param_by_id(hull_id)?;
            let hull_index = hull_param.index().to_string();
            let vehicle = target_player.vehicle.vehicle()?;
            vehicle.hull_upgrades()?.keys().find(|k| k.contains(&hull_index)).cloned()
        });

        let mut pane = ArmorPane::empty(0);
        pane.show_plate_edges = true;
        pane.armor_opacity = 1.0;
        pane.trajectory_mode = true;
        pane.selected_hull = selected_hull;
        // Trajectories are in body-frame mesh-space and must rotate with the
        // model matrix (world_space=false).
        pane.trajectory_world_space = false;

        Self {
            title,
            open: Arc::new(AtomicBool::new(true)),
            bridge,
            target_entity_id: target_player.entity_id,
            target_team_id: target_player.team_id,
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
            salvo_groups: Vec::new(),
            salvo_group_index: HashMap::new(),
            unmatched_counter: 0,
            selection: None,
            selection_dirty: false,
            target_vehicle: target_player.vehicle.clone(),
            show_secondaries: false,
            is_main_battery: HashMap::new(),
            processed_hit_count: 0,
            needs_repaint: true,
            command_tx,
            shot_timeline: None,
            timeline_ingested: false,
            auto_scroll: true,
            last_auto_scroll_clock: GameClock(0.0),
        }
    }

    /// Load the target ship's armor model (called once, on first frame).
    fn start_ship_load(&mut self) {
        self.start_ship_load_with_lod(self.pane.hull_lod);
    }

    /// Load the target ship's armor model at the given LOD level.
    fn start_ship_load_with_lod(&mut self, lod: usize) {
        self.pane.hull_lod = lod;
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
        let selected_hull = self.pane.selected_hull.clone();

        let (tx, rx) = mpsc::channel();
        let requested_lod = lod;

        std::thread::spawn(move || {
            let result = (|| {
                use wowsunpack::game_params::types::GameParamProvider;
                let param = ship_assets.metadata().game_param_by_index(vehicle.index());
                let v =
                    param.as_ref().and_then(|p| p.vehicle().cloned()).ok_or_else(|| "No vehicle found".to_string())?;

                let dock_y_offset = crate::armor_viewer::common::resolve_dock_y_offset(&v, &selected_hull);
                let hull_upgrade_names = crate::armor_viewer::common::build_hull_upgrade_names(&v);
                let load_opts = crate::armor_viewer::common::ShipLoadOptions {
                    display_name,
                    lod: requested_lod,
                    selected_hull,
                    hull_upgrade_names,
                    dock_y_offset,
                    ..Default::default()
                };
                crate::armor_viewer::common::load_ship_armor(&v, &ship_assets, load_opts)
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
                tracing::debug!("RealtimeArmorViewer: seek detected, resetting live-edge state");
                drop(bridge);

                if self.timeline_ingested {
                    // Pre-computed timeline covers the full replay — keep salvo groups,
                    // just clear GPU trajectories and reset live-edge cursor.
                    for traj in &self.pane.trajectories {
                        if let Some(mesh_id) = traj.mesh_id {
                            self.pane.viewport.remove_mesh(mesh_id);
                        }
                    }
                    self.pane.trajectories.clear();
                    self.pane.viewport.mark_dirty();

                    // Reset trajectory_id on all shells (GPU meshes were removed)
                    for group in &mut self.salvo_groups {
                        for shell in &mut group.shells {
                            shell.trajectory_id = None;
                            shell.comparison = None;
                        }
                    }

                    self.selection = None;
                    self.selection_dirty = false;
                    self.processed_hit_count = self.bridge.lock().shot_hits.len();
                } else {
                    // No pre-computed timeline yet — full clear and reprocess
                    self.processed_hit_count = 0;
                    self.clear_and_reprocess();
                }
                self.needs_repaint = true;
            }
        }
    }

    /// Build the inverse ship rotation: undoes yaw, pitch, roll to get body-frame.
    ///
    /// In BigWorld, yaw=0 faces East (+X), yaw increases CCW. The nalgebra
    /// `Ry(+θ)` rotates +X toward −Z (right-hand rule), so the BigWorld
    /// forward rotation is `Ry(−yaw)` and its inverse is `Ry(+yaw)`.
    fn inverse_ship_rotation(yaw: f32, pitch: f32, roll: f32) -> Rotation3<f32> {
        let ry = Rotation3::from_axis_angle(&Vec3::y_axis(), yaw);
        let rx = Rotation3::from_axis_angle(&Vec3::x_axis(), pitch);
        let rz = Rotation3::from_axis_angle(&Vec3::z_axis(), roll);
        rz * rx * ry
    }

    /// Remap from BigWorld body-frame to GLTF mesh-space (partial).
    ///
    /// Body-frame (after inverse rotation, yaw=0 faces East):
    ///   bow = +X, up = +Y
    /// GLTF mesh (model-local, bow along +Z):
    ///   bow = +Z, starboard = +X, up = +Y
    ///
    /// This rotation maps body_x -> mesh_z, body_z -> −mesh_x.
    /// For directions, a subsequent Z-negation is needed to account for the
    /// GLTF exporter's left->right-handed conversion (`positions.push([x, y, -z])`).
    fn axis_remap() -> Rotation3<f32> {
        Rotation3::from_axis_angle(&Vec3::y_axis(), -std::f32::consts::FRAC_PI_2)
    }

    /// Transform a world-space position to model-space given ship position and
    /// the inverse rotation matrix (from `inverse_ship_rotation`).
    fn world_to_model(
        world_pos: &WorldPos,
        ship_pos: &WorldPos,
        rot: &Rotation3<f32>,
        model_center: &Vec3,
        bounds: Option<&(Vec3, Vec3)>,
    ) -> Vec3 {
        let offset = Vec3::new(world_pos.x - ship_pos.x, world_pos.y - ship_pos.y, world_pos.z - ship_pos.z);
        let body = rot * offset;
        let remapped = Self::axis_remap() * body;
        let model = model_center + remapped;
        if let Some((min, max)) = bounds {
            Vec3::new(model.x.clamp(min.x, max.x), model.y, model.z.clamp(min.z, max.z))
        } else {
            model
        }
    }

    /// Group a flat list of hits by `(owner_id, shot_id)`, pairing entry + exit.
    fn group_hits_by_shot(hits: &[ResolvedShotHit]) -> Vec<ShellHitGroup> {
        let mut groups: Vec<ShellHitGroup> = Vec::new();
        // Use an index map keyed by (owner_id, shot_id) for efficient lookup
        let mut index_map: HashMap<(EntityId, ShotId), usize> = HashMap::new();

        for hit in hits {
            let key = (hit.hit.owner_id, hit.hit.shot_id);
            let is_exit = matches!(hit.hit.hit_type.shell_hit, Recognized::Known(ShellHitType::ExitOverpenetration));

            if let Some(&idx) = index_map.get(&key) {
                // Already have a group for this shot — add as exit or replace entry
                if is_exit {
                    groups[idx].exit = Some(hit.clone());
                }
                // If not exit and group already has an entry, skip duplicate
            } else if is_exit {
                // Exit without an entry (rare) — create group anyway
                groups.push(ShellHitGroup {
                    entry: hit.clone(),
                    exit: None,
                    server_outcome: ServerOutcome::Overpenetration,
                });
                index_map.insert(key, groups.len() - 1);
            } else {
                let server_outcome = ServerOutcome::from_shell_hit_type(&hit.hit.hit_type.shell_hit);
                groups.push(ShellHitGroup { entry: hit.clone(), exit: None, server_outcome });
                index_map.insert(key, groups.len() - 1);
            }
        }
        groups
    }

    /// Process new shot hits from the bridge (server-authoritative impact data).
    ///
    /// Insert a shell entry into the appropriate `SalvoGroup`, creating one if needed.
    #[allow(clippy::too_many_arguments)]
    fn insert_shell_into_group(
        &mut self,
        hit: &ResolvedShotHit,
        shell: &ShellInfo,
        firing_range: Meters,
        trajectory_id: Option<u64>,
        comparison: Option<ServerVsSimComparison>,
        server_outcome: ServerOutcome,
        players: &[ReplayPlayerInfo],
    ) {
        let key = match hit.salvo.as_ref() {
            Some(salvo) => SalvoKey::Matched { owner_id: salvo.owner_id, salvo_id: salvo.salvo_id },
            None => {
                let id = self.unmatched_counter;
                self.unmatched_counter += 1;
                SalvoKey::Unmatched(id)
            }
        };

        let shell_entry = ShellEntry {
            shot_id: hit.hit.shot_id,
            clock: hit.clock,
            range: firing_range,
            trajectory_id,
            comparison,
            server_outcome,
            victim_roll: hit.victim_roll,
            victim_yaw: hit.victim_yaw,
            server_material_angle: hit.hit.terminal_ballistics.as_ref().map(|tb| tb.material_angle),
        };

        if let Some(&idx) = self.salvo_group_index.get(&key) {
            let group = &mut self.salvo_groups[idx];
            if hit.clock > group.latest_clock {
                group.latest_clock = hit.clock;
            }
            group.shells.push(shell_entry);
        } else {
            let attacker_name = players
                .iter()
                .find(|p| p.entity_id == hit.hit.owner_id)
                .map(|p| p.username.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            // Translate shell name via IDS_ localization key
            let shell_name = {
                use wowsunpack::data::ResourceLoader;
                let ids_key = format!("IDS_{}", shell.name.to_uppercase());
                self.ship_assets.metadata().localized_name_from_id(&ids_key).unwrap_or_else(|| shell.name.clone())
            };

            let idx = self.salvo_groups.len();
            self.salvo_groups.push(SalvoGroup {
                key: key.clone(),
                attacker_name,
                shell_name,
                ammo_type: shell.ammo_type.clone(),
                shell_info: Some(shell.clone()),
                first_clock: hit.clock,
                latest_clock: hit.clock,
                shells: vec![shell_entry],
            });
            self.salvo_group_index.insert(key, idx);
        }
    }

    /// Filter a shot hit by the current attacker/team/secondary settings.
    /// Returns `Some(shell)` if the hit should be processed, `None` to skip.
    fn filter_hit(&mut self, hit: &ResolvedShotHit, players: &[ReplayPlayerInfo]) -> Option<ShellInfo> {
        // Filter by selected attacker
        if let Some(ref sel) = self.selected_attacker {
            if hit.hit.owner_id != *sel {
                return None;
            }
        } else {
            // "All Enemies" mode: skip shots from the target's own team
            let same_team = players
                .iter()
                .find(|p| p.entity_id == hit.hit.owner_id)
                .map(|p| p.team_id == self.target_team_id)
                .unwrap_or(true);
            if same_team {
                return None;
            }
        }

        // Resolve shell info from the matched salvo's params_id
        let shell_info = hit.salvo.as_ref().map(|s| s.params_id).and_then(|pid| {
            self.shell_cache
                .entry(pid)
                .or_insert_with(|| self.ship_assets.metadata().resolve_shell_from_param_id(pid))
                .clone()
        });

        let shell = shell_info?;

        // Filter out secondary armament shells unless the toggle is on.
        if !self.show_secondaries
            && !self.is_main_battery.is_empty()
            && let Some(salvo) = &hit.salvo
            && !self.is_main_battery.contains_key(&salvo.params_id)
        {
            return None;
        }

        Some(shell)
    }

    /// Shared trajectory simulation core: given a resolved hit and shell info,
    /// ray-cast through the model, run AP simulation, build the arc, upload the
    /// trajectory visualization, and push a `StoredTrajectory`.
    ///
    /// Returns `None` if the ray misses the model entirely (or shell direction
    /// can't be determined). `comparison.exit_divergence` is always `None` —
    /// caller patches it in if needed.
    fn simulate_and_upload_trajectory(
        &mut self,
        hit: &ResolvedShotHit,
        shell: &ShellInfo,
        server_outcome: &ServerOutcome,
    ) -> Option<TrajectorySimResult> {
        let ship_yaw = hit.victim_yaw;
        let ship_world_pos = hit.victim_position;
        let salvo_shots: Vec<_> = hit.salvo.as_ref().map(|s| s.shots.clone()).unwrap_or_default();
        let impact_pos = hit.hit.position;
        let matched_shot = salvo_shots.iter().find(|s| s.shot_id == hit.hit.shot_id);
        let firing_range: Meters = matched_shot.map(|s| s.origin.distance_xz(&impact_pos)).unwrap_or(Meters::new(0.0));

        let rot = Self::inverse_ship_rotation(ship_yaw, hit.victim_pitch, hit.victim_roll);

        let shot = matched_shot?;

        let params = crate::armor_viewer::ballistics::ShellParams::from_shell_info(shell);
        let impact_result = crate::armor_viewer::ballistics::solve_for_range(&params, firing_range);

        let model_center = self.pane.loaded_armor.as_ref().map(|a| a.center()).unwrap_or(Vec3::zeros());
        let bounds = self.pane.loaded_armor.as_ref().map(|a| a.bounds);

        // Transform both origin and impact to mesh-space via world_to_model.
        let model_impact = Self::world_to_model(&impact_pos, &ship_world_pos, &rot, &model_center, bounds.as_ref());

        // Shell direction: transform the world-space travel vector (impact − origin)
        // through inverse_ship_rotation + axis_remap, then negate Z to match the
        // GLTF mesh's right-handed coordinate system (Z-negated during export).
        let world_dir =
            Vec3::new(impact_pos.x - shot.origin.x, impact_pos.y - shot.origin.y, impact_pos.z - shot.origin.z);
        let body_dir = rot * world_dir;
        let remapped = Self::axis_remap() * body_dir;
        let mesh_dir = Vec3::new(remapped.x, remapped.y, -remapped.z);
        let horiz_len = (mesh_dir.x * mesh_dir.x + mesh_dir.z * mesh_dir.z).sqrt();
        if horiz_len < 0.001 {
            return None;
        }
        let horiz_dir = Vec3::new(mesh_dir.x / horiz_len, 0.0, mesh_dir.z / horiz_len);
        let shell_dir: Vec3 = if let Some(ref imp) = impact_result {
            let horiz_angle = imp.impact_angle_horizontal as f32;
            let cos_h = horiz_angle.cos();
            let sin_h = horiz_angle.sin();
            Vec3::new(horiz_dir.x * cos_h, -sin_h, horiz_dir.z * cos_h).normalize()
        } else {
            horiz_dir
        };

        let ray_through = Vec3::new(model_impact.x, 0.0, model_impact.z);
        let ray_origin: Vec3 = ray_through - shell_dir * 100.0;
        let all_hits = self.pane.viewport.pick_all_ray_model_space(ray_origin, shell_dir);

        if all_hits.is_empty() {
            return None;
        }

        // Build trajectory hits
        let traj_hits =
            crate::armor_viewer::common::build_traj_hits(&all_hits, &self.pane.mesh_triangle_info, &shell_dir);

        // Build ImpactResult from ballistic simulation.
        // Note: terminalBallisticsInfo.velocity is post-impact (not incoming), so we
        // cannot use from_terminal_velocity() here — always simulate from range.
        let impact = crate::armor_viewer::ballistics::solve_for_range(&params, firing_range);

        // AP simulation + comparison
        let mut detonation_points = Vec::new();
        let mut last_visible_hit: Option<usize> = None;
        let mut comparison: Option<ServerVsSimComparison> = None;

        if shell.ammo_type == AmmoType::AP
            && let Some(ref imp) = impact
        {
            let ap = crate::armor_viewer::common::simulate_ap_shell(
                &params,
                imp,
                &traj_hits,
                &shell_dir,
                self.pane.continue_on_ricochet,
            );
            if let Some(pos) = ap.detonation_point {
                detonation_points
                    .push(crate::armor_viewer::penetration::DetonationMarker { position: pos, ship_index: 0 });
            }
            if let Some(idx) = ap.last_visible_hit {
                last_visible_hit = Some(last_visible_hit.map_or(idx, |prev: usize| prev.min(idx)));
            }

            let first_angle = traj_hits.first().map(|h| h.angle_deg).unwrap_or(0.0);
            let first_thickness =
                traj_hits.first().map(|h| Millimeters::new(h.thickness_mm)).unwrap_or(Millimeters::new(0.0));
            let verdict = crate::armor_viewer::penetration::compare_with_server(
                &ap.sim,
                &traj_hits,
                server_outcome,
                &params,
                first_angle,
                first_thickness,
            );

            comparison = Some(ServerVsSimComparison {
                server_outcome: server_outcome.clone(),
                sim: ap.sim,
                verdict,
                exit_divergence: None,
            });
        }

        // Build ballistic arc
        let approach_xz = crate::armor_viewer::common::approach_xz_from_shell_dir(&shell_dir);
        let model_extent = self.pane.loaded_armor.as_ref().map(|a| a.max_extent_xz()).unwrap_or(10.0);
        let first_hit_pos = traj_hits.first().map(|h| h.position).unwrap_or(model_center);

        let mut ship_arcs = Vec::new();
        if let Some(ref imp) = impact {
            let arc_points_3d = crate::armor_viewer::common::build_ballistic_arc_3d(
                &params,
                imp,
                approach_xz,
                first_hit_pos,
                model_extent,
            );
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
            hits: traj_hits.clone(),
            total_armor_mm: total_armor,
            ship_arcs,
            detonation_points,
        };

        // Pick trajectory color based on attacker
        let attacker_color_idx = self.enemy_players.iter().position(|p| p.entity_id == hit.hit.owner_id).unwrap_or(0);
        let traj_color = TRAJECTORY_PALETTE[attacker_color_idx % TRAJECTORY_PALETTE.len()];

        let cam_dist = self.pane.viewport.camera.distance;
        let (upload_color, upload_lw) = self.trajectory_display_params(traj_id, traj_color);
        let mesh_id = crate::armor_viewer::ui::tab::upload_trajectory_visualization(
            &mut self.pane.viewport,
            &result,
            &self.render_state.device,
            upload_color,
            last_visible_hit,
            cam_dist,
            self.pane.marker_opacity,
            upload_lw,
            self.pane.trajectory_world_space,
        );

        self.pane.trajectories.push(StoredTrajectory {
            meta: crate::armor_viewer::penetration::TrajectoryMeta {
                id: traj_id,
                color_index: attacker_color_idx % TRAJECTORY_PALETTE.len(),
                range: firing_range.to_km(),
            },
            result,
            mesh_id: Some(mesh_id),
            last_visible_hit,
            marker_cam_dist: cam_dist,
            show_plates_active: false,
            show_zones_active: false,
            shell_sim_cache: None,
            created_at_roll: self.pane.viewport.model_roll,
            created_at_yaw: self.pane.viewport.model_yaw,
        });

        Some(TrajectorySimResult { traj_id, comparison, traj_hits, firing_range })
    }

    /// Each `ResolvedShotHit` contains the actual world-space impact position and
    /// (optionally) terminal ballistics info (velocity, impact angle, detonator state).
    /// Hits are grouped by `shot_id` to pair entry + exit (for overpenetrations).
    /// We compare our simulation result against the server's authoritative outcome.
    fn process_new_shot_hits(&mut self) {
        let bridge = self.bridge.lock();

        let new_count = bridge.shot_hits.len();
        if new_count <= self.processed_hit_count {
            return;
        }

        let new_hits = bridge.shot_hits[self.processed_hit_count..].to_vec();
        let players = bridge.players.clone();
        self.processed_hit_count = new_count;
        drop(bridge);

        // Group hits by (owner_id, shot_id) to pair entry + exit
        let groups = Self::group_hits_by_shot(&new_hits);

        for group in &groups {
            let hit = &group.entry;

            let Some(shell) = self.filter_hit(hit, &players) else {
                continue;
            };

            let mut sim = self.simulate_and_upload_trajectory(hit, &shell, &group.server_outcome);

            // Compute exit divergence for AP overpenetrations
            if let Some(ref sim_result) = sim
                && group.server_outcome == ServerOutcome::Overpenetration
                && let Some(ref cmp) = sim_result.comparison
            {
                let rot = Self::inverse_ship_rotation(hit.victim_yaw, hit.victim_pitch, hit.victim_roll);
                let mc = self.pane.loaded_armor.as_ref().map(|a| a.center()).unwrap_or(Vec3::zeros());
                let exit_div = self.compute_exit_divergence(
                    group,
                    &cmp.sim,
                    &sim_result.traj_hits,
                    &hit.victim_position,
                    &rot,
                    &mc,
                );
                // Patch exit divergence into the comparison
                if let Some(ref mut s) = sim
                    && let Some(ref mut c) = s.comparison
                {
                    c.exit_divergence = exit_div;
                }
            }

            let firing_range = sim.as_ref().map(|s| s.firing_range).unwrap_or_else(|| {
                hit.salvo
                    .as_ref()
                    .and_then(|s| s.shots.iter().find(|sh| sh.shot_id == hit.hit.shot_id))
                    .map(|s| s.origin.distance_xz(&hit.hit.position))
                    .unwrap_or(Meters::new(0.0))
            });
            let traj_id = sim.as_ref().map(|s| s.traj_id);
            let comparison = sim.and_then(|s| s.comparison);

            self.insert_shell_into_group(
                hit,
                &shell,
                firing_range,
                traj_id,
                comparison,
                group.server_outcome.clone(),
                &players,
            );
            self.needs_repaint = true;
        }
    }

    /// Compute exit divergence between server and simulated overpen exit points.
    fn compute_exit_divergence(
        &self,
        group: &ShellHitGroup,
        sim: &crate::armor_viewer::penetration::ShellSimResult,
        traj_hits: &[crate::armor_viewer::penetration::TrajectoryHit],
        ship_world_pos: &WorldPos,
        rot: &Rotation3<f32>,
        model_center: &Vec3,
    ) -> Option<ExitDivergence> {
        // Server exit position from the ExitOverpenetration hit's terminal ballistics
        let exit_tb = group.exit.as_ref().and_then(|e| e.hit.terminal_ballistics.as_ref());
        let server_exit_world =
            exit_tb.map(|tb| tb.position).or_else(|| group.exit.as_ref().map(|e| e.hit.position))?;

        let bounds = self.pane.loaded_armor.as_ref().map(|a| a.bounds);
        let server_exit_model =
            Self::world_to_model(&server_exit_world, ship_world_pos, rot, model_center, bounds.as_ref());

        // Simulated exit: the last plate the shell passed through
        let sim_exit_pos = if sim.stopped_at.is_none() && !traj_hits.is_empty() {
            // Shell passed through all plates — exit is after the last hit
            traj_hits.last().map(|h| h.position)
        } else {
            None
        };

        let distance =
            sim_exit_pos.map(|sim_pos| crate::armor_viewer::penetration::distance_3d(&server_exit_model, &sim_pos));

        Some(ExitDivergence { server_exit_pos: server_exit_model, sim_exit_pos, distance })
    }

    /// Tick state: load ship, process salvos. Called before rendering.
    fn tick(&mut self) {
        // Populate enemy players from bridge on first availability
        if !self.players_populated {
            let bridge = self.bridge.lock();
            if !bridge.players.is_empty() {
                self.enemy_players =
                    bridge.players.iter().filter(|p| p.team_id != self.target_team_id).cloned().collect();
                // Build main battery params_id cache from all players' ammo lists
                {
                    use wowsunpack::game_params::types::GameParamProvider;
                    let metadata = self.ship_assets.metadata();
                    for player in &bridge.players {
                        if let Some(ammo_names) =
                            player.vehicle.vehicle().and_then(|v| v.config_data()).map(|c| &c.main_battery_ammo)
                        {
                            for name in ammo_names {
                                if let Some(param) = metadata.game_param_by_name(name) {
                                    self.is_main_battery.insert(param.id(), true);
                                }
                            }
                        }
                    }
                }
                self.players_populated = true;
                self.needs_repaint = true;
            }
        }

        // Start ship load on first frame
        if !self.ship_loaded && self.pane.load_receiver.is_none() && !self.pane.loading {
            self.start_ship_load();
            self.needs_repaint = true;
        }

        if self.pane.loading {
            self.needs_repaint = true;
        }

        // Poll ship load and hull LOD reload
        if crate::armor_viewer::common::poll_pane_load_receivers(
            &mut self.pane,
            &self.render_state.device,
            &self.render_state.queue,
            &self.gpu_pipeline,
        ) {
            self.ship_loaded = true;
            self.needs_repaint = true;
            if let Some(ref armor) = self.pane.loaded_armor {
                tracing::debug!(
                    "RealtimeArmorViewer: ship loaded — bounds min=[{:.1},{:.1},{:.1}] max=[{:.1},{:.1},{:.1}]",
                    armor.bounds.0.x,
                    armor.bounds.0.y,
                    armor.bounds.0.z,
                    armor.bounds.1.x,
                    armor.bounds.1.y,
                    armor.bounds.1.z,
                );
            }
        }

        // Poll for pre-computed shot timeline from the bridge
        if self.shot_timeline.is_none() {
            let bridge = self.bridge.lock();
            if let Some(ref tl) = bridge.shot_timeline {
                self.shot_timeline = Some(tl.clone());
            }
        }

        // Process shot hits if ship is loaded
        if self.ship_loaded {
            self.check_bridge_generation();
            // Ingest pre-computed timeline once available (replaces sliding window)
            if !self.timeline_ingested
                && let Some(ref _tl) = self.shot_timeline
            {
                self.ingest_precomputed_timeline();
                self.timeline_ingested = true;
            }
            self.process_new_shot_hits();
        } else {
            // Log occasionally that we're waiting for ship load
            static TICK_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
            if n.is_multiple_of(100) {
                tracing::debug!("RealtimeArmorViewer: tick #{n} — ship not loaded yet (loading={})", self.pane.loading);
            }
        }

        if self.needs_repaint {
            trace!("needs repaint");
        }
    }

    /// Get the victim_roll from the currently selected shell, or 0.0 if nothing is selected.
    fn selected_shell_roll(&self) -> f32 {
        let tid = match &self.selection {
            Some(SalvoSelection::Shell { trajectory_id, .. }) => Some(*trajectory_id),
            Some(SalvoSelection::Group(key)) => {
                // For single-shell groups, use that shell's roll
                self.salvo_group_index.get(key).and_then(|&idx| {
                    let g = &self.salvo_groups[idx];
                    if g.shells.len() == 1 { g.shells[0].trajectory_id } else { None }
                })
            }
            None => None,
        };
        tid.and_then(|tid| {
            self.salvo_groups
                .iter()
                .flat_map(|g| &g.shells)
                .find(|s| s.trajectory_id == Some(tid))
                .map(|s| s.victim_roll)
        })
        .unwrap_or(0.0)
    }

    /// Get the victim_yaw from the currently selected shell, or 0.0 if nothing is selected.
    fn selected_shell_yaw(&self) -> f32 {
        let tid = match &self.selection {
            Some(SalvoSelection::Shell { trajectory_id, .. }) => Some(*trajectory_id),
            Some(SalvoSelection::Group(key)) => self.salvo_group_index.get(key).and_then(|&idx| {
                let g = &self.salvo_groups[idx];
                if g.shells.len() == 1 { g.shells[0].trajectory_id } else { None }
            }),
            None => None,
        };
        tid.and_then(|tid| {
            self.salvo_groups
                .iter()
                .flat_map(|g| &g.shells)
                .find(|s| s.trajectory_id == Some(tid))
                .map(|s| s.victim_yaw)
        })
        .unwrap_or(0.0)
    }

    /// Compute the display color and line-width multiplier for a trajectory
    /// based on the current selection state.
    fn trajectory_display_params(&self, trajectory_id: u64, base_color: [f32; 4]) -> ([f32; 4], f32) {
        match &self.selection {
            None => (base_color, 1.0),
            Some(SalvoSelection::Group(sel_key)) => {
                let in_group = self
                    .salvo_groups
                    .iter()
                    .any(|g| g.key == *sel_key && g.shells.iter().any(|s| s.trajectory_id == Some(trajectory_id)));
                if in_group {
                    // Brighten
                    let [r, g, b, a] = base_color;
                    ([(r * 1.3).min(1.0), (g * 1.3).min(1.0), (b * 1.3).min(1.0), a], 1.5)
                } else {
                    // Dim
                    let [r, g, b, _] = base_color;
                    ([r, g, b, 0.2], 0.6)
                }
            }
            Some(SalvoSelection::Shell { group_key, trajectory_id: sel_tid }) => {
                if trajectory_id == *sel_tid {
                    // Emphasized
                    let [r, g, b, a] = base_color;
                    ([(r * 1.5).min(1.0), (g * 1.5).min(1.0), (b * 1.5).min(1.0), a], 2.0)
                } else {
                    let is_sibling = self.salvo_groups.iter().any(|g| {
                        g.key == *group_key && g.shells.iter().any(|s| s.trajectory_id == Some(trajectory_id))
                    });
                    if is_sibling {
                        // Moderate
                        let [r, g, b, _] = base_color;
                        ([r, g, b, 0.5], 1.0)
                    } else {
                        // Dim
                        let [r, g, b, _] = base_color;
                        ([r, g, b, 0.15], 0.5)
                    }
                }
            }
        }
    }

    /// Ingest all pre-computed shot hits from the timeline into salvo groups.
    /// Shells are added without trajectory simulation (lazy — done on selection).
    fn ingest_precomputed_timeline(&mut self) {
        let timeline = match self.shot_timeline {
            Some(ref tl) => Arc::clone(tl),
            None => return,
        };
        let hits = &timeline.hits;
        if hits.is_empty() {
            return;
        }

        let bridge = self.bridge.lock();
        let players = bridge.players.clone();
        drop(bridge);

        for pre_hit in hits.iter() {
            let hit = &pre_hit.hit;

            let Some(shell) = self.filter_hit(hit, &players) else {
                continue;
            };

            let firing_range: Meters = hit
                .salvo
                .as_ref()
                .and_then(|s| s.shots.iter().find(|sh| sh.shot_id == hit.hit.shot_id))
                .map(|s| s.origin.distance_xz(&hit.hit.position))
                .unwrap_or(Meters::new(0.0));

            // Group hits by (owner_id, shot_id) — but for ingestion we process one at a time
            // and rely on insert_shell_into_group to handle grouping.
            let server_outcome = ServerOutcome::from_shell_hit_type(&hit.hit.hit_type.shell_hit);
            self.insert_shell_into_group(hit, &shell, firing_range, None, None, server_outcome, &players);
        }

        // Update processed_hit_count so live-edge process_new_shot_hits doesn't re-add
        self.processed_hit_count = self.bridge.lock().shot_hits.len();
        self.needs_repaint = true;

        tracing::info!(
            "RealtimeArmorViewer: ingested {} pre-computed hits into {} salvo groups",
            hits.len(),
            self.salvo_groups.len(),
        );
    }

    /// Simulate trajectories for all un-simulated shells in the given salvo group.
    /// Called lazily when the user selects a group or shell.
    fn ensure_trajectories_simulated(&mut self, key: &SalvoKey) {
        let group_idx = match self.salvo_group_index.get(key) {
            Some(&idx) => idx,
            None => return,
        };

        // Check if any shells need simulation
        let needs_sim: Vec<usize> = self.salvo_groups[group_idx]
            .shells
            .iter()
            .enumerate()
            .filter(|(_, s)| s.trajectory_id.is_none())
            .map(|(i, _)| i)
            .collect();

        if needs_sim.is_empty() {
            return;
        }

        // We need the pre-computed timeline to get the full ResolvedShotHit data
        let timeline = match &self.shot_timeline {
            Some(tl) => Arc::clone(tl),
            None => return,
        };

        // Collect the shot_ids we need to simulate
        let shot_ids_to_sim: Vec<ShotId> =
            needs_sim.iter().map(|&i| self.salvo_groups[group_idx].shells[i].shot_id).collect();

        // Find matching PreExtractedHits from the timeline
        let mut hit_map: HashMap<ShotId, &crate::replay::renderer::PreExtractedHit> = HashMap::new();
        for pre_hit in &timeline.hits {
            if shot_ids_to_sim.contains(&pre_hit.hit.hit.shot_id) {
                hit_map.insert(pre_hit.hit.hit.shot_id, pre_hit);
            }
        }

        for &shell_idx in &needs_sim {
            let shot_id = self.salvo_groups[group_idx].shells[shell_idx].shot_id;
            let pre_hit = match hit_map.get(&shot_id) {
                Some(ph) => ph,
                None => continue,
            };
            let hit = &pre_hit.hit;

            // Resolve shell info
            let shell_info = hit.salvo.as_ref().map(|s| s.params_id).and_then(|pid| {
                self.shell_cache
                    .entry(pid)
                    .or_insert_with(|| self.ship_assets.metadata().resolve_shell_from_param_id(pid))
                    .clone()
            });
            let Some(shell) = shell_info else { continue };

            let server_outcome = self.salvo_groups[group_idx].shells[shell_idx].server_outcome.clone();
            if let Some(sim) = self.simulate_and_upload_trajectory(hit, &shell, &server_outcome) {
                self.salvo_groups[group_idx].shells[shell_idx].trajectory_id = Some(sim.traj_id);
                self.salvo_groups[group_idx].shells[shell_idx].comparison = sim.comparison;
            }
        }

        self.selection_dirty = true;
        self.needs_repaint = true;
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

                // Tick inside the viewport so load-completion is detected here
                // (cross-window request_repaint doesn't reliably wake deferred
                // viewports, so we can't depend on the parent's tick alone).
                viewer.tick();

                egui::CentralPanel::default().show(ctx, |ui| {
                    viewer.draw_content(ui);
                });

                // Keep the viewport alive while loading so tick() can poll
                // the load receiver. Only repaint this viewport — do NOT wake
                // the parent, which causes event-loop starvation on Windows.
                if viewer.pane.loading {
                    ctx.request_repaint();
                }

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

    /// Draw a compass rose overlay in the top-left corner of the viewport.
    /// Shows absolute world cardinal directions based on camera azimuth only
    /// (independent of ship yaw/roll and camera elevation).
    fn draw_compass(&self, painter: &egui::Painter, viewport_rect: egui::Rect) {
        let vp = &self.pane.viewport;

        // The camera azimuth rotates the view around the Y axis. At azimuth=0,
        // the camera looks along mesh -Z. We need to map BigWorld cardinal
        // directions to screen (right, up) using only the horizontal camera angle.
        //
        // BigWorld: +X = East, +Z = North (from heading conversion: yaw = PI/2 - heading).
        // Mesh coordinate system (from world_to_model axis remap):
        //   mesh +Z = BigWorld +X (East) direction at yaw=0
        //   mesh -X = BigWorld +Z (North) direction
        //
        // Camera at azimuth=a looks from direction (sin(a), 0, cos(a)).
        // Screen-right = camera's right vector projected on XZ = (cos(a), 0, -sin(a))
        // Screen-up projected on XZ = camera's up, but for a top-down compass we use
        // the forward direction on the ground plane = (-sin(a), 0, -cos(a))
        //
        // For a mesh-space direction (mx, 0, mz):
        //   screen_x = mx * cos(a) + mz * (-sin(a))
        //   screen_y = mx * (-sin(a)) + mz * (-cos(a))    (up on screen = into the screen on ground)
        let az = vp.camera.azimuth;
        let (sa, ca) = az.sin_cos();

        let project_world_dir = |world_x: f32, world_z: f32| -> (f32, f32) {
            // BigWorld -> mesh: mesh_x = -world_z, mesh_z = +world_x
            let mx = -world_z;
            let mz = world_x;
            // Project onto screen using camera azimuth
            let screen_x = mx * ca - mz * sa;
            let screen_y = -(mx * sa + mz * ca);
            (screen_x, screen_y)
        };

        let (nx, ny) = project_world_dir(0.0, 1.0);
        let (ex, ey) = project_world_dir(1.0, 0.0);
        let (sx, sy) = (-nx, -ny);
        let (wx, wy) = (-ex, -ey);

        // Compass center: top-left corner with padding
        let radius = 30.0_f32;
        let center = egui::pos2(viewport_rect.left() + radius + 12.0, viewport_rect.top() + radius + 12.0);

        // Background circle
        painter.circle_filled(center, radius + 4.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140));
        painter.circle_stroke(
            center,
            radius + 4.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(180, 180, 180, 100)),
        );

        let directions: [(&str, (f32, f32), egui::Color32); 4] = [
            ("N", (nx, ny), egui::Color32::from_rgb(255, 80, 80)),
            ("W", (ex, ey), egui::Color32::from_rgb(180, 180, 180)),
            ("S", (sx, sy), egui::Color32::from_rgb(180, 180, 180)),
            ("E", (wx, wy), egui::Color32::from_rgb(180, 180, 180)),
        ];

        for &(label, (dx, dy), color) in &directions {
            // Normalize the projected direction to unit length
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                continue;
            }
            let (ux, uy) = (dx / len, dy / len);

            // Arm endpoint (screen Y is flipped: up = negative in screen coords)
            let arm_end = egui::pos2(center.x + ux * radius, center.y - uy * radius);

            // Draw arm line
            let stroke_width = if label == "N" { 2.0 } else { 1.0 };
            painter.line_segment([center, arm_end], egui::Stroke::new(stroke_width, color));

            // Draw label at the tip
            let label_pos = egui::pos2(center.x + ux * (radius + 10.0), center.y - uy * (radius + 10.0));
            let font = egui::FontId::proportional(if label == "N" { 12.0 } else { 10.0 });
            painter.text(label_pos, egui::Align2::CENTER_CENTER, label, font, color);
        }
    }

    /// Draw the 3D armor viewport with toolbar and plate interaction.
    fn draw_viewport(&mut self, ui: &mut egui::Ui) {
        if self.pane.loading {
            ui.centered_and_justified(|ui| {
                ui.spinner();
                ui.label(t!("ui.armor.realtime.loading"));
            });
            return;
        }

        if !self.ship_loaded {
            ui.centered_and_justified(|ui| {
                ui.label(t!("ui.armor.realtime.waiting"));
            });
            return;
        }

        let mut zone_changed = false;
        let translate_part = |name: &str| -> String { name.to_string() };

        // Undo/redo keyboard shortcuts
        if crate::armor_viewer::common::handle_undo_redo(ui, &mut self.pane) {
            zone_changed = true;
        }

        // Toolbar
        let prev_marker_opacity = self.pane.marker_opacity;
        let sidebar_hovered_key: std::cell::Cell<Option<SidebarHighlightKey>> = std::cell::Cell::new(None);
        let lod_change_signal: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
        let hull_change_signal: std::cell::Cell<bool> = std::cell::Cell::new(false);
        if let Some(armor) = self.pane.loaded_armor.take() {
            if !armor.zone_parts.is_empty() {
                ui.horizontal(|ui| {
                    // Armor Zones button
                    let armor_btn = ui
                        .button(wt_translations::icon_t(icons::SHIELD, &t!("ui.armor.armor_toggle")))
                        .on_hover_text(t!("ui.armor.armor_tooltip"));
                    egui::Popup::from_toggle_button_response(&armor_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            let (changed, hkey) = crate::armor_viewer::ui::tab::draw_armor_visibility_popover(
                                ui,
                                &mut self.pane,
                                &armor,
                                &translate_part,
                            );
                            if changed {
                                zone_changed = true;
                            }
                            if hkey.is_some() {
                                sidebar_hovered_key.set(hkey);
                            }
                        });

                    // Hull Model button with popover
                    if !armor.hull_part_groups.is_empty() {
                        let hull_btn = ui
                            .button(wt_translations::icon_t(icons::THREE_D, &t!("ui.armor.hull_toggle")))
                            .on_hover_text(t!("ui.armor.hull_tooltip"));
                        egui::Popup::from_toggle_button_response(&hull_btn)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let hull_result = crate::armor_viewer::ui::tab::draw_hull_visibility_popover(
                                    ui,
                                    &mut self.pane,
                                    &armor,
                                );
                                if hull_result.zone_changed {
                                    zone_changed = true;
                                }
                                if let Some(k) = hull_result.hovered_key {
                                    sidebar_hovered_key.set(Some(k));
                                }
                                if hull_result.new_lod.is_some() {
                                    lod_change_signal.set(hull_result.new_lod);
                                }
                                if hull_result.hull_changed {
                                    hull_change_signal.set(true);
                                }
                            });
                    }

                    // ── Splash Boxes button with popover ──
                    if !armor.splash_box_groups.is_empty() {
                        let splash_label = if self.pane.show_splash_boxes {
                            format!("{} {} \u{25CF}", icons::CUBE, t!("ui.armor.splash_toggle"))
                        } else {
                            wt_translations::icon_t(icons::CUBE, &t!("ui.armor.splash_toggle"))
                        };
                        let splash_btn = ui.button(splash_label).on_hover_text(t!("ui.armor.splash_tooltip"));
                        egui::Popup::from_toggle_button_response(&splash_btn)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let (changed, hkey) = crate::armor_viewer::ui::tab::draw_splash_box_visibility_popover(
                                    ui,
                                    &mut self.pane,
                                    &armor,
                                );
                                if changed {
                                    zone_changed = true;
                                }
                                if let Some(k) = hkey {
                                    sidebar_hovered_key.set(Some(k));
                                }
                            });
                    }

                    // Display settings button
                    let display_btn = ui
                        .button(wt_translations::icon_t(icons::GEAR_FINE, &t!("ui.armor.display")))
                        .on_hover_text(t!("ui.armor.display_tooltip"));
                    egui::Popup::from_toggle_button_response(&display_btn)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            if crate::armor_viewer::ui::tab::draw_display_settings_popover(ui, &mut self.pane, &armor) {
                                zone_changed = true;
                            }
                            if !self.pane.trajectories.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label(t!("ui.armor.marker_opacity"));
                                    ui.add(
                                        egui::Slider::new(&mut self.pane.marker_opacity, 0.0..=1.0).fixed_decimals(2),
                                    );
                                });
                            }
                        });

                    // ── Roll slider ──
                    ui.separator();
                    crate::armor_viewer::ui::tab::draw_roll_slider(ui, &mut self.pane.viewport);
                });
                ui.separator();
            }
            self.pane.loaded_armor = Some(armor);
        }

        // Handle hull upgrade change — full ship reload
        if hull_change_signal.get() {
            self.start_ship_load();
        }

        // Handle LOD change from hull popover — hull-only reload
        if let Some(new_lod) = lod_change_signal.into_inner()
            && let Some(param_index) = self.pane.selected_ship.clone()
        {
            crate::armor_viewer::ui::tab::start_hull_lod_reload(
                &mut self.pane,
                &self.ship_assets,
                &param_index,
                new_lod,
            );
        }

        // Sidebar hover highlight lifecycle
        if let Some(armor) = self.pane.loaded_armor.take() {
            crate::armor_viewer::common::update_sidebar_highlight(
                &mut self.pane,
                &armor,
                sidebar_hovered_key.into_inner(),
                &self.render_state.device,
            );
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
            if crate::armor_viewer::ui::tab::handle_plate_interaction(
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

            // Draw splash box labels on top of the viewport
            crate::armor_viewer::ui::tab::draw_splash_box_labels(&self.pane, ui.painter(), response.rect);

            // Draw disclaimer watermark
            crate::armor_viewer::ui::tab::draw_viewport_watermark(ui.painter(), response.rect);

            // Draw compass rose overlay
            self.draw_compass(ui.painter(), response.rect);
        }

        // Re-upload armor and trajectories if visibility changed
        let marker_opacity_changed = (self.pane.marker_opacity - prev_marker_opacity).abs() > 0.001;
        let needs_traj_reupload = zone_changed || marker_opacity_changed || self.selection_dirty;

        if needs_traj_reupload {
            // Pre-compute display params per trajectory (selection-aware color + line width)
            let dp: Vec<crate::armor_viewer::common::TrajectoryDisplayParams> = self
                .pane
                .trajectories
                .iter()
                .map(|traj| {
                    let base_color = TRAJECTORY_PALETTE[traj.meta.color_index % TRAJECTORY_PALETTE.len()];
                    let (color, lw) = self.trajectory_display_params(traj.meta.id, base_color);
                    crate::armor_viewer::common::TrajectoryDisplayParams { color, line_width_mult: lw }
                })
                .collect();

            if zone_changed {
                // Full re-upload: armor + trajectories + splash wireframes
                crate::armor_viewer::common::reupload_after_zone_change(
                    &mut self.pane,
                    &self.render_state.device,
                    &self.render_state.queue,
                    &self.gpu_pipeline,
                    &[],
                    false,
                    &dp,
                );
            } else if !self.pane.trajectories.is_empty() {
                // Only trajectories need re-upload (marker opacity or selection change)
                crate::armor_viewer::common::reupload_trajectory_meshes(
                    &mut self.pane,
                    &self.render_state.device,
                    &dp,
                    true,
                );
            }
            self.selection_dirty = false;
            self.needs_repaint = true;
        }
    }

    /// Draw the health timeline strip: health line + shot ticks + current time marker.
    /// Returns `Some(clock)` if the user clicked to seek to a specific time.
    fn draw_health_timeline(&self, ui: &mut egui::Ui) -> Option<GameClock> {
        let timeline = self.shot_timeline.as_ref()?;
        if timeline.health_history.is_empty() {
            return None;
        }

        let current_clock = self.bridge.lock().last_clock;

        // Determine time range from health history
        let first_clock = timeline.health_history.keys().next()?.seconds();
        let last_clock = timeline.health_history.keys().next_back()?.seconds();
        let time_span = (last_clock - first_clock).max(1.0);

        let desired_size = egui::vec2(ui.available_width(), 60.0);
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click());
        let rect = response.rect;

        // Background
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(30));

        let map_x = |clock: f32| -> f32 {
            let t = ((clock - first_clock) / time_span).clamp(0.0, 1.0);
            rect.left() + t * rect.width()
        };

        // Draw shot impact ticks (red vertical lines at bottom)
        let tick_height = rect.height() * 0.2;
        for pre_hit in &timeline.hits {
            let x = map_x(pre_hit.clock.seconds());
            painter.line_segment(
                [egui::pos2(x, rect.bottom() - tick_height), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 60, 60, 140)),
            );
        }

        // Draw health line (green)
        let health_points: Vec<egui::Pos2> = timeline
            .health_history
            .iter()
            .map(|(clock, snap)| {
                let x = map_x(clock.seconds());
                let ratio = if snap.max_health > 0.0 { snap.health / snap.max_health } else { 1.0 };
                let y = rect.bottom() - tick_height - ratio * (rect.height() - tick_height);
                egui::pos2(x, y)
            })
            .collect();

        if health_points.len() >= 2 {
            for window in health_points.windows(2) {
                painter
                    .line_segment([window[0], window[1]], egui::Stroke::new(1.5, egui::Color32::from_rgb(80, 220, 80)));
            }
        }

        // Current time marker (white vertical line)
        let current_x = map_x(current_clock.seconds());
        painter.line_segment(
            [egui::pos2(current_x, rect.top()), egui::pos2(current_x, rect.bottom())],
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        );

        // Time labels
        let start_min = (first_clock / 60.0).floor() as i32;
        let start_sec = (first_clock % 60.0) as i32;
        let end_min = (last_clock / 60.0).floor() as i32;
        let end_sec = (last_clock % 60.0) as i32;
        painter.text(
            rect.left_top() + egui::vec2(2.0, 1.0),
            egui::Align2::LEFT_TOP,
            format!("{}:{:02}", start_min, start_sec),
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(160),
        );
        painter.text(
            rect.right_top() + egui::vec2(-2.0, 1.0),
            egui::Align2::RIGHT_TOP,
            format!("{}:{:02}", end_min, end_sec),
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(160),
        );

        // Click to seek
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let seek_clock = first_clock + t * time_span;
            return Some(GameClock(seek_clock));
        }

        None
    }

    /// Draw the side panel with attacker selector and salvo log.
    fn draw_side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(t!("ui.armor.realtime.incoming_fire"));
        ui.separator();

        // Health timeline strip
        if let Some(seek_clock) = self.draw_health_timeline(ui)
            && let Some(ref tx) = self.command_tx
        {
            let _ = tx.send(crate::replay::renderer::PlaybackCommand::Seek(seek_clock));
        }
        ui.separator();

        // Attacker selector
        ui.label(egui::RichText::new(t!("ui.armor.realtime.attacker_filter").as_ref()).strong());
        let all_enemies = t!("ui.armor.realtime.all_enemies");
        let current_label = if let Some(ref sel) = self.selected_attacker {
            self.enemy_players
                .iter()
                .find(|p| p.entity_id == *sel)
                .map(|p| format!("{} ({})", p.username, p.ship_display_name))
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            all_enemies.to_string()
        };

        let mut attacker_changed = false;
        egui::ComboBox::from_id_salt("attacker_selector").selected_text(&current_label).show_ui(ui, |ui| {
            if ui.selectable_value(&mut self.selected_attacker, None, all_enemies.as_ref()).changed() {
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

        if ui.checkbox(&mut self.show_secondaries, t!("ui.armor.realtime.show_secondaries")).changed() {
            self.clear_and_reprocess();
            self.needs_repaint = true;
        }

        if ui.checkbox(&mut self.pane.continue_on_ricochet, t!("ui.armor.continue_ricochet")).changed() {
            self.clear_and_reprocess();
            self.needs_repaint = true;
        }

        ui.separator();

        // Stats
        let total_shells: usize = self.salvo_groups.iter().map(|g| g.shells.len()).sum();
        ui.label(
            egui::RichText::new(format!("{} salvos, {} shells tracked", self.salvo_groups.len(), total_shells)).small(),
        );

        ui.separator();

        // Salvo log — collapsible groups with individual shell entries
        #[derive(Clone)]
        enum ClickAction {
            SelectAllInGroup(SalvoKey),
            SelectShell {
                group_key: SalvoKey,
                trajectory_id: u64,
            },
            /// Select a shell by index (used when trajectory_id is not yet computed).
            SelectShellByIndex {
                group_key: SalvoKey,
                shell_index: usize,
            },
            /// Seek replay to a specific clock time.
            SeekTo(GameClock),
        }
        let mut click_action: Option<ClickAction> = None;

        let sel_bg = ui.visuals().selection.bg_fill;
        let normal_bg = ui.visuals().widgets.noninteractive.bg_fill;
        let active_bg = egui::Color32::from_rgba_unmultiplied(255, 200, 60, 30);

        // Auto-scroll toggle + current clock
        let current_clock = self.bridge.lock().last_clock;
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.auto_scroll, t!("ui.armor.realtime.auto_scroll"));
            let cs_f = current_clock.seconds();
            let cm = (cs_f / 60.0).floor() as i32;
            let cs = (cs_f % 60.0) as i32;
            ui.label(egui::RichText::new(format!("{}:{:02}", cm, cs)).small().weak());
        });

        // Find the target group for auto-scroll (last group with first_clock <= current_clock)
        let auto_scroll_target = if self.auto_scroll && (current_clock - self.last_auto_scroll_clock).abs() > 0.5 {
            self.last_auto_scroll_clock = current_clock;
            self.salvo_groups.iter().rposition(|g| g.first_clock <= current_clock)
        } else {
            None
        };

        egui::ScrollArea::vertical()
            .id_salt("salvo_log_scroll")
            .auto_shrink([false; 2])
            .max_height(ui.available_height() * 0.4)
            .show(ui, |ui| {
                for group_idx in 0..self.salvo_groups.len() {
                    let group = &self.salvo_groups[group_idx];
                    let group_key = group.key.clone();
                    let shell_count = group.shells.len();

                    let clock_secs = group.first_clock.seconds();

                    // Is this salvo "active" (within 2s of current clock)?
                    let is_active = (group.first_clock - current_clock).abs() < 2.0;
                    let time_min = (clock_secs / 60.0).floor() as i32;
                    let time_sec = (clock_secs % 60.0) as i32;

                    let avg_range_km = if group.shells.is_empty() {
                        0.0
                    } else {
                        let sum: f32 = group.shells.iter().map(|s| s.range.to_km().value()).sum();
                        sum / shell_count as f32
                    };

                    // Is this group (or a shell in it) selected?
                    let group_selected = match &self.selection {
                        Some(SalvoSelection::Group(k)) => *k == group_key,
                        Some(SalvoSelection::Shell { group_key: gk, .. }) => *gk == group_key,
                        None => false,
                    };

                    let caliber_str =
                        group.shell_info.as_ref().map(|s| format!("{:.0}mm", s.caliber.value())).unwrap_or_default();

                    let header_text = format!(
                        "{}:{:02} \u{2014} {}x {} {} \u{2022} {} \u{2022} {:.1}km",
                        time_min,
                        time_sec,
                        shell_count,
                        caliber_str,
                        group.shell_name,
                        group.attacker_name,
                        avg_range_km,
                    );

                    if shell_count <= 1 {
                        // Single-shell group: just a clickable frame, no collapsing
                        let header_bg = if group_selected {
                            sel_bg
                        } else if is_active {
                            active_bg
                        } else {
                            normal_bg
                        };
                        let resp = egui::Frame::group(ui.style()).fill(header_bg).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .small_button(icons::CLOCK_COUNTER_CLOCKWISE)
                                    .on_hover_text(t!("ui.armor.realtime.seek_to_salvo"))
                                    .clicked()
                                {
                                    click_action = Some(ClickAction::SeekTo(group.first_clock));
                                }
                                ui.label(egui::RichText::new(&header_text).strong().small());
                            });
                            if let Some(shell) = group.shells.first() {
                                let outcome_str = shell.server_outcome.display_name();
                                ui.label(egui::RichText::new(format!("  {}", outcome_str)).small());
                            }
                        });
                        if resp.response.interact(egui::Sense::click()).clicked() {
                            // For single-shell, select the shell directly (trigger sim if needed)
                            if let Some(tid) = group.shells.first().and_then(|s| s.trajectory_id) {
                                click_action =
                                    Some(ClickAction::SelectShell { group_key: group_key.clone(), trajectory_id: tid });
                            } else {
                                click_action = Some(ClickAction::SelectShellByIndex {
                                    group_key: group_key.clone(),
                                    shell_index: 0,
                                });
                            }
                        }
                    } else {
                        // Multi-shell group: collapsing header with select-all button
                        let collapsing_id = ui.make_persistent_id(format!("salvo_group_{}", group_idx));
                        let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            collapsing_id,
                            false,
                        );

                        state
                            .show_header(ui, |ui| {
                                if ui
                                    .small_button(icons::CLOCK_COUNTER_CLOCKWISE)
                                    .on_hover_text(t!("ui.armor.realtime.seek_to_salvo"))
                                    .clicked()
                                {
                                    click_action = Some(ClickAction::SeekTo(group.first_clock));
                                }
                                // Select All / Deselect button
                                let btn_label = if group_selected {
                                    t!("ui.armor.realtime.deselect")
                                } else {
                                    t!("ui.armor.realtime.select_all")
                                };
                                if ui.small_button(btn_label.as_ref()).clicked() {
                                    click_action = Some(ClickAction::SelectAllInGroup(group_key.clone()));
                                }
                                ui.label(egui::RichText::new(&header_text).strong().small());
                            })
                            .body(|ui| {
                                for (shell_idx, shell) in group.shells.iter().enumerate() {
                                    let shell_selected = matches!(
                                        &self.selection,
                                        Some(SalvoSelection::Shell { trajectory_id: tid, .. })
                                        if shell.trajectory_id == Some(*tid)
                                    );
                                    let shell_bg = if shell_selected { sel_bg } else { normal_bg };

                                    let outcome_str = shell.server_outcome.display_name();
                                    let range_km = shell.range.to_km().value();
                                    let label = format!(
                                        "Shell #{} \u{2022} {:.1}km \u{2022} {}",
                                        shell_idx + 1,
                                        range_km,
                                        outcome_str,
                                    );

                                    let shell_resp = egui::Frame::group(ui.style())
                                        .fill(shell_bg)
                                        .inner_margin(egui::Margin::symmetric(8, 2))
                                        .show(ui, |ui| {
                                            ui.label(egui::RichText::new(label).small());
                                        });

                                    if shell_resp.response.interact(egui::Sense::click()).clicked() {
                                        if let Some(tid) = shell.trajectory_id {
                                            click_action = Some(ClickAction::SelectShell {
                                                group_key: group_key.clone(),
                                                trajectory_id: tid,
                                            });
                                        } else {
                                            click_action = Some(ClickAction::SelectShellByIndex {
                                                group_key: group_key.clone(),
                                                shell_index: shell_idx,
                                            });
                                        }
                                    }
                                }
                            });
                    }

                    // Auto-scroll: scroll this group into view if it's the target
                    if auto_scroll_target == Some(group_idx) {
                        ui.scroll_to_cursor(Some(egui::Align::Center));
                    }
                }
            });

        // Process click actions
        if let Some(action) = click_action {
            match action {
                ClickAction::SelectAllInGroup(key) => {
                    let already_selected = matches!(&self.selection, Some(SalvoSelection::Group(k)) if *k == key);
                    if already_selected {
                        self.selection = None;
                    } else {
                        self.ensure_trajectories_simulated(&key);
                        self.selection = Some(SalvoSelection::Group(key));
                    }
                    self.auto_scroll = false;
                    self.selection_dirty = true;
                    self.needs_repaint = true;
                }
                ClickAction::SelectShell { group_key, trajectory_id } => {
                    let already_selected = matches!(
                        &self.selection,
                        Some(SalvoSelection::Shell { trajectory_id: tid, .. }) if *tid == trajectory_id
                    );
                    if already_selected {
                        self.selection = None;
                    } else {
                        self.selection = Some(SalvoSelection::Shell { group_key, trajectory_id });
                    }
                    self.auto_scroll = false;
                    self.selection_dirty = true;
                    self.needs_repaint = true;
                }
                ClickAction::SelectShellByIndex { group_key, shell_index } => {
                    // Trigger lazy simulation, then select by the now-populated trajectory_id
                    self.ensure_trajectories_simulated(&group_key);
                    if let Some(&idx) = self.salvo_group_index.get(&group_key)
                        && let Some(shell) = self.salvo_groups[idx].shells.get(shell_index)
                    {
                        if let Some(tid) = shell.trajectory_id {
                            self.selection = Some(SalvoSelection::Shell { group_key, trajectory_id: tid });
                        } else {
                            // Simulation didn't produce a trajectory (no armor hit)
                            self.selection = Some(SalvoSelection::Group(group_key));
                        }
                    }
                    self.selection_dirty = true;
                    self.needs_repaint = true;
                }
                ClickAction::SeekTo(clock) => {
                    if let Some(ref tx) = self.command_tx {
                        let _ = tx.send(crate::replay::renderer::PlaybackCommand::Seek(clock));
                    }
                }
            }

            // Update viewport model roll/yaw from the selected shell's victim orientation
            let roll = self.selected_shell_roll();
            if (self.pane.viewport.model_roll - roll).abs() > 1e-6 {
                self.pane.viewport.model_roll = roll;
                self.pane.viewport.mark_dirty();
            }
            // model_yaw = +victim_yaw: the renderer applies Ry(model_yaw) to the
            // hull mesh (bow=+Z), rotating the bow to the correct compass heading.
            let yaw = self.selected_shell_yaw();
            if (self.pane.viewport.model_yaw - yaw).abs() > 1e-6 {
                self.pane.viewport.model_yaw = yaw;
                self.pane.viewport.mark_dirty();
            }
        }

        // Detail panel for selected shell
        let selected_trajectory_id = match &self.selection {
            Some(SalvoSelection::Shell { trajectory_id, .. }) => Some(*trajectory_id),
            Some(SalvoSelection::Group(key)) => {
                // For single-shell groups, show detail
                self.salvo_group_index.get(key).and_then(|&idx| {
                    let g = &self.salvo_groups[idx];
                    if g.shells.len() == 1 { g.shells[0].trajectory_id } else { None }
                })
            }
            None => None,
        };
        if let Some(tid) = selected_trajectory_id {
            ui.separator();
            self.draw_salvo_detail(ui, tid);
        }
    }

    /// Draw the plate-by-plate detail panel for a selected salvo.
    fn draw_salvo_detail(&self, ui: &mut egui::Ui, trajectory_id: u64) {
        use crate::armor_viewer::penetration::PlateOutcome;
        use crate::armor_viewer::penetration::enclosing_zone;

        // Find the matching shell entry (from salvo groups) and trajectory
        let shell_and_group = self
            .salvo_groups
            .iter()
            .find_map(|g| g.shells.iter().find(|s| s.trajectory_id == Some(trajectory_id)).map(|s| (s, g)));
        let traj = self.pane.trajectories.iter().find(|t| t.meta.id == trajectory_id);

        let (Some((shell_entry, group)), Some(traj)) = (shell_and_group, traj) else {
            ui.label(egui::RichText::new(t!("ui.armor.realtime.no_data").as_ref()).small().weak());
            return;
        };

        let result = &traj.result;

        // Header
        ui.label(egui::RichText::new(&group.shell_name).strong());

        // Impact stats from the ballistic arc
        if let Some(arc) = result.ship_arcs.first()
            && let Some(ref impact) = arc.ballistic_impact
        {
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

        // Use stored comparison if available (AP), otherwise compute HE/SAP outcome
        let sim = shell_entry.comparison.as_ref().map(|c| &c.sim);

        // Server outcome + comparison verdict (AP with comparison data)
        if let Some(ref cmp) = shell_entry.comparison {
            // Server outcome line
            let server_color = match cmp.server_outcome {
                ServerOutcome::Penetration => egui::Color32::from_rgb(255, 140, 40),
                ServerOutcome::Citadel => egui::Color32::from_rgb(255, 80, 80),
                ServerOutcome::Ricochet => egui::Color32::from_rgb(120, 120, 255),
                ServerOutcome::Shatter => egui::Color32::RED,
                ServerOutcome::Overpenetration => egui::Color32::YELLOW,
                ServerOutcome::Underwater => egui::Color32::from_rgb(80, 180, 255),
                ServerOutcome::Unknown(_) => egui::Color32::GRAY,
            };
            ui.label(
                egui::RichText::new(format!("Server: {}", cmp.server_outcome.display_name()))
                    .strong()
                    .small()
                    .color(server_color),
            );

            // Verdict line
            match &cmp.verdict {
                ComparisonVerdict::Match => {
                    ui.label(
                        egui::RichText::new(t!("ui.armor.realtime.sim_agrees").as_ref())
                            .small()
                            .color(egui::Color32::from_rgb(80, 220, 80)),
                    );
                }
                ComparisonVerdict::RicochetRngDefer { angle_deg, range_start_deg, range_end_deg } => {
                    ui.label(
                        egui::RichText::new(format!(
                            "RNG zone ({:.1}° in [{:.1}°–{:.1}°])",
                            angle_deg, range_start_deg, range_end_deg,
                        ))
                        .small()
                        .color(egui::Color32::YELLOW),
                    );
                }
                ComparisonVerdict::Mismatch { sim_desc, server_desc } => {
                    ui.label(
                        egui::RichText::new(format!("Sim: {} / Server: {}", sim_desc, server_desc))
                            .small()
                            .color(egui::Color32::from_rgb(255, 80, 80)),
                    );
                }
            }

            // Exit divergence for overpens
            if let Some(ref exit_div) = cmp.exit_divergence {
                if let Some(dist) = exit_div.distance {
                    let div_color = if dist < 0.5 {
                        egui::Color32::from_rgb(80, 220, 80)
                    } else if dist < 2.0 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::from_rgb(255, 80, 80)
                    };
                    ui.label(egui::RichText::new(format!("Exit divergence: {dist:.2} units")).small().color(div_color));
                } else if exit_div.sim_exit_pos.is_none() {
                    ui.label(
                        egui::RichText::new("Exit divergence: sim has no exit").small().color(egui::Color32::GRAY),
                    );
                }
            }
        } else {
            // HE/SAP outcome for non-AP shells
            let he_sap_outcome = group.shell_info.as_ref().and_then(|shell| match shell.ammo_type {
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
            if let Some((color, text)) = he_sap_outcome {
                ui.label(egui::RichText::new(&text).strong().small().color(color));
            }
        }

        ui.separator();

        // Plate-by-plate breakdown
        let last_visible = traj.last_visible_hit;

        egui::ScrollArea::vertical().id_salt("plate_detail_scroll").auto_shrink([false; 2]).show(ui, |ui| {
            for (i, hit) in result.hits.iter().enumerate() {
                let is_post_detonation = last_visible.is_some_and(|lv| i > lv);

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
                    if i == 0
                        && let Some(server_angle) = shell_entry.server_material_angle
                    {
                        ui.label(
                            egui::RichText::new(format!("(Server: {:.1}°)", server_angle.to_degrees()))
                                .small()
                                .color(egui::Color32::GRAY),
                        );
                    }
                });
                ui.label(
                    egui::RichText::new(format!("  {} / {}", hit.zone, hit.material))
                        .small()
                        .color(egui::Color32::GRAY),
                );

                // Per-plate penetration outcome (AP)
                if !is_post_detonation
                    && let Some(sim) = sim
                    && let Some(plate) = sim.plates.get(i)
                {
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
                ui.label(egui::RichText::new(t!("ui.armor.realtime.no_armor_hit").as_ref()).small().weak());
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
        self.salvo_groups.clear();
        self.salvo_group_index.clear();
        self.unmatched_counter = 0;
        self.selection = None;
        self.selection_dirty = false;
        self.processed_hit_count = 0;
        self.timeline_ingested = false;
        // Next tick() will re-ingest the pre-computed timeline and/or call process_new_shot_hits().
    }
}

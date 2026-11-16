//! Shells: the hits a replay hands over, the comparison of each with this app's own ballistics,
//! the arcs cast by hand, and splash bursts.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use rust_i18n::t;
use wows_replay_insights::hull_impact::LoggedHit;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wows_toolkit_armor::length_marks::Reading;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::splash;
use wows_toolkit_armor::trajectory;
use wows_toolkit_armor::trajectory::ArcColor;
use wows_toolkit_armor::trajectory::ArcId;
use wows_toolkit_armor::upload;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_armor::visibility::VisibilitySnapshot;
use wows_toolkit_viewmodel::armor::arc::ArcPlate;
use wows_toolkit_viewmodel::armor::arc::ComparisonVerdict;
use wows_toolkit_viewmodel::armor::arc::PlateKeyParts;
use wows_toolkit_viewmodel::armor::arc::Strike;
use wows_toolkit_viewmodel::armor::arc::ZoneShape;
use wows_toolkit_viewmodel::armor::arc::compare_with_server;
use wows_toolkit_viewmodel::armor::arc::describe_arc;
use wows_toolkit_viewmodel::armor::arc::isolate;
use wows_toolkit_viewmodel::armor::incoming::SalvoKey;
use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wows_toolkit_viewmodel::armor::penetration::Ifhe;
use wows_toolkit_viewmodel::armor::plate_detail::ArcDetail;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::Radians;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::models::geometry::SplashBox;

use super::depth_cues::landed;
use super::model::Burst;
use super::model::CastArc;
use super::model::HitPulse;
use super::model::IncomingCast;
use super::model::IncomingPath;
use super::model::IncomingSelection;
use super::model::PendingComparison;
use super::model::ViewportModel;
use super::state::ArmorViewportState;
use super::state::ViewCx;
use super::types::ArcSummary;
use super::types::CastReading;
use super::types::CastShell;
use super::types::GapState;
use super::types::IncomingPick;
use super::types::IncomingShellDetail;
use super::types::Isolate;
use super::types::SimAgreement;
use super::types::SplashBoxName;
use super::types::SplashState;
use super::types::TrajectoryState;
use super::types::ViewportChange;
use super::types::agreement;

/// How far outside the hull a cast ray starts, in model units, so it crosses every plate on the
/// way in rather than only those past the point clicked.
const CAST_STANDOFF: f32 = 50.0;

/// How wide a ship is assumed to be before its bounds are known, so an arc drawn early has a
/// length rather than none.
const DEFAULT_MODEL_EXTENT: f32 = 20.0;

/// How many lines of the shot log are shown at once.
const SHOT_LOG_LINES: usize = 12;

/// Why a Shift-click cast no shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CastMiss {
    /// No armor plate is drawn under the pointer.
    NothingUnderPointer,
    /// The plates have no thickness to fire against: a model no ship gives armor values for.
    ThicknessUnknown,
    /// The shell's path, which is not the line of sight, crosses no plate.
    NoPlateOnPath,
}

/// A landed shell cast back along the way it came: the plates its ray crossed, in strike order,
/// and the flight that brought it.
struct LandedCast {
    params: ShellParams,
    impact: ImpactResult,
    shell_dir: Vec3,
    hits: Vec<trajectory::TrajectoryHit>,
    range: Km,
}

impl ViewportModel {
    /// Draws what the ship had taken, replacing whatever was drawn before.
    ///
    /// A step backwards in playback cannot be undrawn hit by hit, so a viewport hands over the
    /// whole set each time rather than what changed.
    pub(super) fn show_hits(&mut self, hits: Vec<PreExtractedHit>, cx: &mut ViewCx) {
        self.hits = hits;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Draws what the ship had taken as playback moves, sending a ring out from each hit that has
    /// landed since the last call.
    pub(super) fn follow_hits(&mut self, hits: Vec<PreExtractedHit>, cx: &mut ViewCx) {
        /// A seek forward lands many hits at once; rings for all of them would cover the hull, so
        /// only the latest few get one.
        const MOST_RINGS: usize = 4;

        if let Some(armor) = self.current_armor.as_ref().filter(|_| !cx.reduce_motion()) {
            let center = armor.center();
            let center = [center.x, center.y, center.z];
            let bounds = armor.bounds;
            let bounds = Some(([bounds.0.x, bounds.0.y, bounds.0.z], [bounds.1.x, bounds.1.y, bounds.1.z]));
            let landed = landed(&self.hits, &hits, |hit| hit.clock);
            let started = Instant::now();
            let rings = landed
                .iter()
                .rev()
                .filter_map(|hit| {
                    let at = wows_replay_insights::hull_impact::hull_impact(&hit.hit, center, bounds)?;
                    Some(HitPulse { at: Vec3::new(at[0], at[1], at[2]), started })
                })
                .take(MOST_RINGS);
            self.hit_pulses.extend(rings);
        }

        self.show_hits(hits, cx);
    }

    /// What this app's own simulation made of a landed shell.
    ///
    /// Absent for a shell that was not simulated: one fired with something other than
    /// armour-piercing, one whose salvo this build cannot name the shell of, one whose victim's
    /// pose was not held at that moment, and one whose server outcome the simulation does not
    /// model.
    pub(super) fn sim_agreement(&self, shot: ShotId) -> Option<&SimAgreement> {
        self.sim_verdicts.get(&shot)
    }

    /// How many armour-piercing shells the comparison could not answer for.
    pub(super) fn sim_unchecked(&self) -> usize {
        self.sim_unchecked
    }

    /// Sends each landed shell through this hull and compares the result with what the server said
    /// the shell did.
    ///
    /// The reading beside each shell in the incoming-fire log, which is how a disagreement between
    /// this app's ballistics and the game's is found. Run over the log rather than over what is
    /// drawn, so a shell need not be picked to be read. Asked for before the armor is on the
    /// device, it is kept and run once it is, since the cast needs the uploaded plates.
    pub(super) fn compare_incoming(
        &mut self,
        taken: &[PreExtractedHit],
        shells: &HashMap<GameParamId, ShellInfo>,
        cx: &mut ViewCx,
    ) {
        let had = !self.sim_verdicts.is_empty() || self.sim_unchecked > 0 || self.incoming_selection.is_some();
        self.sim_verdicts.clear();
        self.sim_unchecked = 0;
        // A pick names a shell of the log being replaced.
        self.incoming_selection = None;
        if !self.is_gpu_ready() || self.current_armor.is_none() {
            self.pending_comparison = Some(PendingComparison { taken: taken.to_vec(), shells: shells.clone() });
            if had {
                cx.notify();
            }
            return;
        }
        self.pending_comparison = None;
        if let Some(armor) = self.current_armor.clone() {
            let center = armor.center();
            for taken in taken {
                let hit = &taken.hit;
                let Some(shell) = hit.salvo.as_ref().and_then(|salvo| shells.get(&salvo.params_id)) else { continue };
                // Only armour-piercing: the plate chain simulates AP, and calling an HE hit a
                // mismatch would report on the simulation rather than on the shell.
                if shell.ammo_type != AmmoType::AP {
                    continue;
                }
                match self.compare_one(hit, shell, center) {
                    Some(verdict) => {
                        self.sim_verdicts.insert(hit.hit.shot_id, agreement(&verdict));
                    }
                    // A shell whose victim's pose was not held, whose flight does not solve, whose
                    // ray crosses no plating, or whose outcome the simulation does not model.
                    None => self.sim_unchecked += 1,
                }
            }
        }
        if had || !self.sim_verdicts.is_empty() || self.sim_unchecked > 0 {
            cx.notify();
        }
    }

    /// One landed shell, cast back along the way it came.
    fn compare_one(&self, hit: &ResolvedShotHit, shell: &ShellInfo, center: Vec3) -> Option<ComparisonVerdict> {
        let landed = self.cast_landed(hit, shell, center)?;
        let first = landed.hits.first()?;
        let strike = Strike { angle: first.angle_from_normal, thickness: first.thickness };
        let plates: Vec<ArcPlate> = landed.hits.iter().map(arc_plate).collect();

        let sim = trajectory::simulate(&landed.params, &landed.impact, &landed.hits, self.continue_on_ricochet);
        let server = ServerOutcome::from_shell_hit_type(&hit.hit.hit_type.shell_hit);
        compare_with_server(&sim, &plates, strike, &server, &landed.params)
    }

    /// The plates a landed shell crossed on its way in, in the model's own frame, with the flight
    /// that brought it. `None` for a shell whose victim's pose was not held or whose flight does
    /// not solve.
    fn cast_landed(&self, hit: &ResolvedShotHit, shell: &ShellInfo, center: Vec3) -> Option<LandedCast> {
        let arrival = wows_replay_insights::hull_impact::shell_arrival(hit)?;
        // Placed without the hull's bounds, unlike a drawn marker: holding an impact inside them
        // keeps a marker off open water, but a strike dragged to the hull's edge would be
        // simulated through plating the shell never met and reported as a disagreement.
        let at = wows_replay_insights::hull_impact::hull_impact(hit, [center.x, center.y, center.z], None)?;

        let params = ShellParams::from_shell_info(shell)?;
        // The arrival state comes from solving the shot's own flight: the terminal-ballistics
        // velocity a hit carries is post-impact.
        let impact = solve_for_range(&params, arrival.travelled)?;
        let fall = impact.impact_angle_horizontal;
        let (cos, sin) = (fall.cos(), fall.sin());
        let bearing = arrival.bearing;
        let shell_dir = Vec3::new(bearing.x * cos, -sin, bearing.z * cos).normalize();

        // Cast from outside the hull along that bearing, so the ray crosses every plate the shell
        // would have met rather than only those past the point it stopped at. Through the impact's
        // own height, since a hit up a superstructure did not cross the belt. Every plate counts,
        // drawn or not: hiding one did not stop the shell.
        let through = Vec3::new(at[0], at[1], at[2]);
        let armor = self.current_armor.as_ref()?;
        let show_zero_mm = self.show_zero_mm();
        let hits = trajectory::cast_through(
            &armor.meshes,
            through - shell_dir * CAST_STANDOFF,
            shell_dir,
            |info| show_zero_mm || !upload::is_zero_mm(info.thickness_mm),
            |at| at,
        );

        Some(LandedCast { params, impact, shell_dir, hits, range: arrival.travelled.to_km() })
    }

    /// Picks landed shells in the incoming-fire log and casts each back through the hull, or puts
    /// the pick down when it is the one held.
    ///
    /// `taken` is the log's hits, from which the picked salvo's shells are read, and `shells`
    /// names the shell each salvo was fired with.
    pub(super) fn toggle_incoming(
        &mut self,
        pick: IncomingPick,
        taken: &[PreExtractedHit],
        shells: &HashMap<GameParamId, ShellInfo>,
        cx: &mut ViewCx,
    ) {
        let held = self.incoming_pick() == Some(pick);
        self.incoming_selection = None;
        cx.notify();
        if held {
            return;
        }
        let Some(armor) = self.current_armor.clone() else { return };

        let center = armor.center();
        let casts = taken
            .iter()
            .map(|taken| &taken.hit)
            .filter(|hit| {
                let Some(salvo) = hit.salvo.as_ref() else { return false };
                let key = SalvoKey { owner: salvo.owner_id, salvo_id: salvo.salvo_id };
                match pick {
                    IncomingPick::Salvo(picked) => key == picked,
                    IncomingPick::Shell { salvo: picked, shot } => key == picked && hit.hit.shot_id == shot,
                }
            })
            .map(|hit| {
                let shell = hit.salvo.as_ref().and_then(|salvo| shells.get(&salvo.params_id)).cloned();
                let path = shell.as_ref().and_then(|shell| self.incoming_path(hit, shell, center));
                let server_angle = hit
                    .hit
                    .terminal_ballistics
                    .as_ref()
                    .map(|ballistics| Radians::from(ballistics.material_angle).to_degrees());
                IncomingCast {
                    shot: hit.hit.shot_id,
                    shell,
                    server: ServerOutcome::from_shell_hit_type(&hit.hit.hit_type.shell_hit),
                    server_angle,
                    path,
                }
            })
            .collect();
        self.incoming_selection = Some(IncomingSelection { pick, armor, casts });
    }

    /// A landed shell's path through the hull, laid out as a cast arc is.
    fn incoming_path(&self, hit: &ResolvedShotHit, shell: &ShellInfo, center: Vec3) -> Option<IncomingPath> {
        let landed = self.cast_landed(hit, shell, center)?;

        Some(self.lay_incoming_path(landed.hits, landed.shell_dir, shell, &landed.params, &landed.impact, landed.range))
    }

    /// The path of `shell` through `hits`, simulated with the ricochet setting as it stands.
    fn lay_incoming_path(
        &self,
        hits: Vec<trajectory::TrajectoryHit>,
        shell_dir: Vec3,
        shell: &ShellInfo,
        params: &ShellParams,
        impact: &ImpactResult,
        range: Km,
    ) -> IncomingPath {
        let fired = Some((params, impact));
        let mut trajectory =
            trajectory::cast(hits, shell_dir, fired, self.model_extent(), self.continue_on_ricochet, range);
        // The plate chain models armour-piercing shells; an HE or SAP shell's path is drawn through
        // every plate the ray crossed, as a cast with no shell is, and read flat in the detail.
        if shell.ammo_type != AmmoType::AP {
            trajectory.sim = None;
            trajectory.detonation = None;
        }
        let drawing = trajectory::drawing(&trajectory);

        IncomingPath { trajectory, drawing }
    }

    /// Simulates the picked landed shells again over the plates they crossed, after how a
    /// ricochet is followed changed.
    fn resimulate_incoming(&mut self) {
        let Some(mut selection) = self.incoming_selection.take() else { return };
        for cast in &mut selection.casts {
            let (Some(shell), Some(path)) = (cast.shell.as_ref(), cast.path.take()) else { continue };
            let held = path.trajectory;
            let solved = ShellParams::from_shell_info(shell)
                .and_then(|params| Some((solve_for_range(&params, held.range.to_meters())?, params)));
            cast.path = match solved {
                Some((impact, params)) => {
                    Some(self.lay_incoming_path(held.hits, held.shell_dir, shell, &params, &impact, held.range))
                }
                // It solved when it was picked, with the same shell and range.
                None => Some(IncomingPath { drawing: trajectory::drawing(&held), trajectory: held }),
            };
        }
        self.incoming_selection = Some(selection);
    }

    /// The selection on the scene shown. One made on another scene is of no use here.
    pub(super) fn incoming_selection(&self) -> Option<&IncomingSelection> {
        let shown = self.current_armor.as_ref()?;

        self.incoming_selection.as_ref().filter(|selection| Arc::ptr_eq(&selection.armor, shown))
    }

    /// What the incoming-fire log has picked.
    pub(super) fn incoming_pick(&self) -> Option<IncomingPick> {
        self.incoming_selection().map(|selection| selection.pick)
    }

    /// The picked shell as its detail shows it. `None` unless one shell is picked.
    pub(super) fn incoming_detail(&self) -> Option<IncomingShellDetail> {
        let selection = self.incoming_selection()?;
        let IncomingPick::Shell { shot, .. } = selection.pick else { return None };
        let cast = selection.casts.iter().find(|cast| cast.shot == shot)?;

        let reading = cast.path.as_ref().map(|path| {
            let trajectory = &path.trajectory;
            let plates: Vec<ArcPlate> = trajectory.hits.iter().map(arc_plate).collect();
            let ammo = cast.shell.as_ref().map(|shell| &shell.ammo_type);
            CastReading {
                outcome: describe_arc(trajectory.sim.as_ref(), &plates),
                detail: ArcDetail::new(ammo, trajectory.impact.as_ref(), trajectory.sim.as_ref(), &plates),
            }
        });

        Some(IncomingShellDetail {
            shell: cast.shell.clone(),
            server: cast.server.clone(),
            server_angle: cast.server_angle,
            agreement: self.sim_verdicts.get(&shot).cloned(),
            cast: reading,
        })
    }

    /// How many hits are drawn on the hull.
    pub(super) fn hits_drawn(&self) -> usize {
        self.hits.len()
    }

    /// Says what the ship's health was at the moment being shown.
    ///
    /// Set apart from the hits because it changes without them: a fire or a flood takes health
    /// off between one shell and the next.
    pub(super) fn set_hit_health(&mut self, health: Option<f32>, cx: &mut ViewCx) {
        if self.hit_health == health {
            return;
        }
        self.hit_health = health;
        cx.notify();
    }

    /// The shot log beside the hull: what landed, newest first.
    ///
    /// Bounded, because a battleship's worth of secondaries is hundreds of lines and a log that
    /// long is not read, it is scrolled past.
    pub(super) fn shot_log(&self) -> Vec<LoggedHit> {
        self.hits.iter().rev().take(SHOT_LOG_LINES).map(wows_replay_insights::hull_impact::log_line).collect()
    }

    /// What a replay viewer reads beside the hull: how much of the ship was left, and how much of
    /// it had been hit.
    pub(super) fn hit_readout(&self) -> Option<String> {
        let health = self.hit_health?;
        Some(t!("ui.armor.realtime_readout", health = format!("{health:.0}"), hits = self.hits.len()).into_owned())
    }

    /// The shown model's overall length and beam, while the length marks are.
    pub(super) fn length_readout(&self) -> Option<String> {
        let marks = self.length_marks.as_ref().filter(|_| self.display_settings.show_length_marks)?.marks();
        let (length, beam) = (Reading::of_size(marks.length()).text(), Reading::of_size(marks.beam()).text());

        Some(t!("ui.armor.length_readout", length = length, beam = beam).into_owned())
    }

    /// Whether the openings in the armor are marked, and how many there were.
    pub(super) fn gaps(&self) -> GapState {
        GapState { shown: self.show_gaps, count: self.gap_count }
    }

    /// How many shells have been cast, and whether a ricochet is followed.
    pub(super) fn trajectory_state(&self) -> TrajectoryState {
        TrajectoryState { count: self.arcs.len(), continue_on_ricochet: self.continue_on_ricochet }
    }

    /// Adopts the ship and shell the next cast is fired with.
    ///
    /// An arc cast before any ship was chosen takes it, since it was waiting for one; an arc
    /// already fired with a shell keeps its own.
    pub(super) fn set_cast_shell(&mut self, shell: Option<CastShell>, cx: &mut ViewCx) {
        let unchanged = match (&self.cast_shell, &shell) {
            (Some(held), Some(next)) => held.is_same(next),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }

        self.cast_shell = shell;
        if let Some(shell) = self.cast_shell.clone() {
            let waiting: Vec<ArcId> = self.arcs.iter().filter(|arc| arc.shell.is_none()).map(|arc| arc.id).collect();
            for id in waiting {
                self.set_arc_shell(id, shell.clone(), cx);
            }
        }
        cx.notify();
    }

    /// The range the next cast is fired from.
    pub(super) fn cast_range(&self) -> Km {
        self.cast_range
    }

    /// Sets the range the next cast is fired from. Arcs already cast keep their own; each is
    /// edited from its marker.
    pub(super) fn set_cast_range(&mut self, range: Km, cx: &mut ViewCx) {
        self.cast_range = range;
        cx.notify();
    }

    /// Whether a click bursts a shell, whether this ship has boxes to burst against, and what the
    /// last burst reached.
    pub(super) fn splash_state(&self) -> SplashState {
        SplashState {
            shown: self.splash_mode,
            boxes_shown: self.show_splash_boxes,
            has_data: self.current_armor.as_ref().is_some_and(|armor| armor.splash_data.is_some()),
            zones_reached: self.burst.as_ref().map(|burst| burst.result.hit_zones.len()).unwrap_or(0),
        }
    }

    /// Takes whether the checker counts Inertia Fuse for HE Shells, and shades the burst's plates
    /// again by it.
    pub(super) fn set_burst_ifhe(&mut self, ifhe: Ifhe, cx: &mut ViewCx) {
        if self.burst_ifhe == ifhe {
            return;
        }
        self.burst_ifhe = ifhe;
        if self.burst.is_some() {
            self.reupload_current_armor(cx);
            cx.notify();
        }
    }

    /// Turns splash bursting on or off.
    pub(super) fn set_splash_mode(&mut self, on: bool, cx: &mut ViewCx) {
        if self.splash_mode == on {
            return;
        }
        self.splash_mode = on;
        cx.notify();
    }

    /// Shows or hides the outlines of the ship's named splash boxes. A preference, so the pane
    /// reports it to be saved. Turned on with no box ticked, every box is, as a load does.
    pub(super) fn set_show_splash_boxes(&mut self, show: bool, cx: &mut ViewCx) {
        if self.show_splash_boxes == show {
            return;
        }
        if show
            && self.splash_boxes_checked.is_empty()
            && let Some(armor) = self.shown().cloned()
        {
            self.check_every_splash_box(&armor);
        }
        self.apply_splash_boxes_shown(show, cx);
    }

    /// The boxes ticked in the splash zone list.
    pub fn splash_boxes_checked(&self) -> &HashSet<SplashBoxName> {
        &self.splash_boxes_checked
    }

    /// Which groups of the splash zone list are expanded.
    pub fn expanded_splash_groups(&self) -> &HashSet<String> {
        &self.expanded_splash_groups
    }

    /// The boxes of `armor` outlined: the ticked ones, while the outlines are shown.
    pub(super) fn drawn_splash_boxes<'a>(&self, armor: &'a LoadedShipArmor) -> Vec<&'a SplashBox> {
        let Some(data) = armor.splash_data.as_ref().filter(|_| self.show_splash_boxes) else { return Vec::new() };

        data.boxes.iter().filter(|shape| self.splash_boxes_checked.contains(shape.name.as_str())).collect()
    }

    /// Ticks every box `armor` has.
    pub(super) fn check_every_splash_box(&mut self, armor: &LoadedShipArmor) {
        let Some(data) = armor.splash_data.as_ref() else { return };

        self.splash_boxes_checked = data.boxes.iter().map(|shape| SplashBoxName::new(shape.name.clone())).collect();
    }

    /// Ticks or unticks every box, turning the outlines on or off with them, as the list's All
    /// and None do.
    pub(super) fn set_every_splash_box_checked(&mut self, checked: bool, cx: &mut ViewCx) {
        self.splash_boxes_checked.clear();
        if checked && let Some(armor) = self.shown().cloned() {
            self.check_every_splash_box(&armor);
        }
        self.apply_splash_boxes_shown(checked, cx);
    }

    /// Ticks or unticks `names`. The outlines are on while any box is ticked.
    pub(super) fn set_splash_boxes_checked(&mut self, names: Vec<SplashBoxName>, checked: bool, cx: &mut ViewCx) {
        for name in names {
            if checked {
                self.splash_boxes_checked.insert(name);
            } else {
                self.splash_boxes_checked.remove(&name);
            }
        }

        let any_checked = !self.splash_boxes_checked.is_empty();
        self.apply_splash_boxes_shown(any_checked, cx);
    }

    pub(super) fn toggle_splash_group_expanded(&mut self, group: String, cx: &mut ViewCx) {
        if !self.expanded_splash_groups.remove(&group) {
            self.expanded_splash_groups.insert(group);
        }
        cx.notify();
    }

    /// Draws the ticked boxes again with the outlines `show`n or not. Only the switch itself is a
    /// saved preference; which boxes are ticked is the pane's own.
    fn apply_splash_boxes_shown(&mut self, show: bool, cx: &mut ViewCx) {
        let switched = self.show_splash_boxes != show;
        self.show_splash_boxes = show;
        self.reupload_current_armor(cx);
        if switched {
            cx.emit(ViewportChange::Settings);
        }
        cx.notify();
    }

    /// Carries the splash zone list over a reload that swapped `old` for `new`.
    pub(super) fn carry_splash_boxes(&mut self, old: Option<&LoadedShipArmor>, new: &LoadedShipArmor) {
        let had: HashSet<&str> = old.map(|old| splash_box_names(old).collect()).unwrap_or_default();
        self.splash_boxes_checked =
            carried_splash_ticks(&self.splash_boxes_checked, &had, splash_box_names(new), self.show_splash_boxes);
        self.expanded_splash_groups.retain(|group| new.splash_box_groups.iter().any(|(name, _)| name == group));
    }

    /// Bursts a shell where the pointer is.
    ///
    /// The burst is sized by the chosen attacker's bursting shell. Clicking past the ship clears
    /// the burst.
    pub(super) fn burst_at(&mut self, pointer: Vec2, cx: &mut ViewCx) {
        let Some(rect) = self.last_bounds else { return };
        let Some(surface) = self.viewport.pick(pointer, rect) else {
            self.burst = None;
            self.reupload_current_armor(cx);
            cx.notify();
            return;
        };

        let Some(armor) = self.current_armor.clone() else { return };
        let Some(data) = armor.splash_data.as_ref() else { return };

        let point: [f32; 3] = surface.world_position.into();
        let shell = self.cast_shell.as_ref().and_then(|cast| splash::bursting_shell(&cast.ship.shells)).cloned();
        // With no bursting shell the burst still marks the click at the smallest reach rather than
        // ignoring it; the readout says no shell sized it.
        let reach = match &shell {
            Some(shell) => splash::splash_half_extent(shell.caliber),
            None => splash::ModelUnit::new(0.0),
        };
        let result = splash::compute_splash(point, reach, data, armor.hit_locations.as_ref(), &armor.meshes);
        self.burst = Some(Burst { result, shell });
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// What the burst on screen reached, with each HE and SAP shell of `ships` held against it.
    /// `None` while there is no burst.
    pub(super) fn splash_readout(&self, ships: &[ComparisonShip], ifhe: Ifhe) -> Option<splash::SplashReadout> {
        let burst = self.burst.as_ref()?;

        Some(splash::splash_readout(&burst.result, burst.shell.as_ref(), ships, ifhe))
    }

    /// Whether a ricocheting shell is followed past the bounce.
    ///
    /// Everything already cast is fired again, since the answer changes where each of those
    /// shells ended.
    pub(super) fn set_continue_on_ricochet(&mut self, on: bool, cx: &mut ViewCx) {
        if self.continue_on_ricochet == on {
            return;
        }
        self.continue_on_ricochet = on;
        self.recast_arcs(cx);
        self.resimulate_incoming();
    }

    /// Casts every arc again at its own range and with its own shell, as the plates under it
    /// moved: the hull was rolled, or how a shell is followed changed.
    pub(super) fn recast_arcs(&mut self, cx: &mut ViewCx) {
        let recast: Vec<trajectory::Trajectory> =
            self.arcs.iter().map(|arc| self.refire(arc, arc.trajectory.range, arc.shell.as_ref())).collect();
        for (arc, trajectory) in self.arcs.iter_mut().zip(recast) {
            arc.set_trajectory(trajectory);
        }
        self.follow_arc_isolation(cx);
        cx.notify();
    }

    /// Drops every cast shell.
    pub(super) fn clear_trajectories(&mut self, cx: &mut ViewCx) {
        if self.arcs.is_empty() {
            return;
        }
        let was_isolating =
            self.arcs.iter().any(|arc| arc.trajectory.isolating_plates || arc.trajectory.isolating_zones);
        self.arcs.clear();
        self.selected_arc = None;
        // What the arcs were hiding is given back with them, rather than leaving most of the ship
        // gone with nothing on screen saying why.
        if was_isolating {
            self.apply_arc_isolation(cx);
            return;
        }
        cx.notify();
    }

    /// Marks the openings in the armor, or stops marking them.
    pub(super) fn set_show_gaps(&mut self, show: bool, cx: &mut ViewCx) {
        if self.show_gaps == show {
            return;
        }
        self.show_gaps = show;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Whether only the plates the game's own viewer hides are being shown.
    pub(super) fn show_hidden_only(&self) -> bool {
        self.show_hidden_only
    }

    /// Shows only the plates the game's own viewer hides, or everything again. Changes which
    /// triangles are uploaded, so the armor is rebuilt.
    pub(super) fn set_show_hidden_only(&mut self, hidden_only: bool, cx: &mut ViewCx) {
        if self.show_hidden_only == hidden_only {
            return;
        }
        self.show_hidden_only = hidden_only;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Whether a Shift-click on the scene shown can cast: it has plates, and they have a thickness
    /// to fire against.
    pub(super) fn can_cast(&self) -> bool {
        let armored = self.scene_features().is_some_and(|features| features.armor);

        armored && !self.shown().is_some_and(|armor| armor.thickness_unknown())
    }

    /// Casts a shell at whatever the pointer is over, adds it to the arcs already cast, and
    /// selects it. Says why nothing was cast when the pointer was not over armor a shell can be
    /// fired at.
    ///
    /// The shell comes in along the camera's own bearing, flattened: a trajectory is read against
    /// the ship, and firing from wherever the eye happens to sit vertically would say more about
    /// the camera than the armor.
    pub(super) fn cast_at(&mut self, pointer: Vec2, cx: &mut ViewCx) -> Result<ArcId, CastMiss> {
        // Said wherever the click lands: no plate of such a model can be cast at, shown or not.
        if self.current_armor.as_ref().is_some_and(|armor| armor.thickness_unknown()) {
            return Err(CastMiss::ThicknessUnknown);
        }
        // Before the first frame nothing is drawn under the pointer to be picked.
        let Some(rect) = self.last_bounds else { return Err(CastMiss::NothingUnderPointer) };
        let Some(surface) = self.viewport.pick(pointer, rect) else { return Err(CastMiss::NothingUnderPointer) };
        let Some((_, ray)) = self.viewport.screen_to_ray(pointer, rect) else {
            return Err(CastMiss::NothingUnderPointer);
        };
        let approach = trajectory::approach_xz(&ray);
        let aim = surface.world_position;
        let trajectory = self.fire_at(aim, approach).ok_or(CastMiss::NoPlateOnPath)?;

        Ok(self.add_arc(aim, approach, trajectory, cx))
    }

    /// Keeps a shell cast at `aim` on the bearing `approach`, fired with the next cast's ship and
    /// shell, in a colour no other arc holds, and selects it: the arc just cast is the one the
    /// reader means to look at.
    pub(super) fn add_arc(
        &mut self,
        aim: Vec3,
        approach: Vec3,
        trajectory: trajectory::Trajectory,
        cx: &mut ViewCx,
    ) -> ArcId {
        let id = self.next_arc;
        self.next_arc = id.next();
        let color = ArcColor::unused(self.arcs.iter().map(|arc| arc.color));
        let drawing = trajectory::drawing(&trajectory);
        let (aim, approach) = self.viewport.ray_to_model_space(aim, approach);
        let shell = self.cast_shell.clone();
        self.arcs.push(CastArc { id, color, shell, aim, approach, trajectory, drawing });
        self.selected_arc = Some(id);
        cx.notify();

        id
    }

    /// A shell fired with the next cast's settings at `aim`, coming in on `approach`. `None` when
    /// the ray crosses no plate anything is known about.
    fn fire_at(&self, aim: Vec3, approach: Vec3) -> Option<trajectory::Trajectory> {
        let fired = self.cast_shell.as_ref().and_then(|shell| shell.fire(self.cast_range));
        let shell_dir = fall_direction(approach, fired.as_ref().map(|(_, impact)| impact));

        // Cast from outside the hull so the ray crosses every plate on the way in, not just those
        // past the point clicked.
        let origin = aim - shell_dir * CAST_STANDOFF;
        let ray_hits = self.viewport.pick_all_ray(origin, shell_dir);
        let hits = trajectory::build_hits(&ray_hits, &self.mesh_triangle_info, &shell_dir);
        if hits.is_empty() {
            return None;
        }

        let shell = fired.as_ref().map(|(params, impact)| (params, impact));
        Some(trajectory::cast(hits, shell_dir, shell, self.model_extent(), self.continue_on_ricochet, self.cast_range))
    }

    /// `arc` cast again from `range` with `shell`, at the point it was aimed at, keeping what it
    /// isolates.
    ///
    /// The aim and bearing are held in model space, so they turn with the hull; the fall is
    /// struck against the world, since gravity does not roll with the ship. A new fall finds its
    /// own plates, so the ray is cast again rather than the first cast's plates struck anew.
    fn refire(&self, arc: &CastArc, range: Km, shell: Option<&CastShell>) -> trajectory::Trajectory {
        let fired = shell.and_then(|shell| shell.fire(range));
        let approach = trajectory::approach_xz(&self.viewport.pos_to_world_space(arc.approach));
        let shell_dir = fall_direction(approach, fired.as_ref().map(|(_, impact)| impact));
        let aim = self.viewport.pos_to_world_space(arc.aim);
        let hits = self.cast_through_armor(aim, shell_dir);
        let shell = fired.as_ref().map(|(params, impact)| (params, impact));
        let mut refired =
            trajectory::cast(hits, shell_dir, shell, self.model_extent(), self.continue_on_ricochet, range);
        refired.isolating_plates = arc.trajectory.isolating_plates;
        refired.isolating_zones = arc.trajectory.isolating_zones;
        refired
    }

    /// Every plate of the shown armor a shell aimed at `aim` along `shell_dir` crosses, from
    /// outside the hull. Drawn or not, so hiding a plate does not let a shell through it; a plate
    /// with no thickness counts only while such plates are drawn.
    fn cast_through_armor(&self, aim: Vec3, shell_dir: Vec3) -> Vec<trajectory::TrajectoryHit> {
        let Some(armor) = self.current_armor.as_ref() else { return Vec::new() };

        let show_zero_mm = self.show_zero_mm();
        let (origin, direction) = self.viewport.ray_to_model_space(aim - shell_dir * CAST_STANDOFF, shell_dir);
        trajectory::cast_through(
            &armor.meshes,
            origin,
            direction,
            |info| show_zero_mm || !upload::is_zero_mm(info.thickness_mm),
            |at| self.viewport.pos_to_world_space(at),
        )
    }

    /// Every cast arc, as the trajectory section lists them.
    pub(super) fn arcs(&self) -> Vec<ArcSummary> {
        self.arcs.iter().map(arc_summary).collect()
    }

    /// One arc, and where it sits among the arcs, which is the number its marker carries.
    pub(super) fn arc(&self, id: ArcId) -> Option<(usize, ArcSummary)> {
        let index = self.arcs.iter().position(|arc| arc.id == id)?;

        Some((index, arc_summary(&self.arcs[index])))
    }

    /// The arc being edited, and the range it is fired from.
    pub(super) fn selected_arc(&self) -> Option<(ArcId, Km)> {
        let id = self.selected_arc?;
        let arc = self.arcs.iter().find(|arc| arc.id == id)?;
        Some((id, arc.trajectory.range))
    }

    /// Selects an arc, or deselects it when it is the one selected.
    pub(super) fn toggle_arc_selection(&mut self, id: ArcId, cx: &mut ViewCx) {
        let open = self.selected_arc == Some(id);
        self.selected_arc = if open { None } else { self.arcs.iter().any(|arc| arc.id == id).then_some(id) };
        cx.notify();
    }

    /// Drops one arc.
    pub(super) fn remove_arc(&mut self, id: ArcId, cx: &mut ViewCx) {
        let Some(index) = self.arcs.iter().position(|arc| arc.id == id) else { return };
        let dropped = self.arcs.remove(index);
        if self.selected_arc == Some(id) {
            self.selected_arc = None;
        }

        // Only what it was hiding has to be given back; an arc that hid nothing leaves the
        // reader's own hiding where it is.
        if dropped.trajectory.isolating_plates || dropped.trajectory.isolating_zones {
            self.apply_arc_isolation(cx);
            return;
        }
        cx.notify();
    }

    /// Fires one arc from a different range, leaving the others where they are. A steeper or
    /// flatter fall crosses different plates, so what it isolates follows them.
    pub(super) fn set_arc_range(&mut self, id: ArcId, range: Km, cx: &mut ViewCx) {
        let Some(index) = self.arcs.iter().position(|arc| arc.id == id) else { return };
        let held = &self.arcs[index];
        if (held.trajectory.range.value() - range.value()).abs() < f32::EPSILON {
            return;
        }

        let refired = self.refire(held, range, held.shell.as_ref());
        self.arcs[index].set_trajectory(refired);
        self.follow_arc_isolation(cx);
        cx.notify();
    }

    /// Fires one arc with a different ship's shell, or another of its own.
    pub(super) fn set_arc_shell(&mut self, id: ArcId, shell: CastShell, cx: &mut ViewCx) {
        let Some(index) = self.arcs.iter().position(|arc| arc.id == id) else { return };
        let held = &self.arcs[index];

        let refired = self.refire(held, held.trajectory.range, Some(&shell));
        self.arcs[index].set_trajectory(refired);
        self.arcs[index].shell = Some(shell);
        self.follow_arc_isolation(cx);
        cx.notify();
    }

    /// Hides what one arc did not cross, or stops hiding it.
    ///
    /// Plates and zones are two answers to one question, so asking for either withdraws the
    /// other.
    pub(super) fn isolate_arc(&mut self, id: ArcId, what: Isolate, on: bool, cx: &mut ViewCx) {
        let Some(arc) = self.arcs.iter_mut().find(|arc| arc.id == id) else { return };
        let cast = &mut arc.trajectory;
        match what {
            Isolate::Plates => {
                cast.isolating_plates = on;
                cast.isolating_zones = false;
            }
            Isolate::Zones => {
                cast.isolating_zones = on;
                cast.isolating_plates = false;
            }
        }
        self.apply_arc_isolation(cx);
    }

    /// The same for every arc at once, which is what reads out the whole cast.
    pub(super) fn isolate_all_arcs(&mut self, what: Isolate, cx: &mut ViewCx) {
        for arc in &mut self.arcs {
            arc.trajectory.isolating_plates = what == Isolate::Plates;
            arc.trajectory.isolating_zones = what == Isolate::Zones;
        }
        self.apply_arc_isolation(cx);
    }

    /// Rebuilds the visibility the arcs' isolation asks for, as one undoable step.
    ///
    /// The whole visibility is recomputed rather than adjusted, because an arc that stops
    /// isolating has to give back exactly what it hid and nothing else. With no arc isolating
    /// anything, the ship goes back to whole.
    pub(super) fn apply_arc_isolation(&mut self, cx: &mut ViewCx) {
        self.undo_stack.push(VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        });
        self.isolate_by_arcs(cx);
        // A change of visibility like any other: saved, and followed by a synced pane.
        cx.emit(ViewportChange::Settings);
    }

    /// Keeps what the arcs isolate in step with the plates they cross after they were cast again.
    /// Not an undo step: the reader moved a slider, and did not hide anything. What redo held no
    /// longer follows from what is hidden now, so it goes.
    fn follow_arc_isolation(&mut self, cx: &mut ViewCx) {
        if self.arcs.iter().any(|arc| arc.trajectory.isolating_plates || arc.trajectory.isolating_zones) {
            self.undo_stack.forget_redo();
            self.isolate_by_arcs(cx);
            // Followed by a synced pane, but not saved on every step of the slider.
            cx.emit(ViewportChange::Synced);
        }
    }

    fn isolate_by_arcs(&mut self, cx: &mut ViewCx) {
        let mut hit_zones: HashSet<String> = HashSet::new();
        let mut hit_plates: HashSet<PlateKeyParts> = HashSet::new();
        for cast in self.arcs.iter().map(|arc| &arc.trajectory) {
            for hit in &cast.hits {
                if cast.isolating_zones {
                    hit_zones.insert(hit.zone.clone());
                }
                if cast.isolating_plates {
                    hit_plates.insert((
                        hit.zone.clone(),
                        hit.material.name().to_owned(),
                        (hit.thickness.value() * 10.0).round() as i32,
                    ));
                }
            }
        }

        self.part_visibility.clear();
        self.mesh_seed.armor = false;
        self.plate_visibility.clear();
        if let Some(armor) = self.current_armor.clone() {
            let zones: Vec<ZoneShape<'_>> = armor
                .zone_part_plates
                .iter()
                .map(|zone| ZoneShape {
                    name: zone.name.as_str(),
                    parts: zone.parts.iter().map(|part| (part.name.as_str(), part.plates.as_slice())).collect(),
                })
                .collect();
            let isolation = isolate(&zones, &hit_zones, &hit_plates);
            self.part_visibility = isolation.parts;
            // The shared map says whether a plate is shown; this one says whether it is hidden, so
            // the sense is turned over on the way in.
            self.plate_visibility = isolation
                .plates
                .into_iter()
                .map(|((zone, material_name, thickness_tenths), shown)| {
                    (PlateKey { zone, material_name, thickness_tenths }, !shown)
                })
                .collect();
        }
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// How wide the ship is, which sets how long an incoming arc is drawn.
    ///
    /// A ship whose bounds are not known yet gets a default rather than a zero-length arc.
    pub(super) fn model_extent(&self) -> f32 {
        match self.model_bounds {
            Some((min, max)) => (max[0] - min[0]).max(max[2] - min[2]),
            None => DEFAULT_MODEL_EXTENT,
        }
    }
}

/// The direction a shell travels, given the bearing it comes in on.
///
/// With a shell to fire the fall angle is the solver's; without one the shell comes in flat, which
/// is what the lead-in line then shows.
fn fall_direction(approach: Vec3, impact: Option<&ImpactResult>) -> Vec3 {
    match impact {
        Some(impact) => {
            let fall = impact.impact_angle_horizontal.value().to_radians();
            Vec3::new(approach[0] * fall.cos(), -fall.sin(), approach[2] * fall.cos()).normalize()
        }
        None => approach,
    }
}

/// A plate a cast crossed, as the outcome and the plate list read it.
fn arc_plate(hit: &trajectory::TrajectoryHit) -> ArcPlate {
    ArcPlate { zone: hit.zone.clone(), thickness: hit.thickness, material: hit.material, angle: hit.angle_from_normal }
}

/// A cast arc, as the trajectory section shows it.
fn arc_summary(arc: &CastArc) -> ArcSummary {
    let cast = &arc.trajectory;
    let plates: Vec<ArcPlate> = cast.hits.iter().map(arc_plate).collect();
    let ammo = arc.shell.as_ref().and_then(CastShell::shell).map(|shell| &shell.ammo_type);

    ArcSummary {
        id: arc.id,
        color: arc.color,
        hits: cast.hits.len(),
        total_armor: Millimeters::from(cast.hits.iter().map(|hit| hit.thickness.value()).sum::<f32>()),
        range: cast.range,
        shell: arc.shell.clone(),
        isolating_plates: cast.isolating_plates,
        isolating_zones: cast.isolating_zones,
        outcome: describe_arc(cast.sim.as_ref(), &plates),
        detail: ArcDetail::new(ammo, cast.impact.as_ref(), cast.sim.as_ref(), &plates),
    }
}

/// The names of `armor`'s splash boxes, in the splash file's order.
fn splash_box_names(armor: &LoadedShipArmor) -> impl Iterator<Item = &str> {
    armor.splash_data.iter().flat_map(|data| &data.boxes).map(|shape| shape.name.as_str())
}

/// The boxes ticked once a reload swaps a hull with the boxes `had` for one with the boxes `has`:
/// a box both have keeps its tick, and a box only the new hull has is ticked while the outlines are
/// `shown`, as a load ticks every box.
fn carried_splash_ticks<'a>(
    checked: &HashSet<SplashBoxName>,
    had: &HashSet<&str>,
    has: impl Iterator<Item = &'a str>,
    shown: bool,
) -> HashSet<SplashBoxName> {
    has.filter(|name| if had.contains(name) { checked.contains(*name) } else { shown })
        .map(SplashBoxName::new)
        .collect()
}

impl ArmorViewportState {
    /// Draws what the ship had taken, replacing whatever was drawn before.
    pub fn show_hits(self, hits: Vec<PreExtractedHit>) {
        self.update(|this, cx| this.show_hits(hits, cx));
    }

    /// Draws what the ship had taken as playback moves, with a ring from each new hit.
    pub fn follow_hits(self, hits: Vec<PreExtractedHit>) {
        self.update(|this, cx| this.follow_hits(hits, cx));
    }

    pub fn set_hit_health(self, health: Option<f32>) {
        self.update(|this, cx| this.set_hit_health(health, cx));
    }

    /// Compares each landed shell with this app's own ballistics.
    pub fn compare_incoming(self, taken: &[PreExtractedHit], shells: &HashMap<GameParamId, ShellInfo>) {
        self.update(|this, cx| this.compare_incoming(taken, shells, cx));
    }

    pub fn sim_agreement(self, shot: ShotId) -> Option<SimAgreement> {
        self.read(|this| this.sim_agreement(shot).cloned()).flatten()
    }

    pub fn sim_unchecked(self) -> usize {
        self.read(|this| this.sim_unchecked()).unwrap_or_default()
    }

    /// Casts the picked landed shells back through the hull, or puts the pick down when it is the
    /// one held.
    pub fn toggle_incoming(
        self,
        pick: IncomingPick,
        taken: &[PreExtractedHit],
        shells: &HashMap<GameParamId, ShellInfo>,
    ) {
        self.update(|this, cx| this.toggle_incoming(pick, taken, shells, cx));
    }

    pub fn incoming_pick(self) -> Option<IncomingPick> {
        self.read(|this| this.incoming_pick()).flatten()
    }

    pub fn incoming_detail(self) -> Option<IncomingShellDetail> {
        self.read(|this| this.incoming_detail()).flatten()
    }

    pub fn hits_drawn(self) -> usize {
        self.read(|this| this.hits_drawn()).unwrap_or_default()
    }

    #[cfg(test)]
    pub fn hit_readout(self) -> Option<String> {
        self.read(|this| this.hit_readout()).flatten()
    }

    pub fn gaps(self) -> GapState {
        self.read(|this| this.gaps()).unwrap_or_default()
    }

    pub fn trajectory_state(self) -> TrajectoryState {
        self.read(|this| this.trajectory_state()).unwrap_or(TrajectoryState { count: 0, continue_on_ricochet: false })
    }

    pub fn set_cast_shell(self, shell: Option<CastShell>) {
        self.update(|this, cx| this.set_cast_shell(shell, cx));
    }

    pub fn set_burst_ifhe(self, ifhe: Ifhe) {
        self.update(|this, cx| this.set_burst_ifhe(ifhe, cx));
    }

    /// The range the next cast is fired from; the default range once the state is gone.
    pub fn cast_range(self) -> Km {
        self.read(|this| this.cast_range()).unwrap_or(super::model::DEFAULT_CAST_RANGE)
    }

    pub fn set_cast_range(self, range: Km) {
        self.update(|this, cx| this.set_cast_range(range, cx));
    }

    pub fn splash_state(self) -> SplashState {
        self.read(|this| this.splash_state()).unwrap_or(SplashState {
            shown: false,
            boxes_shown: false,
            has_data: false,
            zones_reached: 0,
        })
    }

    pub fn set_splash_mode(self, on: bool) {
        self.update(|this, cx| this.set_splash_mode(on, cx));
    }

    pub fn set_show_splash_boxes(self, show: bool) {
        self.update(|this, cx| this.set_show_splash_boxes(show, cx));
    }

    pub fn set_every_splash_box_checked(self, checked: bool) {
        self.update(|this, cx| this.set_every_splash_box_checked(checked, cx));
    }

    pub fn set_splash_boxes_checked(self, names: Vec<SplashBoxName>, checked: bool) {
        self.update(|this, cx| this.set_splash_boxes_checked(names, checked, cx));
    }

    pub fn toggle_splash_group_expanded(self, group: String) {
        self.update(|this, cx| this.toggle_splash_group_expanded(group, cx));
    }

    /// The boxes of the scene last handed to this pane that are outlined, in the splash file's
    /// order.
    #[cfg(test)]
    pub fn drawn_splash_boxes(self) -> Vec<SplashBoxName> {
        self.read_untracked(|this| {
            let Some(armor) = this.shown() else { return Vec::new() };
            this.drawn_splash_boxes(armor).iter().map(|shape| SplashBoxName::new(shape.name.clone())).collect()
        })
        .unwrap_or_default()
    }

    pub fn splash_readout(self, ships: &[ComparisonShip], ifhe: Ifhe) -> Option<splash::SplashReadout> {
        self.read(|this| this.splash_readout(ships, ifhe)).flatten()
    }

    /// Puts a burst on screen as a click would, without the device a click picks through.
    #[cfg(test)]
    pub fn set_burst(self, result: splash::SplashResult, shell: Option<ShellInfo>) {
        self.update(|this, cx| {
            this.burst = Some(Burst { result, shell });
            cx.notify();
        });
    }

    pub fn set_continue_on_ricochet(self, on: bool) {
        self.update(|this, cx| this.set_continue_on_ricochet(on, cx));
    }

    pub fn clear_trajectories(self) {
        self.update(|this, cx| this.clear_trajectories(cx));
    }

    pub fn set_show_gaps(self, show: bool) {
        self.update(|this, cx| this.set_show_gaps(show, cx));
    }

    pub fn show_hidden_only(self) -> bool {
        self.read(|this| this.show_hidden_only()).unwrap_or_default()
    }

    pub fn set_show_hidden_only(self, hidden_only: bool) {
        self.update(|this, cx| this.set_show_hidden_only(hidden_only, cx));
    }

    /// Keeps a cast shell and selects it. `None` once the state is gone.
    #[cfg(test)]
    pub fn add_arc(self, aim: Vec3, approach: Vec3, trajectory: trajectory::Trajectory) -> Option<ArcId> {
        self.update(|this, cx| this.add_arc(aim, approach, trajectory, cx))
    }

    pub fn arcs(self) -> Vec<ArcSummary> {
        self.read(|this| this.arcs()).unwrap_or_default()
    }

    /// One arc, and its place among the arcs.
    pub fn arc(self, id: ArcId) -> Option<(usize, ArcSummary)> {
        self.read(|this| this.arc(id)).flatten()
    }

    /// [`Self::arc`] without following the state.
    pub fn arc_untracked(self, id: ArcId) -> Option<(usize, ArcSummary)> {
        self.read_untracked(|this| this.arc(id)).flatten()
    }

    pub fn selected_arc(self) -> Option<(ArcId, Km)> {
        self.read(|this| this.selected_arc()).flatten()
    }

    pub fn toggle_arc_selection(self, id: ArcId) {
        self.update(|this, cx| this.toggle_arc_selection(id, cx));
    }

    pub fn remove_arc(self, id: ArcId) {
        self.update(|this, cx| this.remove_arc(id, cx));
    }

    pub fn set_arc_range(self, id: ArcId, range: Km) {
        self.update(|this, cx| this.set_arc_range(id, range, cx));
    }

    pub fn set_arc_shell(self, id: ArcId, shell: CastShell) {
        self.update(|this, cx| this.set_arc_shell(id, shell, cx));
    }

    pub fn isolate_arc(self, id: ArcId, what: Isolate, on: bool) {
        self.update(|this, cx| this.isolate_arc(id, what, on, cx));
    }

    pub fn isolate_all_arcs(self, what: Isolate) {
        self.update(|this, cx| this.isolate_all_arcs(what, cx));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use wows_replay_insights::timeline::PreExtractedHit;
    use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
    use wows_replays::analyzer::decoder::HitType;
    use wows_replays::analyzer::decoder::Recognized;
    use wows_replays::analyzer::decoder::ShotHit;
    use wows_replays::types::EntityId;
    use wows_replays::types::GameClock;
    use wows_replays::types::ShotId;
    use wows_replays::types::WorldPos;
    use wows_toolkit_armor::trajectory;
    use wows_toolkit_armor::trajectory::TrajectoryHit;
    use wows_toolkit_armor::viewport::types::Vec3;
    use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
    use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
    use wowsunpack::game_params::types::Degrees;
    use wowsunpack::game_params::types::Km;
    use wowsunpack::game_params::types::Millimeters;
    use wowsunpack::game_params::types::ShipModelDistance;
    use wowsunpack::game_params::types::Species;

    use super::super::model::ViewportModel;
    use super::super::state::ViewCx;
    use super::super::types::CastShell;
    use super::super::types::Isolate;
    use super::super::types::SplashBoxName;
    use super::SHOT_LOG_LINES;
    use super::carried_splash_ticks;

    fn plate(zone: &str, thickness: f32) -> TrajectoryHit {
        TrajectoryHit {
            position: Vec3::new(0.0, 0.0, 0.0),
            thickness: Millimeters::from(thickness),
            zone: zone.to_string(),
            material: ArmorMaterial::parse("Belt").expect("Belt is a table key"),
            angle_from_normal: Degrees::from(0.0),
            distance_from_start: ShipModelDistance::from(0.0),
            normal: Vec3::y(),
        }
    }

    fn cast(range: f32) -> trajectory::Trajectory {
        trajectory::cast(vec![plate("Citadel", 100.0)], Vec3::x(), None, 20.0, false, Km::new(range))
    }

    fn ship(param_index: &str) -> CastShell {
        CastShell {
            ship: ComparisonShip {
                param_index: param_index.to_string(),
                display_name: param_index.to_string(),
                tier: 10,
                nation: "usa".to_string(),
                species: Species::Cruiser,
                shells: Vec::new(),
            },
            shell_index: None,
        }
    }

    fn landed_at(seconds: f32) -> PreExtractedHit {
        let victim = EntityId::from(3u32);
        PreExtractedHit {
            clock: GameClock(seconds),
            hit: ResolvedShotHit {
                clock: GameClock(seconds),
                hit: ShotHit {
                    owner_id: victim,
                    hit_type: HitType {
                        collision: Recognized::Unknown("0".to_string()),
                        shell_hit: Recognized::Unknown("0".to_string()),
                        raw: 0,
                    },
                    shot_id: ShotId::from(1u32),
                    position: WorldPos::new(0.0, 0.0, 0.0),
                    terminal_ballistics: None,
                },
                victim_entity_id: victim,
                salvo: None,
                fired_at: None,
                victim_pose: None,
            },
        }
    }

    #[test]
    fn a_cast_arc_is_selected_and_a_second_toggle_deselects_it() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let first = model.add_arc(Vec3::zeros(), Vec3::x(), cast(8.0), &mut cx);
        let second = model.add_arc(Vec3::zeros(), Vec3::x(), cast(12.0), &mut cx);
        assert_eq!(model.selected_arc(), Some((second, Km::new(12.0))));

        model.toggle_arc_selection(first, &mut cx);
        assert_eq!(model.selected_arc(), Some((first, Km::new(8.0))));
        model.toggle_arc_selection(first, &mut cx);
        assert_eq!(model.selected_arc(), None);
        assert!(cx.notified());
    }

    #[test]
    fn removing_the_selected_arc_clears_the_selection() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let first = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);
        let second = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);

        model.remove_arc(first, &mut cx);
        assert_eq!(model.selected_arc().map(|(id, _)| id), Some(second));
        model.remove_arc(second, &mut cx);
        assert_eq!(model.selected_arc(), None);
        assert!(model.arcs().is_empty());
    }

    #[test]
    fn isolating_plates_withdraws_zones_and_back() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let first = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);
        let second = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);

        model.isolate_arc(first, Isolate::Zones, true, &mut cx);
        model.isolate_arc(first, Isolate::Plates, true, &mut cx);
        let arcs = model.arcs();
        assert!(arcs[0].isolating_plates && !arcs[0].isolating_zones);
        assert!(!arcs[1].isolating_plates && !arcs[1].isolating_zones);

        model.isolate_all_arcs(Isolate::Zones, &mut cx);
        assert!(model.arcs().iter().all(|arc| arc.isolating_zones && !arc.isolating_plates));

        model.remove_arc(second, &mut cx);
        model.clear_trajectories(&mut cx);
        assert!(model.arcs().is_empty());
        assert!(model.plate_visibility.is_empty() && model.part_visibility.is_empty());
    }

    #[test]
    fn a_waiting_arc_adopts_the_cast_shell_and_a_fired_one_keeps_its_own() {
        let mut model = ViewportModel::new();
        let mut cx = ViewCx::for_test();
        let waiting = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);
        model.set_cast_shell(Some(ship("first")), &mut cx);
        let fired = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);
        model.set_cast_shell(None, &mut cx);
        let late = model.add_arc(Vec3::zeros(), Vec3::x(), cast(10.0), &mut cx);

        model.set_cast_shell(Some(ship("second")), &mut cx);

        let shell_of = |id| {
            model
                .arcs()
                .into_iter()
                .find(|arc| arc.id == id)
                .and_then(|arc| arc.shell)
                .map(|shell| shell.ship.param_index)
        };
        assert_eq!(shell_of(waiting).as_deref(), Some("first"));
        assert_eq!(shell_of(fired).as_deref(), Some("first"));
        assert_eq!(shell_of(late).as_deref(), Some("second"));
    }

    #[test]
    fn the_shot_log_is_bounded_and_newest_first() {
        let mut model = ViewportModel::new();
        model.hits = (0..SHOT_LOG_LINES + 5).map(|second| landed_at(second as f32)).collect();

        let log = model.shot_log();
        assert_eq!(log.len(), SHOT_LOG_LINES);
        let newest = wows_replay_insights::hull_impact::log_line(model.hits.last().expect("hits were added"));
        assert_eq!(log[0], newest);
    }

    #[test]
    fn a_reload_keeps_the_ticks_of_boxes_both_hulls_have() {
        let checked: HashSet<SplashBoxName> = ["cit_1", "gone"].into_iter().map(SplashBoxName::new).collect();
        let had: HashSet<&str> = ["cit_1", "cit_2", "gone"].into_iter().collect();
        let has = ["cit_1", "cit_2", "new"];
        let ticked = |shown: bool| {
            let mut names: Vec<String> = carried_splash_ticks(&checked, &had, has.into_iter(), shown)
                .into_iter()
                .map(|name| name.as_str().to_owned())
                .collect();
            names.sort();
            names
        };

        assert_eq!(ticked(true), ["cit_1", "new"], "a box only the new hull has is ticked with the outlines on");
        assert_eq!(ticked(false), ["cit_1"], "and left unticked with them off");
    }
}

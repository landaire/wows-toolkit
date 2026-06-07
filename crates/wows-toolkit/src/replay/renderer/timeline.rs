use std::collections::HashMap;

use egui::Color32;

use wows_battle_world::BattleWorld;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::decoder::Consumable;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::recognized::Recognized;

use super::ENEMY_COLOR;
use super::FRIENDLY_COLOR;
use wows_replays::analyzer::battle_controller::state::ControlPointType;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;

pub(crate) enum TimelineEventKind {
    HealthLost {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
        percent_lost: f32,
        old_hp: f32,
        new_hp: f32,
        max_hp: f32,
    },
    Death {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
        killer_ship: String,
        killer_player: String,
    },
    CapContested {
        cap_label: String,
        owner_is_friendly: bool,
    },
    CapFlipped {
        cap_label: String,
        capturer_is_friendly: bool,
    },
    CapBeingCaptured {
        cap_label: String,
        capturer_is_friendly: bool,
    },
    RadarUsed {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
    },
    AdvantageChanged {
        label: String,
        is_friendly: bool,
    },
    Disconnected {
        ship_name: String,
        player_name: String,
        is_friendly: bool,
    },
}

pub(crate) struct TimelineEvent {
    pub(crate) clock: ElapsedClock,
    pub(crate) kind: TimelineEventKind,
}
/// Health snapshot for a ship entity at a point in time.
#[derive(Clone, Debug)]
pub struct HealthSnapshot {
    pub health: f32,
    pub max_health: f32,
}

/// Pre-extracted shot hit for a target ship (full replay).
#[derive(Clone, Debug)]
pub struct PreExtractedHit {
    pub clock: GameClock,
    pub hit: ResolvedShotHit,
}

/// Counts from the timeline pass used to pre-allocate buffers in pass 3.
#[derive(Clone, Debug, Default)]
pub struct ShotCountHints {
    /// Number of individual shell impacts against this ship.
    pub shell_count: usize,
}

/// Per-ship shot timeline, pre-computed from the full replay.
#[derive(Clone, Debug)]
pub struct ShipShotTimeline {
    pub hits: Vec<PreExtractedHit>,
    /// Health over time, keyed by GameClock. BTreeMap allows efficient
    /// lookup of health at any game clock via range queries.
    pub health_history: std::collections::BTreeMap<GameClock, HealthSnapshot>,
}

pub(crate) fn event_color(is_friendly: bool) -> Color32 {
    if is_friendly { FRIENDLY_COLOR } else { ENEMY_COLOR }
}

pub(crate) fn format_timeline_event(event: &TimelineEvent) -> String {
    let mins = event.clock.seconds() as u32 / 60;
    let secs = event.clock.seconds() as u32 % 60;
    let time = format!("{:02}:{:02}", mins, secs);
    let desc = match &event.kind {
        TimelineEventKind::HealthLost { ship_name, player_name, percent_lost, old_hp, new_hp, max_hp, .. } => {
            format!(
                "{} ({}) -{}% HP ({:.0}/{:.0} -> {:.0}/{:.0})",
                ship_name,
                player_name,
                (percent_lost * 100.0) as u32,
                old_hp,
                max_hp,
                new_hp,
                max_hp
            )
        }
        TimelineEventKind::Death { ship_name, player_name, killer_ship, killer_player, .. } => {
            if killer_ship.is_empty() {
                format!("{} ({}) destroyed", ship_name, player_name)
            } else {
                format!("{} ({}) destroyed by {} ({})", ship_name, player_name, killer_ship, killer_player)
            }
        }
        TimelineEventKind::CapContested { cap_label, .. } => format!("{} contested", cap_label),
        TimelineEventKind::CapFlipped { cap_label, .. } => format!("{} captured", cap_label),
        TimelineEventKind::CapBeingCaptured { cap_label, .. } => format!("{} being captured", cap_label),
        TimelineEventKind::RadarUsed { ship_name, player_name, .. } => {
            format!("{} ({}) used radar", ship_name, player_name)
        }
        TimelineEventKind::AdvantageChanged { label, .. } => label.clone(),
        TimelineEventKind::Disconnected { ship_name, player_name, .. } => {
            format!("{} ({}) disconnected", ship_name, player_name)
        }
    };
    format!("[{}] {}", time, desc)
}

/// Parse the entire replay and extract significant game events for the timeline.
/// Returns `(events, battle_start)` where `battle_start` is the absolute game clock
/// at which the battle started. Event clocks are adjusted to elapsed time.
/// Result from the timeline extraction pass (pass 2).
pub(super) struct TimelineExtractionResult {
    pub(super) events: Vec<TimelineEvent>,
    pub(super) battle_start: GameClock,
    pub(super) shot_counts: HashMap<EntityId, ShotCountHints>,
    pub(super) health_histories: HashMap<EntityId, std::collections::BTreeMap<GameClock, HealthSnapshot>>,
}

pub(super) fn extract_timeline_events(
    replay_file: &ReplayFile,
    game_metadata: &GameMetadataProvider,
    game_constants: Option<&GameConstants>,
) -> TimelineExtractionResult {
    let mut events = Vec::new();
    let mut controller = BattleWorld::new(&replay_file.meta, game_metadata, game_constants);
    let replay_version = wowsunpack::data::Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut parser = wows_replays::packet2::Parser::with_version(game_metadata.entity_specs(), replay_version);

    // Player info lookups (populated once players are available)
    let mut ship_names: HashMap<EntityId, String> = HashMap::new();
    let mut player_names: HashMap<EntityId, String> = HashMap::new();
    let mut is_friendly: HashMap<EntityId, bool> = HashMap::new();
    let mut viewer_team_id: Option<i64> = None;
    let mut players_populated = false;

    // Health tracking: entity -> (window_start_clock, health_at_window_start)
    let mut health_windows: HashMap<EntityId, (GameClock, f32)> = HashMap::new();

    // Shot counting for pre-allocation hints
    let mut shot_counts: HashMap<EntityId, ShotCountHints> = HashMap::new();

    // Health history: per-entity health snapshots on every change
    let mut health_histories: HashMap<EntityId, std::collections::BTreeMap<GameClock, HealthSnapshot>> = HashMap::new();
    let mut last_health: HashMap<EntityId, f32> = HashMap::new();

    // Kill tracking
    let mut last_kill_count: usize = 0;

    // Cap tracking: cap_index -> (previous has_invaders, previous team_id)
    let mut cap_prev_contested: HashMap<usize, bool> = HashMap::new();
    let mut cap_prev_team: HashMap<usize, i64> = HashMap::new();
    let mut cap_prev_invader_team: HashMap<usize, i64> = HashMap::new();

    // Radar tracking: entity -> number of radar activations seen so far
    let mut radar_counts: HashMap<EntityId, usize> = HashMap::new();

    // Advantage tracking
    use wows_minimap_renderer::advantage;
    use wows_minimap_renderer::advantage::ScoringParams;
    use wows_minimap_renderer::advantage::TeamAdvantage;
    use wows_minimap_renderer::advantage::TeamState;
    let mut prev_advantage: TeamAdvantage = TeamAdvantage::Even;
    let mut advantage_check_clock = GameClock(0.0);

    let mut remaining = &replay_file.packet_data[..];
    let mut prev_clock = GameClock(0.0);

    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => {
                if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
                    // Populate player info on first tick where players are available
                    if !players_populated {
                        let players = controller.player_entities();
                        if !players.is_empty() {
                            for (entity_id, player) in players {
                                let ship_name =
                                    game_metadata.localized_name_from_param(player.vehicle()).unwrap_or_default();
                                ship_names.insert(*entity_id, ship_name);
                                player_names.insert(*entity_id, player.initial_state().username().to_string());

                                let relation = player.relation();
                                let friendly = relation.is_self() || relation.is_ally();
                                is_friendly.insert(*entity_id, friendly);

                                if relation.is_self() {
                                    viewer_team_id = Some(player.initial_state().team_id());
                                }
                            }
                            players_populated = true;
                        }
                    }

                    let clock = prev_clock;

                    // --- Health loss detection ---
                    for (entity_id, props) in controller.vehicle_props_all() {
                        let current_health = props.health();
                        let max_health = props.max_health();

                        if max_health <= 0.0 {
                            continue;
                        }

                        if let Some((window_start, health_at_start)) = health_windows.get_mut(&entity_id) {
                            if clock - *window_start >= 3.0 {
                                let loss = (*health_at_start - current_health) / max_health;
                                if loss > 0.25 {
                                    let sname = ship_names.get(&entity_id).cloned().unwrap_or_default();
                                    let pname = player_names.get(&entity_id).cloned().unwrap_or_default();
                                    let friendly = is_friendly.get(&entity_id).copied().unwrap_or(false);
                                    events.push(TimelineEvent {
                                        clock: ElapsedClock(clock.seconds()),
                                        kind: TimelineEventKind::HealthLost {
                                            ship_name: sname,
                                            player_name: pname,
                                            is_friendly: friendly,
                                            percent_lost: loss,
                                            old_hp: *health_at_start,
                                            new_hp: current_health,
                                            max_hp: max_health,
                                        },
                                    });
                                }
                                *window_start = clock;
                                *health_at_start = current_health;
                            }
                        } else if props.is_alive() {
                            health_windows.insert(entity_id, (clock, current_health));
                        }
                    }

                    // --- Death detection ---
                    let kills = controller.kills();
                    if kills.len() > last_kill_count {
                        for kill in &kills[last_kill_count..] {
                            let victim_ship = ship_names.get(&kill.victim).cloned().unwrap_or_default();
                            let victim_player = player_names.get(&kill.victim).cloned().unwrap_or_default();
                            let friendly = is_friendly.get(&kill.victim).copied().unwrap_or(false);
                            let killer_ship = ship_names.get(&kill.killer).cloned().unwrap_or_default();
                            let killer_player = player_names.get(&kill.killer).cloned().unwrap_or_default();
                            events.push(TimelineEvent {
                                clock: ElapsedClock(kill.clock.seconds()),
                                kind: TimelineEventKind::Death {
                                    ship_name: victim_ship,
                                    player_name: victim_player,
                                    is_friendly: friendly,
                                    killer_ship,
                                    killer_player,
                                },
                            });
                        }
                        last_kill_count = kills.len();
                    }

                    // --- Capture point events ---
                    let viewer_team = viewer_team_id.unwrap_or(0);
                    for cap in controller.capture_points() {
                        let cap_idx = cap.index;

                        let is_base = cap
                            .control_point_type
                            .as_ref()
                            .and_then(|r| r.known().copied())
                            .map(|t| {
                                matches!(
                                    t,
                                    ControlPointType::Base
                                        | ControlPointType::BaseWithPoints
                                        | ControlPointType::MegaBase
                                )
                            })
                            .unwrap_or(false);
                        let cap_label =
                            if is_base { "\u{2691}".to_string() } else { ((b'A' + cap_idx as u8) as char).to_string() };

                        // Cap contested: both_inside transitions false -> true
                        let prev_contested = cap_prev_contested.get(&cap_idx).copied().unwrap_or(false);
                        if cap.both_inside && !prev_contested {
                            events.push(TimelineEvent {
                                clock: ElapsedClock(clock.seconds()),
                                kind: TimelineEventKind::CapContested {
                                    cap_label: cap_label.clone(),
                                    owner_is_friendly: cap.team_id == viewer_team,
                                },
                            });
                        }
                        cap_prev_contested.insert(cap_idx, cap.both_inside);

                        // Cap being captured (uncontested): invader_team transitions from
                        // no-invader (<0) to a valid team (>=0), while not contested
                        let prev_invader = cap_prev_invader_team.get(&cap_idx).copied().unwrap_or(-1);
                        if cap.invader_team >= 0 && prev_invader < 0 && !cap.both_inside {
                            events.push(TimelineEvent {
                                clock: ElapsedClock(clock.seconds()),
                                kind: TimelineEventKind::CapBeingCaptured {
                                    cap_label: cap_label.clone(),
                                    capturer_is_friendly: cap.invader_team == viewer_team,
                                },
                            });
                        }
                        cap_prev_invader_team.insert(cap_idx, cap.invader_team);

                        // Cap flipped: team_id changes
                        if let Some(&prev_team) = cap_prev_team.get(&cap_idx)
                            && cap.team_id != prev_team
                            && cap.team_id >= 0
                        {
                            events.push(TimelineEvent {
                                clock: ElapsedClock(clock.seconds()),
                                kind: TimelineEventKind::CapFlipped {
                                    cap_label,
                                    capturer_is_friendly: cap.team_id == viewer_team,
                                },
                            });
                        }
                        cap_prev_team.insert(cap_idx, cap.team_id);
                    }

                    // --- Radar activation detection ---
                    for (entity_id, consumables) in controller.active_consumables() {
                        let radar_count =
                            consumables.iter().filter(|c| c.consumable == Recognized::Known(Consumable::Radar)).count();
                        let prev_count = radar_counts.get(&entity_id).copied().unwrap_or(0);
                        if radar_count > prev_count {
                            let sname = ship_names.get(&entity_id).cloned().unwrap_or_default();
                            let pname = player_names.get(&entity_id).cloned().unwrap_or_default();
                            let friendly = is_friendly.get(&entity_id).copied().unwrap_or(false);
                            events.push(TimelineEvent {
                                clock: ElapsedClock(clock.seconds()),
                                kind: TimelineEventKind::RadarUsed {
                                    ship_name: sname,
                                    player_name: pname,
                                    is_friendly: friendly,
                                },
                            });
                        }
                        radar_counts.insert(entity_id, radar_count);
                    }

                    // --- Advantage change detection (check every ~3 seconds) ---
                    if clock - advantage_check_clock >= 3.0 && players_populated {
                        advantage_check_clock = clock;

                        let viewer_team = viewer_team_id.unwrap_or(0);
                        let swap = viewer_team == 1;
                        // Snapshot players before vehicle_props_all (&mut borrow).
                        let players: Vec<(
                            wows_replays::types::EntityId,
                            wows_replays::Rc<wows_replays::analyzer::battle_controller::Player>,
                        )> = controller
                            .player_entities()
                            .iter()
                            .map(|(id, p)| (*id, wows_replays::Rc::clone(p)))
                            .collect();
                        let all_vehicle_props = controller.vehicle_props_all();

                        let mut teams = [
                            TeamState {
                                score: 0,
                                uncontested_caps: 0,
                                total_hp: 0.0,
                                max_hp: 0.0,
                                ships_alive: 0,
                                ships_total: 0,
                                ships_known: 0,
                                destroyers: Default::default(),
                                cruisers: Default::default(),
                                battleships: Default::default(),
                                submarines: Default::default(),
                                carriers: Default::default(),
                            },
                            TeamState {
                                score: 0,
                                uncontested_caps: 0,
                                total_hp: 0.0,
                                max_hp: 0.0,
                                ships_alive: 0,
                                ships_total: 0,
                                ships_known: 0,
                                destroyers: Default::default(),
                                cruisers: Default::default(),
                                battleships: Default::default(),
                                submarines: Default::default(),
                                carriers: Default::default(),
                            },
                        ];

                        let scores = controller.team_scores();
                        if scores.len() >= 2 {
                            teams[0].score = scores[0].score;
                            teams[1].score = scores[1].score;
                        }

                        for cp in controller.capture_points() {
                            if !cp.is_enabled || cp.has_invaders {
                                continue;
                            }
                            if cp.team_id == 0 {
                                teams[0].uncontested_caps += 1;
                            } else if cp.team_id == 1 {
                                teams[1].uncontested_caps += 1;
                            }
                        }

                        for (entity_id, player) in &players {
                            let team = player.initial_state().team_id() as usize;
                            if team > 1 {
                                continue;
                            }
                            teams[team].ships_total += 1;
                            if let Some(props) = all_vehicle_props.get(entity_id) {
                                teams[team].ships_known += 1;
                                teams[team].max_hp += props.max_health();
                                if props.is_alive() {
                                    teams[team].ships_alive += 1;
                                    teams[team].total_hp += props.health();
                                }
                            }
                        }

                        let scoring = controller
                            .scoring_rules()
                            .map(|r| ScoringParams {
                                team_win_score: r.team_win_score,
                                hold_reward: r.hold_reward,
                                hold_period: r.hold_period,
                            })
                            .unwrap_or(ScoringParams { team_win_score: 1000, hold_reward: 3, hold_period: 5.0 });

                        let result =
                            advantage::calculate_advantage(&teams[0], &teams[1], &scoring, controller.time_left());

                        // Swap so Team0 = friendly
                        let current = if swap {
                            match result.advantage {
                                TeamAdvantage::Team0(level) => TeamAdvantage::Team1(level),
                                TeamAdvantage::Team1(level) => TeamAdvantage::Team0(level),
                                other => other,
                            }
                        } else {
                            result.advantage
                        };

                        if current != prev_advantage {
                            let level_label = |adv: &TeamAdvantage| -> Option<(&str, bool)> {
                                match adv {
                                    TeamAdvantage::Team0(level) => Some((level.label(), true)),
                                    TeamAdvantage::Team1(level) => Some((level.label(), false)),
                                    TeamAdvantage::Even => None,
                                }
                            };

                            let label = match (level_label(&prev_advantage), level_label(&current)) {
                                // Gained advantage from even
                                (None, Some((new_label, _))) => {
                                    format!("{} advantage gained", new_label)
                                }
                                // Lost advantage to even
                                (Some((old_label, _)), None) => {
                                    format!("{} advantage lost", old_label)
                                }
                                // Same team, level changed
                                (Some((old_label, old_friendly)), Some((new_label, new_friendly)))
                                    if old_friendly == new_friendly =>
                                {
                                    let old_val = match &prev_advantage {
                                        TeamAdvantage::Team0(l) | TeamAdvantage::Team1(l) => Some(*l),
                                        _ => None,
                                    };
                                    let new_val = match &current {
                                        TeamAdvantage::Team0(l) | TeamAdvantage::Team1(l) => Some(*l),
                                        _ => None,
                                    };
                                    // Compare by discriminant order (Absolute=0 > Strong=1 > Moderate=2 > Weak=3)
                                    if let (Some(o), Some(n)) = (old_val, new_val) {
                                        if (n as u8) < (o as u8) {
                                            format!("{} advantage gained", new_label)
                                        } else {
                                            format!("Dropped to {} advantage", new_label)
                                        }
                                    } else {
                                        format!("{} advantage", new_label)
                                    }
                                }
                                // Advantage flipped teams
                                (Some(_), Some((new_label, _))) => {
                                    format!("{} advantage gained", new_label)
                                }
                                _ => String::new(),
                            };

                            if !label.is_empty() {
                                let is_friendly = match &current {
                                    TeamAdvantage::Team0(_) => true,
                                    TeamAdvantage::Team1(_) => false,
                                    TeamAdvantage::Even => match &prev_advantage {
                                        TeamAdvantage::Team1(_) => true, // enemy lost advantage = good for us
                                        _ => false,
                                    },
                                };
                                events.push(TimelineEvent {
                                    clock: ElapsedClock(clock.seconds()),
                                    kind: TimelineEventKind::AdvantageChanged { label, is_friendly },
                                });
                            }
                            prev_advantage = current;
                        }
                    }

                    prev_clock = packet.clock;
                } else if prev_clock.seconds() == 0.0 {
                    prev_clock = packet.clock;
                }

                controller.process(&packet);

                // --- Shot counting (for pre-allocation in pass 3) ---
                for hit in controller.shot_hits() {
                    let counts = shot_counts.entry(hit.victim_entity_id).or_default();
                    counts.shell_count += 1;
                }

                // --- Health history snapshots (on every change) ---
                for (entity_id, props) in controller.vehicle_props_all() {
                    let current_hp = props.health();
                    let max_hp = props.max_health();
                    if max_hp <= 0.0 {
                        continue;
                    }
                    let prev_hp = last_health.get(&entity_id).copied();
                    if prev_hp.is_none() || (current_hp - prev_hp.unwrap()).abs() > 0.1 {
                        last_health.insert(entity_id, current_hp);
                        health_histories
                            .entry(entity_id)
                            .or_default()
                            .insert(packet.clock, HealthSnapshot { health: current_hp, max_health: max_hp });
                    }
                }
            }
            Err(_) => break,
        }
    }

    controller.finish();

    // --- Disconnect events (non-death) ---
    {
        use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
        for (entity_id, player) in controller.player_entities() {
            for info in player.connection_change_info().iter() {
                if info.event_kind() == ConnectionChangeKind::Disconnected && !info.had_death_event() {
                    let sname = ship_names.get(entity_id).cloned().unwrap_or_default();
                    let pname = player_names.get(entity_id).cloned().unwrap_or_default();
                    let friendly = is_friendly.get(entity_id).copied().unwrap_or(false);
                    events.push(TimelineEvent {
                        clock: ElapsedClock(info.at_game_duration().as_secs_f32()),
                        kind: TimelineEventKind::Disconnected {
                            ship_name: sname,
                            player_name: pname,
                            is_friendly: friendly,
                        },
                    });
                }
            }
        }
    }

    // Translate event times from absolute game clock to elapsed time since battle start
    let battle_start = controller.battle_start_clock().unwrap_or(GameClock(0.0));
    for event in &mut events {
        // Events were created with GameClock values stored as ElapsedClock(abs_seconds).
        // Convert to real elapsed time.
        let abs = GameClock(event.clock.seconds());
        event.clock = abs.to_elapsed(battle_start);
    }

    // Sort events by clock time
    events.sort_by(|a, b| a.clock.cmp(&b.clock));
    TimelineExtractionResult { events, battle_start, shot_counts, health_histories }
}
/// Parse the entire replay and extract all `ResolvedShotHit`s per ship.
/// Uses `shot_count_hints` from pass 2 to pre-allocate buffers.
/// Health histories from pass 2 are merged into the returned timelines.
pub(super) fn extract_all_shots(
    raw_meta: &[u8],
    packet_data: &[u8],
    game_metadata: &GameMetadataProvider,
    game_constants: Option<&GameConstants>,
    shot_count_hints: &HashMap<EntityId, ShotCountHints>,
    health_histories: HashMap<EntityId, std::collections::BTreeMap<GameClock, HealthSnapshot>>,
) -> HashMap<EntityId, ShipShotTimeline> {
    let replay_file = match ReplayFile::from_decrypted_parts(raw_meta.to_vec(), packet_data.to_vec()) {
        Ok(rf) => rf,
        Err(e) => {
            tracing::error!("extract_all_shots: failed to parse replay: {:?}", e);
            return HashMap::new();
        }
    };
    let mut controller = BattleWorld::new(&replay_file.meta, game_metadata, game_constants);
    // shot tracking is ON by default (ShotTracking::Tracked)
    let replay_version = wowsunpack::data::Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut parser = wows_replays::packet2::Parser::with_version(game_metadata.entity_specs(), replay_version);

    // Pre-allocate per-ship timelines from hints
    let mut timelines: HashMap<EntityId, ShipShotTimeline> = shot_count_hints
        .iter()
        .map(|(&eid, hints)| {
            (
                eid,
                ShipShotTimeline {
                    hits: Vec::with_capacity(hints.shell_count),
                    health_history: health_histories.get(&eid).cloned().unwrap_or_default(),
                },
            )
        })
        .collect();

    // Also create timelines for ships that had health changes but no shot hits
    for (eid, hh) in health_histories {
        timelines.entry(eid).or_insert_with(|| ShipShotTimeline { hits: Vec::new(), health_history: hh });
    }

    let mut remaining = &replay_file.packet_data[..];
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => {
                controller.process(&packet);

                // Accumulate all shot_hits (cleared each packet by the controller)
                for hit in controller.shot_hits() {
                    if let Some(timeline) = timelines.get_mut(&hit.victim_entity_id) {
                        timeline.hits.push(PreExtractedHit { clock: hit.clock, hit: hit.clone() });
                    } else {
                        // Ship not in hints (e.g. friendly fire) — create on demand
                        let mut tl = ShipShotTimeline {
                            hits: Vec::with_capacity(100),
                            health_history: std::collections::BTreeMap::new(),
                        };
                        tl.hits.push(PreExtractedHit { clock: hit.clock, hit: hit.clone() });
                        timelines.insert(hit.victim_entity_id, tl);
                    }
                }
            }
            Err(_) => break,
        }
    }

    tracing::info!(
        "extract_all_shots: {} ships, {} total hits",
        timelines.len(),
        timelines.values().map(|t| t.hits.len()).sum::<usize>(),
    );

    timelines
}

#[cfg(test)]
mod extraction_snapshots {
    use super::*;
    use std::path::PathBuf;

    use wows_replays::ReplayFile;
    use wows_replays::game_constants::GameConstants;
    use wowsunpack::game_params::provider::GameMetadataProvider;
    use wowsunpack::vfs::VfsPath;
    use wowsunpack::vfs::impls::physical::PhysicalFS;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("replays")
    }

    fn load_build_resources(build: u32) -> (GameMetadataProvider, GameConstants) {
        let dir = wows_data_mgr::game_dir_for_build(build)
            .unwrap_or_else(|| panic!("game data for build {} not available", build));
        let vfs_root = dir.join("vfs");
        let vfs = VfsPath::new(PhysicalFS::new(&vfs_root));
        let rkyv_path = dir.join("game_params.rkyv");
        let provider = match wowsunpack::game_params::cache::load(&rkyv_path) {
            Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs)
                .unwrap_or_else(|e| panic!("failed to build game metadata for build {build}: {e:?}")),
            None => GameMetadataProvider::from_vfs(&vfs)
                .unwrap_or_else(|e| panic!("failed to load GameParams for build {build}: {e:?}")),
        };
        let constants = GameConstants::from_vfs(&vfs);
        (provider, constants)
    }

    #[derive(serde::Serialize)]
    struct EventSnapshot {
        clock_s: f32,
        kind: String,
    }

    #[derive(serde::Serialize)]
    struct ShotCountRow {
        entity_id: u32,
        shell_count: usize,
    }

    #[derive(serde::Serialize)]
    struct HealthHistoryRow {
        entity_id: u32,
        sample_count: usize,
        first_clock_s: f32,
        last_clock_s: f32,
        health_sum: f32,
        min_health: f32,
    }

    #[derive(serde::Serialize)]
    struct ShotTimelineRow {
        entity_id: u32,
        hit_count: usize,
        first_hit_clock_s: Option<f32>,
        last_hit_clock_s: Option<f32>,
        hit_type_counts: std::collections::BTreeMap<String, usize>,
    }

    #[derive(serde::Serialize)]
    struct Snapshot {
        battle_start_s: f32,
        events: Vec<EventSnapshot>,
        shot_counts: Vec<ShotCountRow>,
        health_histories: Vec<HealthHistoryRow>,
        shot_timelines: Vec<ShotTimelineRow>,
    }

    fn r3(v: f32) -> f32 {
        (v * 1000.0).round() / 1000.0
    }

    fn event_kind_label(kind: &TimelineEventKind) -> String {
        match kind {
            TimelineEventKind::HealthLost { ship_name, player_name, percent_lost, new_hp, .. } => {
                format!(
                    "HealthLost({ship_name}/{player_name} pct={} new_hp={})",
                    (percent_lost * 1000.0).round() as i64,
                    new_hp.round() as i64,
                )
            }
            TimelineEventKind::Death { ship_name, player_name, .. } => {
                format!("Death({ship_name}/{player_name})")
            }
            TimelineEventKind::CapContested { cap_label, .. } => format!("CapContested({cap_label})"),
            TimelineEventKind::CapFlipped { cap_label, .. } => format!("CapFlipped({cap_label})"),
            TimelineEventKind::CapBeingCaptured { cap_label, .. } => format!("CapBeingCaptured({cap_label})"),
            TimelineEventKind::RadarUsed { ship_name, player_name, .. } => {
                format!("RadarUsed({ship_name}/{player_name})")
            }
            TimelineEventKind::AdvantageChanged { label, is_friendly } => {
                format!("AdvantageChanged({label} friendly={is_friendly})")
            }
            TimelineEventKind::Disconnected { ship_name, player_name, .. } => {
                format!("Disconnected({ship_name}/{player_name})")
            }
        }
    }

    #[test]
    #[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
    fn timeline_and_shots_golden() {
        let (provider, constants) = load_build_resources(11965230);

        let fixture = fixtures_dir().join("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
        let replay = ReplayFile::from_file(&fixture)
            .unwrap_or_else(|e| panic!("failed to load Vermont fixture: {e:?}"));

        let result = extract_timeline_events(&replay, &provider, Some(&constants));

        let raw_meta = replay.raw_meta.as_bytes();
        let packet_data = &replay.packet_data[..];

        let shots = extract_all_shots(
            raw_meta,
            packet_data,
            &provider,
            Some(&constants),
            &result.shot_counts,
            result.health_histories.clone(),
        );

        let mut events: Vec<EventSnapshot> = result
            .events
            .iter()
            .map(|e| EventSnapshot {
                clock_s: r3(e.clock.seconds()),
                kind: event_kind_label(&e.kind),
            })
            .collect();
        events.sort_by(|a, b| a.clock_s.total_cmp(&b.clock_s).then(a.kind.cmp(&b.kind)));

        let mut shot_counts: Vec<ShotCountRow> = result
            .shot_counts
            .iter()
            .map(|(&eid, h)| ShotCountRow { entity_id: eid.raw(), shell_count: h.shell_count })
            .collect();
        shot_counts.sort_by_key(|r| r.entity_id);

        let mut health_histories: Vec<HealthHistoryRow> = result
            .health_histories
            .iter()
            .map(|(&eid, hh)| {
                let first = hh.keys().next().map(|c| r3(c.seconds())).unwrap_or(0.0);
                let last = hh.keys().next_back().map(|c| r3(c.seconds())).unwrap_or(0.0);
                let health_sum = r3(hh.values().map(|s| s.health).sum::<f32>());
                let min_health = hh.values().map(|s| s.health).fold(f32::INFINITY, f32::min);
                let min_health = r3(if min_health.is_infinite() { 0.0 } else { min_health });
                HealthHistoryRow {
                    entity_id: eid.raw(),
                    sample_count: hh.len(),
                    first_clock_s: first,
                    last_clock_s: last,
                    health_sum,
                    min_health,
                }
            })
            .collect();
        health_histories.sort_by_key(|r| r.entity_id);

        let mut shot_timelines: Vec<ShotTimelineRow> = shots
            .iter()
            .map(|(&eid, tl)| {
                let first = tl.hits.first().map(|h| r3(h.clock.seconds()));
                let last = tl.hits.last().map(|h| r3(h.clock.seconds()));
                let mut hit_type_counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for peh in &tl.hits {
                    let label = format!("{}", peh.hit.hit.hit_type.shell_hit);
                    *hit_type_counts.entry(label).or_insert(0) += 1;
                }
                ShotTimelineRow {
                    entity_id: eid.raw(),
                    hit_count: tl.hits.len(),
                    first_hit_clock_s: first,
                    last_hit_clock_s: last,
                    hit_type_counts,
                }
            })
            .collect();
        shot_timelines.sort_by_key(|r| r.entity_id);

        let snapshot = Snapshot {
            battle_start_s: r3(result.battle_start.seconds()),
            events,
            shot_counts,
            health_histories,
            shot_timelines,
        };

        insta::assert_yaml_snapshot!(snapshot);
    }
}

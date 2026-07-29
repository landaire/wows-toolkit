use std::collections::HashMap;
use std::collections::HashSet;

use egui::Color32;

use wows_battle_world::scan::WorldScanCollector;
use wows_battle_world::scan::scan_replay_world;
use wows_battle_world::view::BattleView;
use wows_replays::ReplayFile;
use wows_replays::analyzer::decoder::Consumable;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Packet;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_types::TeamId;
use wowsunpack::recognized::Recognized;

use wows_replays::analyzer::battle_controller::state::ControlPointType;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;

use crate::replay::minimap_view::ENEMY_COLOR;
use crate::replay::minimap_view::FRIENDLY_COLOR;

#[derive(Clone, Debug)]
pub(crate) enum TimelineEventKind {
    HealthLost {
        ship_name: String,
        player_name: String,
        team: TeamId,
        percent_lost: f32,
        old_hp: f32,
        new_hp: f32,
        max_hp: f32,
    },
    Death {
        ship_name: String,
        player_name: String,
        team: TeamId,
        killer_ship: String,
        killer_player: String,
    },
    CapContested {
        cap_label: String,
        owner_team: Option<TeamId>,
    },
    CapFlipped {
        cap_label: String,
        capturer_team: TeamId,
    },
    CapBeingCaptured {
        cap_label: String,
        capturer_team: TeamId,
    },
    RadarUsed {
        ship_name: String,
        player_name: String,
        team: TeamId,
    },
    AdvantageChanged {
        label: String,
        is_friendly: bool,
    },
    Disconnected {
        ship_name: String,
        player_name: String,
        team: TeamId,
    },
}

#[derive(Clone, Debug)]
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

/// Per-ship shot timeline, pre-computed from the full replay.
#[derive(Clone, Debug)]
pub struct ShipShotTimeline {
    pub hits: Vec<PreExtractedHit>,
    /// Health over time, keyed by GameClock. BTreeMap allows efficient
    /// lookup of health at any game clock via range queries.
    pub health_history: std::collections::BTreeMap<GameClock, HealthSnapshot>,
}

pub(crate) fn event_color(team: TeamId, viewer_team: Option<TeamId>) -> Color32 {
    // Without a known viewer team every ship reads as an opponent, which is the
    // safer default: it never claims an enemy is an ally.
    match viewer_team {
        Some(viewer) if viewer == team => FRIENDLY_COLOR,
        _ => ENEMY_COLOR,
    }
}

/// Advantage events are viewer-relative and carry no absolute team.
pub(crate) fn advantage_color(is_friendly: bool) -> Color32 {
    if is_friendly { FRIENDLY_COLOR } else { ENEMY_COLOR }
}

/// Capture points use a negative team id to mean "no team holds this".
fn cap_team(raw: i64) -> Option<TeamId> {
    (raw >= 0).then(|| TeamId::new(raw))
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
/// Result from the combined timeline + shot extraction pass.
pub(crate) struct TimelineExtractionResult {
    pub(crate) events: Vec<TimelineEvent>,
    pub(crate) battle_start: GameClock,
    pub(crate) battle_end: Option<GameClock>,
    pub(crate) viewer_team: Option<TeamId>,
}

struct TimelineEventsCollector<'a> {
    game_metadata: &'a GameMetadataProvider,
    events: Vec<TimelineEvent>,
    ship_names: HashMap<EntityId, String>,
    player_names: HashMap<EntityId, String>,
    teams: HashMap<EntityId, TeamId>,
    viewer_team: Option<TeamId>,
    players_populated: bool,
    health_windows: HashMap<EntityId, (GameClock, f32)>,
    health_histories: HashMap<EntityId, std::collections::BTreeMap<GameClock, HealthSnapshot>>,
    last_health: HashMap<EntityId, f32>,
    last_kill_count: usize,
    cap_prev_contested: HashMap<usize, bool>,
    cap_prev_team: HashMap<usize, Option<TeamId>>,
    cap_prev_invader_team: HashMap<usize, Option<TeamId>>,
    radar_counts: HashMap<EntityId, usize>,
    prev_advantage: wows_minimap_renderer::advantage::TeamAdvantage,
    advantage_check_clock: GameClock,
    battle_start: GameClock,
    battle_end: Option<GameClock>,
}

impl<'a> TimelineEventsCollector<'a> {
    fn new(game_metadata: &'a GameMetadataProvider) -> Self {
        Self {
            game_metadata,
            events: Vec::new(),
            ship_names: HashMap::new(),
            player_names: HashMap::new(),
            teams: HashMap::new(),
            viewer_team: None,
            players_populated: false,
            health_windows: HashMap::new(),
            health_histories: HashMap::new(),
            last_health: HashMap::new(),
            last_kill_count: 0,
            cap_prev_contested: HashMap::new(),
            cap_prev_team: HashMap::new(),
            cap_prev_invader_team: HashMap::new(),
            radar_counts: HashMap::new(),
            prev_advantage: wows_minimap_renderer::advantage::TeamAdvantage::Even,
            advantage_check_clock: GameClock(0.0),
            battle_start: GameClock(0.0),
            battle_end: None,
        }
    }
}

impl WorldScanCollector for TimelineEventsCollector<'_> {
    fn observe_pre(&mut self, packet: &Packet<'_, '_>, prev_clock: GameClock, view: &BattleView<'_>) {
        if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
            if !self.players_populated {
                let players = view.player_entities();
                if !players.is_empty() {
                    for (entity_id, player) in players {
                        let ship_name =
                            self.game_metadata.localized_name_from_param(player.vehicle()).unwrap_or_default();
                        self.ship_names.insert(*entity_id, ship_name);
                        self.player_names.insert(*entity_id, player.initial_state().username().to_string());

                        let team = TeamId::new(player.initial_state().team_id());
                        self.teams.insert(*entity_id, team);

                        if player.relation().is_self() {
                            self.viewer_team = Some(team);
                        }
                    }
                    self.players_populated = true;
                }
            }

            let clock = prev_clock;

            for (entity_id, props) in view.vehicle_props_all() {
                let current_health = props.health();
                let max_health = props.max_health();

                if max_health <= 0.0 {
                    continue;
                }

                if let Some((window_start, health_at_start)) = self.health_windows.get_mut(&entity_id) {
                    if clock - *window_start >= 3.0 {
                        let loss = (*health_at_start - current_health) / max_health;
                        if loss > 0.25
                            && let Some(team) = self.teams.get(&entity_id).copied()
                        {
                            let sname = self.ship_names.get(&entity_id).cloned().unwrap_or_default();
                            let pname = self.player_names.get(&entity_id).cloned().unwrap_or_default();
                            self.events.push(TimelineEvent {
                                clock: ElapsedClock(clock.seconds()),
                                kind: TimelineEventKind::HealthLost {
                                    ship_name: sname,
                                    player_name: pname,
                                    team,
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
                    self.health_windows.insert(entity_id, (clock, current_health));
                }
            }

            let kills = view.kills();
            if kills.len() > self.last_kill_count {
                for kill in &kills[self.last_kill_count..] {
                    let Some(team) = self.teams.get(&kill.victim).copied() else {
                        continue;
                    };
                    let victim_ship = self.ship_names.get(&kill.victim).cloned().unwrap_or_default();
                    let victim_player = self.player_names.get(&kill.victim).cloned().unwrap_or_default();
                    let killer_ship = self.ship_names.get(&kill.killer).cloned().unwrap_or_default();
                    let killer_player = self.player_names.get(&kill.killer).cloned().unwrap_or_default();
                    self.events.push(TimelineEvent {
                        clock: ElapsedClock(kill.clock.seconds()),
                        kind: TimelineEventKind::Death {
                            ship_name: victim_ship,
                            player_name: victim_player,
                            team,
                            killer_ship,
                            killer_player,
                        },
                    });
                }
                self.last_kill_count = kills.len();
            }

            for cap in view.capture_points() {
                let cap_idx = cap.index;

                let is_base = cap
                    .control_point_type
                    .as_ref()
                    .and_then(|r| r.known().copied())
                    .map(|t| {
                        matches!(
                            t,
                            ControlPointType::Base | ControlPointType::BaseWithPoints | ControlPointType::MegaBase
                        )
                    })
                    .unwrap_or(false);
                let cap_label =
                    if is_base { "\u{2691}".to_string() } else { ((b'A' + cap_idx as u8) as char).to_string() };

                let prev_contested = self.cap_prev_contested.get(&cap_idx).copied().unwrap_or(false);
                if cap.both_inside && !prev_contested {
                    self.events.push(TimelineEvent {
                        clock: ElapsedClock(clock.seconds()),
                        kind: TimelineEventKind::CapContested {
                            cap_label: cap_label.clone(),
                            owner_team: cap_team(cap.team_id),
                        },
                    });
                }
                self.cap_prev_contested.insert(cap_idx, cap.both_inside);

                let prev_invader = self.cap_prev_invader_team.get(&cap_idx).copied().flatten();
                if let Some(capturer_team) = cap_team(cap.invader_team)
                    && prev_invader.is_none()
                    && !cap.both_inside
                {
                    self.events.push(TimelineEvent {
                        clock: ElapsedClock(clock.seconds()),
                        kind: TimelineEventKind::CapBeingCaptured { cap_label: cap_label.clone(), capturer_team },
                    });
                }
                self.cap_prev_invader_team.insert(cap_idx, cap_team(cap.invader_team));

                let current_team = cap_team(cap.team_id);
                if let Some(&prev_team) = self.cap_prev_team.get(&cap_idx)
                    && current_team != prev_team
                    && let Some(capturer_team) = current_team
                {
                    self.events.push(TimelineEvent {
                        clock: ElapsedClock(clock.seconds()),
                        kind: TimelineEventKind::CapFlipped { cap_label, capturer_team },
                    });
                }
                self.cap_prev_team.insert(cap_idx, current_team);
            }

            for (entity_id, consumables) in view.active_consumables() {
                let radar_count =
                    consumables.iter().filter(|c| c.consumable == Recognized::Known(Consumable::Radar)).count();
                let prev_count = self.radar_counts.get(&entity_id).copied().unwrap_or(0);
                if radar_count > prev_count
                    && let Some(team) = self.teams.get(&entity_id).copied()
                {
                    let sname = self.ship_names.get(&entity_id).cloned().unwrap_or_default();
                    let pname = self.player_names.get(&entity_id).cloned().unwrap_or_default();
                    self.events.push(TimelineEvent {
                        clock: ElapsedClock(clock.seconds()),
                        kind: TimelineEventKind::RadarUsed { ship_name: sname, player_name: pname, team },
                    });
                }
                self.radar_counts.insert(entity_id, radar_count);
            }

            if clock - self.advantage_check_clock >= 3.0 && self.players_populated {
                use wows_minimap_renderer::advantage;
                use wows_minimap_renderer::advantage::ScoringParams;
                use wows_minimap_renderer::advantage::TeamAdvantage;
                use wows_minimap_renderer::advantage::TeamState;

                self.advantage_check_clock = clock;

                // Advantage is computed relative to the viewer's team, so there is nothing
                // meaningful to report until that team is known.
                let Some(viewer_team) = self.viewer_team else {
                    return;
                };
                let swap = viewer_team.raw() == 1;

                let players = view.player_entities();
                let all_vehicle_props = view.vehicle_props_all();

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

                let scores = view.team_scores();
                if scores.len() >= 2 {
                    teams[0].score = scores[0].score;
                    teams[1].score = scores[1].score;
                }

                for cp in view.capture_points() {
                    if !cp.is_enabled || cp.has_invaders {
                        continue;
                    }
                    if cp.team_id == 0 {
                        teams[0].uncontested_caps += 1;
                    } else if cp.team_id == 1 {
                        teams[1].uncontested_caps += 1;
                    }
                }

                for (entity_id, player) in players {
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

                let scoring = view
                    .scoring_rules()
                    .map(|r| ScoringParams {
                        team_win_score: r.team_win_score,
                        hold_reward: r.hold_reward,
                        hold_period: r.hold_period,
                    })
                    .unwrap_or(ScoringParams { team_win_score: 1000, hold_reward: 3, hold_period: 5.0 });

                let result = advantage::calculate_advantage(&teams[0], &teams[1], &scoring, view.time_left());

                let current = if swap {
                    match result.advantage {
                        TeamAdvantage::Team0(level) => TeamAdvantage::Team1(level),
                        TeamAdvantage::Team1(level) => TeamAdvantage::Team0(level),
                        other => other,
                    }
                } else {
                    result.advantage
                };

                if current != self.prev_advantage {
                    let level_label = |adv: &TeamAdvantage| -> Option<(&str, bool)> {
                        match adv {
                            TeamAdvantage::Team0(level) => Some((level.label(), true)),
                            TeamAdvantage::Team1(level) => Some((level.label(), false)),
                            TeamAdvantage::Even => None,
                        }
                    };

                    let label = match (level_label(&self.prev_advantage), level_label(&current)) {
                        (None, Some((new_label, _))) => {
                            format!("{} advantage gained", new_label)
                        }
                        (Some((old_label, _)), None) => {
                            format!("{} advantage lost", old_label)
                        }
                        (Some((old_label, old_friendly)), Some((new_label, new_friendly)))
                            if old_friendly == new_friendly =>
                        {
                            let old_val = match &self.prev_advantage {
                                TeamAdvantage::Team0(l) | TeamAdvantage::Team1(l) => Some(*l),
                                _ => None,
                            };
                            let new_val = match &current {
                                TeamAdvantage::Team0(l) | TeamAdvantage::Team1(l) => Some(*l),
                                _ => None,
                            };
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
                        (Some(_), Some((new_label, _))) => {
                            format!("{} advantage gained", new_label)
                        }
                        _ => String::new(),
                    };

                    if !label.is_empty() {
                        let is_friendly = match &current {
                            TeamAdvantage::Team0(_) => true,
                            TeamAdvantage::Team1(_) => false,
                            TeamAdvantage::Even => matches!(&self.prev_advantage, TeamAdvantage::Team1(_)),
                        };
                        self.events.push(TimelineEvent {
                            clock: ElapsedClock(clock.seconds()),
                            kind: TimelineEventKind::AdvantageChanged { label, is_friendly },
                        });
                    }
                    self.prev_advantage = current;
                }
            }
        }
    }

    fn observe(&mut self, packet: &Packet<'_, '_>, _prev_clock: GameClock, view: &BattleView<'_>) {
        for (entity_id, props) in view.vehicle_props_all() {
            let current_hp = props.health();
            let max_hp = props.max_health();
            if max_hp <= 0.0 {
                continue;
            }
            let prev_hp = self.last_health.get(&entity_id).copied();
            if prev_hp.is_none() || (current_hp - prev_hp.unwrap()).abs() > 0.1 {
                self.last_health.insert(entity_id, current_hp);
                self.health_histories
                    .entry(entity_id)
                    .or_default()
                    .insert(packet.clock, HealthSnapshot { health: current_hp, max_health: max_hp });
            }
        }
    }

    fn finish(&mut self, view: &BattleView<'_>) {
        use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
        for (entity_id, player) in view.player_entities() {
            for info in player.connection_change_info().iter() {
                if info.event_kind() == ConnectionChangeKind::Disconnected
                    && !info.had_death_event()
                    && let Some(team) = self.teams.get(entity_id).copied()
                {
                    let sname = self.ship_names.get(entity_id).cloned().unwrap_or_default();
                    let pname = self.player_names.get(entity_id).cloned().unwrap_or_default();
                    self.events.push(TimelineEvent {
                        clock: ElapsedClock(info.at_game_duration().as_secs_f32()),
                        kind: TimelineEventKind::Disconnected { ship_name: sname, player_name: pname, team },
                    });
                }
            }
        }
        self.battle_start = view.battle_start_clock().unwrap_or(GameClock(0.0));
        self.battle_end = view.battle_end_clock();
    }
}

struct ShotTimelineCollector {
    timelines: HashMap<EntityId, ShipShotTimeline>,
}

impl ShotTimelineCollector {
    fn new() -> Self {
        Self { timelines: HashMap::new() }
    }
}

impl WorldScanCollector for ShotTimelineCollector {
    fn observe(&mut self, _packet: &Packet<'_, '_>, _prev_clock: GameClock, view: &BattleView<'_>) {
        for hit in view.shot_hits() {
            if let Some(timeline) = self.timelines.get_mut(&hit.victim_entity_id) {
                timeline.hits.push(PreExtractedHit { clock: hit.clock, hit: hit.clone() });
            } else {
                let mut tl = ShipShotTimeline {
                    hits: Vec::with_capacity(100),
                    health_history: std::collections::BTreeMap::new(),
                };
                tl.hits.push(PreExtractedHit { clock: hit.clock, hit: hit.clone() });
                self.timelines.insert(hit.victim_entity_id, tl);
            }
        }
    }
}

pub(crate) fn extract_timeline_and_shots(
    replay_file: &ReplayFile,
    game_metadata: &GameMetadataProvider,
    game_constants: Option<&GameConstants>,
) -> (TimelineExtractionResult, HashMap<EntityId, ShipShotTimeline>) {
    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut timeline_col = TimelineEventsCollector::new(game_metadata);
    let mut shot_col = ShotTimelineCollector::new();

    {
        let cols: &mut [&mut dyn WorldScanCollector] = &mut [&mut timeline_col, &mut shot_col];
        scan_replay_world(
            &replay_file.meta,
            game_metadata,
            game_constants.unwrap_or(&*wows_replays::game_constants::DEFAULT_GAME_CONSTANTS),
            replay_version,
            replay_file,
            cols,
        );
    }

    let health_histories = std::mem::take(&mut timeline_col.health_histories);
    let timeline_result = finish_timeline_collector(timeline_col);

    for (eid, hh) in &health_histories {
        shot_col
            .timelines
            .entry(*eid)
            .or_insert_with(|| ShipShotTimeline { hits: Vec::new(), health_history: hh.clone() });
        if let Some(tl) = shot_col.timelines.get_mut(eid)
            && tl.health_history.is_empty()
        {
            tl.health_history = hh.clone();
        }
    }

    tracing::info!(
        "extract_timeline_and_shots: {} ships, {} total hits",
        shot_col.timelines.len(),
        shot_col.timelines.values().map(|t| t.hits.len()).sum::<usize>(),
    );

    (timeline_result, shot_col.timelines)
}

fn finish_timeline_collector(mut col: TimelineEventsCollector<'_>) -> TimelineExtractionResult {
    let battle_start = col.battle_start;
    for event in &mut col.events {
        let abs = GameClock(event.clock.seconds());
        event.clock = abs.to_elapsed(battle_start);
    }
    col.events.sort_by(|a, b| a.clock.cmp(&b.clock));
    TimelineExtractionResult {
        events: col.events,
        battle_start,
        battle_end: col.battle_end,
        viewer_team: col.viewer_team,
    }
}

/// Scan `replay_file` for timeline events only, skipping the per-ship shot
/// collection that the renderer needs but the inspector does not.
pub(crate) fn extract_timeline_events(
    replay_file: &ReplayFile,
    game_metadata: &GameMetadataProvider,
    game_constants: Option<&GameConstants>,
) -> TimelineExtractionResult {
    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut timeline_col = TimelineEventsCollector::new(game_metadata);

    {
        let cols: &mut [&mut dyn WorldScanCollector] = &mut [&mut timeline_col];
        scan_replay_world(
            &replay_file.meta,
            game_metadata,
            game_constants.unwrap_or(&*wows_replays::game_constants::DEFAULT_GAME_CONSTANTS),
            replay_version,
            replay_file,
            cols,
        );
    }

    finish_timeline_collector(timeline_col)
}

/// Identity used to recognise the same real-world event seen by two clients.
/// Clocks are bucketed to whole seconds because two recordings of one battle
/// time the same moment slightly differently.
fn dedup_key(event: &TimelineEvent) -> (u8, String, i64) {
    let bucket = event.clock.seconds().round() as i64;
    match &event.kind {
        TimelineEventKind::HealthLost { ship_name, player_name, .. } => {
            (0, format!("{ship_name}/{player_name}"), bucket)
        }
        TimelineEventKind::Death { ship_name, player_name, .. } => (1, format!("{ship_name}/{player_name}"), bucket),
        TimelineEventKind::CapContested { cap_label, .. } => (2, cap_label.clone(), bucket),
        TimelineEventKind::CapFlipped { cap_label, .. } => (3, cap_label.clone(), bucket),
        TimelineEventKind::CapBeingCaptured { cap_label, .. } => (4, cap_label.clone(), bucket),
        TimelineEventKind::RadarUsed { ship_name, player_name, .. } => {
            (5, format!("{ship_name}/{player_name}"), bucket)
        }
        TimelineEventKind::AdvantageChanged { label, .. } => (6, label.clone(), bucket),
        TimelineEventKind::Disconnected { ship_name, player_name, .. } => {
            (7, format!("{ship_name}/{player_name}"), bucket)
        }
    }
}

/// Union the primary scan with alternate-perspective scans of the same battle.
///
/// The primary is inserted first and wins every collision, so its strings and
/// ordering stay authoritative. Alternate `AdvantageChanged` events are dropped
/// outright: the advantage calculation swaps team indices so index 0 is the
/// scanning client's team, and its label is baked from that perspective, so
/// keeping both streams would emit contradictory events at one timestamp.
pub(crate) fn merge_timelines(
    primary: TimelineExtractionResult,
    alts: Vec<TimelineExtractionResult>,
) -> TimelineExtractionResult {
    let TimelineExtractionResult { mut events, battle_start, battle_end, viewer_team } = primary;

    let mut seen: HashSet<(u8, String, i64)> = events.iter().map(dedup_key).collect();

    for alt in alts {
        for event in alt.events {
            if matches!(event.kind, TimelineEventKind::AdvantageChanged { .. }) {
                continue;
            }
            if seen.insert(dedup_key(&event)) {
                events.push(event);
            }
        }
    }

    events.sort_by(|a, b| a.clock.cmp(&b.clock));

    TimelineExtractionResult { events, battle_start, battle_end, viewer_team }
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tests").join("fixtures").join("replays")
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

    fn team_label(team: Option<TeamId>) -> String {
        match team {
            Some(t) => t.to_string(),
            None => "none".to_string(),
        }
    }

    fn event_kind_label(kind: &TimelineEventKind) -> String {
        match kind {
            TimelineEventKind::HealthLost { ship_name, player_name, team, percent_lost, new_hp, .. } => {
                format!(
                    "HealthLost({ship_name}/{player_name} team={team} pct={} new_hp={})",
                    (percent_lost * 1000.0).round() as i64,
                    new_hp.round() as i64,
                )
            }
            TimelineEventKind::Death { ship_name, player_name, team, .. } => {
                format!("Death({ship_name}/{player_name} team={team})")
            }
            TimelineEventKind::CapContested { cap_label, owner_team } => {
                format!("CapContested({cap_label} team={})", team_label(*owner_team))
            }
            TimelineEventKind::CapFlipped { cap_label, capturer_team } => {
                format!("CapFlipped({cap_label} team={capturer_team})")
            }
            TimelineEventKind::CapBeingCaptured { cap_label, capturer_team } => {
                format!("CapBeingCaptured({cap_label} team={capturer_team})")
            }
            TimelineEventKind::RadarUsed { ship_name, player_name, team } => {
                format!("RadarUsed({ship_name}/{player_name} team={team})")
            }
            TimelineEventKind::AdvantageChanged { label, is_friendly } => {
                format!("AdvantageChanged({label} friendly={is_friendly})")
            }
            TimelineEventKind::Disconnected { ship_name, player_name, team } => {
                format!("Disconnected({ship_name}/{player_name} team={team})")
            }
        }
    }

    #[test]
    #[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
    fn timeline_and_shots_golden() {
        let (provider, constants) = load_build_resources(11965230);

        let fixture = fixtures_dir().join("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
        let replay =
            ReplayFile::from_file(&fixture).unwrap_or_else(|e| panic!("failed to load Vermont fixture: {e:?}"));

        let (result, shots) = extract_timeline_and_shots(&replay, &provider, Some(&constants));

        let mut events: Vec<EventSnapshot> = result
            .events
            .iter()
            .map(|e| EventSnapshot { clock_s: r3(e.clock.seconds()), kind: event_kind_label(&e.kind) })
            .collect();
        events.sort_by(|a, b| a.clock_s.total_cmp(&b.clock_s).then(a.kind.cmp(&b.kind)));

        let mut shot_counts: Vec<ShotCountRow> = shots
            .iter()
            .filter(|(_, tl)| !tl.hits.is_empty())
            .map(|(&eid, tl)| ShotCountRow { entity_id: eid.raw(), shell_count: tl.hits.len() })
            .collect();
        shot_counts.sort_by_key(|r| r.entity_id);

        let mut health_histories: Vec<HealthHistoryRow> = shots
            .iter()
            .filter(|(_, tl)| !tl.health_history.is_empty())
            .map(|(&eid, tl)| {
                let hh = &tl.health_history;
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
                let mut hit_type_counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
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

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn death(secs: f32, ship: &str, team: i64) -> TimelineEvent {
        TimelineEvent {
            clock: ElapsedClock(secs),
            kind: TimelineEventKind::Death {
                ship_name: ship.to_owned(),
                player_name: "p".to_owned(),
                team: TeamId::new(team),
                killer_ship: String::new(),
                killer_player: String::new(),
            },
        }
    }

    fn advantage(secs: f32, label: &str) -> TimelineEvent {
        TimelineEvent {
            clock: ElapsedClock(secs),
            kind: TimelineEventKind::AdvantageChanged { label: label.to_owned(), is_friendly: true },
        }
    }

    fn result(events: Vec<TimelineEvent>, viewer_team: i64) -> TimelineExtractionResult {
        TimelineExtractionResult {
            events,
            battle_start: GameClock(0.0),
            battle_end: None,
            viewer_team: Some(TeamId::new(viewer_team)),
        }
    }

    #[test]
    fn identical_events_from_both_perspectives_collapse_to_one() {
        let merged = merge_timelines(
            result(vec![death(100.0, "Yamato", 1)], 0),
            vec![result(vec![death(100.0, "Yamato", 1)], 1)],
        );
        assert_eq!(merged.events.len(), 1);
    }

    #[test]
    fn near_simultaneous_duplicates_collapse_but_distinct_ones_do_not() {
        // Two clients time the same kill slightly differently; 0.4s apart is
        // the same event, 2.0s apart is not.
        let merged = merge_timelines(
            result(vec![death(100.0, "Yamato", 1)], 0),
            vec![result(vec![death(100.4, "Yamato", 1), death(102.0, "Yamato", 1)], 1)],
        );
        assert_eq!(merged.events.len(), 2);
    }

    #[test]
    fn events_only_the_alt_saw_are_kept() {
        let merged = merge_timelines(
            result(vec![death(100.0, "Yamato", 1)], 0),
            vec![result(vec![death(150.0, "Gearing", 0)], 1)],
        );
        assert_eq!(merged.events.len(), 2);
    }

    #[test]
    fn alt_advantage_events_are_dropped() {
        // Advantage is computed relative to the scanning client, so the alt's
        // stream would contradict the primary's at the same timestamps.
        let merged = merge_timelines(
            result(vec![advantage(100.0, "Major advantage gained")], 0),
            vec![result(vec![advantage(100.0, "Dropped to Major advantage")], 1)],
        );
        assert_eq!(merged.events.len(), 1);
        match &merged.events[0].kind {
            TimelineEventKind::AdvantageChanged { label, .. } => {
                assert_eq!(label, "Major advantage gained");
            }
            other => panic!("expected AdvantageChanged, got {other:?}"),
        }
    }

    #[test]
    fn output_is_sorted_by_clock() {
        let merged = merge_timelines(
            result(vec![death(300.0, "A", 0)], 0),
            vec![result(vec![death(100.0, "B", 1), death(200.0, "C", 1)], 1)],
        );
        let clocks: Vec<f32> = merged.events.iter().map(|e| e.clock.seconds()).collect();
        assert_eq!(clocks, vec![100.0, 200.0, 300.0]);
    }

    #[test]
    fn primary_viewer_team_is_preserved() {
        let merged = merge_timelines(result(vec![], 0), vec![result(vec![], 1)]);
        assert_eq!(merged.viewer_team, Some(TeamId::new(0)));
    }
}

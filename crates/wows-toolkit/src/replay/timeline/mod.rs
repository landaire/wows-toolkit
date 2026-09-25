//! The timeline as the egui app reaches it.
//!
//! The events themselves are derived in `wows_replay_insights::timeline`, which
//! both front ends read; only the egui list and its filter bar live here.

pub(crate) mod ui;

pub(crate) use wows_replay_insights::timeline::*;

#[cfg(test)]
mod extraction_snapshots {
    use super::*;
    use std::path::PathBuf;

    use wows_replays::ReplayFile;
    use wows_replays::game_constants::GameConstants;
    use wowsunpack::game_params::provider::GameMetadataProvider;
    use wowsunpack::game_types::TeamId;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tests").join("fixtures").join("replays")
    }

    fn load_build_resources(build: u32) -> (GameMetadataProvider, GameConstants) {
        let dump = wows_data_mgr::dump_for_build(build)
            .unwrap_or_else(|| panic!("game data for build {} not available", build));
        let vfs = dump.vfs();
        let cached = dump.derived_path("game_params.rkyv").and_then(|path| wowsunpack::game_params::cache::load(&path));
        let provider = match cached {
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
            TimelineEventKind::CapContested { cap_label, owner_team, .. } => {
                format!("CapContested({cap_label} team={})", team_label(*owner_team))
            }
            TimelineEventKind::CapFlipped { cap_label, capturer_team, .. } => {
                format!("CapFlipped({cap_label} team={capturer_team})")
            }
            TimelineEventKind::CapBeingCaptured { cap_label, capturer_team, .. } => {
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

//! Egui-free normalized battle report and the raw->named results resolution
//! that feeds it.

mod build;
mod resolve;
mod results;

pub use build::*;
pub use resolve::resolve_battle_results;
pub use results::*;

use std::collections::HashMap;

use serde_json::Value;
use wows_battle_world::report::BattleReport;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::recognized::Recognized;

use crate::personal_rating::PersonalRatingResult;

/// Match-level metadata, mirroring the fields the replay export emits.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MatchMetadata {
    pub map: String,
    pub game_mode: String,
    pub game_type: String,
    pub match_group: String,
    pub version: Version,
    pub max_duration: u32,
    pub played_duration: Option<f32>,
    pub extra_duration: Option<f32>,
    pub timestamp: jiff::Timestamp,
    pub battle_result: Option<BattleResult>,
}

/// One player's normalized identity, ship, and results. Presentation (colors,
/// hover text, locale-formatted labels) is rebuilt by consumers from these
/// values.
#[derive(Clone, Debug, serde::Serialize)]
pub struct NormalizedPlayer {
    pub db_id: AccountId,
    pub name: String,
    pub clan: String,
    /// Clan-league color as packed `0xRRGGBB`; `0` when clanless. Computed
    /// identically to the export's `Player::from`.
    pub clan_color_rgb: u32,
    pub realm: Option<String>,
    pub division_id: Option<u32>,
    pub division_label: Option<String>,
    pub team_id: u32,
    pub relation: Relation,
    pub is_self: bool,
    pub is_bot: bool,
    pub is_abuser: bool,
    pub ship_index: String,
    pub ship_name: String,
    pub ship_nation: String,
    pub ship_class: Species,
    pub ship_tier: u32,
    pub is_test_ship: bool,
    /// Server-provided results. `Some` only when the resolved player object
    /// carries a `damage` key (mirrors the export's server-results gate).
    pub server_results: Option<ServerResults>,
    pub observed_results: ObservedResults,
    pub skill_info: SkillInfo,
    pub build: Option<TranslatedBuild>,
    pub achievements: Vec<AchievementResult>,
    pub ribbons: Vec<RibbonResult>,
    pub consumables: Vec<ConsumableResult>,
    pub heal_count: Option<u32>,
    pub personal_rating: Option<PersonalRatingResult>,
    pub time_lived_secs: Option<u64>,
}

/// A fully normalized, egui-free battle report.
#[derive(Clone, Debug, serde::Serialize)]
pub struct NormalizedBattleReport {
    pub metadata: MatchMetadata,
    pub players: Vec<NormalizedPlayer>,
}

impl NormalizedBattleReport {
    pub fn from_battle_report(
        report: &BattleReport,
        meta: &ReplayMeta,
        provider: &GameMetadataProvider,
        constants: &Value,
    ) -> NormalizedBattleReport {
        let resolved_results: Option<Value> = report
            .battle_results()
            .and_then(|s| serde_json::from_str(s).ok())
            .map(|raw| resolve_battle_results(raw, constants));

        let mut players: Vec<NormalizedPlayer> = report
            .players()
            .iter()
            .map(|player| {
                let player_results = resolved_results
                    .as_ref()
                    .and_then(|r| r.pointer(&format!("/playersPublicInfo/{}", player.initial_state().db_id())));
                build_player(player, report, provider, player_results)
            })
            .collect();

        attribute_received_damage(&mut players);
        compute_inverse_percentages(&mut players);

        let metadata = MatchMetadata {
            map: report.map_name().to_string(),
            game_mode: report.game_mode().to_string(),
            game_type: report.game_type().to_string(),
            match_group: report.match_group().to_string(),
            version: report.version(),
            max_duration: report.max_duration(),
            played_duration: report.played_duration(),
            extra_duration: report.extra_duration(),
            timestamp: replay_timestamp(meta),
            battle_result: report.battle_result().cloned(),
        };

        NormalizedBattleReport { metadata, players }
    }
}

/// Parse the replay's `dateTime` field into a timestamp. Ported from the
/// toolkit's `util::replay_timestamp` so insights carries no toolkit dependency.
fn replay_timestamp(meta: &ReplayMeta) -> jiff::Timestamp {
    const REPLAY_DATE_FORMAT: &str = "%d.%m.%Y %H:%M:%S";

    jiff::civil::DateTime::strptime(REPLAY_DATE_FORMAT, &meta.dateTime)
        .expect("failed to parse replay timestamp")
        .to_zoned(jiff::tz::TimeZone::system())
        .expect("failed to convert DateTime to zoned time")
        .into()
}

/// Packed `0xRRGGBB` team color, matching the toolkit's
/// `player_color_for_team_relation` (egui `Color32::WHITE` / `LIGHT_GREEN` /
/// `LIGHT_RED`). Kept in sync by value since insights is egui-free.
fn team_relation_rgb(relation: Relation) -> u32 {
    if relation.is_self() {
        0xFFFFFF
    } else if relation.is_ally() {
        0x90EE90
    } else {
        0xFF8080
    }
}

/// Clan-league color as packed `0xRRGGBB`, computed exactly as the export's
/// `Player::from`: `0` when clanless, else `clanColor & 0xFFFFFF`, else the team
/// color for replays that omit `clanColor`.
fn clan_color_rgb(player: &Player) -> u32 {
    let state = player.initial_state();
    if state.clan().is_empty() {
        return 0;
    }
    match state.raw_with_names().get("clanColor").and_then(|c| c.as_i64()) {
        Some(clan_color) => (clan_color & 0xFFFFFF) as u32,
        None => {
            tracing::warn!("player '{}' has no clanColor; using team color", state.username());
            team_relation_rgb(player.relation())
        }
    }
}

/// Sum of self-player controller damage for a category, used as the spotting
/// fallback when the server results omit `scouting_damage`. Ported from the
/// numeric part of the toolkit's `build_damage_stat_total`.
fn damage_stat_total(stats: &[DamageStatEntry], category: DamageStatCategory) -> Option<u64> {
    let mut total = 0.0f64;
    for entry in stats {
        if entry.category == Recognized::Known(category) {
            total += entry.total;
        }
    }
    if total > 0.0 { Some(total as u64) } else { None }
}

/// Build the 9-field per-type damage breakdown from a key lookup. A key mapping
/// to a zero or missing value yields `None`, matching the original `>0` filter.
fn damage_breakdown<F: Fn(&str) -> Option<u64>>(get: F) -> Damage {
    let pick = |key: &str| get(key).filter(|n| *n > 0);
    Damage {
        ap: pick(DAMAGE_MAIN_AP),
        sap: pick(DAMAGE_MAIN_CS),
        he: pick(DAMAGE_MAIN_HE),
        he_secondaries: pick(DAMAGE_ATBA_HE),
        sap_secondaries: pick(DAMAGE_ATBA_CS),
        torps: pick(DAMAGE_TPD_NORMAL),
        deep_water_torps: pick(DAMAGE_TPD_DEEP),
        fire: pick(DAMAGE_FIRE),
        flooding: pick(DAMAGE_FLOOD),
    }
}

/// Read an integer field that may be encoded as an integer or a float (potential
/// damage keys are floats in some result formats).
fn get_u64_or_f64(v: &Value, key: &str) -> Option<u64> {
    let field = v.get(key)?;
    field.as_u64().or_else(|| field.as_f64().map(|f| f as u64))
}

/// Purely-JSON per-player numeric extraction. Returns `Some` only when the
/// resolved player object carries a `damage` key (the server-results gate).
/// Needs no game data, so it is unit-testable offline.
fn extract_server_results(pr: &Value) -> Option<ServerResults> {
    let damage_number = pr.get("damage").and_then(|v| v.as_u64())?;

    let damage_details = damage_breakdown(|k| pr.get(k).and_then(|v| v.as_u64()));

    let hit = |key: &str| pr.get(key).and_then(|v| v.as_u64()).filter(|n| *n > 0);
    let hits_details = Hits {
        ap: hit(HITS_MAIN_AP),
        sap: hit(HITS_MAIN_CS),
        he: hit(HITS_MAIN_HE),
        he_secondaries: hit(HITS_ATBA_HE),
        sap_secondaries: hit(HITS_ATBA_CS),
        ap_secondaries_manual: hit(HITS_ATBA_AP_MANUAL),
        he_secondaries_manual: hit(HITS_ATBA_HE_MANUAL),
        sap_secondaries_manual: hit(HITS_ATBA_CS_MANUAL),
        torps: hit(HITS_TPD_NORMAL),
    };

    let mut potential_damage = 0u64;
    for (key, _) in POTENTIAL_DAMAGE_DESCRIPTIONS {
        if let Some(num) = get_u64_or_f64(pr, key)
            && num > 0
        {
            potential_damage += num;
        }
    }
    let potential_damage_details = PotentialDamage {
        artillery: get_u64_or_f64(pr, "agro_art").filter(|n| *n > 0).unwrap_or_default(),
        torpedoes: get_u64_or_f64(pr, "agro_tpd").filter(|n| *n > 0).unwrap_or_default(),
        planes: get_u64_or_f64(pr, "agro_air").filter(|n| *n > 0).unwrap_or_default(),
    };

    let mut received_damage = 0u64;
    for (key, _) in RECEIVED_DAMAGE_DESCRIPTIONS {
        if let Some(num) = pr.get(format!("received_{key}").as_str()).and_then(|v| v.as_u64())
            && num > 0
        {
            received_damage += num;
        }
    }
    let received_damage_details =
        damage_breakdown(|k| pr.get(format!("received_{k}").as_str()).and_then(|v| v.as_u64()));

    let mut damage_interactions: HashMap<AccountId, DamageInteraction> = HashMap::new();
    let mut fires_dealt = 0u64;
    let mut floods_dealt = 0u64;
    let mut citadels_dealt = 0u64;
    let mut crits_dealt = 0u64;

    if let Some(interactions) = pr.get("interactions").and_then(|v| v.as_object()) {
        for (victim, victim_data) in interactions {
            let victim_id = AccountId(victim.parse::<i64>().unwrap_or_default());

            fires_dealt += victim_data.get("fires").and_then(|v| v.as_u64()).unwrap_or(0);
            floods_dealt += victim_data.get("floods").and_then(|v| v.as_u64()).unwrap_or(0);
            citadels_dealt += victim_data.get("citadels").and_then(|v| v.as_u64()).unwrap_or(0);
            crits_dealt += victim_data.get("crits").and_then(|v| v.as_u64()).unwrap_or(0);

            let mut interaction = DamageInteraction::default();
            let all_damage: u64 = DAMAGE_DESCRIPTIONS
                .iter()
                .map(|(key, _)| victim_data.get(*key).and_then(|v| v.as_u64()).unwrap_or(0))
                .sum();
            interaction.damage_dealt = all_damage;
            interaction.damage_dealt_by_type = damage_breakdown(|k| victim_data.get(k).and_then(|v| v.as_u64()));
            if interaction.damage_dealt > 0 {
                interaction.damage_dealt_percentage = (all_damage as f64 / damage_number as f64) * 100.0;
            }

            damage_interactions.insert(victim_id, interaction);
        }
    }

    Some(ServerResults {
        xp: pr.get("exp").and_then(|v| v.as_i64()).unwrap_or_default(),
        raw_xp: pr.get("raw_exp").and_then(|v| v.as_i64()).unwrap_or_default(),
        damage: damage_number,
        damage_details,
        hits_details,
        spotting_damage: pr.get("scouting_damage").and_then(|v| v.as_u64()).unwrap_or_default(),
        potential_damage,
        potential_damage_details,
        received_damage,
        received_damage_details,
        fires_dealt,
        floods_dealt,
        citadels_dealt,
        crits_dealt,
        distance_traveled: pr.get("distance").and_then(|v| v.as_f64()).unwrap_or_default(),
        kills: pr.get("ships_killed").and_then(|v| v.as_i64()).unwrap_or_default(),
        damage_interactions,
    })
}

/// Resolve a player's equipped consumables and tally activations against each
/// slot. Ported from the toolkit's `resolve_player_consumables` (result-only).
fn resolve_player_consumables(
    player: &Player,
    provider: &GameMetadataProvider,
    version: Version,
    activations: &[ActiveConsumable],
) -> (Vec<ConsumableResult>, Option<u32>) {
    let Some(build) = crate::build::ResolvedBuild::from_player(player, provider, version) else {
        return (Vec::new(), None);
    };

    let mut charges_used: Vec<u32> = vec![0; build.slots.len()];
    for activation in activations {
        let pick = build.slots.iter().enumerate().find(|(idx, slot)| {
            if slot.consumable_type.known() != activation.consumable.known() {
                return false;
            }
            match slot.total_charges {
                wowsunpack::game_types::ChargeCount::Unlimited => true,
                wowsunpack::game_types::ChargeCount::Finite(total) => charges_used[*idx] < total,
            }
        });
        if let Some((idx, _)) = pick {
            charges_used[idx] = charges_used[idx].saturating_add(1);
        }
    }

    let mut heal_count: Option<u32> = None;
    for (slot, used) in build.slots.iter().zip(charges_used.iter()) {
        if slot.consumable_type.known() == Some(&wowsunpack::game_types::Consumable::RepairParty) {
            heal_count = Some(heal_count.unwrap_or(0).saturating_add(*used));
        }
    }

    let consumables = build
        .slots
        .iter()
        .zip(charges_used)
        .map(|(slot, used)| {
            let display_name = wowsunpack::game_params::translations::translate_consumable(&slot.icon_key, provider)
                .unwrap_or_else(|| slot.consumable_type_raw.clone());
            let description =
                wowsunpack::game_params::translations::translate_consumable_description(&slot.icon_key, provider)
                    .unwrap_or_default();
            ConsumableResult {
                display_name,
                description,
                icon_key: slot.icon_key.clone(),
                charges_used: used,
                total_charges: slot.total_charges,
            }
        })
        .collect();

    (consumables, heal_count)
}

/// Extract one normalized player. Numeric derivations mirror the toolkit's
/// `UiReport::new` exactly; presentation (colors, labels, hover text) is dropped.
fn build_player(
    player: &Player,
    report: &BattleReport,
    provider: &GameMetadataProvider,
    player_results: Option<&Value>,
) -> NormalizedPlayer {
    let vehicle = player.vehicle_entity();
    let state = player.initial_state();
    let vehicle_param = player.vehicle();
    let relation = player.relation();
    let is_self = relation.is_self();

    let species = vehicle_param.species().and_then(|r| r.known().cloned()).expect("ship has no species?");

    let ship_name =
        provider.localized_name_from_param(vehicle_param).unwrap_or_else(|| format!("{}", vehicle_param.id()));

    let is_test_ship =
        vehicle_param.data().vehicle_ref().map(|vehicle| vehicle.group().starts_with("demo")).unwrap_or_default();

    let observed_damage = vehicle.map(|v| v.damage().ceil() as u64).unwrap_or(0);
    let observed_kills = vehicle.map(|v| v.frags().len() as i64).unwrap_or(0);

    let (skill_points, num_skills, highest_tier, num_tier_1_skills) = vehicle
        .and_then(|v| v.commander_skills(species))
        .map(|skills| {
            let points =
                skills.iter().fold(0usize, |accum, skill| accum + skill.tier().get_for_species(species).get() as usize);
            let highest_tier = skills.iter().map(|skill| skill.tier().get_for_species(species).get() as usize).max();
            let num_tier_1_skills = skills.iter().fold(0, |mut accum, skill| {
                if skill.tier().get_for_species(species).get() as usize == 1 {
                    accum += 1;
                }
                accum
            });

            (points, skills.len(), highest_tier.unwrap_or(0), num_tier_1_skills)
        })
        .unwrap_or((0, 0, 0, 0));
    let skill_info = SkillInfo { skill_points, num_skills, highest_tier, num_tier_1_skills };

    let mut server_results = player_results.and_then(extract_server_results);
    if let Some(sr) = server_results.as_mut() {
        // Self-player spotting falls back to controller totals when the server
        // results omit the scouting_damage key.
        if is_self && player_results.and_then(|pr| pr.get("scouting_damage")?.as_u64()).is_none() {
            sr.spotting_damage = damage_stat_total(report.self_damage_stats(), DamageStatCategory::Spot).unwrap_or(0);
        }
    }

    let achievements = player_results
        .and_then(|pr| pr.get("achievements")?.as_array())
        .map(|achievements_array| {
            achievements_array
                .iter()
                .filter_map(|achievement_info| {
                    // Index defensively: some result formats carry empty or
                    // short arrays, so a malformed entry is skipped, not panicked.
                    let achievement_info = achievement_info.as_array()?;
                    let achievement_id = achievement_info.first()?.as_u64()?;
                    let achievement_count = achievement_info.get(1)?.as_u64()?;

                    let game_param = <GameMetadataProvider as GameParamProvider>::game_param_by_id(
                        provider,
                        (achievement_id as u32).into(),
                    )?;

                    let ParamData::Achievement(achievement_data) = game_param.data() else {
                        return None;
                    };

                    let ui_name = achievement_data.ui_name().to_string();
                    let display_name =
                        wowsunpack::game_params::translations::translate_achievement_name(&ui_name, provider)?;
                    let description =
                        wowsunpack::game_params::translations::translate_achievement_description(&ui_name, provider)?;

                    Some(AchievementResult {
                        name: game_param.name().to_string(),
                        display_name,
                        description,
                        icon_key: ui_name,
                        count: achievement_count as usize,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Ribbon keys start with RIBBON_ in the resolved object; iteration order
    // follows the resolved map (sorted by key), so the Vec is deterministic.
    let ribbons = player_results
        .and_then(|pr| pr.as_object())
        .map(|pr_obj| {
            let mut ribbons = Vec::new();
            for (key, value) in pr_obj {
                if !key.starts_with("RIBBON_") {
                    continue;
                }
                let count = value.as_u64().unwrap_or(0);
                if count == 0 {
                    continue;
                }
                let Some(ribbon_translation) = wowsunpack::game_params::translations::translate_ribbon(key, provider)
                else {
                    continue;
                };
                ribbons.push(RibbonResult {
                    name: key.to_string(),
                    display_name: ribbon_translation.display_name,
                    description: ribbon_translation.description,
                    icon_key: ribbon_translation.icon_key,
                    is_subribbon: ribbon_translation.is_subribbon,
                    count,
                });
            }
            ribbons
        })
        .unwrap_or_default();

    let (consumables, heal_count) = resolve_player_consumables(
        player,
        provider,
        report.version(),
        report.active_consumables().get(&state.entity_id()).map(Vec::as_slice).unwrap_or(&[]),
    );

    let time_lived_secs = vehicle.and_then(|v| v.death_info()).map(|death_info| death_info.time_lived().as_secs());

    let division_label = report.divisions().get(&state.entity_id()).copied().map(|div| format!("({div})"));
    let division_id = if state.division_id() > 0 { Some(state.division_id() as u32) } else { None };

    NormalizedPlayer {
        db_id: state.db_id(),
        name: state.username().to_string(),
        clan: state.clan().to_string(),
        clan_color_rgb: clan_color_rgb(player),
        realm: state.realm().map(str::to_owned),
        division_id,
        division_label,
        team_id: state.team_id() as u32,
        relation,
        is_self,
        is_bot: state.is_bot(),
        is_abuser: state.is_abuser(),
        ship_index: vehicle_param.index().to_string(),
        ship_name,
        ship_nation: vehicle_param.nation().to_string(),
        ship_class: species,
        ship_tier: vehicle_param.data().vehicle_ref().expect("no vehicle ref").level(),
        is_test_ship,
        server_results,
        observed_results: ObservedResults { damage: observed_damage, kills: observed_kills },
        skill_info,
        build: TranslatedBuild::new(player, provider, &report.version()),
        achievements,
        ribbons,
        consumables,
        heal_count,
        personal_rating: None,
        time_lived_secs,
    }
}

/// Second pass: attribute each player's received damage from the per-victim
/// interactions the attackers recorded, filling the received side and its
/// per-type breakdown. Mirrors the two received-damage passes in `UiReport::new`.
fn attribute_received_damage(players: &mut [NormalizedPlayer]) {
    // victim db_id -> attacker db_id -> (damage dealt to victim, per-type breakdown)
    let mut all_received: HashMap<AccountId, HashMap<AccountId, (u64, Damage)>> = HashMap::new();

    for this in players.iter() {
        let this_id = this.db_id;
        for attacker in players.iter() {
            let attacker_id = attacker.db_id;
            if attacker_id == this_id {
                continue;
            }
            let Some(sr) = attacker.server_results.as_ref() else {
                continue;
            };
            let Some(interaction) = sr.damage_interactions.get(&this_id) else {
                continue;
            };
            if interaction.damage_dealt == 0 {
                continue;
            }
            all_received
                .entry(this_id)
                .or_default()
                .insert(attacker_id, (interaction.damage_dealt, interaction.damage_dealt_by_type.clone()));
        }
    }

    for player in players.iter_mut() {
        let this_id = player.db_id;
        let Some(received_map) = all_received.remove(&this_id) else {
            continue;
        };
        let Some(sr) = player.server_results.as_mut() else {
            continue;
        };

        // Sum from per-interaction attacker data so all damage types are
        // counted consistently in both numerator and denominator.
        let total_received: u64 = received_map.values().map(|(dmg, _)| *dmg).sum();

        for (attacker_id, (received_damage, by_type)) in received_map {
            let interaction = sr.damage_interactions.entry(attacker_id).or_default();
            interaction.damage_received = received_damage;
            interaction.damage_received_by_type = by_type;
            if total_received > 0 {
                interaction.damage_received_percentage = (received_damage as f64 / total_received as f64) * 100.0;
            }
        }
    }
}

/// Third pass: compute inverse percentages against the other player's totals.
/// `dealt_inverse` = dealt / victim's total received; `received_inverse` =
/// received / attacker's total dealt.
fn compute_inverse_percentages(players: &mut [NormalizedPlayer]) {
    let totals: HashMap<AccountId, (u64, u64)> = players
        .iter()
        .map(|player| {
            let dealt = player.server_results.as_ref().map(|sr| sr.damage).unwrap_or_default();
            let received: u64 = player
                .server_results
                .as_ref()
                .map(|sr| sr.damage_interactions.values().map(|i| i.damage_received).sum())
                .unwrap_or_default();
            (player.db_id, (dealt, received))
        })
        .collect();

    for player in players.iter_mut() {
        let Some(sr) = player.server_results.as_mut() else {
            continue;
        };
        for (other_id, interaction) in sr.damage_interactions.iter_mut() {
            let Some(&(other_dealt, other_received)) = totals.get(other_id) else {
                continue;
            };

            if other_received > 0 && interaction.damage_dealt > 0 {
                interaction.damage_dealt_inverse_percentage =
                    (interaction.damage_dealt as f64 / other_received as f64) * 100.0;
            }

            if other_dealt > 0 && interaction.damage_received > 0 {
                interaction.damage_received_inverse_percentage =
                    (interaction.damage_received as f64 / other_dealt as f64) * 100.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_server_results_reads_numbers_and_per_type_interactions() {
        let pr = json!({
            "damage": 50000u64,
            "exp": 1500,
            "raw_exp": 1200,
            "damage_main_ap": 30000u64,
            "damage_main_he": 20000u64,
            "hits_main_ap": 12u64,
            "hits_main_he": 34u64,
            "scouting_damage": 8000u64,
            "agro_art": 100000u64,
            "agro_tpd": 5000u64,
            "received_damage_main_he": 4000u64,
            "received_damage_fire": 1000u64,
            "distance": 42.5,
            "ships_killed": 2,
            "interactions": {
                "111": { "damage_main_ap": 30000u64, "damage_fire": 2000u64, "fires": 3, "citadels": 1 }
            }
        });

        let sr = extract_server_results(&pr).expect("damage key present should yield Some");

        assert_eq!(sr.damage, 50000);
        assert_eq!(sr.xp, 1500);
        assert_eq!(sr.raw_xp, 1200);
        assert_eq!(sr.damage_details.ap, Some(30000));
        assert_eq!(sr.damage_details.he, Some(20000));
        assert_eq!(sr.hits_details.ap, Some(12));
        assert_eq!(sr.hits_details.he, Some(34));
        assert_eq!(sr.spotting_damage, 8000);
        assert_eq!(sr.potential_damage, 105000);
        assert_eq!(sr.potential_damage_details.artillery, 100000);
        assert_eq!(sr.potential_damage_details.torpedoes, 5000);
        assert_eq!(sr.potential_damage_details.planes, 0);
        assert_eq!(sr.received_damage, 5000);
        assert_eq!(sr.received_damage_details.he, Some(4000));
        assert_eq!(sr.received_damage_details.fire, Some(1000));
        assert_eq!(sr.distance_traveled, 42.5);
        assert_eq!(sr.kills, 2);
        assert_eq!(sr.fires_dealt, 3);
        assert_eq!(sr.citadels_dealt, 1);

        let interaction = sr.damage_interactions.get(&AccountId(111)).expect("victim 111 interaction present");
        assert_eq!(interaction.damage_dealt, 32000);
        assert_eq!(interaction.damage_dealt_by_type.ap, Some(30000));
        assert_eq!(interaction.damage_dealt_by_type.fire, Some(2000));
        assert!((interaction.damage_dealt_percentage - 64.0).abs() < 1e-9);
    }

    #[test]
    fn extract_server_results_requires_damage_key() {
        let pr = json!({ "exp": 10, "interactions": {} });
        assert!(extract_server_results(&pr).is_none());
    }
}

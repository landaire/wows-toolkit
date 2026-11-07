use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use wows_battle_world::process::ProcessOptions;
use wows_battle_world::process::battle_report_for;
use wows_core::units::Km;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::NormalizedPlayer;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::context::GameDataContext;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wowsunpack::data::Version;
use wowsunpack::game_params::translations::translate_game_mode;
use wowsunpack::game_params::translations::translate_map_name;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::DeathCause;
use wowsunpack::recognized::Recognized;

use crate::error::AnalysisError;

/// What a replay records about its match: the match itself, every player
/// with their ship and results, each kill and the chat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayAnalysis {
    pub game: MatchFacts,
    pub players: Vec<PlayerSummary>,

    /// In the order they happened.
    pub kills: Vec<Kill>,
    pub chat: Vec<ChatLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchFacts {
    /// The client's version as `major.minor.patch.build`.
    pub client_version: String,
    pub map: String,
    pub game_mode: String,
    pub game_type: String,
    pub match_group: String,

    /// The header's local recording time, `dd.mm.yyyy hh:mm:ss`.
    pub date_time: String,
    pub arena_id: ArenaId,
    pub recorder: String,

    /// When the battle phase began. `None` when the recording ends before it.
    pub battle_start: Option<GameClock>,
    pub time_limit: Duration,

    /// Battle start to battle end. `None` when the recording stops before
    /// the battle ends.
    pub played: Option<Duration>,

    /// From the recorder's side. `None` when the recording stops before the
    /// battle ends and the replay carries no results.
    pub outcome: Option<Outcome>,

    /// Whether the server's results are in the replay. Without them only
    /// what the recorder's client observed is known.
    pub has_battle_results: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Win,
    Loss,
    Draw,
}

/// The recorder's view of a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Recorder,
    Ally,
    Enemy,
}

impl From<Relation> for Side {
    fn from(relation: Relation) -> Self {
        if relation.is_self() {
            Self::Recorder
        } else if relation.is_ally() {
            Self::Ally
        } else {
            Self::Enemy
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSummary {
    /// The display name: the username, or a bot's translated name.
    pub name: String,
    pub clan: Option<String>,
    pub side: Side,
    pub team_id: u32,
    pub is_bot: bool,
    pub account: AccountId,

    /// The ship's entity in the packet stream.
    pub ship_entity: EntityId,

    /// The player's avatar entity, which sends chat and some methods.
    pub avatar_entity: Option<EntityId>,
    pub division: Option<char>,
    pub ship: ShipFacts,

    /// The server's results. `None` when the replay carries none.
    pub results: Option<PlayerResults>,

    /// Damage and kills the recorder's client saw this player deal.
    pub observed_damage: u64,
    pub observed_kills: i64,

    /// `None` for a player who had no ship.
    pub survived: Option<bool>,
    pub time_lived: Option<Duration>,
    pub death: Option<Death>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShipFacts {
    pub name: String,

    /// The ship's GameParams index, such as `PJSD012`.
    pub index: String,
    pub param_id: GameParamId,
    pub tier: Option<u32>,
    pub class: Species,
    pub nation: String,
}

/// One player's figures from the server's battle results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerResults {
    pub base_xp: Option<i64>,
    pub raw_xp: Option<i64>,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub spotting_damage: Option<u64>,
    pub potential_damage: u64,
    pub received_damage: u64,
    pub main_battery_hits: Option<u64>,
    pub fires: Option<u64>,
    pub floods: Option<u64>,
    pub citadels: Option<u64>,
    pub crits: Option<u64>,

    pub distance: Option<Km>,

    /// Keyed by the results' damage and hit kinds, such as `DAMAGE_MAIN_AP`.
    pub damage_by_type: BTreeMap<String, u64>,
    pub hits_by_type: BTreeMap<String, u64>,
    pub received_by_type: BTreeMap<String, u64>,

    /// Damage this player dealt to and took from each other player, by
    /// account.
    pub interactions: Vec<Interaction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interaction {
    pub account: AccountId,
    pub damage_dealt: u64,
    pub damage_received: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Death {
    pub clock: Option<GameClock>,
    pub killer: Option<EntityId>,
    /// Absent when the game names no cause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Recognized<DeathCause>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kill {
    pub clock: Option<GameClock>,

    /// Ship entities; `None` when the killer is no player's ship, such as
    /// a sea mine.
    pub killer: Option<EntityId>,
    pub victim: EntityId,
    /// Absent when the game names no cause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Recognized<DeathCause>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatLine {
    pub clock: GameClock,
    pub sender: String,
    pub side: Option<Side>,
    pub channel: ChatChannel,
    pub text: String,
}

/// Parses `replay` with `ctx`'s game data. `results_constants` is the
/// build's wows-constants JSON, which names the fields of the server's
/// battle results; `None` leaves the results out.
pub fn analyze(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    results_constants: Option<&Value>,
) -> Result<ReplayAnalysis, AnalysisError> {
    let version = replay_version(replay)?;
    let provider = ctx.metadata_provider(&version)?;
    let report = battle_report_for(replay, ctx, ProcessOptions::default())?;

    // Without constants the results cannot be named; they are left out
    // rather than guessed.
    let constants = results_constants.cloned().unwrap_or(Value::Null);
    let normalized = NormalizedBattleReport::from_battle_report(&report, &replay.meta, &provider, &constants);
    let has_battle_results = results_constants.is_some() && report.battle_results().is_some();

    let players: Vec<PlayerSummary> = normalized
        .players
        .iter()
        .zip(report.players())
        .map(|(normalized, player)| {
            let state = player.initial_state();
            let death = player.vehicle_entity().and_then(|vehicle| vehicle.death_info()).map(|death| Death {
                clock: report.deaths_by_victim().get(&state.entity_id()).copied(),
                killer: death.killer(),
                cause: death.cause().cloned(),
            });

            PlayerSummary {
                name: normalized.display_name.clone(),
                clan: Some(normalized.clan.clone()).filter(|clan| !clan.is_empty()),
                side: normalized.relation.into(),
                team_id: normalized.team_id,
                is_bot: normalized.is_bot,
                account: normalized.db_id,
                ship_entity: state.entity_id(),
                avatar_entity: state.avatar_id().map(|avatar| EntityId::from(avatar.raw())),
                division: report.divisions().get(&state.entity_id()).copied(),
                ship: ShipFacts {
                    name: normalized.ship_name.clone(),
                    index: normalized.ship_index.clone(),
                    param_id: normalized.ship_id,
                    tier: normalized.ship_tier,
                    class: normalized.ship_class,
                    nation: normalized.ship_nation.clone(),
                },
                results: has_battle_results.then(|| player_results(normalized)).flatten(),
                observed_damage: normalized.observed_results.damage,
                observed_kills: normalized.observed_results.kills,
                survived: normalized.survived,
                time_lived: normalized.time_lived_secs.map(Duration::from_secs),
                death,
            }
        })
        .collect();

    let mut kills: Vec<Kill> = players
        .iter()
        .filter_map(|player| {
            let death = player.death.as_ref()?;
            Some(Kill {
                clock: death.clock,
                killer: death.killer,
                victim: player.ship_entity,
                cause: death.cause.clone(),
            })
        })
        .collect();
    kills.sort_by(|left, right| clock_seconds(left.clock).total_cmp(&clock_seconds(right.clock)));

    let sides: HashMap<&str, Side> = players.iter().map(|player| (player.name.as_str(), player.side)).collect();
    let chat = report
        .game_chat()
        .iter()
        .map(|message| ChatLine {
            clock: message.clock,
            sender: message.sender_name.clone(),
            side: message.sender_relation.map(Side::from).or_else(|| sides.get(message.sender_name.as_str()).copied()),
            channel: message.channel.clone(),
            text: message.message.clone(),
        })
        .collect();

    let game = MatchFacts {
        client_version: format!(
            "{}.{}.{}.{}",
            version.major,
            version.minor,
            version.patch,
            version.build_number().map_or_else(|| "unknown".to_owned(), |build| build.to_string())
        ),
        map: translate_map_name(&replay.meta.mapName, &*provider),
        game_mode: report.game_mode().to_owned(),
        game_type: match replay.meta.gameType.as_deref() {
            Some(game_type) => translate_game_mode(game_type, &*provider),
            None => report.game_type().to_string(),
        },
        match_group: report.match_group().to_owned(),
        date_time: replay.meta.dateTime.clone(),
        arena_id: report.arena_id(),
        recorder: replay.meta.playerName.clone(),
        battle_start: report.battle_start_clock(),
        time_limit: Duration::from_secs(u64::from(report.max_duration())),
        played: played_duration(report.played_duration()),
        outcome: normalized.metadata.resolved_battle_result().map(outcome),
        has_battle_results,
    };

    Ok(ReplayAnalysis { game, players, kills, chat })
}

/// The version in the replay's header.
pub fn replay_version(replay: &ReplayFile) -> Result<Version, AnalysisError> {
    Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| AnalysisError::UnparsableVersion { version: replay.meta.clientVersionFromExe.clone() })
}

fn player_results(player: &NormalizedPlayer) -> Option<PlayerResults> {
    let results = player.server_results.as_ref()?;

    let mut interactions: Vec<Interaction> = results
        .damage_interactions
        .iter()
        .map(|(account, interaction)| Interaction {
            account: *account,
            damage_dealt: interaction.damage_dealt,
            damage_received: interaction.damage_received,
        })
        .filter(|interaction| interaction.damage_dealt > 0 || interaction.damage_received > 0)
        .collect();
    // The account breaks ties: the results come in hash order.
    interactions.sort_by_key(|interaction| {
        (std::cmp::Reverse(interaction.damage_dealt + interaction.damage_received), interaction.account)
    });

    Some(PlayerResults {
        base_xp: results.xp,
        raw_xp: results.raw_xp,
        damage: results.damage,
        kills: results.kills,
        spotting_damage: player.spotting_damage(),
        potential_damage: results.potential_damage,
        received_damage: results.received_damage,
        main_battery_hits: results.hits,
        fires: results.fires_dealt,
        floods: results.floods_dealt,
        citadels: results.citadels_dealt,
        crits: results.crits_dealt,
        distance: results.distance_traveled.map(|km| Km::new(km as f32)),
        damage_by_type: results.damage_by_type.clone(),
        hits_by_type: results.hits_by_type.clone(),
        received_by_type: results.received_damage_by_type.clone(),
        interactions,
    })
}

/// `BattleResult` is already from the recorder's side.
fn outcome(result: BattleResult) -> Outcome {
    match result {
        BattleResult::Win(_) => Outcome::Win,
        BattleResult::Loss(_) => Outcome::Loss,
        BattleResult::Draw => Outcome::Draw,
    }
}

/// Unknown clocks sort first.
fn clock_seconds(clock: Option<GameClock>) -> f32 {
    clock.map_or(f32::MIN, GameClock::seconds)
}

/// `None` for a negative or NaN difference of packet clocks, which a crafted
/// replay can produce.
fn played_duration(seconds: Option<f32>) -> Option<Duration> {
    seconds.and_then(|seconds| Duration::try_from_secs_f32(seconds).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unordered_or_nan_clocks_have_no_played_duration() {
        assert_eq!(played_duration(Some(-3.0)), None);
        assert_eq!(played_duration(Some(f32::NAN)), None);
        assert_eq!(played_duration(Some(f32::INFINITY)), None);
        assert_eq!(played_duration(Some(90.5)), Some(Duration::from_millis(90_500)));
    }
}

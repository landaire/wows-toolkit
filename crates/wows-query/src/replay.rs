use serde::Serialize;
use wows_replays::ReplayFile;
use wowsunpack::game_params::translations::translate_game_mode;
use wowsunpack::game_params::translations::translate_map_name;

use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::ShipSummary;

/// Header facts about a replay. Reading the packet stream needs game data for
/// the replay's exact build and is not done here.
#[derive(Debug, Clone, Serialize)]
pub struct ReplaySummary {
    pub player: String,
    pub player_ship: String,
    pub map: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_type: Option<String>,

    pub client_version: String,
    pub date_time: String,
    pub duration_seconds: u32,
    pub players: Vec<ReplayPlayer>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayPlayer {
    pub name: String,
    pub team: Team,

    /// `None` when the ship is not in the loaded game data, as for ships
    /// removed since the replay was recorded.
    pub ship: Option<ShipSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    Recorder,
    Ally,
    Enemy,
    Other,
}

impl Team {
    /// `VehicleInfoMeta::relation` as the client writes it.
    fn from_relation(relation: u32) -> Self {
        match relation {
            0 => Self::Recorder,
            1 => Self::Ally,
            2 => Self::Enemy,
            _ => Self::Other,
        }
    }
}

impl GameData {
    /// Summarizes a `.wowsreplay` file's header.
    pub fn replay_summary(&self, bytes: &[u8]) -> Result<ReplaySummary, QueryError> {
        let meta =
            ReplayFile::meta_from_bytes(bytes).map_err(|report| QueryError::Replay(report.into_current_context()))?;

        let mut players: Vec<ReplayPlayer> = meta
            .vehicles
            .iter()
            .map(|vehicle| ReplayPlayer {
                name: vehicle.name.clone(),
                team: Team::from_relation(vehicle.relation),
                ship: self.ship_by_id(vehicle.shipId),
            })
            .collect();
        players.sort_by_key(|player| player.team);

        // The recorder's own entry carries the ship id; `playerVehicle` is a
        // hyphenated param name.
        let player_ship = players
            .iter()
            .find(|player| player.team == Team::Recorder)
            .and_then(|player| player.ship.as_ref())
            .map_or_else(|| meta.playerVehicle.clone(), |ship| ship.name.clone());

        Ok(ReplaySummary {
            player: meta.playerName.clone(),
            player_ship,
            map: translate_map_name(&meta.mapName, self.provider()),
            game_type: meta.gameType.as_deref().map(|game_type| translate_game_mode(game_type, self.provider())),
            client_version: meta.clientVersionFromExe.clone(),
            date_time: meta.dateTime.clone(),
            duration_seconds: meta.duration,
            players,
        })
    }
}

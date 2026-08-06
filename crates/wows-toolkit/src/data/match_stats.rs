//! Client for the shipbuilds `match_stats` API, which answers a match roster
//! with each player's win rates, PR and battle counts.

use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_replays::types::GameParamId;

/// A region the stats service covers. The service holds no data for any other
/// realm, so a roster from one is never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Eu,
    Na,
    Asia,
}

impl Region {
    /// The region a replay's realm string names, or `None` where the service
    /// has no data for it.
    pub fn from_realm(realm: &str) -> Option<Self> {
        match realm.to_ascii_lowercase().as_str() {
            "eu" => Some(Self::Eu),
            "na" => Some(Self::Na),
            "asia" => Some(Self::Asia),
            _ => None,
        }
    }

    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Eu => "eu",
            Self::Na => "na",
            Self::Asia => "asia",
        }
    }

    /// The form the region takes in a shipbuilds player URL.
    pub fn as_url_segment(self) -> &'static str {
        match self {
            Self::Eu => "EU",
            Self::Na => "NA",
            Self::Asia => "ASIA",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerRef {
    pub account_id: AccountId,
    pub region: Region,
    pub ship_id: GameParamId,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchStatsRequest {
    pub arena_id: ArenaId,
    pub players: Vec<PlayerRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerStatsStatus {
    Ok,
    Hidden,
    Unavailable,
    /// A status this client does not know. Kept so one unexpected value costs
    /// a single row rather than the whole response.
    #[serde(other)]
    Unknown,
}

/// One player's stats. `pr_tier` is deliberately absent: the band is derived
/// from `pr` through `PersonalRatingCategory::from_pr`, so the chips here and
/// the ones in the replay inspector cannot disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatsOut {
    pub account_id: AccountId,
    pub region: String,
    pub ship_id: GameParamId,
    pub status: PlayerStatsStatus,
    pub battles: Option<i64>,
    pub overall_win_rate: Option<f64>,
    pub ship_win_rate: Option<f64>,
    pub ship_battles: Option<i64>,
    pub pr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStatsResponse {
    pub arena_id: ArenaId,
    pub players: Vec<PlayerStatsOut>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips_through_bson_with_integer_ids() {
        let request = MatchStatsRequest {
            arena_id: ArenaId::from(9_876_543_210i64),
            players: vec![PlayerRef {
                account_id: AccountId(1_003_924_023),
                region: Region::Na,
                ship_id: GameParamId::from(4_179_539_664u64),
            }],
        };

        let bytes = bson::serialize_to_vec(&request).expect("request encodes");
        let document: bson::Document = bson::deserialize_from_slice(&bytes).expect("bytes are a document");

        assert_eq!(document.get_i64("arena_id").expect("arena_id is an int64"), 9_876_543_210);
        let player = document.get_array("players").expect("players is an array")[0]
            .as_document()
            .expect("a player is a document");
        assert_eq!(player.get_i64("account_id").expect("account_id is an int64"), 1_003_924_023);
        assert_eq!(player.get_i64("ship_id").expect("ship_id must land as an int64"), 4_179_539_664);
        assert_eq!(player.get_str("region").expect("region is a string"), "na");
    }

    #[test]
    fn a_response_round_trips_and_keeps_nulls_as_none() {
        let response = MatchStatsResponse {
            arena_id: ArenaId::from(7i64),
            players: vec![PlayerStatsOut {
                account_id: AccountId(1),
                region: "eu".to_string(),
                ship_id: GameParamId::from(2u64),
                status: PlayerStatsStatus::Hidden,
                battles: None,
                overall_win_rate: None,
                ship_win_rate: None,
                ship_battles: None,
                pr: None,
            }],
        };

        let bytes = bson::serialize_to_vec(&response).expect("response encodes");
        let decoded: MatchStatsResponse = bson::deserialize_from_slice(&bytes).expect("response decodes");

        assert_eq!(decoded.players[0].status, PlayerStatsStatus::Hidden);
        assert_eq!(decoded.players[0].battles, None);
        assert_eq!(decoded.players[0].pr, None);
    }

    /// A status the server adds later must degrade one player's row, not fail
    /// the whole response and lose the other 23 players with it.
    #[test]
    fn an_unrecognised_status_decodes_as_unknown() {
        let document = bson::doc! {
            "arena_id": 1i64,
            "players": [ bson::doc! {
                "account_id": 1i64, "region": "eu", "ship_id": 2i64, "status": "throttled",
                "battles": bson::Bson::Null, "overall_win_rate": bson::Bson::Null,
                "ship_win_rate": bson::Bson::Null, "ship_battles": bson::Bson::Null,
                "pr": bson::Bson::Null,
            } ],
        };
        let bytes = bson::serialize_to_vec(&document).expect("document encodes");

        let decoded: MatchStatsResponse = bson::deserialize_from_slice(&bytes).expect("response decodes");

        assert_eq!(decoded.players[0].status, PlayerStatsStatus::Unknown);
    }

    #[test]
    fn only_eu_na_and_asia_are_supported_regions() {
        assert_eq!(Region::from_realm("eu"), Some(Region::Eu));
        assert_eq!(Region::from_realm("NA"), Some(Region::Na));
        assert_eq!(Region::from_realm("asia"), Some(Region::Asia));
        assert_eq!(Region::from_realm("ru"), None);
        assert_eq!(Region::from_realm(""), None);
    }

    #[test]
    fn a_regions_wire_form_is_lowercase_and_its_url_form_uppercase() {
        assert_eq!(Region::Asia.as_wire(), "asia");
        assert_eq!(Region::Asia.as_url_segment(), "ASIA");
        assert_eq!(Region::Eu.as_wire(), "eu");
        assert_eq!(Region::Eu.as_url_segment(), "EU");
    }
}

use std::collections::HashMap;

use jiff::Timestamp;
use wows_replays::ReplayMeta;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::Species;

use super::model::TrackedPlayer;
use crate::ui::replay_parser::PlayerTint;

/// The roster of the match currently in progress, captured from the game's
/// `tempArenaInfo.json`.
#[derive(Debug, Clone)]
pub(crate) struct LiveMatch {
    pub started_at: Timestamp,
    /// Build the roster was captured from, used to look up ship params. `None`
    /// when the client version string carries no build number, in which case
    /// ships stay unresolved rather than being guessed from another build.
    pub build: Option<u32>,
    pub players: Vec<LiveMatchPlayer>,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveMatchPlayer {
    pub name: String,
    pub ship_id: GameParamId,
    pub relation: Relation,
}

/// One rendered roster entry: the live player joined to game data and to
/// tracked history.
#[derive(Debug, Clone)]
pub(crate) struct LiveRosterRow {
    pub name: String,
    pub tint: PlayerTint,
    pub species: Option<Species>,
    /// Localized ship name. `None` until the roster's build is loaded.
    pub ship_name: Option<String>,
    /// Localized ship class name, shown when hovering the class icon.
    pub species_text: Option<String>,
    /// Tracked account this entry joined to. `tempArenaInfo` carries no account
    /// id, so the join is by name and misses across a rename.
    pub tracked: Option<AccountId>,
}

impl LiveMatch {
    pub(crate) fn from_meta(meta: &ReplayMeta) -> Self {
        let build = Version::try_from_client_exe(&meta.clientVersionFromExe).and_then(|v| v.build_number());
        let players = meta
            .vehicles
            .iter()
            .map(|vehicle| LiveMatchPlayer {
                name: vehicle.name.clone(),
                ship_id: vehicle.shipId,
                relation: Relation::new(vehicle.relation),
            })
            .collect();

        Self { started_at: crate::util::replay_timestamp(meta), build, players }
    }
}

/// Tracked players keyed by lower-cased name, covering the current name and
/// every recorded alias. A current name beats another account's stale alias.
pub(crate) fn build_name_index(players: &HashMap<AccountId, TrackedPlayer>) -> HashMap<String, AccountId> {
    let mut index = HashMap::with_capacity(players.len());

    for (id, player) in players {
        for alias in player.names.iter().filter(|alias| !alias.is_empty()) {
            index.entry(alias.to_ascii_lowercase()).or_insert(*id);
        }
    }
    for (id, player) in players {
        if !player.last_name.is_empty() {
            index.insert(player.last_name.to_ascii_lowercase(), *id);
        }
    }

    index
}

/// Orders one team the way the replay inspector orders players: ship class
/// ascending. The inspector tie-breaks on account id, which the live roster
/// does not carry, so ship name then player name stand in. Entries whose ship
/// could not be resolved sort last so an unloaded build does not scatter them
/// through the list.
pub(crate) fn order_roster(rows: &mut [LiveRosterRow]) {
    rows.sort_by(|a, b| {
        a.species
            .is_none()
            .cmp(&b.species.is_none())
            .then_with(|| a.species.cmp(&b.species))
            .then_with(|| a.ship_name.cmp(&b.ship_name))
            .then_with(|| a.name.cmp(&b.name))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `tempArenaInfo` payload. Every key without `#[serde(default)]`
    /// on `ReplayMeta` must be present, which is why `gameLogic` and `logic`
    /// appear as explicit nulls.
    fn meta_json(client_version: &str, vehicles: &str) -> String {
        format!(
            r#"{{
                "gameMode": 7,
                "clientVersionFromExe": "{client_version}",
                "mapDisplayName": "ocean",
                "mapId": 1,
                "clientVersionFromXml": "{client_version}",
                "duration": 1200,
                "gameLogic": null,
                "name": "12x12",
                "scenario": "Domination",
                "playerID": 0,
                "vehicles": [{vehicles}],
                "playersPerTeam": 12,
                "dateTime": "28.12.2023 00:52:26",
                "mapName": "spaces/00_CO_ocean",
                "playerName": "Me",
                "scenarioConfigId": 1,
                "teamsCount": 2,
                "logic": null,
                "playerVehicle": "PFSD110-Kleber"
            }}"#
        )
    }

    fn vehicle(name: &str, ship_id: u64, relation: u32) -> String {
        format!(r#"{{"shipId": {ship_id}, "relation": {relation}, "id": 1, "name": "{name}"}}"#)
    }

    fn row(name: &str, species: Option<Species>, ship_name: Option<&str>) -> LiveRosterRow {
        LiveRosterRow {
            name: name.to_string(),
            tint: PlayerTint::Enemy,
            species,
            ship_name: ship_name.map(str::to_string),
            species_text: None,
            tracked: None,
        }
    }

    fn tracked(last_name: &str, aliases: &[&str]) -> TrackedPlayer {
        let mut player = TrackedPlayer::default();
        last_name.clone_into(&mut player.last_name);
        player.names = aliases.iter().map(|a| a.to_string()).collect();
        player
    }

    #[test]
    fn from_meta_captures_ships_relations_and_build() {
        let json = meta_json("13, 11, 0, 12668706", &format!("{},{}", vehicle("Ally", 100, 1), vehicle("Foe", 200, 2)));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");

        let live = LiveMatch::from_meta(&meta);

        assert_eq!(live.build, Some(12668706));
        assert_eq!(live.players.len(), 2);
        assert_eq!(live.players[0].name, "Ally");
        assert_eq!(live.players[0].ship_id, GameParamId::from(100u64));
        assert!(live.players[0].relation.is_ally());
        assert!(live.players[1].relation.is_enemy());
    }

    #[test]
    fn from_meta_without_a_build_number_reports_none() {
        let json = meta_json("0, 0, 0, 0", &vehicle("Ally", 100, 1));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");

        assert_eq!(LiveMatch::from_meta(&meta).build, None);
    }

    #[test]
    fn name_index_matches_current_name_and_aliases_case_insensitively() {
        let mut players = HashMap::new();
        players.insert(AccountId(1), tracked("Harvey635", &["fordy890"]));

        let index = build_name_index(&players);

        assert_eq!(index.get("harvey635"), Some(&AccountId(1)));
        assert_eq!(index.get("fordy890"), Some(&AccountId(1)));
        assert_eq!(index.get("nobody"), None);
    }

    #[test]
    fn name_index_prefers_a_current_name_over_another_accounts_alias() {
        let mut players = HashMap::new();
        players.insert(AccountId(1), tracked("Shared", &[]));
        players.insert(AccountId(2), tracked("Other", &["Shared"]));

        let index = build_name_index(&players);

        assert_eq!(index.get("shared"), Some(&AccountId(1)));
    }

    #[test]
    fn name_index_skips_empty_names() {
        let mut players = HashMap::new();
        players.insert(AccountId(1), tracked("", &[""]));

        assert!(build_name_index(&players).is_empty());
    }

    #[test]
    fn order_roster_sorts_by_species_then_ship_then_player() {
        let mut rows = vec![
            row("Zoe", Some(Species::Destroyer), Some("Shimakaze")),
            row("Amy", Some(Species::AirCarrier), Some("Hakuryu")),
            row("Bob", Some(Species::Destroyer), Some("Halland")),
            row("Al", Some(Species::Destroyer), Some("Halland")),
        ];

        order_roster(&mut rows);

        let order: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(order, ["Amy", "Al", "Bob", "Zoe"]);
    }

    #[test]
    fn order_roster_puts_unresolved_ships_last() {
        let mut rows = vec![row("Unknown", None, None), row("Known", Some(Species::Battleship), Some("Yamato"))];

        order_roster(&mut rows);

        assert_eq!(rows[0].name, "Known");
        assert_eq!(rows[1].name, "Unknown");
    }
}

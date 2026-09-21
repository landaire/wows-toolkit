//! The roster of the match currently in progress.
//!
//! Read from the game's own `tempArenaInfo.json` and joined to game data,
//! tracked history, and the identities a packet scan recovers. All of that is
//! the same work in both front ends; only the drawing differs.

use std::collections::HashMap;

use jiff::Timestamp;
use wows_replay_insights::battle_report::try_replay_timestamp;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::Species;

use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;

use crate::match_stats::Region;

/// The roster of the match currently in progress, captured from the game's
/// `tempArenaInfo.json`.
#[derive(Debug, Clone)]
pub struct LiveMatch {
    pub started_at: Timestamp,
    /// Build the roster was captured from, used to look up ship params. `None`
    /// when the client version string carries no build number, in which case
    /// ships stay unresolved rather than being guessed from another build.
    pub build: Option<u32>,
    pub players: Vec<LiveMatchPlayer>,
}

#[derive(Debug, Clone)]
pub struct LiveMatchPlayer {
    pub name: String,
    pub ship_id: GameParamId,
    pub relation: Relation,
}

/// How a roster entry is coloured: by relation, with the two stronger
/// classifications layered over it by the caller. The colours themselves
/// belong to each front end's theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTint {
    SelfPlayer,
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

impl PlayerTint {
    /// Classifies by relation alone (self/ally/enemy). Division-mate and
    /// abuser overrides are layered on top by the caller, since those are
    /// stronger classifications than plain relation.
    pub fn from_relation(relation: Relation) -> Self {
        if relation.is_self() {
            Self::SelfPlayer
        } else if relation.is_ally() {
            Self::Ally
        } else {
            Self::Enemy
        }
    }

    /// Applies the abuser override on top of this (row) tint: abuser beats
    /// everything else, including DivisionMate. This is the role the player's
    /// name renders with; the row's other colours (stats, icon) use the tint
    /// as-is, without this override.
    pub fn with_abuser_override(self, is_abuser: bool) -> Self {
        if is_abuser { Self::Abuser } else { self }
    }
}

/// One rendered roster entry: the live player joined to game data and to
/// tracked history.
#[derive(Debug, Clone)]
pub struct LiveRosterRow {
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
    /// Account id joined from the live-match identity scan, by name. `None`
    /// until a scan lands, or if the scan never named this player.
    pub account_id: Option<AccountId>,
    pub region: Option<Region>,
    /// Clan tag joined from the identity scan. `tempArenaInfo` carries no clan
    /// info at all, so this is `None` until the scan lands, same as
    /// `account_id`, and stays `None` for a player with no clan.
    pub clan: Option<String>,
    /// The clan's colour, `None` when the scan carried none or has not
    /// landed. Resolved against the row's relation tint at render time.
    pub clan_color: Option<ClanColor>,
}

impl LiveMatch {
    /// The roster `meta` describes.
    ///
    /// `None` when the header carries no date the game's format reads, which
    /// a file the game is still flushing can be; the caller retries rather
    /// than showing a battle that started at an invented time.
    pub fn from_meta(meta: &ReplayMeta) -> Option<Self> {
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

        Some(Self { started_at: try_replay_timestamp(meta)?, build, players })
    }
}

/// The identity of one live-match player, read off `onArenaStateReceived`.
/// `tempArenaInfo` carries neither field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveIdentity {
    pub account_id: AccountId,
    /// `None` where the replay's realm is one the stats service does not
    /// cover, which is distinct from a realm that was never recorded.
    pub region: Option<Region>,
    /// `None` for a player with no clan. `tempArenaInfo` carries no clan
    /// field at all, so this only ever comes from the packet scan.
    pub clan: Option<String>,
    /// The clan colour the server sent, packed `0xRRGGBB`. `None` for a
    /// player with no clan and for a replay predating clan colours; the
    /// wire carries both as zero, which is not a colour.
    pub clan_color: Option<ClanColor>,
}

/// A clan's server-sent colour, packed `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClanColor(pub u32);

impl ClanColor {
    /// The colour a raw server value names, if it names one. Zero is how the
    /// wire spells "none", not black.
    pub fn from_raw(raw: i64) -> Option<Self> {
        (raw != 0).then_some(Self((raw & 0xFF_FF_FF) as u32))
    }

    pub fn red(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub fn green(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub fn blue(self) -> u8 {
        self.0 as u8
    }
}

/// Every identity one `onArenaStateReceived` packet carried, keyed by
/// lower-cased name. Names are unique within a match, so the join onto the
/// `tempArenaInfo` roster is exact.
#[derive(Debug, Clone)]
pub struct LiveIdentities {
    pub by_name: HashMap<String, LiveIdentity>,
}

impl LiveIdentities {
    pub fn from_player_states(players: &[PlayerStateData]) -> Self {
        let by_name = players
            .iter()
            .filter(|player| !player.is_bot())
            .map(|player| {
                let region = player.realm().and_then(Region::from_realm);
                let clan = (!player.clan().is_empty()).then(|| player.clan().to_string());
                let identity = LiveIdentity {
                    account_id: player.db_id(),
                    region,
                    clan,
                    clan_color: ClanColor::from_raw(player.clan_color()),
                };
                (player.username().to_ascii_lowercase(), identity)
            })
            .collect();

        Self { by_name }
    }
}

/// The tracked-player lookup a roster is joined against.
///
/// `players` is how many accounts went into it, which is not `by_name.len()`:
/// an account known under several names contributes several entries, and the
/// consumers check staleness against their own player count.
pub struct TrackedIndex {
    pub by_name: HashMap<String, AccountId>,
    pub players: usize,
}

/// Tracked players keyed by lower-cased name.
///
/// `aliases` are every name an account has been seen under and `current` the
/// name it goes by now; a current name beats another account's stale alias,
/// which is why the two arrive separately rather than as one list.
pub fn build_name_index<'a>(
    players: usize,
    aliases: impl Iterator<Item = (AccountId, &'a str)>,
    current: impl Iterator<Item = (AccountId, &'a str)>,
) -> TrackedIndex {
    let mut by_name = HashMap::new();

    for (id, alias) in aliases.filter(|(_, alias)| !alias.is_empty()) {
        by_name.entry(alias.to_ascii_lowercase()).or_insert(id);
    }
    for (id, name) in current.filter(|(_, name)| !name.is_empty()) {
        by_name.insert(name.to_ascii_lowercase(), id);
    }

    TrackedIndex { by_name, players }
}

/// Orders one team the way the replay inspector orders players: ship class
/// ascending. The inspector tie-breaks on account id, which the live roster
/// does not carry, so ship name then player name stand in. Entries whose ship
/// could not be resolved sort last so an unloaded build does not scatter them
/// through the list.
pub fn order_roster(rows: &mut [LiveRosterRow]) {
    rows.sort_by(|a, b| {
        a.species
            .is_none()
            .cmp(&b.species.is_none())
            .then_with(|| a.species.cmp(&b.species))
            .then_with(|| a.ship_name.cmp(&b.ship_name))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// A `LiveMatch` resolved against game data and tracked history, split by team.
#[derive(Debug, Clone)]
pub struct ResolvedRoster {
    pub started_at: Timestamp,
    /// Whether game data for the roster's build was loaded when this was built.
    /// Drives a retry, so the roster fills in once a lazy build load completes.
    pub ships_resolved: bool,
    /// Tracked-player count the name join was built against, so the join is
    /// rebuilt after new replays are indexed. Counts players, not names: one
    /// account can be known under several, and the consumers compare this
    /// against how many players they hold.
    pub tracked_count: usize,
    /// Identity count the name join was built against, so the join is rebuilt
    /// once the scan lands.
    pub identity_count: usize,
    pub friendly: Vec<LiveRosterRow>,
    pub enemy: Vec<LiveRosterRow>,
}

/// Joins the live roster to game data, tracked history and the identity scan.
///
/// `name_index` is the tracked-player lookup [`build_name_index`] produces;
/// taking the index rather than the tracker keeps this free of whatever shape
/// each front end stores its tracked players in.
pub fn resolve_roster(
    live: &LiveMatch,
    tracked: &TrackedIndex,
    identities: Option<&LiveIdentities>,
    metadata: Option<&GameMetadataProvider>,
) -> ResolvedRoster {
    let name_index = &tracked.by_name;
    let mut friendly = Vec::new();
    let mut enemy = Vec::new();

    for player in &live.players {
        let param = metadata.and_then(|provider| GameParamProvider::game_param_by_id(provider, player.ship_id));
        let species = param.as_ref().and_then(|p| p.species()).and_then(|r| r.known().cloned());
        let ship_name = match (metadata, param.as_ref()) {
            (Some(provider), Some(param)) => provider.localized_name_from_param(param),
            _ => None,
        };
        let species_text = match (metadata, species.as_ref()) {
            // A missing class translation degrades to the raw class name, which
            // is the true species rather than a stand-in for an unknown one.
            (Some(provider), Some(species)) => provider
                .localized_name_from_id(&TranslationKey::new(species.translation_id()))
                .or_else(|| Some(species.name().to_string())),
            _ => None,
        };

        let identity = identities.and_then(|ids| ids.by_name.get(&player.name.to_ascii_lowercase()));

        let row = LiveRosterRow {
            tint: PlayerTint::from_relation(player.relation),
            species,
            ship_name,
            species_text,
            tracked: name_index.get(&player.name.to_ascii_lowercase()).copied(),
            account_id: identity.map(|identity| identity.account_id),
            region: identity.and_then(|identity| identity.region),
            clan: identity.and_then(|identity| identity.clan.clone()),
            clan_color: identity.and_then(|identity| identity.clan_color),
            name: player.name.clone(),
        };

        if player.relation.is_enemy() { enemy.push(row) } else { friendly.push(row) }
    }

    order_roster(&mut friendly);
    order_roster(&mut enemy);

    ResolvedRoster {
        started_at: live.started_at,
        ships_resolved: metadata.is_some(),
        tracked_count: tracked.players,
        identity_count: identities.map_or(0, |ids| ids.by_name.len()),
        friendly,
        enemy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
            account_id: None,
            region: None,
            clan: None,
            clan_color: None,
        }
    }

    /// One tracked player's names, in the shape the index takes.
    struct Names {
        id: AccountId,
        current: String,
        aliases: HashSet<String>,
    }

    fn tracked(id: i64, last_name: &str, aliases: &[&str]) -> Names {
        Names {
            id: AccountId(id),
            current: last_name.to_string(),
            aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
        }
    }

    fn index(players: &[Names]) -> TrackedIndex {
        build_name_index(
            players.len(),
            players.iter().flat_map(|player| player.aliases.iter().map(|alias| (player.id, alias.as_str()))),
            players.iter().map(|player| (player.id, player.current.as_str())),
        )
    }

    #[test]
    fn from_meta_captures_ships_relations_and_build() {
        let json = meta_json("13, 11, 0, 12668706", &format!("{},{}", vehicle("Ally", 100, 1), vehicle("Foe", 200, 2)));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");

        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

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

        assert_eq!(LiveMatch::from_meta(&meta).expect("the fixture carries a date").build, None);
    }

    #[test]
    fn name_index_matches_current_name_and_aliases_case_insensitively() {
        let index = index(&[tracked(1, "Harvey635", &["fordy890"])]);

        assert_eq!(index.by_name.get("harvey635"), Some(&AccountId(1)));
        assert_eq!(index.by_name.get("fordy890"), Some(&AccountId(1)));
        assert_eq!(index.by_name.get("nobody"), None);
    }

    #[test]
    fn name_index_prefers_a_current_name_over_another_accounts_alias() {
        let index = index(&[tracked(1, "Shared", &[]), tracked(2, "Other", &["Shared"])]);

        assert_eq!(index.by_name.get("shared"), Some(&AccountId(1)));
    }

    #[test]
    fn name_index_skips_empty_names() {
        assert!(index(&[tracked(1, "", &[""])]).by_name.is_empty());
    }

    /// A header the game has not finished writing can carry a date that is
    /// not yet the game's format; that is a retry, not a battle at an
    /// invented time.
    #[test]
    fn a_header_with_an_unreadable_date_is_not_a_roster() {
        let json = meta_json("13, 11, 0, 12668706", &vehicle("Ally", 100, 1)).replace("28.12.2023 00:52:26", "");
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");

        assert!(LiveMatch::from_meta(&meta).is_none());
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

    #[test]
    fn resolve_roster_splits_teams_and_keeps_self_with_allies() {
        let json = meta_json(
            "13, 11, 0, 12668706",
            &format!("{},{},{}", vehicle("Me", 100, 0), vehicle("Ally", 101, 1), vehicle("Foe", 200, 2)),
        );
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");
        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

        let resolved = resolve_roster(&live, &TrackedIndex { by_name: HashMap::new(), players: 0 }, None, None);

        let friendly: Vec<&str> = resolved.friendly.iter().map(|r| r.name.as_str()).collect();
        let enemy: Vec<&str> = resolved.enemy.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(friendly.len(), 2);
        assert!(friendly.contains(&"Me"));
        assert!(friendly.contains(&"Ally"));
        assert_eq!(enemy, ["Foe"]);
    }

    #[test]
    fn resolve_roster_marks_ships_unresolved_without_game_data() {
        let json = meta_json("13, 11, 0, 12668706", &vehicle("Ally", 100, 1));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");
        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

        let resolved = resolve_roster(&live, &TrackedIndex { by_name: HashMap::new(), players: 0 }, None, None);

        assert!(!resolved.ships_resolved);
        assert_eq!(resolved.friendly[0].ship_name, None);
        assert_eq!(resolved.friendly[0].species, None);
    }

    #[test]
    fn resolve_roster_joins_tracked_players_by_name() {
        let json = meta_json(
            "13, 11, 0, 12668706",
            &format!("{},{}", vehicle("Harvey635", 100, 2), vehicle("Stranger", 101, 2)),
        );
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");
        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

        let index = index(&[tracked(42, "Harvey635", &[])]);

        let resolved = resolve_roster(&live, &index, None, None);

        let harvey = resolved.enemy.iter().find(|r| r.name == "Harvey635").expect("roster keeps the tracked player");
        let stranger = resolved.enemy.iter().find(|r| r.name == "Stranger").expect("roster keeps the unknown player");
        assert_eq!(harvey.tracked, Some(AccountId(42)));
        assert_eq!(stranger.tracked, None);
        assert_eq!(resolved.tracked_count, 1);
    }

    /// A minimal `PlayerStateData` fixture. Its fields are crate-private to the
    /// parser, so it is built the same way `player_tracker::tests` does: through
    /// `Deserialize`, with every key `PlayerStateData` requires present.
    fn player_state(username: &str, db_id: i64, clan: &str, clan_color: i64, is_bot: bool) -> PlayerStateData {
        serde_json::from_value(serde_json::json!({
            "username": username,
            "clan": clan,
            "clan_id": if clan.is_empty() { 0 } else { 7 },
            "clan_color": clan_color,
            "db_id": db_id,
            "realm": "na",
            "player_id": 0,
            "entity_id": 0,
            "team_id": 0,
            "max_health": 40_000,
            "is_abuser": false,
            "is_hidden": false,
            "is_bot": is_bot,
            "human_properties": {
                "avatar_id": 0,
                "prebattle_id": 0,
                "is_client_loaded": true,
                "is_connected": true,
            },
        }))
        .expect("the fixture matches PlayerStateData's shape")
    }

    #[test]
    fn from_player_states_carries_the_clan_tag_and_colour() {
        let identities =
            LiveIdentities::from_player_states(&[player_state("Harvey635", 42, "RAIN", 0x00_ff_00, false)]);

        let identity = identities.by_name.get("harvey635").expect("the player is indexed by lower-cased name");
        assert_eq!(identity.account_id, AccountId(42));
        assert_eq!(identity.clan, Some("RAIN".to_string()));
        assert_eq!(identity.clan_color, Some(ClanColor(0x00_ff_00)));
    }

    #[test]
    fn from_player_states_leaves_a_clanless_player_with_no_tag() {
        let identities = LiveIdentities::from_player_states(&[player_state("Stranger", 43, "", 0, false)]);

        let identity = identities.by_name.get("stranger").expect("the player is indexed");
        assert_eq!(identity.clan, None);
    }

    #[test]
    fn from_player_states_skips_bots() {
        let identities = LiveIdentities::from_player_states(&[player_state("Bot", 0, "", 0, true)]);

        assert!(identities.by_name.is_empty());
    }

    #[test]
    fn identities_key_on_a_lower_cased_name() {
        let identities = LiveIdentities {
            by_name: HashMap::from([(
                "harvey635".to_string(),
                LiveIdentity {
                    account_id: AccountId(42),
                    region: Some(Region::Na),
                    clan: Some("RAIN".to_string()),
                    clan_color: Some(ClanColor(0x00_ff_00)),
                },
            )]),
        };
        let json = meta_json("13, 11, 0, 12668706", &vehicle("Harvey635", 100, 2));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");
        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

        let resolved =
            resolve_roster(&live, &TrackedIndex { by_name: HashMap::new(), players: 0 }, Some(&identities), None);

        assert_eq!(resolved.enemy[0].account_id, Some(AccountId(42)));
        assert_eq!(resolved.enemy[0].region, Some(Region::Na));
        assert_eq!(resolved.enemy[0].clan, Some("RAIN".to_string()));
        assert_eq!(resolved.enemy[0].clan_color, Some(ClanColor(0x00_ff_00)));
        assert_eq!(resolved.identity_count, 1);
    }

    #[test]
    fn a_roster_without_identities_keeps_todays_behaviour() {
        let json = meta_json("13, 11, 0, 12668706", &vehicle("Harvey635", 100, 2));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");
        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

        let resolved = resolve_roster(&live, &TrackedIndex { by_name: HashMap::new(), players: 0 }, None, None);

        assert_eq!(resolved.enemy[0].account_id, None);
        assert_eq!(resolved.enemy[0].region, None);
        assert_eq!(resolved.enemy[0].clan, None, "a roster without identities carries no clan tags either");
        assert_eq!(resolved.identity_count, 0);
    }

    #[test]
    fn a_player_the_scan_missed_gets_no_identity() {
        let identities = LiveIdentities {
            by_name: HashMap::from([(
                "someone_else".to_string(),
                LiveIdentity { account_id: AccountId(42), region: Some(Region::Eu), clan: None, clan_color: None },
            )]),
        };
        let json = meta_json("13, 11, 0, 12668706", &vehicle("Harvey635", 100, 2));
        let meta: ReplayMeta = serde_json::from_str(&json).expect("meta parses");
        let live = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

        let resolved =
            resolve_roster(&live, &TrackedIndex { by_name: HashMap::new(), players: 0 }, Some(&identities), None);

        assert_eq!(resolved.enemy[0].account_id, None);
    }
}

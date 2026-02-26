use crate::PResult;
use crate::packet2::{EntityMethodPacket, Packet, PacketType};
use crate::types::{AccountId, AvatarId, EntityId, GameParamId, NormalizedPos, PlaneId, ShotId, WorldPos, WorldPos2D};
use kinded::Kinded;
use pickled::Value;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::convert::TryInto;
use std::iter::FromIterator;
use winnow::Parser;
use winnow::binary::{le_f32, le_u8, le_u16, le_u64};
use wowsunpack::data::Version;
use wowsunpack::game_constants::{DEFAULT_BATTLE_CONSTANTS, DEFAULT_COMMON_CONSTANTS, DEFAULT_SHIPS_CONSTANTS};
use wowsunpack::game_params::convert::pickle_to_json;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::rpc::typedefs::ArgValue;
use wowsunpack::unpack_rpc_args;

use super::analyzer::Analyzer;

pub struct DecoderBuilder {
    silent: bool,
    no_meta: bool,
    path: Option<String>,
}

impl DecoderBuilder {
    pub fn new(silent: bool, no_meta: bool, output: Option<&str>) -> Self {
        Self { silent, no_meta, path: output.map(|s| s.to_string()) }
    }

    pub fn build(self, meta: &crate::ReplayMeta) -> Box<dyn Analyzer> {
        let version = Version::from_client_exe(&meta.clientVersionFromExe);
        let mut decoder = Decoder {
            silent: self.silent,
            output: self
                .path
                .as_ref()
                .map(|path| Box::new(std::fs::File::create(path).unwrap()) as Box<dyn std::io::Write>),
            packet_decoder: PacketDecoder::builder().version(version).build(),
        };
        if !self.no_meta {
            decoder.write(&serde_json::to_string(&meta).unwrap());
        }
        Box::new(decoder)
    }
}
pub use wowsunpack::game_types::{
    BatteryState, BattleStage, BuoyancyState, CameraMode, CollisionType, Consumable, DeathCause, FinishType, Ribbon,
    ShellHitType, VoiceLine, WeaponType,
};
pub use wowsunpack::recognized::Recognized;

/// Properties only present for human players (not bots)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanPlayerProperties {
    /// Their avatar entity ID in the game
    pub(crate) avatar_id: AvatarId,
    /// Division ID
    pub(crate) prebattle_id: i64,
    /// Has the client loaded into the game
    pub(crate) is_client_loaded: bool,
    /// Is the client connected into the game
    pub(crate) is_connected: bool,
}

/// Contains the information describing a player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStateData {
    /// The username of this player
    pub(crate) username: String,
    /// The player's clan
    pub(crate) clan: String,
    /// The player's clan DB id
    pub(crate) clan_id: i64,
    /// The color of the player's clan tag as an RGB integer
    pub(crate) clan_color: i64,
    /// The player's DB ID (unique player ID)
    pub(crate) db_id: AccountId,
    /// The realm this player belongs to
    pub(crate) realm: Option<String>,
    /// Their meta ID in the game (account-level identifier)
    pub(crate) meta_ship_id: AccountId,
    /// This player's entity created by a CreateEntity packet
    pub(crate) entity_id: EntityId,
    /// Which team they're on.
    pub(crate) team_id: i64,
    /// Their starting health
    pub(crate) max_health: i64,
    /// ????
    pub(crate) is_abuser: bool,
    /// Has hidden stats
    pub(crate) is_hidden: bool,
    /// Is this player a bot (AI-controlled)
    pub(crate) is_bot: bool,
    /// Properties only present for human players
    pub(crate) human_properties: Option<HumanPlayerProperties>,

    /// This is a raw dump (with the values converted to strings) of every key for the player.
    // TODO: Replace String with the actual pickle value (which is cleanly serializable)
    #[serde(skip_deserializing)]
    pub(crate) raw: HashMap<i64, String>,
    #[serde(skip_deserializing)]
    pub(crate) raw_with_names: HashMap<&'static str, serde_json::Value>,
}

impl PlayerStateData {
    // Key string constants for player data fields
    pub(crate) const KEY_ACCOUNT_DBID: &'static str = "accountDBID";
    pub(crate) const KEY_ANTI_ABUSE_ENABLED: &'static str = "antiAbuseEnabled";
    pub(crate) const KEY_AVATAR_ID: &'static str = "avatarId";
    pub(crate) const KEY_CAMOUFLAGE_INFO: &'static str = "camouflageInfo";
    pub(crate) const KEY_CLAN_COLOR: &'static str = "clanColor";
    pub(crate) const KEY_CLAN_ID: &'static str = "clanID";
    pub(crate) const KEY_CLAN_TAG: &'static str = "clanTag";
    pub(crate) const KEY_CREW_PARAMS: &'static str = "crewParams";
    pub(crate) const KEY_DOG_TAG: &'static str = "dogTag";
    pub(crate) const KEY_FRAGS_COUNT: &'static str = "fragsCount";
    pub(crate) const KEY_FRIENDLY_FIRE_ENABLED: &'static str = "friendlyFireEnabled";
    pub(crate) const KEY_ID: &'static str = "id";
    pub(crate) const KEY_INVITATIONS_ENABLED: &'static str = "invitationsEnabled";
    pub(crate) const KEY_IS_ABUSER: &'static str = "isAbuser";
    pub(crate) const KEY_IS_ALIVE: &'static str = "isAlive";
    pub(crate) const KEY_IS_BOT: &'static str = "isBot";
    pub(crate) const KEY_IS_CLIENT_LOADED: &'static str = "isClientLoaded";
    pub(crate) const KEY_IS_CONNECTED: &'static str = "isConnected";
    pub(crate) const KEY_IS_HIDDEN: &'static str = "isHidden";
    pub(crate) const KEY_IS_LEAVER: &'static str = "isLeaver";
    pub(crate) const KEY_IS_PRE_BATTLE_OWNER: &'static str = "isPreBattleOwner";
    pub(crate) const KEY_IS_T_SHOOTER: &'static str = "isTShooter";
    pub(crate) const KEY_KEY_TARGET_MARKERS: &'static str = "keyTargetMarkers";
    pub(crate) const KEY_KILLED_BUILDINGS_COUNT: &'static str = "killedBuildingsCount";
    pub(crate) const KEY_MAX_HEALTH: &'static str = "maxHealth";
    pub(crate) const KEY_NAME: &'static str = "name";
    pub(crate) const KEY_PLAYER_MODE: &'static str = "playerMode";
    pub(crate) const KEY_PRE_BATTLE_ID_ON_START: &'static str = "preBattleIdOnStart";
    pub(crate) const KEY_PRE_BATTLE_SIGN: &'static str = "preBattleSign";
    pub(crate) const KEY_PREBATTLE_ID: &'static str = "prebattleId";
    pub(crate) const KEY_REALM: &'static str = "realm";
    pub(crate) const KEY_SHIP_COMPONENTS: &'static str = "shipComponents";
    pub(crate) const KEY_SHIP_CONFIG_DUMP: &'static str = "shipConfigDump";
    pub(crate) const KEY_SHIP_ID: &'static str = "shipId";
    pub(crate) const KEY_SHIP_PARAMS_ID: &'static str = "shipParamsId";
    pub(crate) const KEY_SKIN_ID: &'static str = "skinId";
    pub(crate) const KEY_TEAM_ID: &'static str = "teamId";
    pub(crate) const KEY_TTK_STATUS: &'static str = "ttkStatus";

    fn convert_raw_dict(values: &HashMap<i64, Value>, version: &Version, is_bot: bool) -> HashMap<&'static str, Value> {
        let keys: HashMap<&'static str, i64> =
            if is_bot { Self::bot_key_map(version) } else { Self::player_key_map(version) };

        let mut raw_with_names = HashMap::new();
        for (k, v) in values.iter() {
            if let Some(name) = keys.iter().find_map(|(name, idx)| if *idx == *k { Some(*name) } else { None }) {
                raw_with_names.insert(name, v.clone());
            }
        }

        raw_with_names
    }

    fn player_key_map(version: &Version) -> HashMap<&'static str, i64> {
        if version.is_at_least(&Version::from_client_exe("0,12,8,0")) {
            let mut h = HashMap::new();
            h.insert(Self::KEY_ACCOUNT_DBID, 0);
            h.insert(Self::KEY_ANTI_ABUSE_ENABLED, 1);
            h.insert(Self::KEY_AVATAR_ID, 2);
            h.insert(Self::KEY_CAMOUFLAGE_INFO, 3);
            h.insert(Self::KEY_CLAN_COLOR, 4);
            h.insert(Self::KEY_CLAN_ID, 5);
            h.insert(Self::KEY_CLAN_TAG, 6);
            h.insert(Self::KEY_CREW_PARAMS, 7);
            h.insert(Self::KEY_DOG_TAG, 8);
            h.insert(Self::KEY_FRAGS_COUNT, 9);
            h.insert(Self::KEY_FRIENDLY_FIRE_ENABLED, 10);
            h.insert(Self::KEY_ID, 11);
            h.insert(Self::KEY_INVITATIONS_ENABLED, 12);
            h.insert(Self::KEY_IS_ABUSER, 13);
            h.insert(Self::KEY_IS_ALIVE, 14);
            h.insert(Self::KEY_IS_BOT, 15);
            h.insert(Self::KEY_IS_CLIENT_LOADED, 16);
            h.insert(Self::KEY_IS_CONNECTED, 17);
            h.insert(Self::KEY_IS_HIDDEN, 18);
            h.insert(Self::KEY_IS_LEAVER, 19);
            h.insert(Self::KEY_IS_PRE_BATTLE_OWNER, 20);
            h.insert(Self::KEY_IS_T_SHOOTER, 21);
            h.insert(Self::KEY_KEY_TARGET_MARKERS, 22);
            h.insert(Self::KEY_KILLED_BUILDINGS_COUNT, 23);
            h.insert(Self::KEY_MAX_HEALTH, 24);
            h.insert(Self::KEY_NAME, 25);
            h.insert(Self::KEY_PLAYER_MODE, 26);
            h.insert(Self::KEY_PRE_BATTLE_ID_ON_START, 27);
            h.insert(Self::KEY_PRE_BATTLE_SIGN, 28);
            h.insert(Self::KEY_PREBATTLE_ID, 29);
            h.insert(Self::KEY_REALM, 30);
            h.insert(Self::KEY_SHIP_COMPONENTS, 31);
            h.insert(Self::KEY_SHIP_CONFIG_DUMP, 32);
            h.insert(Self::KEY_SHIP_ID, 33);
            h.insert(Self::KEY_SHIP_PARAMS_ID, 34);
            h.insert(Self::KEY_SKIN_ID, 35);
            h.insert(Self::KEY_TEAM_ID, 36);
            h.insert(Self::KEY_TTK_STATUS, 37);
            h
        } else if version.is_at_least(&Version::from_client_exe("0,10,9,0")) {
            let mut h = HashMap::new();
            h.insert(Self::KEY_AVATAR_ID, 0x2);
            h.insert(Self::KEY_CLAN_TAG, 0x6);
            h.insert(Self::KEY_MAX_HEALTH, 0x17);
            h.insert(Self::KEY_NAME, 0x18);
            h.insert(Self::KEY_SHIP_ID, 0x20);
            h.insert(Self::KEY_SHIP_PARAMS_ID, 0x21);
            h.insert(Self::KEY_SKIN_ID, 0x22);
            h.insert(Self::KEY_TEAM_ID, 0x23);
            h
        } else if version.is_at_least(&Version::from_client_exe("0,10,7,0")) {
            let mut h = HashMap::new();
            h.insert(Self::KEY_AVATAR_ID, 0x1);
            h.insert(Self::KEY_CLAN_TAG, 0x5);
            h.insert(Self::KEY_MAX_HEALTH, 0x16);
            h.insert(Self::KEY_NAME, 0x17);
            h.insert(Self::KEY_SHIP_ID, 0x1e);
            h.insert(Self::KEY_SHIP_PARAMS_ID, 0x1f);
            h.insert(Self::KEY_SKIN_ID, 0x20);
            h.insert(Self::KEY_TEAM_ID, 0x21);
            h
        } else {
            let mut h = HashMap::new();
            h.insert(Self::KEY_AVATAR_ID, 0x1);
            h.insert(Self::KEY_CLAN_TAG, 0x5);
            h.insert(Self::KEY_MAX_HEALTH, 0x15);
            h.insert(Self::KEY_NAME, 0x16);
            h.insert(Self::KEY_SHIP_ID, 0x1d);
            h.insert(Self::KEY_SHIP_PARAMS_ID, 0x1e);
            h.insert(Self::KEY_SKIN_ID, 0x1f);
            h.insert(Self::KEY_TEAM_ID, 0x20);
            h
        }
    }

    /// Bot key mapping — bots have a different (smaller) set of fields with different indices.
    fn bot_key_map(version: &Version) -> HashMap<&'static str, i64> {
        if version.is_at_least(&Version::from_client_exe("0,12,8,0")) {
            let mut h = HashMap::new();
            h.insert(Self::KEY_ACCOUNT_DBID, 0);
            h.insert(Self::KEY_ANTI_ABUSE_ENABLED, 1);
            h.insert(Self::KEY_CAMOUFLAGE_INFO, 2);
            h.insert(Self::KEY_CLAN_COLOR, 3);
            h.insert(Self::KEY_CLAN_ID, 4);
            h.insert(Self::KEY_CLAN_TAG, 5);
            h.insert(Self::KEY_CREW_PARAMS, 6);
            h.insert(Self::KEY_DOG_TAG, 7);
            h.insert(Self::KEY_FRAGS_COUNT, 8);
            h.insert(Self::KEY_FRIENDLY_FIRE_ENABLED, 9);
            h.insert(Self::KEY_ID, 10);
            h.insert(Self::KEY_IS_ABUSER, 11);
            h.insert(Self::KEY_IS_ALIVE, 12);
            h.insert(Self::KEY_IS_BOT, 13);
            h.insert(Self::KEY_IS_HIDDEN, 14);
            h.insert(Self::KEY_IS_T_SHOOTER, 15);
            h.insert(Self::KEY_KILLED_BUILDINGS_COUNT, 16);
            h.insert(Self::KEY_KEY_TARGET_MARKERS, 17);
            h.insert(Self::KEY_MAX_HEALTH, 18);
            h.insert(Self::KEY_NAME, 19);
            h.insert(Self::KEY_REALM, 20);
            h.insert(Self::KEY_SHIP_COMPONENTS, 21);
            h.insert(Self::KEY_SHIP_CONFIG_DUMP, 22);
            h.insert(Self::KEY_SHIP_ID, 23);
            h.insert(Self::KEY_SHIP_PARAMS_ID, 24);
            h.insert(Self::KEY_SKIN_ID, 25);
            h.insert(Self::KEY_TEAM_ID, 26);
            h.insert(Self::KEY_TTK_STATUS, 27);
            h
        } else {
            // For older versions, bots weren't separately tracked or had
            // the same layout as players. Fall back to player key map.
            Self::player_key_map(version)
        }
    }

    fn from_pickle(value: &pickled::Value, version: &Version, is_bot: bool) -> Self {
        let raw_values = convert_flat_dict_to_real_dict(value);

        let mapped_values = Self::convert_raw_dict(&raw_values, version, is_bot);
        Self::from_values(raw_values, mapped_values, version)
    }

    fn from_values(
        raw_values: HashMap<i64, pickled::Value>,
        mut mapped_values: HashMap<&'static str, pickled::Value>,
        _version: &Version,
    ) -> Self {
        let username =
            mapped_values.get(Self::KEY_NAME).unwrap().string_ref().expect("name is not a string").inner().clone();

        let clan = mapped_values
            .get(Self::KEY_CLAN_TAG)
            .unwrap()
            .string_ref()
            .expect("clanTag is not a string")
            .inner()
            .clone();

        let clan_id = *mapped_values.get(Self::KEY_CLAN_ID).unwrap().i64_ref().expect("clanID is not an i64");

        let shipid = *mapped_values.get(Self::KEY_SHIP_ID).unwrap().i64_ref().expect("shipId is not an i64");
        let meta_ship_id = *mapped_values.get(Self::KEY_ID).unwrap().i64_ref().expect("id is not an i64");
        let team = *mapped_values.get(Self::KEY_TEAM_ID).unwrap().i64_ref().expect("teamId is not an i64");
        let health = *mapped_values.get(Self::KEY_MAX_HEALTH).unwrap().i64_ref().expect("maxHealth is not an i64");

        let realm = mapped_values.get(Self::KEY_REALM).unwrap().string_ref().map(|realm| realm.inner().clone());

        let db_id =
            mapped_values.get(Self::KEY_ACCOUNT_DBID).unwrap().i64_ref().cloned().expect("accountDBID is not an i64");

        let is_abuser =
            mapped_values.get(Self::KEY_IS_ABUSER).unwrap().bool_ref().cloned().expect("isAbuser is not a bool");

        let is_hidden =
            mapped_values.get(Self::KEY_IS_HIDDEN).unwrap().bool_ref().cloned().expect("isHidden is not a bool");

        let is_bot = mapped_values.get(Self::KEY_IS_BOT).and_then(|v| v.bool_ref().cloned()).unwrap_or(false);

        let clan_color =
            mapped_values.get(Self::KEY_CLAN_COLOR).unwrap().i64_ref().cloned().expect("clanColor is not an integer");

        // Human-only properties (not present for bots)
        let human_properties =
            mapped_values.get(Self::KEY_AVATAR_ID).and_then(|v| v.i64_ref().copied()).map(|avatar_id| {
                let prebattle_id =
                    mapped_values.get(Self::KEY_PREBATTLE_ID).and_then(|v| v.i64_ref().copied()).unwrap_or(0);
                let is_connected =
                    mapped_values.get(Self::KEY_IS_CONNECTED).and_then(|v| v.bool_ref().copied()).unwrap_or(false);
                let is_client_loaded =
                    mapped_values.get(Self::KEY_IS_CLIENT_LOADED).and_then(|v| v.bool_ref().copied()).unwrap_or(false);
                HumanPlayerProperties {
                    avatar_id: AvatarId::from(avatar_id as u32),
                    prebattle_id,
                    is_connected,
                    is_client_loaded,
                }
            });

        let mut raw = HashMap::new();
        for (k, v) in raw_values.iter() {
            raw.insert(*k, format!("{:?}", v));
        }

        PlayerStateData {
            username,
            clan,
            clan_id,
            clan_color,
            realm,
            db_id: AccountId::from(db_id),
            meta_ship_id: AccountId::from(meta_ship_id),
            entity_id: EntityId::from(shipid),
            team_id: team,
            max_health: health,
            is_abuser,
            is_hidden,
            is_bot,
            human_properties,
            raw,
            raw_with_names: HashMap::from_iter(mapped_values.drain().map(|(k, v)| (k, pickle_to_json(v)))),
        }
    }

    /// Updates the PlayerStateData from a dictionary of values.
    /// Only fields present in the dictionary will be updated.
    pub fn update_from_dict(&mut self, values: &HashMap<&'static str, pickled::Value>) {
        if let Some(v) = values.get(Self::KEY_AVATAR_ID)
            && let Some(id) = v.i64_ref()
            && let Some(ref mut hp) = self.human_properties
        {
            hp.avatar_id = AvatarId::from(*id);
        }
        if let Some(v) = values.get(Self::KEY_NAME)
            && let Some(s) = v.string_ref()
        {
            self.username = s.inner().clone();
        }
        if let Some(v) = values.get(Self::KEY_CLAN_TAG)
            && let Some(s) = v.string_ref()
        {
            self.clan = s.inner().clone();
        }
        if let Some(v) = values.get(Self::KEY_CLAN_ID)
            && let Some(id) = v.i64_ref()
        {
            self.clan_id = *id;
        }
        if let Some(v) = values.get(Self::KEY_CLAN_COLOR)
            && let Some(id) = v.i64_ref()
        {
            self.clan_color = *id;
        }
        if let Some(v) = values.get(Self::KEY_SHIP_ID)
            && let Some(id) = v.i64_ref()
        {
            self.entity_id = EntityId::from(*id);
        }
        if let Some(v) = values.get(Self::KEY_ID)
            && let Some(id) = v.i64_ref()
        {
            self.meta_ship_id = AccountId::from(*id);
        }
        if let Some(v) = values.get(Self::KEY_TEAM_ID)
            && let Some(id) = v.i64_ref()
        {
            self.team_id = *id;
        }
        if let Some(v) = values.get(Self::KEY_MAX_HEALTH)
            && let Some(id) = v.i64_ref()
        {
            self.max_health = *id;
        }
        if let Some(v) = values.get(Self::KEY_REALM)
            && let Some(s) = v.string_ref()
        {
            self.realm = Some(s.inner().clone());
        }
        if let Some(v) = values.get(Self::KEY_ACCOUNT_DBID)
            && let Some(id) = v.i64_ref()
        {
            self.db_id = AccountId::from(*id);
        }
        if let Some(v) = values.get(Self::KEY_PREBATTLE_ID)
            && let Some(id) = v.i64_ref()
            && let Some(ref mut hp) = self.human_properties
        {
            hp.prebattle_id = *id;
        }
        if let Some(v) = values.get(Self::KEY_IS_ABUSER)
            && let Some(b) = v.bool_ref()
        {
            self.is_abuser = *b;
        }
        if let Some(v) = values.get(Self::KEY_IS_HIDDEN)
            && let Some(b) = v.bool_ref()
        {
            self.is_hidden = *b;
        }
        if let Some(v) = values.get(Self::KEY_IS_CONNECTED)
            && let Some(b) = v.bool_ref()
            && let Some(ref mut hp) = self.human_properties
        {
            hp.is_connected = *b;
        }
        if let Some(v) = values.get(Self::KEY_IS_CLIENT_LOADED)
            && let Some(b) = v.bool_ref()
            && let Some(ref mut hp) = self.human_properties
        {
            hp.is_client_loaded = *b;
        }
        if let Some(v) = values.get(Self::KEY_IS_BOT)
            && let Some(b) = v.bool_ref()
        {
            self.is_bot = *b;
        }

        // Update raw_with_names with any new values
        for (k, v) in values.iter() {
            self.raw_with_names.insert(k, pickle_to_json(v.clone()));
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn clan(&self) -> &str {
        &self.clan
    }

    pub fn clan_id(&self) -> i64 {
        self.clan_id
    }

    pub fn clan_color(&self) -> i64 {
        self.clan_color
    }

    pub fn db_id(&self) -> AccountId {
        self.db_id
    }

    pub fn realm(&self) -> Option<&str> {
        self.realm.as_deref()
    }

    pub fn avatar_id(&self) -> Option<AvatarId> {
        self.human_properties.as_ref().map(|hp| hp.avatar_id)
    }

    pub fn meta_ship_id(&self) -> AccountId {
        self.meta_ship_id
    }

    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    pub fn team_id(&self) -> i64 {
        self.team_id
    }

    pub fn division_id(&self) -> i64 {
        self.human_properties.as_ref().map(|hp| hp.prebattle_id).unwrap_or(0)
    }

    /// Returns true if `other` is in the same division as `self` (and is not `self`).
    pub fn is_division_mate(&self, other: &PlayerStateData) -> bool {
        self.db_id() != other.db_id() && self.division_id() > 0 && other.division_id() == self.division_id()
    }

    pub fn max_health(&self) -> i64 {
        self.max_health
    }

    pub fn is_abuser(&self) -> bool {
        self.is_abuser
    }

    pub fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    pub fn is_client_loaded(&self) -> bool {
        self.human_properties.as_ref().map(|hp| hp.is_client_loaded).unwrap_or_else(|| self.is_bot())
    }

    pub fn is_connected(&self) -> bool {
        self.human_properties.as_ref().map(|hp| hp.is_connected).unwrap_or_else(|| self.is_bot())
    }

    pub fn human_properties(&self) -> Option<&HumanPlayerProperties> {
        self.human_properties.as_ref()
    }

    pub fn is_bot(&self) -> bool {
        self.is_bot
    }

    pub fn raw(&self) -> &HashMap<i64, String> {
        &self.raw
    }

    pub fn raw_with_names(&self) -> &HashMap<&'static str, serde_json::Value> {
        &self.raw_with_names
    }
}

/// Converts a list of key-value pairs to a real dictionary
fn convert_flat_dict_to_real_dict(value: &Value) -> HashMap<i64, Value> {
    let mut raw_values = HashMap::new();
    if let pickled::value::Value::List(elements) = value {
        for elem in elements.inner().iter() {
            if let pickled::value::Value::Tuple(kv) = elem {
                let key = kv.inner()[0].i64_ref().expect("tuple first value was not an integer");

                raw_values.insert(*key, kv.inner()[1].clone());
            }
        }
    }

    raw_values
}

/// Indicates that the given attacker has dealt damage
#[derive(Debug, Clone, Serialize)]
pub struct DamageReceived {
    /// Ship ID of the aggressor
    pub aggressor: EntityId,
    /// Amount of damage dealt
    pub damage: f32,
}

/// Sent to update the minimap display
#[derive(Debug, Clone, Serialize)]
pub struct MinimapUpdate {
    /// The ship ID of the ship to update
    pub entity_id: EntityId,
    /// True when the raw packed position is (0, 0), indicating the ship is not
    /// visible on the minimap. Checked on raw 11-bit integer values before float
    /// conversion to avoid floating-point precision issues.
    pub is_sentinel: bool,
    /// Set to true if the ship should disappear from the minimap (false otherwise)
    pub disappearing: bool,
    /// The heading of the ship. Unit is degrees, 0 is up, positive is clockwise
    /// (so 90.0 is East)
    pub heading: f32,
    /// Normalized position on the minimap
    pub position: NormalizedPos,
    /// Unknown, but this appears to be something related to the big hunt
    pub unknown: bool,
}

impl MinimapUpdate {
    /// Returns true if this is a hydrophone-style minimap ping: a one-shot
    /// position flash from minimap-only detection (e.g. submarine hydrophone).
    ///
    /// These updates have `disappearing=true` with a valid (non-sentinel)
    /// position. They are always isolated — never preceded by active tracking
    /// and never followed by a sentinel. The position is valid at the instant
    /// of the ping but should not be treated as sustained detection.
    pub fn is_minimap_ping(&self) -> bool {
        self.disappearing && !self.is_sentinel
    }
}

/// A single shell in an artillery salvo (from SHOT in alias.xml)
#[derive(Debug, Clone, Serialize)]
pub struct ArtilleryShotData {
    pub origin: WorldPos,
    /// Gun barrel pitch angle at fire time (radians).
    pub pitch: f32,
    pub speed: f32,
    pub target: WorldPos,
    pub shot_id: ShotId,
    /// Which barrel within the turret fired this shell.
    pub gun_barrel_id: u16,
    /// Server-side time remaining for the shell to reach the target (seconds).
    pub server_time_left: f32,
    /// Height of the shooter above sea level at fire time.
    pub shooter_height: f32,
    /// Distance from the gun to the aimed target point.
    pub hit_distance: f32,
}

/// A salvo of artillery shells from one ship
#[derive(Debug, Clone, Serialize)]
pub struct ArtillerySalvo {
    pub owner_id: EntityId,
    pub params_id: GameParamId,
    pub salvo_id: u32,
    pub shots: Vec<ArtilleryShotData>,
}

/// Homing torpedo maneuver state (from TORPEDO_MANEUVER_DUMP in alias.xml).
#[derive(Debug, Clone, Serialize)]
pub struct TorpedoManeuverDump {
    pub target_yaw: f32,
    pub change_time: f32,
    pub stop_time: f32,
    pub current_time: f32,
    pub yaw_speed: f32,
    pub arm_pos: WorldPos,
    pub final_pos: WorldPos,
}

/// Acoustic torpedo guidance state (from TORPEDO_ACOUSTIC_DUMP in alias.xml).
#[derive(Debug, Clone, Serialize)]
pub struct TorpedoAcousticDump {
    pub is_chasing_target: bool,
    pub prediction_lost: bool,
    pub modificators_level: u8,
    pub activation_time: f32,
    pub degradation_time: f32,
    pub speed_coef: f32,
    pub rotation_yaw: f32,
    pub vertical_speed: f32,
    pub target_yaw: f32,
    pub target_depth: f32,
}

/// A single torpedo launch (from TORPEDO in alias.xml)
#[derive(Debug, Clone, Serialize)]
pub struct TorpedoData {
    pub owner_id: EntityId,
    pub params_id: GameParamId,
    pub salvo_id: u32,
    /// Torpedo skin (cosmetic variant).
    pub skin_id: GameParamId,
    pub shot_id: ShotId,
    pub origin: WorldPos,
    /// Direction vector whose magnitude is the torpedo speed in m/s.
    pub direction: WorldPos,
    /// Whether the torpedo warhead is armed (can detonate on contact).
    pub armed: bool,
    /// Homing torpedo maneuver state. None for straight-running torpedoes.
    pub maneuver_dump: Option<TorpedoManeuverDump>,
    /// Acoustic torpedo guidance state. None for non-acoustic torpedoes.
    pub acoustic_dump: Option<TorpedoAcousticDump>,
}

/// Physics body state for a ship hull fragment after cracking apart on death.
/// Serialized as a 72-byte (0x48) raw binary blob by the engine's `dumpState()`.
/// Used by `syncShipCracks` to synchronize sinking animation between server and client.
#[derive(Debug, Clone, Serialize)]
pub struct PhysicsBodyState {
    /// Elasticity/friction coefficient (body struct offset +0x88, next to mass at +0x84)
    pub elasticity: f32,
    /// World position (x, y, z)
    pub position: WorldPos,
    /// Orientation as a quaternion (x, y, z, w)
    pub orientation: [f32; 4],
    /// Linear velocity in m/s (x, y, z)
    pub linear_velocity: WorldPos,
    /// Angular velocity in rad/s (x, y, z)
    pub angular_velocity: WorldPos,
    /// Unknown physics parameters (likely buoyancy/damping state)
    pub unknown1: [f32; 2],
    /// Unknown physics parameter (likely water damping coefficient)
    pub unknown2: f32,
}

impl PhysicsBodyState {
    /// Parse a 72-byte physics body state blob.
    /// Returns None if the blob is empty or not exactly 72 bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != 72 {
            return None;
        }
        let f = |offset: usize| -> f32 { f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) };
        Some(PhysicsBodyState {
            elasticity: f(0x00),
            position: WorldPos { x: f(0x04), y: f(0x08), z: f(0x0C) },
            orientation: [f(0x10), f(0x14), f(0x18), f(0x1C)],
            linear_velocity: WorldPos { x: f(0x20), y: f(0x24), z: f(0x28) },
            angular_velocity: WorldPos { x: f(0x2C), y: f(0x30), z: f(0x34) },
            unknown1: [f(0x38), f(0x3C)],
            unknown2: f(0x40),
        })
    }
}

/// Packed hit type from SHOTKILL, encoding both collision type and shell hit type.
/// Packed as `collision_type << 5 | shell_hit_type` by `IntPackerUnpacker`.
#[derive(Debug, Clone, Serialize)]
pub struct HitType {
    pub collision: Recognized<CollisionType>,
    pub shell_hit: Recognized<ShellHitType>,
    /// The raw packed byte, preserved in case of unknown values.
    pub raw: u8,
}

impl HitType {
    pub fn from_raw(raw: u8, ships_constants: &wowsunpack::game_constants::ShipsConstants, version: &Version) -> Self {
        let collision_id = ((raw >> 5) & 0x07) as i32;
        let shell_hit_id = (raw & 0x1F) as i32;
        let collision = CollisionType::from_id(collision_id, ships_constants, *version)
            .unwrap_or(Recognized::Unknown(format!("{collision_id}")));
        let shell_hit = ShellHitType::from_id(shell_hit_id, ships_constants, *version)
            .unwrap_or(Recognized::Unknown(format!("{shell_hit_id}")));
        Self { collision, shell_hit, raw }
    }
}

/// Terminal ballistics state at the moment of shell impact (from TERMINAL_BALLISTICS_INFO).
/// Contains the shell's position, velocity vector, detonator state, and the angle
/// against the impacted armor material. Available in game version 14.8+.
#[derive(Debug, Clone, Serialize)]
pub struct TerminalBallisticsInfo {
    /// Shell position at impact in world coordinates.
    pub position: WorldPos,
    /// Shell velocity vector at impact (direction and magnitude in m/s).
    pub velocity: WorldPos,
    /// Whether the AP detonator has been activated (fuse armed).
    pub detonator_activated: bool,
    /// Angle between the shell trajectory and the armor plate normal (radians).
    pub material_angle: f32,
}

/// A single projectile hit (from receiveShotKills)
#[derive(Debug, Clone, Serialize)]
pub struct ShotHit {
    pub owner_id: EntityId,
    pub hit_type: HitType,
    pub shot_id: ShotId,
    /// World-space position where the projectile impacted.
    pub position: WorldPos,
    /// Terminal ballistics info at impact (shell velocity, detonator state, armor angle).
    /// Only present in game versions that include TERMINAL_BALLISTICS_INFO in SHOTKILL.
    pub terminal_ballistics: Option<TerminalBallisticsInfo>,
}

/// Enumerates the "cruise states". See <https://github.com/lkolbly/wows-replays/issues/14#issuecomment-976784004>
/// for more information.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum CruiseState {
    /// Possible values for the throttle range from -1 for reverse to 4 for full power ahead.
    Throttle,
    /// Note that not all rudder changes are indicated via cruise states, only ones
    /// set via the Q & E keys. Temporarily setting the rudder will not trigger this
    /// packet.
    ///
    /// Possible associated values are:
    /// - -2: Full rudder to port,
    /// - -1: Half rudder to port,
    /// - 0: Neutral
    /// - 1: Half rudder to starboard,
    /// - 2: Full rudder to starboard.
    Rudder,
    /// Sets the dive depth. Known values are:
    /// - 0: 0m
    /// - 1: -6m (periscope depth)
    /// - 2: -18m
    /// - 3: -30m
    /// - 4: -42m
    /// - 5: -54m
    /// - 6: -66m
    /// - 7: -80m
    DiveDepth,
    /// Indicates an unknown cruise state. Send me your replay!
    Unknown(u32),
}

#[derive(Debug, Serialize)]
pub struct ChatMessageExtra {
    pre_battle_sign: i64,
    pre_battle_id: i64,
    player_clan_tag: String,
    typ: i64,
    player_avatar_id: EntityId,
    player_name: String,
}

#[derive(Debug, Serialize, Kinded)]
#[kinded(derive(Serialize))]
pub enum DecodedPacketPayload<'replay, 'argtype, 'rawpacket> {
    /// Represents a chat message. Note that this only includes text chats, voicelines
    /// are represented by the VoiceLine variant.
    Chat {
        entity_id: EntityId,
        /// Avatar ID of the sender
        sender_id: AccountId,
        /// Represents the audience for the chat: Division, team, or all.
        audience: &'replay str,
        /// The actual chat message.
        message: &'replay str,
        /// Extra data that may be present if sender_id is 0
        extra_data: Option<ChatMessageExtra>,
    },
    /// Sent when a voice line is played (for example, "Wilco!")
    VoiceLine {
        /// Avatar ID of the player sending the voiceline
        sender_id: AccountId,
        /// True if the voiceline is visible in all chat, false if only in team chat
        is_global: bool,
        /// Which voiceline it is.
        message: VoiceLine,
    },
    /// Sent when the player earns a ribbon
    Ribbon(Ribbon),
    /// Indicates the position of the given object.
    Position(crate::packet2::PositionPacket),
    /// Indicates the position of the player's object or camera.
    PlayerOrientation(crate::packet2::PlayerOrientationPacket),
    /// Indicates updating a damage statistic. The first tuple, `(i64,i64)`, is a two-part
    /// label indicating what type of damage this refers to. The second tuple, `(i64,f64)`,
    /// indicates the actual damage counter increment.
    ///
    /// Some known keys include:
    /// - (1, 0) key is (# AP hits that dealt damage, total AP damage dealt)
    /// - (1, 3) is (# artillery fired, total possible damage) ?
    /// - (2, 0) is (# HE penetrations, total HE damage)
    /// - (17, 0) is (# fire tick marks, total fire damage)
    DamageStat(Vec<((i64, i64), (i64, f64))>),
    /// Sent when a ship is destroyed.
    ShipDestroyed {
        /// The ship ID (note: Not the avatar ID) of the killer
        killer: EntityId,
        /// The ship ID (note: Not the avatar ID) of the victim
        victim: EntityId,
        /// Cause of death
        cause: Recognized<DeathCause>,
    },
    EntityMethod(&'rawpacket EntityMethodPacket<'argtype>),
    EntityProperty(&'rawpacket crate::packet2::EntityPropertyPacket<'argtype>),
    BasePlayerCreate(&'rawpacket crate::packet2::BasePlayerCreatePacket<'argtype>),
    CellPlayerCreate(&'rawpacket crate::packet2::CellPlayerCreatePacket<'argtype>),
    EntityEnter(&'rawpacket crate::packet2::EntityEnterPacket),
    EntityLeave(&'rawpacket crate::packet2::EntityLeavePacket),
    EntityCreate(&'rawpacket crate::packet2::EntityCreatePacket<'argtype>),
    /// Contains all of the info required to setup the arena state and show the initial loading screen.
    OnArenaStateReceived {
        /// Unknown
        arena_id: i64,
        /// Unknown
        team_build_type_id: i8,
        /// Unknown
        pre_battles_info: HashMap<i64, Vec<Option<HashMap<String, String>>>>,
        /// A list of the human players in this game
        player_states: Vec<PlayerStateData>,
        /// A list of the bot players in this game
        bot_states: Vec<PlayerStateData>,
    },
    /// Contains info when the arena state changes
    OnGameRoomStateChanged {
        /// Updated player states
        player_states: Vec<HashMap<&'static str, pickled::Value>>,
    },
    CheckPing(u64),
    /// Indicates that the given victim has received damage from one or more attackers.
    DamageReceived {
        /// Ship ID of the ship being damaged
        victim: EntityId,
        /// List of damages happening to this ship
        aggressors: Vec<DamageReceived>,
    },
    /// Contains data for a minimap update
    MinimapUpdate {
        /// A list of the updates to make to the minimap
        updates: Vec<MinimapUpdate>,
        /// Unknown
        arg1: &'rawpacket Vec<ArgValue<'argtype>>,
    },
    /// Indicates a property update. Note that many properties contain a hierarchy of properties,
    /// for example the "state" property on the battle manager contains nested dictionaries and
    /// arrays. The top-level entity and property are specified by the `entity_id` and `property`
    /// fields. The nesting structure and how to modify the leaves are indicated by the
    /// `update_cmd` field.
    ///
    /// Within the `update_cmd` field is two fields, `levels` and `action`. `levels` indicates how
    /// to traverse to the leaf property, for example by following a dictionary key or array index.
    /// `action` indicates what action to perform once there, such as setting a subproperty to
    /// a specific value.
    ///
    /// For example, to set the `state[controlPoints][0][hasInvaders]` property, you will see a
    /// packet payload that looks like:
    /// ```ignore
    /// {
    ///     "entity_id": 576258,
    ///     "property": "state",
    ///     "update_cmd": {
    ///         "levels": [
    ///             {"DictKey": "controlPoints"},
    ///             {"ArrayIndex": 0}
    ///         ],
    ///         "action": {
    ///             "SetKey":{"key":"hasInvaders","value":1}
    ///         }
    ///     }
    /// }
    /// ```
    /// This says to take the "state" property on entity 576258, navigate to `state["controlPoints"][0]`,
    /// and set the sub-key `hasInvaders` there to 1.
    ///
    /// The following properties and values are known:
    /// - `state["controlPoints"][N]["invaderTeam"]`: Indicates the team ID of the team currently
    ///   contesting the control point. -1 if nobody is invading point.
    /// - `state["controlPoints"][N]["hasInvaders"]`: 1 if the point is being contested, 0 otherwise.
    /// - `state["controlPoints"][N]["progress"]`: A tuple of two elements. The first is the fraction
    ///   captured, ranging from 0 to 1 as the point is captured, and the second is the amount of
    ///   time remaining until the point is captured.
    /// - `state["controlPoints"][N]["bothInside"]`: 1 if both teams are currently in point, 0 otherwise.
    /// - `state["missions"]["teamsScore"][N]["score"]`: The value of team N's score.
    PropertyUpdate(&'rawpacket crate::packet2::PropertyUpdatePacket<'argtype>),
    /// Indicates that the battle has ended
    BattleEnd {
        /// The team ID of the winning team (corresponds to the teamid in [OnArenaStateReceivedPlayer])
        winning_team: Option<i8>,
        /// How the battle ended (from `FINISH_TYPE` in battle.xml)
        finish_type: Option<Recognized<FinishType>>,
    },
    /// Sent when a consumable is activated
    Consumable {
        /// The ship ID of the ship using the consumable
        entity: EntityId,
        /// The consumable
        consumable: Recognized<Consumable>,
        /// How long the consumable will be active for
        duration: f32,
    },
    /// Indicates a change to the "cruise state," which is the fixed settings for various controls
    /// such as steering (using the Q & E keys), throttle, and dive planes.
    CruiseState {
        /// Which cruise state is being affected
        state: CruiseState,
        /// See [CruiseState] for what the values mean.
        value: i32,
    },
    Map(&'rawpacket crate::packet2::MapPacket<'replay>),
    /// A string representation of the game version this replay is from.
    Version(String),
    Camera(&'rawpacket crate::packet2::CameraPacket),
    /// Indicates a change in the current camera mode
    CameraMode(Recognized<CameraMode>),
    /// If true, indicates that the player has enabled the "free look" camera (by holding right click)
    CameraFreeLook(bool),
    /// Artillery shells fired
    ArtilleryShots {
        avatar_id: AvatarId,
        salvos: Vec<ArtillerySalvo>,
    },
    /// Torpedoes launched
    TorpedoesReceived {
        avatar_id: AvatarId,
        torpedoes: Vec<TorpedoData>,
    },
    /// Homing torpedo direction/position update
    TorpedoDirection {
        owner_id: EntityId,
        shot_id: ShotId,
        position: WorldPos,
        target_yaw: f32,
        speed_coef: f32,
    },
    /// Projectile hits (shells or torpedoes hitting targets)
    ShotKills {
        avatar_id: AvatarId,
        hits: Vec<ShotHit>,
    },
    /// Turret rotation sync for a ship
    GunSync {
        entity_id: EntityId,
        /// Gun group (0 = main battery)
        group: u32,
        /// Turret index within the group
        turret: u32,
        /// Turret yaw in radians relative to ship heading (0 = forward, PI = aft)
        yaw: f32,
        /// Barrel elevation in radians
        pitch: f32,
    },
    /// A new squadron appears on the minimap
    PlaneAdded {
        entity_id: EntityId,
        plane_id: PlaneId,
        /// Team index: 0 = recording player's team, 1 = enemy team
        team_id: u32,
        params_id: GameParamId,
        position: WorldPos2D,
    },
    /// A fighter patrol ward is placed (from receive_wardAdded).
    /// This is the game's mechanism for marking patrol circle areas.
    WardAdded {
        entity_id: EntityId,
        plane_id: PlaneId,
        /// Patrol center position (world coordinates)
        position: WorldPos,
        /// Patrol radius in BigWorld units
        radius: BigWorldDistance,
        /// Owner ship entity ID
        owner_id: EntityId,
    },
    /// A fighter patrol ward is removed (from receive_wardRemoved).
    WardRemoved {
        entity_id: EntityId,
        plane_id: PlaneId,
    },
    /// A squadron is removed from the minimap
    PlaneRemoved {
        entity_id: EntityId,
        plane_id: PlaneId,
    },
    /// Plane/squadron position update on the minimap
    PlanePosition {
        entity_id: EntityId,
        plane_id: PlaneId,
        position: WorldPos2D,
    },
    /// Ammo type selected for a weapon group
    SetAmmoForWeapon {
        entity_id: EntityId,
        /// 0 = artillery, 2 = torpedo
        weapon_type: u32,
        /// GameParamId of the projectile (look up ammoType in GameParams)
        ammo_param_id: GameParamId,
        /// True if the player just switched ammo and is reloading
        is_reload: bool,
    },
    /// EntityControl — transfers entity ownership to the client.
    EntityControl(&'rawpacket crate::packet2::EntityControlPacket),
    /// Non-volatile entity position update (no direction/dead-reckoning).
    NonVolatilePosition(&'rawpacket crate::packet2::NonVolatilePositionPacket),
    /// Player network stats: fps (u8), ping in ms (u16), isLaggingNow (bool).
    PlayerNetStats(&'rawpacket crate::packet2::PlayerNetStatsPacket),
    /// Server timestamp at session start.
    ServerTimestamp(f64),
    /// Links the Avatar to its owned ship entity.
    OwnShip(&'rawpacket crate::packet2::OwnShipPacket),
    /// `onSetWeaponLock` — weapon lock state change.
    SetWeaponLock(&'rawpacket crate::packet2::SetWeaponLockPacket),
    /// Server tick rate constant (observed as 1/7).
    ServerTick(f64),
    /// Submarine controller mode change (0/1 toggle, likely surface/dive).
    SubController(&'rawpacket crate::packet2::SubControllerPacket),
    /// Shot tracking change (entity_id + i64 value, fire control related).
    ShotTracking(&'rawpacket crate::packet2::ShotTrackingPacket),
    /// Gun marker / aiming state (target point, marker position/direction/diameter, etc.).
    GunMarker(&'rawpacket crate::packet2::GunMarkerPacket),
    /// Synchronizes physics state for the two hull fragments after a ship is destroyed
    /// and cracks apart. The engine uses `correctBodyFromServer()` to smoothly interpolate
    /// toward the server state rather than snapping. Purely visual — controls the sinking
    /// animation of the two ship halves.
    SyncShipCracks {
        entity_id: EntityId,
        /// Physics body state for crack part 1 (bow or stern half). None if blob is empty.
        state1: Option<PhysicsBodyState>,
        /// Physics body state for crack part 2 (the other half). None if blob is empty.
        state2: Option<PhysicsBodyState>,
    },
    /// Packet 0x10: Init flag at clock=0.
    InitFlag(u8),
    /// Packet 0x13: Empty init marker at clock=0.
    InitMarker,
    /// This is a packet of unknown type
    Unknown(&'replay [u8]),
    /// This is a packet of known type, but which we were unable to parse
    Invalid(&'rawpacket crate::packet2::InvalidPacket<'replay>),
    /// If parsing with audits enabled, this indicates a packet that may be of special interest
    /// for whoever is reading the audits.
    Audit(String),
    /// End of battle results (free xp, damage details, etc.)
    BattleResults(&'replay str),
    /*
    ArtilleryHit(ArtilleryHitPacket<'a>),
    */
}

fn try_convert_hashable_pickle_to_string(value: pickled::value::HashableValue) -> pickled::value::HashableValue {
    match value {
        pickled::value::HashableValue::Bytes(b) => {
            if let Ok(s) = std::str::from_utf8(b.inner()) {
                pickled::value::HashableValue::String(s.to_owned().into())
            } else {
                pickled::value::HashableValue::Bytes(b)
            }
        }
        pickled::value::HashableValue::Tuple(t) => pickled::value::HashableValue::Tuple(
            t.inner().iter().cloned().map(try_convert_hashable_pickle_to_string).collect::<Vec<_>>().into(),
        ),
        pickled::value::HashableValue::FrozenSet(s) => pickled::value::HashableValue::FrozenSet(
            s.inner().iter().cloned().map(try_convert_hashable_pickle_to_string).collect::<BTreeSet<_>>().into(),
        ),
        value => value,
    }
}

/// Helper function to recursively convert byte values to strings where possible.
fn try_convert_pickle_to_string(value: pickled::value::Value) -> pickled::value::Value {
    match value {
        pickled::value::Value::Bytes(b) => {
            if let Ok(s) = std::str::from_utf8(b.inner()) {
                pickled::value::Value::String(s.to_owned().into())
            } else {
                pickled::value::Value::Bytes(b)
            }
        }
        pickled::value::Value::List(l) => pickled::value::Value::List(
            l.inner().iter().cloned().map(try_convert_pickle_to_string).collect::<Vec<_>>().into(),
        ),
        pickled::value::Value::Tuple(t) => pickled::value::Value::Tuple(
            t.inner().iter().cloned().map(try_convert_pickle_to_string).collect::<Vec<_>>().into(),
        ),
        pickled::value::Value::Set(s) => pickled::value::Value::Set(
            s.inner().iter().cloned().map(try_convert_hashable_pickle_to_string).collect::<BTreeSet<_>>().into(),
        ),
        pickled::value::Value::FrozenSet(s) => pickled::value::Value::FrozenSet(
            s.inner().iter().cloned().map(try_convert_hashable_pickle_to_string).collect::<BTreeSet<_>>().into(),
        ),
        pickled::value::Value::Dict(d) => pickled::value::Value::Dict(
            d.inner()
                .iter()
                .map(|(k, v)| {
                    (try_convert_hashable_pickle_to_string(k.clone()), try_convert_pickle_to_string(v.clone()))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
                .into(),
        ),
        value => value,
    }
}

fn parse_receive_common_cmd_blob(blob: &[u8]) -> PResult<(VoiceLine, bool)> {
    let i = &mut &*blob;
    let line = le_u16.parse_next(i)?;
    let audience = le_u8.parse_next(i)?;

    let is_global = match audience {
        0 => false,
        1 => true,
        _ => {
            panic!("Got unknown audience {}", audience);
        }
    };
    let message = match line {
        1 => {
            let x = le_u16.parse_next(i)?;
            let y = le_u16.parse_next(i)?;
            VoiceLine::AttentionToSquare(x as u32, y as u32)
        }
        2 => {
            let target_type = le_u16.parse_next(i)?;
            let target_id = le_u64.parse_next(i)?;
            VoiceLine::QuickTactic(target_type, target_id)
        }
        3 => VoiceLine::RequestingSupport(None),
        // 4 is "QUICK_SOS"
        // 5 is AYE_AYE
        5 => VoiceLine::Wilco,
        // 6 is NO_WAY
        6 => VoiceLine::Negative,
        // GOOD_GAME
        7 => VoiceLine::WellDone, // TODO: Find the corresponding field
        // GOOD_LUCK
        8 => VoiceLine::FairWinds,
        // CARAMBA
        9 => VoiceLine::Curses,
        // 10 -> THANK_YOU
        10 => VoiceLine::DefendTheBase,
        // 11 -> NEED_AIR_DEFENSE
        11 => VoiceLine::ProvideAntiAircraft,
        // BACK
        12 => {
            let _target_type = le_u16.parse_next(i)?;
            let target_id = le_u64.parse_next(i)?;
            VoiceLine::Retreat(if target_id != 0 { Some(target_id as i32) } else { None })
        }
        // NEED_VISION
        13 => VoiceLine::IntelRequired,
        // NEED_SMOKE
        14 => VoiceLine::SetSmokeScreen,
        // RLS
        15 => VoiceLine::UsingRadar,
        // SONAR
        16 => VoiceLine::UsingHydroSearch,
        // FOLLOW_ME
        17 => VoiceLine::FollowMe,
        // MAP_POINT_ATTENTION
        18 => {
            let x = le_f32.parse_next(i)?;
            let y = le_f32.parse_next(i)?;
            VoiceLine::MapPointAttention(x, y)
        }
        //  SUBMARINE_LOCATOR
        19 => VoiceLine::UsingSubmarineLocator,
        line => {
            eprintln!("Warning: Unknown voice line {}, {:#X?}", line, *i);
            VoiceLine::Unknown(line as i64)
        }
    };

    Ok((message, is_global))
}

impl<'replay, 'argtype, 'rawpacket> DecodedPacketPayload<'replay, 'argtype, 'rawpacket>
where
    'rawpacket: 'replay,
    'rawpacket: 'argtype,
{
    fn from(
        version: &Version,
        audit: bool,
        payload: &'rawpacket crate::packet2::PacketType<'replay, 'argtype>,
        _packet_type: u32,
        battle_constants: &wowsunpack::game_constants::BattleConstants,
        common_constants: &wowsunpack::game_constants::CommonConstants,
        ships_constants: &wowsunpack::game_constants::ShipsConstants,
    ) -> Self {
        match payload {
            PacketType::EntityMethod(em) => DecodedPacketPayload::from_entity_method(
                version,
                audit,
                em,
                battle_constants,
                common_constants,
                ships_constants,
            ),
            PacketType::Camera(camera) => DecodedPacketPayload::Camera(camera),
            PacketType::CameraMode(mode) => {
                if let Some(cm) = CameraMode::from_id(*mode as i32, battle_constants, *version) {
                    DecodedPacketPayload::CameraMode(cm)
                } else if audit {
                    DecodedPacketPayload::Audit(format!("CameraMode({})", mode))
                } else {
                    DecodedPacketPayload::CameraMode(Recognized::Unknown(format!("{}", mode)))
                }
            }
            PacketType::CameraFreeLook(freelook) => match freelook {
                0 => DecodedPacketPayload::CameraFreeLook(false),
                1 => DecodedPacketPayload::CameraFreeLook(true),
                _ => {
                    if audit {
                        DecodedPacketPayload::Audit(format!("CameraFreeLook({})", freelook))
                    } else {
                        DecodedPacketPayload::CameraFreeLook(true)
                    }
                }
            },
            PacketType::CruiseState(cs) => match cs.key {
                0 => DecodedPacketPayload::CruiseState { state: CruiseState::Throttle, value: cs.value },
                1 => DecodedPacketPayload::CruiseState { state: CruiseState::Rudder, value: cs.value },
                2 => DecodedPacketPayload::CruiseState { state: CruiseState::DiveDepth, value: cs.value },
                _ => {
                    if audit {
                        DecodedPacketPayload::Audit(format!("CruiseState(unknown={}, {})", cs.key, cs.value))
                    } else {
                        DecodedPacketPayload::CruiseState { state: CruiseState::Unknown(cs.key), value: cs.value }
                    }
                }
            },
            PacketType::Map(map) => {
                if audit && map.unknown != 0 && map.unknown != 1 {
                    DecodedPacketPayload::Audit(format!("Map: Unknown bool is not a bool (is {})", map.unknown))
                } else if audit
                    && map.matrix
                        != [
                            0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0,
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                            0, 0, 0, 0, 0, 128, 63,
                        ]
                {
                    DecodedPacketPayload::Audit(format!("Map: Unit matrix is not a unit matrix (is {:?})", map.matrix))
                } else {
                    DecodedPacketPayload::Map(map)
                }
            }
            PacketType::EntityProperty(p) => DecodedPacketPayload::EntityProperty(p),
            PacketType::Position(pos) => DecodedPacketPayload::Position((*pos).clone()),
            PacketType::PlayerOrientation(pos) => DecodedPacketPayload::PlayerOrientation((*pos).clone()),
            PacketType::BasePlayerCreate(b) => DecodedPacketPayload::BasePlayerCreate(b),
            PacketType::CellPlayerCreate(c) => DecodedPacketPayload::CellPlayerCreate(c),
            PacketType::EntityEnter(e) => DecodedPacketPayload::EntityEnter(e),
            PacketType::EntityLeave(e) => DecodedPacketPayload::EntityLeave(e),
            PacketType::EntityCreate(e) => DecodedPacketPayload::EntityCreate(e),
            PacketType::PropertyUpdate(update) => DecodedPacketPayload::PropertyUpdate(update),
            PacketType::Version(version) => DecodedPacketPayload::Version(version.clone()),
            PacketType::Unknown(u) => DecodedPacketPayload::Unknown(u),
            PacketType::Invalid(u) => DecodedPacketPayload::Invalid(u),
            PacketType::BattleResults(results) => DecodedPacketPayload::BattleResults(results),
            PacketType::EntityControl(ec) => DecodedPacketPayload::EntityControl(ec),
            PacketType::NonVolatilePosition(sd) => DecodedPacketPayload::NonVolatilePosition(sd),
            PacketType::PlayerNetStats(ns) => DecodedPacketPayload::PlayerNetStats(ns),
            PacketType::ServerTimestamp(st) => DecodedPacketPayload::ServerTimestamp(st.timestamp),
            PacketType::OwnShip(os) => DecodedPacketPayload::OwnShip(os),
            PacketType::SetWeaponLock(wl) => DecodedPacketPayload::SetWeaponLock(wl),
            PacketType::ServerTick(tick) => DecodedPacketPayload::ServerTick(*tick),
            PacketType::SubController(sc) => DecodedPacketPayload::SubController(sc),
            PacketType::ShotTracking(st) => DecodedPacketPayload::ShotTracking(st),
            PacketType::GunMarker(gm) => DecodedPacketPayload::GunMarker(gm),
            PacketType::InitFlag(flag) => DecodedPacketPayload::InitFlag(*flag),
            PacketType::InitMarker => DecodedPacketPayload::InitMarker,
        }
    }

    fn extract_vec3(val: Option<&ArgValue>) -> WorldPos {
        match val {
            Some(ArgValue::Vector3((x, y, z))) => WorldPos { x: *x, y: *y, z: *z },
            Some(ArgValue::Array(a)) if a.len() >= 3 => {
                let x: f32 = (&a[0]).try_into().unwrap_or(0.0);
                let y: f32 = (&a[1]).try_into().unwrap_or(0.0);
                let z: f32 = (&a[2]).try_into().unwrap_or(0.0);
                WorldPos { x, y, z }
            }
            _ => WorldPos::default(),
        }
    }

    fn from_entity_method(
        version: &Version,
        audit: bool,
        packet: &'rawpacket EntityMethodPacket<'argtype>,
        battle_constants: &wowsunpack::game_constants::BattleConstants,
        common_constants: &wowsunpack::game_constants::CommonConstants,
        ships_constants: &wowsunpack::game_constants::ShipsConstants,
    ) -> Self {
        let entity_id = &packet.entity_id;
        let method = &packet.method;
        let args = &packet.args;
        if *method == "onChatMessage" {
            let target = match &args[1] {
                ArgValue::String(s) => s,
                _ => panic!("foo"),
            };
            let message = match &args[2] {
                ArgValue::String(s) => s,
                _ => panic!("foo"),
            };
            let sender_id = match &args[0] {
                ArgValue::Int32(i) => i,
                _ => panic!("foo"),
            };
            let mut extra_data = None;
            if *sender_id == 0 && args.len() >= 4 {
                let extra =
                    pickled::de::value_from_slice(args[3].string_ref().expect("failed"), pickled::de::DeOptions::new())
                        .expect("value is not pickled");
                let mut extra_dict: HashMap<String, Value> = HashMap::from_iter(
                    extra.dict().expect("value is not a dictionary").inner().iter().map(|(key, value)| {
                        let key = match key {
                            pickled::HashableValue::Bytes(bytes) => {
                                String::from_utf8(bytes.inner().clone()).expect("key is not a valid utf-8 sequence")
                            }
                            pickled::HashableValue::String(string) => string.inner().clone(),
                            other => {
                                panic!("unexpected key type {:?}", other)
                            }
                        };

                        let value = match value {
                            Value::Bytes(bytes) => {
                                if let Ok(result) = String::from_utf8(bytes.inner().clone()) {
                                    Value::String(result.into())
                                } else {
                                    Value::Bytes(bytes.clone())
                                }
                            }
                            other => other.clone(),
                        };

                        (key, value)
                    }),
                );

                let extra = ChatMessageExtra {
                    pre_battle_sign: extra_dict
                        .remove("preBattleSign")
                        .unwrap()
                        .i64()
                        .expect("preBattleSign is not an i64"),
                    pre_battle_id: extra_dict.remove("prebattleId").unwrap().i64().expect("preBattleId is not an i64"),
                    player_clan_tag: extra_dict
                        .remove("playerClanTag")
                        .unwrap()
                        .string()
                        .expect("playerClanTag is not a string")
                        .inner()
                        .clone(),
                    typ: extra_dict.remove("type").unwrap().i64().expect("type is not an i64"),
                    player_avatar_id: EntityId::from(
                        extra_dict.remove("playerAvatarId").unwrap().i64().expect("playerAvatarId is not an i64"),
                    ),
                    player_name: extra_dict
                        .remove("playerName")
                        .unwrap()
                        .string()
                        .expect("playerName is not a string")
                        .inner()
                        .clone(),
                };

                assert!(extra_dict.is_empty());

                extra_data = Some(extra);
            }
            DecodedPacketPayload::Chat {
                entity_id: *entity_id,
                sender_id: AccountId::from(*sender_id),
                audience: std::str::from_utf8(target).unwrap(),
                message: std::str::from_utf8(message).unwrap(),
                extra_data,
            }
        } else if *method == "receive_CommonCMD" {
            let (sender_id, message, is_global) = if version.is_at_least(&Version::from_client_exe("0,12,8,0")) {
                let sender = *args[0].int_32_ref().expect("receive_CommonCMD: sender is not an i32");

                let blob = args[1].blob_ref().expect("receive_CommonCMD: second argument is not a blob");

                let (message_type, is_global) = match parse_receive_common_cmd_blob(blob.as_ref()) {
                    Ok(result) => result,
                    Err(e) => {
                        eprintln!("Warning: receive_CommonCMD: failed to parse blob: {:?}", e);
                        (VoiceLine::Unknown(0), false)
                    }
                };

                (sender, message_type, is_global)
            } else {
                let (audience, sender_id, line, a, b) = unpack_rpc_args!(args, u8, i32, u8, u32, u64);
                let is_global = match audience {
                    0 => false,
                    1 => true,
                    _ => {
                        panic!(
                            "Got unknown audience {} sender=0x{:x} line={} a={:x} b={:x}",
                            audience, sender_id, line, a, b
                        );
                    }
                };
                let message = match line {
                    1 => VoiceLine::AttentionToSquare(a, b as u32),
                    2 => VoiceLine::QuickTactic(a as u16, b),
                    3 => VoiceLine::RequestingSupport(None),
                    5 => VoiceLine::Wilco,
                    6 => VoiceLine::Negative,
                    7 => VoiceLine::WellDone, // TODO: Find the corresponding field
                    8 => VoiceLine::FairWinds,
                    9 => VoiceLine::Curses,
                    10 => VoiceLine::DefendTheBase,
                    11 => VoiceLine::ProvideAntiAircraft,
                    12 => VoiceLine::Retreat(if b != 0 { Some(b as i32) } else { None }),
                    13 => VoiceLine::IntelRequired,
                    14 => VoiceLine::SetSmokeScreen,
                    15 => VoiceLine::UsingRadar,
                    16 => VoiceLine::UsingHydroSearch,
                    17 => VoiceLine::FollowMe,
                    18 => VoiceLine::MapPointAttention(a as f32, b as f32),
                    19 => VoiceLine::UsingSubmarineLocator,
                    _ => {
                        eprintln!("Warning: Unknown voice line {} a={:x} b={:x}!", line, a, b);
                        VoiceLine::Unknown(line as i64)
                    }
                };

                (sender_id, message, is_global)
            };

            // let (audience, sender_id, line, a, b) = unpack_rpc_args!(args, u8, i32, u8, u32, u64);

            DecodedPacketPayload::VoiceLine { sender_id: AccountId::from(sender_id), is_global, message }
        } else if *method == "onGameRoomStateChanged" {
            let player_states = pickled::de::value_from_slice(
                args[0].blob_ref().expect("player_states arg is not a blob"),
                pickled::de::DeOptions::new(),
            )
            .expect("failed to deserialize player_states");

            let player_states = try_convert_pickle_to_string(player_states);

            let mut players_out = vec![];
            if let pickled::value::Value::List(players) = &player_states {
                for player in players.inner().iter() {
                    let raw_values = convert_flat_dict_to_real_dict(player);

                    let mapped_values = PlayerStateData::convert_raw_dict(&raw_values, version, false);
                    players_out.push(mapped_values);
                }
            }
            DecodedPacketPayload::OnGameRoomStateChanged { player_states: players_out }
        } else if *method == "onArenaStateReceived" {
            let (arg0, arg1) = unpack_rpc_args!(args, i64, i8);

            let value = pickled::de::value_from_slice(
                match &args[2] {
                    ArgValue::Blob(x) => x,
                    _ => panic!("foo"),
                },
                pickled::de::DeOptions::new(),
            )
            .unwrap();

            let value = match value {
                pickled::value::Value::Dict(d) => d,
                _ => panic!(),
            };
            let mut arg2 = HashMap::new();
            for (k, v) in value.inner().iter() {
                let k = match k {
                    pickled::value::HashableValue::I64(i) => *i,
                    _ => panic!(),
                };
                let v = match v {
                    pickled::value::Value::List(l) => l,
                    _ => panic!(),
                };
                let v: Vec<_> = v
                    .inner()
                    .iter()
                    .map(|elem| match elem {
                        pickled::value::Value::Dict(d) => Some(
                            d.inner()
                                .iter()
                                .map(|(k, v)| {
                                    let k = match k {
                                        pickled::value::HashableValue::Bytes(b) => {
                                            std::str::from_utf8(b.inner()).unwrap().to_string()
                                        }
                                        _ => panic!(),
                                    };
                                    let v = format!("{:?}", v);
                                    (k, v)
                                })
                                .collect(),
                        ),
                        pickled::value::Value::None => None,
                        _ => panic!(),
                    })
                    .collect();
                arg2.insert(k, v);
            }

            let value = pickled::de::value_from_slice(
                match &args[3] {
                    ArgValue::Blob(x) => x,
                    _ => panic!("foo"),
                },
                pickled::de::DeOptions::new(),
            )
            .unwrap();
            let value = try_convert_pickle_to_string(value);

            let mut players_out = vec![];
            if let pickled::value::Value::List(players) = &value {
                for player in players.inner().iter() {
                    players_out.push(PlayerStateData::from_pickle(player, version, false));
                }
            }

            let mut bots_out = vec![];
            if let Some(ArgValue::Blob(blob)) = args.get(4)
                && let Ok(value) = pickled::de::value_from_slice(blob, pickled::de::DeOptions::new())
            {
                let value = try_convert_pickle_to_string(value);
                if let pickled::value::Value::List(bots) = &value {
                    for bot in bots.inner().iter() {
                        bots_out.push(PlayerStateData::from_pickle(bot, version, true));
                    }
                }
            }

            DecodedPacketPayload::OnArenaStateReceived {
                arena_id: arg0,
                team_build_type_id: arg1,
                pre_battles_info: arg2,
                player_states: players_out,
                bot_states: bots_out,
            }
        } else if *method == "receiveDamageStat" {
            let value = pickled::de::value_from_slice(
                match &args[0] {
                    ArgValue::Blob(x) => x,
                    _ => panic!("foo"),
                },
                pickled::de::DeOptions::new(),
            )
            .unwrap();

            let mut stats = vec![];
            match value {
                pickled::value::Value::Dict(d) => {
                    for (k, v) in d.inner().iter() {
                        let k = match k {
                            pickled::value::HashableValue::Tuple(t) => {
                                let t = t.inner();
                                assert!(t.len() == 2);
                                (
                                    match &t[0] {
                                        pickled::value::HashableValue::I64(i) => *i,
                                        _ => panic!("foo"),
                                    },
                                    match &t[1] {
                                        pickled::value::HashableValue::I64(i) => *i,
                                        _ => panic!("foo"),
                                    },
                                )
                            }
                            _ => panic!("foo"),
                        };
                        let v = match v {
                            pickled::value::Value::List(t) => {
                                let t = t.inner();
                                assert!(t.len() == 2);
                                (
                                    match &t[0] {
                                        pickled::value::Value::I64(i) => *i,
                                        _ => panic!("foo"),
                                    },
                                    match &t[1] {
                                        pickled::value::Value::F64(i) => *i,
                                        // TODO: This appears in the (17,2) key,
                                        // it is unknown what it means
                                        pickled::value::Value::I64(i) => *i as f64,
                                        _ => panic!("foo"),
                                    },
                                )
                            }
                            _ => panic!("foo"),
                        };
                        //println!("{:?}: {:?}", k, v);

                        stats.push((k, v));
                    }
                }
                _ => panic!("foo"),
            }
            DecodedPacketPayload::DamageStat(stats)
        } else if *method == "receiveVehicleDeath" {
            let (victim, killer, cause) = unpack_rpc_args!(args, i32, i32, u32);
            let cause = if let Some(dc) = DeathCause::from_id(cause as i32, battle_constants, *version) {
                dc
            } else if audit {
                return DecodedPacketPayload::Audit(format!(
                    "receiveVehicleDeath(victim={}, killer={}, unknown cause {})",
                    victim, killer, cause
                ));
            } else {
                Recognized::Unknown(format!("{}", cause))
            };
            DecodedPacketPayload::ShipDestroyed {
                victim: EntityId::from(victim),
                killer: EntityId::from(killer),
                cause,
            }
        } else if *method == "onRibbon" {
            let (ribbon,) = unpack_rpc_args!(args, i8);
            let ribbon = match ribbon {
                1 => Ribbon::TorpedoHit,
                3 => Ribbon::PlaneShotDown,
                4 => Ribbon::Incapacitation,
                5 => Ribbon::Destroyed,
                6 => Ribbon::SetFire,
                7 => Ribbon::Flooding,
                8 => Ribbon::Citadel,
                9 => Ribbon::Defended,
                10 => Ribbon::Captured,
                11 => Ribbon::AssistedInCapture,
                13 => Ribbon::SecondaryHit,
                14 => Ribbon::OverPenetration,
                15 => Ribbon::Penetration,
                16 => Ribbon::NonPenetration,
                17 => Ribbon::Ricochet,
                19 => Ribbon::Spotted,
                21 => Ribbon::DiveBombPenetration,
                25 => Ribbon::RocketPenetration,
                26 => Ribbon::RocketNonPenetration,
                27 => Ribbon::ShotDownByAircraft,
                28 => Ribbon::TorpedoProtectionHit,
                30 => Ribbon::RocketTorpedoProtectionHit,
                31 => Ribbon::DepthChargeHit,
                33 => Ribbon::BuffSeized,
                39 => Ribbon::SonarOneHit,
                40 => Ribbon::SonarTwoHits,
                41 => Ribbon::SonarNeutralized,
                ribbon => {
                    if audit {
                        return DecodedPacketPayload::Audit(format!("onRibbon(unknown ribbon {})", ribbon));
                    } else {
                        Ribbon::Unknown(ribbon)
                    }
                }
            };
            DecodedPacketPayload::Ribbon(ribbon)
        } else if *method == "receiveDamagesOnShip" {
            let mut v = vec![];
            for elem in match &args[0] {
                ArgValue::Array(a) => a,
                _ => panic!(),
            } {
                let map = match elem {
                    ArgValue::FixedDict(m) => m,
                    _ => panic!(),
                };
                let aggressor_raw: i32 = map.get("vehicleID").unwrap().try_into().unwrap();
                v.push(DamageReceived {
                    aggressor: EntityId::from(aggressor_raw),
                    damage: map.get("damage").unwrap().try_into().unwrap(),
                });
            }
            DecodedPacketPayload::DamageReceived { victim: *entity_id, aggressors: v }
        } else if *method == "onCheckGamePing" {
            let (ping,) = unpack_rpc_args!(args, u64);
            DecodedPacketPayload::CheckPing(ping)
        } else if *method == "updateMinimapVisionInfo" {
            let v = match &args[0] {
                ArgValue::Array(a) => a,
                _ => panic!(),
            };
            let mut updates = vec![];
            for minimap_update in v.iter() {
                let minimap_update = match minimap_update {
                    ArgValue::FixedDict(m) => m,
                    _ => panic!(),
                };
                let vehicle_id = minimap_update.get("vehicleID").unwrap();

                let packed_data: u32 = minimap_update.get("packedData").unwrap().try_into().unwrap();
                let update = RawMinimapUpdate::from_bytes(packed_data.to_le_bytes());
                let heading = update.heading() as f32 / 256. * 360. - 180.;

                // Check raw 11-bit values for the sentinel (0, 0) before float
                // conversion to avoid any floating-point precision issues.
                // Raw 0 maps to -2500 in world coords (the Python renderer
                // checks `x != -2500 or y != -2500`).
                let is_sentinel = update.x() == 0 && update.y() == 0;

                let x = update.x() as f32 / 512. - 1.5;
                let y = update.y() as f32 / 512. - 1.5;

                updates.push(MinimapUpdate {
                    entity_id: match vehicle_id {
                        ArgValue::Uint32(u) => EntityId::from(*u),
                        _ => panic!(),
                    },
                    position: NormalizedPos { x, y },
                    heading,
                    is_sentinel,
                    disappearing: update.is_disappearing(),
                    unknown: update.unknown(),
                })
            }

            let args1 = match &args[1] {
                ArgValue::Array(a) => a,
                _ => panic!(),
            };

            DecodedPacketPayload::MinimapUpdate { updates, arg1: args1 }
        } else if *method == "onBattleEnd" {
            let (winning_team, finish_type) = if args.len() >= 2 {
                let (winning_team, raw_finish) = unpack_rpc_args!(args, i8, u8);
                let ft = if let Some(ft) = FinishType::from_id(raw_finish as i32, battle_constants, *version) {
                    ft
                } else {
                    Recognized::Unknown(format!("{}", raw_finish))
                };
                (Some(winning_team), Some(ft))
            } else {
                (None, None)
            };
            DecodedPacketPayload::BattleEnd { winning_team, finish_type }
        } else if *method == "consumableUsed" || *method == "onConsumableUsed" {
            // onConsumableUsed may use different integer width than consumableUsed
            let consumable: i8 = match &args[0] {
                ArgValue::Int8(v) => *v,
                ArgValue::Uint8(v) => *v as i8,
                ArgValue::Int16(v) => *v as i8,
                ArgValue::Uint16(v) => *v as i8,
                ArgValue::Int32(v) => *v as i8,
                other => panic!("onConsumableUsed: unexpected consumable arg type: {:?}", other),
            };
            let duration: f32 = match &args[1] {
                ArgValue::Float32(v) => *v,
                ArgValue::Float64(v) => *v as f32,
                other => panic!("onConsumableUsed: unexpected duration arg type: {:?}", other),
            };
            let raw_consumable = consumable;
            // Try runtime-loaded consumable types from game data first
            let consumable = if let Some(c) = Consumable::from_id(raw_consumable as i32, common_constants, *version) {
                c
            } else if audit {
                return DecodedPacketPayload::Audit(format!(
                    "consumableUsed({},{},{})",
                    entity_id, raw_consumable, duration
                ));
            } else {
                Recognized::Unknown(format!("{}", raw_consumable))
            };

            DecodedPacketPayload::Consumable { entity: *entity_id, consumable, duration }
        } else if *method == "receiveArtilleryShots" {
            let salvos_array = match &args[0] {
                ArgValue::Array(a) => a,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let mut salvos = Vec::new();
            for salvo_val in salvos_array.iter() {
                let salvo_dict = match salvo_val {
                    ArgValue::FixedDict(m) => m,
                    _ => continue,
                };
                let owner_id: i32 = salvo_dict.get("ownerID").and_then(ArgValue::as_i32).unwrap_or(0);
                let params_id: u32 = salvo_dict.get("paramsID").and_then(ArgValue::as_u32).unwrap_or(0);
                let salvo_id: u32 = salvo_dict.get("salvoID").and_then(ArgValue::as_u32).unwrap_or(0);
                let shots_array = match salvo_dict.get("shots") {
                    Some(ArgValue::Array(a)) => a,
                    _ => continue,
                };
                let mut shots = Vec::new();
                for shot_val in shots_array.iter() {
                    let shot_dict = match shot_val {
                        ArgValue::FixedDict(m) => m,
                        _ => continue,
                    };
                    let pos = Self::extract_vec3(shot_dict.get("pos"));
                    let pitch: f32 = shot_dict.get("pitch").and_then(ArgValue::as_f32).unwrap_or(0.0);
                    let speed: f32 = shot_dict.get("speed").and_then(ArgValue::as_f32).unwrap_or(0.0);
                    let tar_pos = Self::extract_vec3(shot_dict.get("tarPos"));
                    let shot_id: u32 = shot_dict.get("shotID").and_then(ArgValue::as_u32).unwrap_or(0);
                    let gun_barrel_id: u16 = match shot_dict.get("gunBarrelID") {
                        Some(ArgValue::Uint16(v)) => *v,
                        Some(ArgValue::Int16(v)) => *v as u16,
                        Some(ArgValue::Uint8(v)) => *v as u16,
                        _ => 0,
                    };
                    let server_time_left: f32 =
                        shot_dict.get("serverTimeLeft").and_then(ArgValue::as_f32).unwrap_or(0.0);
                    let shooter_height: f32 = shot_dict.get("shooterHeight").and_then(ArgValue::as_f32).unwrap_or(0.0);
                    let hit_distance: f32 = shot_dict.get("hitDistance").and_then(ArgValue::as_f32).unwrap_or(0.0);
                    shots.push(ArtilleryShotData {
                        origin: pos,
                        pitch,
                        speed,
                        target: tar_pos,
                        shot_id: ShotId::from(shot_id),
                        gun_barrel_id,
                        server_time_left,
                        shooter_height,
                        hit_distance,
                    });
                }
                salvos.push(ArtillerySalvo {
                    owner_id: EntityId::from(owner_id),
                    params_id: GameParamId::from(params_id),
                    salvo_id,
                    shots,
                });
            }
            DecodedPacketPayload::ArtilleryShots { avatar_id: AvatarId::from(*entity_id), salvos }
        } else if *method == "receiveTorpedoes" {
            let salvos_array = match &args[0] {
                ArgValue::Array(a) => a,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let mut torpedoes = Vec::new();
            for salvo_val in salvos_array.iter() {
                let salvo_dict = match salvo_val {
                    ArgValue::FixedDict(m) => m,
                    _ => continue,
                };
                let owner_id: i32 = salvo_dict.get("ownerID").and_then(ArgValue::as_i32).unwrap_or(0);
                let params_id: u32 = salvo_dict.get("paramsID").and_then(ArgValue::as_u32).unwrap_or(0);
                let salvo_id: u32 = salvo_dict.get("salvoID").and_then(ArgValue::as_u32).unwrap_or(0);
                let skin_id: u32 = salvo_dict.get("skinID").and_then(ArgValue::as_u32).unwrap_or(0);
                let torps_array = match salvo_dict.get("torpedoes") {
                    Some(ArgValue::Array(a)) => a,
                    _ => continue,
                };
                for torp_val in torps_array.iter() {
                    let torp_dict = match torp_val {
                        ArgValue::FixedDict(m) => m,
                        _ => continue,
                    };
                    let pos = Self::extract_vec3(torp_dict.get("pos"));
                    let dir = Self::extract_vec3(torp_dict.get("dir"));
                    let shot_id: u32 = torp_dict.get("shotID").and_then(ArgValue::as_u32).unwrap_or(0);
                    let armed = match torp_dict.get("armed") {
                        Some(ArgValue::Uint8(v)) => *v != 0,
                        Some(ArgValue::Int8(v)) => *v != 0,
                        _ => false,
                    };
                    let maneuver_dump = torp_dict.get("maneuverDump").and_then(|v| {
                        let d = match v {
                            ArgValue::FixedDict(d) => d,
                            ArgValue::NullableFixedDict(Some(d)) => d,
                            _ => return None,
                        };
                        Some(TorpedoManeuverDump {
                            target_yaw: d.get("targetYaw").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            change_time: d.get("changeTime").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            stop_time: d.get("stopTime").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            current_time: d.get("currentTime").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            yaw_speed: d.get("yawSpeed").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            arm_pos: Self::extract_vec3(d.get("armPos")),
                            final_pos: Self::extract_vec3(d.get("finalPos")),
                        })
                    });
                    let acoustic_dump = torp_dict.get("acousticDump").and_then(|v| {
                        let d = match v {
                            ArgValue::FixedDict(d) => d,
                            ArgValue::NullableFixedDict(Some(d)) => d,
                            _ => return None,
                        };
                        Some(TorpedoAcousticDump {
                            is_chasing_target: match d.get("isChasingTarget") {
                                Some(ArgValue::Uint8(v)) => *v != 0,
                                Some(ArgValue::Int8(v)) => *v != 0,
                                _ => false,
                            },
                            prediction_lost: match d.get("predictionLost") {
                                Some(ArgValue::Uint8(v)) => *v != 0,
                                Some(ArgValue::Int8(v)) => *v != 0,
                                _ => false,
                            },
                            modificators_level: match d.get("modificatorsLevel") {
                                Some(ArgValue::Uint8(v)) => *v,
                                Some(ArgValue::Int8(v)) => *v as u8,
                                _ => 0,
                            },
                            activation_time: d.get("activationTime").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            degradation_time: d.get("degradationTime").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            speed_coef: d.get("speedCoef").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            rotation_yaw: d.get("rotationYaw").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            vertical_speed: d.get("verticalSpeed").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            target_yaw: d.get("targetYaw").and_then(ArgValue::as_f32).unwrap_or(0.0),
                            target_depth: d.get("targetDepth").and_then(ArgValue::as_f32).unwrap_or(0.0),
                        })
                    });
                    torpedoes.push(TorpedoData {
                        owner_id: EntityId::from(owner_id),
                        params_id: GameParamId::from(params_id),
                        salvo_id,
                        skin_id: GameParamId::from(skin_id),
                        shot_id: ShotId::from(shot_id),
                        origin: pos,
                        direction: dir,
                        armed,
                        maneuver_dump,
                        acoustic_dump,
                    });
                }
            }
            DecodedPacketPayload::TorpedoesReceived { avatar_id: AvatarId::from(*entity_id), torpedoes }
        } else if *method == "receiveShotKills" {
            // SHOTKILLS_PACK: Array of { ownerID: PLAYER_ID, hitType: UINT8, kills: Array<SHOTKILL> }
            // SHOTKILL: { pos: VECTOR3, shotID: SHOT_ID }
            let packs = match &args[0] {
                ArgValue::Array(a) => a,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let mut hits = Vec::new();
            for pack in packs {
                let pack_dict = match pack {
                    ArgValue::FixedDict(d) => d,
                    _ => continue,
                };
                let owner_id: i32 = pack_dict.get("ownerID").and_then(ArgValue::as_i32).unwrap_or(0);
                let hit_type: u8 = match pack_dict.get("hitType") {
                    Some(ArgValue::Uint8(v)) => *v,
                    Some(ArgValue::Int8(v)) => *v as u8,
                    _ => 0,
                };
                let kills_array = match pack_dict.get("kills") {
                    Some(ArgValue::Array(a)) => a,
                    _ => continue,
                };
                for kill in kills_array {
                    let kill_dict = match kill {
                        ArgValue::FixedDict(d) => d,
                        _ => continue,
                    };
                    let shot_id: u32 = kill_dict.get("shotID").and_then(ArgValue::as_u32).unwrap_or(0);
                    let pos = Self::extract_vec3(kill_dict.get("pos"));
                    let terminal_ballistics = kill_dict.get("terminalBallisticsInfo").and_then(|v| {
                        let d = match v {
                            ArgValue::FixedDict(d) => d,
                            ArgValue::NullableFixedDict(Some(d)) => d,
                            _ => return None,
                        };
                        let position = Self::extract_vec3(d.get("position"));
                        let velocity = Self::extract_vec3(d.get("velocity"));
                        let detonator_activated = match d.get("detonatorActivated") {
                            Some(ArgValue::Uint8(v)) => *v != 0,
                            Some(ArgValue::Int8(v)) => *v != 0,
                            _ => false,
                        };
                        let material_angle = d.get("materialAngle").and_then(ArgValue::as_f32).unwrap_or(0.0);
                        Some(TerminalBallisticsInfo { position, velocity, detonator_activated, material_angle })
                    });
                    hits.push(ShotHit {
                        owner_id: EntityId::from(owner_id),
                        hit_type: HitType::from_raw(hit_type, ships_constants, version),
                        shot_id: ShotId::from(shot_id),
                        position: pos,
                        terminal_ballistics,
                    });
                }
            }
            DecodedPacketPayload::ShotKills { avatar_id: AvatarId::from(*entity_id), hits }
        } else if *method == "receiveTorpedoDirection" {
            // args: [owner_id, shot_id, position, target_yaw, ?, speed_coef, rotation_yaw, ?, is_chasing]
            let owner_id: EntityId = match &args[0] {
                ArgValue::Int32(v) => EntityId::from(*v),
                ArgValue::Uint32(v) => EntityId::from(*v),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let shot_id: ShotId = match &args[1] {
                ArgValue::Int32(v) => ShotId::from(*v as u32),
                ArgValue::Uint32(v) => ShotId::from(*v),
                ArgValue::Uint8(v) => ShotId::from(*v as u32),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let position = Self::extract_vec3(Some(&args[2]));
            let target_yaw = match &args[3] {
                ArgValue::Float32(v) => *v,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let speed_coef = match &args[5] {
                ArgValue::Float32(v) => *v,
                _ => 1.0,
            };
            DecodedPacketPayload::TorpedoDirection { owner_id, shot_id, position, target_yaw, speed_coef }
        } else if *method == "receive_addMinimapSquadron" {
            // args: [plane_id, team_id, params_id, position, unknown]
            let plane_id: PlaneId = match &args[0] {
                ArgValue::Uint64(v) => PlaneId::from(*v),
                ArgValue::Int64(v) => PlaneId::from(*v),
                ArgValue::Uint32(v) => PlaneId::from(*v as u64),
                ArgValue::Int32(v) => PlaneId::from(*v as i64),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let team_id: u32 = match &args[1] {
                ArgValue::Uint32(v) => *v,
                ArgValue::Int32(v) => *v as u32,
                ArgValue::Uint64(v) => *v as u32,
                ArgValue::Int64(v) => *v as u32,
                ArgValue::Uint8(v) => *v as u32,
                ArgValue::Int8(v) => *v as u32,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let params_id: u64 = match &args[2] {
                ArgValue::Uint64(v) => *v,
                ArgValue::Int64(v) => *v as u64,
                ArgValue::Uint32(v) => *v as u64,
                ArgValue::Int32(v) => *v as u64,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let position = match &args[3] {
                ArgValue::Array(a) if a.len() >= 2 => {
                    let x: f32 = (&a[0]).try_into().unwrap_or(0.0);
                    let y: f32 = (&a[1]).try_into().unwrap_or(0.0);
                    (x, y)
                }
                ArgValue::Vector2((x, y)) => (*x, *y),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::PlaneAdded {
                entity_id: *entity_id,
                plane_id,
                team_id,
                params_id: GameParamId::from(params_id),
                position: WorldPos2D { x: position.0, z: position.1 },
            }
        } else if *method == "receive_removeMinimapSquadron" {
            let plane_id: PlaneId = match &args[0] {
                ArgValue::Uint64(v) => PlaneId::from(*v),
                ArgValue::Int64(v) => PlaneId::from(*v),
                ArgValue::Uint32(v) => PlaneId::from(*v as u64),
                ArgValue::Int32(v) => PlaneId::from(*v as i64),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::PlaneRemoved { entity_id: *entity_id, plane_id }
        } else if *method == "receive_updateMinimapSquadron" {
            let plane_id: PlaneId = match &args[0] {
                ArgValue::Uint64(v) => PlaneId::from(*v),
                ArgValue::Int64(v) => PlaneId::from(*v),
                ArgValue::Uint32(v) => PlaneId::from(*v as u64),
                ArgValue::Int32(v) => PlaneId::from(*v as i64),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let position = match &args[1] {
                ArgValue::Array(a) if a.len() >= 2 => {
                    let x: f32 = (&a[0]).try_into().unwrap_or(0.0);
                    let y: f32 = (&a[1]).try_into().unwrap_or(0.0);
                    (x, y)
                }
                ArgValue::Vector2((x, y)) => (*x, *y),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::PlanePosition {
                entity_id: *entity_id,
                plane_id,
                position: WorldPos2D { x: position.0, z: position.1 },
            }
        } else if *method == "receive_wardAdded" {
            // args: [squadronId, position, unknown, radius, relation, ownerId, unknown2]
            let plane_id: PlaneId = match &args[0] {
                ArgValue::Uint64(v) => PlaneId::from(*v),
                ArgValue::Int64(v) => PlaneId::from(*v),
                ArgValue::Uint32(v) => PlaneId::from(*v as u64),
                ArgValue::Int32(v) => PlaneId::from(*v as i64),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let position = match &args[1] {
                ArgValue::Vector3((x, y, z)) => WorldPos { x: *x, y: *y, z: *z },
                ArgValue::Array(a) if a.len() >= 3 => {
                    let x: f32 = (&a[0]).try_into().unwrap_or(0.0);
                    let y: f32 = (&a[1]).try_into().unwrap_or(0.0);
                    let z: f32 = (&a[2]).try_into().unwrap_or(0.0);
                    WorldPos { x, y, z }
                }
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let radius: f32 = match &args[3] {
                ArgValue::Float32(v) => *v,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let owner_id: EntityId = match &args[5] {
                ArgValue::Uint32(v) => EntityId::from(*v),
                ArgValue::Int32(v) => EntityId::from(*v),
                ArgValue::Uint64(v) => EntityId::from(*v as u32),
                ArgValue::Int64(v) => EntityId::from(*v),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::WardAdded {
                entity_id: *entity_id,
                plane_id,
                position,
                radius: BigWorldDistance::from(radius),
                owner_id,
            }
        } else if *method == "receive_wardRemoved" {
            // args: [squadronId]
            let plane_id: PlaneId = match &args[0] {
                ArgValue::Uint64(v) => PlaneId::from(*v),
                ArgValue::Int64(v) => PlaneId::from(*v),
                ArgValue::Uint32(v) => PlaneId::from(*v as u64),
                ArgValue::Int32(v) => PlaneId::from(*v as i64),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::WardRemoved { entity_id: *entity_id, plane_id }
        } else if *method == "syncGun" {
            // args: [group: int, turret: int, yaw: f32, pitch: f32, state: int, f32, array]
            let group = match &args[0] {
                ArgValue::Uint8(v) => *v as u32,
                ArgValue::Int8(v) => *v as u32,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let turret = match &args[1] {
                ArgValue::Uint8(v) => *v as u32,
                ArgValue::Int8(v) => *v as u32,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let yaw = match &args[2] {
                ArgValue::Float32(v) => *v,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let pitch = match &args[3] {
                ArgValue::Float32(v) => *v,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::GunSync { entity_id: *entity_id, group, turret, yaw, pitch }
        } else if *method == "setAmmoForWeapon" {
            // args: [weaponType: u8, ammoParamsId: u32, isReload: bool (optional in older replays)]
            let weapon_type = match &args[0] {
                ArgValue::Uint8(v) => *v as u32,
                ArgValue::Int8(v) => *v as u32,
                ArgValue::Uint32(v) => *v,
                ArgValue::Int32(v) => *v as u32,
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let ammo_param_id = match &args[1] {
                ArgValue::Uint32(v) => GameParamId::from(*v),
                ArgValue::Int32(v) => GameParamId::from(*v as u32),
                ArgValue::Uint64(v) => GameParamId::from(*v),
                ArgValue::Int64(v) => GameParamId::from(*v),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let is_reload = if args.len() > 2 {
                match &args[2] {
                    ArgValue::Uint8(v) => *v != 0,
                    ArgValue::Int8(v) => *v != 0,
                    _ => false,
                }
            } else {
                false
            };
            DecodedPacketPayload::SetAmmoForWeapon { entity_id: *entity_id, weapon_type, ammo_param_id, is_reload }
        } else if *method == "syncShipCracks" {
            // args: [state1: BLOB, state2: BLOB] — 72-byte physics body states for each hull half
            let raw1 = match &args[0] {
                ArgValue::String(s) => s.as_slice(),
                ArgValue::Blob(b) => b.as_slice(),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            let raw2 = match &args[1] {
                ArgValue::String(s) => s.as_slice(),
                ArgValue::Blob(b) => b.as_slice(),
                _ => return DecodedPacketPayload::EntityMethod(packet),
            };
            DecodedPacketPayload::SyncShipCracks {
                entity_id: *entity_id,
                state1: PhysicsBodyState::parse(raw1),
                state2: PhysicsBodyState::parse(raw2),
            }
        } else {
            DecodedPacketPayload::EntityMethod(packet)
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DecodedPacket<'replay, 'argtype, 'rawpacket> {
    pub packet_type: u32,
    pub clock: crate::types::GameClock,
    pub payload: DecodedPacketPayload<'replay, 'argtype, 'rawpacket>,
    /// Bytes remaining after parsing. Non-empty means the parser didn't consume
    /// the full packet payload.
    #[serde(skip_serializing_if = "<[u8]>::is_empty")]
    pub leftover: &'replay [u8],
}

/// Reusable packet decoder that holds version and game constants.
///
/// Create once per replay, then call `decode()` for each packet.
#[derive(bon::Builder)]
pub struct PacketDecoder<'a> {
    version: Version,
    #[builder(default)]
    audit: bool,
    #[builder(default = &DEFAULT_BATTLE_CONSTANTS)]
    battle_constants: &'a wowsunpack::game_constants::BattleConstants,
    #[builder(default = &DEFAULT_COMMON_CONSTANTS)]
    common_constants: &'a wowsunpack::game_constants::CommonConstants,
    #[builder(default = &DEFAULT_SHIPS_CONSTANTS)]
    ships_constants: &'a wowsunpack::game_constants::ShipsConstants,
}

impl<'a> PacketDecoder<'a> {
    pub fn decode<'replay, 'argtype, 'rawpacket>(
        &self,
        packet: &'rawpacket Packet<'_, '_>,
    ) -> DecodedPacket<'replay, 'argtype, 'rawpacket>
    where
        'rawpacket: 'replay,
        'rawpacket: 'argtype,
    {
        DecodedPacket {
            clock: packet.clock,
            packet_type: packet.packet_type,
            payload: DecodedPacketPayload::from(
                &self.version,
                self.audit,
                &packet.payload,
                packet.packet_type,
                self.battle_constants,
                self.common_constants,
                self.ships_constants,
            ),
            leftover: packet.leftover,
        }
    }
}

struct Decoder {
    silent: bool,
    output: Option<Box<dyn std::io::Write>>,
    packet_decoder: PacketDecoder<'static>,
}

impl Decoder {
    fn write(&mut self, line: &str) {
        if !self.silent {
            match self.output.as_mut() {
                Some(f) => {
                    writeln!(f, "{}", line).unwrap();
                }
                None => {
                    println!("{}", line);
                }
            }
        }
    }
}

mod raw_minimap_update {
    #![allow(dead_code)]
    use modular_bitfield::prelude::*;

    #[bitfield]
    #[derive(Debug)]
    pub(crate) struct RawMinimapUpdate {
        pub x: B11,
        pub y: B11,
        pub heading: B8,
        pub unknown: bool,
        pub is_disappearing: bool,
    }
}
use raw_minimap_update::RawMinimapUpdate;

impl Analyzer for Decoder {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &Packet<'_, '_>) {
        let decoded = self.packet_decoder.decode(packet);
        //println!("{:#?}", decoded);
        //println!("{}", serde_json::to_string_pretty(&decoded).unwrap());
        let encoded = serde_json::to_string(&decoded).unwrap();
        self.write(&encoded);
    }
}

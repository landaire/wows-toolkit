use std::collections::HashMap;
use std::sync::LazyLock;

#[cfg(feature = "parsing")]
use std::borrow::Cow;
pub use wowsunpack::game_constants::BattleConstants;
pub use wowsunpack::game_constants::ChannelConstants;
pub use wowsunpack::game_constants::CommonConstants;
pub use wowsunpack::game_constants::ShipsConstants;
pub use wowsunpack::game_constants::WeaponsConstants;
#[cfg(feature = "vfs")]
use wowsunpack::vfs::VfsPath;

pub static DEFAULT_GAME_CONSTANTS: LazyLock<GameConstants> = LazyLock::new(GameConstants::defaults);

/// Composed game constants that knows which sub-constants are needed.
#[derive(Clone)]
pub struct GameConstants {
    battle: BattleConstants,
    ships: ShipsConstants,
    weapons: WeaponsConstants,
    common: CommonConstants,
    channel: ChannelConstants,
    player_num_member_map: HashMap<String, i64>,
    bot_num_member_map: HashMap<String, i64>,
}

impl GameConstants {
    /// Load all constants from game files via VFS.
    #[cfg(feature = "vfs")]
    pub fn from_vfs(vfs: &VfsPath) -> Self {
        use wowsunpack::game_constants::load_battle_constants;
        use wowsunpack::game_constants::load_channel_constants;
        use wowsunpack::game_constants::load_common_constants;
        use wowsunpack::game_constants::load_ships_constants;
        use wowsunpack::game_constants::load_weapons_constants;
        Self {
            battle: load_battle_constants(vfs),
            ships: load_ships_constants(vfs),
            weapons: load_weapons_constants(vfs),
            common: load_common_constants(vfs),
            channel: load_channel_constants(vfs),
            player_num_member_map: HashMap::new(),
            bot_num_member_map: HashMap::new(),
        }
    }

    /// The one recipe for a build's decode constants: the game's own
    /// constants when a VFS is available (defaults otherwise), overlaid with
    /// wows-constants overrides, then the per-version consumable table (the
    /// replay's client version is authoritative for consumable ids, so it is
    /// applied last and wins).
    #[cfg(feature = "vfs")]
    pub fn for_build(
        vfs: Option<&VfsPath>,
        overrides: Option<&serde_json::Value>,
        version: Option<wowsunpack::data::Version>,
    ) -> Self {
        let mut constants = match vfs {
            Some(vfs) => Self::from_vfs(vfs),
            None => Self::defaults(),
        };
        if let Some(overrides) = overrides {
            constants.merge_replay_constants(overrides, version.unwrap_or_default());
        }
        if let Some(version) = version {
            wowsunpack::game_constants::apply_version_consumables(constants.common_mut(), version);
        }
        constants
    }

    /// Hardcoded defaults (no game files needed).
    pub fn defaults() -> Self {
        Self {
            battle: BattleConstants::defaults(),
            ships: ShipsConstants::defaults(),
            weapons: WeaponsConstants::defaults(),
            common: CommonConstants::defaults(),
            channel: ChannelConstants::defaults(),
            player_num_member_map: HashMap::new(),
            bot_num_member_map: HashMap::new(),
        }
    }

    pub fn battle(&self) -> &BattleConstants {
        &self.battle
    }

    pub fn ships(&self) -> &ShipsConstants {
        &self.ships
    }

    pub fn weapons(&self) -> &WeaponsConstants {
        &self.weapons
    }

    pub fn common(&self) -> &CommonConstants {
        &self.common
    }

    pub fn channel(&self) -> &ChannelConstants {
        &self.channel
    }

    pub fn player_num_member_map(&self) -> &HashMap<String, i64> {
        &self.player_num_member_map
    }

    pub fn bot_num_member_map(&self) -> &HashMap<String, i64> {
        &self.bot_num_member_map
    }

    pub fn game_mode_name(&self, id: i32) -> Option<&str> {
        self.battle.game_mode(id)
    }

    pub fn death_reason_name(&self, id: i32) -> Option<&str> {
        self.battle.death_reason(id)
    }

    pub fn camera_mode_name(&self, id: i32) -> Option<&str> {
        self.battle.camera_mode(id)
    }

    pub fn battle_mut(&mut self) -> &mut BattleConstants {
        &mut self.battle
    }

    pub fn ships_mut(&mut self) -> &mut ShipsConstants {
        &mut self.ships
    }

    pub fn weapons_mut(&mut self) -> &mut WeaponsConstants {
        &mut self.weapons
    }

    pub fn common_mut(&mut self) -> &mut CommonConstants {
        &mut self.common
    }

    pub fn channel_mut(&mut self) -> &mut ChannelConstants {
        &mut self.channel
    }

    /// Merge replay constants JSON (from wows-constants repo) into this instance.
    ///
    /// Overrides replay field maps, consumable IDs, and battle stages from JSON data.
    /// The `version` is forwarded to version-aware battle stage parsing.
    #[cfg(feature = "parsing")]
    pub fn merge_replay_constants(&mut self, replay_constants: &serde_json::Value, version: wowsunpack::data::Version) {
        merge_num_member_map(&mut self.player_num_member_map, replay_constants, "PLAYER_NUM_MEMBER_MAP");
        merge_num_member_map(&mut self.bot_num_member_map, replay_constants, "BOT_NUM_MEMBER_MAP");
        if let Some(consumable_ids) = replay_constants.pointer("/CONSUMABLE_IDS").and_then(|ids| ids.as_object()) {
            let types = self.common.consumable_types_mut();
            for (key, value) in consumable_ids {
                if let Some(id) = value.as_i64() {
                    types.insert(id as i32, Cow::Owned(key.clone()));
                }
            }
        }
        if let Some(battle_stages) = replay_constants.pointer("/BATTLE_STAGES").and_then(|s| s.as_object()) {
            let stages = self.common.battle_stages_mut();
            for (key, value) in battle_stages {
                if let Some(id) = value.as_i64()
                    && let Some(stage) = wowsunpack::game_types::BattleStage::from_name(key, version).into_known()
                {
                    stages.insert(id as i32, stage);
                }
            }
        }
    }
}

#[cfg(feature = "parsing")]
fn merge_num_member_map(target: &mut HashMap<String, i64>, constants: &serde_json::Value, name: &str) {
    let Some(fields) = constants.get(name).and_then(serde_json::Value::as_object) else { return };
    for (index, field) in fields {
        let Some(index) = index.parse::<i64>().ok() else { continue };
        let Some(field) = field.as_str() else { continue };
        target.insert(field.to_string(), index);
    }
}

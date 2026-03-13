use std::sync::LazyLock;

#[cfg(feature = "parsing")]
use std::borrow::Cow;
pub use wowsunpack::game_constants::BattleConstants;
pub use wowsunpack::game_constants::ChannelConstants;
pub use wowsunpack::game_constants::CommonConstants;
pub use wowsunpack::game_constants::ShipsConstants;
pub use wowsunpack::game_constants::WeaponsConstants;
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
}

impl GameConstants {
    /// Load all constants from game files via VFS.
    pub fn from_vfs(vfs: &VfsPath) -> Self {
        Self {
            battle: BattleConstants::load(vfs),
            ships: ShipsConstants::load(vfs),
            weapons: WeaponsConstants::load(vfs),
            common: CommonConstants::load(vfs),
            channel: ChannelConstants::load(vfs),
        }
    }

    /// Hardcoded defaults (no game files needed).
    pub fn defaults() -> Self {
        Self {
            battle: BattleConstants::defaults(),
            ships: ShipsConstants::defaults(),
            weapons: WeaponsConstants::defaults(),
            common: CommonConstants::defaults(),
            channel: ChannelConstants::defaults(),
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
    /// Overrides `CONSUMABLE_IDS` and `BATTLE_STAGES` mappings from the JSON data.
    /// The `build` number is used for version-aware battle stage parsing.
    #[cfg(feature = "parsing")]
    pub fn merge_replay_constants(&mut self, replay_constants: &serde_json::Value, build: u32) {
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
            let version = wowsunpack::data::Version { major: 0, minor: 0, patch: 0, build };
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

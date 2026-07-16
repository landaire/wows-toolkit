//! Egui-free build (loadout + skills) types and the result-only holders for
//! achievements, ribbons, and consumables.

/// Information about a player's skill build.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SkillInfo {
    pub skill_points: usize,
    pub num_skills: usize,
    pub highest_tier: usize,
    pub num_tier_1_skills: usize,
}

/// A translated consumable ability.
#[derive(Clone, Debug, serde::Serialize)]
pub struct TranslatedAbility {
    pub name: Option<String>,
    pub game_params_name: String,
}

/// A translated ship module (upgrade).
#[derive(Clone, Debug, serde::Serialize)]
pub struct TranslatedModule {
    pub name: Option<String>,
    pub description: Option<String>,
    pub game_params_name: String,
}

/// A player's complete translated build including modules, abilities, and skills.
#[derive(Clone, Debug, serde::Serialize)]
pub struct TranslatedBuild {
    /// Upgrade slots in slot order; `None` is an empty slot. Length is the ship's
    /// total modernization slot count.
    pub modernization_slots: Vec<Option<TranslatedModule>>,
    /// Mounted combat signal flags (game_params_name = Param::name() = icon key).
    pub signals: Vec<TranslatedModule>,
    /// Equipped tech-tree modules (hull, guns, fire control, engine, ...) from the
    /// ship-config unit slots. Populated for every replay version that carries a
    /// ship config, so old and new replays show the same loadout view.
    pub loadout: Vec<TranslatedModule>,
    pub abilities: Vec<TranslatedAbility>,
    pub captain_skills: Option<Vec<wowsunpack::game_params::skill_grid_data::SkillGridRow>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AchievementResult {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub count: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RibbonResult {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub is_subribbon: bool,
    pub count: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ConsumableResult {
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub charges_used: u32,
    pub total_charges: wowsunpack::game_types::ChargeCount,
}

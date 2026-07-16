//! Egui-free build (loadout + skills) types and the result-only holders for
//! achievements, ribbons, and consumables.

use wows_replays::analyzer::battle_controller::Player;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;

/// Reproduce the old "static description, else generated-from-modifiers" rule
/// against a `ParamDescription`. The generated fallback is built from
/// `Formatted` lines only, matching `generated_param_description` (which drops
/// modifiers with no settings-table entry). Unresolved lines are excluded.
fn static_or_generated(d: &wowsunpack::game_params::describe::ParamDescription) -> Option<String> {
    use wowsunpack::game_params::describe::ModifierResolution;
    d.description.clone().or_else(|| {
        let lines: Vec<&str> = d
            .modifier_descriptions
            .iter()
            .filter(|m| m.resolution == ModifierResolution::Formatted)
            .map(|m| m.text.as_str())
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    })
}

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

impl TranslatedBuild {
    pub fn new(player: &Player, metadata_provider: &GameMetadataProvider, version: &Version) -> Option<Self> {
        let vehicle_entity = player.vehicle_entity()?;
        let config = vehicle_entity.props().ship_config();
        let species = *player.vehicle().species()?.known()?;
        let result = Self {
            modernization_slots: {
                let ship = player.vehicle();
                let slot_count = wowsunpack::game_params::types::modernization_slot_count(
                    <GameMetadataProvider as GameParamProvider>::params(metadata_provider),
                    ship,
                );
                let mut slots: Vec<Option<TranslatedModule>> = vec![None; slot_count];
                for id in config.modernization() {
                    let Some(param) =
                        <GameMetadataProvider as GameParamProvider>::game_param_by_id(metadata_provider, *id)
                    else {
                        continue;
                    };
                    use wowsunpack::game_params::describe::DescribeContext;
                    let game_params_name = param.name().to_string();
                    let ctx = DescribeContext {
                        resource_loader: metadata_provider,
                        version,
                        species: Some(species),
                        param_name: None,
                    };
                    let described = param.describe(&ctx);
                    let name = described.name.clone();
                    let description = static_or_generated(&described);
                    let module = TranslatedModule { name, description, game_params_name };
                    match param.modernization().and_then(|m| m.slot()) {
                        Some(i) if (i as usize) < slots.len() => slots[i as usize] = Some(module),
                        _ => slots.push(Some(module)),
                    }
                }
                slots
            },
            signals: config
                .exteriors()
                .iter()
                .filter_map(|id| <GameMetadataProvider as GameParamProvider>::game_param_by_id(metadata_provider, *id))
                .filter(|param| {
                    matches!(
                        param.species().and_then(|r| r.known()),
                        Some(wowsunpack::game_params::types::Species::Flags)
                    )
                })
                .map(|param| {
                    use wowsunpack::game_params::describe::DescribeContext;
                    let game_params_name = param.name().to_string();
                    let ctx = DescribeContext {
                        resource_loader: metadata_provider,
                        version,
                        species: Some(species),
                        param_name: None,
                    };
                    let described = param.describe(&ctx);
                    let name = described.name.clone();
                    let description = static_or_generated(&described);
                    TranslatedModule { name, description, game_params_name }
                })
                .collect(),
            loadout: config
                .units()
                .iter()
                .filter(|id| id.raw() != 0)
                .filter_map(|id| {
                    use wowsunpack::game_params::describe::DescribeContext;
                    let param = <GameMetadataProvider as GameParamProvider>::game_param_by_id(metadata_provider, *id)?;
                    let game_params_name = param.name().to_string();
                    let ctx = DescribeContext {
                        resource_loader: metadata_provider,
                        version,
                        species: Some(species),
                        param_name: None,
                    };
                    let name = param.display_name(&ctx);

                    Some(TranslatedModule { name, description: None, game_params_name })
                })
                .collect(),
            abilities: config
                .abilities()
                .iter()
                .filter_map(|id| {
                    use wowsunpack::game_params::describe::DescribeContext;
                    let param = <GameMetadataProvider as GameParamProvider>::game_param_by_id(metadata_provider, *id)?;
                    let game_params_name = param.name().to_string();
                    let ctx = DescribeContext {
                        resource_loader: metadata_provider,
                        version,
                        species: Some(species),
                        param_name: None,
                    };
                    let name = param.display_name(&ctx);

                    Some(TranslatedAbility { name, game_params_name })
                })
                .collect(),
            captain_skills: vehicle_entity.captain().and_then(|c| c.data().crew_ref()).map(|crew| {
                let learned: std::collections::HashSet<wowsunpack::game_params::types::CrewSkillType> = vehicle_entity
                    .commander_skills_raw(species)
                    .iter()
                    .map(|s| wowsunpack::game_params::types::CrewSkillType::from(*s))
                    .collect();
                wowsunpack::game_params::skill_grid_data::build_skill_grid(
                    Some(crew),
                    &learned,
                    species,
                    metadata_provider,
                    version,
                )
            }),
        };

        Some(result)
    }
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

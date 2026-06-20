use std::collections::HashMap;
use std::sync::Arc;

use egui::Color32;
use egui::RichText;
use serde::Serialize;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleEntity;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::data::wows_data::GameAsset;
use crate::data::wows_data::WorldOfWarshipsData;

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

/// Returns the ship class icon for a given species.
pub fn ship_class_icon_from_species(species: Species, wows_data: &WorldOfWarshipsData) -> Option<Arc<GameAsset>> {
    wows_data.ship_icons.get(&species).cloned()
}

/// Information about a player's skill build.
#[derive(Clone, Serialize)]
pub struct SkillInfo {
    pub skill_points: usize,
    pub num_skills: usize,
    pub highest_tier: usize,
    pub num_tier_1_skills: usize,
    #[serde(skip)]
    pub hover_text: Option<String>,
    #[serde(skip)]
    pub label_text: RichText,
}

/// Damage breakdown by type.
#[derive(Clone, Serialize)]
pub struct Damage {
    pub ap: Option<u64>,
    pub sap: Option<u64>,
    pub he: Option<u64>,
    pub he_secondaries: Option<u64>,
    pub sap_secondaries: Option<u64>,
    pub torps: Option<u64>,
    pub deep_water_torps: Option<u64>,
    pub fire: Option<u64>,
    pub flooding: Option<u64>,
}

/// Hit counts by weapon type.
#[derive(Clone, Serialize)]
pub struct Hits {
    pub ap: Option<u64>,
    pub sap: Option<u64>,
    pub he: Option<u64>,
    pub he_secondaries: Option<u64>,
    pub sap_secondaries: Option<u64>,
    pub ap_secondaries_manual: Option<u64>,
    pub he_secondaries_manual: Option<u64>,
    pub sap_secondaries_manual: Option<u64>,
    pub torps: Option<u64>,
}

/// Potential damage breakdown by source type.
#[derive(Clone, Serialize)]
pub struct PotentialDamage {
    pub artillery: u64,
    pub torpedoes: u64,
    pub planes: u64,
}

/// A translated consumable ability.
#[derive(Clone, Serialize)]
pub struct TranslatedAbility {
    pub name: Option<String>,
    pub game_params_name: String,
}

/// A translated ship module (upgrade).
#[derive(Clone, Serialize)]
pub struct TranslatedModule {
    pub name: Option<String>,
    pub description: Option<String>,
    pub game_params_name: String,
}

/// A player's complete translated build including modules, abilities, and skills.
#[derive(Clone, Serialize)]
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
                    version.build_number().expect("replay version carries a build"),
                    metadata_provider,
                    version,
                )
            }),
        };

        Some(result)
    }
}

/// Damage interaction between two players.
#[derive(Debug, Default)]
pub struct DamageInteraction {
    pub damage_dealt: u64,
    pub damage_dealt_text: String,
    /// Per-type breakdown (e.g. "AP: 5,000\nDepth Charge (Direct): 3,000")
    pub damage_dealt_hover_text: String,
    /// % of this player's total dealt damage
    pub damage_dealt_percentage: f64,
    pub damage_dealt_percentage_text: String,
    /// % of the victim's total received damage
    pub damage_dealt_inverse_percentage: f64,
    pub damage_dealt_inverse_percentage_text: String,
    pub damage_received: u64,
    pub damage_received_text: String,
    /// Per-type breakdown of received damage
    pub damage_received_hover_text: String,
    /// % of this player's total received damage
    pub damage_received_percentage: f64,
    pub damage_received_percentage_text: String,
    /// % of the attacker's total dealt damage
    pub damage_received_inverse_percentage: f64,
    pub damage_received_inverse_percentage_text: String,
}

impl DamageInteraction {
    pub fn damage_dealt(&self) -> u64 {
        self.damage_dealt
    }

    pub fn damage_dealt_percentage(&self) -> f64 {
        self.damage_dealt_percentage
    }

    pub fn damage_received(&self) -> u64 {
        self.damage_received
    }

    pub fn damage_received_percentage(&self) -> f64 {
        self.damage_received_percentage
    }
}

/// An achievement earned in battle.
#[derive(Clone)]
pub struct Achievement {
    pub game_param: Arc<Param>,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub count: usize,
}

/// A ribbon earned in battle.
#[derive(Clone)]
pub struct Ribbon {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub is_subribbon: bool,
    pub count: u64,
}

/// One consumable slot equipped on a player's ship, with charge usage
/// resolved from the battle controller's activation log.
#[derive(Clone)]
pub struct PlayerConsumable {
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub charges_used: u32,
    pub total_charges: wowsunpack::game_types::ChargeCount,
}

/// Report for a single player in a battle.
pub struct PlayerReport {
    pub player: Arc<Player>,
    pub color: Color32,
    pub name_text: RichText,
    pub clan_text: Option<RichText>,
    pub ship_species_text: String,
    pub icon: Option<Arc<GameAsset>>,
    pub division_label: Option<String>,
    pub base_xp: Option<i64>,
    pub base_xp_text: Option<RichText>,
    pub raw_xp: Option<i64>,
    pub raw_xp_text: Option<String>,
    pub observed_damage: u64,
    pub observed_damage_text: String,
    pub actual_damage: Option<u64>,
    pub actual_damage_report: Option<Damage>,
    pub actual_damage_text: Option<RichText>,
    /// RichText to support monospace font
    pub actual_damage_hover_text: Option<RichText>,
    pub hits: Option<u64>,
    pub hits_report: Option<Hits>,
    pub hits_text: Option<RichText>,
    /// RichText to support monospace font
    pub hits_hover_text: Option<RichText>,
    pub ship_name: String,
    pub spotting_damage: Option<u64>,
    pub spotting_damage_text: Option<String>,
    pub spotting_damage_hover_text: Option<RichText>,
    pub potential_damage: Option<u64>,
    pub potential_damage_text: Option<String>,
    pub potential_damage_hover_text: Option<RichText>,
    pub potential_damage_report: Option<PotentialDamage>,
    pub time_lived_secs: Option<u64>,
    pub time_lived_text: Option<String>,
    pub skill_info: SkillInfo,
    pub received_damage: Option<u64>,
    pub received_damage_text: Option<RichText>,
    pub received_damage_hover_text: Option<RichText>,
    pub received_damage_report: Option<Damage>,
    pub damage_interactions: Option<HashMap<AccountId, DamageInteraction>>,
    pub fires: Option<u64>,
    pub floods: Option<u64>,
    pub citadels: Option<u64>,
    pub crits: Option<u64>,
    pub distance_traveled: Option<f64>,
    pub is_test_ship: bool,
    pub relation: Relation,
    pub manual_stat_hide_toggle: bool,
    // TODO: Maybe in the future refactor this to be a HashMap<Rc<Player>, DeathInfo> ?
    pub kills: Option<i64>,
    pub observed_kills: i64,
    pub translated_build: Option<TranslatedBuild>,
    pub achievements: Vec<Achievement>,
    pub ribbons: HashMap<String, Ribbon>,
    pub consumables: Vec<PlayerConsumable>,
    /// Number of Repair Party (`RepairParty`) activations observed for this
    /// player. `None` when the ship doesn't carry a Repair Party slot. This
    /// count only covers consumable activations seen in the parsed packets,
    /// so it may be incomplete for ships outside the recording perspective(s).
    pub heal_count: Option<u32>,
    pub personal_rating: Option<crate::util::personal_rating::PersonalRatingResult>,
    pub has_vehicle_entity: bool,
}

#[allow(dead_code)]
impl PlayerReport {
    pub fn remove_nda_info(&mut self) {
        self.observed_damage = 0;
        self.observed_damage_text = "NDA".to_string();
        self.actual_damage = Some(0);
        self.actual_damage_text = Some("NDA".into());
        self.actual_damage_hover_text = None;
        self.spotting_damage_hover_text = None;
        self.potential_damage = Some(0);
        self.potential_damage_text = Some("NDA".into());
        self.potential_damage_hover_text = None;
        self.received_damage = Some(0);
        self.received_damage_text = Some("NDA".into());
        self.received_damage_hover_text = None;
        self.fires = Some(0);
        self.floods = Some(0);
        self.citadels = Some(0);
        self.crits = Some(0);
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn vehicle(&self) -> Option<&VehicleEntity> {
        self.player.vehicle_entity()
    }

    pub fn color(&self) -> Color32 {
        self.color
    }

    pub fn name_text(&self) -> &RichText {
        &self.name_text
    }

    pub fn clan_text(&self) -> Option<&RichText> {
        self.clan_text.as_ref()
    }

    pub fn ship_species_text(&self) -> &str {
        &self.ship_species_text
    }

    pub fn icon(&self) -> Option<Arc<GameAsset>> {
        self.icon.clone()
    }

    pub fn division_label(&self) -> Option<&String> {
        self.division_label.as_ref()
    }

    pub fn base_xp(&self) -> Option<i64> {
        self.base_xp
    }

    pub fn base_xp_text(&self) -> Option<&RichText> {
        self.base_xp_text.as_ref()
    }

    pub fn raw_xp(&self) -> Option<i64> {
        self.raw_xp
    }

    pub fn raw_xp_text(&self) -> Option<&String> {
        self.raw_xp_text.as_ref()
    }

    pub fn observed_damage(&self) -> u64 {
        self.observed_damage
    }

    pub fn observed_damage_text(&self) -> &str {
        &self.observed_damage_text
    }

    pub fn actual_damage(&self) -> Option<u64> {
        self.actual_damage
    }

    pub fn actual_damage_report(&self) -> Option<&Damage> {
        self.actual_damage_report.as_ref()
    }

    pub fn actual_damage_text(&self) -> Option<&RichText> {
        self.actual_damage_text.as_ref()
    }

    pub fn actual_damage_hover_text(&self) -> Option<&RichText> {
        self.actual_damage_hover_text.as_ref()
    }

    pub fn ship_name(&self) -> &str {
        &self.ship_name
    }

    pub fn spotting_damage(&self) -> Option<u64> {
        self.spotting_damage
    }

    pub fn spotting_damage_text(&self) -> Option<&String> {
        self.spotting_damage_text.as_ref()
    }

    pub fn spotting_damage_hover_text(&self) -> Option<&RichText> {
        self.spotting_damage_hover_text.as_ref()
    }

    pub fn potential_damage(&self) -> Option<u64> {
        self.potential_damage
    }

    pub fn potential_damage_text(&self) -> Option<&String> {
        self.potential_damage_text.as_ref()
    }

    pub fn potential_damage_hover_text(&self) -> Option<&RichText> {
        self.potential_damage_hover_text.as_ref()
    }

    pub fn potential_damage_report(&self) -> Option<&PotentialDamage> {
        self.potential_damage_report.as_ref()
    }

    pub fn time_lived_secs(&self) -> Option<u64> {
        self.time_lived_secs
    }

    pub fn time_lived_text(&self) -> Option<&String> {
        self.time_lived_text.as_ref()
    }

    pub fn skill_info(&self) -> &SkillInfo {
        &self.skill_info
    }

    pub fn received_damage(&self) -> Option<u64> {
        self.received_damage
    }

    pub fn received_damage_text(&self) -> Option<&RichText> {
        self.received_damage_text.as_ref()
    }

    pub fn received_damage_hover_text(&self) -> Option<&RichText> {
        self.received_damage_hover_text.as_ref()
    }

    pub fn received_damage_report(&self) -> Option<&Damage> {
        self.received_damage_report.as_ref()
    }

    pub fn fires(&self) -> Option<u64> {
        self.fires
    }

    pub fn floods(&self) -> Option<u64> {
        self.floods
    }

    pub fn citadels(&self) -> Option<u64> {
        self.citadels
    }

    pub fn crits(&self) -> Option<u64> {
        self.crits
    }

    pub fn distance_traveled(&self) -> Option<f64> {
        self.distance_traveled
    }

    pub fn is_test_ship(&self) -> bool {
        self.is_test_ship
    }

    pub fn observed_kills(&self) -> i64 {
        self.observed_kills
    }

    pub fn kills(&self) -> Option<i64> {
        self.kills
    }

    pub fn translated_build(&self) -> Option<&TranslatedBuild> {
        self.translated_build.as_ref()
    }

    pub fn should_hide_stats(&self) -> bool {
        self.manual_stat_hide_toggle || (!self.relation.is_self() && self.is_test_ship)
    }

    pub fn hits_report(&self) -> Option<&Hits> {
        self.hits_report.as_ref()
    }

    pub fn damage_interactions(&self) -> Option<&HashMap<AccountId, DamageInteraction>> {
        self.damage_interactions.as_ref()
    }

    pub fn personal_rating(&self) -> Option<&crate::util::personal_rating::PersonalRatingResult> {
        self.personal_rating.as_ref()
    }

    pub fn relation(&self) -> Relation {
        self.relation
    }
}

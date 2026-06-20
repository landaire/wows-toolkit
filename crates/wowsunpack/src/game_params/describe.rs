//! Unified description API: translated name, plain description, the complete raw
//! modifier/stat set, and best-effort translated modifier text, for every
//! describable game entity. See docs spec 2026-06-18-describable-game-params.

use crate::data::ResourceLoader;
use crate::data::Version;
use crate::game_params::modifier_settings_data::format_modifier;
use crate::game_params::modifier_settings_data::modifier_setting;
use crate::game_params::translations::translate_exterior_by_name;
use crate::game_params::types::Ability;
use crate::game_params::types::AbilityCategory;
use crate::game_params::types::CrewSkill;
use crate::game_params::types::CrewSkillModifier;
use crate::game_params::types::Exterior;
use crate::game_params::types::Modernization;
use crate::game_params::types::Param;
use crate::game_params::types::Species;
use crate::game_params::types::Unit;

/// Per-species modifier values (the fixed six ship species the game models).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeciesValues {
    pub aircraft_carrier: f32,
    pub battleship: f32,
    pub cruiser: f32,
    pub destroyer: f32,
    pub submarine: f32,
    pub auxiliary: f32,
}

/// A modifier's value: a single number, or per-species when no ship context fixes one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModifierValue {
    Scalar(f32),
    PerSpecies(SpeciesValues),
}

/// One raw modifier/stat: the GameParams key and its value. Always complete; a
/// `Modifier` is never omitted for a missing format/label entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    pub name: String,
    pub value: ModifierValue,
}

/// Why a `ModifierDescription`'s text reads the way it does. A returned
/// `ModifierDescription` is only ever `Formatted` or `Unresolved`; no-op and
/// client-hidden modifiers yield no `ModifierDescription` (they remain in the
/// raw modifier list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierResolution {
    Formatted,
    Unresolved,
}

/// A rendered modifier line.
#[derive(Clone, Debug, PartialEq)]
pub struct ModifierDescription {
    pub modifier: String,
    pub text: String,
    pub resolution: ModifierResolution,
}

/// The unified description of an entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamDescription {
    pub name: Option<String>,
    pub description: Option<String>,
    pub modifiers: Vec<Modifier>,
    pub modifier_descriptions: Vec<ModifierDescription>,
}

/// Inputs for rendering a description.
pub struct DescribeContext<'a> {
    pub resource_loader: &'a dyn ResourceLoader,
    pub version: &'a Version,
    /// Ship context. `None` leaves per-species modifiers unresolved.
    pub species: Option<Species>,
    /// Owning `Param` name, for IDS key building. `None` for entities (e.g.
    /// `CrewSkill`) that key off their own identity.
    pub param_name: Option<&'a str>,
}

pub(crate) trait Describable {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String>;
    fn plain_description(&self, ctx: &DescribeContext) -> Option<String>;
    fn modifiers(&self, ctx: &DescribeContext) -> Vec<Modifier>;

    fn modifier_descriptions(&self, ctx: &DescribeContext) -> Vec<ModifierDescription> {
        render_modifier_descriptions(&self.modifiers(ctx), ctx)
    }

    fn describe(&self, ctx: &DescribeContext) -> ParamDescription {
        let modifiers = self.modifiers(ctx);
        let modifier_descriptions = render_modifier_descriptions(&modifiers, ctx);
        ParamDescription {
            name: self.display_name(ctx),
            description: self.plain_description(ctx),
            modifiers,
            modifier_descriptions,
        }
    }
}

/// Render each modifier. `Formatted` when the settings table produces a line;
/// `Unresolved` (raw `name = value`) when no table entry exists for the build.
/// No-op (value == base) and client-hidden modifiers produce no line.
fn render_modifier_descriptions(mods: &[Modifier], ctx: &DescribeContext) -> Vec<ModifierDescription> {
    let version = *ctx.version;
    let mut out = Vec::new();
    for m in mods {
        // A per-species modifier with no ship context cannot be resolved to one
        // display number; fabricating a species would be a confidently-wrong line.
        let value = match m.value {
            ModifierValue::Scalar(v) => Some(v),
            ModifierValue::PerSpecies(s) => ctx.species.map(|sp| species_slot(&s, sp)),
        };
        let Some(value) = value else {
            out.push(ModifierDescription {
                modifier: m.name.clone(),
                text: format!("{} (per-species; no ship context)", m.name),
                resolution: ModifierResolution::Unresolved,
            });
            continue;
        };
        match modifier_setting(version, &m.name) {
            Some(_) => {
                // species_or_default only affects the rarely-used species-suffixed
                // label fallback; the value here is already species-independent.
                if let Some(text) =
                    format_modifier(version, &m.name, value, species_or_default(ctx.species), ctx.resource_loader)
                {
                    out.push(ModifierDescription {
                        modifier: m.name.clone(),
                        text,
                        resolution: ModifierResolution::Formatted,
                    });
                }
                // None here means no-op or client-hidden: intentionally no line.
            }
            None => {
                out.push(ModifierDescription {
                    modifier: m.name.clone(),
                    text: format!("{} = {value}", m.name),
                    resolution: ModifierResolution::Unresolved,
                });
            }
        }
    }
    out
}

fn species_slot(s: &SpeciesValues, species: Species) -> f32 {
    debug_assert!(
        matches!(
            species,
            Species::AirCarrier
                | Species::Battleship
                | Species::Cruiser
                | Species::Destroyer
                | Species::Submarine
                | Species::Auxiliary
        ),
        "species_slot got non-ship species {species:?}"
    );
    match species {
        Species::AirCarrier => s.aircraft_carrier,
        Species::Battleship => s.battleship,
        Species::Cruiser => s.cruiser,
        Species::Destroyer => s.destroyer,
        Species::Submarine => s.submarine,
        Species::Auxiliary => s.auxiliary,
        // format_modifier and the per-species accessors take a non-optional
        // Species; the raw per-species values stay intact on Modifier, so this
        // fallback only affects the rendered display line, not the data.
        _ => s.battleship,
    }
}

fn species_or_default(species: Option<Species>) -> Species {
    // format_modifier requires a Species; with no ship context the display line
    // uses the battleship slot. The raw per-species data remains on Modifier.
    species.unwrap_or(Species::Battleship)
}

/// Map the game's per-species modifier records into unified `Modifier`s.
// `excluded_consumables` is deliberately dropped: the render layer does not use
// it, so it stays off `Modifier` by decision, not oversight.
pub(crate) fn modifiers_from_crew_skill(mods: &[CrewSkillModifier]) -> Vec<Modifier> {
    mods.iter()
        .map(|m| Modifier {
            name: m.name().to_string(),
            value: ModifierValue::PerSpecies(SpeciesValues {
                aircraft_carrier: m.get_for_species(&Species::AirCarrier),
                battleship: m.get_for_species(&Species::Battleship),
                cruiser: m.get_for_species(&Species::Cruiser),
                destroyer: m.get_for_species(&Species::Destroyer),
                submarine: m.get_for_species(&Species::Submarine),
                auxiliary: m.get_for_species(&Species::Auxiliary),
            }),
        })
        .collect()
}

impl Describable for Modernization {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        crate::game_params::translations::translate_module(name, ctx.resource_loader).0
    }
    fn plain_description(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        crate::game_params::translations::translate_module(name, ctx.resource_loader).1
    }
    fn modifiers(&self, _ctx: &DescribeContext) -> Vec<Modifier> {
        modifiers_from_crew_skill(self.modifiers())
    }
}

impl Describable for Exterior {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        translate_exterior_by_name(name, self.title(), ctx.resource_loader).0
    }
    fn plain_description(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        translate_exterior_by_name(name, self.title(), ctx.resource_loader).1
    }
    fn modifiers(&self, _ctx: &DescribeContext) -> Vec<Modifier> {
        modifiers_from_crew_skill(self.modifiers())
    }
}

impl Describable for CrewSkill {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        let (primary, fallback) = self.skill_translation_keys_pub("IDS_SKILL", ctx.version);
        ctx.resource_loader
            .localized_name_from_id(&primary)
            .or_else(|| ctx.resource_loader.localized_name_from_id(&fallback))
    }
    fn plain_description(&self, ctx: &DescribeContext) -> Option<String> {
        self.description_with_pub(species_or_default(ctx.species), ctx.resource_loader, ctx.version)
    }
    fn modifiers(&self, _ctx: &DescribeContext) -> Vec<Modifier> {
        let mut out = self.modifiers().map(|m| modifiers_from_crew_skill(m)).unwrap_or_default();
        if let Some(trig) = self.logic_trigger()
            && let Some(tmods) = trig.modifiers()
        {
            out.extend(modifiers_from_crew_skill(tmods));
        }
        out
    }
}

impl Describable for Unit {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        crate::game_params::translations::translate_unit(name, ctx.resource_loader)
    }
    fn plain_description(&self, _ctx: &DescribeContext) -> Option<String> {
        None // Phase 3: resolve a module description key if the catalog has one.
    }
    fn modifiers(&self, _ctx: &DescribeContext) -> Vec<Modifier> {
        Vec::new() // Phase 3: raw module stats.
    }
}

impl Describable for Ability {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        crate::game_params::translations::translate_consumable(name, ctx.resource_loader)
    }
    fn plain_description(&self, ctx: &DescribeContext) -> Option<String> {
        let name = ctx.param_name?;
        crate::game_params::translations::translate_consumable_description(name, ctx.resource_loader)
    }
    fn modifiers(&self, _ctx: &DescribeContext) -> Vec<Modifier> {
        Vec::new() // Effects are per-flavor; see AbilityCategory::effect_modifiers.
    }
}

impl AbilityCategory {
    /// Raw effect fields as unified scalar modifiers. The render step filters to
    /// the ones MODIFIER_SETTINGS recognizes (matching the client's generic
    /// consumable attribute display); this returns the complete set.
    pub fn effect_modifiers(&self) -> Vec<Modifier> {
        self.effect_fields()
            .iter()
            .map(|(name, value)| Modifier { name: name.clone(), value: ModifierValue::Scalar(*value) })
            .collect()
    }

    /// Rendered, translated effect lines for this resolved consumable flavor.
    /// Consumable param dicts carry non-attribute fields (e.g. lifeCycleType) that
    /// the client's getAttributesDict drops. Only fields known to MODIFIER_SETTINGS
    /// are displayable attributes; the complete raw set stays on effect_modifiers().
    pub fn describe_effects(&self, ctx: &DescribeContext) -> Vec<ModifierDescription> {
        let known: Vec<Modifier> = self
            .effect_modifiers()
            .into_iter()
            .filter(|m| modifier_setting(*ctx.version, &m.name).is_some())
            .collect();
        render_modifier_descriptions(&known, ctx)
    }
}

impl Describable for Param {
    fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        self.describe(ctx).name
    }
    fn plain_description(&self, ctx: &DescribeContext) -> Option<String> {
        self.describe(ctx).description
    }
    fn modifiers(&self, ctx: &DescribeContext) -> Vec<Modifier> {
        self.describe(ctx).modifiers
    }
    fn describe(&self, ctx: &DescribeContext) -> ParamDescription {
        // Thread this param's own name through, regardless of caller-supplied ctx.
        let child = DescribeContext {
            resource_loader: ctx.resource_loader,
            version: ctx.version,
            species: ctx.species,
            param_name: Some(self.name()),
        };
        if let Some(m) = self.modernization() {
            return m.describe(&child);
        }
        if let Some(e) = self.exterior() {
            return e.describe(&child);
        }
        if let Some(u) = self.unit() {
            return u.describe(&child);
        }
        if let Some(a) = self.ability() {
            return a.describe(&child);
        }
        ParamDescription::default()
    }
}

/// Public entry point to the description API. External callers reach `Describable`
/// only through `Param`, so a correct `param_name` is always threaded.
impl Param {
    pub fn describe(&self, ctx: &DescribeContext) -> ParamDescription {
        <Self as Describable>::describe(self, ctx)
    }
    pub fn display_name(&self, ctx: &DescribeContext) -> Option<String> {
        <Self as Describable>::display_name(self, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::Param;
    use crate::game_params::types::ParamData;
    use crate::game_params::types::Species;

    struct EchoLoader;
    impl ResourceLoader for EchoLoader {
        fn localized_name_from_param(&self, _p: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &str) -> Option<String> {
            Some(id.to_string())
        }
        fn game_param_by_id(&self, _id: crate::game_types::GameParamId) -> Option<crate::Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    fn ctx<'a>(v: &'a Version, loader: &'a EchoLoader) -> DescribeContext<'a> {
        DescribeContext { resource_loader: loader, version: v, species: Some(Species::Battleship), param_name: None }
    }

    fn version(build: u32) -> Version {
        Version { major: 99, minor: 0, patch: 0, build }
    }

    #[test]
    fn unresolved_modifier_is_surfaced_not_dropped() {
        let loader = EchoLoader;
        let v = version(11791718);
        let c = ctx(&v, &loader);
        let mods = vec![Modifier { name: "definitely_not_a_modifier".into(), value: ModifierValue::Scalar(2.0) }];
        let out = render_modifier_descriptions(&mods, &c);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].resolution, ModifierResolution::Unresolved);
        assert!(out[0].text.contains("definitely_not_a_modifier"), "got {}", out[0].text);
    }

    #[test]
    fn known_modifier_is_formatted() {
        let loader = EchoLoader;
        let v = version(11791718);
        let c = ctx(&v, &loader);
        // GMRotationSpeed: percent, base 1.0. 0.9 -> a formatted "-10%" line.
        let mods = vec![Modifier { name: "GMRotationSpeed".into(), value: ModifierValue::Scalar(0.9) }];
        let out = render_modifier_descriptions(&mods, &c);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].resolution, ModifierResolution::Formatted);
    }

    #[test]
    fn per_species_with_no_species_is_unresolved() {
        let loader = EchoLoader;
        let v = version(11791718);
        let c = DescribeContext { resource_loader: &loader, version: &v, species: None, param_name: None };
        let mods = vec![Modifier {
            name: "definitely_not_a_modifier".into(),
            value: ModifierValue::PerSpecies(SpeciesValues {
                aircraft_carrier: 7.0,
                battleship: 1.23,
                cruiser: 8.0,
                destroyer: 9.0,
                submarine: 10.0,
                auxiliary: 11.0,
            }),
        }];
        let out = render_modifier_descriptions(&mods, &c);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].resolution, ModifierResolution::Unresolved);
        // A per-species modifier with no ship context must not fabricate a slot.
        assert!(!out[0].text.contains("1.23"), "got {}", out[0].text);
        assert!(out[0].text.contains("definitely_not_a_modifier"), "got {}", out[0].text);
    }

    #[test]
    fn crew_skill_modifiers_map_to_per_species() {
        use crate::game_params::types::CrewSkillModifier;
        let m = CrewSkillModifier::builder()
            .name("speedCoef".to_string())
            .aircraft_carrier(1.0)
            .auxiliary(1.0)
            .battleship(1.05)
            .cruiser(1.0)
            .destroyer(1.0)
            .submarine(1.0)
            .excluded_consumables(vec![])
            .build();
        let mods = modifiers_from_crew_skill(&[m]);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "speedCoef");
        match mods[0].value {
            ModifierValue::PerSpecies(s) => assert_eq!(s.battleship, 1.05),
            _ => panic!("expected per-species"),
        }
    }

    #[test]
    fn exterior_name_falls_back_to_direct_key() {
        use crate::game_params::types::CrewSkillModifier;
        let loader = EchoLoader;
        let v = version(11791718);
        let ext = Exterior::builder()
            .modifiers(vec![
                CrewSkillModifier::builder()
                    .name("speedCoef".to_string())
                    .aircraft_carrier(1.0)
                    .auxiliary(1.0)
                    .battleship(1.05)
                    .cruiser(1.0)
                    .destroyer(1.0)
                    .submarine(1.0)
                    .excluded_consumables(vec![])
                    .build(),
            ])
            .build();
        let c = DescribeContext {
            resource_loader: &loader,
            version: &v,
            species: Some(Species::Battleship),
            param_name: Some("PCEF005_SM_SignalFlag"),
        };
        // EchoLoader echoes every id, so translate_module's IDS_TITLE_<NAME> wins.
        assert_eq!(ext.display_name(&c).as_deref(), Some("IDS_TITLE_PCEF005_SM_SIGNALFLAG"));
        assert_eq!(Describable::modifiers(&ext, &c).len(), 1);
    }

    #[test]
    fn crew_skill_describes_name_and_modifiers() {
        use crate::game_params::types::CrewSkill;
        use crate::game_params::types::CrewSkillModifier;
        use crate::game_params::types::CrewSkillTiers;
        use crate::game_params::types::CrewSkillType;
        use crate::game_params::types::SkillPointCost;
        let loader = EchoLoader;
        let v = version(11791718);
        let cost = SkillPointCost::new(1);
        let tier = CrewSkillTiers::builder()
            .aircraft_carrier(cost)
            .auxiliary(cost)
            .battleship(cost)
            .cruiser(cost)
            .destroyer(cost)
            .submarine(cost)
            .build();
        let skill = CrewSkill::builder()
            .internal_name("GreaseTheGears".into())
            .can_be_learned(true)
            .is_epic(false)
            .modifiers(vec![
                CrewSkillModifier::builder()
                    .name("speedCoef".to_string())
                    .aircraft_carrier(1.0)
                    .auxiliary(1.0)
                    .battleship(1.05)
                    .cruiser(1.0)
                    .destroyer(1.0)
                    .submarine(1.0)
                    .excluded_consumables(vec![])
                    .build(),
            ])
            .skill_type(CrewSkillType::new(0))
            .tier(tier)
            .ui_treat_as_trigger(false)
            .build();
        let c = DescribeContext {
            resource_loader: &loader,
            version: &v,
            species: Some(Species::Battleship),
            param_name: None,
        };
        // EchoLoader echoes ids; rework-era build keys IDS_SKILL_<UPPER_SNAKE>.
        assert_eq!(skill.display_name(&c).as_deref(), Some("IDS_SKILL_GREASE_THE_GEARS"));
        let mods = Describable::modifiers(&skill, &c);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "speedCoef");
        match mods[0].value {
            ModifierValue::PerSpecies(s) => assert_eq!(s.battleship, 1.05),
            _ => panic!("expected per-species"),
        }
    }

    #[test]
    fn unit_describes_name_only() {
        let loader = EchoLoader;
        let v = version(11791718);
        let unit = Unit::new(Some("PXIH001".into()));
        let c = DescribeContext {
            resource_loader: &loader,
            version: &v,
            species: Some(Species::Battleship),
            param_name: Some("PXIH001"),
        };
        assert_eq!(unit.display_name(&c).as_deref(), Some("IDS_PXIH001"));
        assert_eq!(unit.plain_description(&c), None);
        assert!(Describable::modifiers(&unit, &c).is_empty());
    }

    #[test]
    fn ability_describes_name_and_description() {
        use crate::game_params::types::Ability;
        let loader = EchoLoader;
        let v = version(11791718);
        let ability = Ability::builder()
            .can_buy(false)
            .cost_credits(0)
            .cost_gold(0)
            .is_free(true)
            .categories(Default::default())
            .build();
        let c = DescribeContext {
            resource_loader: &loader,
            version: &v,
            species: Some(Species::Battleship),
            param_name: Some("PCY001_CrashCrew"),
        };
        assert_eq!(ability.display_name(&c).as_deref(), Some("IDS_DOCK_CONSUME_TITLE_PCY001_CRASHCREW"));
        assert_eq!(ability.plain_description(&c).as_deref(), Some("IDS_DOCK_CONSUME_DESCRIPTION_PCY001_CRASHCREW"));
        assert!(Describable::modifiers(&ability, &c).is_empty());
    }

    #[test]
    fn param_delegate_threads_own_name_when_ctx_name_absent() {
        let loader = EchoLoader;
        let v = version(11791718);
        // Incoming ctx has param_name: None; the delegate must fill it from the
        // param's own name so the Unit impl can build the IDS key.
        let c = DescribeContext {
            resource_loader: &loader,
            version: &v,
            species: Some(Species::Battleship),
            param_name: None,
        };
        let param = Param::builder()
            .id(crate::game_types::GameParamId::from(1u32))
            .index("PXIH001".to_string())
            .name("PXIH001".to_string())
            .nation(String::new())
            .data(ParamData::Unit(Unit::new(Some("PXIH001".into()))))
            .build();
        assert_eq!(param.describe(&c).name.as_deref(), Some("IDS_PXIH001"));
    }

    #[test]
    fn ability_category_describes_per_flavor_effects() {
        use std::collections::BTreeMap;
        let loader = EchoLoader;
        let v = version(11791718);
        let c = ctx(&v, &loader);
        // reloadTime is in MODIFIER_SETTINGS (base 1.0); 5.0 differs -> Formatted.
        // totally_not_a_field has no settings entry -> dropped from the display
        // (matches the client's getAttributesDict), but kept in effect_modifiers().
        let mut fields = BTreeMap::new();
        fields.insert("reloadTime".to_string(), 5.0_f32);
        fields.insert("totally_not_a_field".to_string(), 1.0_f32);
        let cat = AbilityCategory::builder()
            .consumable_type("foo".to_string())
            .group("default".to_string())
            .icon_id("icon".to_string())
            .num_consumables(2)
            .preparation_time(0.0)
            .reload_time(5.0)
            .work_time(10.0)
            .effect_fields(fields)
            .build();

        let mods = cat.effect_modifiers();
        assert_eq!(mods.len(), 2);
        for m in &mods {
            assert!(matches!(m.value, ModifierValue::Scalar(_)), "expected scalar for {}", m.name);
        }

        let out = cat.describe_effects(&c);
        let reload = out.iter().find(|d| d.modifier == "reloadTime").expect("reloadTime line");
        assert_eq!(reload.resolution, ModifierResolution::Formatted);
        assert!(
            out.iter().all(|d| d.modifier != "totally_not_a_field"),
            "unknown field must be dropped from the display"
        );
    }

    #[test]
    fn noop_known_modifier_yields_no_line() {
        let loader = EchoLoader;
        let v = version(11791718);
        let c = ctx(&v, &loader);
        let mods = vec![Modifier { name: "GMRotationSpeed".into(), value: ModifierValue::Scalar(1.0) }];
        assert!(render_modifier_descriptions(&mods, &c).is_empty());
    }
}

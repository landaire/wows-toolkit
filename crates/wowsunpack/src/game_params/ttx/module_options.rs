//! Per-module stat enumeration: for each selectable upgrade slot, every module option
//! and the `ShipStats` mounting it yields (the rest of a baseline loadout held fixed).
//! Pure over the param graph; the caller renders/diffs the resulting cards.

use crate::game_params::ttx::components::ShipTtxComponents;
use crate::game_params::ttx::model::ShipStats;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::orchestration::ship_stats;
use crate::game_params::ttx::selection::ShipUpgradeSelection;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::Param;

/// A TTX-relevant upgrade slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleSlot {
    Hull,
    Engine,
    Artillery,
    Torpedoes,
    FireControl,
}

/// One selectable module in a slot and the full ship card mounting it produces.
#[derive(Clone, Debug)]
pub struct ModuleOption {
    /// `ShipUpgradeInfo` key; resolve to a display name via `translate_unit`.
    pub upgrade_name: String,
    /// `true` when this option equals the baseline selection for its slot.
    pub is_baseline: bool,
    pub stats: ShipStats,
}

/// Every selectable module in one slot, plus the baseline upgrade name there.
#[derive(Clone, Debug)]
pub struct SlotOptions {
    pub slot: ModuleSlot,
    pub baseline: Option<String>,
    pub options: Vec<ModuleOption>,
}

/// Per-slot selectable modules and the card each yields.
#[derive(Clone, Debug, Default)]
pub struct ModuleOptions {
    pub slots: Vec<SlotOptions>,
}

const SLOTS: [ModuleSlot; 5] =
    [ModuleSlot::Hull, ModuleSlot::Engine, ModuleSlot::Artillery, ModuleSlot::Torpedoes, ModuleSlot::FireControl];

/// Selectable upgrade names for `slot`, sorted (HashMap iteration order is nondeterministic).
fn slot_upgrade_names(components: &ShipTtxComponents, slot: ModuleSlot) -> Vec<String> {
    let mut names: Vec<String> = match slot {
        ModuleSlot::Hull => components.hulls.keys().cloned().collect(),
        ModuleSlot::Engine => components.engines.keys().cloned().collect(),
        ModuleSlot::Artillery => components.artillery.keys().cloned().collect(),
        ModuleSlot::Torpedoes => components.torpedoes.keys().cloned().collect(),
        ModuleSlot::FireControl => components.fire_controls.keys().cloned().collect(),
    };
    names.sort();
    names
}

/// The baseline upgrade name for `slot`.
fn baseline_name(baseline: &ShipUpgradeSelection, slot: ModuleSlot) -> Option<String> {
    match slot {
        ModuleSlot::Hull => baseline.hull.clone(),
        ModuleSlot::Engine => baseline.engine.clone(),
        ModuleSlot::Artillery => baseline.artillery.clone(),
        ModuleSlot::Torpedoes => baseline.torpedoes.clone(),
        ModuleSlot::FireControl => baseline.fire_control.clone(),
    }
}

/// `baseline` with `slot` set to `name`.
fn with_slot(baseline: &ShipUpgradeSelection, slot: ModuleSlot, name: &str) -> ShipUpgradeSelection {
    let mut selection = baseline.clone();
    let name = Some(name.to_string());
    match slot {
        ModuleSlot::Hull => selection.hull = name,
        ModuleSlot::Engine => selection.engine = name,
        ModuleSlot::Artillery => selection.artillery = name,
        ModuleSlot::Torpedoes => selection.torpedoes = name,
        ModuleSlot::FireControl => selection.fire_control = name,
    }
    selection
}

/// Enumerate, per slot, every selectable module and the `ShipStats` mounting it yields
/// with the rest of `baseline` fixed. Slots the ship lacks are omitted; a non-vehicle or
/// component-less ship yields an empty `ModuleOptions`.
pub fn module_options(
    ship: &Param,
    baseline: &ShipUpgradeSelection,
    modifiers: &ModifierBundle,
    level: u32,
    provider: &dyn GameParamProvider,
) -> ModuleOptions {
    let Some(components) = ship.vehicle().and_then(|v| v.ttx_components()) else {
        return ModuleOptions::default();
    };

    let mut slots = Vec::new();
    for slot in SLOTS {
        let names = slot_upgrade_names(components, slot);
        if names.is_empty() {
            continue;
        }
        let baseline_for_slot = baseline_name(baseline, slot);
        let options = names
            .into_iter()
            .map(|name| {
                let selection = with_slot(baseline, slot, &name);
                let stats = ship_stats(ship, &selection, modifiers, level, provider);
                let is_baseline = baseline_for_slot.as_deref() == Some(name.as_str());
                ModuleOption { upgrade_name: name, is_baseline, stats }
            })
            .collect();
        slots.push(SlotOptions { slot, baseline: baseline_for_slot, options });
    }
    ModuleOptions { slots }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rc;
    use crate::data::ResourceLoader;
    use crate::game_params::ttx::components::EngineComponentStats;
    use crate::game_params::ttx::components::HullComponentStats;
    use crate::game_params::ttx::components::ShipTtxComponents;
    use crate::game_params::ttx::labels::TtxStat;
    use crate::game_params::ttx::model::Hp;
    use crate::game_params::types::ParamData;
    use crate::game_params::types::Species;
    use crate::game_params::types::Vehicle;
    use crate::game_types::GameParamId;
    use crate::recognized::Recognized;

    struct EmptyProvider;
    impl GameParamProvider for EmptyProvider {
        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_index(&self, _index: &str) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_name(&self, _name: &str) -> Option<Rc<Param>> {
            None
        }
        fn params(&self) -> &[Rc<Param>] {
            &[]
        }
    }

    struct EchoLoader;
    impl ResourceLoader for EchoLoader {
        fn localized_name_from_param(&self, _p: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &crate::data::TranslationKey) -> Option<String> {
            Some(id.as_str().to_string())
        }
        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    fn hull(health: f32) -> HullComponentStats {
        HullComponentStats { health: Some(Hp::from(health)), ..Default::default() }
    }

    fn engine(speed_coef: f32) -> EngineComponentStats {
        EngineComponentStats { speed_coef: Some(speed_coef) }
    }

    fn ship_from(components: ShipTtxComponents) -> Param {
        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .maybe_abilities(None)
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .ttx_components(components)
            .build();
        Param::builder()
            .id(GameParamId::from(900u32))
            .index("IDX".to_string())
            .name("Fixture".to_string())
            .nation("USA".to_string())
            .species(Recognized::Known(Species::Cruiser))
            .data(ParamData::Vehicle(vehicle))
            .build()
    }

    fn two_hull_ship() -> Param {
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("HULL_A".to_string(), hull(19400.0));
        components.hulls.insert("HULL_B".to_string(), hull(21000.0));
        components.stock_selection = ShipUpgradeSelection::new(Some("HULL_A".to_string()), None, None, None, None);
        ship_from(components)
    }

    fn hull_and_engine_ship() -> Param {
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("HULL_A".to_string(), hull(19400.0));
        components.hulls.insert("HULL_B".to_string(), hull(21000.0));
        components.engines.insert("ENG_A".to_string(), engine(0.0));
        components.stock_selection =
            ShipUpgradeSelection::new(Some("HULL_A".to_string()), Some("ENG_A".to_string()), None, None, None);
        ship_from(components)
    }

    #[test]
    fn enumerates_hull_options_sorted_with_baseline_flag() {
        let ship = two_hull_ship();
        let baseline = ShipUpgradeSelection::stock(&ship);
        let provider = EmptyProvider;
        let opts = module_options(&ship, &baseline, &ModifierBundle::empty(Species::Cruiser), 10, &provider);

        let hull_slot = opts.slots.iter().find(|s| s.slot == ModuleSlot::Hull).expect("hull slot");
        assert_eq!(hull_slot.baseline.as_deref(), Some("HULL_A"));
        let names: Vec<&str> = hull_slot.options.iter().map(|o| o.upgrade_name.as_str()).collect();
        assert_eq!(names, vec!["HULL_A", "HULL_B"]);

        let a = &hull_slot.options[0];
        let b = &hull_slot.options[1];
        assert!(a.is_baseline);
        assert!(!b.is_baseline);
        assert_eq!(a.stats.durability.as_ref().unwrap().health.unwrap().value(), 19400.0);
        assert_eq!(b.stats.durability.as_ref().unwrap().health.unwrap().value(), 21000.0);

        assert_eq!(opts.slots.len(), 1);
    }

    #[test]
    fn non_vehicle_yields_empty() {
        let proj = Param::builder()
            .id(GameParamId::from(1u32))
            .index("S0001".to_string())
            .name("X".to_string())
            .nation("USA".to_string())
            .data(ParamData::Unit(crate::game_params::types::Unit::new(None)))
            .build();
        let provider = EmptyProvider;
        let opts = module_options(
            &proj,
            &ShipUpgradeSelection::default(),
            &ModifierBundle::empty(Species::Cruiser),
            10,
            &provider,
        );
        assert!(opts.slots.is_empty());
    }

    #[test]
    fn enumerates_multiple_slots_in_order() {
        let ship = hull_and_engine_ship();
        let baseline = ShipUpgradeSelection::stock(&ship);
        let provider = EmptyProvider;
        let opts = module_options(&ship, &baseline, &ModifierBundle::empty(Species::Cruiser), 10, &provider);
        let slots: Vec<ModuleSlot> = opts.slots.iter().map(|s| s.slot).collect();
        assert_eq!(slots, vec![ModuleSlot::Hull, ModuleSlot::Engine]);
        assert_eq!(opts.slots[0].options.len(), 2);
        let eng = &opts.slots[1];
        assert_eq!(eng.options.len(), 1);
        assert_eq!(eng.baseline.as_deref(), Some("ENG_A"));
        assert!(eng.options[0].is_baseline);
    }

    #[test]
    fn module_diff_renders_hull_change() {
        use crate::game_params::ttx::render::diff_stat_rows;
        let ship = two_hull_ship();
        let baseline = ShipUpgradeSelection::stock(&ship);
        let provider = EmptyProvider;
        let opts = module_options(&ship, &baseline, &ModifierBundle::empty(Species::Cruiser), 10, &provider);
        let hull = opts.slots.iter().find(|s| s.slot == ModuleSlot::Hull).expect("hull slot");
        let base_opt = hull.options.iter().find(|o| o.is_baseline).expect("baseline (HULL_A)");
        let cand_opt = hull.options.iter().find(|o| !o.is_baseline).expect("non-baseline (HULL_B)");

        let loader = EchoLoader;
        let deltas = diff_stat_rows(&base_opt.stats.rows(), &cand_opt.stats.rows(), &loader);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].stat, TtxStat::Health);
        assert!(deltas[0].from.is_some() && deltas[0].to.is_some() && deltas[0].from != deltas[0].to);
    }
}

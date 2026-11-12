//! Ship modules (hull, guns, torpedoes, engine, fire control) by English name.

use serde::Serialize;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::translations::translate_unit;
use wowsunpack::game_params::ttx::ModuleSlot;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;

use crate::data::GameData;
use crate::error::QueryError;
use crate::names::search_key;
use crate::ships::Ship;
use crate::text::clean;

/// The kind of module a ship mounts in one research slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleKind {
    Hull,
    Guns,
    Torpedoes,
    Engine,
    FireControl,
}

impl ModuleKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hull => "hull",
            Self::Guns => "guns",
            Self::Torpedoes => "torpedoes",
            Self::Engine => "engine",
            Self::FireControl => "fire control",
        }
    }

    fn from_slot(slot: ModuleSlot) -> Self {
        match slot {
            ModuleSlot::Hull => Self::Hull,
            ModuleSlot::Engine => Self::Engine,
            ModuleSlot::Artillery => Self::Guns,
            ModuleSlot::Torpedoes => Self::Torpedoes,
            ModuleSlot::FireControl => Self::FireControl,
        }
    }

    const SLOTS: [ModuleSlot; 5] =
        [ModuleSlot::Hull, ModuleSlot::Artillery, ModuleSlot::Torpedoes, ModuleSlot::Engine, ModuleSlot::FireControl];

    pub(crate) const ALL: [ModuleKind; 5] = [Self::Hull, Self::Guns, Self::Torpedoes, Self::Engine, Self::FireControl];

    /// Mounts the module `param` of this kind in `selection`.
    pub(crate) fn set(self, selection: &mut ShipUpgradeSelection, param: String) {
        let name = Some(param);
        match self {
            Self::Hull => selection.hull = name,
            Self::Guns => selection.artillery = name,
            Self::Torpedoes => selection.torpedoes = name,
            Self::Engine => selection.engine = name,
            Self::FireControl => selection.fire_control = name,
        }
    }
}

/// One module a ship can mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Module {
    pub kind: ModuleKind,

    /// English name, unique among the ship's modules of the same kind. Hulls
    /// read "Hull A", "Hull B".
    pub name: String,

    /// `ShipUpgradeInfo` key, e.g. "PAUH502_Cleveland_1945".
    pub param: String,

    pub stock: bool,
}

/// A module selection resolved against one ship.
#[derive(Debug, Clone)]
pub(crate) struct ModuleSelection {
    pub(crate) selection: ShipUpgradeSelection,

    /// The modules named in the request, in request order.
    pub(crate) chosen: Vec<Module>,
}

impl GameData {
    /// Every module the ship can mount, by kind then GameParams name.
    pub(crate) fn ship_modules(&self, ship: &Ship) -> Vec<Module> {
        let Some(components) = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
            return Vec::new();
        };
        let stock = components.stock_selection();

        let mut modules = Vec::new();
        for slot in ModuleKind::SLOTS {
            let mut params: Vec<&String> = match slot {
                ModuleSlot::Hull => components.hulls.keys().collect(),
                ModuleSlot::Engine => components.engines.keys().collect(),
                ModuleSlot::Artillery => components.artillery.keys().collect(),
                ModuleSlot::Torpedoes => components.torpedoes.keys().collect(),
                ModuleSlot::FireControl => components.fire_controls.keys().collect(),
            };
            params.sort();

            let stock_param = slot_name(stock, slot);
            let kind = ModuleKind::from_slot(slot);
            let mut slot_modules: Vec<Module> = params
                .into_iter()
                .map(|param| Module {
                    kind,
                    name: self.module_display_name(kind, param),
                    param: param.clone(),
                    stock: stock_param == Some(param.as_str()),
                })
                .collect();
            disambiguate(&mut slot_modules);
            modules.extend(slot_modules);
        }

        modules
    }

    /// Resolves module names against the ship: each replaces the stock module
    /// of its kind.
    pub(crate) fn resolve_modules(&self, ship: &Ship, queries: &[String]) -> Result<ModuleSelection, QueryError> {
        let mut selection = ShipUpgradeSelection::stock(&ship.param);
        let modules = self.ship_modules(ship);

        let mut chosen: Vec<Module> = Vec::new();
        for query in queries {
            let module = resolve_module(&modules, query, ship)?;
            if chosen.iter().any(|other| other.param == module.param) {
                continue;
            }
            if let Some(other) = chosen.iter().find(|other| other.kind == module.kind) {
                return Err(QueryError::ModuleSlotConflict {
                    first: other.name.clone(),
                    second: module.name.clone(),
                    kind: module.kind.label(),
                });
            }

            module.kind.set(&mut selection, module.param.clone());
            chosen.push(module.clone());
        }

        Ok(ModuleSelection { selection, chosen })
    }

    fn module_display_name(&self, kind: ModuleKind, param: &str) -> String {
        let translated = translate_unit(param, self.provider()).map(|name| clean(&name));
        if kind == ModuleKind::Hull
            && let Some(letter) = translated.as_deref().and_then(hull_letter)
        {
            return format!("Hull {letter}");
        }

        translated.unwrap_or_else(|| param.to_owned())
    }

    /// Whether the selected hull mounts the selected guns and torpedoes. Data
    /// without component links places no limit.
    pub(crate) fn hull_fits(&self, ship: &Ship, selection: &ShipUpgradeSelection) -> bool {
        let Some(vehicle) = ship.param.vehicle() else {
            return true;
        };
        let Some(components) = vehicle.ttx_components() else {
            return true;
        };
        let Some(hull) = selection.hull.as_deref().and_then(|hull| vehicle.hull_upgrade(hull)) else {
            return true;
        };

        [
            (ComponentType::Artillery, selection.artillery.as_deref()),
            (ComponentType::Torpedoes, selection.torpedoes.as_deref()),
        ]
        .into_iter()
        .all(|(component_type, param)| {
            param.is_none_or(|param| {
                components.upgrade_components(param).is_empty()
                    || components.component_on_hull(param, hull, component_type).is_some()
            })
        })
    }
}

fn slot_name(selection: &ShipUpgradeSelection, slot: ModuleSlot) -> Option<&str> {
    match slot {
        ModuleSlot::Hull => selection.hull.as_deref(),
        ModuleSlot::Engine => selection.engine.as_deref(),
        ModuleSlot::Artillery => selection.artillery.as_deref(),
        ModuleSlot::Torpedoes => selection.torpedoes.as_deref(),
        ModuleSlot::FireControl => selection.fire_control.as_deref(),
    }
}

/// The letter of a hull name such as "Cleveland (B)".
fn hull_letter(name: &str) -> Option<char> {
    let inner = name.strip_suffix(')')?.rsplit_once('(')?.1.trim();
    let mut characters = inner.chars();
    let letter = characters.next().filter(char::is_ascii_alphabetic)?;

    characters.next().is_none().then(|| letter.to_ascii_uppercase())
}

/// Modules of one kind that share a translated name, such as stock and
/// upgraded guns of the same model, are told apart by "stock" and
/// "upgraded", then by GameParams name.
fn disambiguate(modules: &mut [Module]) {
    let names: Vec<String> = modules.iter().map(|module| module.name.clone()).collect();
    for module in modules.iter_mut() {
        let shared = names.iter().filter(|name| **name == module.name).count();
        if shared < 2 {
            continue;
        }

        let qualifier = if module.stock { "stock" } else { "upgraded" };
        module.name = format!("{} ({qualifier})", module.name);
    }

    let names: Vec<String> = modules.iter().map(|module| module.name.clone()).collect();
    for module in modules.iter_mut() {
        if names.iter().filter(|name| **name == module.name).count() > 1 {
            module.name = format!("{} ({})", module.name, module.param);
        }
    }
}

fn resolve_module<'a>(modules: &'a [Module], query: &str, ship: &Ship) -> Result<&'a Module, QueryError> {
    let trimmed = query.trim();
    if let Some(module) = modules.iter().find(|module| module.param.eq_ignore_ascii_case(trimmed)) {
        return Ok(module);
    }
    // Names that differ only in punctuation, such as "Mk IX" and "Mk IX*",
    // share a search key; the exact name tells them apart.
    if let Some(module) = modules.iter().find(|module| module.name.eq_ignore_ascii_case(trimmed)) {
        return Ok(module);
    }

    let key = search_key(query);

    // "B", "B hull" and "hull (B)" all name Hull B.
    let hull_key = key.strip_prefix("hull ").or_else(|| key.strip_suffix(" hull")).unwrap_or(&key);
    let exact: Vec<&Module> = modules
        .iter()
        .filter(|module| {
            let name = search_key(&module.name);
            name == key || (module.kind == ModuleKind::Hull && name == format!("hull {hull_key}"))
        })
        .collect();
    if let [module] = exact.as_slice() {
        return Ok(module);
    }

    let partial: Vec<&Module> = if exact.is_empty() {
        modules.iter().filter(|module| search_key(&module.name).contains(key.as_str())).collect()
    } else {
        exact
    };
    match partial.as_slice() {
        [] => Err(QueryError::ModuleNotFound {
            ship: ship.name.clone(),
            query: query.to_owned(),
            modules: modules.iter().map(|module| module.name.clone()).collect(),
        }),
        [module] => Ok(module),
        _ => Err(QueryError::AmbiguousModule {
            query: query.to_owned(),
            candidates: partial.iter().map(|module| module.name.clone()).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_letters_come_from_the_translated_name() {
        assert_eq!(hull_letter("Cleveland (B)"), Some('B'));
        assert_eq!(hull_letter("Des Moines (a)"), Some('A'));
        assert_eq!(hull_letter("Hull"), None);
        assert_eq!(hull_letter("Yamato (Refit)"), None);
    }
}

//! Equipped-component selection for the TTX engine.
//!
//! A [`ShipUpgradeSelection`] names, per `ShipUpgradeInfo` slot, which upgrade is
//! mounted. The TTX component maps ([`super::components::ShipTtxComponents`]) are
//! keyed by these same upgrade names, so the orchestration entry point resolves
//! each component by looking the selected name up in the matching map.
//!
//! Slots a ship lacks (e.g. a carrier's artillery/torpedo/fire-control slots, or a
//! gunless ship's artillery) carry `None` and produce a `None` stat section.

/// The upgrade mounted in each TTX-relevant `ShipUpgradeInfo` slot.
///
/// Names are the `ShipUpgradeInfo` keys (e.g. `PAUH911_Gearing_1945`), which is how
/// [`super::components::ShipTtxComponents`] keys its maps. Secondaries are not a
/// separate slot: the ATBA component is referenced by the hull upgrade, so the
/// secondary map is keyed by [`Self::hull`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShipUpgradeSelection {
    /// `_Hull` upgrade name (also keys the secondary/ATBA map).
    pub hull: Option<String>,
    /// `_Engine` upgrade name.
    pub engine: Option<String>,
    /// `_Artillery` upgrade name.
    pub artillery: Option<String>,
    /// `_Torpedoes` upgrade name.
    pub torpedoes: Option<String>,
    /// `_Suo` (fire-control) upgrade name.
    pub fire_control: Option<String>,
}

impl ShipUpgradeSelection {
    /// The stock (base) selection for `ship`: the empty-`prev` upgrade in each slot.
    ///
    /// Stock-pick basis: every `ShipUpgradeInfo` entry carries a `prev` field naming
    /// the upgrade it follows in its slot's research chain; the chain root (the stock
    /// module) is the unique entry with an empty `prev`. This is captured at parse
    /// time into [`super::components::ShipTtxComponents::stock_selection`] (the raw
    /// `ShipUpgradeInfo` pickle is not retained on the typed `Vehicle`), so this is a
    /// cheap field read. Slots the ship lacks stay `None`.
    ///
    /// Returns the default (all-`None`) selection when `ship` is not a vehicle or has
    /// no extracted TTX components.
    pub fn stock(ship: &crate::game_params::types::Param) -> ShipUpgradeSelection {
        ship.vehicle().and_then(|v| v.ttx_components()).map(|c| c.stock_selection().clone()).unwrap_or_default()
    }

    /// Build an explicit selection (e.g. from a replay's known equipped modules).
    pub fn new(
        hull: Option<String>,
        engine: Option<String>,
        artillery: Option<String>,
        torpedoes: Option<String>,
        fire_control: Option<String>,
    ) -> ShipUpgradeSelection {
        ShipUpgradeSelection { hull, engine, artillery, torpedoes, fire_control }
    }
}

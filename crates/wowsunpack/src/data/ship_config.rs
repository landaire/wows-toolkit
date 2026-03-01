//! Parser for the binary ship configuration blob found in replay RPC arguments.
//!
//! Each player's vehicle entity carries a `ShipConfig` that describes their equipped
//! modules, upgrades, consumables, signals, and other loadout details.

use winnow::Parser;
use winnow::binary::le_u32;
use winnow::combinator::repeat;

use super::Version;
use super::parser_utils::WResult;
use crate::game_types::GameParamId;

/// A player's ship loadout as encoded in replay data.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipConfig {
    ship_params_id: GameParamId,
    abilities: Vec<GameParamId>,
    hull: GameParamId,
    modernization: Vec<GameParamId>,
    units: Vec<GameParamId>,
    /// Exterior slot items (signals, camos, flags). Despite the field name, this covers
    /// all ExteriorSlots items, not just signal flags.
    exteriors: Vec<GameParamId>,
    ensigns: Vec<GameParamId>,
    ecoboosts: Vec<GameParamId>,
    naval_flag: u32,
    last_boarded_crew: u32,
}

impl ShipConfig {
    pub fn ship_params_id(&self) -> GameParamId {
        self.ship_params_id
    }

    pub fn exteriors(&self) -> &[GameParamId] {
        self.exteriors.as_ref()
    }

    pub fn units(&self) -> &[GameParamId] {
        self.units.as_ref()
    }

    pub fn modernization(&self) -> &[GameParamId] {
        self.modernization.as_ref()
    }

    pub fn hull(&self) -> GameParamId {
        self.hull
    }

    pub fn abilities(&self) -> &[GameParamId] {
        self.abilities.as_ref()
    }

    pub fn ensigns(&self) -> &[GameParamId] {
        self.ensigns.as_ref()
    }

    pub fn ecoboosts(&self) -> &[GameParamId] {
        self.ecoboosts.as_ref()
    }

    pub fn naval_flag(&self) -> u32 {
        self.naval_flag
    }

    pub fn last_boarded_crew(&self) -> u32 {
        self.last_boarded_crew
    }
}

/// Parse a ship configuration from a binary blob.
///
/// The blob format is version-dependent: versions >= 13.2 include an extra u32 field
/// after the unit slots.
pub fn parse_ship_config(blob: &[u8], version: &Version) -> WResult<ShipConfig> {
    let i = &mut &*blob;
    // Header: version, ship_params_id, element_count
    let _version = le_u32.parse_next(i)?;
    let ship_params_id = le_u32.parse_next(i)?;
    let _element_count = le_u32.parse_next(i)?;

    // Unit type slots (14 fixed slots from UNIT_TYPE_NAMES, some may be 0)
    let unit_count = le_u32.parse_next(i)?;
    let units: Vec<u32> = repeat(unit_count as usize, le_u32).parse_next(i)?;

    if version.is_at_least(&Version { major: 13, minor: 2, patch: 0, build: 0 }) {
        let _unk = le_u32.parse_next(i)?;
    }

    // ModernizationSlots: count + items
    let modernization_count = le_u32.parse_next(i)?;
    let modernization: Vec<u32> = repeat(modernization_count as usize, le_u32).parse_next(i)?;

    // ExteriorSlots: count + items (signals, camos, flags)
    let exterior_count = le_u32.parse_next(i)?;
    let exteriors: Vec<u32> = repeat(exterior_count as usize, le_u32).parse_next(i)?;

    // Supply state (purpose unknown, typically 0)
    let _supply_state = le_u32.parse_next(i)?;

    // ExteriorSlots color schemes: count + (slot_idx, scheme_id) pairs
    let color_scheme_count = le_u32.parse_next(i)?;
    let _color_schemes: Vec<(u32, u32)> = repeat(color_scheme_count as usize, (le_u32, le_u32)).parse_next(i)?;

    // AbilitySlots: count + items (consumables)
    let abilities_count = le_u32.parse_next(i)?;
    let abilities: Vec<u32> = repeat(abilities_count as usize, le_u32).parse_next(i)?;

    // EnsignSlots: count + items
    let ensign_count = le_u32.parse_next(i)?;
    let ensigns: Vec<u32> = repeat(ensign_count as usize, le_u32).parse_next(i)?;

    // EcoboostSlots: count + items (typically 4 slots, some may be 0)
    let ecoboost_count = le_u32.parse_next(i)?;
    let ecoboosts: Vec<u32> = repeat(ecoboost_count as usize, le_u32).parse_next(i)?;

    // Naval flag ID (NationFlags index)
    let naval_flag = le_u32.parse_next(i)?;

    // Full format extras: isOwned, lastBoardedCrew (commander/crew param ID)
    let _is_owned = le_u32.parse_next(i)?;
    let last_boarded_crew = le_u32.parse_next(i)?;

    let to_ids = |v: Vec<u32>| v.into_iter().map(GameParamId::from).collect();
    Ok(ShipConfig {
        ship_params_id: GameParamId::from(ship_params_id),
        abilities: to_ids(abilities),
        hull: GameParamId::from(units[0]),
        modernization: to_ids(modernization),
        units: to_ids(units),
        exteriors: to_ids(exteriors),
        ensigns: to_ids(ensigns),
        ecoboosts: to_ids(ecoboosts),
        naval_flag,
        last_boarded_crew,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: push a little-endian u32 to a byte buffer.
    fn push_u32(buf: &mut Vec<u8>, val: u32) {
        buf.extend_from_slice(&val.to_le_bytes());
    }

    /// Build a ship config blob with the given parameters.
    /// If `include_v13_2_field` is true, inserts the extra u32 after unit slots.
    fn build_blob(
        ship_params_id: u32,
        units: &[u32],
        modernization: &[u32],
        exteriors: &[u32],
        color_schemes: &[(u32, u32)],
        abilities: &[u32],
        ensigns: &[u32],
        ecoboosts: &[u32],
        naval_flag: u32,
        last_boarded_crew: u32,
        include_v13_2_field: bool,
    ) -> Vec<u8> {
        let mut buf = Vec::new();

        // Header
        push_u32(&mut buf, 1); // version
        push_u32(&mut buf, ship_params_id);
        push_u32(&mut buf, 0); // element_count

        // Units
        push_u32(&mut buf, units.len() as u32);
        for &u in units {
            push_u32(&mut buf, u);
        }

        // v13.2+ extra field
        if include_v13_2_field {
            push_u32(&mut buf, 0);
        }

        // Modernization
        push_u32(&mut buf, modernization.len() as u32);
        for &m in modernization {
            push_u32(&mut buf, m);
        }

        // Exteriors
        push_u32(&mut buf, exteriors.len() as u32);
        for &e in exteriors {
            push_u32(&mut buf, e);
        }

        // Supply state
        push_u32(&mut buf, 0);

        // Color schemes
        push_u32(&mut buf, color_schemes.len() as u32);
        for &(slot, scheme) in color_schemes {
            push_u32(&mut buf, slot);
            push_u32(&mut buf, scheme);
        }

        // Abilities
        push_u32(&mut buf, abilities.len() as u32);
        for &a in abilities {
            push_u32(&mut buf, a);
        }

        // Ensigns
        push_u32(&mut buf, ensigns.len() as u32);
        for &e in ensigns {
            push_u32(&mut buf, e);
        }

        // Ecoboosts
        push_u32(&mut buf, ecoboosts.len() as u32);
        for &e in ecoboosts {
            push_u32(&mut buf, e);
        }

        // Naval flag
        push_u32(&mut buf, naval_flag);

        // isOwned + lastBoardedCrew
        push_u32(&mut buf, 1); // isOwned
        push_u32(&mut buf, last_boarded_crew);

        buf
    }

    fn version_15_1() -> Version {
        Version { major: 15, minor: 1, patch: 0, build: 0 }
    }

    fn version_12_3() -> Version {
        Version { major: 12, minor: 3, patch: 0, build: 0 }
    }

    #[test]
    fn parse_minimal_config() {
        let blob = build_blob(
            4_293_001_168, // ship_params_id (Vermont)
            &[100],        // 1 unit (hull)
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            0,
            0,
            true,
        );
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 4_293_001_168);
        assert_eq!(config.hull().raw(), 100);
        assert_eq!(config.units().len(), 1);
        assert!(config.modernization().is_empty());
        assert!(config.exteriors().is_empty());
        assert!(config.abilities().is_empty());
        assert!(config.ensigns().is_empty());
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), 0);
        assert_eq!(config.last_boarded_crew(), 0);
    }

    #[test]
    fn parse_config_v13_2_extra_field() {
        let blob = build_blob(
            1000,
            &[10, 20, 30],
            &[200, 201],
            &[300],
            &[],
            &[400, 401, 402],
            &[],
            &[500, 501, 502, 503],
            7,
            9999,
            true, // v13.2 field present
        );
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 1000);
        assert_eq!(config.hull().raw(), 10);
        assert_eq!(config.units().len(), 3);
        assert_eq!(config.modernization().len(), 2);
        assert_eq!(config.exteriors().len(), 1);
        assert_eq!(config.abilities().len(), 3);
        assert!(config.ensigns().is_empty());
        assert_eq!(config.ecoboosts().len(), 4);
        assert_eq!(config.naval_flag(), 7);
        assert_eq!(config.last_boarded_crew(), 9999);
    }

    #[test]
    fn parse_config_pre_v13_2() {
        let blob = build_blob(
            2000,
            &[50, 60],
            &[150],
            &[250, 251, 252],
            &[],
            &[350],
            &[450],
            &[],
            3,
            8888,
            false, // no v13.2 field
        );
        let config = parse_ship_config(&blob, &version_12_3()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 2000);
        assert_eq!(config.hull().raw(), 50);
        assert_eq!(config.units().len(), 2);
        assert_eq!(config.modernization().len(), 1);
        assert_eq!(config.exteriors().len(), 3);
        assert_eq!(config.abilities().len(), 1);
        assert_eq!(config.ensigns().len(), 1);
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), 3);
        assert_eq!(config.last_boarded_crew(), 8888);
    }

    #[test]
    fn parse_empty_slots() {
        let blob = build_blob(
            42,
            &[1], // need at least one unit for hull
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            0,
            0,
            true,
        );
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert!(config.modernization().is_empty());
        assert!(config.exteriors().is_empty());
        assert!(config.abilities().is_empty());
        assert!(config.ensigns().is_empty());
        assert!(config.ecoboosts().is_empty());
    }

    #[test]
    fn parse_config_with_many_modules() {
        // Realistic slot counts: 14 units, 6 modernizations, 8 exteriors
        let units: Vec<u32> = (100..114).collect();
        let mods: Vec<u32> = (200..206).collect();
        let exteriors: Vec<u32> = (300..308).collect();
        let abilities: Vec<u32> = (400..404).collect();
        let ensigns: Vec<u32> = vec![500, 501];
        let ecoboosts: Vec<u32> = vec![600, 601, 602, 603];
        let color_schemes: Vec<(u32, u32)> = vec![(0, 10), (1, 11), (2, 12)];

        let blob = build_blob(
            5000,
            &units,
            &mods,
            &exteriors,
            &color_schemes,
            &abilities,
            &ensigns,
            &ecoboosts,
            15,
            7777,
            true,
        );
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 5000);
        assert_eq!(config.hull().raw(), 100); // first unit
        assert_eq!(config.units().len(), 14);
        assert_eq!(config.modernization().len(), 6);
        assert_eq!(config.exteriors().len(), 8);
        assert_eq!(config.abilities().len(), 4);
        assert_eq!(config.ensigns().len(), 2);
        assert_eq!(config.ecoboosts().len(), 4);
        assert_eq!(config.naval_flag(), 15);
        assert_eq!(config.last_boarded_crew(), 7777);

        // Verify specific GameParamId values
        assert_eq!(config.units()[5].raw(), 105);
        assert_eq!(config.modernization()[3].raw(), 203);
        assert_eq!(config.exteriors()[7].raw(), 307);
        assert_eq!(config.abilities()[2].raw(), 402);
    }

    #[test]
    fn default_ship_config() {
        let config = ShipConfig::default();
        assert_eq!(config.ship_params_id().raw(), 0);
        assert_eq!(config.hull().raw(), 0);
        assert!(config.units().is_empty());
        assert!(config.modernization().is_empty());
        assert!(config.exteriors().is_empty());
        assert!(config.abilities().is_empty());
        assert!(config.ensigns().is_empty());
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), 0);
        assert_eq!(config.last_boarded_crew(), 0);
    }
}

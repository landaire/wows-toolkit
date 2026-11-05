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
use crate::rpc::typedefs::ArgValue;

const EXTERIOR_KILL_TAG_STATE_MIN_VERSION: Version = Version::base(15, 9, 0);

/// Per-exterior kill-tag visibility override added to ship descriptions in 15.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExteriorKillTagState {
    exterior_id: GameParamId,
    enabled: bool,
}

impl ExteriorKillTagState {
    pub fn new(exterior_id: GameParamId, enabled: bool) -> Self {
        Self { exterior_id, enabled }
    }

    pub fn exterior_id(self) -> GameParamId {
        self.exterior_id
    }

    pub fn enabled(self) -> bool {
        self.enabled
    }
}
/// A player's ship loadout as encoded in replay data.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipConfig {
    ship_params_id: GameParamId,
    abilities: Vec<GameParamId>,
    modernization: Vec<GameParamId>,
    units: Vec<GameParamId>,
    /// Exterior slot items (signals, camos, flags). Despite the field name, this covers
    /// all ExteriorSlots items, not just signal flags.
    exteriors: Vec<GameParamId>,
    exterior_kill_tags: Option<Vec<ExteriorKillTagState>>,
    ensigns: Vec<GameParamId>,
    ecoboosts: Vec<GameParamId>,
    /// `None` when the (older, truncated) blob ended before these trailing fields.
    naval_flag: Option<u32>,
    last_boarded_crew: Option<u32>,
}

impl ShipConfig {
    /// A bare config carrying only the ship's GameParam id (stock loadout). Used for
    /// the <=0.11.x `SHIP_CONFIG` FIXED_DICT shape, whose `cd` blob uses an older
    /// layout we don't decode; the ship id comes from the dict's `shipId` field.
    pub fn stock(ship_params_id: GameParamId) -> Self {
        Self { ship_params_id, ..Default::default() }
    }

    pub fn ship_params_id(&self) -> GameParamId {
        self.ship_params_id
    }

    pub fn set_ship_params_id(&mut self, ship_params_id: GameParamId) {
        self.ship_params_id = ship_params_id;
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

    /// The hull is the first unit slot. `None` if no units were recorded.
    pub fn hull(&self) -> Option<GameParamId> {
        self.units.first().copied()
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

    /// `None` when the client predates exterior kill-tag state or the blob is truncated.
    pub fn exterior_kill_tags(&self) -> Option<&[ExteriorKillTagState]> {
        self.exterior_kill_tags.as_deref()
    }

    /// `None` for older clients whose blob omits the naval-flag field.
    pub fn naval_flag(&self) -> Option<u32> {
        self.naval_flag
    }

    /// `None` for older clients whose blob omits the crew tail.
    pub fn last_boarded_crew(&self) -> Option<u32> {
        self.last_boarded_crew
    }
}

/// Parse a ship configuration from a binary blob.
///
/// Resolve a `shipConfig` create-property value to a [`ShipConfig`], handling both
/// encodings: modern builds send a top-level BLOB; <=0.11.x clients send a
/// FIXED_DICT `{ shipId, cd: BLOB }`. The old `cd` blob uses a pre-modern layout
/// (its header is not the modern version/ship_params_id/element_count triple, so
/// feeding it to [`parse_ship_config`] misreads a hull-unit id as the ship id), so
/// for the dict shape we take the ship id from the dict's own `shipId` field and
/// use a stock loadout — enough to resolve the ship and its dispersion curve.
pub fn parse_ship_config_value(value: &ArgValue, version: &Version) -> Option<ShipConfig> {
    match value {
        ArgValue::Blob(blob) => parse_ship_config(blob, version).ok(),
        ArgValue::FixedDict(m) | ArgValue::NullableFixedDict(Some(m)) => {
            let ship_id = m.get("shipId").and_then(|v| v.as_u32())?;
            Some(ShipConfig::stock(GameParamId::from(ship_id)))
        }
        _ => None,
    }
}

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

    if version.is_at_least(&Version::base(13, 2, 0)) {
        let _unk = le_u32.parse_next(i)?;
    }

    // The trailing layout has grown over time: clients up to ~0.11.x truncate the blob
    // after the ensign slots (no ecoboosts, naval flag, or isOwned/lastBoardedCrew tail).
    // Read each remaining length-prefixed section only while bytes remain, so an old
    // loadout still yields its modernizations and consumables instead of the whole
    // parse failing once it runs off the end of a shorter blob. Full (modern) blobs
    // contain every section and parse identically.
    fn take_u32(i: &mut &[u8]) -> Option<u32> {
        let (head, rest) = i.split_first_chunk::<4>()?;
        *i = rest;
        Some(u32::from_le_bytes(*head))
    }
    fn take_section(i: &mut &[u8]) -> Vec<u32> {
        let Some(count) = take_u32(i) else { return Vec::new() };
        (0..count).map_while(|_| take_u32(i)).collect()
    }

    // ModernizationSlots
    let modernization = take_section(i);
    // ExteriorSlots (signals, camos, flags)
    let exteriors = take_section(i);
    // Bits 0 and 1 enable camouflage and signal auto-buy. Higher bits select
    // the purchase currency for each exterior slot.
    let _exterior_auto_buy_info = take_u32(i);
    // ExteriorSlots color schemes: count + (slot_idx, scheme_id) pairs.
    if let Some(color_scheme_count) = take_u32(i) {
        for _ in 0..color_scheme_count {
            take_u32(i);
            take_u32(i);
        }
    }

    // ExteriorStateKeeper.KillTagState: count + (exterior_id, enabled) pairs.
    let exterior_kill_tags = if version.is_at_least(&EXTERIOR_KILL_TAG_STATE_MIN_VERSION) {
        let states = take_u32(i).and_then(|count| {
            (0..count)
                .map(|_| Some(ExteriorKillTagState::new(GameParamId::from(take_u32(i)?), take_u32(i)? != 0)))
                .collect::<Option<Vec<_>>>()
        });
        if states.is_none() {
            *i = &[];
        }
        states
    } else {
        None
    };
    // AbilitySlots (consumables)
    let abilities = take_section(i);
    // EnsignSlots
    let ensigns = take_section(i);
    // EcoboostSlots (newer clients only)
    let ecoboosts = take_section(i);
    // Naval flag ID (NationFlags index); absent on older truncated blobs.
    let naval_flag = take_u32(i);
    // Full-format tail: isOwned, lastBoardedCrew (commander/crew param ID).
    let _is_owned = take_u32(i);
    let last_boarded_crew = take_u32(i);

    let to_ids = |v: Vec<u32>| v.into_iter().map(GameParamId::from).collect();
    Ok(ShipConfig {
        ship_params_id: GameParamId::from(ship_params_id),
        abilities: to_ids(abilities),
        modernization: to_ids(modernization),
        units: to_ids(units),
        exteriors: to_ids(exteriors),
        ensigns: to_ids(ensigns),
        ecoboosts: to_ids(ecoboosts),
        exterior_kill_tags,
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

    fn push_section(buf: &mut Vec<u8>, values: &[u32]) {
        push_u32(buf, values.len() as u32);
        for &value in values {
            push_u32(buf, value);
        }
    }

    #[derive(Default)]
    struct ShipConfigBlobBuilder {
        ship_params_id: u32,
        units: Vec<u32>,
        modernization: Vec<u32>,
        exteriors: Vec<u32>,
        color_schemes: Vec<(u32, u32)>,
        exterior_kill_tags: Option<Vec<ExteriorKillTagState>>,
        abilities: Vec<u32>,
        ensigns: Vec<u32>,
        ecoboosts: Vec<u32>,
        naval_flag: u32,
        last_boarded_crew: u32,
        include_v13_2_field: bool,
    }

    impl ShipConfigBlobBuilder {
        fn new(ship_params_id: u32) -> Self {
            Self { ship_params_id, ..Default::default() }
        }

        fn units(mut self, values: impl IntoIterator<Item = u32>) -> Self {
            self.units.extend(values);
            self
        }

        fn modernization(mut self, values: impl IntoIterator<Item = u32>) -> Self {
            self.modernization.extend(values);
            self
        }

        fn exteriors(mut self, values: impl IntoIterator<Item = u32>) -> Self {
            self.exteriors.extend(values);
            self
        }

        fn color_schemes(mut self, values: impl IntoIterator<Item = (u32, u32)>) -> Self {
            self.color_schemes.extend(values);
            self
        }

        fn exterior_kill_tags(mut self, values: impl IntoIterator<Item = ExteriorKillTagState>) -> Self {
            self.exterior_kill_tags = Some(values.into_iter().collect());
            self
        }

        fn abilities(mut self, values: impl IntoIterator<Item = u32>) -> Self {
            self.abilities.extend(values);
            self
        }

        fn ensigns(mut self, values: impl IntoIterator<Item = u32>) -> Self {
            self.ensigns.extend(values);
            self
        }

        fn ecoboosts(mut self, values: impl IntoIterator<Item = u32>) -> Self {
            self.ecoboosts.extend(values);
            self
        }

        fn naval_flag(mut self, naval_flag: u32) -> Self {
            self.naval_flag = naval_flag;
            self
        }

        fn last_boarded_crew(mut self, last_boarded_crew: u32) -> Self {
            self.last_boarded_crew = last_boarded_crew;
            self
        }

        fn with_v13_2_field(mut self) -> Self {
            self.include_v13_2_field = true;
            self
        }

        fn build(self) -> Vec<u8> {
            let mut buf = Vec::new();

            push_u32(&mut buf, 1);
            push_u32(&mut buf, self.ship_params_id);
            push_u32(&mut buf, 0);
            push_section(&mut buf, &self.units);

            if self.include_v13_2_field {
                push_u32(&mut buf, 0);
            }

            push_section(&mut buf, &self.modernization);
            push_section(&mut buf, &self.exteriors);
            push_u32(&mut buf, 0);
            push_u32(&mut buf, self.color_schemes.len() as u32);
            for (slot, scheme) in self.color_schemes {
                push_u32(&mut buf, slot);
                push_u32(&mut buf, scheme);
            }

            if let Some(exterior_kill_tags) = self.exterior_kill_tags {
                push_u32(&mut buf, exterior_kill_tags.len() as u32);
                for state in exterior_kill_tags {
                    push_u32(
                        &mut buf,
                        u32::try_from(state.exterior_id().raw()).expect("test exterior ID must fit the wire format"),
                    );
                    push_u32(&mut buf, state.enabled().into());
                }
            }

            push_section(&mut buf, &self.abilities);
            push_section(&mut buf, &self.ensigns);
            push_section(&mut buf, &self.ecoboosts);
            push_u32(&mut buf, self.naval_flag);
            push_u32(&mut buf, 1);
            push_u32(&mut buf, self.last_boarded_crew);
            buf
        }
    }

    fn version_15_1() -> Version {
        Version::base(15, 1, 0)
    }

    fn version_12_3() -> Version {
        Version::base(12, 3, 0)
    }

    #[test]
    fn parse_minimal_config() {
        let blob = ShipConfigBlobBuilder::new(4_293_001_168).units([100]).with_v13_2_field().build();
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 4_293_001_168);
        assert_eq!(config.hull().map(|h| h.raw()), Some(100));
        assert_eq!(config.units().len(), 1);
        assert!(config.modernization().is_empty());
        assert!(config.exteriors().is_empty());
        assert!(config.abilities().is_empty());
        assert!(config.ensigns().is_empty());
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), Some(0));
        assert_eq!(config.last_boarded_crew(), Some(0));
    }

    #[test]
    fn parse_config_v13_2_extra_field() {
        let blob = ShipConfigBlobBuilder::new(1000)
            .units([10, 20, 30])
            .modernization([200, 201])
            .exteriors([300])
            .abilities([400, 401, 402])
            .ecoboosts([500, 501, 502, 503])
            .naval_flag(7)
            .last_boarded_crew(9999)
            .with_v13_2_field()
            .build();
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 1000);
        assert_eq!(config.hull().map(|h| h.raw()), Some(10));
        assert_eq!(config.units().len(), 3);
        assert_eq!(config.modernization().len(), 2);
        assert_eq!(config.exteriors().len(), 1);
        assert_eq!(config.abilities().len(), 3);
        assert!(config.ensigns().is_empty());
        assert_eq!(config.ecoboosts().len(), 4);
        assert_eq!(config.naval_flag(), Some(7));
        assert_eq!(config.last_boarded_crew(), Some(9999));
    }

    #[test]
    fn parse_config_pre_v13_2() {
        let blob = ShipConfigBlobBuilder::new(2000)
            .units([50, 60])
            .modernization([150])
            .exteriors([250, 251, 252])
            .abilities([350])
            .ensigns([450])
            .naval_flag(3)
            .last_boarded_crew(8888)
            .build();
        let config = parse_ship_config(&blob, &version_12_3()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 2000);
        assert_eq!(config.hull().map(|h| h.raw()), Some(50));
        assert_eq!(config.units().len(), 2);
        assert_eq!(config.modernization().len(), 1);
        assert_eq!(config.exteriors().len(), 3);
        assert_eq!(config.abilities().len(), 1);
        assert_eq!(config.ensigns().len(), 1);
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), Some(3));
        assert_eq!(config.last_boarded_crew(), Some(8888));
    }

    #[test]
    fn parse_empty_slots() {
        let blob = ShipConfigBlobBuilder::new(42).units([1]).with_v13_2_field().build();
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
        let blob = ShipConfigBlobBuilder::new(5000)
            .units(100..114)
            .modernization(200..206)
            .exteriors(300..308)
            .color_schemes([(0, 10), (1, 11), (2, 12)])
            .abilities(400..404)
            .ensigns([500, 501])
            .ecoboosts([600, 601, 602, 603])
            .naval_flag(15)
            .last_boarded_crew(7777)
            .with_v13_2_field()
            .build();
        let config = parse_ship_config(&blob, &version_15_1()).unwrap();
        assert_eq!(config.ship_params_id().raw(), 5000);
        assert_eq!(config.hull().map(|h| h.raw()), Some(100)); // first unit
        assert_eq!(config.units().len(), 14);
        assert_eq!(config.modernization().len(), 6);
        assert_eq!(config.exteriors().len(), 8);
        assert_eq!(config.abilities().len(), 4);
        assert_eq!(config.ensigns().len(), 2);
        assert_eq!(config.ecoboosts().len(), 4);
        assert_eq!(config.naval_flag(), Some(15));
        assert_eq!(config.last_boarded_crew(), Some(7777));

        // Verify specific GameParamId values
        assert_eq!(config.units()[5].raw(), 105);
        assert_eq!(config.modernization()[3].raw(), 203);
        assert_eq!(config.exteriors()[7].raw(), 307);
        assert_eq!(config.abilities()[2].raw(), 402);
    }

    #[test]
    fn parse_truncated_old_blob_yields_partial_loadout() {
        // Clients up to ~0.11.x end the blob after the ensign slots: no ecoboost
        // section, naval flag, or crew tail. Simulate by building a full blob with
        // empty ecoboosts and lopping off the trailing four u32s (ecoboost count +
        // naval flag + isOwned + lastBoardedCrew).
        let mut blob = ShipConfigBlobBuilder::new(2000)
            .units([10, 20, 30])
            .modernization([200, 201])
            .exteriors([300])
            .abilities([400, 401])
            .build();
        blob.truncate(blob.len() - 16);

        let config = parse_ship_config(&blob, &version_12_3()).unwrap();
        // Everything up to the truncation still parses.
        assert_eq!(config.units().len(), 3);
        assert_eq!(config.hull().map(|h| h.raw()), Some(10));
        assert_eq!(config.modernization().len(), 2);
        assert_eq!(config.abilities().len(), 2);
        // The absent trailing fields read as missing, not as a fabricated zero.
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), None);
        assert_eq!(config.last_boarded_crew(), None);
    }

    #[test]
    fn default_ship_config() {
        let config = ShipConfig::default();
        assert_eq!(config.ship_params_id().raw(), 0);
        assert_eq!(config.hull(), None);
        assert!(config.units().is_empty());
        assert_eq!(config.exterior_kill_tags(), None);
        assert!(config.modernization().is_empty());
        assert!(config.exteriors().is_empty());
        assert!(config.abilities().is_empty());
        assert!(config.ensigns().is_empty());
        assert!(config.ecoboosts().is_empty());
        assert_eq!(config.naval_flag(), None);
        assert_eq!(config.last_boarded_crew(), None);
    }

    #[test]
    fn parse_v15_9_exterior_kill_tag_state() {
        let blob = ShipConfigBlobBuilder::new(1000)
            .units([10])
            .exteriors([300, 301])
            .exterior_kill_tags([
                ExteriorKillTagState::new(GameParamId::from(300u32), false),
                ExteriorKillTagState::new(GameParamId::from(301u32), true),
            ])
            .abilities([400])
            .naval_flag(7)
            .last_boarded_crew(9999)
            .with_v13_2_field()
            .build();

        let config = parse_ship_config(&blob, &Version::base(15, 9, 0)).unwrap();
        let states = config.exterior_kill_tags().unwrap();
        assert_eq!(
            states,
            [
                ExteriorKillTagState { exterior_id: GameParamId::from(300u32), enabled: false },
                ExteriorKillTagState { exterior_id: GameParamId::from(301u32), enabled: true },
            ]
        );
        assert_eq!(config.abilities(), [GameParamId::from(400u32)].as_slice());
    }

    #[test]
    fn parse_v15_8_layout_does_not_read_exterior_kill_tag_state() {
        let blob = ShipConfigBlobBuilder::new(1000)
            .units([10])
            .exteriors([300, 301])
            .abilities([400, 401])
            .ensigns([500])
            .ecoboosts([600])
            .naval_flag(7)
            .last_boarded_crew(9999)
            .with_v13_2_field()
            .build();

        let config = parse_ship_config(&blob, &Version::base(15, 8, 0)).unwrap();
        assert_eq!(config.exterior_kill_tags(), None);
        assert_eq!(config.abilities(), [GameParamId::from(400u32), GameParamId::from(401u32)].as_slice());
        assert_eq!(config.ensigns(), [GameParamId::from(500u32)].as_slice());
        assert_eq!(config.ecoboosts(), [GameParamId::from(600u32)].as_slice());
        assert_eq!(config.naval_flag(), Some(7));
        assert_eq!(config.last_boarded_crew(), Some(9999));
    }

    #[test]
    fn parse_v15_9_incomplete_exterior_kill_tag_state_is_absent() {
        const AFTER_KILL_TAG_COUNT: usize = 44;
        const AFTER_KILL_TAG_EXTERIOR_ID: usize = 48;

        let complete_blob = ShipConfigBlobBuilder::new(1000)
            .units([10])
            .exterior_kill_tags([ExteriorKillTagState::new(GameParamId::from(300u32), false)])
            .with_v13_2_field()
            .build();

        for end in [AFTER_KILL_TAG_COUNT, AFTER_KILL_TAG_EXTERIOR_ID] {
            let config = parse_ship_config(&complete_blob[..end], &Version::base(15, 9, 0)).unwrap();
            assert_eq!(config.exterior_kill_tags(), None);
            assert!(config.abilities().is_empty());
        }
    }

    #[test]
    fn roster_ship_id_overrides_config_header_id() {
        let mut config = ShipConfig::stock(GameParamId::from(4_076_779_344u32));
        config.set_ship_params_id(GameParamId::from(438_335u32));
        assert_eq!(config.ship_params_id(), GameParamId::from(438_335u32));
    }
}

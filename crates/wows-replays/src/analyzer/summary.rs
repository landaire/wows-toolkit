use wowsunpack::rpc::typedefs::ArgValue;

use wowsunpack::data::Version;
use wowsunpack::game_constants::DEFAULT_BATTLE_CONSTANTS;

use crate::analyzer::decoder::DamageStatCategory;
use crate::analyzer::decoder::DamageStatEntry;
use crate::analyzer::decoder::DamageStatWeapon;
use crate::analyzer::decoder::Recognized;
use crate::packet2::EntityMethodPacket;
use crate::packet2::Packet;
use crate::packet2::PacketType;
use std::collections::BTreeMap;
use std::collections::HashMap;

use serde::Serialize;

use super::analyzer::Analyzer;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Serialize)]
pub enum Ribbon {
    PlaneShotDown,
    Incapacitation,
    SetFire,
    Citadel,
    SecondaryHit,
    OverPenetration,
    Penetration,
    NonPenetration,
    Ricochet,
    TorpedoProtectionHit,
    Captured,
    AssistedInCapture,
    Spotted,
    Destroyed,
    TorpedoHit,
    Defended,
    Flooding,
    DiveBombPenetration,
    RocketPenetration,
    RocketNonPenetration,
    RocketTorpedoProtectionHit,
    ShotDownByAircraft,
}

/// The recording player's ribbons and damage, collected by [`Summary`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SummaryStats {
    pub ribbons: BTreeMap<Ribbon, usize>,

    /// From the last damage report, summed over weapons.
    pub enemy_damage: f64,
}

/// Collects the recording player's ribbons and damage reports.
pub struct Summary {
    ribbons: HashMap<Ribbon, usize>,
    damage: HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry>,
}

impl Summary {
    pub fn new() -> Self {
        Self { ribbons: HashMap::new(), damage: HashMap::new() }
    }

    pub fn stats(&self) -> SummaryStats {
        let enemy_damage: f64 = self
            .damage
            .values()
            .filter(|entry| entry.category == Recognized::Known(DamageStatCategory::Enemy))
            .map(|entry| entry.total)
            .sum();

        SummaryStats { ribbons: self.ribbons.iter().map(|(ribbon, count)| (*ribbon, *count)).collect(), enemy_damage }
    }
}

impl Default for Summary {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer for Summary {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &Packet<'_, '_>) {
        // Collect banners, damage reports, etc.
        if let Packet {
            payload: PacketType::EntityMethod(EntityMethodPacket { entity_id: _entity_id, method, args }),
            ..
        } = packet
        {
            if *method == "onRibbon" {
                let ribbon = match &args[0] {
                    ArgValue::Int8(ribbon) => ribbon,
                    _ => panic!("foo"),
                };
                let ribbon = match ribbon {
                    1 => Ribbon::TorpedoHit,
                    3 => Ribbon::PlaneShotDown,
                    4 => Ribbon::Incapacitation,
                    5 => Ribbon::Destroyed,
                    6 => Ribbon::SetFire,
                    7 => Ribbon::Flooding,
                    8 => Ribbon::Citadel,
                    9 => Ribbon::Defended,
                    10 => Ribbon::Captured,
                    11 => Ribbon::AssistedInCapture,
                    13 => Ribbon::SecondaryHit,
                    14 => Ribbon::OverPenetration,
                    15 => Ribbon::Penetration,
                    16 => Ribbon::NonPenetration,
                    17 => Ribbon::Ricochet,
                    19 => Ribbon::Spotted,
                    21 => Ribbon::DiveBombPenetration,
                    25 => Ribbon::RocketPenetration,
                    26 => Ribbon::RocketNonPenetration,
                    27 => Ribbon::ShotDownByAircraft,
                    28 => Ribbon::TorpedoProtectionHit,
                    30 => Ribbon::RocketTorpedoProtectionHit,
                    _ => {
                        panic!("Unrecognized ribbon {}", ribbon);
                    }
                };
                if let std::collections::hash_map::Entry::Vacant(e) = self.ribbons.entry(ribbon) {
                    e.insert(1);
                } else {
                    *self.ribbons.get_mut(&ribbon).unwrap() += 1;
                }
            } else if *method == "receiveDamageStat" {
                let value = pickled::de::value_from_slice(
                    match &args[0] {
                        ArgValue::Blob(x) => x,
                        _ => panic!("foo"),
                    },
                    pickled::de::DeOptions::new(),
                )
                .unwrap();

                match value {
                    pickled::value::Value::Dict(d) => {
                        for (k, v) in d.inner().iter() {
                            let (weapon_raw, category_raw) = match k {
                                pickled::value::HashableValue::Tuple(t) => {
                                    let t = t.inner();
                                    assert!(t.len() == 2);
                                    (
                                        match t[0] {
                                            pickled::value::HashableValue::I64(i) => i,
                                            _ => panic!("foo"),
                                        },
                                        match t[1] {
                                            pickled::value::HashableValue::I64(i) => i,
                                            _ => panic!("foo"),
                                        },
                                    )
                                }
                                _ => panic!("foo"),
                            };
                            let (count, total) = match v {
                                pickled::value::Value::List(t) => {
                                    let t = t.inner();
                                    assert!(t.len() == 2);
                                    (
                                        match t[0] {
                                            pickled::value::Value::I64(i) => i,
                                            _ => panic!("foo"),
                                        },
                                        match t[1] {
                                            pickled::value::Value::F64(i) => i,
                                            // Spotting damage can be sent as integer 0
                                            pickled::value::Value::I64(i) => i as f64,
                                            _ => panic!("foo"),
                                        },
                                    )
                                }
                                _ => panic!("foo"),
                            };

                            let version = Version::from_client_exe("0,0,0,0");
                            let weapon =
                                DamageStatWeapon::from_id(weapon_raw as i32, &DEFAULT_BATTLE_CONSTANTS, version)
                                    .unwrap_or_else(|| Recognized::Unknown(weapon_raw.to_string()));
                            let category =
                                DamageStatCategory::from_id(category_raw as i32, &DEFAULT_BATTLE_CONSTANTS, version)
                                    .unwrap_or_else(|| Recognized::Unknown(category_raw.to_string()));
                            let key = (weapon.clone(), category.clone());
                            let entry = DamageStatEntry { weapon, category, count, total };
                            self.damage.insert(key, entry);
                        }
                    }
                    _ => panic!("foo"),
                }
            }
        }
    }
}

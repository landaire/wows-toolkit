use wowsunpack::rpc::typedefs::ArgValue;

use wowsunpack::data::Version;
use wowsunpack::game_constants::DEFAULT_BATTLE_CONSTANTS;

use crate::analyzer::decoder::{DamageStatCategory, DamageStatEntry, DamageStatWeapon, Recognized};
use crate::packet2::EntityMethodPacket;
use crate::packet2::Packet;
use crate::packet2::PacketType;
use std::collections::HashMap;

use super::analyzer::Analyzer;

pub struct SummaryBuilder;

impl Default for SummaryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SummaryBuilder {
    pub fn new() -> Self {
        Self
    }

    pub fn build(self, meta: &crate::ReplayMeta) -> Box<dyn Analyzer> {
        println!("Username: {}", meta.playerName);
        println!("Date/time: {}", meta.dateTime);
        println!("Map: {}", meta.mapDisplayName);
        println!("Vehicle: {}", meta.playerVehicle);
        println!("Game mode: {} {:?}", meta.name, meta.gameLogic);
        println!("Game version: {}", meta.clientVersionFromExe);
        println!();

        Box::new(Summary { ribbons: HashMap::new(), damage: HashMap::new() })
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
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

struct Summary {
    ribbons: HashMap<Ribbon, usize>,
    damage: HashMap<(Recognized<DamageStatWeapon>, Recognized<DamageStatCategory>), DamageStatEntry>,
}

impl Analyzer for Summary {
    fn finish(&mut self) {
        for (ribbon, count) in self.ribbons.iter() {
            println!("{:?}: {}", ribbon, count);
        }
        println!();

        let enemy_damage: f64 = self
            .damage
            .values()
            .filter(|entry| entry.category == Recognized::Known(DamageStatCategory::Enemy))
            .map(|entry| entry.total)
            .sum();
        println!("Total damage: {:.0}", enemy_damage);
    }

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
                            let weapon = DamageStatWeapon::from_id(weapon_raw as i32, &DEFAULT_BATTLE_CONSTANTS, version)
                                .unwrap_or(Recognized::Unknown(format!("{weapon_raw}")));
                            let category = DamageStatCategory::from_id(category_raw as i32, &DEFAULT_BATTLE_CONSTANTS, version)
                                .unwrap_or(Recognized::Unknown(format!("{category_raw}")));
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

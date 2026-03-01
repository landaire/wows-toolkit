use wowsunpack::rpc::typedefs::ArgValue;

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
    damage: HashMap<(i64, i64), (i64, f64)>,
}

impl Analyzer for Summary {
    fn finish(&mut self) {
        for (ribbon, count) in self.ribbons.iter() {
            println!("{:?}: {}", ribbon, count);
        }
        println!();
        println!(
            "Total damage: {:.0}",
            self.damage.get(&(1, 0)).unwrap_or(&(0, 0.)).1
                + self.damage.get(&(2, 0)).unwrap_or(&(0, 0.)).1
                + self.damage.get(&(17, 0)).unwrap_or(&(0, 0.)).1
        );
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
                            let k = match k {
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
                            let v = match v {
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
                                            // TODO: This appears in the (17,2) key,
                                            // it is unknown what it means
                                            pickled::value::Value::I64(i) => i as f64,
                                            _ => panic!("foo"),
                                        },
                                    )
                                }
                                _ => panic!("foo"),
                            };
                            //println!("{:?}: {:?}", k, v);

                            // The (1,0) key is (# AP hits that dealt damage, total AP damage dealt)
                            // (1,3) is (# artillery fired, total possible damage) ?
                            // (2, 0) is (# HE penetrations, total HE damage)
                            // (17, 0) is (# fire tick marks, total fire damage)
                            self.damage.insert(k, v);
                        }
                    }
                    _ => panic!("foo"),
                }
            }
        }
    }
}

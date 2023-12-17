use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    path::Path,
    str::FromStr,
    time::Instant,
};

use egui::ahash::HashMapExt;
use flate2::read::ZlibDecoder;
use itertools::Itertools;
use ouroboros::self_referencing;
use serde_pickle::{DeOptions, HashableValue, Value};
use wows_replays::{
    game_params::{Param, ParamBuilder, ParamData, Species},
    resource_loader::{EntityType, Vehicle, VehicleBuilder},
};
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

use crate::error::DataLoadError;

pub struct GameParams {
    params: wows_replays::game_params::GameParams,
    param_id_to_translation_id: HashMap<i64, String>,
    #[borrows(params)]
    #[covariant]
    param_id_to_dict: HashMap<i64, &'this BTreeMap<HashableValue, Value>>,
}

impl GameParams {
    pub fn from_pkg(
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Result<GameParams, DataLoadError> {
        println!("loading game params");
        let cache_path = Path::new("game_params.bin");
        println!("deserializing gameparams");

        let start = Instant::now();
        let params = cache_path
            .exists()
            .then(|| {
                let cache_data = std::fs::File::open(cache_path).ok()?;
                bincode::deserialize_from(cache_data).ok()
            })
            .flatten();

        let params = if let Some(params) = params {
            params
        } else {
            let game_params = file_tree.find("content/GameParams.data")?;
            let mut game_params_data = Vec::new();
            game_params.read_file(pkg_loader, &mut game_params_data)?;
            game_params_data.reverse();

            let mut decompressed_data = Cursor::new(Vec::new());
            let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));
            std::io::copy(&mut decoder, &mut decompressed_data)?;
            decompressed_data.set_position(0);
            let pickled_params = serde_pickle::from_reader(
                &mut decompressed_data,
                DeOptions::default()
                    .replace_unresolved_globals()
                    .decode_strings(),
            )
            .expect("failed to load game params");

            let new_params = if let Value::List(mut params_list) = pickled_params {
                let params = params_list.remove(0);
                if let Value::Dict(params_dict) = params {
                    params_dict.values().filter_map(|param| {
                        if let Value::Dict(param_data) = param {
                            param_data.get(&HashableValue::String("typeinfo".to_string())).and_then(|type_info| {
                                if let Value::Dict(type_info) = type_info {
                                    Some((type_info.get(&HashableValue::String("nation".to_string()))?, type_info.get(&HashableValue::String("species".to_string()))?, type_info.get(&HashableValue::String("type".to_string()))?))
                                } else {
                                    None
                                }
                            }).and_then(|(nation, species, typ)| {
                                if let (Value::String(nation), Value::String(typ)) = (nation, typ) {
                                    let entity_type = EntityType::from_str(&typ).ok()?;
                                    let nation = nation.clone();
                                    let species = if let Value::String(species) = species{
                                        Species::from_str(species).ok()
                                    } else {
                                        None
                                    };

                                    let parsed_param_data = match entity_type {
                                        EntityType::Ship => {
                                            let level = if let Value::I64(level) = param_data.get(&HashableValue::String("level".to_string())).expect("vehicle does not have level attribute") {
                                                *level as u32
                                            } else {
                                                panic!("vehicle level is not an i64");
                                            };
                                            let group = if let Value::String(group) = param_data.get(&HashableValue::String("group".to_string())).expect("vehicle does not have group attribute") {
                                                group.clone()
                                            } else {
                                                panic!("vehicle leve is not an i64");
                                            };
                                            VehicleBuilder::default().level(level).group(group).build().ok().map(|v| ParamData::Vehicle(v))
                                    },
                                    _ => None,
                                    }?;

                                    let id = if let Value::I64(id) = param_data.get(&HashableValue::String("id".to_string())).expect("param has no id field") {
                                        *id as u32
                                    } else {
                                        panic!("param id is not an i64");
                                    };

                                    let index = if let Value::String(index) = param_data.get(&HashableValue::String("index".to_string())).expect("param has no index field") {
                                        index.clone()
                                    } else {
                                        panic!("param index is not a string");
                                    };

                                    let name = if let Value::String(name) = param_data.get(&HashableValue::String("name".to_string())).expect("param has no name field") {
                                        name.clone()
                                    } else {
                                        panic!("param name is not a string");
                                    };

                                    ParamBuilder::default().id(id).index(index).name(name).species(species).nation(nation).data(parsed_param_data).build().ok()
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        }
                    }).collect::<Vec<wows_replays::game_params::Param>>()
                } else {
                    panic!("params is not a list");
                }
            } else {
                panic!("game params are not a list");
            };

            let file = std::fs::File::create(cache_path).unwrap();
            bincode::serialize_into(file, &new_params)
                .expect("failed to serialize cached game params");

            new_params
        };

        let now = Instant::now();
        println!("took {} seconds to load", (now - start).as_secs());

        panic!("{:#?}", params);

        // println!("loading ships");

        // let mut ship_id_to_translation_id = HashMap::new();
        // // let mut ship_id_to_ship = HashMap::new();
        // if let Value::List(mut list) = params {
        //     params = list.remove(0);
        // }
        // if let Value::Dict(dict) = &params {
        //     for (key, value) in dict {
        //         if let Value::Dict(ship_dict) = value {
        //             let id = ship_dict
        //                 .get(&HashableValue::String("id".to_string()))
        //                 .unwrap();
        //             let id = if let Value::I64(id) = id {
        //                 *id
        //             } else {
        //                 panic!("ID is not an i64")
        //             };

        //             let index = ship_dict
        //                 .get(&HashableValue::String("index".to_string()))
        //                 .unwrap();
        //             let index = if let Value::String(index) = index {
        //                 index.clone()
        //             } else {
        //                 panic!("ID is not an i64")
        //             };

        //             ship_id_to_translation_id.insert(id, format!("IDS_{index}"));
        //             // ship_id_to_ship.insert(id, dict);
        //         }
        //     }
        // }
        // panic!("???");

        // Ok(GameParamsBuilder {
        //     params,
        //     param_id_to_translation_id: ship_id_to_translation_id,
        //     param_id_to_dict_builder: |params| {
        //         let mut param_id_to_dict = HashMap::new();
        //         if let Value::Dict(root_dict) = &params {
        //             for (key, value) in root_dict {
        //                 if let Value::Dict(ship_dict) = value {
        //                     let id = ship_dict
        //                         .get(&HashableValue::String("id".to_string()))
        //                         .unwrap();
        //                     let id = if let Value::I64(id) = id {
        //                         *id
        //                     } else {
        //                         panic!("ID is not an i64")
        //                     };

        //                     param_id_to_dict.insert(id, ship_dict);
        //                 }
        //             }
        //         }

        //         param_id_to_dict
        //     },
        // }
        // .build())
    }

    pub fn ship_id_to_localization_id(&self, ship_id: i64) -> Option<&str> {
        self.borrow_param_id_to_translation_id()
            .get(&ship_id)
            .map(|s| s.as_str())
    }

    pub fn ship_type_info(&self, ship_id: i64) -> Option<&BTreeMap<HashableValue, Value>> {
        self.borrow_param_id_to_dict()
            .get(&ship_id)
            .map(|s| {
                if let Some(Value::Dict(dict)) =
                    s.get(&HashableValue::String("typeinfo".to_string()))
                {
                    Some(dict)
                } else {
                    None
                }
            })
            .flatten()
    }

    // pub fn get(&self, path: &str) -> Option<&serde_pickle::Value> {
    //     let path_parts = path.split("/");
    //     let mut current = Some(&self.0);
    //     while let Some(serde_pickle::Value::Dict(dict)) = current {

    //     }
    //     None
    // }
}

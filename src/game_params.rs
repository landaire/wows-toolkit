use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    path::Path,
    str::FromStr,
    time::Instant,
};

use egui::ahash::HashMapExt;
use flate2::read::ZlibDecoder;
use gettext::Catalog;
use itertools::Itertools;
use ouroboros::self_referencing;
use serde_pickle::{DeOptions, HashableValue, Value};
use wows_replays::{
    game_params::{GameParamProvider, GameParams, Param, ParamBuilder, ParamData, Species},
    resource_loader::{EntityType, ResourceLoader, Vehicle, VehicleBuilder},
};
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

use crate::error::DataLoadError;

pub struct GameMetadataProvider {
    params: wows_replays::game_params::GameParams,
    param_id_to_translation_id: HashMap<u32, String>,
    translations: Option<Catalog>,
}

impl GameParamProvider for GameMetadataProvider {
    fn by_id(&self, id: u32) -> Option<&Param> {
        self.params.by_id(id)
    }

    fn by_index(&self, index: &str) -> Option<&Param> {
        todo!()
    }

    fn by_name(&self, name: &str) -> Option<&Param> {
        todo!()
    }
}

impl ResourceLoader for GameMetadataProvider {
    fn localized_name_from_param(&self, param: &Param) -> Option<&str> {
        self.param_localization_id(param.id()).and_then(|id| {
            self.translations
                .as_ref()
                .map(|catalog| catalog.gettext(id))
        })
    }

    fn localized_name_from_id(&self, id: &str) -> Option<&str> {
        self.translations
            .as_ref()
            .map(|catalog| catalog.gettext(id))
    }

    fn param_by_id(&self, id: u32) -> Option<&Param> {
        self.params.by_id(id)
    }
}

impl GameMetadataProvider {
    pub fn from_pkg(
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Result<GameMetadataProvider, DataLoadError> {
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
            let pickled_params: Value = serde_pickle::from_reader(
                &mut decompressed_data,
                DeOptions::default()
                    .replace_unresolved_globals()
                    .decode_strings(),
            )
            .expect("failed to load game params");

            let params_list = pickled_params
                .list_ref()
                .expect("Root game params is not a list");

            let params = &params_list[0];
            let params_dict = params
                .dict_ref()
                .expect("First element of GameParams is not a dictionary");

            let new_params = params_dict
                .values()
                .filter_map(|param| {
                    let param_data = param
                        .dict_ref()
                        .expect("Params root level dictionary values are not dictionaries");

                    param_data
                        .get(&HashableValue::String("typeinfo".to_string()))
                        .and_then(|type_info| {
                            type_info.dict_ref().and_then(|type_info| {
                                Some((
                                    type_info.get(&HashableValue::String("nation".to_string()))?,
                                    type_info.get(&HashableValue::String("species".to_string()))?,
                                    type_info.get(&HashableValue::String("type".to_string()))?,
                                ))
                            })
                        })
                        .and_then(|(nation, species, typ)| {
                            if let (Value::String(nation), Value::String(typ)) = (nation, typ) {
                                let entity_type = EntityType::from_str(&typ).ok()?;
                                let nation = nation.clone();
                                let species =
                                    species.string_ref().and_then(|s| Species::from_str(s).ok());

                                let parsed_param_data = match entity_type {
                                    EntityType::Ship => {
                                        let level = *param_data
                                            .get(&HashableValue::String("level".to_string()))
                                            .expect("vehicle does not have level attribute")
                                            .i64_ref()
                                            .expect("vehicle level is not an int64")
                                            as u32;

                                        let group = param_data
                                            .get(&HashableValue::String("group".to_string()))
                                            .expect("vehicle does not have group attribute")
                                            .string_ref()
                                            .cloned()
                                            .expect("vehicle group is not a string");

                                        VehicleBuilder::default()
                                            .level(level)
                                            .group(group)
                                            .build()
                                            .ok()
                                            .map(|v| ParamData::Vehicle(v))
                                    }
                                    _ => None,
                                }?;

                                let id = *param_data
                                    .get(&HashableValue::String("id".to_string()))
                                    .expect("param has no id field")
                                    .i64_ref()
                                    .expect("param id is not an i64")
                                    as u32;

                                let index = param_data
                                    .get(&HashableValue::String("index".to_string()))
                                    .expect("param has no index field")
                                    .string_ref()
                                    .cloned()
                                    .expect("param index is not a string");

                                let name = param_data
                                    .get(&HashableValue::String("name".to_string()))
                                    .expect("param has no name field")
                                    .string_ref()
                                    .cloned()
                                    .expect("param name is not a string");

                                ParamBuilder::default()
                                    .id(id)
                                    .index(index)
                                    .name(name)
                                    .species(species)
                                    .nation(nation)
                                    .data(parsed_param_data)
                                    .build()
                                    .ok()
                            } else {
                                None
                            }
                        })
                })
                .collect::<Vec<wows_replays::game_params::Param>>();

            let file = std::fs::File::create(cache_path).unwrap();
            bincode::serialize_into(file, &new_params)
                .expect("failed to serialize cached game params");

            new_params
        };

        let now = Instant::now();
        println!("took {} seconds to load", (now - start).as_secs());

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

        let param_id_to_translation_id = HashMap::from_iter(
            params
                .iter()
                .map(|param| (param.id(), format!("IDS_{}", param.index()))),
        );

        Ok(GameMetadataProvider {
            params: params.into(),
            param_id_to_translation_id,
            translations: None,
        })
    }

    pub fn set_translations(&mut self, catalog: Catalog) {
        self.translations = Some(catalog);
    }

    pub fn param_localization_id(&self, ship_id: u32) -> Option<&str> {
        self.param_id_to_translation_id
            .get(&ship_id)
            .map(|s| s.as_str())
    }

    // pub fn get(&self, path: &str) -> Option<&serde_pickle::Value> {
    //     let path_parts = path.split("/");
    //     let mut current = Some(&self.0);
    //     while let Some(serde_pickle::Value::Dict(dict)) = current {

    //     }
    //     None
    // }
}

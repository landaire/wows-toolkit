use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    time::Instant,
};

use egui::ahash::HashMapExt;
use flate2::read::ZlibDecoder;
use itertools::Itertools;
use ouroboros::self_referencing;
use serde_pickle::{DeOptions, HashableValue, Value};
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

use crate::error::DataLoadError;

#[self_referencing]
pub struct GameParams {
    params: serde_pickle::Value,
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
        let game_params = file_tree.find("content/GameParams.data")?;
        let mut game_params_data = Vec::new();
        game_params.read_file(pkg_loader, &mut game_params_data)?;
        game_params_data.reverse();

        let mut decompressed_data = Cursor::new(Vec::new());
        let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));
        std::io::copy(&mut decoder, &mut decompressed_data)?;
        decompressed_data.set_position(0);

        println!("deserializing gameparams");

        let start = Instant::now();
        let mut params: Value = serde_pickle::from_reader(
            &mut decompressed_data,
            DeOptions::default()
                .replace_unresolved_globals()
                .decode_strings(),
        )
        .expect("failed to load game params");

        let now = Instant::now();
        println!("took {} seconds to load", (now - start).as_secs());

        println!("loading ships");

        let mut ship_id_to_translation_id = HashMap::new();
        // let mut ship_id_to_ship = HashMap::new();
        if let Value::List(mut list) = params {
            params = list.remove(0);
        }
        if let Value::Dict(dict) = &params {
            for (key, value) in dict {
                if let Value::Dict(ship_dict) = value {
                    let id = ship_dict
                        .get(&HashableValue::String("id".to_string()))
                        .unwrap();
                    let id = if let Value::I64(id) = id {
                        *id
                    } else {
                        panic!("ID is not an i64")
                    };

                    let index = ship_dict
                        .get(&HashableValue::String("index".to_string()))
                        .unwrap();
                    let index = if let Value::String(index) = index {
                        index.clone()
                    } else {
                        panic!("ID is not an i64")
                    };

                    ship_id_to_translation_id.insert(id, format!("IDS_{index}"));
                    // ship_id_to_ship.insert(id, dict);
                }
            }
        }

        Ok(GameParamsBuilder {
            params,
            param_id_to_translation_id: ship_id_to_translation_id,
            param_id_to_dict_builder: |params| {
                let mut param_id_to_dict = HashMap::new();
                if let Value::Dict(root_dict) = &params {
                    for (key, value) in root_dict {
                        if let Value::Dict(ship_dict) = value {
                            let id = ship_dict
                                .get(&HashableValue::String("id".to_string()))
                                .unwrap();
                            let id = if let Value::I64(id) = id {
                                *id
                            } else {
                                panic!("ID is not an i64")
                            };

                            param_id_to_dict.insert(id, ship_dict);
                        }
                    }
                }

                param_id_to_dict
            },
        }
        .build())
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

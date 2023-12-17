use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    path::Path,
    time::Instant,
};

use bson::{Decimal128, Document};
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

fn hashable_pickle_to_bson(pickled: &serde_pickle::HashableValue) -> bson::Bson {
    match pickled {
        serde_pickle::HashableValue::None => bson::Bson::Null,
        serde_pickle::HashableValue::Bool(v) => bson::Bson::Boolean(*v),
        serde_pickle::HashableValue::I64(v) => bson::Bson::Int64(*v),
        serde_pickle::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
        serde_pickle::HashableValue::F64(v) => bson::to_bson(&v).unwrap(),
        serde_pickle::HashableValue::Bytes(v) => bson::to_bson(&v).unwrap(),
        serde_pickle::HashableValue::String(v) => bson::Bson::String(v.clone()),
        serde_pickle::HashableValue::Tuple(v) => {
            bson::Bson::Array(v.into_iter().map(hashable_pickle_to_bson).collect())
        }
        serde_pickle::HashableValue::FrozenSet(v) => {
            bson::Bson::Array(v.into_iter().map(hashable_pickle_to_bson).collect())
        }
    }
}

pub fn pickle_to_bson(pickled: &serde_pickle::Value) -> bson::Bson {
    match pickled {
        serde_pickle::Value::None => bson::Bson::Null,
        serde_pickle::Value::Bool(v) => bson::Bson::Boolean(*v),
        serde_pickle::Value::I64(v) => bson::Bson::Int64(*v),
        serde_pickle::Value::Int(_v) => todo!("Int -> JSON"),
        serde_pickle::Value::F64(v) => bson::to_bson(&v).unwrap(),
        serde_pickle::Value::Bytes(v) => bson::to_bson(&v).unwrap(),
        serde_pickle::Value::String(v) => bson::to_bson(&v).unwrap(),
        serde_pickle::Value::List(v) => {
            bson::Bson::Array(v.into_iter().map(pickle_to_bson).collect())
        }
        serde_pickle::Value::Tuple(v) => {
            bson::Bson::Array(v.into_iter().map(pickle_to_bson).collect())
        }
        serde_pickle::Value::Set(v) => {
            bson::Bson::Array(v.into_iter().map(hashable_pickle_to_bson).collect())
        }
        serde_pickle::Value::FrozenSet(v) => {
            bson::Bson::Array(v.into_iter().map(hashable_pickle_to_bson).collect())
        }
        serde_pickle::Value::Dict(v) => {
            let mut doc = bson::Document::new();
            for (key, value) in v {
                let converted_key = hashable_pickle_to_bson(key);
                let string_key = match converted_key {
                    bson::Bson::Int32(num) => num.to_string(),
                    bson::Bson::Int64(num) => num.to_string(),
                    bson::Bson::String(s) => s.to_string(),
                    _other => {
                        continue;
                        // panic!(
                        //     "Unsupported key type: {:?} (original: {:#?}, {:#?})",
                        //     other, key, v
                        // );
                    }
                };

                let converted_value = pickle_to_bson(value);

                doc.insert(string_key, converted_value);
            }

            bson::Bson::Document(doc)
        }
    }
}

fn hashable_pickle_to_cbor(pickled: &serde_pickle::HashableValue) -> cbor::Cbor {
    match pickled {
        serde_pickle::HashableValue::None => cbor::Cbor::Null,
        serde_pickle::HashableValue::Bool(v) => cbor::Cbor::Bool(*v),
        serde_pickle::HashableValue::I64(v) => cbor::Cbor::Signed(cbor::CborSigned::Int64(*v)),
        serde_pickle::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
        serde_pickle::HashableValue::F64(v) => cbor::Cbor::Float(cbor::CborFloat::Float64(*v)),
        serde_pickle::HashableValue::Bytes(v) => cbor::Cbor::Bytes(cbor::CborBytes(v.clone())),
        serde_pickle::HashableValue::String(v) => cbor::Cbor::Unicode(v.clone()),
        serde_pickle::HashableValue::Tuple(v) => {
            cbor::Cbor::Array(v.into_iter().map(hashable_pickle_to_cbor).collect())
        }
        serde_pickle::HashableValue::FrozenSet(v) => {
            cbor::Cbor::Array(v.into_iter().map(hashable_pickle_to_cbor).collect())
        }
    }
}

pub fn pickle_to_cbor(pickled: &serde_pickle::Value) -> cbor::Cbor {
    match pickled {
        serde_pickle::Value::None => cbor::Cbor::Null,
        serde_pickle::Value::Bool(v) => cbor::Cbor::Bool(*v),
        serde_pickle::Value::I64(v) => cbor::Cbor::Signed(cbor::CborSigned::Int64(*v)),
        serde_pickle::Value::Int(_v) => todo!("Int -> JSON"),
        serde_pickle::Value::F64(v) => cbor::Cbor::Float(cbor::CborFloat::Float64(*v)),
        serde_pickle::Value::Bytes(v) => cbor::Cbor::Bytes(cbor::CborBytes(v.clone())),
        serde_pickle::Value::String(v) => cbor::Cbor::Unicode(v.clone()),
        serde_pickle::Value::List(v) => {
            cbor::Cbor::Array(v.into_iter().map(pickle_to_cbor).collect())
        }
        serde_pickle::Value::Tuple(v) => {
            cbor::Cbor::Array(v.into_iter().map(pickle_to_cbor).collect())
        }
        serde_pickle::Value::Set(v) => {
            cbor::Cbor::Array(v.into_iter().map(hashable_pickle_to_cbor).collect())
        }
        serde_pickle::Value::FrozenSet(v) => {
            cbor::Cbor::Array(v.into_iter().map(hashable_pickle_to_cbor).collect())
        }
        serde_pickle::Value::Dict(v) => {
            cbor::Cbor::Map(HashMap::from_iter(v.iter().filter_map(|(key, value)| {
                let converted_key = hashable_pickle_to_cbor(key);
                let string_key = match converted_key {
                    cbor::Cbor::Unsigned(unsigned) => unsigned.into_u64().to_string(),
                    cbor::Cbor::Signed(signed) => signed.into_i64().to_string(),
                    cbor::Cbor::Unicode(s) => s.to_string(),
                    other => {
                        return None;
                    }
                };
                let converted_value = pickle_to_cbor(value);
                Some((string_key, converted_value))
            })))
        }
    }
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
        let mut params: Value = if cache_path.exists() {
            let data = std::fs::read(cache_path).unwrap();
            let mut decoder = cbor::Decoder::from_reader(data.as_slice());
            let des: cbor::Cbor = decoder.items().next().unwrap().unwrap();

            let now = Instant::now();
            panic!("took {} seconds to load", (now - start).as_secs());
        } else {
            let game_params = file_tree.find("content/GameParams.data")?;
            let mut game_params_data = Vec::new();
            game_params.read_file(pkg_loader, &mut game_params_data)?;
            game_params_data.reverse();

            let mut decompressed_data = Cursor::new(Vec::new());
            let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));
            std::io::copy(&mut decoder, &mut decompressed_data)?;
            decompressed_data.set_position(0);
            let params = serde_pickle::from_reader(
                &mut decompressed_data,
                DeOptions::default()
                    .replace_unresolved_globals()
                    .decode_strings(),
            )
            .expect("failed to load game params");

            let mut file = std::fs::File::create(cache_path).unwrap();
            let mut encoder = cbor::Encoder::from_writer(&mut file);
            encoder.encode([&pickle_to_cbor(&params)]).unwrap();

            params
        };

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

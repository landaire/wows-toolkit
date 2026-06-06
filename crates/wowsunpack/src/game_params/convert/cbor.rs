use std::io::Read;
use std::io::Write;

use crate::error::GameDataError;
#[cfg(feature = "cbor")]
use ciborium::value::Value as CborValue;

#[cfg(feature = "cbor")]
fn hashable_pickle_to_cbor(pickled: pickled::HashableValue) -> CborValue {
    match pickled {
        pickled::HashableValue::None => CborValue::Null,
        pickled::HashableValue::Bool(v) => CborValue::Bool(v),
        pickled::HashableValue::I64(v) => CborValue::Integer(v.into()),
        pickled::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
        pickled::HashableValue::F64(v) => CborValue::Float(v),
        pickled::HashableValue::Bytes(v) => CborValue::Bytes(v.into_raw_or_cloned()),
        pickled::HashableValue::String(v) => CborValue::Text(v.into_raw_or_cloned()),
        pickled::HashableValue::Tuple(v) => {
            CborValue::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_cbor).collect())
        }
        pickled::HashableValue::FrozenSet(v) => {
            CborValue::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_cbor).collect())
        }
    }
}

#[cfg(feature = "cbor")]
pub fn pickle_to_cbor(pickled: pickled::Value) -> CborValue {
    match pickled {
        pickled::Value::None => CborValue::Null,
        pickled::Value::Bool(v) => CborValue::Bool(v),
        pickled::Value::I64(v) => CborValue::Integer(v.into()),
        pickled::Value::Int(_v) => todo!("Int -> JSON"),
        pickled::Value::F64(v) => CborValue::Float(v),
        pickled::Value::Bytes(v) => CborValue::Bytes(v.into_raw_or_cloned()),
        pickled::Value::String(v) => CborValue::Text(v.into_raw_or_cloned()),
        pickled::Value::List(v) => {
            CborValue::Array(v.into_raw_or_cloned().into_iter().map(pickle_to_cbor).collect())
        }
        pickled::Value::Tuple(v) => {
            CborValue::Array(v.into_raw_or_cloned().into_iter().map(pickle_to_cbor).collect())
        }
        pickled::Value::Set(v) => {
            CborValue::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_cbor).collect())
        }
        pickled::Value::FrozenSet(v) => {
            CborValue::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_cbor).collect())
        }
        pickled::Value::Dict(v) => {
            use std::collections::BTreeMap;

            // Dedup colliding string keys (last write wins) and keep a stable,
            // lexically-sorted key order, matching the previous BTreeMap-based
            // encoding. ciborium's `Value::Map` is a plain Vec and does neither
            // on its own, so we collect into a BTreeMap first.
            let v = v.into_raw_or_cloned();
            let mut map: BTreeMap<String, CborValue> = BTreeMap::new();
            for (key, value) in &v {
                let converted_key = hashable_pickle_to_cbor(key.clone());
                let string_key = match converted_key {
                    CborValue::Integer(num) => i128::from(num).to_string(),
                    CborValue::Text(s) => s,
                    _other => {
                        continue;
                        // panic!(
                        //     "Unsupported key type: {:?} (original: {:#?}, {:#?})",
                        //     other, key, v
                        // );
                    }
                };

                map.insert(string_key, pickle_to_cbor(value.clone()));
            }

            CborValue::Map(map.into_iter().map(|(k, value)| (CborValue::Text(k), value)).collect())
        }
        pickled::Value::Object(ref o) => pickle_to_cbor(o.inner().__reduce__().state_or_none()),
    }
}

#[cfg(feature = "cbor")]
pub fn read_game_params_as_cbor<W: Write>(
    vfs: &vfs::VfsPath,
    writer: &mut W,
) -> Result<(), crate::error::GameDataError> {
    use super::game_params_to_pickle;

    let mut game_params_data = Vec::new();
    vfs.join("content/GameParams.data")?.open_file()?.read_to_end(&mut game_params_data)?;

    let decoded = game_params_to_pickle(game_params_data)?;

    let converted = if let pickled::Value::List(list) = decoded {
        pickle_to_cbor(list.into_raw_or_cloned().into_iter().next().unwrap())
    } else {
        return Err(GameDataError::InvalidGameParamsData);
    };

    Ok(ciborium::into_writer(&converted, writer)?)
}

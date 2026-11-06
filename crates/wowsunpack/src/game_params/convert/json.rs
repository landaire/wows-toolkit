use serde_json::Map;

#[cfg(feature = "vfs")]
use std::io::Read;
#[cfg(feature = "vfs")]
use std::io::Write;

#[cfg(feature = "vfs")]
use crate::error::GameDataError;

fn hashable_pickle_to_json(pickled: pickled::HashableValue) -> serde_json::Value {
    match pickled {
        pickled::HashableValue::None => serde_json::Value::Null,
        pickled::HashableValue::Bool(v) => serde_json::Value::Bool(v),
        pickled::HashableValue::I64(v) => serde_json::Value::Number(serde_json::Number::from(v)),
        pickled::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
        pickled::HashableValue::F64(v) => {
            serde_json::Value::Number(serde_json::Number::from_f64(v).expect("invalid f64"))
        }
        pickled::HashableValue::Bytes(v) => serde_json::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                .collect(),
        ),
        pickled::HashableValue::String(v) => serde_json::Value::String(v.into_raw_or_cloned()),
        pickled::HashableValue::Tuple(v) => {
            serde_json::Value::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_json).collect())
        }
        pickled::HashableValue::FrozenSet(v) => {
            serde_json::Value::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_json).collect())
        }
    }
}

pub fn pickle_to_json(pickled: pickled::Value) -> serde_json::Value {
    match pickled {
        pickled::Value::None => serde_json::Value::Null,
        pickled::Value::Bool(v) => serde_json::Value::Bool(v),
        pickled::Value::I64(v) => serde_json::Value::Number(serde_json::Number::from(v)),
        pickled::Value::Int(_v) => todo!("Int -> JSON"),
        pickled::Value::F64(v) => serde_json::Value::Number(serde_json::Number::from_f64(v).expect("invalid f64")),
        pickled::Value::Bytes(v) => serde_json::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                .collect(),
        ),
        pickled::Value::String(v) => serde_json::Value::String(v.into_raw_or_cloned()),
        pickled::Value::List(v) => {
            serde_json::Value::Array(v.into_raw_or_cloned().into_iter().map(pickle_to_json).collect())
        }
        pickled::Value::Tuple(v) => {
            serde_json::Value::Array(v.into_raw_or_cloned().into_iter().map(pickle_to_json).collect())
        }
        pickled::Value::Set(v) => {
            serde_json::Value::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_json).collect())
        }
        pickled::Value::FrozenSet(v) => {
            serde_json::Value::Array(v.into_raw_or_cloned().into_iter().map(hashable_pickle_to_json).collect())
        }
        pickled::Value::Dict(v) => {
            let mut map = Map::new();
            let v = v.into_raw_or_cloned();
            for (key, value) in &v {
                let converted_key = hashable_pickle_to_json(key.clone());
                let string_key = match converted_key {
                    serde_json::Value::Number(num) => num.to_string(),
                    serde_json::Value::String(s) => s.to_string(),
                    _other => {
                        continue;
                        // panic!(
                        //     "Unsupported key type: {:?} (original: {:#?}, {:#?})",
                        //     other, key, v
                        // );
                    }
                };

                let converted_value = pickle_to_json(value.clone());

                map.insert(string_key, converted_value);
            }

            serde_json::Value::Object(map)
        }
        pickled::Value::Object(ref o) => pickle_to_json(o.inner().__reduce__().state_or_none()),
        // Recursive back-edge: no finite serialization, emit null.
        pickled::Value::Weak(_) => serde_json::Value::Null,
    }
}

/// Each base parameter of a decoded GameParams tree as its name and JSON, in
/// dictionary order. A parameter is converted when the iterator reaches it, so
/// a caller that drops each one holds at most one converted parameter.
pub fn base_params_json(
    root: &pickled::Value,
) -> Result<impl Iterator<Item = (String, serde_json::Value)> + use<>, crate::error::GameDataError> {
    let dict = super::base_params_dict(root)?;
    // Cloning shares the parameter trees; it copies no parameter data.
    let entries: Vec<(pickled::HashableValue, pickled::Value)> =
        dict.inner().iter().map(|(key, value)| (key.clone(), value.clone())).collect();

    Ok(entries.into_iter().filter_map(|(key, value)| {
        let name = match hashable_pickle_to_json(key) {
            serde_json::Value::String(name) => name,
            serde_json::Value::Number(number) => number.to_string(),
            // Param names are strings; any other key is not a parameter.
            _ => return None,
        };

        Some((name, pickle_to_json(value)))
    }))
}

#[cfg(feature = "vfs")]
pub fn read_game_params_as_json<W: Write>(
    pretty_print: bool,
    vfs: &vfs::VfsPath,
    writer: &mut W,
) -> Result<(), crate::error::GameDataError> {
    let mut game_params_data = Vec::new();
    vfs.join("content/GameParams.data")?.open_file()?.read_to_end(&mut game_params_data)?;

    let decoded = super::game_params_to_pickle(game_params_data)?;
    println!("decoded to pickle");

    let converted = if let pickled::Value::List(list) = decoded {
        pickle_to_json(list.into_raw_or_cloned().into_iter().next().unwrap())
    } else {
        return Err(GameDataError::InvalidGameParamsData);
    };

    println!("converted to json");

    if pretty_print {
        serde_json::to_writer_pretty(writer, &converted)?;
    } else {
        serde_json::to_writer(writer, &converted)?;
    }

    Ok(())
}

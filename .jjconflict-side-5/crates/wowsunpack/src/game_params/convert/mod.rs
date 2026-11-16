#[cfg(feature = "cbor")]
mod cbor;
#[cfg(feature = "json")]
mod json;

#[cfg(feature = "cbor")]
pub use crate::game_params::convert::cbor::*;

#[cfg(feature = "json")]
pub use crate::game_params::convert::json::*;

use std::io::Cursor;

use flate2::read::ZlibDecoder;
use pickled::DeOptions;
use pickled::HashableValue;
use pickled::Value;
use pickled::object::DictObject;
use pickled::value::Shared;

use crate::error::GameDataError;

/// Dict extraction for both `Value::Dict` and `Value::Object`.
pub(crate) trait ValueDictExt {
    fn dict_or_object_dict(&self) -> Option<Shared<pickled::Dict>>;
}

impl ValueDictExt for Value {
    fn dict_or_object_dict(&self) -> Option<Shared<pickled::Dict>> {
        match self {
            Value::Dict(d) => Some(d.clone()),
            Value::Object(o) => {
                let inner = o.inner();
                let dict_obj = inner.as_any().downcast_ref::<DictObject>()?;
                Some(Shared::new(dict_obj.state().clone()))
            }
            _ => None,
        }
    }
}

/// Converts a raw pickled GameParams.data file to its pickled representation. This operation is quite
/// expensive.
///
/// Uses `decode_strings()` which tries UTF-8 first, then falls back to latin1 decoding.
/// This handles both modern builds (UTF-8) and old builds (Python 2 latin1 byte strings).
pub fn game_params_to_pickle(mut game_params_data: Vec<u8>) -> Result<pickled::Value, crate::error::GameDataError> {
    game_params_data.reverse();

    let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));

    // `replace_reconstructor_objects_structures` is required, not optional:
    // GameParams pickles build their `GPData` entries through
    // `copy_reg._reconstructor`, and without it those decode to an unresolved
    // global, which `replace_unresolved_globals` then turns into `None`. The
    // loss is silent and partial -- a crew keeps its scalar fields but loses
    // `Skills` -- so it reads as missing game data rather than a decode error.
    Ok(pickled::value_from_reader(
        &mut decoder,
        DeOptions::default()
            .replace_unresolved_globals()
            .replace_recursive_structures()
            .replace_reconstructor_objects_structures()
            .decode_strings(),
    )?)
}

/// The base parameters of a decoded GameParams tree, keyed by param name.
///
/// Modern builds wrap them as `{"": {name: param}}`; old builds store the
/// dictionary flat, or as the first element of a list or tuple.
pub fn base_params_dict(root: &Value) -> Result<Shared<pickled::Dict>, GameDataError> {
    let dict = if let Some(root_dict) = root.dict_or_object_dict() {
        let nested = root_dict.inner().get(&HashableValue::String(String::new().into())).cloned();
        match nested {
            Some(nested) => nested.dict_or_object_dict(),
            None => Some(root_dict),
        }
    } else if let Some(list) = root.list_ref() {
        list.inner().first().and_then(Value::dict_or_object_dict)
    } else if let Some(tuple) = root.tuple_ref() {
        tuple.inner().first().and_then(Value::dict_or_object_dict)
    } else {
        None
    };

    dict.ok_or(GameDataError::InvalidGameParamsData)
}

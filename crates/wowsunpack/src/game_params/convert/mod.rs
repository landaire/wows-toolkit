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

/// Converts a raw pickled GameParams.data file to its pickled representation. This operation is quite
/// expensive.
///
/// Uses `decode_strings()` which tries UTF-8 first, then falls back to latin1 decoding.
/// This handles both modern builds (UTF-8) and old builds (Python 2 latin1 byte strings).
pub fn game_params_to_pickle(mut game_params_data: Vec<u8>) -> Result<pickled::Value, crate::error::GameDataError> {
    game_params_data.reverse();

    let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));

    Ok(pickled::value_from_reader(
        &mut decoder,
        DeOptions::default().replace_unresolved_globals().replace_recursive_structures().decode_strings(),
    )?)
}

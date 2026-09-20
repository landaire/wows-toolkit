//! Dumping the game's `GameParams.data` to a readable file.
//!
//! The archive is one pickled tree; a dump either writes it whole or writes
//! the smaller decoded set the toolkit itself keeps. Reported as errors rather
//! than panics, so a bad path or a full disk surfaces in the UI.

use std::fs::File;
use std::io::BufWriter;
use std::io::Read as _;
use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;
use wowsunpack::game_params::convert::game_params_to_pickle;
use wowsunpack::vfs::VfsPath;

/// Where the pickled parameters sit inside a build's package VFS.
pub const GAME_PARAMS_PATH: &str = "content/GameParams.data";

/// What a dump writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameParamsFormat {
    /// The whole pickled tree, as indented JSON.
    Json,
    /// The whole pickled tree, as CBOR.
    Cbor,
    /// Only the parameters the toolkit decoded, as JSON. Much smaller, and
    /// only the fields the toolkit understands.
    MinimalJson,
    /// The same decoded set, as CBOR.
    MinimalCbor,
}

impl GameParamsFormat {
    pub const ALL: [GameParamsFormat; 4] = [Self::Json, Self::Cbor, Self::MinimalJson, Self::MinimalCbor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Cbor => "CBOR",
            Self::MinimalJson => "Minimal JSON",
            Self::MinimalCbor => "Minimal CBOR",
        }
    }

    /// The extension a file of this format gets.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Json | Self::MinimalJson => "json",
            Self::Cbor | Self::MinimalCbor => "cbor",
        }
    }

    /// Whether this format writes the toolkit's decoded set rather than the
    /// raw pickled tree, and so needs a loaded metadata provider.
    pub fn is_minimal(self) -> bool {
        matches!(self, Self::MinimalJson | Self::MinimalCbor)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GameParamsDumpError {
    #[error("the build has no {GAME_PARAMS_PATH}")]
    NotFound {
        #[source]
        source: wowsunpack::vfs::VfsError,
    },
    #[error("{GAME_PARAMS_PATH} could not be read")]
    Read {
        #[source]
        source: wowsunpack::vfs::VfsError,
    },
    #[error("{GAME_PARAMS_PATH} could not be decoded")]
    Decode(String),
    /// The root was neither of the two shapes the base-parameters option
    /// knows how to reach into.
    #[error("the parameters root is not a dictionary or a list, so it has no base entry")]
    NoBaseParameters,
    #[error("could not create {path}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}")]
    Write { path: PathBuf, reason: String },
}

/// Reads and decodes the pickled parameters out of `package_vfs`.
///
/// Reads the whole file: it is one archive and the decoder wants it entire.
/// Callers run this off the UI thread.
pub fn load_pickled(package_vfs: &VfsPath) -> Result<pickled::Value, GameParamsDumpError> {
    let archive = package_vfs.join(GAME_PARAMS_PATH).map_err(|source| GameParamsDumpError::NotFound { source })?;

    let mut bytes = Vec::new();
    archive
        .open_file()
        .and_then(|mut file| {
            file.read_to_end(&mut bytes)?;
            Ok(())
        })
        .map_err(|source| GameParamsDumpError::Read { source })?;

    game_params_to_pickle(bytes).map_err(|err| GameParamsDumpError::Decode(err.to_string()))
}

/// Narrows the decoded tree to its base parameters.
///
/// The root is a dictionary keyed by an empty string, or a list whose first
/// entry is the same thing; anything else has no base entry to take.
pub fn base_parameters(root: pickled::Value) -> Result<pickled::Value, GameParamsDumpError> {
    match root {
        pickled::Value::Dict(dict) => dict
            .inner()
            .get(&pickled::HashableValue::String(String::new().into()))
            .cloned()
            .ok_or(GameParamsDumpError::NoBaseParameters),
        pickled::Value::List(list) => {
            let mut inner = list.inner_mut();
            if inner.is_empty() {
                return Err(GameParamsDumpError::NoBaseParameters);
            }
            Ok(inner.remove(0))
        }
        _ => Err(GameParamsDumpError::NoBaseParameters),
    }
}

/// Writes `value` to `path` in `format`.
pub fn write_value<T: Serialize>(value: &T, path: &Path, format: GameParamsFormat) -> Result<(), GameParamsDumpError> {
    let file = BufWriter::new(
        File::create(path).map_err(|source| GameParamsDumpError::Create { path: path.to_path_buf(), source })?,
    );

    match format {
        GameParamsFormat::Json => {
            let mut serializer = serde_json::Serializer::pretty(file);
            value
                .serialize(&mut serializer)
                .map_err(|err| GameParamsDumpError::Write { path: path.to_path_buf(), reason: err.to_string() })
        }
        GameParamsFormat::MinimalJson => serde_json::to_writer(file, value)
            .map_err(|err| GameParamsDumpError::Write { path: path.to_path_buf(), reason: err.to_string() }),
        GameParamsFormat::Cbor | GameParamsFormat::MinimalCbor => ciborium::into_writer(value, file)
            .map_err(|err| GameParamsDumpError::Write { path: path.to_path_buf(), reason: err.to_string() }),
    }
}

/// Dumps the raw pickled tree, optionally narrowed to its base parameters.
///
/// The minimal formats do not come through here: they write the toolkit's own
/// decoded parameters, which the caller holds, not this tree.
pub fn dump_pickled(
    package_vfs: &VfsPath,
    path: &Path,
    format: GameParamsFormat,
    base_params_only: bool,
) -> Result<(), GameParamsDumpError> {
    let root = load_pickled(package_vfs)?;
    let value = if base_params_only { base_parameters(root)? } else { root };
    write_value(&value, path, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wt-gpui-gameparams-{name}-{}.out", std::process::id()))
    }

    #[test]
    fn each_format_names_its_extension_and_says_whether_it_needs_the_provider() {
        assert_eq!(GameParamsFormat::Json.extension(), "json");
        assert_eq!(GameParamsFormat::Cbor.extension(), "cbor");
        assert_eq!(GameParamsFormat::MinimalJson.extension(), "json");
        assert!(!GameParamsFormat::Json.is_minimal());
        assert!(GameParamsFormat::MinimalJson.is_minimal());
        assert!(GameParamsFormat::MinimalCbor.is_minimal());
        assert_eq!(GameParamsFormat::ALL.len(), 4);
    }

    #[test]
    fn base_parameters_takes_the_empty_key_out_of_a_dictionary_root() {
        let dict = pickled::value::Dict::from_sorted_unchecked(vec![(
            pickled::HashableValue::String(String::new().into()),
            pickled::Value::I64(7),
        )]);
        let root = pickled::Value::Dict(pickled::value::Shared::new(dict));

        assert_eq!(base_parameters(root).unwrap(), pickled::Value::I64(7));
    }

    #[test]
    fn base_parameters_takes_the_first_entry_of_a_list_root() {
        let root =
            pickled::Value::List(pickled::value::Shared::new(vec![pickled::Value::I64(1), pickled::Value::I64(2)]));
        assert_eq!(base_parameters(root).unwrap(), pickled::Value::I64(1));
    }

    #[test]
    fn a_root_with_no_base_entry_is_reported_rather_than_panicking() {
        assert!(matches!(base_parameters(pickled::Value::I64(3)), Err(GameParamsDumpError::NoBaseParameters)));
        assert!(matches!(
            base_parameters(pickled::Value::List(pickled::value::Shared::new(Vec::new()))),
            Err(GameParamsDumpError::NoBaseParameters)
        ));
        let empty = pickled::value::Dict::new();
        assert!(matches!(
            base_parameters(pickled::Value::Dict(pickled::value::Shared::new(empty))),
            Err(GameParamsDumpError::NoBaseParameters)
        ));
    }

    #[test]
    fn writing_json_produces_a_readable_file() {
        let path = temp_path("json");
        write_value(&pickled::Value::I64(42), &path, GameParamsFormat::Json).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("42"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writing_to_a_path_that_cannot_be_created_is_reported() {
        // A path under a file, rather than a directory, cannot be created.
        let blocker = temp_path("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("nested.json");

        let error =
            write_value(&pickled::Value::I64(1), &path, GameParamsFormat::Json).expect_err("the parent is a file");
        assert!(matches!(error, GameParamsDumpError::Create { .. }));
        let _ = std::fs::remove_file(&blocker);
    }
}

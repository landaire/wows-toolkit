/// VFS abstraction for reading files from an assets.bin PrototypeDatabase
#[cfg(feature = "vfs")]
pub mod assets_bin_vfs;
/// Main logic for parsing the game's resource index files
pub mod idx;
/// VFS abstraction for reading files from IDX/PKG archives
#[cfg(feature = "vfs")]
pub mod idx_vfs;
/// Shared winnow parsing utilities
pub mod parser_utils;
/// Utilities for helping load and maintain `.pkg` files (memmap-backed)
#[cfg(feature = "vfs-mmap")]
pub mod pkg;
/// Ship configuration (loadout) binary parser
pub mod ship_config;
// File tree serialization utilities
pub mod serialization;
/// Wrapper types for VFS data sources
#[cfg(feature = "vfs")]
pub mod wrappers;

use std::borrow::Cow;

use crate::Rc;
use crate::error::GameDataError;
use crate::game_params::types::Param;
use crate::game_types::GameParamId;
use crate::rpc::entitydefs::EntitySpec;

pub trait ResourceLoader {
    fn localized_name_from_param(&self, param: &Param) -> Option<String>;
    fn localized_name_from_id(&self, id: &str) -> Option<String>;
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>>;
    fn entity_specs(&self) -> &[EntitySpec];
}

/// Game version. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::data::Version` paths keep working.
pub use wows_core::Version;

pub trait DataFileLoader {
    fn get(&self, path: &str) -> Result<Cow<'static, [u8]>, GameDataError>;
}

pub struct DataFileWithCallback<F> {
    callback: F,
}

impl<F> DataFileWithCallback<F>
where
    F: Fn(&str) -> Result<Cow<'static, [u8]>, GameDataError>,
{
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> DataFileLoader for DataFileWithCallback<F>
where
    F: Fn(&str) -> Result<Cow<'static, [u8]>, GameDataError>,
{
    fn get(&self, path: &str) -> Result<Cow<'static, [u8]>, GameDataError> {
        (self.callback)(path)
    }
}


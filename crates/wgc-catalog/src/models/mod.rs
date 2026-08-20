//! Typed models for WGC update-service (WGUS) XML responses.

mod metadata;
mod newtypes;
mod patches_chain;

pub use metadata::AppId;
pub use metadata::ClientType;
pub use metadata::ClientTypes;
pub use metadata::Executable;
pub use metadata::Executables;
pub use metadata::LaunchOption;
pub use metadata::LaunchOptions;
pub use metadata::MetadataResponse;
pub use metadata::PredefinedSection;
pub use metadata::parse_metadata;
pub use newtypes::BuildVersion;
pub use newtypes::InfoHash;
pub use newtypes::PartName;
pub use patches_chain::File;
pub use patches_chain::Files;
pub use patches_chain::PartInfo;
pub use patches_chain::PartsInfo;
pub use patches_chain::Patch;
pub use patches_chain::PatchesChain;
pub use patches_chain::PatchesChainResponse;
pub use patches_chain::Torrent;
pub use patches_chain::Urls;
pub use patches_chain::WebSeedUrl;
pub use patches_chain::WebSeeds;
pub use patches_chain::parse_patches_chain;

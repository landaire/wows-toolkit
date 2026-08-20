//! `api/v1/patches_chain/` response: enumerates downloadable builds per part.

use serde::Deserialize;

use crate::error::CatalogError;
use crate::models::newtypes::BuildVersion;
use crate::models::newtypes::InfoHash;
use crate::models::newtypes::PartName;

#[derive(Debug, Clone, Deserialize)]
pub struct PatchesChainResponse {
    #[serde(rename = "@name")]
    pub protocol_name: String,
    #[serde(rename = "@version")]
    pub protocol_version: String,
    pub parts_info: PartsInfo,
    #[serde(default, rename = "patches_chain")]
    pub patches_chain: Vec<PatchesChain>,
    #[serde(default)]
    pub web_seeds: WebSeeds,
    pub version_name: String,
    pub meta_need_update: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartsInfo {
    #[serde(default, rename = "part")]
    pub part: Vec<PartInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartInfo {
    pub name: PartName,
    pub version: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PatchesChain {
    #[serde(rename = "@type")]
    pub chain_type: String,
    #[serde(default, rename = "patch")]
    pub patch: Vec<Patch>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Patch {
    pub files: Files,
    pub torrent: Torrent,
    pub part: PartName,
    pub version_to: BuildVersion,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Files {
    #[serde(default, rename = "file")]
    pub file: Vec<File>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct File {
    pub name: String,
    pub size: u64,
    pub unpacked_size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Torrent {
    pub hash: InfoHash,
    pub urls: Urls,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Urls {
    #[serde(default, rename = "url")]
    pub url: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WebSeeds {
    #[serde(default, rename = "url")]
    pub url: Vec<WebSeedUrl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebSeedUrl {
    #[serde(rename = "@threads")]
    pub threads: u32,
    #[serde(rename = "@name")]
    pub name: String,
    /// The web-seed base URL. Kept as raw text (not `url::Url`) since a
    /// malformed value should surface at the call site that needs to use it,
    /// not fail the whole response parse.
    #[serde(rename = "$text")]
    pub base: String,
}

/// Parse an `api/v1/patches_chain/` XML response body.
pub fn parse_patches_chain(xml: &str) -> Result<PatchesChainResponse, CatalogError> {
    quick_xml::de::from_str(xml).map_err(|e| CatalogError::PatchesChain { detail: e.to_string() })
}

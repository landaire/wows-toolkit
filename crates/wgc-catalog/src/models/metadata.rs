//! `api/v1/metadata/` response.
//!
//! The real server nests most fields under `<predefined_section>` (report A
//! sec 3.1); older captures had some of the same fields flat under
//! `<protocol>`. Only the current nesting is modeled here since that is what
//! the fixtures show; unknown/unmapped elements are ignored by quick-xml's
//! serde deserializer rather than rejected.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::CatalogError;

#[derive(Debug, Clone, Deserialize)]
pub struct MetadataResponse {
    #[serde(rename = "@name")]
    pub protocol_name: String,
    #[serde(rename = "@version")]
    pub protocol_version: String,
    /// The `<version>` element text: the metadata/catalog build stamp
    /// (distinct from the `@version` protocol/schema version above).
    ///
    /// Kept as a raw string for now (not a `MetadataVersion` newtype):
    /// nothing in this crate parses or compares it yet. `chain_id` below is
    /// deferred the same way.
    pub version: String,
    pub predefined_section: PredefinedSection,
}

impl MetadataResponse {
    pub fn app_id(&self) -> &str {
        &self.predefined_section.app_id.id
    }

    pub fn chain_id(&self) -> &str {
        &self.predefined_section.chain_id
    }

    pub fn metadata_version(&self) -> &str {
        &self.version
    }

    pub fn default_language(&self) -> &str {
        &self.predefined_section.default_language
    }

    /// Whitespace-split `--safe`-style args from the first `<executable>`.
    ///
    /// An empty result means either no executable element was present, or
    /// its `@args` attribute was absent; both are legitimate server
    /// responses, not parse failures.
    pub fn safe_mode_args(&self) -> Vec<&str> {
        self.predefined_section
            .executables
            .executable
            .first()
            .and_then(|e| e.args.as_deref())
            .map(|args| args.split_whitespace().collect())
            .unwrap_or_default()
    }

    /// `launch_options/option[@id]` as an id -> value map.
    pub fn launch_options(&self) -> BTreeMap<&str, &str> {
        self.predefined_section.launch_options.option.iter().map(|o| (o.id.as_str(), o.value.as_str())).collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PredefinedSection {
    pub app_id: AppId,
    pub chain_id: String,
    pub default_language: String,
    pub name: String,
    pub fs_name: String,
    #[serde(default)]
    pub executables: Executables,
    #[serde(default)]
    pub launch_options: LaunchOptions,
    /// Present on test environments (e.g. PT's `"PT"`); absent on live
    /// production (observed on `WOWS.WW.PRODUCTION`), not malformed data.
    #[serde(default)]
    pub environment_name: Option<String>,
    pub client_types: ClientTypes,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppId {
    #[serde(rename = "@public")]
    pub public: bool,
    #[serde(rename = "$text")]
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Executables {
    #[serde(default, rename = "executable")]
    pub executable: Vec<Executable>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Executable {
    /// Absent on some real responses (observed live on `WOWS.WW.PRODUCTION`):
    /// legitimately "no launch args for this executable", not malformed data.
    #[serde(default, rename = "@args")]
    pub args: Option<String>,
    #[serde(rename = "$text")]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LaunchOptions {
    #[serde(default, rename = "option")]
    pub option: Vec<LaunchOption>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LaunchOption {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "$text")]
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientTypes {
    #[serde(rename = "@default")]
    pub default: String,
    #[serde(default, rename = "client_type")]
    pub client_type: Vec<ClientType>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientType {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@arch")]
    pub arch: String,
    #[serde(rename = "@aliases")]
    pub aliases: Option<String>,
}

/// Parse an `api/v1/metadata/` XML response body.
pub fn parse_metadata(xml: &str) -> Result<MetadataResponse, CatalogError> {
    quick_xml::de::from_str(xml).map_err(|source| CatalogError::Metadata { source })
}

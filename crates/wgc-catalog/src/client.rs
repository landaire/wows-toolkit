//! Async HTTP client for the WGC update-service (WGUS) catalog API.
//!
//! Unauthenticated GETs only (report A sec 3-4): `metadata` and
//! `patches_chain`. `integrity_check` (api/v2) is not modeled yet.

use crate::error::CatalogError;
use crate::models::BuildVersion;
use crate::models::MetadataResponse;
use crate::models::PatchesChainResponse;
use crate::models::parse_metadata;
use crate::models::parse_patches_chain;

/// WGC's own user-agent string, so WGUS sees the same client identity a real
/// Game Center install would send.
const USER_AGENT: &str = "WargamingGameCenter/26.04.01.3190";

/// Fallback `lang` query value when no install-specific language is known.
/// Real captures (report A sec 3-4; the ledger's validated `patches_chain`
/// capture) show WGUS wants an upper-cased region code (e.g. `EN`), matching
/// `game_info.xml`'s `localization` field, which callers should pass through
/// instead of relying on this default.
const DEFAULT_LANG: &str = "EN";

const PROTOCOL_VERSION: &str = "1.11";
const METADATA_PROTOCOL_VERSION: &str = "7.10";
const CLIENT_TYPE: &str = "high";

/// The current on-disk version of each WOWS part, sent so WGUS can compute a
/// delta chain. `"0"` for a part asks WGUS for its full newest chain instead
/// of a delta from a specific version; this is WGUS's own protocol
/// convention (report A sec 3-4), not an internal sentinel this crate
/// invented.
///
/// Scoped to WoWS's four parts (client/locale/sdcontent/hotfix); other WG
/// titles have different parts (`PartName` also models hdcontent/udsound/tm/
/// wwlocale). Extend or replace this type if this crate ever targets those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentPartVersions {
    pub client: BuildVersion,
    pub locale: BuildVersion,
    pub sdcontent: BuildVersion,
    pub hotfix: BuildVersion,
}

impl CurrentPartVersions {
    /// Ask WGUS for the full newest chain for every part.
    pub fn full() -> CurrentPartVersions {
        let zero = || BuildVersion::new("0".to_string());
        CurrentPartVersions { client: zero(), locale: zero(), sdcontent: zero(), hotfix: zero() }
    }
}

pub struct CatalogClient {
    http: reqwest::Client,
}

impl CatalogClient {
    /// Build a client with no proxy.
    pub fn new() -> Result<CatalogClient, CatalogError> {
        CatalogClient::with_proxy(None)
    }

    /// Build a client, optionally routed through `proxy_url` (e.g. the
    /// toolkit's resolved `ProxyConfig::url`). Kept independent of
    /// `wows-toolkit`'s `ProxyConfig` type so this crate has no dependency
    /// back on the app crate; callers translate their own proxy config into
    /// a URL string.
    pub fn with_proxy(proxy_url: Option<&str>) -> Result<CatalogClient, CatalogError> {
        let mut builder = reqwest::Client::builder().user_agent(USER_AGENT);
        if let Some(proxy_url) = proxy_url {
            let proxy = reqwest::Proxy::all(proxy_url).map_err(|source| CatalogError::ClientBuild { source })?;
            builder = builder.proxy(proxy);
        }
        let http = builder.build().map_err(|source| CatalogError::ClientBuild { source })?;
        Ok(CatalogClient { http })
    }

    /// Wrap an already-built `reqwest::Client` (e.g. the toolkit's shared
    /// async client), so callers that already manage a client's lifetime,
    /// connection pool, and TLS session cache do not need a second one.
    pub fn with_client(http: reqwest::Client) -> CatalogClient {
        CatalogClient { http }
    }

    /// `GET {base}/api/v1/metadata/?guid={game_id}&chain_id={chain_id}&protocol_version=7.10`
    pub async fn fetch_metadata(
        &self,
        base: &url::Url,
        game_id: &str,
        chain_id: &str,
    ) -> Result<MetadataResponse, CatalogError> {
        let url =
            base.join("api/v1/metadata/").map_err(|e| CatalogError::BadUrl { base: base.to_string(), source: e })?;
        let query = [("guid", game_id), ("chain_id", chain_id), ("protocol_version", METADATA_PROTOCOL_VERSION)];
        let body = self.get_body(url, &query).await?;
        parse_metadata(&body)
    }

    /// `GET {base}/api/v1/patches_chain/?...` with `*_current_version` set
    /// from `current` (send `"0"` per part for the full newest chain).
    ///
    /// `lang` should be the install's `game_info.xml` `localization` value
    /// (e.g. `"EN"`); pass `None` to fall back to [`DEFAULT_LANG`].
    pub async fn fetch_patches_chain(
        &self,
        base: &url::Url,
        game_id: &str,
        chain_id: &str,
        metadata_version: &str,
        lang: Option<&str>,
        current: &CurrentPartVersions,
    ) -> Result<PatchesChainResponse, CatalogError> {
        let url = base
            .join("api/v1/patches_chain/")
            .map_err(|e| CatalogError::BadUrl { base: base.to_string(), source: e })?;
        let query = [
            ("protocol_version", PROTOCOL_VERSION),
            ("metadata_protocol_version", METADATA_PROTOCOL_VERSION),
            ("game_id", game_id),
            ("client_type", CLIENT_TYPE),
            ("chain_id", chain_id),
            ("metadata_version", metadata_version),
            ("installation_id", "0"),
            ("lang", lang.unwrap_or(DEFAULT_LANG)),
            ("client_current_version", current.client.as_str()),
            ("locale_current_version", current.locale.as_str()),
            ("sdcontent_current_version", current.sdcontent.as_str()),
            ("hotfix_current_version", current.hotfix.as_str()),
        ];
        let body = self.get_body(url, &query).await?;
        parse_patches_chain(&body)
    }

    /// GET `url` with `query`, returning the response body decoded as UTF-8.
    /// Non-2xx status and an empty body are both `CatalogError`s: WGUS never
    /// legitimately answers a catalog GET with an empty 2xx body.
    async fn get_body(&self, url: url::Url, query: &[(&str, &str)]) -> Result<String, CatalogError> {
        let url_str = url.to_string();
        let response = self
            .http
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|source| CatalogError::Request { url: url_str.clone(), source })?;

        let status = response.status();
        if !status.is_success() {
            // Best-effort: WGUS error responses (maintenance, rate limit) carry
            // a body worth surfacing, but a body read can itself fail.
            let body = response.text().await.unwrap_or_default();
            return Err(CatalogError::Http { url: url_str, status: status.as_u16(), body });
        }

        let bytes = response.bytes().await.map_err(|source| CatalogError::Request { url: url_str.clone(), source })?;
        // Decoded explicitly rather than via `Response::text()`, which sniffs
        // charset from headers/content: WGUS's XML is always UTF-8, and a
        // malformed body should surface as this crate's own error, not an
        // implicit lossy replacement.
        let body =
            String::from_utf8(bytes.to_vec()).map_err(|source| CatalogError::Utf8 { url: url_str.clone(), source })?;

        if body.trim().is_empty() {
            return Err(CatalogError::EmptyResponse { url: url_str });
        }

        Ok(body)
    }
}

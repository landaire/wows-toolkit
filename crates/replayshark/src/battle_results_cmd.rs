//! Per-build wows-constants resolution for the `battle-results` command.
//!
//! `ConstantsFetcher::fetch` never errors on a build mismatch: it silently
//! falls back to the nearest older published build. Callers must compare the
//! returned build against the requested one to detect an approximate match.

use std::collections::HashMap;

pub enum ConstantsResolution {
    Exact(serde_json::Value),
    Approximate { data: serde_json::Value, resolved_build: u32 },
}

#[derive(thiserror::Error, Debug)]
pub enum ConstantsError {
    #[error("no constants published upstream for build {requested} or any older build")]
    Unresolved { requested: u32 },
    #[error("failed to initialize constants fetcher: {0}")]
    Init(String),
}

pub struct ConstantsResolver {
    fetcher: wows_data_mgr::constants::ConstantsFetcher,
    memo: HashMap<u32, ConstantsResolution>,
}

impl ConstantsResolver {
    pub fn new() -> Result<Self, ConstantsError> {
        let fetcher =
            wows_data_mgr::constants::ConstantsFetcher::new().map_err(|e| ConstantsError::Init(e.to_string()))?;
        Ok(Self { fetcher, memo: HashMap::new() })
    }

    /// Resolve constants for a replay build. Memoized by the requested build.
    pub fn resolve(
        &mut self,
        build: u32,
        friendly_version: Option<&str>,
    ) -> Result<&ConstantsResolution, ConstantsError> {
        if !self.memo.contains_key(&build) {
            let (data, actual) =
                self.fetcher.fetch(build, friendly_version).ok_or(ConstantsError::Unresolved { requested: build })?;
            self.memo.insert(build, classify(build, actual, data));
        }
        Ok(self.memo.get(&build).unwrap())
    }
}

/// Pure classifier: exact match vs fallback to an older published build.
pub fn classify(requested: u32, actual: u32, data: serde_json::Value) -> ConstantsResolution {
    if actual == requested {
        ConstantsResolution::Exact(data)
    } else {
        ConstantsResolution::Approximate { data, resolved_build: actual }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_exact_vs_approximate() {
        let v = serde_json::json!({"COMMON_RESULTS": []});
        assert!(matches!(classify(100, 100, v.clone()), ConstantsResolution::Exact(_)));
        match classify(100, 90, v) {
            ConstantsResolution::Approximate { resolved_build, .. } => assert_eq!(resolved_build, 90),
            _ => panic!("expected approximate"),
        }
    }
}

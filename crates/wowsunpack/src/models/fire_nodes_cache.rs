//! On-disk cache of fire-section geometry, keyed by game build.
//!
//! [`resolve_fire_sections`](crate::models::fire_nodes::resolve_fire_sections) costs
//! a full `parse_assets_bin` over a ~178 MB file and then an O(path store size)
//! walk per hull. [`FireSectionCache`] persists the result to a small JSON file
//! so that cost is paid once per hull per game build, not once per hull per
//! replay. Node positions move when a hull model is reworked between patches,
//! so the cache is keyed by build: a file written for another build is
//! discarded rather than reused.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::game_params::types::Meters;
use crate::models::fire_nodes::FireSectionGeometry;

const CACHE_FILE_NAME: &str = "fire_sections.json";

/// Fire-section geometry for every hull of one game build, persisted as JSON.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FireSectionCache {
    build: u32,
    /// Hull model path to per-node longitudinal offsets in meters.
    sections: HashMap<String, Vec<f32>>,
}

impl FireSectionCache {
    fn empty(build: u32) -> FireSectionCache {
        FireSectionCache { build, sections: HashMap::new() }
    }

    /// Load the cache for `build` from `cache_dir`, or an empty cache when the
    /// file is absent or was written for a different build.
    ///
    /// This is derived data, always recomputable from assets.bin: a missing
    /// file, unparseable JSON, and a build mismatch are all the same "cold
    /// cache" outcome to the caller, so none of them are surfaced as an error
    /// here. The caller re-resolves and re-populates the cache on a miss.
    pub fn load(cache_dir: &Path, build: u32) -> FireSectionCache {
        let path = cache_dir.join(CACHE_FILE_NAME);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "fire-section cache miss: unreadable");
                return FireSectionCache::empty(build);
            }
        };
        let cache: FireSectionCache = match serde_json::from_str(&contents) {
            Ok(cache) => cache,
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "fire-section cache miss: unparseable");
                return FireSectionCache::empty(build);
            }
        };
        if cache.build != build {
            tracing::debug!(
                cached_build = cache.build,
                requested_build = build,
                "fire-section cache miss: build changed"
            );
            return FireSectionCache::empty(build);
        }
        cache
    }

    /// Write the cache to `cache_dir`. Errors are propagated rather than
    /// swallowed: a silently failed write means the next launch re-parses
    /// assets.bin instead of hitting this cache, and the caller should be
    /// able to log it.
    pub fn save(&self, cache_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(cache_dir)?;
        let contents = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(cache_dir.join(CACHE_FILE_NAME), contents)
    }

    /// Key is the hull model path, which is stable and already unique per hull.
    pub fn get(&self, hull_model_path: &str) -> Option<FireSectionGeometry> {
        let longitudinal = self.sections.get(hull_model_path)?;
        // from_longitudinal re-validates the node count on the way back into a
        // typed FireSectionGeometry, so a hand-edited or truncated entry falls
        // back to a miss here rather than needing a second check on the raw
        // f32 list.
        FireSectionGeometry::from_longitudinal(longitudinal.iter().copied().map(Meters::from).collect())
    }

    pub fn insert(&mut self, hull_model_path: &str, geom: &FireSectionGeometry) {
        let longitudinal = geom.longitudinal().iter().map(|m| m.value()).collect();
        self.sections.insert(hull_model_path.to_string(), longitudinal);
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own directory: `std::process::id()` alone is shared
    /// by every test in this process, so two tests writing `fire_sections.json`
    /// under the same directory would race.
    fn temp_dir(label: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wt-fire-cache-{}-{label}", std::process::id()));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = temp_dir("round_trip");
        let mut cache = FireSectionCache::load(&dir, 12830008);
        assert!(cache.is_empty());

        let geom = FireSectionGeometry::from_longitudinal([93.0, 19.0, -35.0, -99.0].map(Meters::from).to_vec())
            .expect("geom");
        cache.insert("content/.../ASB028_Iowa_1945.model", &geom);
        cache.save(&dir).expect("save");

        let reloaded = FireSectionCache::load(&dir, 12830008);
        let got = reloaded.get("content/.../ASB028_Iowa_1945.model").expect("hit");
        assert_eq!(got.longitudinal(), &[93.0, 19.0, -35.0, -99.0].map(Meters::from));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cache written for another build must not be reused: node positions
    /// move when models are reworked between patches.
    #[test]
    fn a_different_build_loads_empty() {
        let dir = temp_dir("different_build");
        let mut cache = FireSectionCache::load(&dir, 12830008);
        let geom = FireSectionGeometry::from_longitudinal(vec![Meters::from(1.0)]).expect("geom");
        cache.insert("x.model", &geom);
        cache.save(&dir).expect("save");

        assert!(FireSectionCache::load(&dir, 12999999).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A corrupt cache file is a cold cache, never a panic. It is derived data
    /// and is always recomputable from the install.
    #[test]
    fn a_corrupt_cache_loads_empty() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join("fire_sections.json"), b"{not json").expect("write");
        assert!(FireSectionCache::load(&dir, 12830008).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

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

/// How the stored numbers were derived. Bump it whenever their meaning changes:
/// the model-to-meters scale in [`ShipModelDistance`](crate::game_params::types::ShipModelDistance),
/// which is a measured constant and can be revised, or the shape of what
/// [`resolve_fire_sections`](crate::models::fire_nodes::resolve_fire_sections)
/// returns. The build number catches neither, because neither moves with it.
const DERIVATION_VERSION: u32 = 1;

/// A geometry whose section count is not the one the hull has. The count is the
/// hull's `burnNodes` length and is the resolver's central invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("hull has {expected} fire sections, geometry has {found}")]
pub struct NodeCountDisagreement {
    pub expected: usize,
    pub found: usize,
}

/// Fire-section geometry for every hull of one game build, persisted as JSON.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FireSectionCache {
    version: u32,
    build: u32,
    /// Hull model path to per-node longitudinal offsets in meters.
    sections: HashMap<String, Vec<f32>>,
}

impl FireSectionCache {
    fn empty(build: u32) -> FireSectionCache {
        FireSectionCache { version: DERIVATION_VERSION, build, sections: HashMap::new() }
    }

    /// Load the cache for `build` from `cache_dir`, or an empty cache when the
    /// file is absent, was written for a different build, or was written by a
    /// different derivation.
    ///
    /// This is derived data, always recomputable from assets.bin: a missing
    /// file, unparseable JSON, a build mismatch and a derivation mismatch are
    /// all the same "cold cache" outcome to the caller, so none of them are
    /// surfaced as an error here. The caller re-resolves and re-populates the
    /// cache on a miss.
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
        if cache.version != DERIVATION_VERSION {
            tracing::debug!(
                cached_version = cache.version,
                current_version = DERIVATION_VERSION,
                "fire-section cache miss: derivation changed"
            );
            return FireSectionCache::empty(build);
        }
        cache
    }

    /// Write the cache to `cache_dir`. Errors are propagated rather than
    /// swallowed: a silently failed write means the next launch re-parses
    /// assets.bin instead of hitting this cache, and the caller should be
    /// able to log it.
    ///
    /// The write goes to a temp file in the same directory and is renamed into
    /// place, so a second process racing this one reads either the old file or
    /// the new one, never a truncated file it would have to recover from with a
    /// full assets.bin reparse.
    pub fn save(&self, cache_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(cache_dir)?;
        let contents = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let staged = cache_dir.join(format!("{CACHE_FILE_NAME}.{}.tmp", std::process::id()));
        fs::write(&staged, contents)?;
        match fs::rename(&staged, cache_dir.join(CACHE_FILE_NAME)) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&staged);
                Err(error)
            }
        }
    }

    /// Key is the hull model path, which is stable and already unique per hull.
    /// `expected_nodes` is the hull's `burnNodes` length; an entry stored under a
    /// different count is a miss, so putting this cache in front of
    /// [`resolve_fire_sections`](crate::models::fire_nodes::resolve_fire_sections)
    /// cannot route around the count check the resolver exists to enforce.
    pub fn get(&self, hull_model_path: &str, expected_nodes: usize) -> Option<FireSectionGeometry> {
        let longitudinal = self.sections.get(hull_model_path)?;
        if longitudinal.len() != expected_nodes {
            tracing::debug!(
                hull = hull_model_path,
                expected_nodes,
                cached_nodes = longitudinal.len(),
                "fire-section cache miss: node count changed"
            );
            return None;
        }
        // from_longitudinal re-validates the node count against MAX_NODES on the
        // way back into a typed FireSectionGeometry, so a hand-edited entry falls
        // back to a miss here rather than needing a second bounds check.
        FireSectionGeometry::from_longitudinal(longitudinal.iter().copied().map(Meters::from).collect())
    }

    /// `expected_nodes` is the hull's `burnNodes` length. A geometry that does not
    /// have that many sections is refused rather than stored, so the disagreement
    /// cannot reach disk and be read back later as fact.
    pub fn insert(
        &mut self,
        hull_model_path: &str,
        expected_nodes: usize,
        geom: &FireSectionGeometry,
    ) -> Result<(), NodeCountDisagreement> {
        if geom.node_count() != expected_nodes {
            return Err(NodeCountDisagreement { expected: expected_nodes, found: geom.node_count() });
        }
        let longitudinal = geom.longitudinal().iter().map(|m| m.value()).collect();
        self.sections.insert(hull_model_path.to_string(), longitudinal);
        Ok(())
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

    const IOWA: &str = "content/.../ASB028_Iowa_1945.model";

    fn iowa_geometry() -> FireSectionGeometry {
        FireSectionGeometry::from_longitudinal([93.0, 19.0, -35.0, -99.0].map(Meters::from).to_vec()).expect("geom")
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = temp_dir("round_trip");
        let mut cache = FireSectionCache::load(&dir, 12830008);
        assert!(cache.is_empty());

        cache.insert(IOWA, 4, &iowa_geometry()).expect("insert");
        cache.save(&dir).expect("save");

        let reloaded = FireSectionCache::load(&dir, 12830008);
        let got = reloaded.get(IOWA, 4).expect("hit");
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
        cache.insert("x.model", 1, &geom).expect("insert");
        cache.save(&dir).expect("save");

        assert!(FireSectionCache::load(&dir, 12999999).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The stored numbers are `raw_model_z * ShipModelDistance`'s scale, and that
    /// scale is measured rather than exact. Changing it leaves the build number
    /// where it was, so the derivation version is what invalidates the file.
    #[test]
    fn a_different_derivation_version_loads_empty() {
        let dir = temp_dir("derivation_version");
        let stale = format!(
            r#"{{"version":{},"build":12830008,"sections":{{"x.model":[1.0]}}}}"#,
            DERIVATION_VERSION.wrapping_add(1)
        );
        std::fs::write(dir.join(CACHE_FILE_NAME), stale).expect("write");

        assert!(FireSectionCache::load(&dir, 12830008).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The resolver's whole invariant is that a hull's section count equals its
    /// `burnNodes` length. A cache that cannot express it would let a caller put
    /// the cache in front of the resolver and lose the check.
    #[test]
    fn a_geometry_is_only_returned_for_the_node_count_it_was_stored_under() {
        let dir = temp_dir("node_count");
        let mut cache = FireSectionCache::load(&dir, 12830008);
        cache.insert(IOWA, 4, &iowa_geometry()).expect("insert");

        assert!(cache.get(IOWA, 4).is_some());
        assert!(cache.get(IOWA, 2).is_none());
        assert!(cache.get(IOWA, 1).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Inserting a geometry the hull cannot have is rejected at the door, so the
    /// disagreement cannot reach disk and be read back as fact.
    #[test]
    fn inserting_a_disagreeing_node_count_is_an_error() {
        let dir = temp_dir("insert_disagree");
        let mut cache = FireSectionCache::load(&dir, 12830008);

        let err = cache.insert(IOWA, 2, &iowa_geometry());
        assert_eq!(err, Err(NodeCountDisagreement { expected: 2, found: 4 }));
        assert!(cache.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The cache file is replaced by a rename, so a reader never sees a partial
    /// write, and the temp file it renames from does not survive the save.
    #[test]
    fn saving_leaves_no_partial_file_behind() {
        let dir = temp_dir("atomic_save");
        let mut cache = FireSectionCache::load(&dir, 12830008);
        cache.insert(IOWA, 4, &iowa_geometry()).expect("insert");
        cache.save(&dir).expect("save");
        cache.save(&dir).expect("overwrite an existing cache");

        let names: Vec<String> = std::fs::read_dir(&dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![CACHE_FILE_NAME.to_string()], "stray files: {names:?}");
        assert!(FireSectionCache::load(&dir, 12830008).get(IOWA, 4).is_some());
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

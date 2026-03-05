//! Cap layout cache: extraction from replays and persistent storage.
//!
//! Capture point positions are server-side only (the `scenario_templates` XML
//! files are never shipped to the client). We build a local cache by extracting
//! cap data from InteractiveZone EntityCreate packets in replay files. The
//! cache is keyed by `(mapId, scenarioConfigId)` and persisted to disk with
//! rkyv for fast load/save.
//!
//! ## Forward compatibility
//!
//! All rkyv-serialized structs use **append-only field ordering** — new fields
//! may only be added at the end, never reordered or removed. The database is
//! wrapped in [`Versioned<T>`] which serializes the inner value out-of-line via
//! `AsBox`, allowing newer versions (with appended fields) to be read by older
//! code. A file-level version constant provides a final safety net: if
//! validation fails the cache is simply discarded and rebuilt from replays.

use std::collections::HashMap;
use std::path::Path;

use tracing::warn;
use wows_replays::ReplayFile;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::battle_controller::state::CapturePointState;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Parser;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_types::ControlPointType;
use wowsunpack::game_types::WorldPos2D;

/// Bump this whenever the on-disk schema changes in a way that cannot be
/// handled by the append-only backwards-compat strategy.
///
/// v2: switched to 8-byte aligned header to fix rkyv deserialization failures.
const CAP_LAYOUT_DB_VERSION: u32 = 2;

/// File header size in bytes. Must be a multiple of 8 so the rkyv payload
/// starts at the alignment required by archived types.
const HEADER_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Key for the cap layout cache.
#[derive(Clone, Debug, Hash, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(Hash, Eq, PartialEq))]
pub struct CapLayoutKey {
    pub map_id: u32,
    pub scenario_config_id: u32,
}

/// A single capture point's static layout.
///
/// Uses typed units from `wowsunpack`: [`WorldPos2D`] for the position in
/// BigWorld coordinates and [`BigWorldDistance`] for the zone radius.
///
/// **Append-only field ordering** for rkyv backwards compatibility.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CapPointLayout {
    /// Cap point index: A=0, B=1, C=2, …
    pub index: usize,
    /// World-space position (X/Z plane, BigWorld units).
    pub position: WorldPos2D,
    /// Zone radius in BigWorld units.
    pub radius: BigWorldDistance,
    /// Control point sub-type (domination, base, epicenter, etc.).
    pub cp_type: ControlPointType,
    /// Team that owns this cap at start. -1 = neutral.
    pub team_id: i64,
    /// Whether this cap is enabled at match start (arms race caps start disabled).
    pub initially_enabled: bool,
    // Future fields go here (append-only).
}

/// A full cap layout for one (map, mode) combination.
///
/// **Append-only field ordering** for rkyv backwards compatibility.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CapLayout {
    pub key: CapLayoutKey,
    /// Map space name, e.g. `"spaces/16_OC_bees_to_honey"`.
    pub map_name: String,
    /// Scenario identifier, e.g. `"armsrace"`, `"Domination"`.
    pub scenario: String,
    /// Numeric game mode (7=DOMINATION, 12=EPICENTER, 15=ARMS_RACE, …).
    pub game_mode: u32,
    /// Capture points in this layout.
    pub points: Vec<CapPointLayout>,
    // Future fields go here (append-only).
}

/// Persistent cache of all known cap layouts.
///
/// **Append-only field ordering** for rkyv backwards compatibility.
#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CapLayoutDb {
    pub layouts: HashMap<CapLayoutKey, CapLayout>,
    // Future fields go here (append-only).
}

/// Wrapper that serializes `T` out-of-line via `AsBox` so that newer versions
/// (with appended fields) can be read by older code that only knows the earlier
/// fields. See the rkyv `backwards_compat.rs` example.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[repr(transparent)]
struct Versioned<T>(#[rkyv(with = rkyv::with::AsBox)] pub T);

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Extract a [`CapLayout`] from an already-parsed controller's capture point
/// state. Returns `None` if there are no capture points.
pub fn extract_cap_layout_from_controller(
    meta: &ReplayMeta,
    capture_points: &[CapturePointState],
) -> Option<CapLayout> {
    if capture_points.is_empty() {
        return None;
    }

    // Only include points that actually have a position (they should all have
    // one after EntityCreate, but be defensive).
    let points: Vec<CapPointLayout> = capture_points
        .iter()
        .filter_map(|cp| {
            let pos = cp.position?;
            let cp_type =
                cp.control_point_type.as_ref().and_then(|r| r.known().copied()).unwrap_or(ControlPointType::Control);

            Some(CapPointLayout {
                index: cp.index,
                position: WorldPos2D { x: pos.x, z: pos.z },
                radius: BigWorldDistance::from(cp.radius),
                cp_type,
                team_id: cp.team_id,
                initially_enabled: cp.is_enabled,
            })
        })
        .collect();

    if points.is_empty() {
        return None;
    }

    Some(CapLayout {
        key: CapLayoutKey { map_id: meta.mapId, scenario_config_id: meta.scenarioConfigId },
        map_name: meta.mapName.clone(),
        scenario: meta.scenario.clone(),
        game_mode: meta.gameMode,
        points,
    })
}

/// Do a lightweight replay parse (first packets until `clock > 0`) to extract
/// only the cap layout. Returns `None` on any error or if the replay has no
/// capture points.
pub fn extract_cap_layout_from_replay<G: ResourceLoader>(
    path: &Path,
    resource_loader: &G,
    game_constants: Option<&GameConstants>,
) -> Option<CapLayout> {
    let replay_file = match ReplayFile::from_file(path) {
        Ok(f) => f,
        Err(e) => {
            warn!("failed to read replay {}: {e}", path.display());
            return None;
        }
    };

    let mut controller = BattleController::new(&replay_file.meta, resource_loader, game_constants);
    controller.set_track_shots(false);

    let mut parser = Parser::new(resource_loader.entity_specs());
    let mut remaining = replay_file.packet_data.as_slice();

    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => {
                // Stop once we've moved past the initial entity creation burst
                if packet.clock.0 > 0.0 {
                    break;
                }
                controller.process(&packet);
            }
            Err(_) => break,
        }
    }

    extract_cap_layout_from_controller(&replay_file.meta, controller.capture_points())
}

// ---------------------------------------------------------------------------
// CapLayoutDb methods
// ---------------------------------------------------------------------------

impl CapLayoutDb {
    /// Check if a layout for this key is already cached.
    pub fn contains(&self, key: &CapLayoutKey) -> bool {
        self.layouts.contains_key(key)
    }

    /// Insert a layout. Returns `true` if it was newly inserted.
    ///
    /// Deduplicates by checking both the key AND the actual cap positions:
    /// two layouts on the same map with identical cap positions (even under
    /// different `scenario_config_id` values) are considered duplicates.
    pub fn insert(&mut self, layout: CapLayout) -> bool {
        // Already have this exact key?
        if self.layouts.contains_key(&layout.key) {
            return false;
        }
        // Check if any existing layout on the same map has identical cap positions.
        let duplicate_positions = self
            .layouts
            .values()
            .any(|existing| existing.key.map_id == layout.key.map_id && layouts_have_same_caps(existing, &layout));
        if duplicate_positions {
            return false;
        }
        self.layouts.insert(layout.key.clone(), layout);
        true
    }

    /// Get a layout by key.
    pub fn get(&self, key: &CapLayoutKey) -> Option<&CapLayout> {
        self.layouts.get(key)
    }

    /// Return all unique maps as `(map_id, map_name)` pairs, sorted by name.
    pub fn maps(&self) -> Vec<(u32, String)> {
        let mut seen = HashMap::<u32, String>::new();
        for layout in self.layouts.values() {
            seen.entry(layout.key.map_id).or_insert_with(|| layout.map_name.clone());
        }
        let mut maps: Vec<_> = seen.into_iter().collect();
        maps.sort_by(|a, b| a.1.cmp(&b.1));
        maps
    }

    /// Return all known layouts for a given map, sorted by game mode.
    pub fn modes_for_map(&self, map_id: u32) -> Vec<&CapLayout> {
        let mut layouts: Vec<_> = self.layouts.values().filter(|l| l.key.map_id == map_id).collect();
        layouts.sort_by_key(|l| l.game_mode);
        layouts
    }

    /// Total number of cached layouts.
    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    /// Remove layouts that duplicate another layout on the same map
    /// (same cap positions and radii, different `scenario_config_id`).
    /// Returns the number of duplicates removed.
    pub fn dedup(&mut self) -> usize {
        let keys: Vec<CapLayoutKey> = self.layouts.keys().cloned().collect();
        let mut to_remove = Vec::new();

        for (i, key_a) in keys.iter().enumerate() {
            if to_remove.contains(key_a) {
                continue;
            }
            let layout_a = &self.layouts[key_a];
            for key_b in &keys[i + 1..] {
                if to_remove.contains(key_b) {
                    continue;
                }
                let layout_b = &self.layouts[key_b];
                if layout_a.key.map_id == layout_b.key.map_id && layouts_have_same_caps(layout_a, layout_b) {
                    to_remove.push(key_b.clone());
                }
            }
        }

        for key in &to_remove {
            self.layouts.remove(key);
        }
        to_remove.len()
    }
}

/// Two layouts have the "same caps" if they have the same number of points
/// and every point matches on position (bitwise) and radius.
fn layouts_have_same_caps(a: &CapLayout, b: &CapLayout) -> bool {
    if a.points.len() != b.points.len() {
        return false;
    }
    // Sort by index so ordering doesn't matter.
    let mut a_pts: Vec<_> = a.points.iter().collect();
    let mut b_pts: Vec<_> = b.points.iter().collect();
    a_pts.sort_by_key(|p| p.index);
    b_pts.sort_by_key(|p| p.index);

    a_pts.iter().zip(b_pts.iter()).all(|(pa, pb)| {
        pa.position.x.to_bits() == pb.position.x.to_bits()
            && pa.position.z.to_bits() == pb.position.z.to_bits()
            && pa.radius == pb.radius
    })
}

// ---------------------------------------------------------------------------
// Persistence (rkyv)
// ---------------------------------------------------------------------------

/// Return the on-disk path for the cap layout cache, or `None` if no storage
/// directory is available.
pub fn cache_path() -> Option<std::path::PathBuf> {
    eframe::storage_dir(crate::APP_NAME).map(|d| d.join("cap_layouts.bin"))
}

impl CapLayoutDb {
    /// Load the cap layout database from disk. Returns `None` if the file
    /// doesn't exist, is corrupt, or has an incompatible version (in which
    /// case the caller should rebuild from replays).
    pub fn load(path: &Path) -> Option<Self> {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => return None,
        };

        // File format (v2+): [u32 version LE] [4 bytes padding] [rkyv payload]
        // The 8-byte header ensures the rkyv payload is 8-byte aligned.
        if data.len() < HEADER_SIZE {
            warn!("cap layout db too small, discarding");
            return None;
        }

        let file_version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let payload = &data[HEADER_SIZE..];

        if file_version >= CAP_LAYOUT_DB_VERSION {
            match rkyv::from_bytes::<Versioned<CapLayoutDb>, rkyv::rancor::Error>(payload) {
                Ok(versioned) => return Some(versioned.0),
                Err(e) => warn!("cap layout db v{file_version} deserialization failed: {e}, discarding"),
            }
        } else {
            // Old v1 files used a 4-byte header that broke rkyv alignment.
            // Discard and rebuild from replays.
            warn!("cap layout db version {file_version} < current {CAP_LAYOUT_DB_VERSION}, discarding");
        }

        None
    }

    /// Save the cap layout database to disk.
    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let versioned = Versioned(self.clone());
        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&versioned).map_err(|e| format!("{e}"))?;

        // Header: [u32 version LE] [u32 padding]  — 8 bytes total so the rkyv
        // payload starts at 8-byte alignment (required by archived types).
        let mut data = Vec::with_capacity(HEADER_SIZE + payload.len());
        data.extend_from_slice(&CAP_LAYOUT_DB_VERSION.to_le_bytes());
        data.extend_from_slice(&[0u8; 4]); // padding
        data.extend_from_slice(&payload);

        // Write atomically via temp file + rename.
        let dir = path.parent().unwrap_or(Path::new("."));
        let tmp = tempfile::NamedTempFile::new_in(dir)?;
        std::fs::write(tmp.path(), &data)?;
        tmp.persist(path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowsunpack::game_types::ControlPointType;

    fn sample_db() -> CapLayoutDb {
        let mut db = CapLayoutDb::default();
        db.insert(CapLayout {
            key: CapLayoutKey { map_id: 45, scenario_config_id: 100 },
            map_name: "spaces/45_Zigzag".to_string(),
            scenario: "Domination".to_string(),
            game_mode: 7,
            points: vec![CapPointLayout {
                index: 0,
                position: WorldPos2D { x: 100.0, z: 200.0 },
                radius: BigWorldDistance::from(75.0),
                cp_type: ControlPointType::Control,
                team_id: -1,
                initially_enabled: true,
            }],
        });
        db
    }

    #[test]
    fn raw_rkyv_roundtrip_without_versioned() {
        let db = sample_db();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&db).unwrap();
        match rkyv::from_bytes::<CapLayoutDb, rkyv::rancor::Error>(&bytes) {
            Ok(loaded) => assert_eq!(loaded.len(), 1),
            Err(e) => panic!("direct roundtrip failed: {e}"),
        }
    }

    #[test]
    fn raw_rkyv_roundtrip_with_versioned() {
        let db = sample_db();
        let versioned = Versioned(db);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&versioned).unwrap();
        match rkyv::from_bytes::<Versioned<CapLayoutDb>, rkyv::rancor::Error>(&bytes) {
            Ok(loaded) => assert_eq!(loaded.0.len(), 1),
            Err(e) => panic!("versioned roundtrip failed: {e}"),
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let db = sample_db();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap_layouts.bin");

        db.save(&path).expect("save failed");
        let loaded = CapLayoutDb::load(&path).expect("load returned None");

        assert_eq!(loaded.len(), db.len());
        let key = CapLayoutKey { map_id: 45, scenario_config_id: 100 };
        assert!(loaded.contains(&key));
    }

    #[test]
    fn load_actual_cache_file() {
        // Attempt to load the real on-disk cache, save it, and verify roundtrip.
        // The on-disk file may be an old version that gets discarded — in that
        // case we just verify load returns None gracefully (no panic).
        if let Some(path) = cache_path() {
            if path.exists() {
                match CapLayoutDb::load(&path) {
                    Some(db) => {
                        eprintln!("loaded {} cap layouts from {}", db.len(), path.display());

                        // Roundtrip: save and reload
                        let dir = tempfile::tempdir().unwrap();
                        let tmp_path = dir.path().join("cap_layouts.bin");
                        db.save(&tmp_path).expect("save failed");
                        let reloaded = CapLayoutDb::load(&tmp_path).expect("roundtrip load failed");
                        assert_eq!(reloaded.len(), db.len());
                    }
                    None => {
                        eprintln!("on-disk cache at {} is outdated or corrupt, skipping", path.display());
                    }
                }
            }
        }
    }
}

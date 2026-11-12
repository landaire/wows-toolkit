//! Reading the replays for the capture layouts of the modes they were played in.
//!
//! The layouts are what the mode picker offers, and the only place they can be read from is a
//! battle that used them. Each is written once: a machine with a year of replays pays for this
//! walk once and never again.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_replays::game_constants::GameConstants;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::tactics::ScanProgress;

/// A walk the board has asked for, to run off the UI thread.
pub struct ScanJob {
    pub(super) dir: PathBuf,
    pub(super) provider: Arc<GameMetadataProvider>,
    pub(super) constants: GameConstants,
    /// What the board already held when the walk was asked for.
    pub(super) known: CapLayoutDb,
    pub(super) cancelled: Arc<AtomicBool>,
}

/// What a finished walk found.
pub struct ScanOutcome {
    /// Every layout known, the board's and the walk's.
    pub found: CapLayoutDb,
    /// The layouts nothing had before, which are what is written to the store.
    pub fresh: Vec<CapLayout>,
    /// How many replays were walked.
    pub total: usize,
}

impl ScanJob {
    /// Reads every replay, reporting as it goes. Stops early once the board has gone.
    pub fn run(self, mut report: impl FnMut(ScanProgress)) -> ScanOutcome {
        let mut found = self.known;
        let files = replay_files(&self.dir);
        let total = files.len();
        report(ScanProgress { read: 0, total: Some(total) });
        let mut fresh = Vec::new();
        for (read, path) in files.iter().enumerate() {
            if self.cancelled.load(Ordering::Relaxed) {
                break;
            }
            if let Some(layout) = layout_of(path, &self.provider, &self.constants, &found) {
                let kept = layout.clone();
                if found.insert(layout) {
                    fresh.push(kept);
                }
            }
            report(ScanProgress { read: read + 1, total: Some(total) });
        }
        ScanOutcome { found, fresh, total }
    }
}

/// Every replay under `dir`, in a stable order.
///
/// Symbolic links are not followed: a directory the reader picked may link back into its own
/// ancestry, which would walk for ever. A subdirectory that cannot be read is skipped rather than
/// losing the rest.
pub fn replay_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    // Depth first, in name order, without recursing: the depth of the tree is the reader's.
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        let mut subdirs = Vec::new();
        for entry in entries {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                subdirs.push(path);
                continue;
            }
            // The battle in progress has no results and no layout to read; it is also being
            // written to while this reads it.
            if path.extension().is_some_and(|ext| ext == "wowsreplay")
                && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
            {
                found.push(path);
            }
        }
        pending.extend(subdirs.into_iter().rev());
    }
    found
}

/// The capture layout `path` was played on, when it is one nothing has yet.
///
/// The replay's own metadata says which layout it used, so a battle on a layout already recorded
/// is not parsed at all: that is the difference between a walk that takes seconds and one that
/// takes an hour.
fn layout_of(
    path: &Path,
    provider: &GameMetadataProvider,
    constants: &GameConstants,
    known: &CapLayoutDb,
) -> Option<CapLayout> {
    let replay = wows_replays::ReplayFile::from_file(path).ok()?;
    let key = CapLayoutKey { map_id: replay.meta.mapId, scenario_config_id: replay.meta.scenarioConfigId };
    if known.contains(&key) {
        return None;
    }
    wows_replay_insights::cap_layout::extract_cap_layout_from_replay(path, provider, Some(constants))
}

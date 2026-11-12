//! Work over a whole listing: indexing, forced re-indexing, contributing, and the palette
//! commands that start it.

use std::path::Path;

use wows_toolkit_replay_data::force_reindex::Confirm;
use wows_toolkit_replay_data::force_reindex::ReindexCandidate;

use crate::tabs::replays::browser::folders::BrowserControl;
use crate::tabs::replays::browser::folders::FolderId;

/// Offers the listing's commands on the palette for as long as the browser is up.
pub fn offer(_control: BrowserControl) {}

/// Reads `candidates` again and rewrites their index entries, once confirmed as `confirm` asks.
pub fn force_reindex(_control: BrowserControl, _candidates: Vec<ReindexCandidate>, _confirm: Confirm) {}

/// Indexes what a chosen directory's own source lacks, once its tab has found the directory.
pub fn index_folder(_control: BrowserControl, _id: FolderId) {}

/// Stops the closed tab's index pass.
pub fn folder_closed(_control: BrowserControl, _id: FolderId) {}

/// Indexes a replay that landed in a watched directory.
pub fn replay_landed(_control: BrowserControl, _id: FolderId, _path: &Path) {}

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::index::rows::RowSummary;
use crate::db::index::rows::WorkspaceId;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::ReplayTab;

/// One open replay listing: a directory of `.wowsreplay` files, the parsed
/// replays and index-sourced summaries backing its listing UI, and the dock
/// of open replay-viewer tabs.
pub(crate) struct ReplayWorkspace {
    /// The directory this workspace lists. `None` before a WoWs directory has
    /// been configured.
    pub root: Option<PathBuf>,
    pub replay_files: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,
    /// Index-sourced display data for the listing, keyed by replay path.
    /// Reloaded whenever `index_generation()` moves past
    /// `replay_row_summaries_generation`.
    pub replay_row_summaries: HashMap<PathBuf, RowSummary>,
    /// The `index_generation()` value of the most recent load *attempt*, stamped
    /// when the task is dispatched rather than when it lands. A load that errors
    /// therefore waits for the index to move again instead of re-dispatching on
    /// every frame. `None` before the first attempt.
    pub replay_row_summaries_generation: Option<u64>,
    /// True while a summary load is in flight, so a slow query cannot spawn a
    /// second task on the next frame.
    pub replay_row_summaries_loading: bool,
    /// True once a load has landed successfully. The listing's panel auto-size
    /// waits on this: measuring before any summary exists would fit the panel to
    /// the "not indexed" placeholder and latch that width for the session.
    pub replay_row_summaries_loaded: bool,
    /// Set when a summary load lands, consumed by the listing to run one
    /// freshness scan over the listed files.
    pub replay_rows_need_reindex_scan: bool,
    /// Paths already handed to the background parser for re-indexing this
    /// session. Prevents a file the parser cannot fix from being re-queued on
    /// every summary reload.
    pub replay_rows_reindex_requested: HashSet<PathBuf>,
    pub replay_dock_state: egui_dock::DockState<ReplayTab>,
    pub next_replay_tab_id: u64,
    /// Whether the replay listing panel has been auto-sized to fit content.
    /// Reset when game state is cleared so the panel re-auto-sizes on next load.
    pub replay_listing_auto_sized: bool,
    /// Whether large grouped listings have had their default collapse applied.
    /// Reset when game state is cleared so the collapse re-applies on next load.
    pub replay_listing_collapse_defaulted: bool,
}

impl ReplayWorkspace {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            replay_files: None,
            replay_row_summaries: HashMap::new(),
            replay_row_summaries_generation: None,
            replay_row_summaries_loading: false,
            replay_row_summaries_loaded: false,
            replay_rows_need_reindex_scan: false,
            replay_rows_reindex_requested: HashSet::new(),
            replay_dock_state: egui_dock::DockState::new(vec![]),
            next_replay_tab_id: 0,
            replay_listing_auto_sized: false,
            replay_listing_collapse_defaulted: false,
        }
    }

    /// Returns the replay shown in the currently focused (or first) replay dock tab, if any.
    pub fn focused_replay(&self) -> Option<Arc<RwLock<Replay>>> {
        // Try focused leaf first
        if let Some(path) = self.replay_dock_state.focused_leaf()
            && let Some(leaf) = self.replay_dock_state[path.surface][path.node].get_leaf()
            && let Some(tab) = leaf.tabs.get(leaf.active.0)
        {
            return Some(Arc::clone(&tab.replay));
        }
        // Fall back to the first tab in any leaf
        let (_, tab) = self.replay_dock_state.iter_all_tabs().next()?;
        Some(Arc::clone(&tab.replay))
    }

    /// Replace the focused tab's replay, or open a new tab if none exists.
    pub fn open_replay_in_focused_tab(&mut self, replay: Arc<RwLock<Replay>>) {
        // Try focused tab first
        if let Some((_rect, tab)) = self.replay_dock_state.find_active_focused() {
            tab.replay = replay;
            return;
        }
        // Fall back to the first tab in any leaf
        if let Some((_, tab)) = self.replay_dock_state.iter_all_tabs_mut().next() {
            tab.replay = replay;
            return;
        }
        self.open_replay_in_new_tab(replay);
    }

    /// Open a replay in a new dock tab.
    pub fn open_replay_in_new_tab(&mut self, replay: Arc<RwLock<Replay>>) {
        let id = self.next_replay_tab_id;
        self.next_replay_tab_id += 1;
        self.replay_dock_state.push_to_focused_leaf(ReplayTab { replay, id });
    }

    /// Clears this workspace's listing and dock state. Called when the WoWs
    /// directory changes to ensure no stale data from the previous directory
    /// persists. `root` is preserved: a reset is not a change of directory.
    /// `replay_row_summaries_loading` is preserved: it is owned by an
    /// in-flight background task, which clears it on completion, and clearing
    /// it here would let a second load dispatch while the first is still
    /// running.
    pub fn reset(&mut self) {
        self.replay_dock_state = egui_dock::DockState::new(vec![]);
        self.next_replay_tab_id = 0;
        self.replay_files = None;
        self.replay_row_summaries.clear();
        self.replay_row_summaries_generation = None;
        self.replay_row_summaries_loaded = false;
        self.replay_rows_need_reindex_scan = false;
        self.replay_rows_reindex_requested.clear();
        self.replay_listing_auto_sized = false;
        self.replay_listing_collapse_defaulted = false;
    }
}

/// One widget's egui id, scoped to a workspace. Every persistent widget in the
/// replay listing needs this: egui keys state by id, so two listings sharing an
/// id share scroll offsets, tree selection and open/closed state.
pub(crate) fn workspace_salt(id: WorkspaceId, name: &str) -> egui::Id {
    egui::Id::new((id.0, name))
}

/// A tree group node's id. `kind` separates the Date and Ship groupings, whose
/// labels can otherwise coincide.
pub(crate) fn workspace_group_salt(id: WorkspaceId, kind: &str, group: &str) -> egui::Id {
    egui::Id::new((id.0, kind, group))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_workspaces_get_different_ids_for_the_same_name() {
        let a = workspace_salt(WorkspaceId::LIVE, "replay_parser_dock");
        let b = workspace_salt(WorkspaceId(1), "replay_parser_dock");
        assert_ne!(a, b, "two workspaces must not share one widget's egui state");
    }

    #[test]
    fn the_same_workspace_and_name_are_stable_across_calls() {
        // egui state is looked up by id every frame, so an unstable id would
        // silently reset the widget's state on each repaint.
        assert_eq!(workspace_salt(WorkspaceId::LIVE, "x"), workspace_salt(WorkspaceId::LIVE, "x"));
    }

    #[test]
    fn different_names_in_one_workspace_do_not_collide() {
        assert_ne!(workspace_salt(WorkspaceId::LIVE, "a"), workspace_salt(WorkspaceId::LIVE, "b"));
    }

    #[test]
    fn identical_group_names_in_different_workspaces_do_not_collide() {
        // Date group labels collide across directories by construction: every
        // listing that spans a given day produces the same label.
        let a = workspace_group_salt(WorkspaceId::LIVE, "date_group", "2026-07-30");
        let b = workspace_group_salt(WorkspaceId(1), "date_group", "2026-07-30");
        assert_ne!(a, b);
    }

    #[test]
    fn group_kinds_do_not_collide_within_one_workspace() {
        let date = workspace_group_salt(WorkspaceId::LIVE, "date_group", "Yamato");
        let ship = workspace_group_salt(WorkspaceId::LIVE, "ship_group", "Yamato");
        assert_ne!(date, ship, "a ship named like a date label must not share tree state");
    }
}

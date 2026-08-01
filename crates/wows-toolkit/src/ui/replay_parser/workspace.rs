use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::index::rows::RowSummary;
use crate::db::index::rows::SourceId;
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
    /// The index source this workspace's listing reads. `None` for the live
    /// workspace, which always resolves the live source at load time because
    /// the indexer -- not this workspace -- creates it and it may not exist
    /// yet. An imported workspace sets this once its source is ensured.
    pub source: Option<SourceId>,
    pub replay_files: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,
    /// True while a directory ingest is walking this workspace's root, so
    /// re-picking the same directory cannot start a second walk over it.
    /// Owned by the in-flight task, which clears it when it finishes.
    pub ingest_in_flight: bool,
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
            source: None,
            replay_files: None,
            ingest_in_flight: false,
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
    /// `replay_row_summaries_loading` and `ingest_in_flight` are preserved:
    /// both are owned by an in-flight background task, which clears them on
    /// completion, and clearing them here would let a second task dispatch
    /// while the first is still running.
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

/// A leaf node's id. Takes the path directly: two workspaces can list the same
/// file, and a `PathBuf -> &str` conversion for `workspace_salt` is lossy.
pub(crate) fn workspace_leaf_salt(id: WorkspaceId, path: &std::path::Path) -> egui::Id {
    egui::Id::new((id.0, path))
}

/// Fish-style shorthand for a workspace root, used as a directory tab's
/// title. The drive/UNC prefix is kept, every ancestor directory is cut to
/// its first character, and the final component -- the directory the
/// workspace actually lists -- is kept in full (middle-truncated if it is
/// implausibly long). The input's separator style is preserved rather than
/// normalised, since a root picked via a folder dialog is already in the
/// platform's native form.
pub(crate) fn shorten_root(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let separator = if raw.contains('\\') { '\\' } else { '/' };

    let mut prefix = String::new();
    let mut has_root = false;
    let mut parts: Vec<String> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix_component) => {
                prefix.push_str(&prefix_component.as_os_str().to_string_lossy());
            }
            Component::RootDir => has_root = true,
            Component::CurDir | Component::ParentDir | Component::Normal(_) => {
                parts.push(component.as_os_str().to_string_lossy().into_owned());
            }
        }
    }

    if parts.is_empty() {
        return prefix;
    }

    let leaf_index = parts.len() - 1;
    let shortened = parts
        .into_iter()
        .enumerate()
        .map(|(i, part)| if i == leaf_index { shorten_leaf(&part) } else { first_char(&part) })
        .collect::<Vec<_>>()
        .join(&separator.to_string());

    if prefix.is_empty() && !has_root { shortened } else { format!("{prefix}{separator}{shortened}") }
}

fn first_char(component: &str) -> String {
    component.chars().next().map(String::from).unwrap_or_default()
}

/// The final path component is kept in full unless it is implausibly long,
/// in which case it is middle-truncated so a stray huge directory name
/// cannot blow out the tab title.
fn shorten_leaf(leaf: &str) -> String {
    const MAX_LEAF_CHARS: usize = 24;
    let chars: Vec<char> = leaf.chars().collect();
    if chars.len() <= MAX_LEAF_CHARS {
        return leaf.to_string();
    }
    let head_len = MAX_LEAF_CHARS / 2;
    let tail_len = MAX_LEAF_CHARS - head_len;
    let head: String = chars[..head_len].iter().collect();
    let tail: String = chars[chars.len() - tail_len..].iter().collect();
    format!("{head}..{tail}")
}

/// A request raised by a listing context menu and consumed by a handler later
/// in the same frame. Typed so a writer and its reader cannot name different
/// slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayRequestSlot {
    OpenReplayNewTab,
    ContextMenuRenderReplay,
    BatchRenderReplays,
    BatchRenderClipboard,
}

impl ReplayRequestSlot {
    const fn name(self) -> &'static str {
        match self {
            ReplayRequestSlot::OpenReplayNewTab => "open_replay_new_tab",
            ReplayRequestSlot::ContextMenuRenderReplay => "context_menu_render_replay",
            ReplayRequestSlot::BatchRenderReplays => "batch_render_replays",
            ReplayRequestSlot::BatchRenderClipboard => "batch_render_clipboard",
        }
    }
}

/// The egui id backing a [`ReplayRequestSlot`] in a given workspace.
pub(crate) fn request_slot_id(workspace: WorkspaceId, slot: ReplayRequestSlot) -> egui::Id {
    workspace_salt(workspace, slot.name())
}

/// The egui id backing the alt-perspective handoff. Deliberately app-wide
/// rather than workspace-scoped: the button that raises it draws later in the
/// frame than the handler that consumes it, so the consumer is whichever
/// replay tab draws next. A workspace-scoped slot would leave the request --
/// and the whole `ReplayFile` it owns -- parked under an id nobody reads. The
/// workspace that raised it travels inside the value instead.
pub(crate) fn alt_perspective_slot_id() -> egui::Id {
    egui::Id::new("alt_perspective_pending")
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

    /// A minimal but real `ReplayTab`: an empty-params `GameMetadataProvider`
    /// (no VFS needed) backing a `Replay` built from a hand-built `ReplayMeta`
    /// round-tripped through `ReplayFile::from_decrypted_parts`, the same
    /// entry point the app uses for a loaded replay's raw JSON.
    fn test_replay_tab(id: u64) -> ReplayTab {
        let meta = wows_replays::ReplayMeta {
            matchGroup: None,
            gameMode: 0,
            gameType: None,
            clientVersionFromExe: "0,0,0,0".to_string(),
            scenarioUiCategoryId: None,
            mapDisplayName: String::new(),
            mapId: 0,
            clientVersionFromXml: String::new(),
            weatherParams: None,
            duration: 0,
            gameLogic: None,
            name: String::new(),
            scenario: String::new(),
            playerID: wows_replays::types::AccountId(0),
            vehicles: Vec::new(),
            playersPerTeam: 0,
            dateTime: String::new(),
            mapName: String::new(),
            playerName: String::new(),
            scenarioConfigId: 0,
            teamsCount: 0,
            logic: None,
            playerVehicle: String::new(),
            battleDuration: None,
        };
        let meta_json = serde_json::to_vec(&meta).expect("ReplayMeta serializes");
        let replay_file = wows_replays::ReplayFile::from_decrypted_parts(meta_json, Vec::new())
            .expect("a ReplayMeta we just serialized parses back");
        let resource_loader = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
                .expect("an empty param list is always valid"),
        );
        let replay = Arc::new(RwLock::new(Replay::new(replay_file, resource_loader)));
        ReplayTab { replay, id }
    }

    #[test]
    fn reset_clears_the_listing_but_keeps_the_root_and_an_in_flight_load() {
        let mut ws = ReplayWorkspace::new(Some(PathBuf::from("replays")));
        ws.replay_files = Some(HashMap::new());
        ws.replay_dock_state.push_to_focused_leaf(test_replay_tab(0));
        assert!(ws.replay_dock_state.iter_all_tabs().next().is_some(), "the tab was actually added before reset");
        ws.replay_row_summaries.insert(
            PathBuf::from("a.wowsreplay"),
            RowSummary {
                outcome: crate::db::index::rows::MatchOutcome::Unknown,
                self_damage: None,
                self_kills: None,
                self_survived: None,
                self_pr: None,
                division_id: None,
                division_mates: Vec::new(),
                results_available: false,
                file_mtime: None,
            },
        );
        ws.replay_row_summaries_generation = Some(7);
        ws.ingest_in_flight = true;
        ws.replay_row_summaries_loading = true;
        ws.replay_row_summaries_loaded = true;
        ws.replay_rows_need_reindex_scan = true;
        ws.replay_rows_reindex_requested.insert(PathBuf::from("a.wowsreplay"));
        ws.next_replay_tab_id = 3;
        ws.replay_listing_auto_sized = true;
        ws.replay_listing_collapse_defaulted = true;

        ws.reset();

        assert_eq!(ws.root, Some(PathBuf::from("replays")));
        assert!(ws.replay_row_summaries_loading, "an in-flight load owns this flag and clears it on completion");
        assert!(ws.ingest_in_flight, "an in-flight ingest owns this flag and clears it on completion");
        assert!(ws.replay_files.is_none());
        assert!(ws.replay_row_summaries.is_empty());
        assert_eq!(ws.replay_row_summaries_generation, None);
        assert!(!ws.replay_row_summaries_loaded);
        assert!(!ws.replay_rows_need_reindex_scan);
        assert!(ws.replay_rows_reindex_requested.is_empty());
        assert_eq!(ws.next_replay_tab_id, 0);
        assert!(!ws.replay_listing_auto_sized);
        assert!(!ws.replay_listing_collapse_defaulted);
        assert!(ws.replay_dock_state.iter_all_tabs().next().is_none());
    }

    #[test]
    fn a_deep_path_abbreviates_its_ancestors() {
        assert_eq!(shorten_root(Path::new("G:/dev/wows/replays/2026-07")), "G:/d/w/r/2026-07");
    }

    #[test]
    fn a_shallow_path_is_left_alone() {
        assert_eq!(shorten_root(Path::new("D:/replays")), "D:/replays");
    }

    #[test]
    fn a_bare_directory_name_is_returned_as_is() {
        assert_eq!(shorten_root(Path::new("replays")), "replays");
    }

    #[test]
    fn an_empty_path_does_not_panic() {
        assert_eq!(shorten_root(Path::new("")), "");
    }

    #[test]
    fn a_very_long_leaf_is_middle_truncated() {
        let long = "a".repeat(60);
        let out = shorten_root(Path::new(&format!("D:/x/{long}")));
        assert_eq!(out, format!("D:/x/{}..{}", "a".repeat(12), "a".repeat(12)));
    }

    /// The head and tail keep distinct halves of the leaf rather than the
    /// same one twice, so a truncated title still discriminates siblings.
    #[test]
    fn a_truncated_leaf_keeps_both_ends() {
        let leaf = format!("{}{}", "h".repeat(20), "t".repeat(20));
        let out = shorten_root(Path::new(&format!("D:/x/{leaf}")));
        assert_eq!(out, format!("D:/x/{}..{}", "h".repeat(12), "t".repeat(12)));
    }

    /// Pins the 24-char threshold from both sides: one under is untouched,
    /// one over truncates. An off-by-one bound fails exactly one of these.
    #[test]
    fn the_leaf_truncation_threshold_is_exact() {
        let at_limit = "b".repeat(24);
        assert_eq!(shorten_root(Path::new(&format!("D:/x/{at_limit}"))), format!("D:/x/{at_limit}"));

        let over_limit = "b".repeat(25);
        assert_eq!(
            shorten_root(Path::new(&format!("D:/x/{over_limit}"))),
            format!("D:/x/{}..{}", "b".repeat(12), "b".repeat(12))
        );
    }

    #[test]
    fn backslash_separators_are_handled() {
        // Windows paths arrive backslashed from the folder picker.
        assert_eq!(shorten_root(Path::new(r"G:\dev\wows\replays")), r"G:\d\w\replays");
    }

    #[test]
    fn every_request_slot_round_trips() {
        let slots = [
            ReplayRequestSlot::OpenReplayNewTab,
            ReplayRequestSlot::ContextMenuRenderReplay,
            ReplayRequestSlot::BatchRenderReplays,
            ReplayRequestSlot::BatchRenderClipboard,
        ];

        for &slot in &slots {
            assert_eq!(
                request_slot_id(WorkspaceId::LIVE, slot),
                request_slot_id(WorkspaceId::LIVE, slot),
                "a slot's id must be stable across calls"
            );
            assert_ne!(
                request_slot_id(WorkspaceId::LIVE, slot),
                request_slot_id(WorkspaceId(1), slot),
                "the same slot in different workspaces must not collide"
            );
            for &other in &slots {
                if other != slot {
                    assert_ne!(
                        request_slot_id(WorkspaceId::LIVE, slot),
                        request_slot_id(WorkspaceId::LIVE, other),
                        "different slots in the same workspace must not collide"
                    );
                }
            }
        }
    }

    #[test]
    fn the_alt_perspective_slot_is_stable_and_workspace_independent() {
        assert_eq!(alt_perspective_slot_id(), alt_perspective_slot_id());

        let scoped = [
            ReplayRequestSlot::OpenReplayNewTab,
            ReplayRequestSlot::ContextMenuRenderReplay,
            ReplayRequestSlot::BatchRenderReplays,
            ReplayRequestSlot::BatchRenderClipboard,
        ];
        for workspace in [WorkspaceId::LIVE, WorkspaceId(1), WorkspaceId(2)] {
            for &slot in &scoped {
                assert_ne!(
                    alt_perspective_slot_id(),
                    request_slot_id(workspace, slot),
                    "the app-wide alt slot must not land on a workspace-scoped one"
                );
            }
        }
    }
}

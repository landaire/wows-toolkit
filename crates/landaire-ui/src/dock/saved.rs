//! A dock's layout as a value to store and read back.

use serde::Deserialize;
use serde::Serialize;

use crate::dock::layout::PanelEntry;
use crate::dock::layout::PanelId;
use crate::dock::layout::RegionExtents;
use crate::dock::layout::Share;
use crate::dock::layout::SkippedPanel;
use crate::dock::layout::SplitAxis;

/// Everything [`DockState::restore`](crate::DockState::restore) needs to put a dock back as it
/// was: each region's tree of splits and tab groups, which regions show, and the room they take.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub center: SavedRegion,
    pub left: SavedRegion,
    pub right: SavedRegion,
    pub bottom: SavedRegion,
    pub extents: RegionExtents,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRegion {
    pub open: bool,
    pub root: Option<SavedNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SavedNode {
    /// A tab group. One with no panels is dropped on restore.
    Tabs {
        panels: Vec<PanelEntry>,
        active: PanelId,
    },
    Split {
        axis: SplitAxis,
        parts: Vec<SavedPart>,
    },
}

impl SavedNode {
    /// Builds a value for the tree from its leaves up.
    pub(crate) fn fold<T>(
        &self,
        mut tabs: impl FnMut(&[PanelEntry], &PanelId) -> T,
        mut split: impl FnMut(SplitAxis, Vec<(Share, T)>) -> T,
    ) -> T {
        enum Visit<'a> {
            Enter(&'a SavedNode),
            Leave(SplitAxis, &'a [SavedPart]),
        }

        let mut pending = vec![Visit::Enter(self)];
        let mut built: Vec<T> = Vec::new();
        while let Some(visit) = pending.pop() {
            match visit {
                Visit::Enter(Self::Tabs { panels, active }) => built.push(tabs(panels, active)),
                Visit::Enter(Self::Split { axis, parts }) => {
                    pending.push(Visit::Leave(*axis, parts));
                    pending.extend(parts.iter().rev().map(|part| Visit::Enter(&part.node)));
                }
                Visit::Leave(axis, parts) => {
                    let children = built.split_off(built.len() - parts.len());
                    built.push(split(axis, parts.iter().map(|part| part.share).zip(children).collect()));
                }
            }
        }

        built.pop().expect("a tree folds to one value")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPart {
    pub share: Share,
    pub node: SavedNode,
}

/// What a restore left out of the layout it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreReport {
    pub skipped: Vec<SkippedPanel>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::layout::CssLength;
    use crate::dock::layout::DockRegion;
    use crate::dock::layout::DockTarget;
    use crate::dock::layout::Edge;
    use crate::dock::layout::Layout;
    use crate::dock::layout::PanelKind;
    use crate::dock::layout::RegionExtent;
    use crate::dock::layout::SkipReason;

    fn entry(id: &str, kind: &str) -> PanelEntry {
        PanelEntry::new(PanelId::new(id), PanelKind::new(kind))
    }

    fn sample() -> Layout {
        let mut layout = Layout::new();
        let overview = layout
            .add_panel(
                DockTarget::Region(DockRegion::Center),
                PanelEntry { closable: false, ..entry("overview", "summary") },
            )
            .expect("adds");
        layout.add_panel(DockTarget::Group(overview), entry("ships", "table")).expect("adds");
        layout
            .add_panel(DockTarget::Beside { group: overview, edge: Edge::Bottom }, entry("log", "log"))
            .expect("adds");
        layout.activate(&PanelId::new("overview")).expect("overview is in the dock");
        layout.add_panel(DockTarget::Region(DockRegion::Right), entry("chart-1", "chart")).expect("adds");
        layout.set_open(DockRegion::Bottom, false);
        layout.extents.right = RegionExtent::Length(CssLength::new(500.0).expect("a length"));

        layout
    }

    #[test]
    fn a_layout_is_written_as_its_tree() {
        insta::assert_yaml_snapshot!(sample().dump());
    }

    #[test]
    fn a_restored_layout_dumps_as_the_one_it_was_saved_from() {
        let saved = sample().dump();

        let (restored, skipped) = Layout::restore(&saved, |_| true);

        assert!(skipped.is_empty());
        assert_eq!(restored.dump(), saved);
    }

    #[test]
    fn a_panel_of_an_unknown_kind_is_left_out_and_reported() {
        let saved = sample().dump();

        let (restored, skipped) = Layout::restore(&saved, |kind| kind.as_str() != "table");

        assert_eq!(skipped, [SkippedPanel { entry: entry("ships", "table"), reason: SkipReason::UnknownKind }]);
        let ids: Vec<PanelId> = restored.panels().into_iter().map(|entry| entry.id).collect();
        assert_eq!(ids, [PanelId::new("overview"), PanelId::new("log"), PanelId::new("chart-1")]);
    }

    #[test]
    fn a_group_left_empty_by_a_restore_gives_its_room_to_its_sibling() {
        let saved = sample().dump();

        let (restored, skipped) = Layout::restore(&saved, |kind| kind.as_str() != "log");

        assert_eq!(skipped.len(), 1);
        let Some(SavedNode::Tabs { panels, .. }) = restored.dump().center.root else {
            panic!("the split folded into the group that was left");
        };
        assert_eq!(panels.len(), 2);
    }

    #[test]
    fn a_second_panel_with_an_id_already_restored_is_reported() {
        let mut saved = sample().dump();
        let Some(SavedNode::Tabs { panels, .. }) = saved.right.root.as_mut() else {
            panic!("the right region is one group");
        };
        panels.push(entry("log", "log"));

        let (_, skipped) = Layout::restore(&saved, |_| true);

        assert_eq!(skipped, [SkippedPanel { entry: entry("log", "log"), reason: SkipReason::DuplicateId }]);
    }
}

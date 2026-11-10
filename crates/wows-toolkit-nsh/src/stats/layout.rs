//! The Stats tab's dock layout and the settings row it is saved in.
//!
//! The row is the one the GPUI app writes (`stats_charts_dock_layout`), in
//! `gpui-base`'s format, which `nsh_base::dock` reads and writes unchanged.
//! Panels are named the way GPUI names them, so a layout arranged in one app
//! opens the same way in the other.

use nsh_base::dock::DockAreaLayout;
use nsh_base::dock::DockAreaState;
use nsh_base::dock::DockLayout;
use nsh_base::dock::DockNode;
use nsh_base::dock::DockPlacement;
use nsh_base::dock::DockRegion;
use nsh_base::dock::PanelId;
use nsh_base::dock::PanelInfo;
use nsh_base::dock::PanelState;
use nsh_core::geometry::px;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;

/// The settings row the open charts are kept in.
pub(crate) const CHARTS_SETTINGS_KEY: &str = "stats_charts";

/// The settings row the dock arrangement is kept in.
pub(crate) const CHARTS_DOCK_LAYOUT_KEY: &str = "stats_charts_dock_layout";

/// How wide a chart dock opens, as the GPUI tab opens it.
const CHART_DOCK_WIDTH: f32 = 500.0;

const OVERVIEW_PANEL_NAME: &str = "StatsOverviewPanel";
const SHIPS_PANEL_NAME: &str = "StatsShipsPanel";
const CHART_PANEL_NAME: &str = "StatsChartPanel";

/// Why a layout could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("the layout names a panel this tab does not have: {panel}")]
    UnknownPanel { panel: String },
    #[error("the layout repeats chart {id}")]
    RepeatedChart { id: usize },
}

/// One panel of the Stats tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatsPanel {
    Overview,
    Ships,
    Chart(ChartId),
}

impl StatsPanel {
    /// The key the dock area and the TSX view know the panel by.
    pub(crate) fn key(self) -> PanelId {
        match self {
            Self::Overview => PanelId::new("overview"),
            Self::Ships => PanelId::new("ships"),
            Self::Chart(id) => PanelId::new(format!("chart-{}", id.0)),
        }
    }

    pub(crate) fn parse(key: &PanelId) -> Option<Self> {
        match key.as_str() {
            "overview" => Some(Self::Overview),
            "ships" => Some(Self::Ships),
            other => other.strip_prefix("chart-")?.parse().ok().map(|id| Self::Chart(ChartId(id))),
        }
    }

    /// The persisted leaf, as GPUI's panels dump themselves.
    fn state(self) -> PanelState {
        match self {
            Self::Overview => PanelState::new(OVERVIEW_PANEL_NAME),
            Self::Ships => PanelState::new(SHIPS_PANEL_NAME),
            Self::Chart(id) => {
                let mut state = PanelState::new(CHART_PANEL_NAME);
                state.info = PanelInfo::panel(serde_json::json!({ "id": id.0 }));
                state
            }
        }
    }

    /// The panel a persisted leaf names. `None` for a leaf this tab has no
    /// panel for, or a chart leaf without its id.
    fn from_state(state: &PanelState) -> Option<Self> {
        match state.panel_name.as_str() {
            OVERVIEW_PANEL_NAME => Some(Self::Overview),
            SHIPS_PANEL_NAME => Some(Self::Ships),
            CHART_PANEL_NAME => {
                let PanelInfo::Panel(value) = &state.info else { return None };
                let id = usize::try_from(value.get("id")?.as_u64()?).ok()?;
                Some(Self::Chart(ChartId(id)))
            }
            _ => None,
        }
    }
}

/// The layout a tab opens with: Overview and Ships share the center, and
/// each chart sits in the right dock.
pub(crate) fn default_layout(charts: &[ChartId]) -> DockAreaLayout {
    let center = DockLayout::tabs().panel(StatsPanel::Overview.key()).panel(StatsPanel::Ships.key());
    let mut layout = DockAreaLayout::new(DockLayout::h_split().child(center, None));
    for id in charts {
        layout = with_chart(layout, *id);
    }

    layout
}

/// `layout` with chart `id` added to the right dock, which is made when
/// there is none and opened when it is closed: where GPUI puts a new chart.
pub(crate) fn with_chart(mut layout: DockAreaLayout, id: ChartId) -> DockAreaLayout {
    let key = StatsPanel::Chart(id).key();
    if panels(&layout).contains(&key) {
        return layout;
    }

    let Some(region) = layout.right.as_mut() else {
        let mut region = DockRegion::new(DockLayout::tabs().panel(key));
        region.size = Some(px(CHART_DOCK_WIDTH));
        region.open = Some(true);
        return layout.with_dock(DockPlacement::Right, region);
    };

    region.open = Some(true);
    let mut pending = vec![&mut region.layout];
    while let Some(node) = pending.pop() {
        match node {
            DockNode::Tabs { panels, active_index, .. } => {
                panels.push(key);
                *active_index = panels.len() - 1;
                return layout;
            }
            DockNode::Split { children, .. } => pending.extend(children.iter_mut().rev()),
        }
    }

    // A dock whose tree holds no tab group gets one.
    region.layout = DockLayout::tabs().panel(key).into();
    layout
}

/// `layout` without chart `id`, its emptied containers folded away.
pub(crate) fn without_chart(mut layout: DockAreaLayout, id: ChartId) -> DockAreaLayout {
    let key = StatsPanel::Chart(id).key();
    let mut pending: Vec<&mut DockNode> = vec![&mut layout.center];
    for region in [&mut layout.left, &mut layout.right, &mut layout.bottom].into_iter().flatten() {
        pending.push(&mut region.layout);
    }
    while let Some(node) = pending.pop() {
        match node {
            DockNode::Tabs { panels, active_index, .. } => {
                let Some(at) = panels.iter().position(|panel| *panel == key) else { continue };
                panels.remove(at);
                if *active_index >= at && *active_index > 0 {
                    *active_index -= 1;
                }
            }
            DockNode::Split { children, .. } => pending.extend(children.iter_mut()),
        }
    }

    layout.normalized()
}

/// Every panel the layout places, center first, then the left, right and
/// bottom docks.
pub(crate) fn panels(layout: &DockAreaLayout) -> Vec<PanelId> {
    let mut pending: Vec<&DockNode> = Vec::new();
    for region in [&layout.bottom, &layout.right, &layout.left].into_iter().flatten() {
        pending.push(&region.layout);
    }
    pending.push(&layout.center);

    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        match node {
            DockNode::Tabs { panels, .. } => found.extend(panels.iter().cloned()),
            DockNode::Split { children, .. } => pending.extend(children.iter().rev()),
        }
    }

    found
}

/// The charts the layout places, in the order it lists them.
pub(crate) fn charts(layout: &DockAreaLayout) -> Result<Vec<ChartId>, LayoutError> {
    let mut charts = Vec::new();
    for key in panels(layout) {
        let Some(panel) = StatsPanel::parse(&key) else {
            return Err(LayoutError::UnknownPanel { panel: key.as_str().to_owned() });
        };
        let StatsPanel::Chart(id) = panel else { continue };
        if charts.contains(&id) {
            return Err(LayoutError::RepeatedChart { id: id.0 });
        }
        charts.push(id);
    }

    Ok(charts)
}

/// A saved layout as the dock area takes it.
pub(crate) fn from_state(state: &DockAreaState) -> Result<DockAreaLayout, LayoutError> {
    let mut unknown: Option<String> = None;
    let mut builder = |leaf: &PanelState| match StatsPanel::from_state(leaf) {
        Some(panel) => panel.key(),
        None => {
            unknown.get_or_insert_with(|| leaf.panel_name.clone());
            PanelId::new(leaf.panel_name.clone())
        }
    };
    let layout = DockAreaLayout::from_state(state, &mut builder);
    if let Some(panel) = unknown {
        return Err(LayoutError::UnknownPanel { panel });
    }

    charts(&layout)?;
    Ok(layout)
}

/// The layout as the settings row stores it.
pub(crate) fn to_state(layout: &DockAreaLayout) -> Result<DockAreaState, LayoutError> {
    charts(layout)?;

    let source = |key: &PanelId| match StatsPanel::parse(key) {
        Some(panel) => panel.state(),
        // `charts` has turned every unknown key away.
        None => PanelState::new(key.as_str()),
    };
    Ok(layout.to_state(&source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_keys_round_trip() {
        for panel in [StatsPanel::Overview, StatsPanel::Ships, StatsPanel::Chart(ChartId(12))] {
            assert_eq!(StatsPanel::parse(&panel.key()), Some(panel));
            assert_eq!(StatsPanel::from_state(&panel.state()), Some(panel));
        }
        assert_eq!(StatsPanel::parse(&PanelId::new("chart-x")), None);
    }

    #[test]
    fn the_default_layout_docks_each_chart_on_the_right() {
        let layout = default_layout(&[ChartId(0), ChartId(3)]);

        let keys: Vec<String> = panels(&layout).iter().map(|key| key.as_str().to_owned()).collect();
        assert_eq!(keys, ["overview", "ships", "chart-0", "chart-3"]);
        let right = layout.right.as_ref().expect("the charts have a dock");
        assert_eq!(right.size, Some(px(500.0)));
        assert_eq!(charts(&layout).expect("the charts are known"), [ChartId(0), ChartId(3)]);
    }

    #[test]
    fn closing_a_chart_leaves_the_other_panels() {
        let layout = default_layout(&[ChartId(0), ChartId(1)]);

        let layout = without_chart(layout, ChartId(0));

        assert_eq!(charts(&layout).expect("charts"), [ChartId(1)]);
        assert!(panels(&layout).contains(&StatsPanel::Overview.key()));
    }

    #[test]
    fn adding_a_chart_opens_a_closed_dock_and_selects_it() {
        let mut layout = default_layout(&[ChartId(0)]);
        layout.right.as_mut().expect("a dock").open = Some(false);

        let layout = with_chart(layout, ChartId(1));

        let right = layout.right.as_ref().expect("a dock");
        assert_eq!(right.open, Some(true));
        let DockNode::Tabs { panels, active_index, .. } = &right.layout else { panic!("a tab group") };
        assert_eq!(panels.len(), 2);
        assert_eq!(*active_index, 1);
    }

    /// The row the GPUI app writes, read and written back unchanged: both
    /// apps keep one arrangement.
    #[test]
    fn a_gpui_layout_round_trips() {
        let row = r#"{
            "version": 1,
            "center": {"panel_name": "StackPanel", "children": [
                {"panel_name": "TabPanel", "children": [
                    {"panel_name": "StatsOverviewPanel", "children": [], "info": {"panel": null}},
                    {"panel_name": "StatsShipsPanel", "children": [], "info": {"panel": null}}
                ], "info": {"tabs": {"active_index": 1}}}
            ], "info": {"stack": {"sizes": [600.0], "axis": 0}}},
            "right_dock": {"panel": {"panel_name": "TabPanel", "children": [
                {"panel_name": "StatsChartPanel", "children": [], "info": {"panel": {"id": 4}}}
            ], "info": {"tabs": {"active_index": 0}}}, "placement": "right", "size": 420.0, "open": true}
        }"#;
        let state: DockAreaState = serde_json::from_str(row).expect("the GPUI row parses");

        let layout = from_state(&state).expect("the layout is ours");

        assert_eq!(charts(&layout).expect("charts"), [ChartId(4)]);
        assert_eq!(layout.right.as_ref().and_then(|right| right.size), Some(px(420.0)));
        let written = to_state(&layout).expect("the layout writes");
        let reread = from_state(&written).expect("and reads back");
        assert_eq!(panels(&reread), panels(&layout));
        let json = serde_json::to_value(&written).expect("the state serializes");
        assert_eq!(json["right_dock"]["panel"]["children"][0]["panel_name"], "StatsChartPanel");
        assert_eq!(json["right_dock"]["panel"]["children"][0]["info"]["panel"]["id"], 4);
    }

    #[test]
    fn a_layout_with_a_foreign_panel_is_refused() {
        let mut state = to_state(&default_layout(&[])).expect("the default layout writes");
        state.center.children[0].children[0].panel_name = "SomethingElse".to_owned();

        assert!(matches!(from_state(&state), Err(LayoutError::UnknownPanel { .. })));
    }
}

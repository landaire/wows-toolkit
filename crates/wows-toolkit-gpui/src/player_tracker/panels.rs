//! The tracker's three sections as dock panels, so they can be split and
//! docked the way the egui tracker's are.
//!
//! Each panel is a shell: the tab itself still owns the period, the filter,
//! the division toggle and the data, and draws the section through
//! [`PlayerTrackerView::render_section`]. What a panel adds is its own scroll
//! position, which is what lets two sections sit beside each other and each
//! scroll its own rows.

use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::*;
use rust_i18n::t;

use super::PlayerTrackerView;
use super::SubTab;

/// How far beyond the visible rows the list keeps drawn, so a scroll does not
/// reveal blank space before it fills. The tab's own former value.
const LIST_OVERDRAW: Pixels = px(200.);

/// One section of the tracker, in a panel of its own.
pub(crate) struct TrackerPanel {
    section: SubTab,
    /// The tab that owns the data. Weak because the panel outlives nothing:
    /// the tab holds the dock that holds this.
    tracker: WeakEntity<PlayerTrackerView>,
    /// This section's own scroll position.
    list: ListState,
    focus_handle: FocusHandle,
}

impl TrackerPanel {
    pub(crate) fn new(section: SubTab, tracker: WeakEntity<PlayerTrackerView>, cx: &mut Context<Self>) -> Self {
        Self {
            section,
            tracker,
            list: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Which section this panel draws.
    pub(crate) fn section(&self) -> SubTab {
        self.section
    }

    /// How many rows this section's own list is holding. Test-only: the
    /// production path pushes counts in rather than reading them back.
    #[cfg(test)]
    pub(crate) fn list_len(&self) -> usize {
        self.list.item_count()
    }

    /// Tells the list how many rows there are now, which is what a filter or
    /// a period change means for it.
    pub(crate) fn rows_changed(&mut self, rows: usize, cx: &mut Context<Self>) {
        self.list.reset(rows);
        cx.notify();
    }

    /// Tells the list that its rows changed height, which is what opening a
    /// row does.
    pub(crate) fn remeasure(&mut self, rows: usize, cx: &mut Context<Self>) {
        self.list.remeasure_items(0..rows);
        cx.notify();
    }
}

impl Focusable for TrackerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for TrackerPanel {}

impl BasePanel for TrackerPanel {
    fn panel_name(&self) -> &'static str {
        "TrackerPanel"
    }

    /// The egui tracker keeps all three; closing one here would leave no way
    /// back to it.
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl Panel for TrackerPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(t!(self.section.label_key()).into_owned())
    }
}

impl Render for TrackerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(tracker) = self.tracker.upgrade() else {
            return div().size_full().into_any_element();
        };
        let section = self.section;
        let list = self.list.clone();
        tracker.update(cx, |tracker, cx| tracker.render_section(section, &list, cx))
    }
}

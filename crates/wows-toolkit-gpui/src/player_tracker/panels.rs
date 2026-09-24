//! The tracker's three sections as dock panels, so they can be split and
//! docked the way the egui tracker's are.
//!
//! Each panel is a shell: the tab itself still owns the period, the filter,
//! the division toggle and the data, and draws the section through
//! [`PlayerTrackerView::render_section`]. What a panel adds is its own scroll
//! position, which is what lets two sections sit beside each other and each
//! scroll its own rows.
//!
//! A panel watches the tab, because the dock draws it through
//! `Entity::cached`: that recycles the drawn subtree until the panel's own
//! entity is notified, and notifying the tab marks only the tab and its
//! ancestors. Without the watch a section would keep last frame's rows until
//! something unrelated forced a redraw.
//!
//! No test covers that watch: the harness draws through `render_frame`, which
//! calls `Window::refresh` and so ignores caching. It has to be read off the
//! running app.

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
    /// Held to keep the watch on the tab alive. Installed on the first draw
    /// rather than at construction, because the tab is not yet reachable
    /// while it is building the panels that point back at it.
    watching: Option<Subscription>,
    focus_handle: FocusHandle,
}

impl TrackerPanel {
    pub(crate) fn new(section: SubTab, tracker: WeakEntity<PlayerTrackerView>, cx: &mut Context<Self>) -> Self {
        Self {
            section,
            tracker,
            list: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            watching: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Which section this panel draws.
    pub(crate) fn section(&self) -> SubTab {
        self.section
    }

    /// How many rows this section's own list is holding. Test-only: each
    /// section sets its own count as it draws, so nothing in the app reads
    /// one back.
    #[cfg(test)]
    pub(crate) fn list_len(&self) -> usize {
        self.list.item_count()
    }

    /// Tells the list that its rows changed height, which is what opening a
    /// row does. The count is the list's own, since only the heights moved.
    pub(crate) fn remeasure(&mut self, cx: &mut Context<Self>) {
        self.list.remeasure_items(0..self.list.item_count());
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
        if self.watching.is_none() {
            self.watching = Some(cx.observe(&tracker, |_panel, _tracker, cx| cx.notify()));
        }
        let section = self.section;
        let list = self.list.clone();
        // The dock focuses this handle when its tab is activated, so the
        // section has to carry it or the focus lands on nothing.
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .child(tracker.update(cx, |tracker, cx| tracker.render_section(section, &list, cx)))
            .into_any_element()
    }
}

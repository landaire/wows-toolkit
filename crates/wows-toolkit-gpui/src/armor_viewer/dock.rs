//! Milestone 5 Task 9b's multi-pane comparison split: `ViewportDock` lays out
//! its `panes` itself with `gpui_kit::component::resizable::h_resizable` rather
//! than a `dock::DockArea`/`TabPanel` (see Task 9a's doc, preserved below,
//! for why: `TabPanel::render_title_bar` always draws an unsuppressable 30px
//! title bar). With exactly one pane this renders with no border, filling
//! the whole area. Once a second pane is added (via `ArmorViewerPane`'s
//! "Compare" action, `pane.rs`/`sidebar.rs`), the panes share the area in
//! resizable splits, laid out along `split`, and the active pane gets a thin
//! accent border. Every pane carries a close button; closing the last one
//! empties it rather than removing it ([`DockEvent::ResetLastPane`]).
//!
//! Camera-mirror and settings-sync across panes (Milestone 5 Task 9c) are
//! out of scope here -- each pane's `ViewportView` remains fully
//! self-contained (own camera, own hull/camo/visibility state, own
//! toolbar/popovers).
//!
//! Task 9a's original doc:
//!
//! This deliberately does not wire a real gpui-component `dock::DockArea`/
//! `TabPanel`, even though the Replay Inspector's dock (`replay_inspector::
//! view::ReplayInspectorView`) is the reference pattern: `TabPanel::
//! render_title_bar` always draws a 30px title bar over the active panel --
//! even with exactly one tab and `PanelStyle::default()` -- with no way to
//! suppress it (see that crate's `tab_panel.rs`).

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::resizable::v_resizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use super::viewport_view::ViewportView;

/// What the dock needs its owner to do, because it cannot do it itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEvent {
    /// The last pane's close button was pressed. A viewer with no viewport
    /// would render nothing, so `ArmorViewerPane` answers this by putting a
    /// fresh empty pane in its place, as the egui app does when its final
    /// tab is closed.
    ResetLastPane,
}

impl EventEmitter<DockEvent> for ViewportDock {}

pub struct ViewportDock {
    panes: Vec<Entity<ViewportView>>,
    active_ix: usize,
    /// Which way the panes are laid out; side by side until asked otherwise.
    split: Axis,
}

impl ViewportDock {
    /// Wraps the single viewport `pane.rs` creates for the tab. `add_pane`
    /// grows this into a real split once the user clicks "Compare".
    pub fn new(viewport: Entity<ViewportView>) -> Self {
        Self { panes: vec![viewport], active_ix: 0, split: Axis::Horizontal }
    }

    /// Every pane currently in the dock, in display order. `ArmorViewerPane`
    /// reads this to fan a shared-GPU-device result out to every pane
    /// (`apply_gpu_result`), including ones added after the device was
    /// already ready.
    pub fn panes(&self) -> &[Entity<ViewportView>] {
        &self.panes
    }

    /// The pane a ship selection should load into: the one the user last
    /// clicked in, or the one a new pane became when added.
    pub fn active_viewport(&self) -> Entity<ViewportView> {
        self.panes[self.active_ix].clone()
    }

    /// Which way the panes are laid out.
    pub fn split_axis(&self) -> Axis {
        self.split
    }

    /// Lays the panes out the other way. Side by side compares two ships'
    /// beams; one above the other compares their lengths, which is what a
    /// long hull needs.
    pub fn set_split(&mut self, split: Axis, cx: &mut Context<Self>) {
        if self.split == split {
            return;
        }
        self.split = split;
        cx.notify();
    }

    /// Pushes `viewport` onto the split and makes it the active pane, so the
    /// next ship selection loads into it. Called by `ArmorViewerPane`'s
    /// "Compare" handler after handing the new pane the shared GPU device
    /// (or leaving it `Initializing` if the device isn't ready yet).
    pub fn add_pane(&mut self, viewport: Entity<ViewportView>, cx: &mut Context<Self>) {
        self.panes.push(viewport);
        self.active_ix = self.panes.len() - 1;
        cx.notify();
    }

    fn activate_pane(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.panes.len() && ix != self.active_ix {
            self.active_ix = ix;
            cx.notify();
        }
    }

    /// Puts `viewport` in as the dock's only pane, answering
    /// [`DockEvent::ResetLastPane`].
    pub fn reset_to(&mut self, viewport: Entity<ViewportView>, cx: &mut Context<Self>) {
        self.panes = vec![viewport];
        self.active_ix = 0;
        cx.notify();
    }

    /// Acts on the close button of pane `ix`. The decision ([`close_outcome`])
    /// and the index math afterward ([`active_ix_after_remove`]) are factored
    /// out so they're unit-testable without a gpui_kit `Context`.
    fn close_pane(&mut self, ix: usize, cx: &mut Context<Self>) {
        match close_outcome(self.panes.len(), ix) {
            CloseOutcome::Ignore => {}
            CloseOutcome::ResetLast => cx.emit(DockEvent::ResetLastPane),
            CloseOutcome::Remove => {
                self.panes.remove(ix);
                self.active_ix = active_ix_after_remove(self.active_ix, ix, self.panes.len());
                cx.notify();
            }
        }
    }
}

/// What closing pane `ix` out of `len` panes does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseOutcome {
    /// `ix` does not name a pane.
    Ignore,
    Remove,
    /// `ix` is the only pane: it is emptied rather than removed, because a
    /// viewer with no viewport would render nothing.
    ResetLast,
}

fn close_outcome(len: usize, ix: usize) -> CloseOutcome {
    if ix >= len {
        CloseOutcome::Ignore
    } else if len > 1 {
        CloseOutcome::Remove
    } else {
        CloseOutcome::ResetLast
    }
}

/// Which pane index should be active after removing `removed_ix`, given the
/// dock's `active_ix` beforehand and `remaining_len` panes afterward (always
/// `> 0` -- callers only reach this once [`close_outcome`] said to remove). A pane
/// before the active one shifts every later index down by one; the active
/// pane being removed (or the active index now pointing past the end)
/// clamps to the new last pane.
fn active_ix_after_remove(active_ix: usize, removed_ix: usize, remaining_len: usize) -> usize {
    if active_ix >= remaining_len {
        remaining_len - 1
    } else if active_ix > removed_ix {
        active_ix - 1
    } else {
        active_ix
    }
}

impl Render for ViewportDock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let multi = self.panes.len() > 1;
        let active_ix = self.active_ix;
        let accent = cx.theme().primary;

        // A resizable group is built for one axis, so switching rebuilds it
        // under a different id; sharing one id would carry the horizontal
        // sizes over to the vertical layout.
        let mut group = match self.split {
            Axis::Horizontal => h_resizable("armor-viewport-dock-h"),
            Axis::Vertical => v_resizable("armor-viewport-dock-v"),
        };
        for (ix, pane) in self.panes.iter().cloned().enumerate() {
            let is_active = ix == active_ix;
            let close_button = div().absolute().top_1().right_1().child(
                Button::new(("armor-viewport-pane-close", ix))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip(t!("ui.armor.close_pane").to_string())
                    .on_click(cx.listener(move |this, _event, _window, cx| this.close_pane(ix, cx))),
            );
            let wrapper = div()
                .id(("armor-viewport-pane", ix))
                .relative()
                .size_full()
                // Fires on any left-click inside the pane, including its
                // toolbar/close buttons -- enabled `gpui-component` `Button`s
                // don't stop propagation, so clicking a control on an
                // inactive pane both runs the control's own action AND
                // activates that pane. Intended: interacting with a pane is
                // what makes it the active one (the target for sidebar ship
                // selections and the source for mirror/sync-on-enable).
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event, _window, cx| this.activate_pane(ix, cx)),
                )
                .when(multi, |this| this.border_2().border_color(if is_active { accent } else { transparent_black() }))
                .child(pane)
                .child(close_button);
            group = group.child(resizable_panel().child(wrapper));
        }

        div().size_full().child(group)
    }
}

#[cfg(test)]
mod tests {
    use super::CloseOutcome;
    use super::active_ix_after_remove;
    use super::close_outcome;

    #[test]
    fn closing_the_only_pane_empties_it() {
        assert_eq!(close_outcome(1, 0), CloseOutcome::ResetLast);
    }

    #[test]
    fn closing_removes_when_multiple_panes_exist() {
        assert_eq!(close_outcome(2, 0), CloseOutcome::Remove);
        assert_eq!(close_outcome(3, 2), CloseOutcome::Remove);
    }

    #[test]
    fn closing_an_out_of_range_index_does_nothing() {
        assert_eq!(close_outcome(3, 3), CloseOutcome::Ignore);
        assert_eq!(close_outcome(0, 0), CloseOutcome::Ignore);
    }

    #[test]
    fn active_ix_shifts_left_when_a_pane_before_it_is_removed() {
        // 3 panes, active = 2 (last); removing pane 0 shifts it to 1.
        assert_eq!(active_ix_after_remove(2, 0, 2), 1);
    }

    #[test]
    fn active_ix_unchanged_when_a_later_pane_is_removed() {
        // 3 panes, active = 0; removing pane 2 (after it) leaves it at 0.
        assert_eq!(active_ix_after_remove(0, 2, 2), 0);
    }

    #[test]
    fn active_ix_clamps_when_the_active_last_pane_is_removed() {
        // 3 panes, active = 2 (the one being removed); clamps to the new last pane.
        assert_eq!(active_ix_after_remove(2, 2, 2), 1);
    }

    #[test]
    fn active_ix_stays_in_place_when_the_active_pane_itself_is_removed_mid_list() {
        // 3 panes, active = 1 (the one being removed); the pane that slides
        // into index 1 becomes active, so the index itself doesn't move.
        assert_eq!(active_ix_after_remove(1, 1, 2), 1);
    }

    #[test]
    fn active_ix_stays_when_removing_the_first_pane_while_it_is_active() {
        assert_eq!(active_ix_after_remove(0, 0, 1), 0);
    }
}

use std::collections::HashMap;

use crate::armor_viewer::state::ArmorPane;
use crate::viewport_3d::ArcballCamera;

/// Direction of a split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Settings to clone into a new pane for comparison.
pub struct CompareSettings {
    pub ship_param_index: String,
    pub ship_display_name: String,
    pub camera: ArcballCamera,
    pub part_visibility: HashMap<(String, String), bool>,
    pub hull_visibility: HashMap<String, bool>,
}

/// Action to apply to the split tree after rendering (deferred mutation).
pub enum SplitAction {
    SplitHorizontal(u64),
    SplitVertical(u64),
    Close(u64),
    /// Split horizontally and load the same ship with cloned settings in the new pane.
    Compare(u64, CompareSettings),
}

/// Recursive split tree. Each node is either a leaf (single pane) or a split
/// containing two children with a draggable divider.
pub enum SplitNode {
    Leaf(ArmorPane),
    Split {
        direction: SplitDirection,
        /// Fraction [0.0, 1.0] of the first child's share. Default 0.5.
        fraction: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

impl SplitNode {
    /// Collect mutable references to all leaf panes.
    pub fn all_panes_mut(&mut self) -> Vec<&mut ArmorPane> {
        match self {
            SplitNode::Leaf(pane) => vec![pane],
            SplitNode::Split { first, second, .. } => {
                let mut panes = first.all_panes_mut();
                panes.extend(second.all_panes_mut());
                panes
            }
        }
    }

    /// Count total leaf panes.
    pub fn pane_count(&self) -> usize {
        match self {
            SplitNode::Leaf(_) => 1,
            SplitNode::Split { first, second, .. } => first.pane_count() + second.pane_count(),
        }
    }

    /// Apply a split action to the tree. Returns true if the action was applied.
    pub fn apply_action(&mut self, action: &SplitAction, new_pane_fn: &mut dyn FnMut() -> ArmorPane) -> bool {
        match self {
            SplitNode::Leaf(pane) => match action {
                SplitAction::SplitHorizontal(id) | SplitAction::SplitVertical(id) if *id == pane.id => {
                    let direction = match action {
                        SplitAction::SplitHorizontal(_) => SplitDirection::Horizontal,
                        SplitAction::SplitVertical(_) => SplitDirection::Vertical,
                        _ => unreachable!(),
                    };
                    let existing = std::mem::replace(self, SplitNode::Leaf(ArmorPane::empty(0)));
                    *self = SplitNode::Split {
                        direction,
                        fraction: 0.5,
                        first: Box::new(existing),
                        second: Box::new(SplitNode::Leaf(new_pane_fn())),
                    };
                    true
                }
                SplitAction::Compare(id, _) if *id == pane.id => {
                    let existing = std::mem::replace(self, SplitNode::Leaf(ArmorPane::empty(0)));
                    *self = SplitNode::Split {
                        direction: SplitDirection::Horizontal,
                        fraction: 0.5,
                        first: Box::new(existing),
                        second: Box::new(SplitNode::Leaf(new_pane_fn())),
                    };
                    true
                }
                SplitAction::Close(id) if *id == pane.id => {
                    // Can't close root-level leaf; handled by caller
                    false
                }
                _ => false,
            },
            SplitNode::Split { first, second, .. } => {
                if first.apply_action(action, new_pane_fn) {
                    return true;
                }
                if second.apply_action(action, new_pane_fn) {
                    return true;
                }

                // Handle close: if a child leaf is being closed, replace this split with the sibling
                if let SplitAction::Close(id) = action {
                    let first_is_target = matches!(first.as_ref(), SplitNode::Leaf(p) if p.id == *id);
                    let second_is_target = matches!(second.as_ref(), SplitNode::Leaf(p) if p.id == *id);

                    if first_is_target {
                        let sibling = std::mem::replace(second.as_mut(), SplitNode::Leaf(ArmorPane::empty(0)));
                        *self = sibling;
                        return true;
                    }
                    if second_is_target {
                        let sibling = std::mem::replace(first.as_mut(), SplitNode::Leaf(ArmorPane::empty(0)));
                        *self = sibling;
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Render the split tree recursively into the given Ui.
    /// The `render_pane` callback renders a single leaf pane and returns an optional action.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        render_pane: &mut dyn FnMut(&mut egui::Ui, &mut ArmorPane) -> Option<SplitAction>,
    ) -> Option<SplitAction> {
        match self {
            SplitNode::Leaf(pane) => render_pane(ui, pane),
            SplitNode::Split { direction, fraction, first, second } => {
                let rect = ui.available_rect_before_wrap();
                let divider_width = 4.0;

                let (first_rect, divider_rect, second_rect) = match direction {
                    SplitDirection::Horizontal => {
                        let split_x = rect.left() + rect.width() * *fraction;
                        let first_r = egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(split_x - divider_width * 0.5, rect.bottom()),
                        );
                        let div_r = egui::Rect::from_min_max(
                            egui::pos2(split_x - divider_width * 0.5, rect.top()),
                            egui::pos2(split_x + divider_width * 0.5, rect.bottom()),
                        );
                        let second_r =
                            egui::Rect::from_min_max(egui::pos2(split_x + divider_width * 0.5, rect.top()), rect.max);
                        (first_r, div_r, second_r)
                    }
                    SplitDirection::Vertical => {
                        let split_y = rect.top() + rect.height() * *fraction;
                        let first_r =
                            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split_y - divider_width * 0.5));
                        let div_r = egui::Rect::from_min_max(
                            egui::pos2(rect.left(), split_y - divider_width * 0.5),
                            egui::pos2(rect.right(), split_y + divider_width * 0.5),
                        );
                        let second_r =
                            egui::Rect::from_min_max(egui::pos2(rect.left(), split_y + divider_width * 0.5), rect.max);
                        (first_r, div_r, second_r)
                    }
                };

                // Draw divider
                let divider_response = ui.allocate_rect(divider_rect, egui::Sense::drag());
                let divider_color = if divider_response.dragged() || divider_response.hovered() {
                    ui.visuals().widgets.active.bg_fill
                } else {
                    ui.visuals().widgets.noninteractive.bg_fill
                };
                ui.painter().rect_filled(divider_rect, 0.0, divider_color);

                // Handle divider dragging
                if divider_response.dragged() {
                    let delta = divider_response.drag_delta();
                    match direction {
                        SplitDirection::Horizontal => {
                            *fraction += delta.x / rect.width().max(1.0);
                        }
                        SplitDirection::Vertical => {
                            *fraction += delta.y / rect.height().max(1.0);
                        }
                    }
                    *fraction = fraction.clamp(0.1, 0.9);
                }

                // Set cursor for divider
                if divider_response.hovered() || divider_response.dragged() {
                    match direction {
                        SplitDirection::Horizontal => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                        SplitDirection::Vertical => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                        }
                    }
                }

                // Render children
                let mut action = None;

                let mut first_ui = ui.new_child(egui::UiBuilder::new().max_rect(first_rect));
                if let Some(a) = first.show(&mut first_ui, render_pane) {
                    action = Some(a);
                }

                let mut second_ui = ui.new_child(egui::UiBuilder::new().max_rect(second_rect));
                if action.is_none() {
                    if let Some(a) = second.show(&mut second_ui, render_pane) {
                        action = Some(a);
                    }
                }

                // Reserve the full rect so the parent UI knows we've used it
                ui.allocate_rect(rect, egui::Sense::hover());

                action
            }
        }
    }
}

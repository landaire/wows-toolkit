//! Armor Thickness legend: a floating, draggable panel shown over the whole
//! Armor Viewer pane once a ship is loaded. Reproduces the egui app's
//! `armor_viewer::ui::legend::show_armor_legend` content -- a bold title,
//! a 2px gap, then one swatch+label row per `armor_color_legend()` entry --
//! inside a panel that can be dragged, collapsed, and closed, matching the
//! egui app's `armor_legend_global` window (`ui/tab.rs` ~626-664): default
//! position (10, 100), gated on `defaults.show_legend`/`legend_collapsed`.
//!
//! Deviation from egui: egui's window shows "Armor Thickness" twice -- once
//! as the window's own title-bar chrome text, once again as
//! `show_armor_legend`'s own first content row. This port folds both into a
//! single title row that doubles as the drag handle, avoiding a redundant
//! duplicate label; the swatch/label rows below are an exact reproduction of
//! `show_armor_legend`'s logic (title, 2px gap, per-entry rows).
//!
//! State (`ArmorViewerPane::legend: LegendState`) and the mouse handlers that
//! move it live on `ArmorViewerPane` (`pane.rs`), since dragging needs to
//! keep tracking the pointer even once it leaves this panel's own small
//! bounds -- the move/up listeners are registered on the pane's full-size
//! wrapping div, mirroring `viewport_view`'s gizmo-drag pattern of attaching
//! move/up to the whole interactive surface rather than the small hit target
//! that started the drag.

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::IconName;
use gpui_component::Sizable;
use gpui_component::button::Button;
use gpui_component::button::ButtonVariants;
use gpui_component::h_flex;
use gpui_component::v_flex;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wowsunpack::export::gltf_export::ArmorLegendEntry;
use wowsunpack::export::gltf_export::armor_color_legend;

use super::pane::ArmorViewerPane;

/// Default floating position, matching the egui app's `armor_legend_global`
/// window default (`ui/tab.rs`: `unwrap_or(egui::pos2(10.0, 100.0))`).
pub const DEFAULT_POS: Point<Pixels> = point(px(10.), px(100.));

const SWATCH_WIDTH: Pixels = px(16.);
const SWATCH_HEIGHT: Pixels = px(12.);
const SWATCH_RADIUS: Pixels = px(2.);
const TITLE_FONT_SIZE: Pixels = px(12.);
const TITLE_GAP: Pixels = px(2.);
const PANEL_WIDTH: Pixels = px(140.);

/// Floating-panel state: visibility, collapsed state, position, and the
/// in-flight drag (if any). Owned by `ArmorViewerPane`, seeded from the
/// persisted `ArmorViewerDefaultsRow` via `from_defaults`.
pub struct LegendState {
    pub visible: bool,
    pub collapsed: bool,
    pub pos: Point<Pixels>,
    pub drag: Option<LegendDrag>,
}

/// Pointer and panel position captured at mouse-down, so the drag-move
/// handler can compute the new panel position from the pointer's total
/// displacement rather than accumulating per-frame deltas.
#[derive(Clone, Copy)]
pub struct LegendDrag {
    pub pointer_start: Point<Pixels>,
    pub panel_start: Point<Pixels>,
}

impl Default for LegendState {
    fn default() -> Self {
        // Matches the egui app's `ArmorViewerDefaults::default()`
        // (`armor_viewer/state.rs`): shown, not collapsed, default position.
        Self { visible: true, collapsed: false, pos: DEFAULT_POS, drag: None }
    }
}

impl LegendState {
    /// Seeds initial visibility/collapsed/position from the persisted
    /// `armor_viewer_defaults` row, falling back to the egui defaults above
    /// when there is no row yet (fresh DB) or the position was never saved.
    pub fn from_defaults(row: Option<&ArmorViewerDefaultsRow>) -> Self {
        let Some(row) = row else { return Self::default() };
        let pos = match (row.legend_pos_x, row.legend_pos_y) {
            (Some(x), Some(y)) => point(px(x as f32), px(y as f32)),
            _ => DEFAULT_POS,
        };
        Self { visible: row.show_legend, collapsed: row.legend_collapsed, pos, drag: None }
    }
}

/// Converts one legend entry's `[f32; 4]` 0..1 RGBA (`ArmorLegendEntry::color`)
/// to a gpui `Hsla`, matching the egui original's `Color32::from_rgba_unmultiplied`
/// conversion (`armor_viewer/ui/legend.rs`): each channel times 255, rounded to u8.
fn swatch_color(color: [f32; 4]) -> Hsla {
    let to_u8 = |c: f32| (c * 255.0).round().clamp(0.0, 255.0) as u8;
    let bytes = [to_u8(color[0]), to_u8(color[1]), to_u8(color[2]), to_u8(color[3])];
    rgba(u32::from_be_bytes(bytes)).into()
}

/// One legend entry's label, matching `show_armor_legend`'s exact branching
/// (`armor_viewer/ui/legend.rs`): `{min}+ mm` for the top-open bucket
/// (`max_mm >= 999.0`), else `{min}-{max} mm`.
fn entry_label(entry: &ArmorLegendEntry) -> String {
    if entry.max_mm >= 999.0 {
        format!("{}+ mm", entry.min_mm as u32)
    } else {
        format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32)
    }
}

fn legend_row(entry: &ArmorLegendEntry) -> AnyElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(div().flex_none().w(SWATCH_WIDTH).h(SWATCH_HEIGHT).rounded(SWATCH_RADIUS).bg(swatch_color(entry.color)))
        .child(div().text_size(TITLE_FONT_SIZE).child(entry_label(entry)))
        .into_any_element()
}

/// The floating panel: a draggable title row (collapse toggle, drag handle
/// doubling as the "Armor Thickness" title, close button) plus the legend
/// rows below when not collapsed. `state.pos` positions the panel absolutely
/// over the whole pane; `cx` wires the header controls to `ArmorViewerPane`'s
/// mutator methods (`pane.rs`).
pub fn render_panel(state: &LegendState, cx: &mut Context<ArmorViewerPane>) -> AnyElement {
    let theme = cx.theme();
    let background = theme.background;
    let border = theme.border;
    let radius = theme.radius;

    let collapse_icon = if state.collapsed { IconName::ChevronRight } else { IconName::ChevronDown };

    let header = h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .px_1()
        .py_1()
        .child(
            Button::new("armor-legend-collapse")
                .icon(collapse_icon)
                .ghost()
                .xsmall()
                .tooltip("Collapse")
                .on_click(cx.listener(ArmorViewerPane::toggle_legend_collapsed)),
        )
        .child(
            div()
                .id("armor-legend-drag-handle")
                .flex_1()
                .cursor_grab()
                .font_weight(FontWeight::BOLD)
                .text_size(TITLE_FONT_SIZE)
                .child("Armor Thickness")
                .on_mouse_down(MouseButton::Left, cx.listener(ArmorViewerPane::start_legend_drag)),
        )
        .child(
            Button::new("armor-legend-close")
                .icon(IconName::Close)
                .ghost()
                .xsmall()
                .tooltip("Hide legend")
                .on_click(cx.listener(ArmorViewerPane::close_legend)),
        );

    let mut panel = v_flex()
        .id("armor-legend-panel")
        .occlude()
        .absolute()
        .left(state.pos.x)
        .top(state.pos.y)
        .w(PANEL_WIDTH)
        .bg(background)
        .border_1()
        .border_color(border)
        .rounded(radius)
        .child(header);

    if !state.collapsed {
        panel = panel.child(
            v_flex()
                .px_2()
                .pb_2()
                .gap_1()
                .child(div().h(TITLE_GAP))
                .children(armor_color_legend().iter().map(legend_row)),
        );
    }

    panel.into_any_element()
}

// `use super::*` here (pulling `gpui_component`'s macro-generated `IconName`
// et al. into a module that also carries `#[test]` attributes) blows the
// compiler's macro-expansion recursion limit for this crate; narrow, explicit
// imports avoid it and are all these tests need.
#[cfg(test)]
mod tests {
    use gpui::Rgba;

    use super::DEFAULT_POS;
    use super::LegendState;
    use super::entry_label;
    use super::swatch_color;
    use wowsunpack::export::gltf_export::armor_color_legend;

    #[test]
    fn label_formatting_matches_egui_legend() {
        let entries = armor_color_legend();
        assert_eq!(entries.len(), 10, "armor_color_legend() entry count changed; update this pin");

        for entry in &entries[..entries.len() - 1] {
            assert!(entry.max_mm < 999.0, "expected a closed bucket for {entry:?}");
            assert_eq!(entry_label(entry), format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32));
        }

        let last = entries.last().expect("non-empty legend");
        assert!(last.max_mm >= 999.0, "expected the last bucket to be top-open");
        assert_eq!(entry_label(last), format!("{}+ mm", last.min_mm as u32));
    }

    #[test]
    fn swatch_color_round_trips_rgba_channels() {
        let color = swatch_color([0.0, 1.0, 0.5, 0.8]);
        let rgba: Rgba = color.into();
        assert_eq!((rgba.r * 255.0).round() as u8, 0);
        assert_eq!((rgba.g * 255.0).round() as u8, 255);
        assert_eq!((rgba.b * 255.0).round() as u8, 128);
        assert_eq!((rgba.a * 255.0).round() as u8, 204);
    }

    #[test]
    fn legend_state_from_defaults_falls_back_to_egui_defaults() {
        let state = LegendState::from_defaults(None);
        assert!(state.visible);
        assert!(!state.collapsed);
        assert_eq!(state.pos, DEFAULT_POS);
    }
}

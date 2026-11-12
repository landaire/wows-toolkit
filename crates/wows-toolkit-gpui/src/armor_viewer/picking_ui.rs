//! The floating hover tooltips over the armor viewport: the plate thickness
//! tooltip (`show_armor_tooltip`, `tab.rs:2778-2820` -- the V1 subset only;
//! the penetration-check section, `tab.rs:2822-2884`, is deferred with the
//! analysis subsystem and is never shown here) and the mesh tooltip.
//! `viewport_view.rs` wires them into the mouse-move handlers and owns the
//! hover state.

use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::FontWeight;
use gpui_kit::Hsla;
use gpui_kit::InteractiveElement;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement;
use gpui_kit::Pixels;
use gpui_kit::Styled;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::px;
use wows_toolkit_armor::hull_picking::MeshTooltip;
use wows_toolkit_armor::load_ship::ArmorTriangleTooltip;
use wowsunpack::export::gltf_export::thickness_to_color;

use super::legend::swatch_color;

/// A small filled, rounded color swatch, matching `legend.rs`'s own swatch
/// convention (a plain colored `div`, not `paint_swatch`'s painter call --
/// there is no `egui::Painter` equivalent needed here).
fn swatch(color: [f32; 4], size: Pixels) -> impl IntoElement {
    div().flex_none().w(size).h(size).rounded(px(2.)).bg(swatch_color(color))
}

/// What the thickness tooltip says in the reader's own language: the zone
/// and material under the pointer, and what a click there can do.
pub struct TooltipLabels<'a> {
    pub zone: &'a str,
    pub material: &'a str,
    pub hint: &'a str,
}

/// Builds the thickness tooltip element for `tooltip`, reproducing the egui
/// app's `show_armor_tooltip` V1 subset (`tab.rs:2778-2820`): a swatch +
/// "{mm} mm" header, then "{zone} / {material}" -- then, only when there is
/// more than one layer, a "N layers for this part:" label and a swatch +
/// "{mm} mm" row per layer (bold when that layer matches the plate's total
/// thickness). A muted line at the foot says what a click there can do,
/// which is the one place a reader looking at a plate is sure to see it.
pub fn tooltip_element(
    tooltip: &ArmorTriangleTooltip,
    background: Hsla,
    border: Hsla,
    radius: Pixels,
    muted: Hsla,
    labels: TooltipLabels<'_>,
) -> AnyElement {
    let mut body = v_flex()
        .gap_1()
        .child(h_flex().gap_1().items_center().child(swatch(tooltip.color, px(12.))).child(
            div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(format!("{:.0} mm", tooltip.thickness_mm)),
        ))
        .child(div().text_sm().child(format!("{} / {}", labels.zone, labels.material)));

    if tooltip.layers.len() > 1 {
        body = body
            .child(div().h(px(1.)).bg(border))
            .child(div().text_xs().text_color(muted).child(format!("{} layers for this part:", tooltip.layers.len())));

        for &layer_mm in &tooltip.layers {
            let is_this = (layer_mm - tooltip.thickness_mm).abs() < 0.1;
            let mut label = div().text_sm().child(format!("{:.0} mm", layer_mm));
            if is_this {
                label = label.font_weight(FontWeight::BOLD);
            }
            body = body.child(
                h_flex().gap_1().items_center().child(swatch(thickness_to_color(layer_mm), px(10.))).child(label),
            );
        }
    }

    body = body.child(div().text_xs().text_color(muted).child(labels.hint.to_string()));

    div()
        .id("armor-plate-tooltip")
        .test_support()
        .bg(background)
        .border_1()
        .border_color(border)
        .rounded(radius)
        .shadow(crate::theme::overlay_shadow())
        .px_2()
        .py_1()
        .child(body)
        .into_any_element()
}

/// Builds the mesh tooltip: the mesh's name, its group, and the model and
/// hardpoint it comes from.
pub fn mesh_tooltip_element(tooltip: &MeshTooltip, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let body = v_flex()
        .gap_1()
        .child(div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(tooltip.name.clone()))
        .child(div().text_sm().child(tooltip.group_label()))
        .children(tooltip.model_label().map(|model| div().text_xs().text_color(muted).child(model)))
        .children(tooltip.hardpoint_label().map(|hardpoint| div().text_xs().text_color(muted).child(hardpoint)));

    crate::ui::overlay_surface(div().id("armor-mesh-tooltip").test_support(), cx)
        .px_2()
        .py_1()
        .child(body)
        .into_any_element()
}

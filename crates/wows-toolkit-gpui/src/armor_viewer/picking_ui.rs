//! The floating plate-thickness tooltip, reproducing the egui app's
//! `show_armor_tooltip` (`armor_viewer/ui/tab.rs:2778-2820`). The picking it
//! reports on is `wows_toolkit_armor::highlight`.

use gpui_kit::AnyElement;
use gpui_kit::FontWeight;
use gpui_kit::Hsla;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement;
use gpui_kit::Pixels;
use gpui_kit::Styled;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::px;
use wows_toolkit_armor::load_ship::ArmorTriangleTooltip;
use wowsunpack::export::gltf_export::thickness_to_color;

use super::legend::swatch_color;

/// A small filled, rounded color swatch, matching `legend.rs`'s own swatch
/// convention (a plain colored `div`, not `paint_swatch`'s painter call --
/// there is no `egui::Painter` equivalent needed here).
fn swatch(color: [f32; 4], size: Pixels) -> impl IntoElement {
    div().flex_none().w(size).h(size).rounded(px(2.)).bg(swatch_color(color))
}

/// Builds the thickness tooltip element for `tooltip`, reproducing the egui
/// app's `show_armor_tooltip` V1 subset (`tab.rs:2778-2820`): a swatch +
/// "{mm} mm" header, then "{zone} / {material}" from the caller's labels in
/// the reader's own language -- then, only when there is more than one
/// layer, a
/// "N layers for this part:" label and a swatch + "{mm} mm" row per layer
/// (bold when that layer matches the plate's total thickness). The
/// penetration-check section (`tab.rs:2822-2884`) is out of scope until the
/// analysis subsystem lands; this tooltip never shows it.
pub fn tooltip_element(
    tooltip: &ArmorTriangleTooltip,
    background: Hsla,
    border: Hsla,
    radius: Pixels,
    muted: Hsla,
    zone_label: &str,
    material_label: &str,
) -> AnyElement {
    let mut body = v_flex()
        .gap_1()
        .child(h_flex().gap_1().items_center().child(swatch(tooltip.color, px(12.))).child(
            div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(format!("{:.0} mm", tooltip.thickness_mm)),
        ))
        .child(div().text_sm().child(format!("{zone_label} / {material_label}")));

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

    div()
        .bg(background)
        .border_1()
        .border_color(border)
        .rounded(radius)
        .shadow_md()
        .px_2()
        .py_1()
        .child(body)
        .into_any_element()
}

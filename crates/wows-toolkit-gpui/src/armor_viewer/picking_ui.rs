//! CPU plate picking support: maps a [`HitResult`] to its tooltip data, ports
//! the egui app's hover-highlight overlay upload (`upload_plate_highlight`,
//! `armor_viewer/ui/tab.rs:2889-2960`), the sidebar-hover highlight uploads
//! (`upload_zone_highlight`/`upload_part_highlight`, `tab.rs:2959-3080`), and
//! builds the floating thickness tooltip element (`show_armor_tooltip`,
//! `tab.rs:2778-2820` -- the V1 subset only; the penetration-check section,
//! `tab.rs:2822-2884`, is deferred with the analysis subsystem and is never
//! shown here). `viewport_view.rs` wires all of these into the mouse-move/
//! click handlers and the visibility popover's row-hover callbacks, and owns
//! the actual `part_visibility`/`plate_visibility`/hover state.

use gpui::AnyElement;
use gpui::FontWeight;
use gpui::Hsla;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Pixels;
use gpui::Styled;
use gpui::div;
use gpui::px;
use gpui_component::h_flex;
use gpui_component::v_flex;

use wowsunpack::export::gltf_export::thickness_to_color;

use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::HitResult;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vertex;

use super::legend::swatch_color;
use super::load_ship::ArmorTriangleTooltip;
use super::load_ship::LoadedShipArmor;
use super::load_ship::PlateKey;
use super::load_ship::transform_normal;
use super::load_ship::transform_point;
use super::upload;
use super::visibility::VisibilityFilter;

/// `armor_viewer::constants::TRAJECTORY_NORMAL_OFFSET`: offsets the hover
/// highlight overlay along each vertex normal so it renders in front of the
/// plate it highlights instead of z-fighting with it. The egui original
/// reuses the trajectory-overlay constant for this same purpose (`tab.rs:2903`).
const TRAJECTORY_NORMAL_OFFSET: f32 = 0.01;

/// Fixed highlight overlay color/opacity, matching the egui call site
/// (`tab.rs:5427`: `upload_plate_highlight(pane, &armor, key, device, [1.0, 1.0, 1.0, 0.35])`).
pub const HOVER_HIGHLIGHT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.35];

/// Looks up the tooltip for a pick hit: finds the `(MeshId, Vec<ArmorTriangleTooltip>)`
/// entry whose id matches the hit's mesh, then indexes by `triangle_index`.
/// Mirrors the egui app's own lookup (`tab.rs:5327-5331`).
pub fn tooltip_for_hit<'a>(
    hit: &HitResult,
    mesh_triangle_info: &'a [(MeshId, Vec<ArmorTriangleTooltip>)],
) -> Option<&'a ArmorTriangleTooltip> {
    mesh_triangle_info.iter().find(|(id, _)| *id == hit.mesh_id).and_then(|(_, infos)| infos.get(hit.triangle_index))
}

/// Derives a triangle's `PlateKey` (zone, material_name, thickness rounded to
/// tenths of mm) from its tooltip. Matches `upload.rs`'s `derive_plate_key`
/// (same formula, different source struct: a tooltip instead of the raw
/// `ArmorTriangleInfo`) and the egui app's own inline derivation (`tab.rs:5334-5335`).
pub fn plate_key_of(tooltip: &ArmorTriangleTooltip) -> PlateKey {
    (tooltip.zone.clone(), tooltip.material_name.clone(), (tooltip.thickness_mm * 10.0).round() as i32)
}

/// Uploads one overlay mesh containing every visible triangle whose plate key
/// matches `key`, offset along its normal so it renders in front of the
/// plate. Applies the same 0mm/part/plate filtering as `upload.rs`'s main
/// upload (via `upload::plate_is_visible`), so a hidden plate highlights to
/// an (empty) no-op mesh. Ports `armor_viewer::ui::tab::upload_plate_highlight`
/// verbatim.
pub fn upload_plate_highlight(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    key: &PlateKey,
    visibility: VisibilityFilter,
    show_zero_mm: bool,
) -> MeshId {
    upload_highlight_mesh(viewport, device, armor, visibility, show_zero_mm, HOVER_HIGHLIGHT_COLOR, |info| {
        &upload::derive_plate_key(info) == key
    })
}

/// Fixed color for the sidebar-hover highlight (a popover row's Zone/Part/
/// Plate hover), matching the egui call site's own constant
/// (`ui::tab::SIDEBAR_HIGHLIGHT_COLOR`, `tab.rs:2956`) -- same value as
/// `HOVER_HIGHLIGHT_COLOR` (the two highlights never overlap: one is
/// raycast-driven, the other popover-hover-driven, tracked separately in
/// `viewport_view.rs` as `hover_highlight`/`sidebar_highlight`).
pub const SIDEBAR_HIGHLIGHT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.35];

/// Uploads a highlight overlay for every visible armor triangle in `zone`.
/// Ports `armor_viewer::ui::tab::upload_zone_highlight` verbatim.
pub fn upload_zone_highlight(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    zone: &str,
    visibility: VisibilityFilter,
    show_zero_mm: bool,
) -> MeshId {
    upload_highlight_mesh(viewport, device, armor, visibility, show_zero_mm, SIDEBAR_HIGHLIGHT_COLOR, |info| {
        info.zone == zone
    })
}

/// Uploads a highlight overlay for every visible armor triangle matching
/// `(zone, material)`. Ports `armor_viewer::ui::tab::upload_part_highlight` verbatim.
pub fn upload_part_highlight(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    zone: &str,
    material: &str,
    visibility: VisibilityFilter,
    show_zero_mm: bool,
) -> MeshId {
    upload_highlight_mesh(viewport, device, armor, visibility, show_zero_mm, SIDEBAR_HIGHLIGHT_COLOR, |info| {
        info.zone == zone && info.material_name == material
    })
}

/// Shared highlight-mesh builder: every visible triangle (per `visibility`
/// and `show_zero_mm`, same 0mm/part/plate filtering as the main upload)
/// that also matches `matches`, offset along its normal so the overlay
/// renders in front of the plate it highlights instead of z-fighting with it.
fn upload_highlight_mesh(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    armor: &LoadedShipArmor,
    visibility: VisibilityFilter,
    show_zero_mm: bool,
    color: [f32; 4],
    matches: impl Fn(&wowsunpack::export::gltf_export::ArmorTriangleInfo) -> bool,
) -> MeshId {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for mesh in &armor.meshes {
        for (tri_idx, info) in mesh.triangle_info.iter().enumerate() {
            if !upload::plate_is_visible(info, visibility, show_zero_mm) {
                continue;
            }
            if !matches(info) {
                continue;
            }

            let base_idx = tri_idx * 3;
            if base_idx + 2 >= mesh.indices.len() {
                continue;
            }

            let new_base = vertices.len() as u32;
            for k in 0..3 {
                let orig_idx = mesh.indices[base_idx + k] as usize;
                if orig_idx < mesh.positions.len() {
                    let mut pos = mesh.positions[orig_idx];
                    let mut norm = mesh.normals[orig_idx];

                    if let Some(t) = &mesh.transform {
                        pos = transform_point(t, pos);
                        norm = transform_normal(t, norm);
                    }

                    pos[0] += norm[0] * TRAJECTORY_NORMAL_OFFSET;
                    pos[1] += norm[1] * TRAJECTORY_NORMAL_OFFSET;
                    pos[2] += norm[2] * TRAJECTORY_NORMAL_OFFSET;

                    vertices.push(Vertex { position: pos, normal: norm, color, uv: [0.0, 0.0] });
                }
            }
            indices.extend_from_slice(&[new_base, new_base + 1, new_base + 2]);
        }
    }

    viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// A small filled, rounded color swatch, matching `legend.rs`'s own swatch
/// convention (a plain colored `div`, not `paint_swatch`'s painter call --
/// there is no `egui::Painter` equivalent needed here).
fn swatch(color: [f32; 4], size: Pixels) -> impl IntoElement {
    div().flex_none().w(size).h(size).rounded(px(2.)).bg(swatch_color(color))
}

/// Builds the thickness tooltip element for `tooltip`, reproducing the egui
/// app's `show_armor_tooltip` V1 subset (`tab.rs:2778-2820`): a swatch +
/// "{mm} mm" header, then "{zone} / {material}" -- material shown raw since
/// this port has no IDS_ translation wired yet (matching its existing
/// convention elsewhere) -- then, only when there is more than one layer, a
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
) -> AnyElement {
    let mut body = v_flex()
        .gap_1()
        .child(h_flex().gap_1().items_center().child(swatch(tooltip.color, px(12.))).child(
            div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(format!("{:.0} mm", tooltip.thickness_mm)),
        ))
        .child(div().text_sm().child(format!("{} / {}", tooltip.zone, tooltip.material_name)));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::types::Vec3;

    fn test_tooltip(zone: &str, material: &str, thickness_mm: f32, layers: Vec<f32>) -> ArmorTriangleTooltip {
        ArmorTriangleTooltip {
            material_name: material.to_string(),
            zone: zone.to_string(),
            thickness_mm,
            layers,
            color: [0.5, 0.5, 0.5, 1.0],
        }
    }

    #[test]
    fn plate_key_of_rounds_thickness_to_tenths_of_a_mm() {
        let tooltip = test_tooltip("Citadel", "Cit_Belt", 32.04, vec![32.04]);
        assert_eq!(plate_key_of(&tooltip), ("Citadel".to_string(), "Cit_Belt".to_string(), 320));
    }

    #[test]
    fn tooltip_for_hit_finds_the_matching_mesh_and_triangle() {
        let mesh_a = MeshId(0);
        let mesh_b = MeshId(1);
        let mesh_triangle_info = vec![
            (mesh_a, vec![test_tooltip("Bow", "Bow_Bottom", 16.0, vec![16.0])]),
            (
                mesh_b,
                vec![
                    test_tooltip("Citadel", "Cit_Belt", 32.0, vec![32.0]),
                    test_tooltip("Citadel", "Cit_Deck", 20.0, vec![20.0]),
                ],
            ),
        ];

        let hit = HitResult { mesh_id: mesh_b, triangle_index: 1, distance: 1.0, world_position: Vec3::zeros() };
        let found = tooltip_for_hit(&hit, &mesh_triangle_info).expect("expected a tooltip match");
        assert_eq!(found.zone, "Citadel");
        assert_eq!(found.material_name, "Cit_Deck");
    }

    #[test]
    fn tooltip_for_hit_returns_none_for_an_unknown_mesh() {
        let mesh_triangle_info = vec![(MeshId(0), vec![test_tooltip("Bow", "Bow_Bottom", 16.0, vec![16.0])])];
        let hit = HitResult { mesh_id: MeshId(9), triangle_index: 0, distance: 1.0, world_position: Vec3::zeros() };
        assert!(tooltip_for_hit(&hit, &mesh_triangle_info).is_none());
    }

    #[test]
    fn tooltip_for_hit_returns_none_for_an_out_of_range_triangle_index() {
        let mesh_triangle_info = vec![(MeshId(0), vec![test_tooltip("Bow", "Bow_Bottom", 16.0, vec![16.0])])];
        let hit = HitResult { mesh_id: MeshId(0), triangle_index: 5, distance: 1.0, world_position: Vec3::zeros() };
        assert!(tooltip_for_hit(&hit, &mesh_triangle_info).is_none());
    }
}

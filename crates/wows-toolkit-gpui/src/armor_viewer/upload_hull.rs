//! Uploads a [`LoadedShipArmor`]'s hull meshes into a [`Viewport3D`],
//! separately from the armor upload (`upload.rs`) so a hull-only visibility
//! or opacity change never rebuilds the (typically much larger) armor mesh
//! set. Ports the egui app's `upload_hull_meshes_to_viewport`
//! (`armor_viewer/ui/tab.rs:1339-1442`).
//!
//! **Camo compositing (Milestone 4 Task 8b).** The active camo's decoded
//! textures/UV transforms are not carried on `armor` (an `Arc<LoadedShipArmor>`
//! is never mutated once loaded) -- `ViewportView` owns that state
//! (`selected_camo`/`active_camo_textures`/`active_camo_uvs`) and passes it in
//! as parameters here on every hull upload. A stem present in
//! `active_camo_textures` takes precedence over `armor.hull_textures`'s base
//! albedo for that mesh; empty maps (no camo selected) reproduce Task 8a's
//! base-albedo-only behavior exactly.
//!
//! Hull-upgrade/LOD reload (Task 8c) is out of scope; this always uploads
//! whatever `armor.hull_meshes` currently holds. The sidebar-hover highlight
//! for a hovered hull row (egui's `SidebarHighlightKey::HullMeshes`) is
//! deferred -- this port's `SidebarHighlightKey` intentionally omits that
//! variant for now (see `visibility.rs`'s module doc); the hull popover's own
//! hover therefore has no 3D highlight yet, only the checkbox state.

use std::collections::HashMap;

use wowsunpack::export::camouflage::UvTransform;

use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::LAYER_DEFAULT;
use crate::viewport::renderer::LAYER_HULL;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vertex;

use super::load_ship::LoadedShipArmor;
use super::load_ship::transform_normal;
use super::load_ship::transform_point;

/// Hull-mesh vertex-color brightness boost for baked colors; the lighting
/// shader multiplies this base by `(flat + key*halfLambert)`. Matches the
/// egui original's `hull_brightness` constant (`tab.rs:1374`).
const HULL_BRIGHTNESS: f32 = 2.0;

/// Fallback per-texture brightness when a hull mesh has no texture data (or a
/// texture with no sampled pixels). Matches the egui original's
/// `unwrap_or(3.5)` (`tab.rs:1395`).
const FALLBACK_TEX_BRIGHTNESS: f32 = 3.5;

/// Target P95 luma [`tex_brightness`] scales toward. Matches the egui
/// original's `TARGET_HI` constant (`tab.rs:1392`).
const TARGET_HI: f32 = 0.85;

/// Sample every 37th RGBA pixel when computing a texture's P95 luma -- cheap
/// enough to run per-upload, dense enough for a stable percentile. Matches
/// the egui original's `step_by(37)` (`tab.rs:1384`).
const LUMA_SAMPLE_STRIDE: usize = 37;

/// Extracts the file stem (no directory, no `.mfm` extension) from an mfm
/// path, for matching a hull mesh's texture against `active_camo_textures`'s
/// keys. Ports `armor_viewer::common::mfm_stem` verbatim.
pub(crate) fn mfm_stem(mfm_path: &str) -> &str {
    let base = mfm_path.rsplit(['/', '\\']).next().unwrap_or(mfm_path);
    base.strip_suffix(".mfm").unwrap_or(base)
}

/// Computes a texture's brightness boost from the 95th-percentile luma of a
/// sampled subset of its pixels, scaling that percentile toward `TARGET_HI`
/// and clamping to `[1.0, 4.0]`. Pure function of `rgba` so it is testable
/// without a GPU. Matches the egui original's inline computation
/// (`tab.rs:1380-1395`) exactly, including the luma weights and stride: a
/// mean-based factor over-boosts a mostly-dark texture that has bright spots
/// (e.g. a camo with light stripes on a dark base); keying off the P95 bright
/// pixels caps the boost by the brightest regions directly, so they do not
/// saturate to white while dark textures still get boosted.
pub(crate) fn tex_brightness(rgba: &[u8]) -> f32 {
    let mut lumas: Vec<f32> = rgba
        .chunks_exact(4)
        .step_by(LUMA_SAMPLE_STRIDE)
        .map(|px| (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0)
        .collect();
    if lumas.is_empty() {
        return FALLBACK_TEX_BRIGHTNESS;
    }
    lumas.sort_by(|a, b| a.total_cmp(b));
    let p95 = lumas[(lumas.len() * 95 / 100).min(lumas.len() - 1)];
    (TARGET_HI / p95.max(0.05)).clamp(1.0, 4.0)
}

/// Re-uploads `armor`'s hull meshes: removes every previously-uploaded hull
/// mesh (`hull_mesh_ids`, drained and replaced in place) and re-adds only the
/// ones `hull_visibility` marks visible (absent/false = hidden, matching the
/// egui app's own default). `hull_opaque` selects the alpha/layer: opaque
/// hulls draw depth-written on [`LAYER_DEFAULT`] like armor plates,
/// translucent ones draw on [`LAYER_HULL`] (no depth write, behind armor).
/// `active_camo_textures`/`active_camo_uvs` are `ViewportView`'s decoded
/// active-camo state (see the module doc); pass empty maps for base albedo
/// only.
///
/// Shares `viewport`'s mesh pool with the armor upload (`upload.rs`) but
/// never calls [`Viewport3D::clear`] itself, so an armor-only change never
/// touches the hull and vice versa -- except that the ARMOR path's own
/// `clear()` does wipe every mesh including the hull's, which
/// `viewport_view.rs::reupload_current_armor` resolves by calling this
/// function again right after, using the (unaffected) `hull_visibility`/
/// `hull_opaque` state.
#[allow(clippy::too_many_arguments)]
pub fn upload_hull_meshes(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
    armor: &LoadedShipArmor,
    hull_mesh_ids: &mut Vec<MeshId>,
    hull_visibility: &HashMap<String, bool>,
    hull_opaque: bool,
    active_camo_textures: &HashMap<String, (u32, u32, Vec<u8>)>,
    active_camo_uvs: &HashMap<String, UvTransform>,
) {
    for mid in hull_mesh_ids.drain(..) {
        viewport.remove_mesh(mid);
    }

    let hull_alpha: f32 = if hull_opaque { 1.0 } else { 0.7 };
    let hull_layer = if hull_opaque { LAYER_DEFAULT } else { LAYER_HULL };

    for mesh in &armor.hull_meshes {
        let visible = hull_visibility.get(&mesh.name).copied().unwrap_or(false);
        if !visible {
            continue;
        }

        let has_uvs = mesh.uvs.len() == mesh.positions.len();
        let stem = mesh.mfm_path.as_deref().map(mfm_stem);
        // Active camo texture for this stem takes precedence; otherwise the base albedo.
        let texture_data = stem
            .and_then(|s| active_camo_textures.get(s))
            .or_else(|| mesh.mfm_path.as_ref().and_then(|p| armor.hull_textures.get(p)));
        // Only apply a tiled UV transform when this stem actually has an
        // active camo texture; otherwise the mesh falls back to base albedo
        // with its own UVs.
        let camo_uv = stem.filter(|s| active_camo_textures.contains_key(*s)).and_then(|s| active_camo_uvs.get(s));
        let has_texture = texture_data.is_some() && has_uvs;

        let brightness = texture_data.map(|(_, _, rgba)| tex_brightness(rgba)).unwrap_or(FALLBACK_TEX_BRIGHTNESS);
        let fallback_color: [f32; 4] =
            [0.6 * HULL_BRIGHTNESS, 0.6 * HULL_BRIGHTNESS, 0.65 * HULL_BRIGHTNESS, hull_alpha];
        let has_baked_colors = mesh.colors.len() == mesh.positions.len();

        let mut vertices: Vec<Vertex> = Vec::with_capacity(mesh.positions.len());
        for i in 0..mesh.positions.len() {
            let mut pos = mesh.positions[i];
            let mut norm = if i < mesh.normals.len() { mesh.normals[i] } else { [0.0, 1.0, 0.0] };

            if let Some(t) = &mesh.transform {
                pos = transform_point(t, pos);
                norm = transform_normal(t, norm);
            }

            let uv = if has_uvs {
                let base_uv = mesh.uvs[i];
                match camo_uv {
                    Some(t) => [base_uv[0] * t.scale[0] + t.offset[0], base_uv[1] * t.scale[1] + t.offset[1]],
                    None => base_uv,
                }
            } else {
                [0.0, 0.0]
            };

            let color = if has_texture {
                [brightness, brightness, brightness, hull_alpha]
            } else if has_baked_colors {
                let c = mesh.colors[i];
                [c[0] * HULL_BRIGHTNESS, c[1] * HULL_BRIGHTNESS, c[2] * HULL_BRIGHTNESS, hull_alpha]
            } else {
                fallback_color
            };
            vertices.push(Vertex { position: pos, normal: norm, color, uv });
        }

        if mesh.indices.is_empty() {
            continue;
        }
        let mid = if let Some((w, h, rgba)) = texture_data.filter(|_| has_uvs) {
            let tex_bind_group = pipeline.create_texture_bind_group(device, queue, rgba, *w, *h);
            viewport.add_textured_non_pickable_mesh(device, &vertices, &mesh.indices, hull_layer, tex_bind_group)
        } else {
            viewport.add_non_pickable_mesh(device, &vertices, &mesh.indices, hull_layer)
        };
        viewport.set_lit(mid, true);
        hull_mesh_ids.push(mid);
    }

    viewport.mark_dirty();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_rgba(pixel: [u8; 4], count: usize) -> Vec<u8> {
        pixel.iter().copied().cycle().take(count * 4).collect()
    }

    #[test]
    fn mfm_stem_strips_directory_and_extension() {
        assert_eq!(mfm_stem("content/gameplay/hull/Hull_A.mfm"), "Hull_A");
        assert_eq!(mfm_stem("content\\gameplay\\hull\\Hull_B.mfm"), "Hull_B");
        assert_eq!(mfm_stem("NoExtension"), "NoExtension");
    }

    #[test]
    fn tex_brightness_falls_back_when_the_texture_is_empty() {
        assert_eq!(tex_brightness(&[]), FALLBACK_TEX_BRIGHTNESS);
    }

    #[test]
    fn tex_brightness_clamps_to_one_for_an_already_bright_texture() {
        // Every sampled pixel is near-white (luma ~1.0): TARGET_HI / 1.0 < 1.0,
        // so the clamp floor of 1.0 applies (never darkens a texture).
        let rgba = solid_rgba([255, 255, 255, 255], LUMA_SAMPLE_STRIDE * 4);
        assert_eq!(tex_brightness(&rgba), 1.0);
    }

    #[test]
    fn tex_brightness_clamps_to_four_for_a_very_dark_texture() {
        // Every sampled pixel is near-black (luma ~0.0): p95.max(0.05) floors
        // the denominator at 0.05, giving 0.85 / 0.05 = 17.0, clamped to 4.0.
        let rgba = solid_rgba([0, 0, 0, 255], LUMA_SAMPLE_STRIDE * 4);
        assert_eq!(tex_brightness(&rgba), 4.0);
    }

    #[test]
    fn tex_brightness_targets_the_p95_luma_for_a_mid_gray_texture() {
        // A uniform luma L means p95 == L (every sample is the same), so the
        // boost is exactly TARGET_HI / L for an L that keeps the ratio inside
        // [1.0, 4.0] -- pick L = 0.5 so 0.85 / 0.5 = 1.7, comfortably clamped.
        let rgba = solid_rgba([128, 128, 128, 255], LUMA_SAMPLE_STRIDE * 4);
        let luma: f32 = (0.2126 * 128.0 + 0.7152 * 128.0 + 0.0722 * 128.0) / 255.0;
        let expected = (TARGET_HI / luma.max(0.05)).clamp(1.0, 4.0);
        assert!((tex_brightness(&rgba) - expected).abs() < 1e-4);
    }
}

//! Gap detection: the openings in an armor hull a shell can pass through
//! without meeting a plate.
//!
//! An armor model is a triangle soup, so a hole in it shows up as an edge
//! belonging to exactly one triangle. Most such edges are not holes: they are
//! the rim of a plate that simply ends, or a seam too narrow for a shell. The
//! two filters here are the egui app's (`ui/tab.rs`'s `upload_gap_edges`):
//! an edge longer than [`MAX_GAP_EDGE_LENGTH`] is model structure rather than
//! a gap, and an edge with another boundary edge within [`MIN_GAP_WIDTH`] is
//! one side of a seam rather than an opening.
//!
//! The geometry is built here and uploaded by the caller, so what counts as a
//! gap is testable without a GPU.

use std::collections::HashMap;

use super::load_ship::LoadedShipArmor;

use crate::viewport::types::Vertex;

use super::upload::plate_is_visible;
use super::visibility::VisibilityFilter;

/// Half the width of the ribbon a gap edge is drawn as.
const GAP_EDGE_HALF_WIDTH: f32 = 0.006;

/// How far the ribbon is lifted off the surface, so it is not z-fought by the
/// plate it runs along.
const GAP_EDGE_NORMAL_OFFSET: f32 = 0.008;

/// The tone a gap is drawn in: this is a hole in the armor, not a note.
const GAP_COLOR: [f32; 4] = [1.0, 0.15, 0.1, 1.0];

/// An edge longer than this is the model's own structure rather than an
/// opening, in metres.
const MAX_GAP_EDGE_LENGTH: f32 = 5.0;

/// How far apart two boundary edges must be before what lies between them is
/// an opening rather than a seam, in metres.
const MIN_GAP_WIDTH: f32 = 0.12;

/// One edge of the armor surface, and which way that surface faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EdgeData {
    pub p0: [f32; 3],
    pub p1: [f32; 3],
    /// The face normal of the triangle the edge was first seen on, which is
    /// what the ribbon is lifted along.
    pub normal: [f32; 3],
}

impl EdgeData {
    fn length(&self) -> f32 {
        distance(self.p0, self.p1)
    }

    fn midpoint(&self) -> [f32; 3] {
        [(self.p0[0] + self.p1[0]) * 0.5, (self.p0[1] + self.p1[1]) * 0.5, (self.p0[2] + self.p1[2]) * 0.5]
    }
}

/// An edge named by its endpoints, rounded so two triangles that meet along
/// it agree on the name.
///
/// Ordered, so the same edge walked in either direction is one key.
type EdgeKey = ([i32; 3], [i32; 3]);

/// Tenths of a millimetre, which is finer than any seam the model draws and
/// coarse enough that two triangles meeting at a shared vertex round to it.
const QUANTIZE_SCALE: f32 = 10_000.0;

fn quantize(point: [f32; 3]) -> [i32; 3] {
    [
        (point[0] * QUANTIZE_SCALE).round() as i32,
        (point[1] * QUANTIZE_SCALE).round() as i32,
        (point[2] * QUANTIZE_SCALE).round() as i32,
    ]
}

fn edge_key(a: [i32; 3], b: [i32; 3]) -> EdgeKey {
    if a < b { (a, b) } else { (b, a) }
}

/// The gap ribbons for `armor`, and how many gaps they draw.
///
/// Only the triangles the viewport is currently showing are walked: hiding a
/// plate opens a hole in the model that is not a hole in the ship, and
/// counting it would report the reader's own filter back to them.
pub(crate) fn build_gap_mesh(
    armor: &LoadedShipArmor,
    visibility: VisibilityFilter,
    show_zero_mm: bool,
) -> (Vec<Vertex>, Vec<u32>, usize) {
    let edges = boundary_edges(armor, visibility, show_zero_mm);
    let open = wide_enough_to_pass(&edges);
    let (vertices, indices) = ribbons(&open);
    (vertices, indices, open.len())
}

/// Every edge of the shown surface that belongs to exactly one triangle, and
/// is short enough to be an opening rather than structure.
fn boundary_edges(armor: &LoadedShipArmor, visibility: VisibilityFilter, show_zero_mm: bool) -> Vec<EdgeData> {
    let mut seen: HashMap<EdgeKey, (usize, EdgeData)> = HashMap::new();

    for mesh in &armor.meshes {
        for (triangle, info) in mesh.triangle_info.iter().enumerate() {
            if !plate_is_visible(info, visibility, show_zero_mm) {
                continue;
            }
            let Some(corners) = triangle_corners(mesh, triangle) else { continue };
            let normal = face_normal(corners);

            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let key = edge_key(quantize(corners[a]), quantize(corners[b]));
                seen.entry(key)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, EdgeData { p0: corners[a], p1: corners[b], normal }));
            }
        }
    }

    seen.into_values()
        .filter(|(count, _)| *count == 1)
        .map(|(_, edge)| edge)
        .filter(|edge| {
            let length = edge.length();
            length > 1e-6 && length <= MAX_GAP_EDGE_LENGTH
        })
        .collect()
}

/// The boundary edges with no other boundary edge within [`MIN_GAP_WIDTH`].
///
/// A seam has a matching edge on its far side a few millimetres away; a real
/// opening has nothing that close.
fn wide_enough_to_pass(edges: &[EdgeData]) -> Vec<EdgeData> {
    let min_gap_sq = MIN_GAP_WIDTH * MIN_GAP_WIDTH;

    edges
        .iter()
        .enumerate()
        .filter(|(ix, edge)| {
            let from = edge.midpoint();
            !edges.iter().enumerate().any(|(other_ix, other)| {
                other_ix != *ix && point_to_segment_dist_sq(from, other.p0, other.p1) <= min_gap_sq
            })
        })
        .map(|(_, edge)| *edge)
        .collect()
}

/// A flat ribbon along each edge, drawn on both faces so it is visible from
/// either side of the hull.
fn ribbons(edges: &[EdgeData]) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for edge in edges {
        let direction = [edge.p1[0] - edge.p0[0], edge.p1[1] - edge.p0[1], edge.p1[2] - edge.p0[2]];
        let Some(tangent) = normalize(cross(direction, edge.normal)) else { continue };

        for face in [1.0_f32, -1.0] {
            let base = vertices.len() as u32;
            let lift = [
                edge.normal[0] * GAP_EDGE_NORMAL_OFFSET * face,
                edge.normal[1] * GAP_EDGE_NORMAL_OFFSET * face,
                edge.normal[2] * GAP_EDGE_NORMAL_OFFSET * face,
            ];
            let normal = [edge.normal[0] * face, edge.normal[1] * face, edge.normal[2] * face];

            for end in [edge.p0, edge.p1] {
                for side in [-1.0_f32, 1.0] {
                    vertices.push(Vertex {
                        position: [
                            end[0] + tangent[0] * GAP_EDGE_HALF_WIDTH * side + lift[0],
                            end[1] + tangent[1] * GAP_EDGE_HALF_WIDTH * side + lift[1],
                            end[2] + tangent[2] * GAP_EDGE_HALF_WIDTH * side + lift[2],
                        ],
                        normal,
                        color: GAP_COLOR,
                        uv: [0.0, 0.0],
                    });
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    (vertices, indices)
}

/// One triangle's three corners in world space. `None` when the mesh's own
/// indices do not reach them, which a truncated model can produce.
fn triangle_corners(
    mesh: &wowsunpack::export::gltf_export::InteractiveArmorMesh,
    triangle: usize,
) -> Option<[[f32; 3]; 3]> {
    let base = triangle * 3;
    let mut corners = [[0.0_f32; 3]; 3];
    for (corner, slot) in corners.iter_mut().enumerate() {
        let index = *mesh.indices.get(base + corner)? as usize;
        let mut position = *mesh.positions.get(index)?;
        if let Some(transform) = &mesh.transform {
            position = transform_point(transform, position);
        }
        *slot = position;
    }
    Some(corners)
}

fn face_normal(corners: [[f32; 3]; 3]) -> [f32; 3] {
    let e1 = [corners[1][0] - corners[0][0], corners[1][1] - corners[0][1], corners[1][2] - corners[0][2]];
    let e2 = [corners[2][0] - corners[0][0], corners[2][1] - corners[0][1], corners[2][2] - corners[0][2]];
    // A degenerate triangle has no facing; up is as good as any, and the
    // ribbon it would produce is discarded by the tangent check anyway.
    normalize(cross(e1, e2)).unwrap_or([0.0, 1.0, 0.0])
}

/// Applies a column-major 4x4 transform to a point.
fn transform_point(t: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        t[0] * p[0] + t[4] * p[1] + t[8] * p[2] + t[12],
        t[1] * p[0] + t[5] * p[1] + t[9] * p[2] + t[13],
        t[2] * p[0] + t[6] * p[1] + t[10] * p[2] + t[14],
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length >= 1e-10).then(|| [v[0] / length, v[1] / length, v[2] / length])
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Squared distance from `p` to the segment `a`-`b`.
///
/// Squared because every caller compares it against a squared threshold; a
/// square root per edge pair is the inner loop of the gap filter.
fn point_to_segment_dist_sq(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
    let ab_len_sq = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    if ab_len_sq < 1e-12 {
        return ap[0] * ap[0] + ap[1] * ap[1] + ap[2] * ap[2];
    }
    let along = ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / ab_len_sq).clamp(0.0, 1.0);
    let closest = [a[0] + along * ab[0], a[1] + along * ab[1], a[2] + along * ab[2]];
    let d = [p[0] - closest[0], p[1] - closest[1], p[2] - closest[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(p0: [f32; 3], p1: [f32; 3]) -> EdgeData {
        EdgeData { p0, p1, normal: [0.0, 1.0, 0.0] }
    }

    /// Two edges facing each other across a seam narrower than a shell are
    /// not an opening, however many of them there are.
    #[test]
    fn a_seam_narrower_than_the_threshold_is_not_a_gap() {
        let seam = MIN_GAP_WIDTH * 0.5;
        let edges = vec![edge([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), edge([0.0, 0.0, seam], [1.0, 0.0, seam])];

        assert!(wide_enough_to_pass(&edges).is_empty());
    }

    /// The same two edges far enough apart are both openings.
    #[test]
    fn edges_further_apart_than_the_threshold_are_gaps() {
        let apart = MIN_GAP_WIDTH * 2.0;
        let edges = vec![edge([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), edge([0.0, 0.0, apart], [1.0, 0.0, apart])];

        assert_eq!(wide_enough_to_pass(&edges).len(), 2);
    }

    /// A lone boundary edge has nothing to be a seam with.
    #[test]
    fn a_single_boundary_edge_is_a_gap() {
        assert_eq!(wide_enough_to_pass(&[edge([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])]).len(), 1);
    }

    /// An edge is measured from its own midpoint to the whole of every other
    /// edge, not to their midpoints.
    ///
    /// The rule is therefore asymmetric, which is what this pins: the short
    /// edge is refused because the long one passes close by it, while the
    /// long one is kept because its own midpoint is nowhere near the short
    /// one. That is the egui app's rule, kept as it is so the two apps report
    /// the same count.
    #[test]
    fn an_edge_is_measured_from_its_midpoint_to_the_whole_of_another() {
        let close = MIN_GAP_WIDTH * 0.5;
        let short = edge([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        // Runs away from the short edge, passing close to its midpoint at one
        // end and far from it at the other.
        let long = edge([0.5, 0.0, close], [0.5, 0.0, 10.0]);

        let open = wide_enough_to_pass(&[short, long]);
        assert_eq!(open, vec![long], "the short edge has the long one beside it; the long one does not");
    }

    /// Both faces are drawn, so a gap is visible from either side of the
    /// hull: four corners per face, two triangles per face.
    #[test]
    fn one_gap_is_drawn_on_both_faces() {
        let (vertices, indices) = ribbons(&[edge([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])]);

        assert_eq!(vertices.len(), 8);
        assert_eq!(indices.len(), 12);
    }

    /// An edge running along its own face normal has no surface to lay a
    /// ribbon on, and is left undrawn rather than drawn as a sliver.
    #[test]
    fn an_edge_with_no_surface_to_lie_on_is_not_drawn() {
        let along_the_normal = EdgeData { p0: [0.0, 0.0, 0.0], p1: [0.0, 1.0, 0.0], normal: [0.0, 1.0, 0.0] };
        let (vertices, indices) = ribbons(&[along_the_normal]);

        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }

    #[test]
    fn an_edge_is_named_the_same_from_either_end() {
        let a = quantize([1.0, 2.0, 3.0]);
        let b = quantize([4.0, 5.0, 6.0]);
        assert_eq!(edge_key(a, b), edge_key(b, a));
    }
}

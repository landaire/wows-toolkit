//! Marking where shells landed on a hull.
//!
//! A hit is a point, and a point is not visible on a model the size of a
//! ship, so each one is drawn as a small cross of three bars along the axes.
//! A cross rather than a sphere because it reads at any angle without being
//! mistaken for part of the ship, and because its size is a setting rather
//! than a mesh.

use nalgebra::Vector3;
use wows_replay_insights::hull_impact;
use wows_replay_insights::timeline::PreExtractedHit;

use crate::viewport::types::Vertex;

type Vec3 = Vector3<f32>;

/// How long each arm of a marker is, in metres.
const ARM: f32 = 1.2;

/// How thick the bars are drawn.
const THICKNESS: f32 = 0.22;

/// What a marker is coloured. One colour for every hit: what kind of hit it
/// was is the penetration checker's answer, not a marker's, and colouring
/// them here would say something this does not know.
const COLOR: [f32; 4] = [1.0, 0.25, 0.15, 1.0];

/// Markers for every hit that can be placed on the hull.
///
/// A hit whose victim was not being watched at that moment carries no pose,
/// and [`hull_impact::hull_impact`] refuses it rather than guessing where it
/// landed, so it is left unmarked instead of marked in the wrong place.
pub(crate) fn build_markers(
    hits: &[PreExtractedHit],
    model_center: Vec3,
    bounds: Option<(Vec3, Vec3)>,
) -> (Vec<Vertex>, Vec<u32>) {
    let center = [model_center.x, model_center.y, model_center.z];
    let bounds = bounds.map(|(low, high)| ([low.x, low.y, low.z], [high.x, high.y, high.z]));

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for hit in hits {
        let Some(at) = hull_impact::hull_impact(&hit.hit, center, bounds) else { continue };
        push_marker(&mut vertices, &mut indices, Vec3::new(at[0], at[1], at[2]));
    }
    (vertices, indices)
}

/// One cross, as three boxes about `at`.
fn push_marker(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, at: Vec3) {
    for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
        push_bar(vertices, indices, at, axis);
    }
}

/// A bar along `axis`, drawn as two crossed quads so it is visible from any
/// angle rather than vanishing edge-on.
fn push_bar(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, at: Vec3, axis: Vec3) {
    let from = at - axis * ARM;
    let to = at + axis * ARM;
    // Any axis not parallel to the bar will do to get a perpendicular from.
    let reference = if axis[1].abs() < 0.9 { Vec3::y() } else { Vec3::x() };
    let across = axis.cross(&reference).normalize();
    let up = axis.cross(&across).normalize();
    let normal: [f32; 3] = across.into();

    for perpendicular in [across, up] {
        let offset = perpendicular * (THICKNESS * 0.5);
        let base = vertices.len() as u32;
        for corner in [from - offset, from + offset, to + offset, to - offset] {
            vertices.push(Vertex { position: corner.into(), normal, color: COLOR, uv: [0.0, 0.0] });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each hit is one cross of three bars, and each bar two crossed quads.
    const PER_HIT: usize = 3 * 2 * 6;

    /// A marker is centred on where the hit landed and reaches an arm's
    /// length along each axis, so it is visible from any angle.
    #[test]
    fn a_marker_is_a_cross_about_the_hit() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_marker(&mut vertices, &mut indices, Vec3::new(1.0, 2.0, 3.0));

        assert_eq!(indices.len(), PER_HIT);
        for axis in 0..3 {
            let low = vertices.iter().map(|v| v.position[axis]).fold(f32::MAX, f32::min);
            let high = vertices.iter().map(|v| v.position[axis]).fold(f32::MIN, f32::max);
            let middle = [1.0, 2.0, 3.0][axis];
            assert!((high - middle - ARM).abs() < 1e-4, "reaches an arm along axis {axis}");
            assert!((middle - low - ARM).abs() < 1e-4);
        }
    }

    /// A bar of no thickness would still be drawn, which is what makes an
    /// edge-on marker disappear, so each is two quads at right angles.
    #[test]
    fn a_bar_is_two_quads_at_right_angles() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_bar(&mut vertices, &mut indices, Vec3::zeros(), Vec3::x());

        assert_eq!(vertices.len(), 8, "two quads");
        assert_eq!(indices.len(), 12);
        // The two quads spread along different axes, or they would be one.
        let spread_y = vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        let spread_z = vertices.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
        assert!(spread_y > 0.0 && spread_z > 0.0, "y {spread_y}, z {spread_z}");
    }

    /// Nothing to mark draws nothing, rather than an empty mesh the viewport
    /// would still upload.
    #[test]
    fn no_hits_draw_nothing() {
        let (vertices, indices) = build_markers(&[], Vec3::zeros(), None);
        assert!(vertices.is_empty() && indices.is_empty());
    }
}

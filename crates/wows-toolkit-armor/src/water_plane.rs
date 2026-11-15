//! A translucent water surface at a ship's waterline, so the armor above and below it reads
//! against the water.

use crate::viewport::renderer::LAYER_HULL;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;

/// How far the water reaches past the ship's middle, in ship lengths and beams.
const WATER_REACH: f32 = 2.25;

/// The water's colour before its opacity.
const WATER_RGB: [f32; 3] = [0.1, 0.4, 0.8];

/// Lays the water under a ship of `bounds`. In world space, so it stays level while the hull
/// heels over. On the hull's layer, which is tested against the armor's depth without writing its
/// own, so the armor above the water hides it.
pub fn upload_water_plane(viewport: &mut Viewport3D, device: &wgpu::Device, bounds: (Vec3, Vec3), opacity: f32) {
    let (vertices, indices) = build_water_plane(bounds, opacity);
    viewport.add_world_space_mesh(device, &vertices, &indices, LAYER_HULL);
}

/// One quad at height zero, where a ship's model puts its waterline, centred under `bounds` and
/// reaching well past them so its edge stays out of a framed view.
pub fn build_water_plane(bounds: (Vec3, Vec3), opacity: f32) -> (Vec<Vertex>, Vec<u32>) {
    let (low, high) = bounds;
    let middle = (low + high) * 0.5;
    let reach_x = (high.x - low.x) * WATER_REACH;
    let reach_z = (high.z - low.z) * WATER_REACH;
    let color = [WATER_RGB[0], WATER_RGB[1], WATER_RGB[2], opacity];
    let corner = |x: f32, z: f32| Vertex {
        position: [x, 0.0, z],
        normal: [0.0, 1.0, 0.0],
        color,
        uv: [0.0, 0.0],
        tangent: [0.0; 4],
    };

    let vertices = vec![
        corner(middle.x - reach_x, middle.z - reach_z),
        corner(middle.x + reach_x, middle.z - reach_z),
        corner(middle.x + reach_x, middle.z + reach_z),
        corner(middle.x - reach_x, middle.z + reach_z),
    ];

    (vertices, vec![0, 1, 2, 0, 2, 3])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The water lies at the waterline under the ship's middle, reaches past both ends and sides,
    /// and carries the opacity asked for.
    #[test]
    fn the_water_lies_level_under_the_ship_and_reaches_past_it() {
        let bounds = (Vec3::new(-2.0, -1.0, -10.0), Vec3::new(4.0, 3.0, 30.0));

        let (vertices, indices) = build_water_plane(bounds, 0.4);

        assert_eq!(indices, [0, 1, 2, 0, 2, 3]);
        assert!(vertices.iter().all(|vertex| vertex.position[1] == 0.0), "level at the waterline");
        assert!(vertices.iter().all(|vertex| vertex.color[3] == 0.4));
        let xs: Vec<f32> = vertices.iter().map(|vertex| vertex.position[0]).collect();
        let zs: Vec<f32> = vertices.iter().map(|vertex| vertex.position[2]).collect();
        assert_eq!(xs, [1.0 - 13.5, 1.0 + 13.5, 1.0 + 13.5, 1.0 - 13.5], "centred on the beam, 2.25 beams each way");
        assert_eq!(zs, [10.0 - 90.0, 10.0 - 90.0, 10.0 + 90.0, 10.0 + 90.0], "centred on the length");
    }
}

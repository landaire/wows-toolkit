//! Per-vertex tangents for normal mapping, derived from a mesh's UV layout.
//! The game's vertex formats do not always carry tangents, and deriving them
//! from the placed positions keeps them right through every transform and
//! mirror applied on the way.

/// One tangent per vertex: each triangle's direction of increasing U,
/// accumulated over the triangles sharing the vertex and made orthogonal to
/// its normal. `w` is the sign that turns `cross(normal, tangent)` toward
/// decreasing V, the top of the texture, where a normal map's +Y points.
///
/// A vertex whose triangles have no UV gradient gets a zero tangent, which
/// the shader leaves unperturbed. `normals` and `uvs` must match `positions`
/// in length; triangles indexing past them are skipped.
pub fn vertex_tangents(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
) -> Vec<[f32; 4]> {
    let count = positions.len().min(normals.len()).min(uvs.len());
    let mut along_u = vec![[0.0_f32; 3]; positions.len()];
    let mut along_v = vec![[0.0_f32; 3]; positions.len()];

    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|index| index as usize);
        if a >= count || b >= count || c >= count {
            continue;
        }

        let edge1 = sub(positions[b], positions[a]);
        let edge2 = sub(positions[c], positions[a]);
        let [du1, dv1] = [uvs[b][0] - uvs[a][0], uvs[b][1] - uvs[a][1]];
        let [du2, dv2] = [uvs[c][0] - uvs[a][0], uvs[c][1] - uvs[a][1]];
        let determinant = du1 * dv2 - du2 * dv1;
        if determinant.abs() < f32::EPSILON {
            continue;
        }

        let r = 1.0 / determinant;
        let triangle_u = std::array::from_fn::<f32, 3, _>(|axis| (edge1[axis] * dv2 - edge2[axis] * dv1) * r);
        let triangle_v = std::array::from_fn::<f32, 3, _>(|axis| (edge2[axis] * du1 - edge1[axis] * du2) * r);
        for vertex in [a, b, c] {
            along_u[vertex] = add(along_u[vertex], triangle_u);
            along_v[vertex] = add(along_v[vertex], triangle_v);
        }
    }

    let mut tangents = vec![[0.0; 4]; positions.len()];
    for vertex in 0..count {
        let normal = normals[vertex];
        let u = along_u[vertex];
        let Some(tangent) = normalize(sub(u, scale(normal, dot(normal, u)))) else {
            continue;
        };

        // Up the texture is toward decreasing V.
        let handedness = if dot(cross(normal, tangent), along_v[vertex]) > 0.0 { -1.0 } else { 1.0 };
        tangents[vertex] = [tangent[0], tangent[1], tangent[2], handedness];
    }

    tangents
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], factor: f32) -> [f32; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(a, a).sqrt();

    (length > 1e-12).then(|| scale(a, 1.0 / length))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit quad in the XY plane facing +Z, two triangles.
    const QUAD_POSITIONS: [[f32; 3]; 4] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    const QUAD_NORMALS: [[f32; 3]; 4] = [[0.0, 0.0, 1.0]; 4];
    const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

    fn assert_tangents(actual: &[[f32; 4]], expected: [f32; 4]) {
        for tangent in actual {
            for axis in 0..4 {
                assert!((tangent[axis] - expected[axis]).abs() < 1e-5, "{actual:?} != {expected:?}");
            }
        }
    }

    #[test]
    fn an_upright_texture_has_u_along_x_and_up_along_y() {
        // V grows downward in the image, so the top row (y = 1) has v = 0.
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

        let tangents = vertex_tangents(&QUAD_POSITIONS, &QUAD_NORMALS, &uvs, &QUAD_INDICES);

        assert_tangents(&tangents, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_horizontally_mirrored_texture_flips_the_tangent_and_its_sign() {
        let uvs = [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]];

        let tangents = vertex_tangents(&QUAD_POSITIONS, &QUAD_NORMALS, &uvs, &QUAD_INDICES);

        // cross(+Z, -X) is -Y, but the top of the texture is still +Y.
        assert_tangents(&tangents, [-1.0, 0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_quarter_turned_texture_runs_u_along_y() {
        // U grows with y and V with x, so the top of the texture (v = 0) faces -X.
        let uvs = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];

        let tangents = vertex_tangents(&QUAD_POSITIONS, &QUAD_NORMALS, &uvs, &QUAD_INDICES);

        // cross(+Z, +Y) is -X, which is up the texture.
        assert_tangents(&tangents, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_tangent_is_made_orthogonal_to_a_tilted_vertex_normal() {
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let tilted = std::f32::consts::FRAC_1_SQRT_2;
        let normals = [[tilted, 0.0, tilted]; 4];

        let tangents = vertex_tangents(&QUAD_POSITIONS, &normals, &uvs, &QUAD_INDICES);

        assert_tangents(&tangents, [tilted, 0.0, -tilted, 1.0]);
    }

    #[test]
    fn a_quad_with_collapsed_uvs_gets_no_tangent() {
        let uvs = [[0.5, 0.5]; 4];

        let tangents = vertex_tangents(&QUAD_POSITIONS, &QUAD_NORMALS, &uvs, &QUAD_INDICES);

        assert_eq!(tangents, vec![[0.0; 4]; 4]);
    }
}

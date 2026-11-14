//! Triangle winding and handedness of loaded and exported models over a real
//! install. Ignored: needs an install named by `WOWS_DIR`; returns early when
//! that directory is absent.

use std::path::PathBuf;
use std::sync::LazyLock;

use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;

use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::LoadedModel;
use wowsunpack::export::model_library::ModelArmorMaps;
use wowsunpack::export::model_library::ModelExportOptions;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::texture::TextureLod;

const HOCKEY_PLAYER: &str = "content/gameplay/common/character/CH005_Hockey_Player_8/CH005_Hockey_Player_8.model";
const IOWA: &str = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";
const YAMATO_TURRET: &str = "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model";
const AIRCRAFT: &str = "content/gameplay/europe/aircraft/scout/WAS001_IMAM_Ro_43/WAS001_IMAM_Ro_43.model";

/// Where the hockey player's albedo holds the name OVECHKIN and the 8 under
/// it, as `[u_min, v_min, u_max, v_max]`. The lettering is upright with v
/// running down the image.
const JERSEY_LETTERING: [f32; 4] = [0.07, 0.73, 0.18, 0.90];

const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

static LIBRARY: LazyLock<Option<ModelLibrary>> = LazyLock::new(|| {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return None;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");
    Some(assets.model_library().expect("model library"))
});

/// A triangle list with every vertex in one space.
struct Surface {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Surface {
    /// `mesh` with its transform applied.
    fn from_mesh(mesh: &InteractiveHullMesh) -> Self {
        let transform = mesh.transform.unwrap_or(IDENTITY);

        Self {
            positions: mesh.positions.iter().map(|&p| transform_point(&transform, p)).collect(),
            normals: mesh.normals.iter().map(|&n| transform_direction(&transform, n)).collect(),
            uvs: mesh.uvs.clone(),
            indices: mesh.indices.clone(),
        }
    }

    fn triangles(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        self.indices.as_chunks::<3>().0.iter().map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
    }

    /// The summed vertex normals of `triangle`, which point out of the surface.
    fn outward(&self, [a, b, c]: [usize; 3]) -> [f32; 3] {
        add(add(self.normals[a], self.normals[b]), self.normals[c])
    }
}

/// Triangles whose counterclockwise front face points out of the surface,
/// along their vertex normals, and those whose front face points in.
#[derive(Debug, Default, Clone, Copy)]
struct Facing {
    outward: usize,
    inward: usize,
}

impl Facing {
    /// `surface`'s triangles. `reflected` marks a surface placed by a
    /// reflecting transform, which glTF draws clockwise.
    fn add(&mut self, surface: &Surface, reflected: bool) {
        for triangle in surface.triangles() {
            let [a, b, c] = triangle.map(|i| surface.positions[i]);
            let front = cross(sub(b, a), sub(c, a));
            let alignment = dot(front, surface.outward(triangle));
            let alignment = if reflected { -alignment } else { alignment };
            if alignment > 0.0 {
                self.outward += 1;
            } else if alignment < 0.0 {
                self.inward += 1;
            }
        }
    }

    fn outward_share(&self) -> f64 {
        self.outward as f64 / (self.outward + self.inward).max(1) as f64
    }
}

/// Triangles of the jersey lettering that read left to right seen from
/// outside, and those that read mirrored.
#[derive(Debug, Default)]
struct Lettering {
    forward: usize,
    mirrored: usize,
}

impl Lettering {
    fn add(&mut self, surface: &Surface) {
        let [u_min, v_min, u_max, v_max] = JERSEY_LETTERING;
        if surface.uvs.len() != surface.positions.len() {
            return;
        }

        for triangle in surface.triangles() {
            let [ua, ub, uc] = triangle.map(|i| surface.uvs[i]);
            let center = [(ua[0] + ub[0] + uc[0]) / 3.0, (ua[1] + ub[1] + uc[1]) / 3.0];
            if !(u_min..u_max).contains(&center[0]) || !(v_min..v_max).contains(&center[1]) {
                continue;
            }

            // dP/du is the reading direction and dP/dv points down the
            // letters, so text seen from outside reads forward when
            // dP/du x dP/dv points into the surface.
            let [a, b, c] = triangle.map(|i| surface.positions[i]);
            let (e1, e2) = (sub(b, a), sub(c, a));
            let (du1, dv1, du2, dv2) = (ub[0] - ua[0], ub[1] - ua[1], uc[0] - ua[0], uc[1] - ua[1]);
            let area = du1 * dv2 - du2 * dv1;
            if area == 0.0 {
                continue;
            }
            let along_u = [0, 1, 2].map(|k| (e1[k] * dv2 - e2[k] * dv1) / area);
            let along_v = [0, 1, 2].map(|k| (e2[k] * du1 - e1[k] * du2) / area);
            if dot(cross(along_u, along_v), surface.outward(triangle)) < 0.0 {
                self.forward += 1;
            } else {
                self.mirrored += 1;
            }
        }
    }

    fn assert_reads_forward(&self, source: &str) {
        let total = self.forward + self.mirrored;
        assert!(total >= 20, "{source}: only {total} triangles carry the lettering");
        assert!(self.forward * 20 >= total * 19, "{source}: the lettering reads mirrored: {self:?}");
    }
}

fn path(raw: &str) -> ModelPath {
    ModelPath::parse(raw).expect("valid model path")
}

fn load(library: &ModelLibrary, raw: &str) -> LoadedModel {
    let options = ModelLoadOptions { textures: TextureDecode::None, ..ModelLoadOptions::default() };
    library.load_model(&path(raw), &options).unwrap_or_else(|error| panic!("{raw} loads: {error}"))
}

/// `model`'s triangles as the viewer draws them: it applies mesh transforms
/// to the vertices and keeps the winding, reflecting or not.
fn facing(model: &LoadedModel) -> Facing {
    let mut facing = Facing::default();
    for mesh in model.meshes() {
        facing.add(&Surface::from_mesh(mesh), false);
    }

    facing
}

fn transform_point(m: &[f32; 16], [x, y, z]: [f32; 3]) -> [f32; 3] {
    add(transform_direction(m, [x, y, z]), [m[12], m[13], m[14]])
}

/// `direction` under the linear part of `m`, which is a rotation for every
/// mesh transform, so normals take it as positions do.
fn transform_direction(m: &[f32; 16], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [m[0] * x + m[4] * y + m[8] * z, m[1] * x + m[5] * y + m[9] * z, m[2] * x + m[6] * y + m[10] * z]
}

fn determinant(m: &[f32; 16]) -> f32 {
    dot([m[0], m[1], m[2]], cross([m[4], m[5], m[6]], [m[8], m[9], m[10]]))
}

fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }

    out
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A character, a ship hull, a plain and a reflected turret, and an aircraft
/// all draw counterclockwise front faces on the outside.
#[test]
#[ignore = "requires a World of Warships install"]
fn models_face_out_counterclockwise() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    for raw in [HOCKEY_PLAYER, IOWA, IOWA_TURRET, YAMATO_TURRET, AIRCRAFT] {
        let facing = facing(&load(library, raw));
        eprintln!("{raw}: {facing:?}");
        assert!(facing.outward_share() >= 0.99, "{raw}: {facing:?}");
    }
}

/// The hockey player is skinned to blend bones that reflect him at rest. The
/// name and number on his back read forward from behind him, not mirrored.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_reflected_character_s_lettering_reads_forward() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let mut lettering = Lettering::default();
    for mesh in load(library, HOCKEY_PLAYER).meshes() {
        lettering.add(&Surface::from_mesh(mesh));
    }

    lettering.assert_reads_forward(HOCKEY_PLAYER);
}

/// The hockey player's GLB faces out and reads forward as the viewer's
/// meshes do, with node transforms applied as glTF does.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_reflected_character_s_glb_faces_out_and_reads_forward() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let options = ModelExportOptions {
        lod: 0,
        damaged: false,
        texture_lod: TextureLod::Full,
        textures: false,
        contents: ExportContents::Mesh,
        hardpoints: false,
    };
    let mut glb = Vec::new();
    library.export_glb(&path(HOCKEY_PLAYER), &options, ModelArmorMaps::default(), &mut glb).expect("export");
    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    let blob = gltf.blob.as_deref().expect("a binary chunk");

    let mut facing = Facing::default();
    let mut lettering = Lettering::default();
    let scene = gltf.document.default_scene().expect("a default scene");
    let mut pending: Vec<(gltf::Node<'_>, [f32; 16])> = scene.nodes().map(|node| (node, IDENTITY)).collect();
    while let Some((node, parent)) = pending.pop() {
        let local: [f32; 16] = node.transform().matrix().as_flattened().try_into().expect("a 4x4 matrix");
        let world = mat4_mul(&parent, &local);
        pending.extend(node.children().map(|child| (child, world)));

        let Some(mesh) = node.mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|_| Some(blob));
            let surface = Surface {
                positions: reader.read_positions().expect("positions").map(|p| transform_point(&world, p)).collect(),
                normals: reader.read_normals().expect("normals").map(|n| transform_direction(&world, n)).collect(),
                uvs: reader.read_tex_coords(0).map(|uvs| uvs.into_f32().collect()).unwrap_or_default(),
                indices: reader.read_indices().expect("indices").into_u32().collect(),
            };
            facing.add(&surface, determinant(&world) < 0.0);
            lettering.add(&surface);
        }
    }

    eprintln!("{HOCKEY_PLAYER} GLB: {facing:?}, {lettering:?}");
    assert!(facing.outward_share() >= 0.99, "{facing:?}");
    lettering.assert_reads_forward("the GLB");
}

/// Every catalogued model of any size faces out counterclockwise. A few
/// small ones, such as the inward-facing map borders, are authored the other
/// way round.
#[test]
#[ignore = "requires a World of Warships install"]
fn every_model_faces_out_counterclockwise() {
    const MIN_TRIANGLES: usize = 100;

    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let threads = std::thread::available_parallelism().map_or(1, |count| (count.get() / 2).max(1));
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");

    let options = ModelLoadOptions { textures: TextureDecode::None, ..ModelLoadOptions::default() };
    let facings: Vec<(&ModelPath, Facing)> = pool.install(|| {
        library
            .catalog()
            .entries()
            .par_iter()
            // A load failure is the load survey's concern, not this one's.
            .filter_map(|entry| {
                library.load_model(entry.path(), &options).ok().map(|model| (entry.path(), facing(&model)))
            })
            .collect()
    });

    let total = facings.iter().fold(Facing::default(), |sum, (_, facing)| Facing {
        outward: sum.outward + facing.outward,
        inward: sum.inward + facing.inward,
    });
    let inside_out: Vec<String> = facings
        .iter()
        .filter(|(_, facing)| facing.outward + facing.inward >= MIN_TRIANGLES && facing.inward > facing.outward)
        .map(|(path, facing)| format!("{path}: {facing:?}"))
        .collect();
    eprintln!("{} models: {total:?}, {} inside out", facings.len(), inside_out.len());
    assert!(inside_out.is_empty(), "{inside_out:#?}");
    assert!(total.outward_share() >= 0.99, "{total:?}");
}

//! Hover picking for a scene's meshes: hull sections, mounts, misc parts and
//! the render sets of a model shown on its own. The loaders build a
//! [`HullPickIndex`] off the UI thread from the same transformed triangles the
//! hull upload draws, so a hover only walks a bounding volume hierarchy.

use std::collections::HashMap;

use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::FontWeight;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement;
use gpui_kit::Styled;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::px;
use gpui_wgpu::wgpu;
use rust_i18n::t;
use wowsunpack::export::armor_ray::ray_triangle_intersect;
use wowsunpack::export::armor_ray::transform_point;
use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::model_catalog::MiscCategory;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelSubKind;
use wowsunpack::export::model_library::ModelPart;
use wowsunpack::export::part_group::PartGroup;
use wowsunpack::export::ship_parts::ShipPart;
use wowsunpack::export::ship_parts::ShipPartKey;
use wowsunpack::game_params::types::ShipModelDistance;

use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;

use super::load_ship::transform_normal;
use super::sidebar_tree::model_sub_kind_label;

/// A mesh's position in `LoadedShipArmor::hull_meshes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HullMeshIndex(usize);

impl HullMeshIndex {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn get(self) -> usize {
        self.0
    }
}

/// Where a mesh comes from, as its hover tooltip names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshSource {
    /// The `.model` the mesh belongs to, when the store names one.
    pub model: Option<ModelPath>,
    /// The ship part the mesh draws, when it is placed on a ship or building.
    pub part: Option<ShipPartKey>,
}

impl MeshSource {
    /// Each of a ship's meshes with the model of the part it draws.
    pub fn for_ship(meshes: &[InteractiveHullMesh], parts: &[ShipPart]) -> Vec<Self> {
        let by_key: HashMap<&ShipPartKey, &ShipPart> = parts.iter().map(|part| (&part.key, part)).collect();

        meshes
            .iter()
            .map(|mesh| Self {
                model: mesh.part.as_ref().and_then(|key| by_key.get(key)).and_then(|part| part.model.clone()),
                part: mesh.part.clone(),
            })
            .collect()
    }

    /// Each of a loaded model's `mesh_count` meshes with the `.model` that
    /// contributed it, all placed as `part`.
    pub fn for_model(model_parts: &[ModelPart], mesh_count: usize, part: Option<&ShipPartKey>) -> Vec<Self> {
        let mut sources = vec![Self { model: None, part: part.cloned() }; mesh_count];
        for model_part in model_parts {
            for source in sources.get_mut(model_part.meshes.clone()).into_iter().flatten() {
                source.model = Some(model_part.model_path.clone());
            }
        }

        sources
    }

    /// The node the part sits at: a mount's hardpoint, or a misc part's node
    /// behind its parent. `None` on a hull section or a model shown alone.
    pub fn hardpoint(&self) -> Option<String> {
        match self.part.as_ref()? {
            ShipPartKey::HullSection(_) => None,
            key @ (ShipPartKey::Mount(_) | ShipPartKey::Misc { .. }) => Some(key.label()),
        }
    }
}

/// The nearest mesh a ray crosses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshHit {
    pub mesh: HullMeshIndex,
    /// Along the ray, for a unit direction.
    pub distance: ShipModelDistance,
}

/// A scene's mesh triangles in model space, with a bounding volume hierarchy
/// over them, and where each mesh comes from.
pub struct HullPickIndex {
    /// Ordered so that every leaf covers one contiguous run.
    triangles: Vec<PickTriangle>,
    /// The root first. Empty when there are no triangles.
    nodes: Vec<BvhNode>,
    /// One per mesh, by [`HullMeshIndex`].
    sources: Vec<MeshSource>,
}

impl HullPickIndex {
    /// Indexes `meshes` with their transforms applied, as the hull upload
    /// places them. `sources` holds one entry per mesh, in the same order.
    pub fn build(meshes: &[InteractiveHullMesh], sources: Vec<MeshSource>) -> Self {
        debug_assert_eq!(meshes.len(), sources.len(), "one source per mesh");

        let mut triangles = Vec::new();
        for (index, mesh) in meshes.iter().enumerate() {
            let positions: Vec<[f32; 3]> = match &mesh.transform {
                Some(transform) => mesh.positions.iter().map(|at| transform_point(transform, *at)).collect(),
                None => mesh.positions.clone(),
            };
            for corners in mesh.indices.chunks_exact(3) {
                let corner = |at: u32| positions.get(at as usize).copied();
                let (Some(a), Some(b), Some(c)) = (corner(corners[0]), corner(corners[1]), corner(corners[2])) else {
                    continue;
                };
                triangles.push(PickTriangle { corners: [a, b, c], mesh: HullMeshIndex(index) });
            }
        }

        let nodes = build_hierarchy(&mut triangles);

        Self { triangles, nodes, sources }
    }

    /// The nearest triangle of a shown mesh the ray from `origin` along
    /// `direction` (model space) crosses. `shown` is indexed by
    /// [`HullMeshIndex`]; a mesh past its end is not shown.
    pub fn pick(&self, origin: Vec3, direction: Vec3, shown: &[bool]) -> Option<MeshHit> {
        // Each split halves its run, so no path from the root is longer than
        // the bits of a triangle count, and the worklist holds one sibling
        // per level plus the node being split.
        const MAX_PENDING: usize = usize::BITS as usize + 2;

        if self.nodes.is_empty() || !shown.contains(&true) {
            return None;
        }

        let origin_array: [f32; 3] = origin.into();
        let direction_array: [f32; 3] = direction.into();
        let mut nearest: Option<MeshHit> = None;
        let mut pending = [0_usize; MAX_PENDING];
        let mut pending_len = 1;

        while pending_len > 0 {
            pending_len -= 1;
            let node = &self.nodes[pending[pending_len]];
            let Some(enter) = node.bounds.entry(&origin, &direction) else {
                continue;
            };
            if nearest.is_some_and(|hit| hit.distance.value() < enter) {
                continue;
            }

            match node.content {
                NodeContent::Leaf { start, end } => {
                    for triangle in &self.triangles[start..end] {
                        if !shown.get(triangle.mesh.0).copied().unwrap_or(false) {
                            continue;
                        }
                        let Some(distance) = ray_triangle_intersect(origin_array, direction_array, &triangle.corners)
                        else {
                            continue;
                        };
                        if nearest.is_none_or(|hit| distance < hit.distance.value()) {
                            nearest = Some(MeshHit { mesh: triangle.mesh, distance: ShipModelDistance::new(distance) });
                        }
                    }
                }
                NodeContent::Inner { left, right } => {
                    pending[pending_len] = left;
                    pending[pending_len + 1] = right;
                    pending_len += 2;
                }
            }
        }

        nearest
    }

    pub fn source(&self, mesh: HullMeshIndex) -> Option<&MeshSource> {
        self.sources.get(mesh.0)
    }
}

/// One triangle in model space and the mesh it belongs to.
struct PickTriangle {
    corners: [[f32; 3]; 3],
    mesh: HullMeshIndex,
}

impl PickTriangle {
    fn centroid(&self, axis: usize) -> f32 {
        (self.corners[0][axis] + self.corners[1][axis] + self.corners[2][axis]) / 3.0
    }

    fn centroid_point(&self) -> [f32; 3] {
        [self.centroid(0), self.centroid(1), self.centroid(2)]
    }
}

#[derive(Clone, Copy, Debug)]
struct Aabb {
    low: Vec3,
    high: Vec3,
}

impl Aabb {
    fn around(points: impl Iterator<Item = [f32; 3]>) -> Self {
        let mut low = Vec3::repeat(f32::INFINITY);
        let mut high = Vec3::repeat(f32::NEG_INFINITY);
        for point in points {
            let point = Vec3::from(point);
            low = low.inf(&point);
            high = high.sup(&point);
        }

        Self { low, high }
    }

    /// Where along the ray it enters the box, by the slab test: zero when it
    /// starts inside, `None` when it misses or the box is behind it.
    fn entry(&self, origin: &Vec3, direction: &Vec3) -> Option<f32> {
        let mut near = 0.0_f32;
        let mut far = f32::INFINITY;
        for axis in 0..3 {
            // A ray parallel to a slab pair runs between them for its whole
            // length or misses them entirely.
            if direction[axis] == 0.0 {
                if origin[axis] < self.low[axis] || origin[axis] > self.high[axis] {
                    return None;
                }
                continue;
            }
            let inverse = 1.0 / direction[axis];
            let first = (self.low[axis] - origin[axis]) * inverse;
            let second = (self.high[axis] - origin[axis]) * inverse;
            near = near.max(first.min(second));
            far = far.min(first.max(second));
            if near > far {
                return None;
            }
        }

        Some(near)
    }

    fn longest_axis(&self) -> (usize, f32) {
        let extent = self.high - self.low;
        (0..3).map(|axis| (axis, extent[axis])).fold(
            (0, f32::NEG_INFINITY),
            |best, each| {
                if each.1 > best.1 { each } else { best }
            },
        )
    }
}

struct BvhNode {
    bounds: Aabb,
    content: NodeContent,
}

#[derive(Clone, Copy)]
enum NodeContent {
    /// Triangles `start..end`.
    Leaf { start: usize, end: usize },
    /// Node indices of the two halves.
    Inner { left: usize, right: usize },
}

/// Orders `triangles` and builds the hierarchy over them, splitting each
/// node at the median centroid along its longest axis.
fn build_hierarchy(triangles: &mut [PickTriangle]) -> Vec<BvhNode> {
    const LEAF_SIZE: usize = 8;

    if triangles.is_empty() {
        return Vec::new();
    }

    let placeholder = || BvhNode {
        bounds: Aabb { low: Vec3::zeros(), high: Vec3::zeros() },
        content: NodeContent::Leaf { start: 0, end: 0 },
    };
    let mut nodes = vec![placeholder()];
    let mut pending = vec![(0, 0, triangles.len())];

    while let Some((node, start, end)) = pending.pop() {
        let run = &mut triangles[start..end];
        let bounds = Aabb::around(run.iter().flat_map(|triangle| triangle.corners));
        let (axis, extent) = Aabb::around(run.iter().map(PickTriangle::centroid_point)).longest_axis();
        // Coincident centroids cannot be told apart by any split.
        if run.len() <= LEAF_SIZE || extent <= 0.0 {
            nodes[node] = BvhNode { bounds, content: NodeContent::Leaf { start, end } };
            continue;
        }

        let half = run.len() / 2;
        run.select_nth_unstable_by(half, |a, b| a.centroid(axis).total_cmp(&b.centroid(axis)));
        let left = nodes.len();
        nodes.push(placeholder());
        let right = nodes.len();
        nodes.push(placeholder());
        nodes[node] = BvhNode { bounds, content: NodeContent::Inner { left, right } };
        pending.push((left, start, start + half));
        pending.push((right, start + half, end));
    }

    nodes
}

/// Which meshes `hull_visibility` shows, by [`HullMeshIndex`]. A name with no
/// entry is hidden, as the hull upload treats it.
pub fn shown_meshes(meshes: &[InteractiveHullMesh], hull_visibility: &HashMap<String, bool>) -> Vec<bool> {
    meshes.iter().map(|mesh| hull_visibility.get(&mesh.name) == Some(&true)).collect()
}

/// What the pointer is over, armor first: a plate hit wins over any mesh
/// behind or in front of it, so the plate tools keep working through the hull.
#[derive(Clone, Debug, PartialEq)]
pub enum HoverTarget<P> {
    Plate(P),
    Mesh(MeshHit),
    Nothing,
}

impl<P> HoverTarget<P> {
    /// Picks the mesh only when no plate was hit.
    pub fn resolve(plate: Option<P>, mesh: impl FnOnce() -> Option<MeshHit>) -> Self {
        match plate {
            Some(plate) => Self::Plate(plate),
            None => mesh().map_or(Self::Nothing, Self::Mesh),
        }
    }
}

/// What a pointer move does to the mesh highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightChange {
    /// The same name stays highlighted, or nothing was and nothing is.
    Keep,
    /// Replace any highlight with one over the meshes of this name.
    Show(String),
    Remove,
}

impl HighlightChange {
    /// From the highlighted mesh name to the hovered one.
    pub fn between(highlighted: Option<&str>, hovered: Option<&str>) -> Self {
        match (highlighted, hovered) {
            (Some(was), Some(now)) if was == now => Self::Keep,
            (_, Some(now)) => Self::Show(now.to_string()),
            (Some(_), None) => Self::Remove,
            (None, None) => Self::Keep,
        }
    }
}

/// Overlay geometry for every mesh named in `names`, offset along its
/// normals so it draws in front of the mesh it covers.
pub fn highlight_geometry(
    meshes: &[InteractiveHullMesh],
    names: &[String],
    color: [f32; 4],
) -> (Vec<Vertex>, Vec<u32>) {
    // Matches the plate highlight's own offset in `picking_ui`.
    const NORMAL_OFFSET: f32 = 0.01;

    let named = || meshes.iter().filter(|mesh| names.contains(&mesh.name));
    let corner_count: usize = named().map(|mesh| mesh.indices.len() / 3 * 3).sum();
    let mut vertices: Vec<Vertex> = Vec::with_capacity(corner_count);
    let mut indices: Vec<u32> = Vec::with_capacity(corner_count);
    for mesh in named() {
        let vertex = |at: u32| -> Option<Vertex> {
            let position = *mesh.positions.get(at as usize)?;
            // As the hull upload draws a vertex with no normal.
            let normal = mesh.normals.get(at as usize).copied().unwrap_or([0.0, 1.0, 0.0]);
            let (position, normal) = match &mesh.transform {
                Some(transform) => (transform_point(transform, position), transform_normal(transform, normal)),
                None => (position, normal),
            };
            let position = [
                position[0] + normal[0] * NORMAL_OFFSET,
                position[1] + normal[1] * NORMAL_OFFSET,
                position[2] + normal[2] * NORMAL_OFFSET,
            ];

            Some(Vertex { position, normal, color, uv: [0.0, 0.0] })
        };
        for corners in mesh.indices.chunks_exact(3) {
            let (Some(a), Some(b), Some(c)) = (vertex(corners[0]), vertex(corners[1]), vertex(corners[2])) else {
                continue;
            };

            let base = vertices.len() as u32;
            vertices.extend([a, b, c]);
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
    }

    (vertices, indices)
}

/// Uploads [`highlight_geometry`] as an overlay mesh.
pub fn upload_hull_highlight(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    meshes: &[InteractiveHullMesh],
    names: &[String],
    color: [f32; 4],
) -> MeshId {
    let (vertices, indices) = highlight_geometry(meshes, names, color);

    viewport.add_overlay_mesh(device, &vertices, &indices)
}

/// What the mesh tooltip says. Held untranslated and written in the reader's
/// language when drawn, so a hover outlives a change of language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshTooltip {
    pub name: String,
    pub group: PartGroup,
    /// The kind of a miscellaneous part, read from its model.
    pub misc: Option<MiscCategory>,
    pub model: Option<String>,
    pub hardpoint: Option<String>,
}

impl MeshTooltip {
    pub fn new(mesh: &InteractiveHullMesh, source: &MeshSource) -> Self {
        let misc = match (mesh.group, source.model.as_ref()) {
            (PartGroup::Misc, Some(model)) => Some(MiscCategory::classify(model.stem())),
            _ => None,
        };

        Self {
            name: mesh.name.clone(),
            group: mesh.group,
            misc,
            model: source.model.as_ref().map(|model| model.stem().to_string()),
            hardpoint: source.hardpoint().map(|hardpoint| hardpoint.to_string()),
        }
    }

    /// The group line: the group's name, and a miscellaneous part's kind.
    fn group_label(&self) -> String {
        let named = part_group_label(self.group);
        match self.misc {
            Some(category) => format!("{named} > {}", model_sub_kind_label(&ModelSubKind::Misc(category))),
            None => named,
        }
    }

    pub fn element(&self, cx: &App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let model = self.model.as_ref().map(|model| t!("ui.model_view.part_model", model = model).into_owned());
        let hardpoint = self
            .hardpoint
            .as_ref()
            .map(|hardpoint| t!("ui.model_view.part_hardpoint", hardpoint = hardpoint).into_owned());
        let body = v_flex()
            .gap_1()
            .child(div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(self.name.clone()))
            .child(div().text_sm().child(self.group_label()))
            .children(model.map(|model| div().text_xs().text_color(muted).child(model)))
            .children(hardpoint.map(|hardpoint| div().text_xs().text_color(muted).child(hardpoint)));

        crate::ui::overlay_surface(div(), cx).px_2().py_1().child(body).into_any_element()
    }
}

/// `group`'s name in the reader's language.
fn part_group_label(group: PartGroup) -> String {
    t!(part_group_key(group)).into_owned()
}

const fn part_group_key(group: PartGroup) -> &'static str {
    match group {
        PartGroup::Hull => "ui.model_view.part_group.hull",
        PartGroup::MainBattery => "ui.model_view.part_group.main_battery",
        PartGroup::SecondaryBattery => "ui.model_view.part_group.secondary_battery",
        PartGroup::AaGuns => "ui.model_view.part_group.aa_guns",
        PartGroup::Torpedoes => "ui.model_view.part_group.torpedoes",
        PartGroup::DepthCharges => "ui.model_view.part_group.depth_charges",
        PartGroup::FireControl => "ui.model_view.part_group.fire_control",
        PartGroup::Radar => "ui.model_view.part_group.radar",
        PartGroup::Missiles => "ui.model_view.part_group.missiles",
        PartGroup::Decorations => "ui.model_view.part_group.decorations",
        PartGroup::Misc => "ui.model_view.part_group.misc",
        PartGroup::Armor => "ui.model_view.part_group.armor",
        PartGroup::Other => "ui.model_view.part_group.other",
    }
}

#[cfg(test)]
mod tests {
    use wowsunpack::export::hardpoint::HardpointName;

    use super::*;

    fn quad(name: &str, z: f32, transform: Option<[f32; 16]>) -> InteractiveHullMesh {
        InteractiveHullMesh {
            name: name.to_string(),
            positions: vec![[-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uvs: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
            mfm_path: None,
            mfm_path_id: 0,
            transform,
            group: PartGroup::Hull,
            part: None,
        }
    }

    fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0]
    }

    fn sourceless(meshes: &[InteractiveHullMesh]) -> Vec<MeshSource> {
        meshes.iter().map(|_| MeshSource { model: None, part: None }).collect()
    }

    fn down_the_z_axis(x: f32, y: f32) -> (Vec3, Vec3) {
        (Vec3::new(x, y, 10.0), Vec3::new(0.0, 0.0, -1.0))
    }

    #[test]
    fn a_ray_over_a_mesh_hits_the_nearest_shown_one() {
        let meshes = vec![quad("Far", 0.0, None), quad("Near", 2.0, None)];
        let index = HullPickIndex::build(&meshes, sourceless(&meshes));
        let (origin, direction) = down_the_z_axis(0.25, 0.5);

        let hit = index.pick(origin, direction, &[true, true]).expect("a hit");

        assert_eq!(hit.mesh, HullMeshIndex::new(1));
        assert!((hit.distance.value() - 8.0).abs() < 1e-5);
    }

    #[test]
    fn a_hidden_mesh_is_skipped_for_the_one_behind_it() {
        let meshes = vec![quad("Far", 0.0, None), quad("Near", 2.0, None)];
        let index = HullPickIndex::build(&meshes, sourceless(&meshes));
        let (origin, direction) = down_the_z_axis(0.25, 0.5);

        assert_eq!(index.pick(origin, direction, &[true, false]).map(|hit| hit.mesh), Some(HullMeshIndex::new(0)));
        assert_eq!(index.pick(origin, direction, &[false, false]), None);
        assert_eq!(index.pick(origin, direction, &[]), None, "a mesh past the mask is hidden");
    }

    #[test]
    fn a_placed_mesh_is_hit_where_its_transform_puts_it() {
        let meshes = vec![quad("Turret", 0.0, Some(translation(10.0, 0.0, 0.0)))];
        let index = HullPickIndex::build(&meshes, sourceless(&meshes));

        let (origin, direction) = down_the_z_axis(0.0, 0.0);
        assert_eq!(index.pick(origin, direction, &[true]), None, "nothing is left at the origin");
        let (origin, direction) = down_the_z_axis(10.5, 0.5);
        assert_eq!(index.pick(origin, direction, &[true]).map(|hit| hit.mesh), Some(HullMeshIndex::new(0)));
    }

    #[test]
    fn the_hierarchy_finds_every_mesh_of_a_large_scene() {
        // A grid of meshes far beyond one leaf, each hit through its own centre.
        let meshes: Vec<InteractiveHullMesh> = (0..40)
            .map(|at| {
                let (x, y) = ((at % 8) as f32 * 3.0, (at / 8) as f32 * 3.0);
                quad(&format!("Part{at}"), (at % 5) as f32, Some(translation(x, y, 0.0)))
            })
            .collect();
        let index = HullPickIndex::build(&meshes, sourceless(&meshes));
        let shown = vec![true; meshes.len()];

        for at in 0..meshes.len() {
            let (origin, direction) = down_the_z_axis((at % 8) as f32 * 3.0 + 0.1, (at / 8) as f32 * 3.0 + 0.2);
            let hit = index.pick(origin, direction, &shown).map(|hit| hit.mesh);
            assert_eq!(hit, Some(HullMeshIndex::new(at)), "mesh {at}");
        }
    }

    #[test]
    fn a_ray_past_every_mesh_hits_nothing() {
        let meshes = vec![quad("Hull", 0.0, None)];
        let index = HullPickIndex::build(&meshes, sourceless(&meshes));
        let (origin, direction) = down_the_z_axis(5.0, 5.0);

        assert_eq!(index.pick(origin, direction, &[true]), None);
        assert_eq!(index.pick(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 1.0), &[true]), None, "behind the ray");
    }

    #[test]
    fn a_ray_nearly_parallel_to_a_face_still_enters_the_box() {
        // Starts just below the box and climbs into it too slowly to read as
        // anything but parallel to a coarse test.
        let meshes = vec![quad("Hull", 0.0, Some(translation(0.0, 1.0, 0.0)))];
        let index = HullPickIndex::build(&meshes, sourceless(&meshes));

        let hit = index.pick(Vec3::new(0.0, -1e-9, 10.0), Vec3::new(0.0, 5e-8, -1.0), &[true]);

        assert_eq!(hit.map(|hit| hit.mesh), Some(HullMeshIndex::new(0)));
    }

    #[test]
    fn an_empty_scene_hits_nothing() {
        let index = HullPickIndex::build(&[], Vec::new());
        let (origin, direction) = down_the_z_axis(0.0, 0.0);

        assert_eq!(index.pick(origin, direction, &[]), None);
    }

    #[test]
    fn shown_meshes_follow_hull_visibility_with_absent_names_hidden() {
        let meshes = vec![quad("Shown", 0.0, None), quad("Hidden", 0.0, None), quad("Unlisted", 0.0, None)];
        let visibility = HashMap::from([("Shown".to_string(), true), ("Hidden".to_string(), false)]);

        assert_eq!(shown_meshes(&meshes, &visibility), vec![true, false, false]);
    }

    #[test]
    fn a_plate_hit_wins_over_a_mesh_hit() {
        let mesh = MeshHit { mesh: HullMeshIndex::new(0), distance: ShipModelDistance::new(1.0) };

        assert_eq!(HoverTarget::resolve(Some("plate"), || Some(mesh)), HoverTarget::Plate("plate"));
        assert_eq!(HoverTarget::resolve(None::<&str>, || Some(mesh)), HoverTarget::Mesh(mesh));
        assert_eq!(HoverTarget::resolve(None::<&str>, || None), HoverTarget::Nothing);
    }

    #[test]
    fn the_highlight_is_added_on_a_new_mesh_and_removed_when_the_pointer_leaves() {
        assert_eq!(HighlightChange::between(None, Some("Turret")), HighlightChange::Show("Turret".to_string()));
        assert_eq!(HighlightChange::between(Some("Turret"), Some("Turret")), HighlightChange::Keep);
        assert_eq!(HighlightChange::between(Some("Turret"), Some("Hull")), HighlightChange::Show("Hull".to_string()));
        assert_eq!(HighlightChange::between(Some("Turret"), None), HighlightChange::Remove);
        assert_eq!(HighlightChange::between(None, None), HighlightChange::Keep);
    }

    #[test]
    fn the_highlight_covers_every_mesh_of_the_name_placed_and_raised_off_it() {
        let meshes = vec![
            quad("Turret", 0.0, Some(translation(10.0, 0.0, 0.0))),
            quad("Hull", 0.0, None),
            quad("Turret", 0.0, Some(translation(-10.0, 0.0, 0.0))),
        ];

        let (vertices, indices) = highlight_geometry(&meshes, &["Turret".to_string()], [1.0; 4]);

        assert_eq!(indices.len(), 12, "both turrets' two triangles");
        assert_eq!(vertices.len(), 12);
        assert!(vertices.iter().all(|vertex| vertex.position[0].abs() >= 9.0), "no hull vertex");
        assert!(vertices.iter().all(|vertex| (vertex.position[2] - 0.01).abs() < 1e-6), "raised along the normal");
    }

    #[test]
    fn a_ship_mesh_takes_the_model_of_its_part() {
        let turret = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
        let mut mounted = quad("Turret [HP_AGM_1]", 0.0, None);
        mounted.part = Some(ShipPartKey::Mount(HardpointName::new("HP_AGM_1")));
        let meshes = vec![mounted, quad("Hull", 0.0, None)];
        let parts = vec![ShipPart {
            key: ShipPartKey::Mount(HardpointName::new("HP_AGM_1")),
            model: Some(turret.clone()),
            group: PartGroup::MainBattery,
            placement: None,
            hardpoints: Vec::new(),
        }];

        let sources = MeshSource::for_ship(&meshes, &parts);

        assert_eq!(sources[0].model, Some(turret));
        assert_eq!(sources[0].hardpoint().as_deref(), Some("HP_AGM_1"));
        assert_eq!(sources[1], MeshSource { model: None, part: None });
        assert_eq!(sources[1].hardpoint(), None);
    }

    #[test]
    fn a_model_mesh_takes_the_section_that_contributed_it() {
        let ship = ModelPath::parse("content/gameplay/usa/ship/battleship/ASB001/ASB001.model").expect("a model path");
        let bow =
            ModelPath::parse("content/gameplay/usa/ship/battleship/ASB001/ASB001_Bow.model").expect("a model path");
        let part = |model_path: &ModelPath, meshes: std::ops::Range<usize>| ModelPart {
            model_path: model_path.clone(),
            visual_path: String::new(),
            geometry_path: String::new(),
            lod_count: 1,
            meshes,
        };
        let mount = ShipPartKey::Mount(HardpointName::new("HP_AGM_1"));

        let sources = MeshSource::for_model(&[part(&ship, 0..1), part(&bow, 1..3)], 3, Some(&mount));

        let models: Vec<Option<&ModelPath>> = sources.iter().map(|source| source.model.as_ref()).collect();
        assert_eq!(models, vec![Some(&ship), Some(&bow), Some(&bow)]);
        assert!(sources.iter().all(|source| source.part.as_ref() == Some(&mount)));
    }

    /// The tooltip names a mesh's group in the reader's language, and a
    /// miscellaneous part by its group and kind.
    #[test]
    fn the_tooltip_names_the_group_in_the_readers_language() {
        rust_i18n::set_locale("en");
        let mut guns = quad("AA", 0.0, None);
        guns.group = PartGroup::AaGuns;
        let mut screw = quad("Screw", 0.0, None);
        screw.group = PartGroup::Misc;
        let propeller = ModelPath::parse("content/gameplay/japan/misc/JSP001_Propeller/JSP001_Propeller.model")
            .expect("a model path");

        let unplaced = MeshTooltip::new(&guns, &MeshSource { model: None, part: None });
        let misc = MeshTooltip::new(&screw, &MeshSource { model: Some(propeller), part: None });

        assert_eq!(unplaced.group_label(), "AA guns");
        assert_eq!(misc.group_label(), "Misc > Propellers");
    }

    /// A group with no name in a language would show its key there.
    #[test]
    fn every_group_is_named_in_every_language() {
        use rust_i18n::Backend as _;

        let catalog = wt_translations::TranslationsBackend::load();
        for language in wt_translations::SUPPORTED_LANGUAGES {
            for group in PartGroup::ALL {
                let key = part_group_key(group);
                assert!(catalog.translate(language.code, key).is_some(), "{} lacks {key}", language.code);
            }
        }
    }
}

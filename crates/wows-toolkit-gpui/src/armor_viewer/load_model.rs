//! Background load of one catalogued model into the scene the ship viewer
//! draws: its meshes with their albedo textures, its hardpoints, and the armor
//! it carries, coloured by the GameParams armor of a ship that mounts it.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::Task;
use wowsunpack::data::ResourceLoader;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelArmorMaps;
use wowsunpack::export::model_library::ModelExportOptions;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::mount_armor::MountArmorIndex;
use wowsunpack::export::mount_armor::MountArmorRef;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::texture::TextureLod;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Vehicle;

use super::assets::ModelAssets;
use super::camo::RgbaTexture;
use super::load_ship::ArmorThickness;
use super::load_ship::ArmorZones;
use super::load_ship::ExportGlbError;
use super::load_ship::HullThicknessSource;
use super::load_ship::LoadedShipArmor;
use super::load_ship::ModelSubject;
use super::load_ship::SceneSubject;
use super::load_ship::ThicknessSource;
use super::load_ship::build_hull_part_groups;
use super::load_ship::scene_bounds;
use super::model_refs::ShipPartKey;
use super::model_refs::ShipPartRef;
use super::model_refs::VehicleParamIndex;

/// Longest texture edge decoded for display. The viewport has no use for the
/// 4096 hull atlases at full size, and each costs 64 MiB as RGBA.
const DISPLAY_TEXTURE_EDGE: u32 = 2048;

/// What armor reads as without a thickness: the grey the legend gives no
/// value.
const UNKNOWN_THICKNESS_COLOR: [f32; 4] = [0.55, 0.55, 0.58, 1.0];

/// Why a model could not be shown.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelSceneError {
    #[error("failed to load model {path}: {reason}")]
    Load { path: ModelPath, reason: String },
}

/// Why a ship or a model could not be shown.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SceneLoadError {
    #[error(transparent)]
    Ship(#[from] super::load_ship::ShipLoadError),
    #[error(transparent)]
    Model(#[from] ModelSceneError),
}

/// A model to show, and where its armor thickness should come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLoadRequest {
    pub path: ModelPath,
    pub from_ship: Option<ShipPartRef>,
    /// The mount picked to colour the plates. `None` takes the part's own
    /// ship when it has one.
    pub thickness_from: Option<MountRef>,
    pub lod: usize,
}

/// One ship mount, by the ship's GameParams index and the hardpoint.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MountRef {
    pub vehicle_param_index: VehicleParamIndex,
    pub hp: HardpointName,
}

impl ModelLoadRequest {
    pub fn new(path: ModelPath, from_ship: Option<ShipPartRef>) -> Self {
        Self { path, from_ship, thickness_from: None, lod: 0 }
    }

    /// The mount to colour the plates by: the one picked, else the mount the
    /// part was opened from.
    fn thickness_mount(&self) -> Option<MountRef> {
        if let Some(mount) = &self.thickness_from {
            return Some(mount.clone());
        }

        // A misc part sits on a mount but is not that mount's model, so only
        // a mount names the GameParams armor of the model shown.
        let part = self.from_ship.as_ref()?;
        let ShipPartKey::Mount(hp) = &part.part else { return None };

        Some(MountRef { vehicle_param_index: part.vehicle_param_index.clone(), hp: hp.clone() })
    }
}

/// Where a request's plate thickness is read from.
enum ThicknessRef<'a> {
    /// A ship mount of the model.
    Mount(&'a MountArmorRef),
    /// The hull `armor` map of the ship a hull section was opened from.
    Hull { vehicle: Arc<Param>, vehicle_index: &'a VehicleParamIndex, section: &'a ModelPath },
}

impl ThicknessRef<'_> {
    fn maps(&self) -> ModelArmorMaps<'_> {
        match self {
            Self::Mount(mount) => ModelArmorMaps { hull: mount.hull_armor(), mount: mount.mount_armor() },
            Self::Hull { vehicle, .. } => {
                ModelArmorMaps { hull: vehicle.vehicle().and_then(Vehicle::armor), mount: None }
            }
        }
    }

    /// The source as the rail names it.
    fn thickness(&self, metadata: &GameMetadataProvider) -> ArmorThickness {
        match self {
            Self::Mount(mount) => ArmorThickness::Ship(thickness_source(metadata, mount)),
            Self::Hull { vehicle_index, section, .. } => ArmorThickness::Hull(HullThicknessSource {
                vehicle_param_index: (*vehicle_index).clone(),
                display_name: vehicle_display_name(metadata, vehicle_index),
                section: (*section).clone(),
            }),
        }
    }
}

/// Where `request` reads its plate thickness from: the mount picked, else
/// the part it was opened from, else the first ship mounting the model.
/// `None` when none of them names any armor for it.
fn thickness_ref<'a>(
    index: &'a MountArmorIndex,
    metadata: &GameMetadataProvider,
    request: &'a ModelLoadRequest,
) -> Option<ThicknessRef<'a>> {
    let wanted = request
        .thickness_mount()
        .and_then(|mount| index.find(&request.path, &mount.vehicle_param_index, &mount.hp))
        .filter(|mount| names_armor(mount));
    if let Some(mount) = wanted {
        return Some(ThicknessRef::Mount(mount));
    }

    if let Some(hull) = hull_section_ref(metadata, request) {
        return Some(hull);
    }

    index.referencing(&request.path).iter().find(|mount| names_armor(mount)).map(ThicknessRef::Mount)
}

/// The hull `armor` map of the ship a hull section was opened from. A hull
/// section is no mount, so the mount index never lists it.
fn hull_section_ref<'a>(metadata: &GameMetadataProvider, request: &'a ModelLoadRequest) -> Option<ThicknessRef<'a>> {
    let part = request.from_ship.as_ref()?;
    let ShipPartKey::HullSection(section) = &part.part else { return None };
    let vehicle = metadata.game_param_by_index(part.vehicle_param_index.as_str())?;
    vehicle.vehicle()?.armor()?;

    Some(ThicknessRef::Hull { vehicle, vehicle_index: &part.vehicle_param_index, section })
}

/// Whether `mount` gives any thickness. One without either map would read
/// every plate as 0 mm, which is not a thickness but its absence.
fn names_armor(mount: &MountArmorRef) -> bool {
    mount.mount_armor().is_some() || mount.hull_armor().is_some()
}

/// The ship `vehicle_index` names, as the rail shows it.
fn vehicle_display_name(metadata: &GameMetadataProvider, vehicle_index: &VehicleParamIndex) -> String {
    // A ship GameParams cannot name is still a ship; its index is what there
    // is to show for it.
    metadata
        .game_param_by_index(vehicle_index.as_str())
        .and_then(|param| metadata.localized_name_from_param(&param).or_else(|| Some(param.name().to_string())))
        .unwrap_or_else(|| vehicle_index.to_string())
}

/// `mount` as the rail's picker names it.
fn thickness_source(metadata: &GameMetadataProvider, mount: &MountArmorRef) -> ThicknessSource {
    ThicknessSource {
        vehicle_param_index: mount.vehicle_index().clone(),
        display_name: vehicle_display_name(metadata, mount.vehicle_index()),
        hp: mount.hp().clone(),
    }
}

/// One mount per ship mounting `model`, in the index's order. A ship's
/// mounts of one model share their armor, so listing each would only repeat
/// it.
fn thickness_choices(
    metadata: &GameMetadataProvider,
    index: &MountArmorIndex,
    model: &ModelPath,
) -> Vec<ThicknessSource> {
    let mut seen = HashSet::new();

    index
        .referencing(model)
        .iter()
        .filter(|mount| names_armor(mount) && seen.insert(mount.vehicle_index()))
        .map(|mount| thickness_source(metadata, mount))
        .collect()
}

/// Whether `request`'s plates have a thickness to show or export.
pub(crate) fn thickness_known(
    models: &ModelAssets,
    metadata: &GameMetadataProvider,
    request: &ModelLoadRequest,
) -> bool {
    thickness_ref(&models.mount_armor, metadata, request).is_some()
}

/// Loads `request` from `models`, resolving armor thickness from `metadata`.
/// Synchronous and CPU/IO-bound; callers run it off the UI thread.
pub(crate) fn load_model_scene(
    models: &ModelAssets,
    metadata: &GameMetadataProvider,
    request: &ModelLoadRequest,
) -> Result<LoadedShipArmor, ModelSceneError> {
    let options = ModelLoadOptions {
        lod: request.lod,
        damaged: false,
        texture_lod: TextureLod::from_max_edge(Some(DISPLAY_TEXTURE_EDGE)),
        textures: TextureDecode::Albedo,
    };
    let model = models
        .library
        .load_model(&request.path, &options)
        .map_err(|report| ModelSceneError::Load { path: request.path.clone(), reason: format!("{report:?}") })?;

    let (meshes, thickness, thickness_choices) = match thickness_ref(&models.mount_armor, metadata, request) {
        Some(source) => {
            let maps = source.maps();
            (
                model.armor_meshes(maps.hull, maps.mount),
                source.thickness(metadata),
                thickness_choices(metadata, &models.mount_armor, &request.path),
            )
        }
        None => {
            let mut meshes = model.armor_meshes(None, None);
            paint_unknown_thickness(&mut meshes);
            (meshes, ArmorThickness::Unknown, Vec::new())
        }
    };

    let lod_count = model.lod_count();
    let parts = model.into_parts();

    let mut hull_textures = HashMap::new();
    for (mfm_path, material) in parts.materials {
        let Some(image) = material.albedo.and_then(|albedo| albedo.image) else { continue };
        let texture = RgbaTexture::new(image.width, image.height, image.pixels);
        hull_textures.insert(mfm_path, texture);
    }

    let subject = ModelSubject {
        path: request.path.clone(),
        from_ship: request.from_ship.clone(),
        hardpoints: parts.hardpoints,
        thickness,
        thickness_choices,
    };
    let lods = SceneLods { count: lod_count, shown: request.lod };

    Ok(model_scene(subject, meshes, parts.meshes, hull_textures, lods))
}

/// How many mesh LODs a model has, and which one is shown.
struct SceneLods {
    count: usize,
    shown: usize,
}

/// One model's scene, with the ship-only data empty.
fn model_scene(
    subject: ModelSubject,
    meshes: Vec<InteractiveArmorMesh>,
    hull_meshes: Vec<wowsunpack::export::gltf_export::InteractiveHullMesh>,
    hull_textures: super::camo::TexturesByPart,
    lods: SceneLods,
) -> LoadedShipArmor {
    let bounds = scene_bounds(&meshes, &hull_meshes);
    let hull_part_groups = build_hull_part_groups(&hull_meshes);
    let ArmorZones { zones, zone_parts, zone_part_plates } = ArmorZones::from_meshes(&meshes);

    LoadedShipArmor {
        display_name: subject.path.stem().to_string(),
        subject: SceneSubject::Model(subject),
        species: None,
        meshes,
        bounds,
        zones,
        zone_parts,
        zone_part_plates,
        hull_meshes,
        hull_part_groups,
        hull_textures,
        hull_lod_count: lods.count,
        hull_lod: lods.shown,
        hull_upgrade_names: Vec::new(),
        loaded_hull: None,
        module_alternatives: Vec::new(),
        camera_trajectories: Vec::new(),
        camo_scheme_infos: Vec::new(),
        camo_source: None,
        splash_data: None,
        splash_box_groups: Vec::new(),
        hit_locations: None,
    }
}

/// Greys out plates whose thickness no ship gives.
fn paint_unknown_thickness(meshes: &mut [InteractiveArmorMesh]) {
    for mesh in meshes {
        mesh.colors.fill(UNKNOWN_THICKNESS_COLOR);
        for info in &mut mesh.triangle_info {
            info.color = UNKNOWN_THICKNESS_COLOR;
        }
    }
}

/// Kicks off a model load on the background executor.
pub fn spawn_load_model_scene(
    models: Arc<ModelAssets>,
    bundle: Arc<super::assets::ArmorAssetsBundle>,
    request: ModelLoadRequest,
    cx: &App,
) -> Task<Result<LoadedShipArmor, ModelSceneError>> {
    cx.background_spawn(async move { load_model_scene(&models, bundle.assets.metadata(), &request) })
}

/// Writes `request`'s model to `path` as a GLB with its textures, hardpoints
/// and, when `options` asks for it, its armor at the thickness the viewer
/// shows it with. Synchronous and CPU/IO-bound; callers run it off the UI
/// thread.
pub(crate) fn export_model_glb(
    models: &ModelAssets,
    metadata: &GameMetadataProvider,
    request: &ModelLoadRequest,
    options: &ShipExportOptions,
    path: &std::path::Path,
) -> Result<(), ExportGlbError> {
    let source = thickness_ref(&models.mount_armor, metadata, request);
    // No source writes no thickness, which is what the viewer shows as unknown.
    let armor = source.as_ref().map(ThicknessRef::maps).unwrap_or_default();
    let export_options = ModelExportOptions {
        lod: options.lod,
        damaged: options.damaged,
        texture_lod: options.texture_lod,
        textures: options.textures,
        contents: options.contents,
        hardpoints: options.hardpoints,
    };

    let file = std::fs::File::create(path)
        .map_err(|source| ExportGlbError::CreateFile { path: path.to_path_buf(), source })?;
    let mut writer = std::io::BufWriter::new(file);
    models
        .library
        .export_glb(&request.path, &export_options, armor, &mut writer)
        .map_err(|report| ExportGlbError::WriteGlb(format!("{report:?}")))?;

    writer.flush().map_err(|e| ExportGlbError::WriteGlb(e.to_string()))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use wowsunpack::export::gltf_export::ArmorTriangleInfo;
    use wowsunpack::export::gltf_export::InteractiveHullMesh;
    use wowsunpack::export::hardpoint::Hardpoint;
    use wowsunpack::export::part_group::PartGroup;
    use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;

    use super::*;

    fn hull_mesh() -> InteractiveHullMesh {
        InteractiveHullMesh {
            name: "Turret".to_string(),
            positions: vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 2.0, 6.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uvs: vec![[0.0, 0.0]; 3],
            indices: vec![0, 1, 2],
            mfm_path: None,
            mfm_path_id: 0,
            colors: Vec::new(),
            transform: None,
            group: PartGroup::MainBattery,
        }
    }

    fn armor_mesh() -> InteractiveArmorMesh {
        let info = ArmorTriangleInfo {
            model_index: 1,
            triangle_index: 0,
            material_id: 0,
            material: ArmorMaterial::parse("Cit_Belt").expect("a table key"),
            zone: "Turret".to_string(),
            thickness_mm: 0.0,
            layers: vec![0.0],
            color: UNKNOWN_THICKNESS_COLOR,
            hidden: false,
        };
        InteractiveArmorMesh {
            name: "CM_Turret".to_string(),
            positions: vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 2.0, 6.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            indices: vec![0, 1, 2],
            colors: vec![UNKNOWN_THICKNESS_COLOR; 3],
            triangle_info: vec![info],
            transform: None,
        }
    }

    /// A one-triangle model with no armor and `hardpoints`.
    pub(crate) fn model_scene(path: ModelPath, hardpoints: Vec<Hardpoint>) -> LoadedShipArmor {
        let subject = ModelSubject {
            path,
            from_ship: None,
            hardpoints,
            thickness: ArmorThickness::Unknown,
            thickness_choices: Vec::new(),
        };
        super::model_scene(subject, Vec::new(), vec![hull_mesh()], HashMap::new(), SceneLods { count: 2, shown: 0 })
    }

    /// A one-triangle model carrying one plate of unknown thickness.
    pub(crate) fn armored_model_scene(path: ModelPath) -> LoadedShipArmor {
        let subject = ModelSubject {
            path,
            from_ship: None,
            hardpoints: Vec::new(),
            thickness: ArmorThickness::Unknown,
            thickness_choices: Vec::new(),
        };
        super::model_scene(
            subject,
            vec![armor_mesh()],
            vec![hull_mesh()],
            HashMap::new(),
            SceneLods { count: 2, shown: 0 },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wowsunpack::export::mount_armor::MountArmorIndex;
    use wowsunpack::export::ship::ShipAssets;
    use wowsunpack::game_params::provider::GameMetadataProvider;
    use wowsunpack::game_params::types::ArmorMap;
    use wowsunpack::game_params::types::ParamData;
    use wowsunpack::game_types::GameParamId;

    use super::*;

    fn ship_with_hull_armor(armor: Option<ArmorMap>) -> GameMetadataProvider {
        let vehicle = Vehicle::builder()
            .level(10)
            .group("battleship".to_owned())
            .maybe_abilities(None)
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(armor)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .maybe_ttx_components(None)
            .innate_skills(Vec::new())
            .build();
        let param = Param::builder()
            .id(GameParamId::from(1u32))
            .index("PASB018".to_owned())
            .name("PASB018_Iowa_1944".to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Vehicle(vehicle))
            .build();

        GameMetadataProvider::from_params_no_specs(vec![param]).expect("a one-ship provider builds")
    }

    fn section() -> ModelPath {
        ModelPath::parse("content/gameplay/usa/ship/battleship/ASB018_Iowa_1944/ASB018_Iowa_1944_Bow.model")
            .expect("a model path")
    }

    fn opened_from_ship(part: ShipPartKey) -> ModelLoadRequest {
        let from_ship = ShipPartRef { vehicle_param_index: VehicleParamIndex::new("PASB018"), part };
        ModelLoadRequest::new(section(), Some(from_ship))
    }

    #[test]
    fn a_hull_section_opened_from_its_ship_reads_that_ships_hull_armor() {
        let armor: ArmorMap = HashMap::from([(7, std::collections::BTreeMap::from([(1, 32.0)]))]);
        let metadata = ship_with_hull_armor(Some(armor.clone()));
        let index = MountArmorIndex::default();
        let request = opened_from_ship(ShipPartKey::HullSection(section()));

        let source = thickness_ref(&index, &metadata, &request).expect("the ship's hull armor");

        assert_eq!(source.maps().hull, Some(&armor));
        assert!(source.maps().mount.is_none());
        let ArmorThickness::Hull(hull) = source.thickness(&metadata) else { panic!("a hull source") };
        assert_eq!(hull.vehicle_param_index.as_str(), "PASB018");
        assert_eq!(hull.section, section());
    }

    #[test]
    fn a_hull_section_has_no_thickness_without_a_ship_or_its_armor() {
        let index = MountArmorIndex::default();
        let armorless = ship_with_hull_armor(None);
        let armored = ship_with_hull_armor(Some(HashMap::from([(7, std::collections::BTreeMap::from([(1, 32.0)]))])));

        let from_armorless_ship = opened_from_ship(ShipPartKey::HullSection(section()));
        assert!(thickness_ref(&index, &armorless, &from_armorless_ship).is_none());

        let on_its_own = ModelLoadRequest::new(section(), None);
        assert!(thickness_ref(&index, &armored, &on_its_own).is_none());

        let misc = ShipPartKey::Misc {
            node: HardpointName::new("MP_Misc_1"),
            parent: wowsunpack::export::ship_parts::MiscParent::HullSection(section()),
        };
        assert!(thickness_ref(&index, &armored, &opened_from_ship(misc)).is_none(), "a misc is not the hull");
    }

    /// Opens the Iowa class main turret on its own against a real install:
    /// its meshes, textures, hardpoints and armor, with the thickness read
    /// from a ship that mounts it. Run with `WOWS_DIR` set to an install;
    /// without one it returns at once.
    #[test]
    #[ignore = "needs a local game install in WOWS_DIR"]
    fn a_turret_opens_with_textures_hardpoints_and_ship_armor() {
        let Ok(wows_dir) = std::env::var("WOWS_DIR") else { return };
        let wows_dir = std::path::PathBuf::from(wows_dir);
        let builds = wowsunpack::game_data::list_available_builds(&wows_dir).expect("installed builds");
        let build = *builds.last().expect("at least one build");
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(
            &wows_dir,
            build,
            wowsunpack::game_data::AssetsBin::Overlay,
        )
        .expect("the game VFS");
        let metadata = Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("GameParams"));
        let assets = ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata)).expect("ship assets");
        let library = assets.model_library().expect("the model library");
        let mount_armor = MountArmorIndex::build(&metadata, library.catalog());
        let models = ModelAssets { library: Arc::new(library), mount_armor };
        let turret = models
            .library
            .catalog()
            .entries()
            .iter()
            .find(|entry| entry.name() == "AGM034_16in50_Mk7")
            .expect("the Iowa class turret is catalogued")
            .path()
            .clone();

        let scene = load_model_scene(&models, &metadata, &ModelLoadRequest::new(turret.clone(), None))
            .expect("the turret loads");

        let model = scene.model().expect("a model scene");
        assert!(!scene.hull_meshes.is_empty(), "it has meshes");
        assert!(!scene.hull_textures.is_empty(), "with their albedo textures");
        assert!(!model.hardpoints.is_empty(), "and hardpoints");
        assert!(scene.has_armor(), "and armor");
        assert!(matches!(model.thickness, ArmorThickness::Ship(_)), "read from a ship that mounts it");
        assert!(!model.thickness_choices.is_empty(), "with others to choose from");
        assert!(
            scene.meshes.iter().flat_map(|mesh| &mesh.triangle_info).any(|info| info.thickness_mm > 100.0),
            "the face plate is thick"
        );
    }
}

//! The armor extract must rebuild the meshes the armor viewer builds from the
//! full models. Skips at runtime when no game install is available.

use std::collections::HashMap;

use wowsunpack::export::armor_extract::MountModelArmor;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::game_params::types::GameParamProvider;

/// Battleships, cruisers, a destroyer and a carrier, with turret armor and
/// hardpoints on turrets.
const SHIPS: &[&str] = &["PJSB018", "PASC020", "PGSB110", "PRSC110", "PJSD012", "PASA108"];

#[test]
fn extract_rebuilds_the_viewer_meshes() {
    let Some((build, vfs)) = wows_data_mgr::latest_build() else {
        eprintln!("Skipping: no game data available");
        return;
    };
    eprintln!("Testing armor extract for build {build}");
    let assets = ShipAssets::load(&vfs).expect("assets load");

    let mut compared = 0;
    for param in assets.metadata().params() {
        if !SHIPS.contains(&param.index()) {
            continue;
        }
        let Some(vehicle) = param.vehicle() else {
            continue;
        };
        let extract = assets.vehicle_armor(param.index(), vehicle).expect("extract");
        let mounts: HashMap<&str, &MountModelArmor> =
            extract.mount_models.iter().map(|model| (model.model_path.as_str(), model)).collect();
        let hull = extract.vehicle.hull_upgrade(None).expect("a stock hull");
        // The viewer always loads the vehicle's model, whatever the upgrade names.
        if Some(extract.vehicle.hull_models[hull.hull_model.0].model_path.as_str()) != vehicle.model_path() {
            continue;
        }

        let options = ShipExportOptions { textures: false, hull: Some(hull.upgrade.clone()), ..Default::default() };
        let context = assets.load_ship_from_vehicle(vehicle, &options).expect("viewer load");
        let viewer = context.interactive_armor_meshes().expect("viewer meshes");
        let rebuilt = extract
            .vehicle
            .interactive_meshes(hull, vehicle, |path| mounts.get(path).copied())
            .expect("extract meshes");

        // Meshes without triangles cannot be hit; which of them each side
        // keeps is not part of the contract, nor is mount order.
        let mut viewer: Vec<&InteractiveArmorMesh> =
            viewer.iter().filter(|mesh| !mesh.triangle_info.is_empty()).collect();
        let mut rebuilt: Vec<&InteractiveArmorMesh> =
            rebuilt.iter().filter(|mesh| !mesh.triangle_info.is_empty()).collect();
        viewer.sort_by(|a, b| a.name.cmp(&b.name));
        rebuilt.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(rebuilt.len(), viewer.len(), "{}: mesh count", param.index());
        for (rebuilt, viewer) in rebuilt.iter().zip(viewer) {
            assert_eq!(rebuilt.name, viewer.name, "{}", param.index());
            assert_eq!(rebuilt.transform, viewer.transform, "{} {}", param.index(), viewer.name);
            assert_eq!(rebuilt.positions, viewer.positions, "{} {}", param.index(), viewer.name);
            assert_eq!(rebuilt.indices, viewer.indices, "{} {}", param.index(), viewer.name);
            for (rebuilt_info, viewer_info) in rebuilt.triangle_info.iter().zip(&viewer.triangle_info) {
                assert_eq!(
                    (
                        rebuilt_info.material_id,
                        rebuilt_info.model_index,
                        &rebuilt_info.zone,
                        rebuilt_info.thickness_mm,
                        &rebuilt_info.layers
                    ),
                    (
                        viewer_info.material_id,
                        viewer_info.model_index,
                        &viewer_info.zone,
                        viewer_info.thickness_mm,
                        &viewer_info.layers
                    ),
                    "{} {}",
                    param.index(),
                    viewer.name
                );
            }
        }
        compared += 1;
    }

    assert!(compared >= 3, "compared {compared} ships");
}

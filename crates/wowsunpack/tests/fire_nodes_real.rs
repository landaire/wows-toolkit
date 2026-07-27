//! Fire-section geometry against a real install. Ignored: needs an install.

use std::path::PathBuf;

use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::models::assets_bin;
use wowsunpack::models::fire_nodes;
use wowsunpack::vfs::VfsPath;

const IOWA_HULL_MODEL: &str = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
const IOWA_LENGTH_M: f32 = 262.1;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn game_vfs() -> VfsPath {
    wowsunpack::game_data::build_game_vfs(&game_dir()).expect("build game vfs")
}

fn read_assets_bin(vfs: &VfsPath) -> Vec<u8> {
    let mut file = vfs.join("content/assets.bin").expect("assets.bin path").open_file().expect("open assets.bin");
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes).expect("read assets.bin");
    bytes
}

/// The stock-hull model path for a ship, by GameParams name.
fn stock_hull_model(provider: &GameMetadataProvider, ship_name: &str) -> String {
    let ship = provider.game_param_by_name(ship_name).unwrap_or_else(|| panic!("{ship_name} not in params"));
    let vehicle = ship.vehicle().expect("vehicle");
    let hull = ShipUpgradeSelection::stock(&ship).hull.expect("stock hull upgrade");
    vehicle.model_path_for_hull(&hull).or_else(|| vehicle.model_path()).expect("hull model path").to_string()
}

/// Iowa's four fire nodes sit one per hull section, monotonic bow to stern, and
/// the resolver returns meters so the outer two land near but inside the ends.
#[test]
#[ignore = "requires a World of Warships install"]
fn iowa_fire_sections_are_monotonic_bow_to_stern() {
    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();

    let geom = fire_nodes::resolve_fire_sections(&db, &index, IOWA_HULL_MODEL, 4).expect("Iowa resolves");

    assert_eq!(geom.node_count(), 4);
    let z = geom.longitudinal();
    assert!(z[0] > z[1] && z[1] > z[2] && z[2] > z[3], "bow to stern, got {z:?}");
    // Every node must fall inside the hull. This is the assertion that catches a
    // wrong scale: with the wrong factor the outer nodes leave the ship.
    let half = IOWA_LENGTH_M / 2.0;
    assert!(z.iter().all(|v| v.value().abs() < half + 1.0), "nodes outside the hull: {z:?}");
    // The bow and stern nodes sit near, but not at, the ends.
    assert!(z[0].value() > half * 0.6, "bow node too far aft: {}", z[0].value());
    assert!(z[3].value() < -half * 0.6, "stern node too far forward: {}", z[3].value());
}

/// The node count is per-hull, not a constant four. Most submarines have one burn
/// node and one EP_Fire node, and the resolver must accept that rather than
/// failing the count check.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_single_node_submarine_resolves() {
    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();
    let provider = GameMetadataProvider::from_vfs(&vfs).expect("provider");

    // Resolve Balao's hull model path from GameParams rather than hardcoding, so a
    // model rename does not silently skip this.
    let model = stock_hull_model(&provider, "PASS110_Balao");

    let geom = fire_nodes::resolve_fire_sections(&db, &index, &model, 1).expect("Balao resolves");
    assert_eq!(geom.node_count(), 1);
}

/// A count mismatch is an error, never a partial result. Asking for four nodes
/// from a one-node hull must not silently return one.
#[test]
#[ignore = "requires a World of Warships install"]
fn node_count_mismatch_is_an_error() {
    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();
    let provider = GameMetadataProvider::from_vfs(&vfs).expect("provider");
    let model = stock_hull_model(&provider, "PASS110_Balao");

    let err = fire_nodes::resolve_fire_sections(&db, &index, &model, 4);
    assert!(matches!(err, Err(fire_nodes::FireNodeError::NodeCountMismatch { .. })), "got {err:?}");
}

/// The `EP_Fire_N` naming convention must generalise across the whole roster, not
/// just fit Iowa. Every ship's stock hull is resolved against its own burnNodes
/// count; more than a few percent failing would mean the convention is wrong.
#[test]
#[ignore = "requires a World of Warships install"]
fn the_whole_roster_resolves() {
    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();
    let provider = GameMetadataProvider::from_vfs(&vfs).expect("provider");

    let mut attempted = 0usize;
    let mut failures = Vec::new();

    for param in provider.params().iter() {
        let Some(vehicle) = param.vehicle() else { continue };
        let Some(hull_upgrade) = ShipUpgradeSelection::stock(param).hull else { continue };
        let Some(hull) = vehicle.ttx_components().and_then(|ttx| ttx.hull(&hull_upgrade)) else { continue };
        if hull.burn_nodes.is_empty() {
            continue;
        }
        let Some(model) = vehicle.model_path_for_hull(&hull_upgrade).or_else(|| vehicle.model_path()) else {
            continue;
        };

        attempted += 1;
        if let Err(e) = fire_nodes::resolve_fire_sections(&db, &index, model, hull.burn_nodes.len()) {
            failures.push(format!("{} ({model}): {e}", param.name()));
        }
    }

    for failure in &failures {
        println!("FAILED {failure}");
    }
    let rate = failures.len() as f64 / attempted as f64;
    println!("resolved {}/{attempted} hulls, failure rate {:.2}%", attempted - failures.len(), rate * 100.0);
    assert!(attempted > 500, "expected the roster sweep to cover the whole ship list, got {attempted}");
    assert!(rate < 0.05, "failure rate {:.2}% exceeds 5%", rate * 100.0);
}

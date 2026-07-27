//! Fire-section geometry against a real install. Ignored: needs an install.

use std::collections::HashMap;
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
/// node, and GameParams gives them a single `fire1` group owning both
/// `HP_FX_Fire_1` and `HP_FX_Fire_2`, so the model carries two bare ordinals for
/// one section. The resolver must accept that rather than failing the count check.
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

/// A surplus is as wrong as a shortfall. Iowa's model carries `EP_Fire_1..4`;
/// asking for two must not quietly resolve against the two bow-most nodes,
/// because that is a different hull's geometry.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_surplus_of_fire_nodes_is_an_error() {
    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();

    let err = fire_nodes::resolve_fire_sections(&db, &index, IOWA_HULL_MODEL, 2);
    assert!(
        matches!(err, Err(fire_nodes::FireNodeError::NodeCountMismatch { expected: 2, found: 4, .. })),
        "got {err:?}"
    );
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
/// just fit Iowa. Every hull upgrade of every ship is resolved against its own
/// burnNodes count, not only the stock one: replays carry non-stock hulls, which
/// are separate models with separate extenders.
///
/// Every skip is counted and printed, because a skip that silently removes hulls
/// from the denominator would let the reported rate describe a different
/// population than the one the sweep claims to cover.
#[test]
#[ignore = "requires a World of Warships install"]
fn the_whole_roster_resolves() {
    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();
    let provider = GameMetadataProvider::from_vfs(&vfs).expect("provider");

    let mut attempted = 0usize;
    let mut ships = 0usize;
    let mut skipped_no_ttx = 0usize;
    let mut skipped_no_hulls = 0usize;
    let mut skipped_empty_burn_nodes = Vec::new();
    let mut skipped_no_model_path = 0usize;
    let mut failures = Vec::new();

    for param in provider.params().iter() {
        let Some(vehicle) = param.vehicle() else { continue };
        ships += 1;
        let Some(ttx) = vehicle.ttx_components() else {
            skipped_no_ttx += 1;
            continue;
        };
        if ttx.hulls.is_empty() {
            skipped_no_hulls += 1;
            continue;
        }
        for (hull_upgrade, hull) in ttx.hulls.iter() {
            if hull.burn_nodes.is_empty() {
                skipped_empty_burn_nodes.push(format!("{}/{hull_upgrade}", param.name()));
                continue;
            }
            let Some(model) = vehicle.model_path_for_hull(hull_upgrade).or_else(|| vehicle.model_path()) else {
                skipped_no_model_path += 1;
                continue;
            };

            attempted += 1;
            if let Err(e) = fire_nodes::resolve_fire_sections(&db, &index, model, hull.burn_nodes.len()) {
                failures.push(format!("{}/{hull_upgrade} ({model}): {e}", param.name()));
            }
        }
    }

    for failure in &failures {
        println!("FAILED {failure}");
    }
    let rate = failures.len() as f64 / attempted as f64;
    println!("vehicles {ships}, hull upgrades attempted {attempted}, failures {}", failures.len());
    println!(
        "skipped: no ttx {skipped_no_ttx}, no hulls {skipped_no_hulls}, empty burnNodes {}, no model path {skipped_no_model_path}",
        skipped_empty_burn_nodes.len()
    );
    println!("resolved {}/{attempted} hulls, failure rate {:.2}%", attempted - failures.len(), rate * 100.0);

    assert!(attempted > 1000, "expected the sweep to cover every hull upgrade, got {attempted}");
    // Every hull GameParams describes carries burnNodes. An empty list here would
    // mean read_burn_nodes dropped a list it could not parse, which is exactly the
    // population this sweep would otherwise never look at.
    assert!(
        skipped_empty_burn_nodes.is_empty(),
        "{} hulls have no burn nodes: {:?}",
        skipped_empty_burn_nodes.len(),
        &skipped_empty_burn_nodes[..10.min(skipped_empty_burn_nodes.len())]
    );
    // The live build fails on four hull upgrades, every one a genuine data
    // problem rather than a naming one: two for a ship whose model is absent from
    // assets.bin, and two where GameParams and the model disagree on the section
    // count. The bar is the count itself, not a percentage, so one more failure
    // fails here instead of disappearing into a rounded rate.
    assert!(failures.len() <= 4, "{} hull upgrades failed to resolve, expected at most 4", failures.len());
}

/// The 15 m per model unit scale, checked against a reference that is not a
/// bounding box.
///
/// The hull's waterline is authored as a pair of splines, `..YHWL..` to port and
/// `..YHWR..` to starboard, whose nodes sit at y = 0 and come in matched
/// port/starboard stations. The widest station is the ship's maximum beam, which
/// for Iowa is at the waterline, and Iowa's beam is a published 32.97 m. So the
/// separation of one matched node pair, times the scale, must be that beam, with
/// no bounding box anywhere in the chain.
#[test]
#[ignore = "requires a World of Warships install"]
fn the_model_scale_matches_the_waterline_beam() {
    use wowsunpack::game_params::types::ShipModelDistance;
    use wowsunpack::models::skeleton_extender;

    /// Iowa-class maximum beam, 108 ft 2 in.
    const IOWA_BEAM_M: f32 = 32.97;

    let vfs = game_vfs();
    let bytes = read_assets_bin(&vfs);
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let index = db.build_self_id_index();

    let (entry_index, _) = db.find_path_by_suffix(IOWA_HULL_MODEL, &index).expect("Iowa model entry");
    let directory_id = db.paths_storage[entry_index].parent_id;

    // Station key (the shared name suffix) to the x of the port and starboard node.
    let mut port: HashMap<String, f32> = HashMap::new();
    let mut starboard: HashMap<String, f32> = HashMap::new();
    for entry in &db.paths_storage {
        if entry.parent_id != directory_id || !entry.name.ends_with(".skel_ext") {
            continue;
        }
        let Some(value) = db.lookup_r2p(entry.self_id) else { continue };
        let location = db.decode_r2p_value(value).expect("r2p value");
        let data =
            db.get_prototype_data(location, skeleton_extender::SKELETON_EXTENDER_ITEM_SIZE).expect("prototype data");
        let extender = skeleton_extender::parse_skeleton_extender(data).expect("skeleton extender");
        for (node, &name_id) in extender.name_ids.iter().enumerate() {
            let Some(name) = db.strings.get_string_by_id(name_id) else { continue };
            let Some(matrix) = extender.matrices.get(node) else { continue };
            if let Some(station) = name.split("YHWL").nth(1) {
                port.insert(station.to_string(), matrix.0[12]);
            } else if let Some(station) = name.split("YHWR").nth(1) {
                starboard.insert(station.to_string(), matrix.0[12]);
            }
        }
    }

    let widest = port
        .iter()
        .filter_map(|(station, x)| starboard.get(station).map(|sx| sx - x))
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(widest.is_finite(), "no matched waterline station pairs on Iowa");

    let measured = IOWA_BEAM_M / widest;
    let adopted = ShipModelDistance::from(1.0).to_meters().value();
    let error = (measured - adopted) / adopted;
    println!("widest waterline station {widest:.4} units, beam {IOWA_BEAM_M} m");
    println!("measured scale {measured:.4} m/unit against the adopted {adopted:.4} ({:+.2}%)", error * 100.0);

    let geom = fire_nodes::resolve_fire_sections(&db, &index, IOWA_HULL_MODEL, 4).expect("Iowa resolves");
    let bow = geom.longitudinal()[0].value();
    println!("EP_Fire_1 at {bow:.2} m adopted, {:.2} m at the measured scale", bow / adopted * measured);

    assert!(
        error.abs() < 0.05,
        "waterline beam puts the scale at {measured:.4} m/unit, {:+.2}% from the adopted {adopted}",
        error * 100.0
    );
}

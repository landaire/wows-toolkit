//! What the armor viewer's tests share: a model with one plate of known thickness, a shell with
//! every ballistic value, a cast through that plate, and replay hits landing on it.

use std::collections::BTreeMap;
use std::collections::HashMap;

use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::battle_controller::state::VictimPose;
use wows_replays::analyzer::decoder::ArtillerySalvo;
use wows_replays::analyzer::decoder::ArtilleryShotData;
use wows_replays::analyzer::decoder::HitType;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::analyzer::decoder::ShellHitType;
use wows_replays::analyzer::decoder::ShotHit;
use wows_replays::analyzer::decoder::TerminalBallisticsInfo;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wows_replays::types::Velocity;
use wows_replays::types::WorldPos;
use wows_toolkit_armor::load_model::fixtures::armored_model_scene;
use wows_toolkit_armor::load_ship::ArmorThickness;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::SceneSubject;
use wows_toolkit_armor::load_ship::ThicknessSource;
use wows_toolkit_armor::model_refs::VehicleParamIndex;
use wows_toolkit_armor::trajectory;
use wows_toolkit_armor::trajectory::ArcId;
use wows_toolkit_armor::trajectory::TrajectoryHit;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_replay_data::replay_render::armor::IncomingContext;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Degrees;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_params::types::ShipModelDistance;

use crate::tabs::armor_viewer::panels::incoming::IncomingFire;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;

/// Where on the fixture's plate each shot lands, in the model's own frame.
pub(crate) const ON_PLATE: [[f32; 3]; 2] = [[4.0 / 3.0, 2.0 / 3.0, 2.0], [1.0, 0.5, 1.5]];

/// The ship the incoming shells land on.
pub(crate) const VICTIM: &str = "PASC001";

/// The armored model fixture with its one plate read as 25 mm from a ship that mounts it.
pub(crate) fn plated_scene() -> LoadedShipArmor {
    let path = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
    let mut scene = armored_model_scene(path);
    for info in scene.meshes.iter_mut().flat_map(|mesh| &mut mesh.triangle_info) {
        info.thickness_mm = 25.0;
    }
    if let SceneSubject::Model(model) = &mut scene.subject {
        model.thickness = ArmorThickness::Ship(ThicknessSource {
            vehicle_param_index: VehicleParamIndex::new(VICTIM.to_string()),
            display_name: "Iowa".to_string(),
            hp: HardpointName::new("HP_AGM_1"),
        });
    }

    scene
}

/// Iowa's 406 mm armour-piercing shell, as GameParams states it.
pub(crate) fn iowa_ap() -> ShellInfo {
    ShellInfo {
        name: "PAPA001".to_owned(),
        ammo_type: AmmoType::AP,
        caliber: Millimeters::from(406.0),
        he_pen_mm: None,
        sap_pen_mm: None,
        alpha_damage: 10_500.0,
        muzzle_velocity: 762.0,
        mass_kg: 1225.0,
        krupp: 2574.0,
        ricochet_angle: Some(45.0),
        always_ricochet_angle: Some(60.0),
        fuse_time: Some(0.033),
        fuse_threshold: Some(68.0),
        burn_prob: None,
        air_drag: 0.321,
        normalization: Some(6.0),
        cap: true,
    }
}

/// Fires [`iowa_ap`] from `range` through a 25 mm plate laid on the fixture's plate and the 300 mm
/// belt behind it, coming in along the model's +Z, and keeps it in `viewport` as a cast arc. A far
/// wall the fuse goes off short of is the third plate on its path.
pub(crate) fn add_cast_arc(viewport: ArmorViewportState, range: Km) -> Option<ArcId> {
    let approach = Vec3::z();
    let params = ShellParams::from_shell_info(&iowa_ap()).expect("the shell has every ballistic value");
    let impact = solve_for_range(&params, range.to_meters()).expect("the range solves");
    let first = Vec3::from(ON_PLATE[0]);
    let plate = |along: f32, thickness: f32, angle: f32, zone: &str, material: &str| TrajectoryHit {
        position: first + approach * along,
        thickness: Millimeters::from(thickness),
        zone: zone.to_owned(),
        material: ArmorMaterial::parse(material).expect("a table key"),
        angle_from_normal: Degrees::from(angle),
        distance_from_start: ShipModelDistance::from(along),
        normal: -approach,
    };
    let hits = vec![
        plate(0.0, 25.0, 12.0, "Bow", "Belt"),
        plate(0.4, 300.0, 18.0, "Citadel", "Cit_Belt"),
        plate(6.0, 25.0, 20.0, "Citadel", "Belt"),
    ];
    let trajectory = trajectory::cast(hits, approach, Some((&params, &impact)), 10.0, false, range);

    viewport.add_arc(first, approach, trajectory)
}

/// A salvo from one enemy whose `shots` land on the fixture's plate, coming in along the model's
/// +Z from 10 km, with the victim at rest. `center` is the model's centre, which the hits are
/// placed around.
pub(crate) fn salvo_on_plate(center: Vec3, shots: usize) -> IncomingFire {
    // Ten kilometres, at the 30 m to the world unit `shell_arrival` reads distances in.
    const STANDOFF: f32 = 10_000.0 / 30.0;

    let attacker = EntityId::from(7u32);
    let victim = EntityId::from(3u32);
    let shell = GameParamId::from(4_100_001u32);
    let pose = VictimPose { position: WorldPos::new(1000.0, 0.0, 2000.0), yaw: 0.0, pitch: 0.0, roll: 0.0 };
    // The hull's frame at rest maps a world offset (x, y, z) to the model's (-z, y, -x).
    let landing = |on_plate: [f32; 3]| {
        let offset = Vec3::from(on_plate) - center;
        WorldPos::new(pose.position.0.x - offset.z, pose.position.0.y + offset.y, pose.position.0.z - offset.x)
    };
    let landings: Vec<WorldPos> = ON_PLATE.iter().take(shots).map(|on_plate| landing(*on_plate)).collect();

    let salvo = ArtillerySalvo {
        owner_id: attacker,
        params_id: shell,
        salvo_id: 1,
        shots: landings
            .iter()
            .enumerate()
            .map(|(index, at)| ArtilleryShotData {
                origin: WorldPos::new(at.0.x + STANDOFF, 20.0, at.0.z),
                pitch: 0.1,
                speed: 762.0,
                target: *at,
                shot_id: ShotId::from(index as u32 + 1),
                gun_barrel_id: 0,
                server_time_left: 0.0,
                shooter_height: 20.0,
                hit_distance: 10_000.0,
            })
            .collect(),
    };
    let taken = landings
        .iter()
        .enumerate()
        .map(|(index, at)| PreExtractedHit {
            clock: GameClock(60.0 + index as f32),
            hit: ResolvedShotHit {
                clock: GameClock(60.0 + index as f32),
                hit: ShotHit {
                    owner_id: attacker,
                    hit_type: HitType {
                        collision: Recognized::Unknown("0".to_string()),
                        shell_hit: Recognized::Known(ShellHitType::Normal),
                        raw: 0,
                    },
                    shot_id: ShotId::from(index as u32 + 1),
                    position: *at,
                    terminal_ballistics: Some(TerminalBallisticsInfo {
                        position: *at,
                        velocity: Velocity::default(),
                        detonator_activated: true,
                        material_angle: 1.1,
                    }),
                },
                victim_entity_id: victim,
                salvo: Some(salvo.clone()),
                fired_at: Some(GameClock(50.0)),
                victim_pose: Some(pose),
            },
        })
        .collect();

    let context = IncomingContext {
        attackers: BTreeMap::from([(attacker, "Yamato".to_string())]),
        taken,
        shells: HashMap::from([(shell, iowa_ap())]),
        ..IncomingContext::default()
    };

    IncomingFire::about(context, VICTIM.to_string())
}

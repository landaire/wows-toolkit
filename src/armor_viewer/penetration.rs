use std::collections::HashSet;
use std::sync::Arc;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::{GameParamProvider, Param, Species};

/// Shell info extracted from GameParams for display and penetration checks.
#[derive(Clone, Debug)]
pub struct ShellInfo {
    pub name: String,
    pub ammo_type: String,
    pub caliber_mm: f32,
    pub he_pen_mm: Option<f32>,
    pub sap_pen_mm: Option<f32>,
    pub alpha_damage: f32,
    pub muzzle_velocity: f32,
    pub mass_kg: f32,
    pub krupp: f32,
    pub ricochet_angle: f32,
    pub always_ricochet_angle: f32,
    pub fuse_time: f32,
    pub fuse_threshold: f32,
    pub burn_prob: f32,
    pub air_drag: f32,
    pub normalization: f32,
}

/// A ship added to the comparison list.
#[derive(Clone, Debug)]
pub struct ComparisonShip {
    pub param_index: String,
    pub display_name: String,
    pub tier: u32,
    pub nation: String,
    pub species: Species,
    pub shells: Vec<ShellInfo>,
}

/// Check result for a single shell vs a single armor thickness.
#[derive(Clone, Debug, PartialEq)]
pub enum PenResult {
    /// Shell penetrates (HE/SAP pen >= thickness, or AP overmatch).
    Penetrates,
    /// Shell does not penetrate.
    Bounces,
    /// Angle-dependent (AP without overmatch — can't determine at point-blank without angle).
    AngleDependent,
}

/// Check if a shell penetrates a given armor thickness at point-blank (no angle consideration).
pub fn check_penetration(shell: &ShellInfo, thickness_mm: f32, ifhe: bool) -> PenResult {
    match shell.ammo_type.as_str() {
        "HE" => {
            let pen = if ifhe { shell.he_pen_mm.unwrap_or(0.0) * 1.25 } else { shell.he_pen_mm.unwrap_or(0.0) };
            if pen >= thickness_mm { PenResult::Penetrates } else { PenResult::Bounces }
        }
        "CS" => {
            let pen = shell.sap_pen_mm.unwrap_or(0.0);
            if pen >= thickness_mm { PenResult::Penetrates } else { PenResult::Bounces }
        }
        "AP" => {
            // Overmatch: caliber > armor * 14.3
            if shell.caliber_mm > thickness_mm * 14.3 { PenResult::Penetrates } else { PenResult::AngleDependent }
        }
        _ => PenResult::Bounces,
    }
}

/// Resolve all unique shells for a ship by param_index.
///
/// Chain: ship param → vehicle → ShipConfigData.main_battery_ammo → Projectile lookup.
pub fn resolve_ship_shells(metadata: &GameMetadataProvider, param_index: &str) -> Option<ComparisonShip> {
    let param: Arc<Param> = metadata.game_param_by_index(param_index)?;

    let species = param.species()?.known().copied()?;
    let vehicle = param.vehicle()?;
    let tier = vehicle.level();
    let nation = param.nation().to_string();

    let display_name =
        metadata.localized_name_from_param(&param).map(|s| s.to_string()).unwrap_or_else(|| param.name().to_string());

    // Get main battery ammo names from the config data
    let config = vehicle.config_data()?;
    let ammo_names: &HashSet<String> = &config.main_battery_ammo;

    let mut shells: Vec<ShellInfo> = Vec::new();
    let mut seen_names: HashSet<&String> = HashSet::new();

    for ammo_name in ammo_names {
        if !seen_names.insert(ammo_name) {
            continue;
        }
        let ammo_param = metadata.game_param_by_name(ammo_name)?;
        let projectile = ammo_param.projectile()?;

        let caliber_m = projectile.bullet_diametr().unwrap_or(0.0);
        let caliber_mm = caliber_m * 1000.0;

        shells.push(ShellInfo {
            name: ammo_name.clone(),
            ammo_type: projectile.ammo_type().to_string(),
            caliber_mm,
            he_pen_mm: projectile.alpha_piercing_he(),
            sap_pen_mm: projectile.alpha_piercing_cs(),
            alpha_damage: projectile.alpha_damage().unwrap_or(0.0),
            muzzle_velocity: projectile.bullet_speed().unwrap_or(0.0),
            mass_kg: projectile.bullet_mass().unwrap_or(0.0),
            krupp: projectile.bullet_krupp().unwrap_or(0.0),
            ricochet_angle: projectile.bullet_ricochet_at().unwrap_or(45.0),
            always_ricochet_angle: projectile.bullet_always_ricochet_at().unwrap_or(60.0),
            fuse_time: projectile.bullet_detonator().unwrap_or(0.033),
            fuse_threshold: projectile.bullet_detonator_threshold().unwrap_or(0.0),
            burn_prob: projectile.burn_prob().unwrap_or(-0.5),
            air_drag: projectile.bullet_air_drag().unwrap_or(0.0),
            normalization: projectile.bullet_cap_normalize_max_angle().unwrap_or(0.0),
        });
    }

    // Sort shells: AP first, then HE, then SAP (CS)
    shells.sort_by(|a, b| {
        fn ammo_order(t: &str) -> u8 {
            match t {
                "AP" => 0,
                "HE" => 1,
                "CS" => 2,
                _ => 3,
            }
        }
        ammo_order(&a.ammo_type)
            .cmp(&ammo_order(&b.ammo_type))
            .then(a.caliber_mm.partial_cmp(&b.caliber_mm).unwrap_or(std::cmp::Ordering::Equal))
    });

    Some(ComparisonShip { param_index: param_index.to_string(), display_name, tier, nation, species, shells })
}

/// Format ammo type for display.
pub fn ammo_type_display(ammo_type: &str) -> &str {
    match ammo_type {
        "AP" => "AP",
        "HE" => "HE",
        "CS" => "SAP",
        _ => ammo_type,
    }
}

/// A single hit along a trajectory ray through the armor model.
#[derive(Clone, Debug)]
pub struct TrajectoryHit {
    pub position: [f32; 3],
    pub thickness_mm: f32,
    pub zone: String,
    pub material: String,
    pub angle_deg: f32,
    pub distance_from_start: f32,
}

/// Result of casting a trajectory ray through the armor model.
#[derive(Clone, Debug)]
pub struct TrajectoryResult {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub hits: Vec<TrajectoryHit>,
    pub total_armor_mm: f32,
    /// 3D arc points from firing position to first hit (empty if range=0 / point-blank).
    pub arc_points_3d: Vec<[f32; 3]>,
    /// Ballistic impact data at the selected range (None if range=0).
    pub ballistic_impact: Option<crate::armor_viewer::ballistics::ImpactResult>,
}

/// Compute the impact angle between a ray direction and a triangle normal (in degrees).
/// Returns 0° for perpendicular hit (straight on), 90° for parallel (glancing).
pub fn impact_angle_deg(ray_dir: &[f32; 3], normal: &[f32; 3]) -> f32 {
    let dot = ray_dir[0] * normal[0] + ray_dir[1] * normal[1] + ray_dir[2] * normal[2];
    // angle between ray and normal is acos(|dot|)
    // impact angle = 90° - that angle (0° = perpendicular to surface = head-on)
    let cos_angle = dot.abs().min(1.0);
    90.0 - cos_angle.acos().to_degrees()
}

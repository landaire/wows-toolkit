//! Whether a shell defeats a plate, and the shells a ship brings.
//!
//! Shared by both front ends: the egui armor viewer's analysis window and the
//! GPUI pane's penetration checker report the same verdicts, so neither can
//! drift from the other on what overmatches what.

use std::collections::HashSet;
use std::sync::Arc;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_params::types::Species;

use wowsunpack::ballistics::is_overmatch;

/// Penetration bonus Inertia Fuse for HE Shells grants.
const IFHE_PENETRATION_MULTIPLIER: f32 = 1.25;

/// Whether the captain's Inertia Fuse for HE Shells skill is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ifhe {
    Applied,
    NotApplied,
}

impl Ifhe {
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled { Ifhe::Applied } else { Ifhe::NotApplied }
    }
}

/// Position of a ship in the penetration comparison list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComparisonShipIndex(usize);

impl ComparisonShipIndex {
    /// The ship a single-ship view (the replay armor viewer) reports against.
    pub const ONLY: ComparisonShipIndex = ComparisonShipIndex(0);

    pub fn new(index: usize) -> Self {
        ComparisonShipIndex(index)
    }

    /// Slot this ship draws from in a fixed-size identity palette.
    pub fn palette_slot(self, palette_len: usize) -> usize {
        self.0 % palette_len
    }
}

/// A ship added to the comparison list.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ComparisonShip {
    pub param_index: String,
    pub display_name: String,
    pub tier: u32,
    pub nation: String,
    pub species: Species,
    pub shells: Vec<ShellInfo>,
}

/// Check result for a single shell vs a single armor thickness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenResult {
    /// Shell penetrates (HE/SAP pen at least the thickness, or AP overmatch).
    Penetrates,
    /// Shell does not penetrate.
    Bounces,
    /// Angle-dependent (AP without overmatch; can't determine at point-blank without angle).
    AngleDependent,
}

/// HE penetration a shell brings to a plate, IFHE included.
///
/// `None` when the projectile carries no HE penetration value; there is no safe
/// numeric default, since 0.0 would read as a shell that penetrates nothing.
pub fn he_penetration(shell: &ShellInfo, ifhe: Ifhe) -> Option<Millimeters> {
    let base = Millimeters::from(shell.he_pen_mm?);
    Some(match ifhe {
        Ifhe::Applied => base * IFHE_PENETRATION_MULTIPLIER,
        Ifhe::NotApplied => base,
    })
}

/// SAP penetration a shell brings to a plate.
///
/// `None` when the projectile carries no SAP penetration value.
pub fn sap_penetration(shell: &ShellInfo) -> Option<Millimeters> {
    shell.sap_pen_mm.map(Millimeters::from)
}

/// Check if a shell penetrates a given armor thickness at point-blank (no angle consideration).
///
/// Returns `None` for unknown ammo types (logged as a warning) and for shells
/// whose penetration value is missing.
pub fn check_penetration(shell: &ShellInfo, thickness: Millimeters, ifhe: Ifhe) -> Option<PenResult> {
    let flat_penetration = match &shell.ammo_type {
        AmmoType::HE => he_penetration(shell, ifhe)?,
        AmmoType::SAP => sap_penetration(shell)?,
        AmmoType::AP => {
            return Some(if is_overmatch(shell.caliber, thickness) {
                PenResult::Penetrates
            } else {
                PenResult::AngleDependent
            });
        }
        AmmoType::Unknown(t) => {
            tracing::warn!("Unknown ammo type '{}' for shell '{}', cannot check penetration", t, shell.name);
            return None;
        }
    };

    Some(if flat_penetration >= thickness { PenResult::Penetrates } else { PenResult::Bounces })
}

/// Resolve all unique shells for a ship by param_index.
///
/// Chain: ship param -> vehicle -> ShipConfigData.main_battery_ammo -> Projectile lookup.
pub fn resolve_ship_shells(metadata: &GameMetadataProvider, param_index: &str) -> Option<ComparisonShip> {
    let param: Arc<Param> = metadata.game_param_by_index(param_index)?;

    let species = param.species()?.known().copied()?;
    let vehicle = param.vehicle()?;
    let tier = vehicle.level();
    let nation = param.nation().to_string();

    let display_name = metadata.localized_name_from_param(&param).unwrap_or_else(|| param.name().to_string());

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
        shells.push(projectile.to_shell_info(ammo_name.clone()));
    }

    // Sort shells: AP first, then HE, then SAP
    shells.sort_by(|a, b| {
        a.ammo_type
            .sort_order()
            .cmp(&b.ammo_type.sort_order())
            .then(a.caliber.partial_cmp(&b.caliber).unwrap_or(std::cmp::Ordering::Equal))
    });

    Some(ComparisonShip { param_index: param_index.to_string(), display_name, tier, nation, species, shells })
}

#[cfg(test)]
mod tests {
    use super::Ifhe;
    use super::PenResult;
    use super::check_penetration;
    use super::he_penetration;
    use super::sap_penetration;
    use wowsunpack::game_params::types::AmmoType;
    use wowsunpack::game_params::types::Millimeters;
    use wowsunpack::game_params::types::ShellInfo;

    fn shell(ammo_type: AmmoType, caliber: f32, he_pen_mm: Option<f32>, sap_pen_mm: Option<f32>) -> ShellInfo {
        ShellInfo {
            name: "PTEST".to_string(),
            ammo_type,
            caliber: Millimeters::from(caliber),
            he_pen_mm,
            sap_pen_mm,
            alpha_damage: 0.0,
            muzzle_velocity: 0.0,
            mass_kg: 0.0,
            krupp: 0.0,
            ricochet_angle: 0.0,
            always_ricochet_angle: 0.0,
            fuse_time: 0.0,
            fuse_threshold: None,
            burn_prob: 0.0,
            air_drag: 0.0,
            normalization: None,
            cap: true,
        }
    }

    #[test]
    fn ifhe_raises_he_penetration_by_a_quarter() {
        let he = shell(AmmoType::HE, 152.0, Some(25.0), None);

        assert_eq!(he_penetration(&he, Ifhe::NotApplied), Some(Millimeters::from(25.0)));
        assert_eq!(he_penetration(&he, Ifhe::Applied), Some(Millimeters::from(31.25)));
    }

    #[test]
    fn a_shell_with_no_penetration_figure_reports_none_rather_than_zero() {
        let he = shell(AmmoType::HE, 152.0, None, None);
        let sap = shell(AmmoType::SAP, 203.0, None, None);

        assert_eq!(he_penetration(&he, Ifhe::Applied), None);
        assert_eq!(sap_penetration(&sap), None);
        assert_eq!(check_penetration(&he, Millimeters::from(19.0), Ifhe::Applied), None);
    }

    #[test]
    fn he_penetrates_only_what_its_figure_covers() {
        let he = shell(AmmoType::HE, 152.0, Some(25.0), None);

        assert_eq!(check_penetration(&he, Millimeters::from(25.0), Ifhe::NotApplied), Some(PenResult::Penetrates));
        assert_eq!(check_penetration(&he, Millimeters::from(26.0), Ifhe::NotApplied), Some(PenResult::Bounces));
        // The same plate, with the skill that raises the figure over it.
        assert_eq!(check_penetration(&he, Millimeters::from(26.0), Ifhe::Applied), Some(PenResult::Penetrates));
    }

    #[test]
    fn ap_overmatches_a_thin_plate_and_otherwise_depends_on_the_angle() {
        // Overmatch is caliber/14.3, so 457 mm defeats 31 mm outright.
        let ap = shell(AmmoType::AP, 457.0, None, None);

        assert_eq!(check_penetration(&ap, Millimeters::from(31.0), Ifhe::NotApplied), Some(PenResult::Penetrates));
        assert_eq!(check_penetration(&ap, Millimeters::from(32.0), Ifhe::NotApplied), Some(PenResult::AngleDependent));
    }

    #[test]
    fn an_unknown_ammo_type_reports_nothing_rather_than_guessing() {
        let odd = shell(AmmoType::Unknown("AMMOTYPE_SOMETHING".to_string()), 100.0, Some(50.0), None);

        assert_eq!(check_penetration(&odd, Millimeters::from(10.0), Ifhe::NotApplied), None);
    }
}

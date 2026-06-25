//! TTX orchestration: the public entry point that ties the factory layer together.
//!
//! [`ship_stats`] resolves each selected component off the ship's
//! [`ShipTtxComponents`], builds the modifier context, calls every factory, and
//! assembles a [`ShipStats`]. Each section is `None` when the ship lacks its
//! components; nothing is fabricated.

use crate::game_params::keys::ComponentType;
use crate::game_params::ttx::constants::DispersionCurve;
use crate::game_params::ttx::constants::DispersionEllipse;
use crate::game_params::ttx::effects::ReloadCoeffs;
use crate::game_params::ttx::factories;
use crate::game_params::ttx::model::ShipStats;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::provenance::ModifierSources;
use crate::game_params::ttx::provenance::Off;
use crate::game_params::ttx::provenance::On;
use crate::game_params::ttx::provenance::Recorder;
use crate::game_params::ttx::provenance::ShipStatsProvenance;
use crate::game_params::ttx::selection::ShipUpgradeSelection;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::Km;
use crate::game_params::types::Param;

/// Stock fire-control range coefficient when no `_Suo` upgrade is selected
/// (PreprocessedFireControl.py:7 identity; FactoryArtillery.py:42 default 1.0).
const NO_FIRE_CONTROL_COEF: f32 = 1.0;

/// Main-battery dispersion resolved for an equipped loadout: the gun's ellipse curve,
/// the FC-adjusted max range, and the `GMIdealRadius` coefficient. Evaluate the ellipse
/// at any aim distance with [`Self::ellipse_at`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArtilleryDispersion {
    pub curve: DispersionCurve,
    pub max_range: Km,
    pub ideal_radius_coef: f32,
}

impl ArtilleryDispersion {
    /// The dispersion ellipse at firing distance `dist` (clamped to `max_range`).
    pub fn ellipse_at(&self, dist: Km) -> DispersionEllipse {
        crate::game_params::ttx::constants::dispersion_ellipse(
            &self.curve,
            dist,
            self.max_range,
            self.ideal_radius_coef,
        )
    }

    /// The ellipse at max range (the port-card point).
    pub fn at_max_range(&self) -> DispersionEllipse {
        self.ellipse_at(self.max_range)
    }
}

/// Resolve the main-battery dispersion profile for an equipped loadout. `None` when the
/// ship has no main battery in `selection`, or the mounted gun lacks dispersion-curve data.
pub fn artillery_dispersion(
    ship: &Param,
    selection: &ShipUpgradeSelection,
    modifiers: &ModifierBundle,
) -> Option<ArtilleryDispersion> {
    let components = ship.vehicle()?.ttx_components()?;
    let arty = selection.artillery.as_deref().and_then(|name| components.artillery(name))?;
    let curve = arty.guns.first()?.dispersion_curve()?;
    let fc_coef = selection
        .fire_control
        .as_deref()
        .and_then(|name| components.fire_control_max_dist_coef(name))
        .unwrap_or(NO_FIRE_CONTROL_COEF);
    let range_km = factories::artillery_range_km(arty, fc_coef, 1.0, modifiers)?;
    Some(ArtilleryDispersion {
        curve,
        max_range: Km::from(range_km),
        ideal_radius_coef: modifiers.coef("GMIdealRadius"),
    })
}

/// `SMALL_SHELL_MAX_DIAMETER` (meters), the `isSmallGun` caliber gate
/// (Modifiers/__init__.py:19 `barrelDiameter < SMALL_SHELL_MAX_DIAMETER`). The
/// straight `.py` decompile zeroes this compiled-module float; the real value
/// recovered via wowsdeob from `ConstantsShip` bytecode is 0.149 (the body does
/// `LOAD_CONST 0.149 / STORE_NAME SMALL_SHELL_MAX_DIAMETER`). At 0.149 m a 127mm
/// DD gun is small and a 152mm cruiser gun is big. The gun's `smallGun` override
/// field is not retained on [`ArtilleryGunStats`], so the caliber threshold is
/// the sole basis.
const SMALL_SHELL_MAX_DIAMETER_M: f32 = 0.149;

/// Compute a ship's full as-shown-in-port stat card for `selection` under `modifiers`.
///
/// Wiring (each factory transcription is documented at its definition in
/// `factories.rs`):
/// - durability/mobility/battery/visibility from the selected hull (engine for speed).
/// - artillery: `fc_max_dist_coef` from the selected `_Suo` upgrade's `maxDistCoef`
///   (default [`NO_FIRE_CONTROL_COEF`] when no FC is selected); `level` is the ship tier.
/// - secondaries from the ATBA component the selected hull references (keyed by hull name).
/// - torpedoes from the selected `_Torpedoes` launchers.
/// - armor: hull armor from `Vehicle::armor`, artillery armor from the selected hull's
///   main-battery mount armor maps.
/// - visibility: `has_big_gun_artillery` = main battery present with a non-small gun
///   (caliber >= [`SMALL_SHELL_MAX_DIAMETER_M`]); `mg_max_dist_km`/`atba_max_dist_km`
///   feed from the computed artillery/secondary range so the secondary-detection floor
///   uses the same numbers the cards show.
///
/// Sections are `None` when their components are absent.
pub fn ship_stats(
    ship: &Param,
    selection: &ShipUpgradeSelection,
    modifiers: &ModifierBundle,
    level: u32,
    provider: &dyn GameParamProvider,
) -> ShipStats {
    ship_stats_with(
        ship,
        selection,
        modifiers,
        &ModifierSources::default(),
        ReloadCoeffs::default(),
        1.0,
        level,
        provider,
        &mut Off,
    )
}

/// `ship_stats` plus per-stat provenance. `sources` carries the per-input raw
/// modifier values (from `EffectiveModifiers::sources`); pass
/// `&ModifierSources::default()` for a module-only (no-modifier) explanation.
pub fn ship_stats_explained(
    ship: &Param,
    selection: &ShipUpgradeSelection,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    level: u32,
    provider: &dyn GameParamProvider,
) -> (ShipStats, ShipStatsProvenance) {
    let mut rec = On::default();
    let stats =
        ship_stats_with(ship, selection, modifiers, sources, ReloadCoeffs::default(), 1.0, level, provider, &mut rec);
    (stats, rec.into_provenance())
}

/// Compute a ship's stat card with per-armament reload multipliers and a spotter
/// artillery range coefficient layered on top of `modifiers`. The public [`ship_stats`]
/// delegates here with identity values so existing callers see no behavior change.
// Threads recorder, modifier bundle, per-source provenance, reload coefficients, and spotter coef alongside the base inputs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ship_stats_with<R: Recorder>(
    ship: &Param,
    selection: &ShipUpgradeSelection,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    reload_coeffs: ReloadCoeffs,
    spotter_dist_coef: f32,
    level: u32,
    provider: &dyn GameParamProvider,
    rec: &mut R,
) -> ShipStats {
    let Some(vehicle) = ship.vehicle() else {
        return ShipStats::default();
    };
    let Some(components) = vehicle.ttx_components() else {
        return ShipStats::default();
    };

    let hull = selection.hull.as_deref().and_then(|name| components.hull(name));
    let hull_name = selection.hull.as_deref().unwrap_or("");

    let durability = hull.map(|h| factories::durability(h, hull_name, modifiers, sources, level, rec));

    let mobility = hull.map(|h| {
        let engine = selection.engine.as_deref().and_then(|name| components.engine(name)).cloned().unwrap_or_default();
        factories::mobility(h, hull_name, &engine, selection.engine.as_deref(), modifiers, sources, rec)
    });

    let battery = hull.and_then(|h| factories::battery(h, hull_name, modifiers, sources, rec));

    // Fire-control coefficient feeds main-battery range (default 1.0 when no FC).
    let fc_coef = selection
        .fire_control
        .as_deref()
        .and_then(|name| components.fire_control_max_dist_coef(name))
        .unwrap_or(NO_FIRE_CONTROL_COEF);

    let arty_name = selection.artillery.as_deref().unwrap_or("");
    let artillery = selection.artillery.as_deref().and_then(|name| components.artillery(name)).and_then(|arty| {
        factories::artillery(
            arty,
            arty_name,
            selection.fire_control.as_deref(),
            modifiers,
            sources,
            fc_coef,
            spotter_dist_coef,
            reload_coeffs.main,
            level,
            provider,
            rec,
        )
    });

    let secondaries = selection.hull.as_deref().and_then(|name| components.secondaries(name)).and_then(|atba| {
        factories::secondaries(atba, hull_name, modifiers, sources, reload_coeffs.secondary, level, provider, rec)
    });

    let torp_name = selection.torpedoes.as_deref().unwrap_or("");
    let torpedoes = selection.torpedoes.as_deref().and_then(|name| components.torpedoes(name)).and_then(|launchers| {
        factories::torpedoes(launchers, modifiers, reload_coeffs.torpedo, provider, torp_name, sources, rec)
    });

    // Armor: hull plate map plus the selected hull's main-battery mount armor maps.
    let armor = vehicle.armor().and_then(|hull_armor| {
        let arti_armor = artillery_armor_maps(vehicle, selection.hull.as_deref());
        factories::armor(hull_armor, hull_name, arti_armor.iter().copied(), rec)
    });

    // has_big_gun: the gate is `artillery present and not isSmallGun` (FactoryVisibility
    // createVisibilityTTX@30). isSmallGun is caliber-based; read the first main-battery
    // gun's barrelDiameter against SMALL_SHELL_MAX_DIAMETER_M.
    let has_big_gun_artillery = selection
        .artillery
        .as_deref()
        .and_then(|name| components.artillery(name))
        .and_then(|arty| arty.guns.first())
        .and_then(|gun| gun.barrel_diameter)
        .is_some_and(|d| d.value() >= SMALL_SHELL_MAX_DIAMETER_M);

    let mg_max_dist_km = artillery.as_ref().and_then(|a| a.range.map(|r| r.value()));
    let atba_max_dist_km = secondaries.as_ref().map(|b| b.range.value());

    let visibility = hull.map(|h| {
        factories::visibility(
            h,
            hull_name,
            modifiers,
            sources,
            has_big_gun_artillery,
            mg_max_dist_km,
            atba_max_dist_km,
            rec,
        )
    });

    ShipStats {
        durability,
        mobility,
        armor,
        battery,
        artillery,
        secondaries,
        torpedoes,
        // Fire control has no standalone card (its coef folds into artillery range).
        fire_control: None,
        visibility,
    }
}

/// The stock (base) stat card for `ship`: stock selection, empty modifier bundle.
///
/// `level` and `species` come from the ship itself; an empty bundle reads identity
/// (1.0/0.0) for every coefficient, giving the unmodernised port card for free.
pub fn ship_stats_stock(ship: &Param, provider: &dyn GameParamProvider) -> ShipStats {
    let selection = ShipUpgradeSelection::stock(ship);
    let level = ship.vehicle().map(|v| v.level()).unwrap_or(0);
    let Some(species) = ship.species().and_then(|s| s.known().copied()) else {
        return ShipStats::default();
    };
    ship_stats(ship, &selection, &ModifierBundle::empty(species), level, provider)
}

/// Collect the main-battery mount armor maps for the selected hull, the
/// `artillery_armor` input the [`factories::armor`] factory takes
/// (`getArmorDictByComponent`, all `HP_AGM_*` mounts). Empty when the hull has no
/// artillery mounts (the factory's no-artillery branch).
fn artillery_armor_maps<'a>(
    vehicle: &'a crate::game_params::types::Vehicle,
    hull_name: Option<&str>,
) -> Vec<&'a ArmorMap> {
    let Some(hull_name) = hull_name else {
        return Vec::new();
    };
    let Some(config) = vehicle.hull_upgrade(hull_name) else {
        return Vec::new();
    };
    let Some(mounts) = config.mounts(ComponentType::Artillery) else {
        return Vec::new();
    };
    mounts.iter().filter_map(|m| m.mount_armor()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rc;
    use crate::game_params::ttx::components::ArtilleryComponentStats;
    use crate::game_params::ttx::components::ArtilleryGunStats;
    use crate::game_params::ttx::components::EngineComponentStats;
    use crate::game_params::ttx::components::HullComponentStats;
    use crate::game_params::ttx::components::ShipTtxComponents;
    use crate::game_params::ttx::components::TorpedoLauncherStats;
    use crate::game_params::ttx::model::DegreesPerSecond;
    use crate::game_params::ttx::model::Hp;
    use crate::game_params::ttx::model::Knots;
    use crate::game_params::ttx::model::Seconds;
    use crate::game_params::types::BigWorldDistance;
    use crate::game_params::types::Km;
    use crate::game_params::types::Meters;
    use crate::game_params::types::Param;
    use crate::game_params::types::ParamData;
    use crate::game_params::types::Projectile;
    use crate::game_params::types::Species;
    use crate::game_params::types::Vehicle;
    use crate::game_types::GameParamId;

    /// Gearing's real default-hull base stats (see factories.rs::tests::gearing_hull).
    fn gearing_hull() -> HullComponentStats {
        HullComponentStats {
            health: Some(Hp::from(19400.0)),
            max_speed: Some(Knots::from(36.0)),
            speed_coef: Some(1.0),
            turning_radius: Some(Meters::from(640.0)),
            rudder_time: Some(Seconds::from(4.25)),
            visibility_factor: Some(Km::from(7.33)),
            visibility_factor_by_plane: Some(Km::from(3.41)),
            visibility_coef_fire: Some(Km::from(2.0)),
            visibility_coef_fire_by_plane: Some(Km::from(2.0)),
            visibility_coef_gk: Some(Km::from(1e-6)),
            visibility_coef_gk_in_smoke: Some(Km::from(2.83)),
            visibility_factor_by_periscope: None,
            flood_prob: Some(0.0),
            battery_capacity: None,
            battery_regen_rate: None,
        }
    }

    /// Gearing's real `PAPT027_Mk_16_mod_1` torpedo (Projectile fields).
    fn gearing_torpedo() -> Projectile {
        Projectile::builder()
            .ammo_type("torpedo".to_string())
            .max_dist(BigWorldDistance::from(350.0))
            .speed(66.0)
            .alpha_damage(53500.0)
            .damage(1200.0)
            .visibility_factor(1.4)
            .torpedo_type(0)
            .build()
    }

    /// Gearing's real torpedo launcher mount (`HP_AGT_*`).
    fn gearing_launcher() -> TorpedoLauncherStats {
        TorpedoLauncherStats {
            shot_delay: Some(Seconds::from(103.0)),
            rotation_speed: Some(DegreesPerSecond::from(25.0)),
            num_barrels: Some(5.0),
            ammo_switch_coeff: None,
            ammo: vec!["PAPT027_Mk_16_mod_1".to_string()],
        }
    }

    /// Gearing's `D10_ART` 127mm main battery: 3 twin mounts, shotDelay 4.6,
    /// barrelDiameter 0.127 (a small DD gun, < SMALL_SHELL_MAX_DIAMETER_M), one HE
    /// shell, component maxDist 11130 (BW) -> 11.13 km stock range.
    fn gearing_artillery() -> ArtilleryComponentStats {
        let gun = || ArtilleryGunStats {
            shot_delay: Some(Seconds::from(4.6)),
            rotation_speed: Some(DegreesPerSecond::from(20.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.127)),
            ammo_switch_coeff: Some(1.0),
            min_radius: Some(1.0),
            ideal_radius: Some(10.0),
            ideal_distance: Some(1000.0),
            radius_on_zero: None,
            radius_on_delim: None,
            radius_on_max: None,
            delim: None,
            ammo: vec!["PAPA127_127mm_HE".to_string()],
        };
        ArtilleryComponentStats { max_dist: Some(Meters::from(11130.0)), guns: vec![gun(), gun(), gun()] }
    }

    fn gearing_he() -> Projectile {
        Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(1800.0)
            .alpha_piercing_he(21.0)
            .burn_prob(0.05)
            .uw_critical(0.0)
            .bullet_diametr(0.127)
            .bullet_speed(792.0)
            .build()
    }

    /// A provider exposing the named projectiles a Gearing-shaped ship resolves.
    struct StubProvider {
        params: Vec<Rc<Param>>,
    }

    impl StubProvider {
        fn new(entries: &[(&str, Projectile)]) -> Self {
            let params = entries
                .iter()
                .enumerate()
                .map(|(i, (name, proj))| {
                    Rc::new(
                        Param::builder()
                            .id(GameParamId::from((i + 1) as u32))
                            .index(format!("S{i:04}"))
                            .name(name.to_string())
                            .nation("USA".to_string())
                            .data(ParamData::Projectile(proj.clone()))
                            .build(),
                    )
                })
                .collect();
            StubProvider { params }
        }
    }

    impl GameParamProvider for StubProvider {
        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_index(&self, _index: &str) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
            self.params.iter().find(|p| p.name() == name).cloned()
        }
        fn params(&self) -> &[Rc<Param>] {
            &self.params
        }
    }

    /// Assemble a ship `Param` (tier 10 destroyer) around the given TTX components.
    fn ship_with(name: &str, level: u32, species: Species, components: ShipTtxComponents) -> Param {
        let vehicle = Vehicle::builder()
            .level(level)
            .group("g".to_string())
            .maybe_abilities(None)
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .ttx_components(components)
            .innate_skills(Vec::new())
            .build();
        Param::builder()
            .id(GameParamId::from(900u32))
            .index("IDX".to_string())
            .name(name.to_string())
            .nation("USA".to_string())
            .species(crate::recognized::Recognized::Known(species))
            .data(ParamData::Vehicle(vehicle))
            .build()
    }

    /// Build a Gearing-shaped ship `Param` populated across hull/engine/artillery/
    /// torpedoes/fire-control slots, with the stock selection pre-recorded (mirroring
    /// the provider walk's empty-`prev` capture).
    fn gearing_ship() -> Param {
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("PAUH911_Gearing_1945".to_string(), gearing_hull());
        components.engines.insert("PAUE903_D10_ENG_STOCK".to_string(), EngineComponentStats { speed_coef: Some(0.0) });
        components.artillery.insert("PAUA903_D10_ART_STOCK".to_string(), gearing_artillery());
        components.torpedoes.insert("PAUT902_D10_NEW_STOCK".to_string(), vec![gearing_launcher()]);
        components.fire_controls.insert("PAUS911_Suo".to_string(), 1.0);
        components.stock_selection = ShipUpgradeSelection::new(
            Some("PAUH911_Gearing_1945".to_string()),
            Some("PAUE903_D10_ENG_STOCK".to_string()),
            Some("PAUA903_D10_ART_STOCK".to_string()),
            Some("PAUT902_D10_NEW_STOCK".to_string()),
            Some("PAUS911_Suo".to_string()),
        );
        ship_with("PASD013_Gearing_1945", 10, Species::Destroyer, components)
    }

    fn gearing_provider() -> StubProvider {
        StubProvider::new(&[("PAPT027_Mk_16_mod_1", gearing_torpedo()), ("PAPA127_127mm_HE", gearing_he())])
    }

    /// Gearing fixture identical to [`gearing_ship`] except the artillery gun carries
    /// all four dispersion-curve fields so `dispersion_curve()` returns `Some`.
    fn gearing_ship_with_dispersion_curve() -> Param {
        let gun = || ArtilleryGunStats {
            radius_on_zero: Some(1.0),
            radius_on_delim: Some(1.4),
            radius_on_max: Some(1.8),
            delim: Some(0.5),
            ..gearing_artillery().guns[0].clone()
        };
        let mut arty = gearing_artillery();
        arty.guns = vec![gun(), gun(), gun()];
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("PAUH911_Gearing_1945".to_string(), gearing_hull());
        components.engines.insert("PAUE903_D10_ENG_STOCK".to_string(), EngineComponentStats { speed_coef: Some(0.0) });
        components.artillery.insert("PAUA903_D10_ART_STOCK".to_string(), arty);
        components.torpedoes.insert("PAUT902_D10_NEW_STOCK".to_string(), vec![gearing_launcher()]);
        components.fire_controls.insert("PAUS911_Suo".to_string(), 1.0);
        components.stock_selection = ShipUpgradeSelection::new(
            Some("PAUH911_Gearing_1945".to_string()),
            Some("PAUE903_D10_ENG_STOCK".to_string()),
            Some("PAUA903_D10_ART_STOCK".to_string()),
            Some("PAUT902_D10_NEW_STOCK".to_string()),
            Some("PAUS911_Suo".to_string()),
        );
        ship_with("PASD013_Gearing_1945", 10, Species::Destroyer, components)
    }

    #[test]
    fn artillery_dispersion_profile_resolves_and_evaluates() {
        let ship = gearing_ship_with_dispersion_curve();
        let sel = ShipUpgradeSelection::stock(&ship);
        let bundle = ModifierBundle::empty(Species::Destroyer);
        let profile = artillery_dispersion(&ship, &sel, &bundle).expect("profile");

        let provider = gearing_provider();
        let card = ship_stats(&ship, &sel, &bundle, 10, &provider).artillery.expect("arty");
        assert!((profile.max_range.value() - card.range.expect("range").value()).abs() < 1e-3);

        let e = profile.at_max_range();
        assert!((e.vertical.value() - e.horizontal.value() * 1.8).abs() < 1e-3);

        let near = profile.ellipse_at(Km::from(profile.max_range.value() / 2.0));
        assert!(near.horizontal.value() < e.horizontal.value());
    }

    #[test]
    fn artillery_dispersion_none_without_curve_or_battery() {
        let ship = gearing_ship();
        let sel = ShipUpgradeSelection::stock(&ship);
        assert!(artillery_dispersion(&ship, &sel, &ModifierBundle::empty(Species::Destroyer)).is_none());

        let no_arty = ShipUpgradeSelection { artillery: None, ..sel };
        assert!(artillery_dispersion(&ship, &no_arty, &ModifierBundle::empty(Species::Destroyer)).is_none());
    }

    #[test]
    fn stock_selection_picks_base_upgrades() {
        let ship = gearing_ship();
        let sel = ShipUpgradeSelection::stock(&ship);
        assert_eq!(sel.hull.as_deref(), Some("PAUH911_Gearing_1945"));
        assert_eq!(sel.engine.as_deref(), Some("PAUE903_D10_ENG_STOCK"));
        assert_eq!(sel.artillery.as_deref(), Some("PAUA903_D10_ART_STOCK"));
        // The torpedo stock is the empty-prev PAUT902, not the chained PAUT901.
        assert_eq!(sel.torpedoes.as_deref(), Some("PAUT902_D10_NEW_STOCK"));
        assert_eq!(sel.fire_control.as_deref(), Some("PAUS911_Suo"));
    }

    #[test]
    fn stock_selection_default_when_not_a_vehicle() {
        let proj = Param::builder()
            .id(GameParamId::from(1u32))
            .index("S0001".to_string())
            .name("X".to_string())
            .nation("USA".to_string())
            .data(ParamData::Projectile(gearing_torpedo()))
            .build();
        assert_eq!(ShipUpgradeSelection::stock(&proj), ShipUpgradeSelection::default());
    }

    #[test]
    fn gearing_stock_ship_stats_sections() {
        let ship = gearing_ship();
        let provider = gearing_provider();
        let stats = ship_stats_stock(&ship, &provider);

        // Durability: health 19400 (validated against the factory's gearing case).
        let durability = stats.durability.expect("durability");
        assert_eq!(durability.health.expect("health").value(), 19400.0);

        // Mobility: 36 kn (engine speedCoef 0.0, hull carries the full coef).
        let mobility = stats.mobility.expect("mobility");
        assert_eq!(mobility.speed.expect("speed").value(), 36.0);
        assert_eq!(mobility.turning_radius.expect("turning").value(), 640.0);

        // Torpedoes present: damage 53500/3 + 1200 = 19033.33.
        let torps = stats.torpedoes.expect("torpedoes");
        let damage = torps.torpedoes[0].damage.expect("torp damage").value();
        assert!((damage - (53500.0 / 3.0 + 1200.0)).abs() < 1e-1, "got {damage}");

        // Artillery present: stock reload 4.6, range 11.13 km.
        let arty = stats.artillery.expect("artillery");
        assert!((arty.reload_time.expect("reload").value() - 4.6).abs() < 1e-3);
        assert!((arty.range.expect("range").value() - 11.13).abs() < 1e-3);

        // Visibility: sea detection 7.33 km.
        let vis = stats.visibility.expect("visibility");
        assert!((vis.sea_detection.expect("sea").value() - 7.33).abs() < 1e-3);

        // A 127mm DD gun is small -> no big-gun visibility penalty: sea stays 7.33.
        // (A modifier-free bundle reads identity, so this is implicit, but assert the
        // gate by checking sea is unscaled.)

        // Gearing has no submarine battery / secondaries.
        assert!(stats.battery.is_none());
        assert!(stats.secondaries.is_none());
        // Fire control folds into artillery range; no standalone card.
        assert!(stats.fire_control.is_none());
    }

    #[test]
    fn fire_control_coef_scales_artillery_range() {
        // An FC upgrade carrying maxDistCoef 1.2 scales main-battery range by 1.2.
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("PAUH911_Gearing_1945".to_string(), gearing_hull());
        components.engines.insert("PAUE903_D10_ENG_STOCK".to_string(), EngineComponentStats { speed_coef: Some(0.0) });
        components.artillery.insert("PAUA903_D10_ART_STOCK".to_string(), gearing_artillery());
        components.fire_controls.insert("PAUS911_Suo".to_string(), 1.2);
        components.stock_selection = ShipUpgradeSelection::new(
            Some("PAUH911_Gearing_1945".to_string()),
            Some("PAUE903_D10_ENG_STOCK".to_string()),
            Some("PAUA903_D10_ART_STOCK".to_string()),
            None,
            Some("PAUS911_Suo".to_string()),
        );
        let ship = ship_with("PASD013_Gearing_1945", 10, Species::Destroyer, components);
        let provider = gearing_provider();
        let stats = ship_stats_stock(&ship, &provider);
        let range = stats.artillery.expect("artillery").range.expect("range").value();
        // 11.13 * 1.2 (fc coef) * 1.0 (GMMaxDist) = 13.356.
        assert!((range - 13.356).abs() < 1e-2, "got {range}");
    }

    #[test]
    fn big_gun_visibility_gate_uses_caliber() {
        // A 152mm gun (>= SMALL_SHELL_MAX_DIAMETER_M) is a big gun; with a non-stock
        // GMBigGunVisibilityCoeff the sea detection takes the penalty. Build a ship
        // whose artillery is 152mm and apply the coefficient via an explicit bundle.
        use crate::game_params::types::CrewSkillModifier;
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("H".to_string(), gearing_hull());
        let big_gun = ArtilleryGunStats { barrel_diameter: Some(Meters::from(0.152)), ..ArtilleryGunStats::default() };
        components.artillery.insert(
            "A".to_string(),
            ArtilleryComponentStats { max_dist: Some(Meters::from(15000.0)), guns: vec![big_gun] },
        );
        components.stock_selection =
            ShipUpgradeSelection::new(Some("H".to_string()), None, Some("A".to_string()), None, None);
        let ship = ship_with("BigGun", 10, Species::Cruiser, components);
        let provider = StubProvider::new(&[]);

        let modifier = CrewSkillModifier::builder()
            .name("GMBigGunVisibilityCoeff".to_string())
            .aircraft_carrier(1.05)
            .auxiliary(1.05)
            .battleship(1.05)
            .cruiser(1.05)
            .destroyer(1.05)
            .submarine(1.05)
            .excluded_consumables(Vec::new())
            .build();
        let bundle =
            ModifierBundle::from_modifiers(&[modifier], Species::Cruiser, crate::data::Version::base(15, 0, 0))
                .expect("test modifiers are all known");
        let sel = ShipUpgradeSelection::stock(&ship);
        let stats = ship_stats(&ship, &sel, &bundle, 10, &provider);
        let sea = stats.visibility.expect("visibility").sea_detection.expect("sea").value();
        // 7.33 * 1.05 (big-gun penalty applies because the gun is 152mm) = 7.6965.
        assert!((sea - 7.6965).abs() < 1e-3, "got {sea}");
    }

    #[test]
    fn non_vehicle_yields_empty_stats() {
        let proj = Param::builder()
            .id(GameParamId::from(1u32))
            .index("S0001".to_string())
            .name("X".to_string())
            .nation("USA".to_string())
            .data(ParamData::Projectile(gearing_torpedo()))
            .build();
        let provider = StubProvider::new(&[]);
        let stats = ship_stats(
            &proj,
            &ShipUpgradeSelection::default(),
            &ModifierBundle::empty(Species::Destroyer),
            10,
            &provider,
        );
        assert!(stats.durability.is_none());
        assert!(stats.artillery.is_none());
    }

    #[test]
    fn stock_stats_default_when_species_unknown() {
        // A vehicle whose species is Unknown has no modifier context; ship_stats_stock
        // returns the default stat card rather than guessing a species.
        let mut components = ShipTtxComponents::default();
        components.hulls.insert("PAUH911_Gearing_1945".to_string(), gearing_hull());
        components.stock_selection =
            ShipUpgradeSelection::new(Some("PAUH911_Gearing_1945".to_string()), None, None, None, None);
        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .maybe_abilities(None)
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .ttx_components(components)
            .innate_skills(Vec::new())
            .build();
        let ship = Param::builder()
            .id(GameParamId::from(901u32))
            .index("IDX".to_string())
            .name("UnknownSpecies".to_string())
            .nation("USA".to_string())
            .species(crate::recognized::Recognized::Unknown("MadeUpSpecies".to_string()))
            .data(ParamData::Vehicle(vehicle))
            .build();
        let provider = StubProvider::new(&[]);
        let stats = ship_stats_stock(&ship, &provider);
        // Default card: even though a hull is present, the unknown species short-circuits
        // before any factory runs, so every section is None.
        assert!(stats.durability.is_none());
        assert!(stats.mobility.is_none());
        assert!(stats.artillery.is_none());
        assert!(stats.visibility.is_none());
    }

    use crate::game_params::ttx::components::SecondaryComponentStats;

    /// 150 mm secondary gun with full dispersion curve and one HE ammo.
    fn g150() -> ArtilleryGunStats {
        ArtilleryGunStats {
            shot_delay: Some(Seconds::from(7.5)),
            rotation_speed: Some(DegreesPerSecond::from(10.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.15)),
            ammo_switch_coeff: None,
            min_radius: Some(2.0),
            ideal_radius: Some(15.0),
            ideal_distance: Some(1000.0),
            radius_on_zero: Some(1.0),
            radius_on_delim: Some(1.4),
            radius_on_max: Some(1.8),
            delim: Some(0.5),
            ammo: vec!["SEC_150mm_HE".to_string()],
        }
    }

    /// 105 mm secondary gun with full dispersion curve and one HE ammo.
    fn g105() -> ArtilleryGunStats {
        ArtilleryGunStats {
            shot_delay: Some(Seconds::from(3.5)),
            rotation_speed: Some(DegreesPerSecond::from(12.0)),
            num_barrels: Some(1.0),
            barrel_diameter: Some(Meters::from(0.105)),
            ammo_switch_coeff: None,
            min_radius: Some(1.5),
            ideal_radius: Some(12.0),
            ideal_distance: Some(1000.0),
            radius_on_zero: Some(1.0),
            radius_on_delim: Some(1.35),
            radius_on_max: Some(1.7),
            delim: Some(0.5),
            ammo: vec!["SEC_105mm_HE".to_string()],
        }
    }

    fn sec150_he() -> crate::game_params::types::Projectile {
        crate::game_params::types::Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(2100.0)
            .alpha_piercing_he(25.0)
            .burn_prob(0.12)
            .uw_critical(0.0)
            .bullet_diametr(0.15)
            .bullet_speed(800.0)
            .build()
    }

    fn sec105_he() -> crate::game_params::types::Projectile {
        crate::game_params::types::Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(1200.0)
            .alpha_piercing_he(17.0)
            .burn_prob(0.07)
            .uw_critical(0.0)
            .bullet_diametr(0.105)
            .bullet_speed(900.0)
            .build()
    }

    fn secondaries_provider() -> StubProvider {
        StubProvider::new(&[("SEC_150mm_HE", sec150_he()), ("SEC_105mm_HE", sec105_he())])
    }

    /// A battleship hull with an ATBA component carrying two distinct calibers
    /// (four guns: two 150 mm, two 105 mm). Secondaries are keyed by the hull
    /// name so `components.secondaries(hull_name)` resolves correctly.
    fn secondaries_ship() -> crate::game_params::types::Param {
        let hull_name = "PGSH010_Hull_B";
        let mut components = ShipTtxComponents::default();
        components.hulls.insert(hull_name.to_string(), gearing_hull());
        components.engines.insert("PGSE_Engine".to_string(), EngineComponentStats { speed_coef: Some(0.0) });
        components.secondaries.insert(
            hull_name.to_string(),
            SecondaryComponentStats {
                max_dist: Some(Meters::from(7600.0)),
                guns: vec![g150(), g150(), g105(), g105()],
            },
        );
        components.stock_selection =
            ShipUpgradeSelection::new(Some(hull_name.to_string()), Some("PGSE_Engine".to_string()), None, None, None);
        ship_with("PGSB_SecondaryBB", 10, Species::Battleship, components)
    }

    #[test]
    fn secondaries_bearing_card_coverage_and_replay() {
        use crate::game_params::ttx::provenance::ShipStatsProvenance;
        let ship = secondaries_ship();
        let provider = secondaries_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let (stats, prov) = ship_stats_explained(
            &ship,
            &sel,
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            10,
            &provider,
        );

        let battery = stats.secondaries.as_ref().expect("secondaries");
        assert_eq!(battery.mounts.len(), 2);

        use std::collections::HashSet;
        let row_keys: HashSet<_> = stats.rows().into_iter().map(|r| (r.stat, r.qualifier)).collect();
        let prov_keys: HashSet<_> = prov.attributions.iter().map(|a| (a.stat, a.qualifier.clone())).collect();
        assert_eq!(row_keys, prov_keys, "every secondary stat row must have exactly one attribution");

        for a in &prov.attributions {
            let replayed = ShipStatsProvenance::replay(a);
            assert!(
                (replayed - a.value).abs() <= 1e-2 + a.value.abs() * 1e-4,
                "replay mismatch for {:?} ({:?}): replayed={} value={}",
                a.stat,
                a.qualifier,
                replayed,
                a.value
            );
        }
    }

    fn test_version() -> crate::data::Version {
        crate::data::Version::base(15, 4, 0)
    }

    fn uniform_modifier(name: &str, value: f32) -> crate::game_params::types::CrewSkillModifier {
        crate::game_params::types::CrewSkillModifier::builder()
            .name(name.to_owned())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    #[test]
    fn effective_modifiers_default_state_matches_ship_stats_empty_bundle() {
        let ship = gearing_ship();
        let provider = gearing_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let level = 10u32;

        let base = ship_stats(&ship, &sel, &ModifierBundle::empty(Species::Destroyer), level, &provider);

        let em = crate::game_params::ttx::effects::Effects::for_test(vec![])
            .resolve(&crate::game_params::ttx::effects::EffectsState::default(), Species::Destroyer, test_version())
            .unwrap();
        let under_em = em.stats(&ship, &sel, level, &provider);

        let base_reload = base.artillery.as_ref().and_then(|a| a.reload_time).map(|s| s.value());
        let em_reload = under_em.artillery.as_ref().and_then(|a| a.reload_time).map(|s| s.value());
        assert_eq!(base_reload, em_reload, "reload must match with identity coefficients");

        let base_range = base.artillery.as_ref().and_then(|a| a.range).map(|r| r.value());
        let em_range = under_em.artillery.as_ref().and_then(|a| a.range).map(|r| r.value());
        assert_eq!(base_range, em_range, "range must match with identity spotter coeff");
    }

    #[test]
    fn adrenaline_reload_half_hp_multiplies_main_reload() {
        use crate::game_params::ttx::effects::Effect;
        use crate::game_params::ttx::effects::EffectActivation;
        use crate::game_params::ttx::effects::EffectId;
        use crate::game_params::ttx::effects::EffectKind;
        use crate::game_params::ttx::effects::Effects;
        use crate::game_params::ttx::effects::EffectsState;
        use crate::game_params::ttx::effects::HealthFraction;

        let ship = gearing_ship();
        let provider = gearing_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let level = 10u32;

        let raw_coeff = 0.2f32;
        let adrenaline = Effect::for_test(
            EffectId::Skill("ArmamentReloadAaDamage".into()),
            EffectKind::HealthScaledReload,
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", raw_coeff)],
        );
        let effects = Effects::for_test(vec![adrenaline]);

        let state_full = EffectsState::default();
        let em_full = effects.resolve(&state_full, Species::Destroyer, test_version()).unwrap();
        let reload_full = em_full
            .stats(&ship, &sel, level, &provider)
            .artillery
            .and_then(|a| a.reload_time)
            .map(|s| s.value())
            .unwrap();

        let state_half = EffectsState::default()
            .set(EffectId::Skill("ArmamentReloadAaDamage".into()), EffectActivation::Health(HealthFraction::new(0.5)));
        let em_half = effects.resolve(&state_half, Species::Destroyer, test_version()).unwrap();
        let reload_half = em_half
            .stats(&ship, &sel, level, &provider)
            .artillery
            .and_then(|a| a.reload_time)
            .map(|s| s.value())
            .unwrap();

        let expected_coeff = 1.0 - 0.5 * raw_coeff;
        let expected_reload = reload_full * expected_coeff;
        assert!((reload_half - expected_reload).abs() < 1e-4, "got {reload_half}, expected {expected_reload}");
        assert!(reload_half < reload_full, "adrenaline at 50% HP must reduce reload");
    }

    #[test]
    fn spotter_consumable_extends_range_and_off_reverts_to_base() {
        use crate::game_params::ttx::effects::Effect;
        use crate::game_params::ttx::effects::EffectActivation;
        use crate::game_params::ttx::effects::EffectId;
        use crate::game_params::ttx::effects::EffectKind;
        use crate::game_params::ttx::effects::Effects;
        use crate::game_params::ttx::effects::EffectsState;

        let ship = gearing_ship();
        let provider = gearing_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let level = 10u32;

        let dist_coeff = 1.2f32;
        let scout_effect = Effect::for_test(
            EffectId::Consumable(crate::game_types::Consumable::SpottingAircraft),
            EffectKind::Consumable { artillery_dist_coeff: dist_coeff },
            vec![uniform_modifier("GMIdealRadius", 0.9)],
        );
        let effects = Effects::for_test(vec![scout_effect]);

        let state_off = EffectsState::default();
        let em_off = effects.resolve(&state_off, Species::Destroyer, test_version()).unwrap();
        let range_off =
            em_off.stats(&ship, &sel, level, &provider).artillery.and_then(|a| a.range).map(|r| r.value()).unwrap();

        let state_on = EffectsState::default()
            .set(EffectId::Consumable(crate::game_types::Consumable::SpottingAircraft), EffectActivation::On);
        let em_on = effects.resolve(&state_on, Species::Destroyer, test_version()).unwrap();
        let range_on =
            em_on.stats(&ship, &sel, level, &provider).artillery.and_then(|a| a.range).map(|r| r.value()).unwrap();

        let base_range = ship_stats(&ship, &sel, &ModifierBundle::empty(Species::Destroyer), level, &provider)
            .artillery
            .and_then(|a| a.range)
            .map(|r| r.value())
            .unwrap();

        // Build a reference On state without the GMIdealRadius modifier to isolate its effect.
        let scout_no_radius_mod = Effect::for_test(
            EffectId::Consumable(crate::game_types::Consumable::SpottingAircraft),
            EffectKind::Consumable { artillery_dist_coeff: dist_coeff },
            vec![],
        );
        let effects_no_radius = Effects::for_test(vec![scout_no_radius_mod]);
        let em_on_no_radius = effects_no_radius.resolve(&state_on, Species::Destroyer, test_version()).unwrap();
        let dispersion_on =
            em_on.stats(&ship, &sel, level, &provider).artillery.and_then(|a| a.dispersion).map(|d| d.value()).unwrap();
        let dispersion_on_no_radius = em_on_no_radius
            .stats(&ship, &sel, level, &provider)
            .artillery
            .and_then(|a| a.dispersion)
            .map(|d| d.value())
            .unwrap();

        assert!((range_off - base_range).abs() < 1e-4, "spotter off: got {range_off}, expected {base_range}");
        assert!(
            (range_on - base_range * dist_coeff).abs() < 1e-3,
            "spotter on: got {range_on}, expected {}",
            base_range * dist_coeff
        );
        assert!(range_on > range_off, "spotter on must increase range");
        assert!(
            dispersion_on < dispersion_on_no_radius,
            "GMIdealRadius 0.9 must tighten dispersion vs no-modifier baseline: on={dispersion_on}, baseline={dispersion_on_no_radius}"
        );
    }

    #[test]
    fn explained_provenance_covers_every_row() {
        let ship = gearing_ship();
        let provider = gearing_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let (stats, prov) = ship_stats_explained(
            &ship,
            &sel,
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &provider,
        );

        use std::collections::HashSet;
        let row_keys: HashSet<_> = stats.rows().into_iter().map(|r| (r.stat, r.qualifier)).collect();
        let prov_keys: HashSet<_> = prov.attributions.iter().map(|a| (a.stat, a.qualifier.clone())).collect();
        assert_eq!(row_keys, prov_keys, "every stat row must have exactly one attribution");
    }

    #[test]
    fn explained_provenance_replays_to_value() {
        use crate::game_params::ttx::provenance::ShipStatsProvenance;
        let ship = gearing_ship();
        let provider = gearing_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let (_stats, prov) = ship_stats_explained(
            &ship,
            &sel,
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &provider,
        );
        for a in &prov.attributions {
            let replayed = ShipStatsProvenance::replay(a);
            assert!(
                (replayed - a.value).abs() <= 1e-2 + a.value.abs() * 1e-4,
                "replay mismatch for {:?}: {} vs {}",
                a.stat,
                replayed,
                a.value
            );
        }
    }

    #[test]
    fn derived_from_links_resolve_to_existing_rows() {
        use crate::game_params::ttx::labels::TtxStat;
        use crate::game_params::ttx::provenance::StatKey;
        use std::collections::HashSet;
        let ship = gearing_ship();
        let provider = gearing_provider();
        let sel = ShipUpgradeSelection::stock(&ship);
        let (_stats, prov) = ship_stats_explained(
            &ship,
            &sel,
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &provider,
        );
        let keys: HashSet<StatKey> =
            prov.attributions.iter().map(|a| StatKey { stat: a.stat, qualifier: a.qualifier.clone() }).collect();
        for a in &prov.attributions {
            for link in &a.derived_from {
                assert!(keys.contains(link), "derived_from {:?} of {:?} has no matching attribution", link, a.stat);
            }
        }
        let rt = prov.attributions.iter().find(|a| a.stat == TtxStat::GunRotationTime).expect("rotation time");
        assert_eq!(rt.derived_from, vec![StatKey { stat: TtxStat::GunRotationSpeed, qualifier: None }]);
    }
}

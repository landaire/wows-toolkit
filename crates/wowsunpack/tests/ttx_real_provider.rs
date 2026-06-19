//! End-to-end validation of the TTX ship-stats engine against a REAL
//! `GameMetadataProvider` built from the installed game's GameParams.
//!
//! This exercises the full `ship_stats_stock` pipeline (stock-module selection,
//! reference-chain resolution, factory assembly) across ship classes, which the
//! per-factory unit tests cannot cover. It is GATED behind `#[ignore]` so a plain
//! `cargo test` on CI (no game data) skips it.
//!
//! The provider is built fresh from the install's `idx`/`res_packages` (the same
//! `IdxVfs` over mmap'd pkgs the live app uses), so it is immune to params-cache
//! schema drift. This needs the `vfs-mmap` feature (on by default via `bin`). Run:
//!   cargo test -p wowsunpack --test ttx_real_provider -- --ignored --nocapture
//!
//! Install resolution order:
//!   1. `$WOWS_DIR` (the `World_of_Warships` root); defaults to `E:\WoWs\World_of_Warships`.
//!   2. `$WOWS_BUILD` (a build number under `bin\`); defaults to the newest build dir.

use std::path::Path;

use wowsunpack::data::idx;
use wowsunpack::data::idx_vfs::IdxVfs;
use wowsunpack::data::wrappers::mmap::MmapPkgSource;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::ttx::ship_stats_stock;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

/// One expected-vs-computed check; a tolerance band around a published port value.
struct Check {
    label: &'static str,
    expected: f32,
    got: Option<f32>,
    tol: f32,
}

impl Check {
    fn matches(&self) -> Option<bool> {
        self.got.map(|g| (g - self.expected).abs() <= self.tol)
    }
}

/// The newest build directory under `<wows>/bin` (largest numeric name).
fn newest_build(wows_dir: &Path) -> Option<String> {
    let mut builds: Vec<u64> = std::fs::read_dir(wows_dir.join("bin"))
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u64>().ok())
        .collect();
    builds.sort_unstable();
    builds.last().map(|b| b.to_string())
}

fn load_provider() -> Option<GameMetadataProvider> {
    let wows_dir = std::env::var("WOWS_DIR").unwrap_or_else(|_| r"E:\WoWs\World_of_Warships".to_string());
    let wows_dir = Path::new(&wows_dir);
    if !wows_dir.is_dir() {
        eprintln!("WoWs dir not found: {}", wows_dir.display());
        return None;
    }
    let build = std::env::var("WOWS_BUILD").ok().or_else(|| newest_build(wows_dir))?;
    let build_dir = wows_dir.join("bin").join(&build);
    eprintln!("building IdxVfs: dir={} build={build}", wows_dir.display());

    let mut idx_files = Vec::new();
    for entry in std::fs::read_dir(build_dir.join("idx")).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() {
            let bytes = std::fs::read(&path).ok()?;
            idx_files.push(idx::parse(&bytes).ok()?);
        }
    }
    let pkg_source = MmapPkgSource::new(wows_dir.join("res_packages"));
    let vfs = VfsPath::new(IdxVfs::new(pkg_source, &idx_files));

    let provider = GameMetadataProvider::from_vfs(&vfs).ok()?;
    eprintln!("parsed {} params", provider.params().len());
    Some(provider)
}

/// Run the cross-class validation: compute `ship_stats_stock` for each ship and
/// compare to known/published port values. Prints a results table; fails if any
/// check mismatches (an unresolved `got` is reported and counts as a mismatch).
#[test]
#[ignore]
fn ttx_cross_class_against_port_values() {
    let Some(provider) = load_provider() else {
        panic!("could not build a provider; set WOWS_DIR to your World_of_Warships install");
    };

    let mut total = 0usize;
    let mut mismatches = 0usize;

    for ship_name in [
        "PASD013_Gearing_1945",
        "PASC210_Worcester",
        "PJSB018_Yamato_1944",
        "PGSB108_Bismarck",
        "PJSD012_Shimakaze_1943",
        "PASC020_Des_Moines_1948",
    ] {
        let ship = provider.game_param_by_name(ship_name).unwrap_or_else(|| panic!("ship {ship_name} not in params"));
        let stats = ship_stats_stock(&ship, &provider);
        let checks = ship_checks(ship_name, &stats);

        eprintln!("\n=== {ship_name} ===");
        eprintln!("{:<26} {:>12} {:>12} {:>8}", "stat", "expected", "computed", "match");
        for c in &checks {
            total += 1;
            let got_str = c.got.map(|g| format!("{g:.3}")).unwrap_or_else(|| "None".to_string());
            let verdict = match c.matches() {
                Some(true) => "ok",
                Some(false) => {
                    mismatches += 1;
                    "MISMATCH"
                }
                None => {
                    mismatches += 1;
                    "MISSING"
                }
            };
            eprintln!("{:<26} {:>12.3} {:>12} {:>8}", c.label, c.expected, got_str, verdict);
        }
    }

    eprintln!("\n{total} checks, {mismatches} mismatch(es)");
    assert_eq!(mismatches, 0, "{mismatches} of {total} stat checks diverged from port values");
}

/// The known port-value checks per ship. Tolerances allow for rounding/display.
fn ship_checks(ship: &str, stats: &wowsunpack::game_params::ttx::model::ShipStats) -> Vec<Check> {
    let durability = stats.durability.as_ref();
    let mobility = stats.mobility.as_ref();
    let arty = stats.artillery.as_ref();
    let torps = stats.torpedoes.as_ref();
    let secn = stats.secondaries.as_ref();
    let vis = stats.visibility.as_ref();

    let hp = durability.and_then(|d| d.health.map(|h| h.value()));
    let speed = mobility.and_then(|m| m.speed.map(|s| s.value()));
    let arty_reload = arty.and_then(|a| a.reload_time.map(|r| r.value()));
    let arty_range = arty.and_then(|a| a.range.map(|r| r.value()));
    let arty_caliber = arty.and_then(|a| a.gun.as_ref()).and_then(|g| g.caliber.map(|c| c.value()));
    let arty_dispersion = arty.and_then(|a| a.dispersion.map(|d| d.value()));
    let sea = vis.and_then(|v| v.sea_detection.map(|s| s.value()));
    let secn_range = secn.and_then(|s| s.range.map(|r| r.value()));

    // First HE/AP shell stats by ammo_kind.
    let he = arty.and_then(|a| a.shells.iter().find(|s| s.ammo_kind.as_deref() == Some("HE")));
    let ap = arty.and_then(|a| a.shells.iter().find(|s| s.ammo_kind.as_deref() == Some("AP")));
    let he_dmg = he.and_then(|s| s.damage.map(|d| d.value()));
    let he_pen = he.and_then(|s| s.penetration.map(|p| p.value()));
    let he_fire = he.and_then(|s| s.burn_chance.map(|b| b.value()));
    let ap_dmg = ap.and_then(|s| s.damage.map(|d| d.value()));

    // First torpedo stats.
    let torp = torps.and_then(|t| t.torpedoes.first());
    let torp_dmg = torp.and_then(|t| t.damage.map(|d| d.value()));
    let torp_range = torp.and_then(|t| t.range.map(|r| r.value()));
    let torp_speed = torp.and_then(|t| t.speed.map(|s| s.value()));

    match ship {
        "PASD013_Gearing_1945" => vec![
            Check { label: "HP", expected: 19400.0, got: hp, tol: 200.0 },
            Check { label: "speed (kn)", expected: 36.5, got: speed, tol: 0.6 },
            // Gearing's stock 127mm/38 Mk16 reload is 3.0s in port (A_Artillery
            // shotDelay=3.0); the task's 4.6 hint was a synthetic unit-test value.
            Check { label: "main reload (s)", expected: 3.0, got: arty_reload, tol: 0.2 },
            Check { label: "main caliber (mm)", expected: 127.0, got: arty_caliber, tol: 1.0 },
            Check { label: "torp damage", expected: 19033.0, got: torp_dmg, tol: 100.0 },
            Check { label: "torp range (km)", expected: 10.5, got: torp_range, tol: 0.3 },
            Check { label: "torp speed (kn)", expected: 66.0, got: torp_speed, tol: 1.0 },
            Check { label: "sea detect (km)", expected: 7.3, got: sea, tol: 0.2 },
        ],
        // Current-patch Worcester: A_Hull health is 45400 and stock A_Artillery
        // maxDist is 16710 (16.71 km); the task's 43900/15.32/138 hints predate a
        // rebalance. Dispersion 148 is the formula's value at the 16.71 km range.
        "PASC210_Worcester" => vec![
            Check { label: "HP", expected: 45400.0, got: hp, tol: 600.0 },
            Check { label: "main caliber (mm)", expected: 152.0, got: arty_caliber, tol: 1.0 },
            Check { label: "main range (km)", expected: 16.71, got: arty_range, tol: 0.4 },
            Check { label: "main reload (s)", expected: 4.6, got: arty_reload, tol: 0.2 },
            Check { label: "main dispersion (m)", expected: 148.3, got: arty_dispersion, tol: 6.0 },
            Check { label: "HE damage", expected: 2200.0, got: he_dmg, tol: 50.0 },
            Check { label: "HE pen (mm)", expected: 30.0, got: he_pen, tol: 2.0 },
            Check { label: "HE fire (%)", expected: 12.0, got: he_fire, tol: 1.0 },
            Check { label: "AP damage", expected: 3200.0, got: ap_dmg, tol: 50.0 },
        ],
        "PJSB018_Yamato_1944" => vec![
            Check { label: "main caliber (mm)", expected: 460.0, got: arty_caliber, tol: 1.0 },
            Check { label: "main dispersion (m)", expected: 273.0, got: arty_dispersion, tol: 8.0 },
            Check { label: "AP damage", expected: 14800.0, got: ap_dmg, tol: 200.0 },
        ],
        "PGSB108_Bismarck" => vec![
            Check { label: "main caliber (mm)", expected: 380.0, got: arty_caliber, tol: 1.0 },
            Check { label: "secondary range (km)", expected: 7.6, got: secn_range, tol: 0.3 },
        ],
        // Shimakaze's STOCK 20km torpedo (Type93) is 62 kn; the 67 kn hint is the
        // researched (non-stock) torpedo. Stock maxDist 667 BW -> 20.01 km.
        "PJSD012_Shimakaze_1943" => vec![
            Check { label: "torp range (km)", expected: 20.0, got: torp_range, tol: 0.5 },
            Check { label: "torp speed (kn)", expected: 62.0, got: torp_speed, tol: 1.0 },
        ],
        "PASC020_Des_Moines_1948" => vec![
            Check { label: "HP", expected: 50600.0, got: hp, tol: 700.0 },
            Check { label: "speed (kn)", expected: 33.0, got: speed, tol: 0.6 },
            Check { label: "main caliber (mm)", expected: 203.0, got: arty_caliber, tol: 1.0 },
            Check { label: "main dispersion (m)", expected: 143.0, got: arty_dispersion, tol: 6.0 },
        ],
        _ => Vec::new(),
    }
}

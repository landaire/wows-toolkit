//! Unit-conversion constants used by the TTX factories.
//!
//! Literal constants are transcribed from the deob `me658a8e4.py`. The BigWorld
//! C++-native scales are not present in any deob source (they are imported from the
//! `BigWorld` engine module); the ones used here are recovered by solving a client
//! formula against a known in-game port value, documented per constant.

/// `KM_TO_M = 1000.0` (me658a8e4.py:41).
pub const KM_TO_M: f32 = 1000.0;

/// `KNOTS_TO_MPS = 1.0 / 3.0` (me658a8e4.py:39).
pub const KNOTS_TO_MPS: f32 = 1.0 / 3.0;

/// `MPS_TO_KNOTS = 1.0 / KNOTS_TO_MPS` = 3.0 (me658a8e4.py:40).
pub const MPS_TO_KNOTS: f32 = 1.0 / KNOTS_TO_MPS;

/// `SHIP_TIME_SCALE = 2.0` (me658a8e4.py:43).
pub const SHIP_TIME_SCALE: f32 = 2.0;

/// `SHIP_TIME_SCALE_INV = 1 / SHIP_TIME_SCALE` = 0.5 (me658a8e4.py:44).
pub const SHIP_TIME_SCALE_INV: f32 = 1.0 / SHIP_TIME_SCALE;

/// `TORPEDO_DAMAGE_CONSTANT = 3.0` (me658a8e4.py:90). Divisor in torpedo damage
/// `(alphaDamage / 3 + damage)`.
pub const TORPEDO_DAMAGE_CONSTANT: f32 = 3.0;

/// `HULL_HEALTH_ROUND = 50` (me658a8e4.py:50). Hull health rounds up to a multiple of 50.
pub const HULL_HEALTH_ROUND: f32 = 50.0;

/// `DEFAULT_UW_DAMAGE_COEFF = 0.333` (ma779114d.py constant; binary-float value
/// recovered from the compiled module). Used in `PreprocessedHull.py:12` to derive
/// hull `floodProb` from `floodNodes[0][0]`; a hull whose `floodNodes[0][0]` equals
/// this constant has no torpedo protection (floodProb 0.0).
pub const DEFAULT_UW_DAMAGE_COEFF: f32 = 0.333;

/// BigWorld -> ballistic-distance scale. Not in any deob source (C++ engine constant).
///
/// Recovered by solving the torpedo-range formula `range_km = maxDist * BW_TO_BALLISTIC
/// / KM_TO_M` against two known in-game torpedo ranges:
///   - Gearing/Fletcher `PAPT027_Mk_16_mod_1` maxDist=350 -> 10.5 km port range
///     => BW_TO_BALLISTIC = 10500 / 350 = 30.0.
///   - Shimakaze `PJPT001_Sea_Torpedo_Type93` maxDist=667 -> 20.0 km port range:
///     667 * 30.0 / 1000 = 20.01 km (matches).
///
/// The deob independently encodes this: `VisibilityDistance.SHIP_BY_SHIP = 100.0/3`
/// and `bwToBallisticKilometers = 1.0 / SHIP_BY_SHIP` (me658a8e4.py:143,233), so
/// `BW_TO_BALLISTIC = KM_TO_M / SHIP_BY_SHIP = 1000 / (100/3) = 30.0`.
pub const BW_TO_BALLISTIC: f32 = 30.0;

/// `BALLISTIC_TO_BW = 1 / BW_TO_BALLISTIC`. Inverse of the recovered scale above.
pub const BALLISTIC_TO_BW: f32 = 1.0 / BW_TO_BALLISTIC;

/// BigWorld -> ship-distance scale. Not in any deob source: imported from the C++
/// `BigWorld` engine module (`from BigWorld import ... BW_TO_SHIP, SHIP_TO_BW`,
/// me658a8e4.py:3), so recovered by solving the main-battery dispersion formula
/// (FactoryArtillery.py:109) against two ships' stock port "Maximum Dispersion":
///   - North Carolina (PASB012): gun minRadius=2.0 idealRadius=12.0 idealDistance=1000.0,
///     A_Artillery.maxDist=21143 -> 21.143 km; port dispersion 271 m.
///     base = (2 + 21.143 * BALLISTIC_TO_BW * KM_TO_M * (12-2)/1000) * 2 = 18.0953;
///     271 / 18.0953 = 14.976.
///   - Yamato (PJSB018): gun minRadius=2.8 idealRadius=10.0 idealDistance=1000.0,
///     A_Artillery.maxDist=26630 -> 26.63 km; port dispersion 273 m.
///     base = (2.8 + 26.63 * BALLISTIC_TO_BW * KM_TO_M * (10-2.8)/1000) * 2 = 18.3824;
///     273 / 18.3824 = 14.851.
///
/// Both (different gun fields) recover ~15 within ~0.8%; at BW_TO_SHIP=15.0 the formula
/// yields NC=271.4 m and Yamato=275.7 m (published values rounded to whole meters).
pub const BW_TO_SHIP: f32 = 15.0;

/// `SHIP_TO_BW = 1 / BW_TO_SHIP`. Inverse of the recovered scale above.
pub const SHIP_TO_BW: f32 = 1.0 / BW_TO_SHIP;

/// Main-battery horizontal dispersion in meters (FactoryArtillery.py:105-110
/// `getDispersionValue`, identical to the alt-fire inline at line 46).
///
/// `componentParams.minRadius/idealRadius` are gun fields; `idealDistance` is a gun
/// field; `maxDist` is the battery range in KM (PreprocessedArtillery.py:32 stores
/// `module.maxDist / KM_TO_M`); `ideal_radius_coef` is the `GMIdealRadius` modifier
/// (stock 1.0).
pub fn dispersion(
    min_radius: f32,
    ideal_radius: f32,
    ideal_distance: f32,
    max_dist_km: f32,
    ideal_radius_coef: f32,
) -> f32 {
    let min_r = min_radius * ideal_radius_coef;
    let ideal_r = ideal_radius * ideal_radius_coef;
    (min_r + max_dist_km * BALLISTIC_TO_BW * KM_TO_M * (ideal_r - min_r) / ideal_distance) * BW_TO_SHIP * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_constants() {
        assert_eq!(KM_TO_M, 1000.0);
        assert_eq!(KNOTS_TO_MPS, 1.0 / 3.0);
        assert_eq!(MPS_TO_KNOTS, 3.0);
        assert_eq!(SHIP_TIME_SCALE, 2.0);
        assert_eq!(SHIP_TIME_SCALE_INV, 0.5);
        assert_eq!(TORPEDO_DAMAGE_CONSTANT, 3.0);
        assert_eq!(HULL_HEALTH_ROUND, 50.0);
        assert_eq!(DEFAULT_UW_DAMAGE_COEFF, 0.333);
    }

    #[test]
    fn bw_to_ballistic_inverse() {
        assert!((BALLISTIC_TO_BW - 1.0 / 30.0).abs() < 1e-9);
    }

    #[test]
    fn gearing_torpedo_range() {
        // PAPT027_Mk_16_mod_1 maxDist=350 -> Gearing/Fletcher in-game range 10.5 km.
        let range_km = 350.0 * BW_TO_BALLISTIC / KM_TO_M;
        assert!((range_km - 10.5).abs() < 0.1, "got {range_km}");
    }

    #[test]
    fn shimakaze_torpedo_range() {
        // PJPT001_Sea_Torpedo_Type93 maxDist=667 -> Shimakaze in-game range 20.0 km.
        let range_km = 667.0 * BW_TO_BALLISTIC / KM_TO_M;
        assert!((range_km - 20.0).abs() < 0.1, "got {range_km}");
    }

    #[test]
    fn bw_to_ship_inverse() {
        assert!((SHIP_TO_BW - 1.0 / 15.0).abs() < 1e-9);
    }

    #[test]
    fn north_carolina_dispersion() {
        // PASB012 gun 2.0/12.0/1000.0, A_Artillery.maxDist=21143 (21.143 km), stock c=1.0.
        // Port "Maximum Dispersion" 271 m.
        let d = dispersion(2.0, 12.0, 1000.0, 21143.0 / 1000.0, 1.0);
        assert!((d - 271.0).abs() < 1.0, "got {d}");
    }

    #[test]
    fn yamato_dispersion() {
        // PJSB018 gun 2.8/10.0/1000.0, A_Artillery.maxDist=26630 (26.63 km), stock c=1.0.
        // Port "Maximum Dispersion" 273 m; formula yields ~275.7 (published rounded).
        let d = dispersion(2.8, 10.0, 1000.0, 26630.0 / 1000.0, 1.0);
        assert!((d - 273.0).abs() < 3.0, "got {d}");
    }
}

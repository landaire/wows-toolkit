//! Pure layout math for the stats panel and team roster, shared by the
//! egui and pixmap render backends so the two stay in lockstep and the
//! arithmetic is unit-tested in one place.

/// Fractions of a horizontal meter for the silhouette HP overlay.
///
/// `colored` is the current-HP portion (drawn in the HP color). `healable_bright`
/// is the HP the active/next heal charge will restore, drawn immediately after
/// it; `healable_dim` is the remaining regenerable pool beyond that charge,
/// drawn darker. The remainder (unhealable lost HP) is left transparent. All
/// values are clamped to `[0, 1]` and the returned fractions never sum past 1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilhouetteRegions {
    pub colored: f32,
    pub healable_bright: f32,
    pub healable_dim: f32,
}

pub fn silhouette_regions(
    hp_current: f32,
    hp_healable: f32,
    hp_healable_per_charge: f32,
    hp_max: f32,
) -> SilhouetteRegions {
    if hp_max <= 0.0 {
        return SilhouetteRegions { colored: 0.0, healable_bright: 0.0, healable_dim: 0.0 };
    }
    let colored = (hp_current / hp_max).clamp(0.0, 1.0);
    let lost = (1.0 - colored).max(0.0);
    let total = (hp_healable / hp_max).clamp(0.0, lost);
    let bright = (hp_healable_per_charge / hp_max).clamp(0.0, total);
    let dim = (total - bright).max(0.0);
    SilhouetteRegions { colored, healable_bright: bright, healable_dim: dim }
}

/// Total current HP over total possible HP for one roster side.
///
/// Numerator sums current HP (floored at 0 so dead/negative rows do not add
/// back); denominator sums max HP including dead ships, so the bar drains as
/// the team dies. Returns `None` when no max HP is known (avoids div-by-zero).
pub fn team_hp_fraction(rows: impl IntoIterator<Item = (f32, f32)>) -> Option<f32> {
    let mut total_current = 0.0f32;
    let mut total_max = 0.0f32;
    for (current, max) in rows {
        total_current += current.max(0.0);
        total_max += max.max(0.0);
    }
    if total_max <= 0.0 {
        return None;
    }
    Some((total_current / total_max).clamp(0.0, 1.0))
}

/// Scale an RGB color toward black by `factor` in `[0, 1]` (1.0 = unchanged).
pub fn darken(color: [u8; 3], factor: f32) -> [u8; 3] {
    let f = factor.clamp(0.0, 1.0);
    [(color[0] as f32 * f) as u8, (color[1] as f32 * f) as u8, (color[2] as f32 * f) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silhouette_full_hp_is_all_colored() {
        let r = silhouette_regions(100.0, 0.0, 0.0, 100.0);
        assert_eq!(r, SilhouetteRegions { colored: 1.0, healable_bright: 0.0, healable_dim: 0.0 });
    }

    #[test]
    fn silhouette_healable_clamped_to_lost_portion() {
        let r = silhouette_regions(40.0, 80.0, 30.0, 100.0);
        assert_eq!(r.colored, 0.4);
        assert!((r.healable_bright - 0.3).abs() < 1e-6);
        assert!((r.healable_dim - 0.3).abs() < 1e-6);
    }

    #[test]
    fn silhouette_bright_capped_at_total() {
        let r = silhouette_regions(50.0, 20.0, 50.0, 100.0);
        assert_eq!(r.colored, 0.5);
        assert_eq!(r.healable_bright, 0.2);
        assert_eq!(r.healable_dim, 0.0);
    }

    #[test]
    fn silhouette_partial_healable() {
        let r = silhouette_regions(50.0, 20.0, 8.0, 100.0);
        assert_eq!(r.colored, 0.5);
        assert!((r.healable_bright - 0.08).abs() < 1e-6);
        assert!((r.healable_dim - 0.12).abs() < 1e-6);
    }

    #[test]
    fn silhouette_zero_max_is_empty() {
        let r = silhouette_regions(0.0, 0.0, 0.0, 0.0);
        assert_eq!(r, SilhouetteRegions { colored: 0.0, healable_bright: 0.0, healable_dim: 0.0 });
    }

    #[test]
    fn team_fraction_sums_and_drains_with_dead() {
        let f = team_hp_fraction([(100.0, 100.0), (0.0, 100.0)]);
        assert_eq!(f, Some(0.5));
    }

    #[test]
    fn team_fraction_negative_current_floored() {
        let f = team_hp_fraction([(-50.0, 100.0)]);
        assert_eq!(f, Some(0.0));
    }

    #[test]
    fn team_fraction_no_max_is_none() {
        assert_eq!(team_hp_fraction([(0.0, 0.0)]), None);
        assert_eq!(team_hp_fraction(std::iter::empty()), None);
    }

    #[test]
    fn darken_halves_channels() {
        assert_eq!(darken([200, 100, 50], 0.5), [100, 50, 25]);
        assert_eq!(darken([80, 200, 120], 1.0), [80, 200, 120]);
    }
}

/// Stable display order for stats-panel ribbons so icons hold fixed positions
/// across the match and only counts change. Sorts by the `RIBBON_*` key
/// ascending, then moves torpedo protection (`RIBBON_BULGE`) to directly after
/// `RIBBON_MAIN_CALIBER`, as the replay inspector does. Without the main
/// battery ribbon the sorted order stands.
pub fn order_ribbon_keys(keys: &mut Vec<String>) {
    keys.sort();

    let Some(main_caliber) = keys.iter().position(|k| k == "RIBBON_MAIN_CALIBER") else {
        return;
    };
    let Some(bulge) = keys.iter().position(|k| k == "RIBBON_BULGE") else {
        return;
    };

    let bulge_key = keys.remove(bulge);
    // Removing an earlier BULGE shifts the main battery ribbon down by one.
    let insert_at = if bulge < main_caliber { main_caliber } else { main_caliber + 1 };
    keys.insert(insert_at, bulge_key);
}

#[cfg(test)]
mod ribbon_order_tests {
    use super::*;

    fn ordered(keys: &[&str]) -> Vec<String> {
        let mut keys: Vec<String> = keys.iter().map(|key| (*key).to_owned()).collect();
        order_ribbon_keys(&mut keys);
        keys
    }

    #[test]
    fn bulge_follows_the_main_battery_ribbon_ahead_of_its_hit_ribbons() {
        let keys = ordered(&[
            "RIBBON_TORPEDO",
            "RIBBON_MAIN_CALIBER_RICOCHET",
            "RIBBON_MAIN_CALIBER",
            "RIBBON_CITADEL",
            "RIBBON_MAIN_CALIBER_PENETRATION",
            "RIBBON_BULGE",
            "RIBBON_MAIN_CALIBER_NO_PENETRATION",
        ]);
        assert_eq!(
            keys,
            vec![
                "RIBBON_CITADEL",
                "RIBBON_MAIN_CALIBER",
                "RIBBON_BULGE",
                "RIBBON_MAIN_CALIBER_NO_PENETRATION",
                "RIBBON_MAIN_CALIBER_PENETRATION",
                "RIBBON_MAIN_CALIBER_RICOCHET",
                "RIBBON_TORPEDO",
            ]
        );
    }

    #[test]
    fn bulge_stays_sorted_without_the_main_battery_ribbon() {
        let keys = ordered(&["RIBBON_MAIN_CALIBER_PENETRATION", "RIBBON_CITADEL", "RIBBON_BULGE"]);
        assert_eq!(keys, vec!["RIBBON_BULGE", "RIBBON_CITADEL", "RIBBON_MAIN_CALIBER_PENETRATION"]);
    }

    #[test]
    fn plain_alpha_sort_without_special_keys() {
        assert_eq!(ordered(&["RIBBON_TORPEDO", "RIBBON_CITADEL"]), vec!["RIBBON_CITADEL", "RIBBON_TORPEDO"]);
    }
}

//! WCAG contrast maths. The single source of truth for the readable-text
//! floor and for repairing colours the app does not author itself.

use egui::Color32;

/// Readable-text contrast floor (WCAG 2.1 AA, normal text).
pub const CONTRAST_FLOOR: f32 = 4.5;

/// WCAG 2.1 relative luminance.
pub fn relative_luminance(color: Color32) -> f32 {
    fn channel(c: u8) -> f32 {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// WCAG contrast ratio. Order independent, always >= 1.0.
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Label colour for a filled badge: whichever of near-black or white reads
/// better on `fill`. Lets a canonical hue stay exactly as-is while the text
/// on top of it stays legible.
pub fn label_on(fill: Color32) -> Color32 {
    const NEAR_BLACK: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
    if contrast_ratio(Color32::WHITE, fill) >= contrast_ratio(NEAR_BLACK, fill) { Color32::WHITE } else { NEAR_BLACK }
}

/// Walk `color`'s lightness away from `bg` until the pair clears
/// `CONTRAST_FLOOR`, preserving hue and saturation so the colour stays
/// recognisable. For colours the app does not author: clan tags from the
/// server, collab peer colours from other users.
///
/// Both directions are tried and the smaller adjustment wins, so a colour is
/// changed as little as the floor allows. If neither direction can reach the
/// floor (only possible against a mid-luminance background), the most legible
/// candidate found is returned.
pub fn readable_on(color: Color32, bg: Color32) -> Color32 {
    if contrast_ratio(color, bg) >= CONTRAST_FLOOR {
        return color;
    }

    let (h, s, l) = color_to_hsl(color);
    const STEPS: u32 = 128;

    let mut best = color;
    let mut best_ratio = contrast_ratio(color, bg);
    let mut winner: Option<(f32, Color32)> = None;

    for lighten in [true, false] {
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let candidate_l = if lighten { l + (1.0 - l) * t } else { l * (1.0 - t) };
            let candidate = hsl_to_color(h, s, candidate_l);
            let ratio = contrast_ratio(candidate, bg);

            if ratio > best_ratio {
                best_ratio = ratio;
                best = candidate;
            }
            if ratio >= CONTRAST_FLOOR {
                let delta = (candidate_l - l).abs();
                if winner.is_none_or(|(best_delta, _)| delta < best_delta) {
                    winner = Some((delta, candidate));
                }
                break;
            }
        }
    }

    winner.map_or(best, |(_, c)| c)
}

/// Hue in degrees, saturation and lightness in 0.0..=1.0.
fn color_to_hsl(color: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(color.r()) / 255.0, f32::from(color.g()) / 255.0, f32::from(color.b()) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;

    if delta <= f32::EPSILON {
        return (0.0, 0.0, l);
    }

    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    (if h < 0.0 { h + 360.0 } else { h }, s, l)
}

fn hsl_to_color(h: f32, s: f32, l: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to_u8 = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(to_u8(r1), to_u8(g1), to_u8(b1))
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;

    const DARK_PANEL: Color32 = Color32::from_rgb(0x12, 0x12, 0x11);
    const LIGHT_PANEL: Color32 = Color32::from_rgb(0xF4, 0xF3, 0xEF);

    #[test]
    fn luminance_spans_black_to_white() {
        assert!(relative_luminance(Color32::BLACK) < 0.001);
        assert!(relative_luminance(Color32::WHITE) > 0.999);
    }

    #[test]
    fn ratio_is_order_independent_and_bounded() {
        let r = contrast_ratio(Color32::BLACK, Color32::WHITE);
        assert!((r - 21.0).abs() < 0.01, "black on white should be 21:1, got {r}");
        assert!((contrast_ratio(Color32::WHITE, Color32::BLACK) - r).abs() < 0.001);
        assert!((contrast_ratio(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
    }

    #[test]
    fn label_picks_the_higher_contrast_side() {
        // Canonical PR "Average" is a bright yellow; a dark label wins.
        let average = Color32::from_rgb(0xFF, 0xC7, 0x1F);
        let label = label_on(average);
        assert!(contrast_ratio(label, average) > contrast_ratio(Color32::WHITE, average));

        // Canonical PR "SuperUnicum" is a deep purple; a white label wins.
        let super_unicum = Color32::from_rgb(0xA0, 0x0D, 0xC5);
        assert_eq!(label_on(super_unicum), Color32::WHITE);
    }

    #[test]
    fn label_always_clears_the_floor_for_the_pr_scale() {
        for fill in [
            Color32::from_rgb(0xFF, 0x00, 0x00),
            Color32::from_rgb(0xFE, 0x79, 0x03),
            Color32::from_rgb(0xFF, 0xC7, 0x1F),
            Color32::from_rgb(0x44, 0xB3, 0x00),
            Color32::from_rgb(0x31, 0x80, 0x00),
            Color32::from_rgb(0x02, 0xC9, 0xB3),
            Color32::from_rgb(0xD0, 0x42, 0xF3),
            Color32::from_rgb(0xA0, 0x0D, 0xC5),
        ] {
            let r = contrast_ratio(label_on(fill), fill);
            assert!(r >= CONTRAST_FLOOR, "{fill:?} badge label only reached {r}");
        }
    }

    #[test]
    fn readable_on_leaves_already_readable_colours_alone() {
        let bone = Color32::from_rgb(0xE8, 0xE4, 0xD8);
        assert_eq!(readable_on(bone, DARK_PANEL), bone);
    }

    #[test]
    fn readable_on_lifts_arbitrary_colours_over_the_floor() {
        // Sweep hue and saturation the way server-supplied clan colours vary.
        for h in (0..360).step_by(15) {
            for s in [40u32, 70, 100] {
                for l in [10u32, 30, 50, 70, 90] {
                    let c = hsl_to_color(h as f32, s as f32 / 100.0, l as f32 / 100.0);
                    for bg in [DARK_PANEL, LIGHT_PANEL] {
                        let fixed = readable_on(c, bg);
                        let r = contrast_ratio(fixed, bg);
                        assert!(r >= CONTRAST_FLOOR, "h{h} s{s} l{l} on {bg:?} only reached {r}");
                    }
                }
            }
        }
    }

    #[test]
    fn readable_on_preserves_hue() {
        for h in (0..360).step_by(15) {
            let c = hsl_to_color(h as f32, 0.8, 0.5);
            for bg in [DARK_PANEL, LIGHT_PANEL] {
                let fixed = readable_on(c, bg);
                let (fixed_h, fixed_s, _) = color_to_hsl(fixed);
                if fixed_s < 0.2 {
                    continue; // hue is meaningless once saturation collapses
                }
                // 8-bit channel quantisation costs roughly a degree of hue
                // precision once lightness pushes chroma down, so the bound
                // proves the hue family is intact rather than exact equality.
                let delta = (fixed_h - h as f32).abs().min(360.0 - (fixed_h - h as f32).abs());
                assert!(delta < 5.0, "hue drifted {delta} degrees from {h}");
            }
        }
    }
}

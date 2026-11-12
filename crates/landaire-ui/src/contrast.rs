use crate::color::Hsl;
use crate::color::Rgb;

/// A WCAG 2.1 contrast ratio, from 1 for identical colors to 21 for black on white.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ContrastRatio(f32);

impl ContrastRatio {
    /// The WCAG AA floor for body text.
    pub const READABLE: Self = Self(4.5);

    pub fn between(first: Rgb, second: Rgb) -> Self {
        let first = first.relative_luminance();
        let second = second.relative_luminance();

        Self((first.max(second) + 0.05) / (first.min(second) + 0.05))
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// `color` moved in lightness until it reads at [`ContrastRatio::READABLE`] on every one of
/// `grounds`, for colors the application does not choose itself.
///
/// Hue and saturation are kept, and the smaller of the two lightness moves that clears the floor
/// wins. Grounds on either side of a relative luminance of about 0.18 can defeat both black and
/// white; then the result is whichever of the two reads better on its worse ground.
pub fn readable_on(color: Rgb, grounds: &[Rgb]) -> Rgb {
    const STEPS: u16 = 128;

    let worst = |candidate: Rgb| {
        grounds
            .iter()
            .map(|ground| ContrastRatio::between(candidate, *ground))
            .fold(ContrastRatio(f32::MAX), |worst, ratio| if ratio < worst { ratio } else { worst })
    };
    if worst(color) >= ContrastRatio::READABLE {
        return color;
    }

    let hsl = color.to_hsl();
    let mut winner: Option<(f32, Rgb)> = None;
    for lighten in [true, false] {
        for step in 1..=STEPS {
            let travel = f32::from(step) / f32::from(STEPS);
            let lightness =
                if lighten { hsl.lightness + (1.0 - hsl.lightness) * travel } else { hsl.lightness * (1.0 - travel) };
            let candidate = Rgb::from_hsl(Hsl { lightness, ..hsl });
            if worst(candidate) >= ContrastRatio::READABLE {
                let moved = (lightness - hsl.lightness).abs();
                if winner.is_none_or(|(least, _)| moved < least) {
                    winner = Some((moved, candidate));
                }
                break;
            }
        }
    }

    match winner {
        Some((_, candidate)) => candidate,
        None if worst(Rgb::WHITE) >= worst(Rgb::BLACK) => Rgb::WHITE,
        None => Rgb::BLACK,
    }
}

/// Near-black or white, whichever has the higher contrast on `ground`.
pub fn text_on(ground: Rgb) -> Rgb {
    const NEAR_BLACK: Rgb = Rgb::new(0x1a1a1a);

    if ContrastRatio::between(NEAR_BLACK, ground) >= ContrastRatio::between(Rgb::WHITE, ground) {
        NEAR_BLACK
    } else {
        Rgb::WHITE
    }
}

/// Black or white for text on a solid `accent` fill.
///
/// The 0.179 cut is where black and white have equal contrast on a ground.
pub fn accent_foreground(accent: Rgb) -> Rgb {
    if accent.relative_luminance() > 0.179 { Rgb::BLACK } else { Rgb::WHITE }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIGHT_ROWS: [Rgb; 2] = [Rgb::new(0xf4f3ef), Rgb::new(0xf0efeb)];
    const DARK_ROWS: [Rgb; 2] = [Rgb::new(0x181816), Rgb::new(0x1c1c1a)];

    #[test]
    fn a_pale_color_is_darkened_on_light_rows_only_as_far_as_the_floor() {
        let pale_green = Rgb::new(0xa0e8a0);

        let lifted = readable_on(pale_green, &LIGHT_ROWS);

        for ground in LIGHT_ROWS {
            assert!(ContrastRatio::between(lifted, ground) >= ContrastRatio::READABLE);
        }
        assert!((lifted.to_hsl().hue - pale_green.to_hsl().hue).abs() < 0.01, "the hue is kept");
        assert!(
            ContrastRatio::between(lifted, LIGHT_ROWS[0]).value() < ContrastRatio::READABLE.value() + 0.5,
            "lifted no further than needed"
        );
    }

    #[test]
    fn a_color_that_already_reads_is_left_alone() {
        let pale_green = Rgb::new(0xa0e8a0);

        assert_eq!(readable_on(pale_green, &DARK_ROWS), pale_green);
    }

    #[test]
    fn grounds_no_lightness_can_satisfy_fall_back_to_the_better_extreme() {
        // White fails the lighter ground and black the darker one; black misses by less.
        let grounds = [Rgb::new(0x6e6e6e), Rgb::new(0x7e7e7e)];

        assert_eq!(readable_on(Rgb::new(0x767676), &grounds), Rgb::BLACK);
    }
}

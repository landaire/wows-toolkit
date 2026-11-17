//! How many output pixels a frame spends on each pixel of its canvas layout.

/// Output pixels per canvas pixel, along each axis.
///
/// The canvas layout ([`crate::MINIMAP_SIZE`], [`crate::HUD_HEIGHT`] and the
/// gutters) is the coordinate system every command and region is in. A frame
/// shown larger than that layout is drawn at this many pixels per canvas
/// pixel, so it is as sharp as the screen it is shown on.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct RenderScale(f32);

impl RenderScale {
    /// The canvas layout's own size, which video export and previews draw at.
    pub const ONE: Self = Self(1.0);
    /// The most a frame is enlarged: past this a frame costs more to draw than a screen shows.
    pub const MAX: Self = Self(3.0);
    /// The least a frame is reduced: past this text and strokes are below a pixel.
    pub const MIN: Self = Self(0.25);

    /// `factor` held to [`Self::MIN`] through [`Self::MAX`]. `None` for a factor that is not a
    /// finite positive number, which is no size at all.
    pub fn new(factor: f32) -> Option<Self> {
        if !factor.is_finite() || factor <= 0.0 {
            return None;
        }

        Some(Self(factor.clamp(Self::MIN.0, Self::MAX.0)))
    }

    pub fn factor(self) -> f32 {
        self.0
    }

    /// How many output pixels `length` canvas pixels cover, rounded to whole pixels and never
    /// less than one.
    pub fn pixels(self, length: u32) -> u32 {
        ((length as f32 * self.0).round() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_factor_is_held_to_the_drawable_range() {
        assert_eq!(RenderScale::new(9.0), Some(RenderScale::MAX));
        assert_eq!(RenderScale::new(0.01), Some(RenderScale::MIN));
        assert_eq!(RenderScale::new(0.0), None);
        assert_eq!(RenderScale::new(f32::NAN), None);
    }
}

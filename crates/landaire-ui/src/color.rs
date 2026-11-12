use std::fmt;

use crate::error::HexColorError;

/// An opaque sRGB color, packed as `0xRRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(u32);

impl Rgb {
    pub const BLACK: Self = Self(0x000000);
    pub const WHITE: Self = Self(0xffffff);

    /// # Panics
    ///
    /// Panics when `packed` has bits above the low 24, at compile time in a constant.
    pub const fn new(packed: u32) -> Self {
        assert!(packed <= 0xffffff, "an Rgb is packed as 0xRRGGBB");

        Self(packed)
    }

    pub const fn from_channels(red: u8, green: u8, blue: u8) -> Self {
        Self(((red as u32) << 16) | ((green as u32) << 8) | blue as u32)
    }

    /// Reads hex text as [`Display`](fmt::Display) writes it: `#rrggbb`, or the short `#rgb`, with
    /// or without the `#`, in either case, with surrounding space ignored.
    pub fn parse(text: &str) -> Result<Self, HexColorError> {
        let text = text.trim();
        let digits = text.strip_prefix('#').unwrap_or(text);
        let mut nibbles = Vec::with_capacity(6);
        for (position, character) in digits.chars().enumerate() {
            let Some(nibble) = character.to_digit(16) else {
                return Err(HexColorError::NotHex { position });
            };
            nibbles.push(nibble);
        }

        let nibbles = match nibbles.len() {
            6 => nibbles,
            3 => nibbles.iter().flat_map(|&nibble| [nibble, nibble]).collect(),
            length => return Err(HexColorError::Length { length }),
        };

        Ok(Self(nibbles.iter().fold(0, |packed, nibble| (packed << 4) | nibble)))
    }

    pub const fn packed(self) -> u32 {
        self.0
    }

    pub const fn red(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub const fn green(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub const fn blue(self) -> u8 {
        self.0 as u8
    }

    /// WCAG 2.1 relative luminance, from 0 for black to 1 for white.
    pub fn relative_luminance(self) -> f32 {
        let linear = |channel: u8| {
            let channel = f32::from(channel) / 255.0;
            if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
        };

        0.2126 * linear(self.red()) + 0.7152 * linear(self.green()) + 0.0722 * linear(self.blue())
    }

    /// This color at `opacity`, as a CSS color value.
    pub fn with_opacity(self, opacity: Opacity) -> String {
        format!("rgb({} {} {} / {})", self.red(), self.green(), self.blue(), opacity.0)
    }

    pub(crate) fn to_hsl(self) -> Hsl {
        let red = f32::from(self.red()) / 255.0;
        let green = f32::from(self.green()) / 255.0;
        let blue = f32::from(self.blue()) / 255.0;
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        let lightness = (max + min) / 2.0;
        let chroma = max - min;
        if chroma == 0.0 {
            return Hsl { hue: 0.0, saturation: 0.0, lightness };
        }

        let saturation = chroma / (1.0 - (2.0 * lightness - 1.0).abs());
        let sector = if max == red {
            ((green - blue) / chroma).rem_euclid(6.0)
        } else if max == green {
            (blue - red) / chroma + 2.0
        } else {
            (red - green) / chroma + 4.0
        };

        Hsl { hue: sector / 6.0, saturation, lightness }
    }

    pub(crate) fn from_hsl(hsl: Hsl) -> Self {
        let chroma = (1.0 - (2.0 * hsl.lightness - 1.0).abs()) * hsl.saturation;
        let sector = hsl.hue.rem_euclid(1.0) * 6.0;
        let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (red, green, blue) = match sector as u8 {
            0 => (chroma, second, 0.0),
            1 => (second, chroma, 0.0),
            2 => (0.0, chroma, second),
            3 => (0.0, second, chroma),
            4 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let offset = hsl.lightness - chroma / 2.0;
        let channel = |value: f32| ((value + offset).clamp(0.0, 1.0) * 255.0).round() as u8;

        Self::from_channels(channel(red), channel(green), channel(blue))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{:06x}", self.0)
    }
}

/// How opaque a color is drawn, from 0 for transparent to 1 for opaque.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Opacity(pub(crate) f32);

impl Opacity {
    pub const OPAQUE: Self = Self(1.0);

    /// `None` when `value` is outside 0 to 1 or is not a number.
    pub fn new(value: f32) -> Option<Self> {
        (0.0..=1.0).contains(&value).then_some(Self(value))
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// Hue, saturation and lightness, each from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hsl {
    pub(crate) hue: f32,
    pub(crate) saturation: f32,
    pub(crate) lightness: f32,
}

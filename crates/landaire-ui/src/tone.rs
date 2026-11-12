use crate::color::Rgb;
use crate::palette::Appearance;

/// Colors that say what something means rather than where it sits, shared by every palette of
/// one appearance.
///
/// The two sets are not each other's inverse: a light panel caps how light a tone may be, so the
/// light set is tuned apart along lightness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tones {
    pub error: Rgb,
    pub warning: Rgb,
    pub success: Rgb,
    /// A value worth noticing that is not a warning.
    pub info: Rgb,
    /// Below the dim text tone: punctuation and hints that are there to be found, not read.
    pub text_faint: Rgb,
}

impl Tones {
    pub const fn for_appearance(appearance: Appearance) -> &'static Self {
        match appearance {
            Appearance::Light => &LIGHT,
            Appearance::Dark => &DARK,
        }
    }
}

const DARK: Tones = Tones {
    error: Rgb::new(0xe8737b),
    warning: Rgb::new(0xe8a54a),
    success: Rgb::new(0x6fd98a),
    info: Rgb::new(0xe5c158),
    text_faint: Rgb::new(0x5c5c52),
};

const LIGHT: Tones = Tones {
    error: Rgb::new(0xae2230),
    warning: Rgb::new(0x8a4b00),
    success: Rgb::new(0x106c34),
    info: Rgb::new(0x775800),
    text_faint: Rgb::new(0x9b9890),
};

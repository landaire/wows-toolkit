use std::fmt;
use std::num::NonZeroU16;
use std::num::NonZeroU32;

use crate::error::InvalidValue;

/// A setting as read from a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Field<T> {
    Known(T),
    /// The file does not hold this key.
    Absent,
    /// A value this version does not recognize; shown, never overwritten.
    Unrecognized(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowMode {
    Windowed,
    Borderless,
    Fullscreen,
}

/// The game's index of the monitor it opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MonitorIndex(u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixelCount(NonZeroU16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Resolution {
    pub width: PixelCount,
    pub height: PixelCount,
}

/// `None` follows the display's own rate (`refresh="0"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RefreshRate(Option<NonZeroU16>);

/// `None` is unlimited (`0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameRateLimit(Option<NonZeroU16>);

/// A linear volume in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume(f32);

/// The interface scale factor; `1.25` is 125%.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiScale(f32);

/// How many replays the game keeps before it deletes the oldest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplayCount(NonZeroU32);

/// A client or voice language code as the game writes it, for example `EN`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LanguageCode(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    Off,
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
    Ultra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraphicsPreset {
    /// `USER`: per-item qualities chosen in the game.
    Custom,
    /// A named preset. [INFERENCE] Named presets reuse the quality names;
    /// only `USER` has been observed.
    Level(Quality),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MsaaMode {
    Off,
    Samples2,
    Samples4,
    Samples8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureFiltering {
    Bilinear,
    Trilinear,
    Anisotropic2x,
    Anisotropic4x,
    Anisotropic8x,
    Anisotropic16x,
}

/// `sound@mixingPreset`. Only the custom mix lets channel volumes stand on
/// their own; other presets appear as `Field::Unrecognized`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixingPreset {
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VolumeChannel {
    Interface,
    Engines,
    Voice,
    World,
    Gunfire,
    Damage,
}

/// A value the game stores as text.
pub(crate) trait PrefsValue: Sized {
    fn parse_text(text: &str) -> Option<Self>;
    fn to_text(&self) -> String;
}

impl<T> Field<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Field::Known(value) => Some(value),
            Field::Absent | Field::Unrecognized(_) => None,
        }
    }

    pub(crate) fn from_text(text: Option<&str>) -> Field<T>
    where
        T: PrefsValue,
    {
        let Some(text) = text else {
            return Field::Absent;
        };

        match T::parse_text(text) {
            Some(value) => Field::Known(value),
            None => Field::Unrecognized(text.to_owned()),
        }
    }
}

impl PrefsValue for WindowMode {
    fn parse_text(text: &str) -> Option<WindowMode> {
        match text {
            "WINDOWED" => Some(WindowMode::Windowed),
            "BORDERLESS" => Some(WindowMode::Borderless),
            "FULLSCREEN" => Some(WindowMode::Fullscreen),
            _ => None,
        }
    }

    fn to_text(&self) -> String {
        let text = match self {
            WindowMode::Windowed => "WINDOWED",
            WindowMode::Borderless => "BORDERLESS",
            WindowMode::Fullscreen => "FULLSCREEN",
        };

        text.to_owned()
    }
}

impl MonitorIndex {
    pub fn new(index: u8) -> MonitorIndex {
        MonitorIndex(index)
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl PrefsValue for MonitorIndex {
    fn parse_text(text: &str) -> Option<MonitorIndex> {
        text.parse().ok().map(MonitorIndex)
    }

    fn to_text(&self) -> String {
        self.0.to_string()
    }
}

impl PixelCount {
    pub fn get(self) -> u16 {
        self.0.get()
    }
}

impl Resolution {
    pub const MIN_WIDTH: u16 = 640;
    pub const MIN_HEIGHT: u16 = 480;
    pub const MAX_SIDE: u16 = 16384;

    pub fn new(width: u16, height: u16) -> Result<Resolution, InvalidValue> {
        let side = |value: u16, min: u16| {
            (min..=Resolution::MAX_SIDE).contains(&value).then(|| NonZeroU16::new(value)).flatten().map(PixelCount)
        };
        let (Some(width_px), Some(height_px)) =
            (side(width, Resolution::MIN_WIDTH), side(height, Resolution::MIN_HEIGHT))
        else {
            return Err(InvalidValue::Resolution { width, height });
        };

        Ok(Resolution { width: width_px, height: height_px })
    }

    /// `width / height` as the game writes it beside the resolution.
    pub(crate) fn aspect_ratio_text(self) -> String {
        format!("{:.4}", f64::from(self.width.get()) / f64::from(self.height.get()))
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {}", self.width.get(), self.height.get())
    }
}

impl RefreshRate {
    pub const MIN_HZ: u16 = 24;
    pub const MAX_HZ: u16 = 1000;

    pub fn display() -> RefreshRate {
        RefreshRate(None)
    }

    pub fn hz(rate: u16) -> Result<RefreshRate, InvalidValue> {
        if !(RefreshRate::MIN_HZ..=RefreshRate::MAX_HZ).contains(&rate) {
            return Err(InvalidValue::RefreshRate { hz: rate });
        }

        Ok(RefreshRate(NonZeroU16::new(rate)))
    }

    pub fn get(self) -> Option<NonZeroU16> {
        self.0
    }
}

impl PrefsValue for RefreshRate {
    fn parse_text(text: &str) -> Option<RefreshRate> {
        match text.parse::<u16>().ok()? {
            0 => Some(RefreshRate::display()),
            rate => RefreshRate::hz(rate).ok(),
        }
    }

    fn to_text(&self) -> String {
        self.0.map_or(0, NonZeroU16::get).to_string()
    }
}

impl FrameRateLimit {
    pub const MIN_FPS: u16 = 10;
    pub const MAX_FPS: u16 = 1000;

    pub fn unlimited() -> FrameRateLimit {
        FrameRateLimit(None)
    }

    pub fn limited(fps: u16) -> Result<FrameRateLimit, InvalidValue> {
        if !(FrameRateLimit::MIN_FPS..=FrameRateLimit::MAX_FPS).contains(&fps) {
            return Err(InvalidValue::FrameRate { fps });
        }

        Ok(FrameRateLimit(NonZeroU16::new(fps)))
    }

    pub fn get(self) -> Option<NonZeroU16> {
        self.0
    }
}

impl PrefsValue for FrameRateLimit {
    fn parse_text(text: &str) -> Option<FrameRateLimit> {
        match text.parse::<u16>().ok()? {
            0 => Some(FrameRateLimit::unlimited()),
            fps => FrameRateLimit::limited(fps).ok(),
        }
    }

    fn to_text(&self) -> String {
        self.0.map_or(0, NonZeroU16::get).to_string()
    }
}

impl Volume {
    pub fn new(volume: f32) -> Result<Volume, InvalidValue> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(InvalidValue::Volume { volume });
        }

        Ok(Volume(volume))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl PrefsValue for Volume {
    fn parse_text(text: &str) -> Option<Volume> {
        Volume::new(text.parse().ok()?).ok()
    }

    fn to_text(&self) -> String {
        format!("{:.4}", self.0)
    }
}

impl UiScale {
    pub const MIN: f32 = 0.5;
    pub const MAX: f32 = 3.0;

    pub fn new(scale: f32) -> Result<UiScale, InvalidValue> {
        if !(UiScale::MIN..=UiScale::MAX).contains(&scale) {
            return Err(InvalidValue::UiScale { scale });
        }

        Ok(UiScale(scale))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl PrefsValue for UiScale {
    fn parse_text(text: &str) -> Option<UiScale> {
        UiScale::new(text.parse().ok()?).ok()
    }

    fn to_text(&self) -> String {
        format!("{:.4}", self.0)
    }
}

impl ReplayCount {
    pub fn new(count: NonZeroU32) -> ReplayCount {
        ReplayCount(count)
    }

    pub fn get(self) -> NonZeroU32 {
        self.0
    }
}

impl PrefsValue for ReplayCount {
    // `0` is not accepted: what the game does with it is unconfirmed.
    fn parse_text(text: &str) -> Option<ReplayCount> {
        text.parse().ok().map(ReplayCount)
    }

    fn to_text(&self) -> String {
        self.0.to_string()
    }
}

impl LanguageCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PrefsValue for LanguageCode {
    fn parse_text(text: &str) -> Option<LanguageCode> {
        (!text.is_empty()).then(|| LanguageCode(text.to_owned()))
    }

    fn to_text(&self) -> String {
        self.0.clone()
    }
}

impl PrefsValue for bool {
    // engine_config.xml writes `False` in places; the game's reader is not
    // case sensitive there.
    fn parse_text(text: &str) -> Option<bool> {
        if text.eq_ignore_ascii_case("true") {
            Some(true)
        } else if text.eq_ignore_ascii_case("false") {
            Some(false)
        } else {
            None
        }
    }

    fn to_text(&self) -> String {
        self.to_string()
    }
}

impl PrefsValue for String {
    fn parse_text(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    fn to_text(&self) -> String {
        self.clone()
    }
}

impl PrefsValue for Quality {
    fn parse_text(text: &str) -> Option<Quality> {
        match text {
            "OFF" => Some(Quality::Off),
            "VERY_LOW" => Some(Quality::VeryLow),
            "LOW" => Some(Quality::Low),
            "MEDIUM" => Some(Quality::Medium),
            "HIGH" => Some(Quality::High),
            "VERY_HIGH" => Some(Quality::VeryHigh),
            "ULTRA" => Some(Quality::Ultra),
            _ => None,
        }
    }

    fn to_text(&self) -> String {
        let text = match self {
            Quality::Off => "OFF",
            Quality::VeryLow => "VERY_LOW",
            Quality::Low => "LOW",
            Quality::Medium => "MEDIUM",
            Quality::High => "HIGH",
            Quality::VeryHigh => "VERY_HIGH",
            Quality::Ultra => "ULTRA",
        };

        text.to_owned()
    }
}

impl PrefsValue for GraphicsPreset {
    fn parse_text(text: &str) -> Option<GraphicsPreset> {
        match text {
            "USER" => Some(GraphicsPreset::Custom),
            _ => Quality::parse_text(text).map(GraphicsPreset::Level),
        }
    }

    fn to_text(&self) -> String {
        match self {
            GraphicsPreset::Custom => "USER".to_owned(),
            GraphicsPreset::Level(quality) => quality.to_text(),
        }
    }
}

impl PrefsValue for MsaaMode {
    fn parse_text(text: &str) -> Option<MsaaMode> {
        match text {
            "OFF" => Some(MsaaMode::Off),
            "SAMPLES_2" => Some(MsaaMode::Samples2),
            "SAMPLES_4" => Some(MsaaMode::Samples4),
            "SAMPLES_8" => Some(MsaaMode::Samples8),
            _ => None,
        }
    }

    fn to_text(&self) -> String {
        let text = match self {
            MsaaMode::Off => "OFF",
            MsaaMode::Samples2 => "SAMPLES_2",
            MsaaMode::Samples4 => "SAMPLES_4",
            MsaaMode::Samples8 => "SAMPLES_8",
        };

        text.to_owned()
    }
}

impl PrefsValue for TextureFiltering {
    fn parse_text(text: &str) -> Option<TextureFiltering> {
        match text {
            "BILINEAR" => Some(TextureFiltering::Bilinear),
            "TRILINEAR" => Some(TextureFiltering::Trilinear),
            "ANISOTROPIC_2X" => Some(TextureFiltering::Anisotropic2x),
            "ANISOTROPIC_4X" => Some(TextureFiltering::Anisotropic4x),
            "ANISOTROPIC_8X" => Some(TextureFiltering::Anisotropic8x),
            "ANISOTROPIC_16X" => Some(TextureFiltering::Anisotropic16x),
            _ => None,
        }
    }

    fn to_text(&self) -> String {
        let text = match self {
            TextureFiltering::Bilinear => "BILINEAR",
            TextureFiltering::Trilinear => "TRILINEAR",
            TextureFiltering::Anisotropic2x => "ANISOTROPIC_2X",
            TextureFiltering::Anisotropic4x => "ANISOTROPIC_4X",
            TextureFiltering::Anisotropic8x => "ANISOTROPIC_8X",
            TextureFiltering::Anisotropic16x => "ANISOTROPIC_16X",
        };

        text.to_owned()
    }
}

impl PrefsValue for MixingPreset {
    fn parse_text(text: &str) -> Option<MixingPreset> {
        (text == "CUSTOM").then_some(MixingPreset::Custom)
    }

    fn to_text(&self) -> String {
        "CUSTOM".to_owned()
    }
}

impl VolumeChannel {
    pub const ALL: [VolumeChannel; 6] = [
        VolumeChannel::Interface,
        VolumeChannel::Engines,
        VolumeChannel::Voice,
        VolumeChannel::World,
        VolumeChannel::Gunfire,
        VolumeChannel::Damage,
    ];
}

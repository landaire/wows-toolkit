//! Themed surfaces and test tooling for zgui applications.
//!
//! [`ThemeRoot`] applies a [`ThemeChoice`] to a window as design tokens: the `--zui-*` set that
//! zgui-ui components read, and the `--lui-*` set this crate's surfaces read.

mod color;
mod contrast;
mod error;
mod motion;
mod palette;
mod root;
mod scheme;
mod tokens;
mod tone;

pub use crate::color::Opacity;
pub use crate::color::Rgb;
pub use crate::contrast::ContrastRatio;
pub use crate::contrast::accent_foreground;
pub use crate::contrast::readable_on;
pub use crate::contrast::text_on;
pub use crate::error::TierOrderError;
pub use crate::motion::CubicBezier;
pub use crate::motion::EASE_PROPERTY;
pub use crate::motion::MotionPreference;
pub use crate::motion::Movement;
pub use crate::motion::REDUCED_MOTION_ATTRIBUTE;
pub use crate::palette::Appearance;
pub use crate::palette::Palette;
pub use crate::palette::Tier;
pub use crate::root::ThemeRoot;
pub use crate::root::ThemeRootProps;
pub use crate::scheme::Scheme;
pub use crate::scheme::ThemeChoice;
pub use crate::tokens::BASE_FONT_SIZE;
pub use crate::tokens::MONO_FAMILY;
pub use crate::tokens::RADIUS;
pub use crate::tokens::RADIUS_LARGE;
pub use crate::tokens::SANS_FAMILY;
pub use crate::tokens::lui_declarations;
pub use crate::tokens::lui_sheet;
pub use crate::tokens::tier_property;
pub use crate::tokens::zui_theme;
pub use crate::tone::Tones;

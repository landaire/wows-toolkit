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
mod surface;
#[cfg(feature = "test-support")]
pub mod test_support;
mod tokens;
mod tone;

pub use crate::color::Opacity;
pub use crate::color::Rgb;
pub use crate::contrast::ContrastRatio;
pub use crate::contrast::accent_foreground;
pub use crate::contrast::readable_on;
pub use crate::contrast::text_on;
#[cfg(feature = "test-support")]
pub use crate::error::HeadlessError;
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
pub use crate::surface::CARD_CLASS;
pub use crate::surface::CardSurface;
pub use crate::surface::CardSurfaceProps;
pub use crate::surface::CollapseButton;
pub use crate::surface::CollapseButtonProps;
pub use crate::surface::ExpandRail;
pub use crate::surface::ExpandRailProps;
pub use crate::surface::IndentGuides;
pub use crate::surface::IndentGuidesProps;
pub use crate::surface::IndentWidth;
pub use crate::surface::OVERLAY_CLASS;
pub use crate::surface::OverlaySurface;
pub use crate::surface::OverlaySurfaceProps;
pub use crate::surface::PanelSide;
pub use crate::surface::RowIndex;
pub use crate::surface::STRIPE_CLASS;
pub use crate::surface::STRIPED_CLASS;
pub use crate::surface::StripedRows;
pub use crate::surface::StripedRowsProps;
pub use crate::surface::TABULAR_CLASS;
pub use crate::surface::TreeDepth;
pub use crate::surface::WORKSPACE_CLASS;
pub use crate::surface::WORKSPACE_INSET_CLASS;
pub use crate::surface::WorkspaceInset;
pub use crate::surface::WorkspaceInsetProps;
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

#![allow(clippy::too_many_arguments)]

pub mod advantage;
#[cfg(feature = "rendering")]
pub mod assets;
pub mod codec;
pub mod config;
pub mod draw_command;
#[cfg(feature = "rendering")]
pub mod drawing;
#[cfg(feature = "rendering")]
pub mod encoder;
pub mod error;
pub mod map_data;
#[cfg(feature = "rendering")]
pub mod renderer;
#[cfg(feature = "rendering")]
pub mod video;

/// Minimap image size in pixels (square). Multiple of 16 for H.264 macroblock alignment.
pub const MINIMAP_SIZE: u32 = 768;
/// Top margin for HUD elements (score bar, timer, kill feed).
pub const HUD_HEIGHT: u32 = 32;
/// Total canvas height: minimap + HUD.
pub const CANVAS_HEIGHT: u32 = MINIMAP_SIZE + HUD_HEIGHT;
/// Width of the stats side panel when enabled.
pub const STATS_PANEL_WIDTH: u32 = 280;
/// Width of each team roster panel (left/right of the minimap) when enabled.
pub const TEAM_ROSTER_WIDTH: u32 = 280;
/// Halo thickness (in icon-pixel space) for the detected-teammate outline
/// drawn around ship icons. Both renderers pad the outline texture by this
/// amount on each side so the halo wraps fully around the icon's silhouette
/// instead of being clipped to the icon's bounding box.
pub const SHIP_ICON_OUTLINE_THICKNESS: u32 = 2;

#[cfg(feature = "rendering")]
pub use assets::GameFonts;
pub use codec::EncoderKind;
pub use codec::VideoCodec;
pub use config::RenderOptions;
pub use draw_command::ActivityFeedEntry;
pub use draw_command::DrawCommand;
pub use draw_command::FontHint;
pub use draw_command::RenderTarget;
pub use draw_command::RibbonCount;
pub use draw_command::ShipConfigFilter;
pub use draw_command::ShipConfigVisibility;
pub use draw_command::ShipVisibility;
#[cfg(feature = "rendering")]
pub use drawing::ImageTarget;
#[cfg(feature = "rendering")]
pub use drawing::ShipIcon;
#[cfg(feature = "rendering")]
pub use encoder::EncoderStatus;
#[cfg(feature = "rendering")]
pub use encoder::check_encoder;
pub use map_data::MapInfo;
pub use map_data::MinimapPos;
#[cfg(feature = "rendering")]
pub use renderer::MinimapRenderer;
#[cfg(feature = "rendering")]
pub use video::DumpMode;
#[cfg(feature = "rendering")]
pub use video::RenderProgress;
#[cfg(feature = "rendering")]
pub use video::RenderStage;
#[cfg(feature = "rendering")]
pub use video::VideoEncoder;

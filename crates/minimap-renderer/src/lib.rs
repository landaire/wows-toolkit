#![allow(clippy::too_many_arguments)]

pub mod advantage;
pub mod assets;
pub mod config;
pub mod draw_command;
pub mod drawing;
pub mod error;
pub mod map_data;
pub mod renderer;
pub mod video;

/// Minimap image size in pixels (square). Multiple of 16 for H.264 macroblock alignment.
pub const MINIMAP_SIZE: u32 = 768;
/// Top margin for HUD elements (score bar, timer, kill feed).
pub const HUD_HEIGHT: u32 = 32;
/// Total canvas height: minimap + HUD.
pub const CANVAS_HEIGHT: u32 = MINIMAP_SIZE + HUD_HEIGHT;

pub use assets::GameFonts;
pub use draw_command::{DrawCommand, FontHint, RenderTarget, ShipConfigFilter, ShipConfigVisibility, ShipVisibility};
pub use drawing::{ImageTarget, ShipIcon};
pub use map_data::{MapInfo, MinimapPos};
pub use renderer::MinimapRenderer;
pub use video::{DumpMode, EncoderStatus, RenderProgress, RenderStage, VideoEncoder, check_encoder};

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
pub use draw_command::DrawCommand;
pub use draw_command::FontHint;
pub use draw_command::RenderTarget;
pub use draw_command::ShipConfigFilter;
pub use draw_command::ShipConfigVisibility;
pub use draw_command::ShipVisibility;
pub use drawing::ImageTarget;
pub use drawing::ShipIcon;
pub use map_data::MapInfo;
pub use map_data::MinimapPos;
pub use renderer::MinimapRenderer;
pub use video::DumpMode;
pub use video::EncoderStatus;
pub use video::RenderProgress;
pub use video::RenderStage;
pub use video::VideoEncoder;
pub use video::check_encoder;

//! Rasterising baked draw commands to an image.
//!
//! The hover preview shows a replay's minimap as a small animation. The egui
//! app draws the same commands through egui's painter; a front end that has
//! no painter of its own can render them here instead and display the
//! resulting image, which is also what the video export does.
//!
//! Assets are loaded once per renderer rather than per frame: a preview plays
//! dozens of frames off one build's icons, and reloading them from the VFS
//! each time would dominate the cost of drawing.

use std::collections::HashMap;

use image::RgbImage;
use wowsunpack::vfs::VfsPath;

use crate::assets;
use crate::draw_command::DrawCommand;
use crate::draw_command::RenderTarget;
use crate::drawing::ImageTarget;
use wowsunpack::data::Version;

/// Why a preview could not be rendered.
///
/// Written out rather than derived: this crate carries no `thiserror`, and
/// `error.rs` states its errors the same way. Callers match the variant; the
/// message is for logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewRenderError {
    NoMapArt { map: String },
}

impl std::fmt::Display for PreviewRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMapArt { map } => write!(f, "the build ships no map art for {map:?}"),
        }
    }
}

impl std::error::Error for PreviewRenderError {}

/// A renderer bound to one build's assets and one map.
pub struct PreviewRenderer {
    target: ImageTarget,
}

impl PreviewRenderer {
    /// Loads the assets a preview of `map_name` needs from `vfs`.
    ///
    /// `version` selects the asset layout, which moved between builds; the
    /// loaders fall back on their own when it is absent.
    pub fn new(vfs: &VfsPath, version: Option<&Version>, map_name: &str) -> Result<Self, PreviewRenderError> {
        let map_image = assets::load_map_image(map_name, vfs)
            .ok_or_else(|| PreviewRenderError::NoMapArt { map: map_name.to_string() })?;

        let target = ImageTarget::new(
            Some(map_image),
            assets::load_game_fonts(vfs),
            assets::load_ship_icons(vfs, version),
            assets::load_plane_icons(vfs, version),
            assets::load_building_icons(vfs, version),
            assets::load_consumable_icons(vfs, version),
            // The preview draws no ribbons: they belong to the stats panel,
            // which a preview does not show.
            HashMap::new(),
            HashMap::new(),
            assets::load_death_cause_icons(vfs, 0, version),
            assets::load_powerup_icons(vfs, 0, version),
        );

        Ok(Self { target })
    }

    /// The pixel size of every frame this renderer produces.
    pub fn canvas_size(&self) -> (u32, u32) {
        self.target.canvas_size()
    }

    /// One frame of `commands`, drawn over the map.
    pub fn render(&mut self, commands: &[DrawCommand]) -> RgbImage {
        self.target.begin_frame();
        for command in commands {
            self.target.draw(command);
        }
        self.target.end_frame();
        self.target.frame()
    }
}

//! The clip's map art and icons as egui textures.

use std::collections::HashMap;

use egui::ColorImage;
use egui::TextureHandle;
use egui::TextureOptions;
use wows_minimap_renderer::track_file::RgbaIcon;
use wows_minimap_renderer::track_file::TrackIcons;
use wt_collab_egui::draw_commands::DrawCommandTextures;

use crate::error::LoadError;

pub struct Textures {
    map: TextureHandle,
    ships: HashMap<String, TextureHandle>,
    planes: HashMap<String, TextureHandle>,
    buildings: HashMap<String, TextureHandle>,
    consumables: HashMap<String, TextureHandle>,
    death_causes: HashMap<String, TextureHandle>,
    powerups: HashMap<String, TextureHandle>,
}

impl Textures {
    pub fn upload(ctx: &egui::Context, map_png: &[u8], icons: TrackIcons) -> Result<Self, LoadError> {
        let map = ctx.load_texture("map", decode_png(map_png)?, TextureOptions::LINEAR);

        let upload = |set: Vec<(String, RgbaIcon)>, prefix: &str| -> HashMap<String, TextureHandle> {
            set.into_iter()
                .map(|(key, icon)| {
                    let image =
                        ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
                    let handle = ctx.load_texture(format!("{prefix}_{key}"), image, TextureOptions::LINEAR);
                    (key, handle)
                })
                .collect()
        };

        Ok(Self {
            map,
            ships: upload(icons.ships, "ship"),
            planes: upload(icons.planes, "plane"),
            buildings: upload(icons.buildings, "building"),
            consumables: upload(icons.consumables, "consumable"),
            death_causes: upload(icons.death_causes, "death"),
            powerups: upload(icons.powerups, "powerup"),
        })
    }

    pub fn map(&self) -> egui::TextureId {
        self.map.id()
    }

    /// The icons as the shared drawing code takes them. The desktop-only
    /// outlines, ribbons and silhouette belong to layers the demo does not
    /// draw.
    pub fn for_drawing(&self) -> DrawCommandTextures<'_> {
        DrawCommandTextures {
            ship_icons: &self.ships,
            ship_icon_outlines: None,
            plane_icons: &self.planes,
            building_icons: Some(&self.buildings),
            consumable_icons: Some(&self.consumables),
            ribbon_icons: None,
            subribbon_icons: None,
            death_cause_icons: Some(&self.death_causes),
            powerup_icons: Some(&self.powerups),
            silhouette_texture: None,
        }
    }
}

fn decode_png(bytes: &[u8]) -> Result<ColorImage, LoadError> {
    // Far above any minimap art the game ships, and small enough to allocate.
    const MAX_PNG_BYTES: usize = 16 << 20;
    const MAX_SIDE: u32 = 4096;

    if bytes.len() > MAX_PNG_BYTES {
        return Err(LoadError::MapTooLarge);
    }

    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info()?;
    let header = reader.info();
    if header.width > MAX_SIDE || header.height > MAX_SIDE {
        return Err(LoadError::MapTooLarge);
    }

    let mut buffer = vec![0u8; reader.output_buffer_size().ok_or(LoadError::MapTooLarge)?];
    let info = reader.next_frame(&mut buffer)?;
    let pixels = &buffer[..info.buffer_size()];
    let size = [info.width as usize, info.height as usize];

    match info.color_type {
        png::ColorType::Rgba => Ok(ColorImage::from_rgba_unmultiplied(size, pixels)),
        png::ColorType::Rgb => Ok(ColorImage::from_rgb(size, pixels)),
        color_type => Err(LoadError::MapColorType { color_type }),
    }
}

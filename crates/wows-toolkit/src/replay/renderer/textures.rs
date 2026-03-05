use std::collections::HashMap;

use egui::TextureHandle;

use super::RendererTextures;
use super::ReplayRendererAssets;

/// Generate an outline RGBA image from a source icon's alpha channel.
/// The outline is `thickness` pixels wide around opaque regions (alpha > 128).
/// Returns (rgba_data, width, height) with the same dimensions as the input.
pub(super) fn generate_icon_outline(data: &[u8], w: u32, h: u32, thickness: i32) -> Vec<u8> {
    let iw = w as i32;
    let ih = h as i32;
    let mut out = vec![0u8; (w * h * 4) as usize];

    for y in 0..ih {
        for x in 0..iw {
            let idx = (y * iw + x) as usize;
            let self_alpha = data[idx * 4 + 3];
            if self_alpha > 128 {
                // Inside the icon — leave transparent (icon itself will be drawn on top)
                continue;
            }

            // Check if any neighbor within `thickness` is opaque
            let mut has_opaque_neighbor = false;
            'outer: for ny in (y - thickness).max(0)..=(y + thickness).min(ih - 1) {
                for nx in (x - thickness).max(0)..=(x + thickness).min(iw - 1) {
                    let ni = (ny * iw + nx) as usize;
                    if data[ni * 4 + 3] > 128 {
                        has_opaque_neighbor = true;
                        break 'outer;
                    }
                }
            }

            if has_opaque_neighbor {
                let oi = idx * 4;
                out[oi] = 255; // R (gold)
                out[oi + 1] = 215; // G
                out[oi + 2] = 0; // B
                out[oi + 3] = 230; // A
            }
        }
    }

    out
}

pub(super) fn upload_textures(ctx: &egui::Context, assets: &ReplayRendererAssets) -> RendererTextures {
    let map_texture = assets.map_image.as_ref().map(|asset| {
        let image =
            egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
        ctx.load_texture("replay_map", image, egui::TextureOptions::LINEAR)
    });

    let mut ship_icons: HashMap<String, TextureHandle> = HashMap::new();
    let mut ship_icon_outlines: HashMap<String, TextureHandle> = HashMap::new();
    for (key, asset) in assets.ship_icons.iter() {
        let image =
            egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
        let handle = ctx.load_texture(format!("ship_{}", key), image, egui::TextureOptions::LINEAR);
        ship_icons.insert(key.clone(), handle);

        let outline_data = generate_icon_outline(&asset.data, asset.width, asset.height, 2);
        let outline_image =
            egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &outline_data);
        let outline_handle =
            ctx.load_texture(format!("ship_outline_{}", key), outline_image, egui::TextureOptions::LINEAR);
        ship_icon_outlines.insert(key.clone(), outline_handle);
    }

    let plane_icons: HashMap<String, TextureHandle> = assets
        .plane_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("plane_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let consumable_icons: HashMap<String, TextureHandle> = assets
        .consumable_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("consumable_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let death_cause_icons: HashMap<String, TextureHandle> = assets
        .death_cause_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("death_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let powerup_icons: HashMap<String, TextureHandle> = assets
        .powerup_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("powerup_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    RendererTextures {
        map_texture,
        ship_icons,
        ship_icon_outlines,
        plane_icons,
        consumable_icons,
        death_cause_icons,
        powerup_icons,
    }
}

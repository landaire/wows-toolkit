use std::collections::HashMap;

use egui::TextureHandle;

use super::RendererTextures;
use super::ReplayRendererAssets;

/// Generate an outline RGBA image from a source icon's alpha channel.
/// The outline is `thickness` pixels wide around opaque regions (alpha > 128).
/// Returns (rgba_data, width, height) with the same dimensions as the input.
/// Build a gold halo around the icon's silhouette.
///
/// The output is padded by `thickness` pixels on each side so the halo can
/// extend beyond the icon's bounding box (matches `make_ship_icon_outline`
/// in `minimap_renderer::drawing`). Uses a circular kernel so the halo
/// tapers cleanly to points at narrow ship features instead of squaring off
/// the way a rectangular kernel does.
///
/// Returns `(rgba, padded_w, padded_h)`.
pub(super) fn generate_icon_outline(data: &[u8], w: u32, h: u32, thickness: i32) -> (Vec<u8>, u32, u32) {
    let t = thickness;
    let iw = w as i32;
    let ih = h as i32;
    let ow = iw + 2 * t;
    let oh = ih + 2 * t;
    let mut out = vec![0u8; (ow * oh * 4) as usize];

    let alpha_at = |x: i32, y: i32| -> u8 {
        let ix = x - t;
        let iy = y - t;
        if ix < 0 || iy < 0 || ix >= iw || iy >= ih { 0 } else { data[((iy * iw + ix) as usize) * 4 + 3] }
    };

    let t_sq = t * t;
    for y in 0..oh {
        for x in 0..ow {
            if alpha_at(x, y) > 128 {
                // Inside the icon — leave transparent; the icon itself is drawn on top.
                continue;
            }
            let mut hit = false;
            'scan: for dy in -t..=t {
                for dx in -t..=t {
                    if dx * dx + dy * dy > t_sq {
                        continue;
                    }
                    if alpha_at(x + dx, y + dy) > 128 {
                        hit = true;
                        break 'scan;
                    }
                }
            }
            if hit {
                let oi = ((y * ow + x) as usize) * 4;
                out[oi] = 255;
                out[oi + 1] = 215;
                out[oi + 2] = 0;
                out[oi + 3] = 230;
            }
        }
    }

    (out, ow as u32, oh as u32)
}

pub(super) fn upload_textures(
    ctx: &egui::Context,
    assets: &ReplayRendererAssets,
    silhouette_raw: Option<&(u32, u32, Vec<u8>)>,
) -> RendererTextures {
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

        let (outline_data, outline_w, outline_h) = generate_icon_outline(
            &asset.data,
            asset.width,
            asset.height,
            wows_minimap_renderer::SHIP_ICON_OUTLINE_THICKNESS as i32,
        );
        let outline_image =
            egui::ColorImage::from_rgba_unmultiplied([outline_w as usize, outline_h as usize], &outline_data);
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

    let building_icons: HashMap<String, TextureHandle> = assets
        .building_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("building_{}", key), image, egui::TextureOptions::LINEAR);
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

    let crew_skill_icons: HashMap<String, TextureHandle> = assets
        .crew_skill_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("skill_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let modernization_icons: HashMap<String, TextureHandle> = assets
        .modernization_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("modernization_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let signal_flag_icons: HashMap<String, TextureHandle> = assets
        .signal_flag_icons
        .iter()
        .map(|(key, asset)| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([asset.width as usize, asset.height as usize], &asset.data);
            let handle = ctx.load_texture(format!("signal_{}", key), image, egui::TextureOptions::LINEAR);
            (key.clone(), handle)
        })
        .collect();

    let silhouette_texture = silhouette_raw.map(|(w, h, data)| {
        let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
        ctx.load_texture("stats_silhouette", image, egui::TextureOptions::LINEAR)
    });

    RendererTextures {
        map_texture,
        ship_icons,
        ship_icon_outlines,
        plane_icons,
        building_icons,
        consumable_icons,
        death_cause_icons,
        powerup_icons,
        crew_skill_icons,
        modernization_icons,
        signal_flag_icons,
        silhouette_texture,
    }
}

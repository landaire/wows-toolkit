use ab_glyph::Font;
use ab_glyph::FontArc;
use ab_glyph::PxScale;
use image::RgbImage;
use image::RgbaImage;
use std::collections::HashMap;
use std::io::Read;
use tracing::debug;
use tracing::warn;
use wowsunpack::vfs::VfsPath;

use crate::MINIMAP_SIZE;
use crate::map_data;

/// Icon size in pixels for rasterized ship icons.
/// Scales proportionally with minimap size (18px at 768px minimap).
pub const ICON_SIZE: u32 = MINIMAP_SIZE * 3 / 128;

/// Read a file from the VFS, returning its bytes or None if not found/empty.
fn read_vfs_file(vfs: &VfsPath, path: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    vfs.join(path).ok()?.open_file().ok()?.read_to_end(&mut buf).ok()?;
    if buf.is_empty() { None } else { Some(buf) }
}

pub fn load_packed_image(path: &str, vfs: &VfsPath) -> Option<image::DynamicImage> {
    let buf = read_vfs_file(vfs, path)?;
    image::load_from_memory(&buf).ok()
}

pub fn load_map_image(map_name: &str, vfs: &VfsPath) -> Option<RgbImage> {
    // map_name from meta is e.g. "spaces/28_naval_mission"
    // minimap images live at spaces/<map>/minimap.png in the packed files
    let bare_name = map_name.strip_prefix("spaces/").unwrap_or(map_name);

    let water_path = format!("spaces/{}/minimap_water.png", bare_name);
    let land_path = format!("spaces/{}/minimap.png", bare_name);

    // Load water (background) and land (foreground with alpha) separately,
    // then composite land over water to get the final map image.
    let water = load_packed_image(&water_path, vfs);
    let land = load_packed_image(&land_path, vfs);

    let result = match (water, land) {
        (Some(water_img), Some(land_img)) => {
            // Composite: start with water, overlay land using alpha
            let mut base = water_img.to_rgba8();
            let overlay = land_img.to_rgba8();
            image::imageops::overlay(&mut base, &overlay, 0, 0);
            debug!(width = base.width(), height = base.height(), "Loaded map image (water + land composited)");
            image::DynamicImage::ImageRgba8(base).to_rgb8()
        }
        (Some(water_img), None) => {
            debug!("Loaded map image: water only");
            water_img.to_rgb8()
        }
        (None, Some(land_img)) => {
            debug!("Loaded map image: land only (no water background)");
            land_img.to_rgb8()
        }
        (None, None) => {
            warn!(map = %map_name, "Could not load map image, using blank background");
            return None;
        }
    };

    if result.width() != MINIMAP_SIZE || result.height() != MINIMAP_SIZE {
        let resized =
            image::imageops::resize(&result, MINIMAP_SIZE, MINIMAP_SIZE, image::imageops::FilterType::Lanczos3);
        return Some(resized);
    }
    Some(result)
}

pub fn load_map_info(map_name: &str, vfs: &VfsPath) -> Option<map_data::MapInfo> {
    let bare_name = map_name.strip_prefix("spaces/").unwrap_or(map_name);

    // Try multiple path variants — the virtual filesystem layout may differ
    let candidates =
        [format!("spaces/{}/space.settings", bare_name), format!("content/gameplay/{}/space.settings", bare_name)];
    let mut buf = None;
    for candidate in &candidates {
        if let Some(data) = read_vfs_file(vfs, candidate) {
            debug!(path = %candidate, "Loaded space.settings");
            buf = Some(data);
            break;
        }
    }
    let buf = match buf {
        Some(b) => b,
        None => {
            warn!(map = %bare_name, tried = ?candidates, "Could not load space.settings, using defaults");
            return None;
        }
    };

    let content = String::from_utf8_lossy(&buf);
    let doc = roxmltree::Document::parse(&content).ok()?;

    // Helper: read a value either as an attribute on `node` or as a child element's text
    let read_value = |parent: &roxmltree::Node, name: &str| -> Option<String> {
        // Try attribute first (e.g. <bounds minX="-9" />)
        if let Some(v) = parent.attribute(name) {
            return Some(v.to_string());
        }
        // Then try child element (e.g. <bounds><minX> -9 </minX></bounds>)
        parent.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(|t| t.trim().to_string())
    };

    let bounds = doc.descendants().find(|n| n.has_tag_name("bounds"))?;
    let min_x: i32 = read_value(&bounds, "minX")?.parse().ok()?;
    let max_x: i32 = read_value(&bounds, "maxX")?.parse().ok()?;
    let min_y: i32 = read_value(&bounds, "minY")?.parse().ok()?;
    let max_y: i32 = read_value(&bounds, "maxY")?.parse().ok()?;

    // chunkSize can be a child element of root or of <terrain>
    let chunk_size: f64 = doc
        .descendants()
        .find(|n| n.has_tag_name("chunkSize"))
        .and_then(|n| n.text().and_then(|t| t.trim().parse().ok()))
        .unwrap_or(100.0);

    // Formula from Python spaces.py:
    // w = len(range(min_x, max_x + 1)) * chunk_size - 4 * chunk_size
    let chunks_x = (max_x - min_x + 1) as f64;
    let chunks_y = (max_y - min_y + 1) as f64;
    let space_w = ((chunks_x - 4.0) * chunk_size).round() as i32;
    let space_h = ((chunks_y - 4.0) * chunk_size).round() as i32;

    // Use the larger dimension as space_size (maps should be square)
    let space_size = space_w.max(space_h);

    debug!(
        map = %bare_name,
        bounds_min = ?(min_x, min_y),
        bounds_max = ?(max_x, max_y),
        chunk_size,
        space_size,
        "Map metadata"
    );

    Some(map_data::MapInfo { space_size })
}

/// Load and rasterize ship SVG icons from game files.
/// Returns a map from species name to RGBA image.
///
/// Loads 5 variants per species:
/// - `"{Species}"` — base icon (visible ally/enemy)
/// - `"{Species}_self"` — player's own ship
/// - `"{Species}_dead"` — destroyed ship
/// - `"{Species}_invisible"` — not currently detected
/// - `"{Species}_last_visible"` — last known position (minimap-only)
pub fn load_ship_icons(vfs: &VfsPath) -> HashMap<String, RgbaImage> {
    let species_names = ["Destroyer", "Cruiser", "Battleship", "AirCarrier", "Submarine", "Auxiliary"];
    // (file suffix, key suffix) — all in gui/fla/minimap/ship_icons/
    let variants: &[(&str, &str)] =
        &[("", ""), ("_dead", "_dead"), ("_invisible", "_invisible"), ("_last_visible", "_last_visible")];
    let mut icons = HashMap::new();
    let load_svg = |path: &str, key: &str, icons: &mut HashMap<String, RgbaImage>| {
        if let Some(buf) = read_vfs_file(vfs, path)
            && let Some(img) = rasterize_svg(&buf, ICON_SIZE)
        {
            icons.insert(key.to_string(), img);
            return true;
        }
        false
    };
    for name in &species_names {
        let lower = name.to_ascii_lowercase();
        for &(file_suffix, key_suffix) in variants {
            let path = format!("gui/fla/minimap/ship_icons/minimap_{}{}.svg", lower, file_suffix);
            let key = format!("{}{}", name, key_suffix);
            load_svg(&path, &key, &mut icons);
        }
        // Self icons from ship_icons_self/ directory
        // Try species-specific first, then generic fallback
        let self_key = format!("{}_self", name);
        let self_paths = [
            format!("gui/fla/minimap/ship_icons_self/minimap_self_alive_{}.svg", lower),
            "gui/fla/minimap/ship_icons_self/minimap_self_alive.svg".to_string(),
        ];
        for path in &self_paths {
            if load_svg(path, &self_key, &mut icons) {
                break;
            }
        }
        // Dead-self variant
        let dead_self_key = format!("{}_dead_self", name);
        let dead_self_paths = [
            format!("gui/fla/minimap/ship_icons_self/minimap_self_dead_{}.svg", lower),
            "gui/fla/minimap/ship_icons_self/minimap_self_dead.svg".to_string(),
        ];
        for path in &dead_self_paths {
            if load_svg(path, &dead_self_key, &mut icons) {
                break;
            }
        }
    }
    debug!(count = icons.len(), "Loaded ship icons");
    if icons.is_empty() {
        warn!("No ship icons loaded, using fallback circles");
    }
    icons
}

/// Load all plane icons from game files into a HashMap keyed by name (e.g. "fighter_ally").
pub fn load_plane_icons(vfs: &VfsPath) -> HashMap<String, RgbaImage> {
    let dirs = [
        "gui/battle_hud/markers_minimap/plane/consumables",
        "gui/battle_hud/markers_minimap/plane/controllable",
        "gui/battle_hud/markers_minimap/plane/airsupport",
    ];
    let suffixes = ["ally", "enemy", "own", "division", "teamkiller"];
    let base_names = [
        // controllable
        "fighter_he",
        "fighter_ap",
        "fighter_he_st2024",
        "bomber_he",
        "bomber_ap",
        "bomber_ap_st2024",
        "skip_he",
        "skip_ap",
        "torpedo_regular",
        "torpedo_regular_st2024",
        "torpedo_deepwater",
        "auxiliary",
        // consumables
        "fighter",
        "fighter_upgrade",
        "scout",
        "smoke",
        // airsupport
        "bomber_depth_charge",
        "bomber_mine",
    ];

    let mut icons = HashMap::new();
    for dir in &dirs {
        // Use the last path component as namespace (e.g. "consumables", "controllable", "airsupport")
        let dir_name = dir.rsplit('/').next().unwrap_or(dir);
        for base in &base_names {
            for suffix in &suffixes {
                let name = format!("{}_{}", base, suffix);
                let path = format!("{}/{}.png", dir, name);
                if let Some(img) = load_packed_image(&path, vfs) {
                    let key = format!("{}/{}", dir_name, name);
                    let rgba = img.to_rgba8();
                    // Resize to ICON_SIZE to scale with minimap
                    let resized =
                        image::imageops::resize(&rgba, ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);
                    icons.insert(key, resized);
                }
            }
        }
    }
    debug!(count = icons.len(), "Loaded plane icons");
    icons
}

/// Load consumable icons from game files into a HashMap keyed by PCY name.
///
/// Discovers all `consumable_PCY*.png` files in `gui/consumables/` to support
/// all ability variants (base, Premium, Super, TimeBased, etc.).
pub fn load_consumable_icons(vfs: &VfsPath) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Ok(dir) = vfs.join("gui/consumables")
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            // Match files like "consumable_PCY009_CrashCrewPremium.png"
            if let Some(pcy_name) = filename.strip_prefix("consumable_").and_then(|s| s.strip_suffix(".png")) {
                if !pcy_name.starts_with("PCY") {
                    continue;
                }
                let path = format!("gui/consumables/{}", filename);
                if let Some(img) = load_packed_image(&path, vfs) {
                    let resized = image::imageops::resize(&img, 28, 28, image::imageops::FilterType::Lanczos3);
                    icons.insert(pcy_name.to_string(), resized);
                }
            }
        }
    }

    debug!(count = icons.len(), "Loaded consumable icons");
    icons
}

/// Load death cause icons from game files into a HashMap keyed by cause name.
///
/// Discovers `icon_frag_*.png` files in `gui/battle_hud/icon_frag/` and stores
/// them resized to `size x size` pixels, keyed by the base name (e.g. `"main_caliber"`).
pub fn load_death_cause_icons(vfs: &VfsPath, size: u32) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Ok(dir) = vfs.join("gui/battle_hud/icon_frag")
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            if let Some(base_name) = filename.strip_prefix("icon_frag_").and_then(|s| s.strip_suffix(".png")) {
                let path = format!("gui/battle_hud/icon_frag/{}", filename);
                if let Some(img) = load_packed_image(&path, vfs) {
                    let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                    icons.insert(base_name.to_string(), resized);
                }
            }
        }
    }

    debug!(count = icons.len(), "Loaded death cause icons");
    icons
}

/// Load powerup (arms race buff) icons from game files.
///
/// Discovers `icon_marker_*.png` files in `gui/powerups/drops/` and stores them
/// resized to `size x size` pixels, keyed by marker name (e.g. `"damage_active"`).
pub fn load_powerup_icons(vfs: &VfsPath, size: u32) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Ok(dir) = vfs.join("gui/powerups/drops")
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            if let Some(marker_name) = filename.strip_prefix("icon_marker_").and_then(|s| s.strip_suffix(".png")) {
                // Skip _small variants
                if marker_name.ends_with("_small") {
                    continue;
                }
                let path = format!("gui/powerups/drops/{}", filename);
                if let Some(img) = load_packed_image(&path, vfs) {
                    let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                    icons.insert(marker_name.to_string(), resized);
                }
            }
        }
    }

    debug!(count = icons.len(), "Loaded powerup icons");
    icons
}

/// Load flag icons for base-type capture points.
///
/// Returns a map keyed by `"ally"`, `"enemy"`, `"neutral"` containing the
/// corresponding flag PNG from `gui/battle_hud/markers/capture_point/`.
pub fn load_flag_icons(vfs: &VfsPath) -> HashMap<String, RgbaImage> {
    let variants = [
        ("ally", "gui/battle_hud/markers/capture_point/icon_base_ally_flag.png"),
        ("enemy", "gui/battle_hud/markers/capture_point/icon_base_enemy_flag.png"),
        ("neutral", "gui/battle_hud/markers/capture_point/icon_base_neutral_flag.png"),
    ];
    let mut icons = HashMap::new();
    for (key, path) in &variants {
        if let Some(img) = load_packed_image(path, vfs) {
            let resized = image::imageops::resize(&img, ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);
            icons.insert(key.to_string(), resized);
        }
    }
    debug!(count = icons.len(), "Loaded flag icons");
    icons
}

/// Rasterize an SVG byte buffer to an RGBA image at the given size.
///
/// Automatically crops transparent padding from the SVG and fills the output
/// as much as possible.
pub fn rasterize_svg(svg_data: &[u8], size: u32) -> Option<RgbaImage> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg_data, &opt).ok()?;

    // Render at a larger internal size for accurate bounding-box detection.
    // Use pre_scale so the offset is in output-pixel space (not scaled).
    let internal_size = size * 4;
    let tree_size = tree.size();
    let sx = internal_size as f32 / tree_size.width();
    let sy = internal_size as f32 / tree_size.height();
    let scale = sx.min(sy);

    let mut pixmap = tiny_skia::Pixmap::new(internal_size, internal_size)?;

    let offset_x = (internal_size as f32 - tree_size.width() * scale) / 2.0;
    let offset_y = (internal_size as f32 - tree_size.height() * scale) / 2.0;
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(offset_x, offset_y);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Find bounding box of non-transparent pixels
    let w = pixmap.width();
    let h = pixmap.height();
    let data = pixmap.data();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize * 4;
            if data[idx + 3] > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if max_x < min_x || max_y < min_y {
        // Fully transparent — return empty icon at target size
        return RgbaImage::from_raw(size, size, vec![0u8; (size * size * 4) as usize]);
    }

    // Crop to bounding box with 1px margin
    let margin = 1u32;
    let crop_x = min_x.saturating_sub(margin);
    let crop_y = min_y.saturating_sub(margin);
    let crop_w = (max_x + 1 + margin).min(w) - crop_x;
    let crop_h = (max_y + 1 + margin).min(h) - crop_y;

    // Extract cropped RGBA data (unpremultiply alpha from tiny-skia's premultiplied format)
    let mut cropped = RgbaImage::new(crop_w, crop_h);
    for y in 0..crop_h {
        for x in 0..crop_w {
            let src_idx = ((crop_y + y) * w + crop_x + x) as usize * 4;
            let a = data[src_idx + 3];
            let (r, g, b) = if a > 0 {
                let af = a as f32 / 255.0;
                (
                    (data[src_idx] as f32 / af).min(255.0) as u8,
                    (data[src_idx + 1] as f32 / af).min(255.0) as u8,
                    (data[src_idx + 2] as f32 / af).min(255.0) as u8,
                )
            } else {
                (0, 0, 0)
            };
            cropped.put_pixel(x, y, image::Rgba([r, g, b, a]));
        }
    }

    // Resize cropped image to fit within size x size, maintaining aspect ratio
    let fit_sx = size as f32 / crop_w as f32;
    let fit_sy = size as f32 / crop_h as f32;
    let fit_scale = fit_sx.min(fit_sy);
    let final_w = (crop_w as f32 * fit_scale).round().max(1.0) as u32;
    let final_h = (crop_h as f32 * fit_scale).round().max(1.0) as u32;

    let resized = image::imageops::resize(&cropped, final_w, final_h, image::imageops::FilterType::Lanczos3);

    // Center in size x size canvas
    let mut output = RgbaImage::new(size, size);
    let ox = (size.saturating_sub(final_w)) / 2;
    let oy = (size.saturating_sub(final_h)) / 2;
    image::imageops::overlay(&mut output, &resized, ox as i64, oy as i64);

    Some(output)
}

// ── Game Fonts ─────────────────────────────────────────────────────────────

/// Game fonts loaded from game files, with CJK fallback support.
///
/// The `primary` font is used for all UI text. For chat messages that contain
/// characters not covered by the primary font, `font_for_text()` selects
/// the first fallback font that can render all glyphs.
///
/// Each font carries a scale correction factor so that glyphs render at
/// visually consistent sizes regardless of the font's internal metrics.
/// Use `scale()` instead of `PxScale::from()` to get correctly-adjusted sizes.
#[derive(Clone)]
pub struct GameFonts {
    /// Primary font (Warhelios Bold) — used for all UI text.
    pub primary: FontArc,
    /// Fallback fonts for CJK characters, tried in order (KO, JP, CN).
    pub fallbacks: Vec<FontArc>,
    /// Scale correction factor for the primary font.
    pub primary_scale_factor: f32,
    /// Per-fallback scale correction factors (same order as `fallbacks`).
    pub fallback_scale_factors: Vec<f32>,
    /// Raw TTF bytes of the primary font (for external consumers like egui).
    pub primary_bytes: Vec<u8>,
    /// Raw TTF bytes of the fallback fonts (same order as `fallbacks`).
    pub fallback_bytes: Vec<Vec<u8>>,
}

impl GameFonts {
    /// Pick the best font that can render all characters in `text`.
    ///
    /// Tries primary first, then each fallback in order. Returns primary
    /// if no font fully covers the text.
    pub fn font_for_text(&self, text: &str) -> &FontArc {
        if Self::can_render(&self.primary, text) {
            return &self.primary;
        }
        for fallback in &self.fallbacks {
            if Self::can_render(fallback, text) {
                return fallback;
            }
        }
        &self.primary
    }

    /// Returns the index into `fallbacks` if a fallback was selected, or None for primary.
    pub fn font_hint_for_text(&self, text: &str) -> Option<usize> {
        if Self::can_render(&self.primary, text) {
            return None;
        }
        for (i, fallback) in self.fallbacks.iter().enumerate() {
            if Self::can_render(fallback, text) {
                return Some(i);
            }
        }
        None
    }

    /// Get a corrected `PxScale` for the primary font.
    ///
    /// Use this instead of `PxScale::from()` to ensure consistent visual sizing.
    pub fn scale(&self, size: f32) -> PxScale {
        PxScale::from(size * self.primary_scale_factor)
    }

    /// Get a corrected `PxScale` for the font indicated by a `FontHint`.
    pub fn scale_for_hint(&self, size: f32, hint: crate::draw_command::FontHint) -> PxScale {
        use crate::draw_command::FontHint;
        let factor = match hint {
            FontHint::Primary => self.primary_scale_factor,
            FontHint::Fallback(i) => self.fallback_scale_factors.get(i).copied().unwrap_or(self.primary_scale_factor),
        };
        PxScale::from(size * factor)
    }

    /// Check if a font can render every character in a string.
    fn can_render(font: &FontArc, text: &str) -> bool {
        use ab_glyph::Font;
        text.chars().all(|c| font.glyph_id(c).0 != 0)
    }
}

/// Compute a scale correction factor for a font so that its cap-height
/// matches the reference (DejaVu Sans Bold).
///
/// Measures the 'M' glyph height at a known scale and compares to a reference
/// ratio. Returns a multiplier to apply to all `PxScale` values.
fn compute_scale_factor(font: &FontArc) -> f32 {
    // Reference cap-height ratio — tuned for visual clarity on minimap.
    const REFERENCE_RATIO: f32 = 0.80;

    let scale = PxScale::from(100.0);
    let glyph_id = font.glyph_id('M');
    let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(0.0, 100.0));
    if let Some(outlined) = font.outline_glyph(glyph) {
        let bounds = outlined.px_bounds();
        let actual_height = bounds.max.y - bounds.min.y;
        let actual_ratio = actual_height / 100.0;
        if actual_ratio > 0.01 {
            let factor = REFERENCE_RATIO / actual_ratio;
            debug!(actual_ratio, factor, "Font scale factor computed");
            return factor;
        }
    }
    1.0
}

/// Load game fonts from packed game files.
///
/// Tries to load Warhelios Bold as the primary font. CJK fallback fonts
/// (Korean, Japanese, Chinese) are loaded if present. Each font gets a
/// scale correction factor computed automatically.
pub fn load_game_fonts(vfs: &VfsPath) -> GameFonts {
    let load_font = |path: &str| -> Option<(FontArc, Vec<u8>)> {
        let buf = read_vfs_file(vfs, path)?;
        let raw_bytes = buf.clone();
        match FontArc::try_from_vec(buf) {
            Ok(font) => {
                debug!(path, "Loaded game font");
                Some((font, raw_bytes))
            }
            Err(_) => {
                warn!(path, "Failed to parse game font");
                None
            }
        }
    };

    let (primary, primary_bytes) = load_font("gui/fonts/Warhelios.ttf")
        .or_else(|| load_font("gui/fonts/Warhelios_Regular.ttf"))
        .or_else(|| load_font("gui/fonts/Warhelios_Bold.ttf"))
        .expect(
            "Failed to load Warhelios font from game files. \
             Make sure the game directory is correct.",
        );

    let fallback_paths = [
        "gui/fonts/WarheliosKO_Bold.ttf",
        "gui/fonts/Source_Han_Sans_JP_Bold_WH.ttf",
        "gui/fonts/Source_Han_Sans_CN_Bold_WH.ttf",
    ];
    let fallbacks_with_bytes: Vec<(FontArc, Vec<u8>)> =
        fallback_paths.iter().filter_map(|path| load_font(path)).collect();
    let fallback_bytes: Vec<Vec<u8>> = fallbacks_with_bytes.iter().map(|(_, b)| b.clone()).collect();
    let fallbacks: Vec<FontArc> = fallbacks_with_bytes.into_iter().map(|(f, _)| f).collect();

    let primary_scale_factor = compute_scale_factor(&primary);
    let fallback_scale_factors: Vec<f32> = fallbacks.iter().map(compute_scale_factor).collect();

    debug!(fallback_count = fallbacks.len(), primary_scale_factor, "Loaded game fonts");

    GameFonts { primary, fallbacks, primary_scale_factor, fallback_scale_factors, primary_bytes, fallback_bytes }
}

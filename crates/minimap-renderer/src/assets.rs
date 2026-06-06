use ab_glyph::Font;
use ab_glyph::FontArc;
use ab_glyph::PxScale;
use image::RgbImage;
use image::RgbaImage;
use std::collections::HashMap;
use std::io::Read;
use tracing::debug;
use tracing::warn;
use wowsunpack::data::Version;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::GuiAssetDir;
use wowsunpack::game_assets::PlaneMarkerKind;
use wowsunpack::game_assets::Relation;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
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

/// Read an already-resolved VFS entry, returning its bytes or None if empty.
fn read_vfs_entry(entry: &VfsPath) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    entry.open_file().ok()?.read_to_end(&mut buf).ok()?;
    if buf.is_empty() { None } else { Some(buf) }
}

pub fn load_packed_image(path: &str, vfs: &VfsPath) -> Option<image::DynamicImage> {
    let buf = read_vfs_file(vfs, path)?;
    image::load_from_memory(&buf).ok()
}

/// Decode an image from an already-resolved VFS entry.
fn load_image_entry(entry: &VfsPath) -> Option<image::DynamicImage> {
    image::load_from_memory(&read_vfs_entry(entry)?).ok()
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
pub fn load_ship_icons(vfs: &VfsPath, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let species = [
        Species::Destroyer,
        Species::Cruiser,
        Species::Battleship,
        Species::AirCarrier,
        Species::Submarine,
        Species::Auxiliary,
    ];
    let mut icons = HashMap::new();
    let load_svg = |asset: GuiAsset<'_>, key: String, icons: &mut HashMap<String, RgbaImage>| {
        if let Some(buf) = asset.read(vfs, version)
            && let Some(img) = rasterize_svg(&buf, ICON_SIZE)
        {
            icons.insert(key, img);
            return true;
        }
        false
    };
    for species in species {
        let name = species.name();
        for (state, key_suffix) in [
            (ShipIconState::Alive, ""),
            (ShipIconState::Dead, "_dead"),
            (ShipIconState::Invisible, "_invisible"),
            (ShipIconState::LastVisible, "_last_visible"),
        ] {
            load_svg(GuiAsset::ShipClassIcon { species, state }, format!("{name}{key_suffix}"), &mut icons);
        }
        // The self-icon resolver falls back from the species-specific path to a generic one.
        load_svg(GuiAsset::SelfShipIcon { species, alive: true }, format!("{name}_self"), &mut icons);
        load_svg(GuiAsset::SelfShipIcon { species, alive: false }, format!("{name}_dead_self"), &mut icons);
    }
    debug!(count = icons.len(), "Loaded ship icons");
    if icons.is_empty() {
        warn!("No ship icons loaded, using fallback circles");
    }
    icons
}

/// Load all plane icons from game files into a HashMap keyed by name (e.g. "fighter_ally").
pub fn load_plane_icons(vfs: &VfsPath, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let kinds = [PlaneMarkerKind::Consumables, PlaneMarkerKind::Controllable, PlaneMarkerKind::AirSupport];
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
    for kind in kinds {
        let Some(dir) = GuiAssetDir::PlaneMarkers(kind).resolve(vfs, version) else {
            continue;
        };
        // Namespace keys by subdirectory (e.g. "consumables", "controllable", "airsupport").
        let dir_name = dir.filename();
        for base in &base_names {
            for suffix in &suffixes {
                let name = format!("{}_{}", base, suffix);
                let Ok(entry) = dir.join(format!("{name}.png")) else { continue };
                if let Some(img) = load_image_entry(&entry) {
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

/// Load building icons from game files into a HashMap keyed by `"{type}_{relation}"`.
///
/// Icons are at `gui/battle_hud/markers/building_icons/normal/icon_ground_{type}_{relation}.png`.
/// Keys use the format `"artillery_enemy"`, `"airbase_dead"`, `"air_defence_suppressed_ally"`, etc.
pub fn load_building_icons(vfs: &VfsPath, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let types = ["airbase", "air_defence", "artillery", "generator", "radar", "station", "supply", "tower"];
    let relations = ["ally", "enemy", "neutral", "dead", "suppressed_ally", "suppressed_enemy", "suppressed_neutral"];

    let mut icons = HashMap::new();
    let Some(dir) = GuiAssetDir::BuildingIcons.resolve(vfs, version) else {
        return icons;
    };
    for btype in &types {
        for relation in &relations {
            let filename = format!("icon_ground_{}_{}.png", btype, relation);
            let Ok(entry) = dir.join(&filename) else { continue };
            if let Some(img) = load_image_entry(&entry) {
                let resized =
                    image::imageops::resize(&img, ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);
                let key = format!("{}_{}", btype, relation);
                icons.insert(key, resized);
            }
        }
    }
    debug!(count = icons.len(), "Loaded building icons");
    icons
}

/// Load consumable icons from game files into a HashMap keyed by PCY name.
///
/// Discovers all `consumable_PCY*.png` files in `gui/consumables/` to support
/// all ability variants (base, Premium, Super, TimeBased, etc.).
pub fn load_consumable_icons(vfs: &VfsPath, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Some(dir) = GuiAssetDir::Consumables.resolve(vfs, version)
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            // Match files like "consumable_PCY009_CrashCrewPremium.png"
            if let Some(pcy_name) = filename.strip_prefix("consumable_").and_then(|s| s.strip_suffix(".png")) {
                if !pcy_name.starts_with("PCY") {
                    continue;
                }
                if let Some(img) = load_image_entry(&entry) {
                    let resized = image::imageops::resize(&img, 28, 28, image::imageops::FilterType::Lanczos3);
                    icons.insert(pcy_name.to_string(), resized);
                }
            }
        }
    }

    debug!(count = icons.len(), "Loaded consumable icons");
    icons
}

/// Load captain-skill icons from `gui/crew_commander/skills/`.
///
/// Filenames are snake_case matching `CrewSkill::internal_name()` after a
/// `Case::Snake` conversion. The map is keyed by that snake_case slug
/// (e.g. `"consumables_duration"`), so callers feed the converted internal
/// name when looking up the icon for a learned skill.
pub fn load_crew_skill_icons(vfs: &VfsPath, size: u32, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Some(dir) = GuiAssetDir::CrewSkills.resolve(vfs, version)
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            if let Some(key) = filename.strip_suffix(".png")
                && let Some(img) = load_image_entry(&entry)
            {
                let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                icons.insert(key.to_string(), resized);
            }
        }
    }

    debug!(count = icons.len(), "Loaded crew skill icons");
    icons
}

/// Load modernization (ship upgrade) icons from `gui/modernization_icons/`.
///
/// Filenames are `icon_modernization_<PCM full name>.png`; the map is keyed
/// by the full PCM name (`Param::name()`, e.g. `"PCM082_SpecialBonus_Mod_I"`)
/// so callers can look up by the upgrade param's full name.
pub fn load_modernization_icons(vfs: &VfsPath, size: u32, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Some(dir) = GuiAssetDir::Modernizations.resolve(vfs, version)
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            if let Some(pcm_name) = filename.strip_prefix("icon_modernization_").and_then(|s| s.strip_suffix(".png"))
                && let Some(img) = load_image_entry(&entry)
            {
                let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                icons.insert(pcm_name.to_string(), resized);
            }
        }
    }

    debug!(count = icons.len(), "Loaded modernization icons");
    icons
}

/// Load signal-flag icons from `gui/signal_flags/`.
///
/// Filenames are `<PCEF full name>.png`; the map is keyed by the full PCEF
/// name (`Param::name()`, e.g. `"PCEF014_NF_SignalFlag"`) so callers can
/// look up by the signal param's full name.
pub fn load_signal_flag_icons(vfs: &VfsPath, size: u32, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Some(dir) = GuiAssetDir::SignalFlags.resolve(vfs, version)
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            if let Some(pcef_name) = filename.strip_suffix(".png") {
                if !pcef_name.starts_with("PCEF") {
                    continue;
                }
                if let Some(img) = load_image_entry(&entry) {
                    let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                    icons.insert(pcef_name.to_string(), resized);
                }
            }
        }
    }

    debug!(count = icons.len(), "Loaded signal flag icons");
    icons
}

/// Load death cause icons from game files into a HashMap keyed by cause name.
///
/// Discovers `icon_frag_*.png` files in `gui/battle_hud/icon_frag/` and stores
/// them resized to `size x size` pixels, keyed by the base name (e.g. `"main_caliber"`).
pub fn load_death_cause_icons(vfs: &VfsPath, size: u32, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Some(dir) = GuiAssetDir::DeathCauseIcons.resolve(vfs, version)
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            // `frags.png` sits next to the per-cause icons and is the in-game
            // kill-count glyph (used by the roster panel's frag column).
            let key = if filename == "frags.png" {
                Some("frags".to_string())
            } else {
                filename.strip_prefix("icon_frag_").and_then(|s| s.strip_suffix(".png")).map(|s| s.to_string())
            };
            if let Some(base_name) = key
                && let Some(img) = load_image_entry(&entry)
            {
                let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
                icons.insert(base_name, resized);
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
pub fn load_powerup_icons(vfs: &VfsPath, size: u32, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let mut icons = HashMap::new();

    if let Some(dir) = GuiAssetDir::PowerupDrops.resolve(vfs, version)
        && let Ok(entries) = dir.read_dir()
    {
        for entry in entries {
            let filename = entry.filename();
            if let Some(marker_name) = filename.strip_prefix("icon_marker_").and_then(|s| s.strip_suffix(".png")) {
                // Skip _small variants
                if marker_name.ends_with("_small") {
                    continue;
                }
                if let Some(img) = load_image_entry(&entry) {
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
pub fn load_flag_icons(vfs: &VfsPath, version: Option<&Version>) -> HashMap<String, RgbaImage> {
    let variants = [("ally", Relation::Ally), ("enemy", Relation::Enemy), ("neutral", Relation::Neutral)];
    let mut icons = HashMap::new();
    for (key, relation) in variants {
        if let Some(buf) = GuiAsset::CapturePointFlag(relation).read(vfs, version)
            && let Ok(img) = image::load_from_memory(&buf)
        {
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

    /// Pick the best font that can render all characters in `text`.
    ///
    /// Tries primary first, then each fallback in order. Returns primary
    /// if no font fully covers the text.
    pub fn font_for_text(&self, text: &str) -> &FontArc {
        match self.font_hint_for_text(text) {
            Some(i) => self.fallbacks.get(i).unwrap_or(&self.primary),
            None => &self.primary,
        }
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

    /// Pick the best font for the given text and return it with the correct scale.
    pub fn font_and_scale(&self, text: &str, size: f32) -> (&FontArc, PxScale) {
        let hint = self.font_hint_for_text(text);
        let font = match hint {
            Some(i) => self.fallbacks.get(i).unwrap_or(&self.primary),
            None => &self.primary,
        };
        let scale = self.scale_for_hint(
            size,
            match hint {
                Some(i) => crate::draw_command::FontHint::Fallback(i),
                None => crate::draw_command::FontHint::Primary,
            },
        );
        (font, scale)
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

/// Load a Latin sans-serif font from a well-known OS location, used when the game
/// VFS carries no usable TrueType face (older clients ship bitmap fonts only).
fn load_system_fallback_font() -> Option<(FontArc, Vec<u8>)> {
    const CANDIDATES: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        // macOS
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
        // Linux
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(font) = FontArc::try_from_vec(bytes.clone())
        {
            debug!(path, "Loaded system fallback font");
            return Some((font, bytes));
        }
    }
    None
}

/// Try to load the game TrueType fonts (Warhelios primary + CJK fallbacks) from
/// a single VFS. Returns None when the VFS carries no usable Warhelios TTF
/// (older clients shipped bitmap fonts only).
fn game_fonts_from_vfs(vfs: &VfsPath) -> Option<GameFonts> {
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
        .or_else(|| load_font("gui/fonts/Warhelios_Bold.ttf"))?;

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

    Some(GameFonts { primary, fallbacks, primary_scale_factor, fallback_scale_factors, primary_bytes, fallback_bytes })
}

/// Build a `GameFonts` from a system sans-serif face, used when no game TTF is
/// available in any VFS.
fn system_game_fonts() -> GameFonts {
    warn!("no Warhelios TTF in game files (older client?); falling back to a system font");
    let (primary, primary_bytes) =
        load_system_fallback_font().expect("no usable font found in the game files or on the system");
    let primary_scale_factor = compute_scale_factor(&primary);
    GameFonts {
        primary,
        fallbacks: Vec::new(),
        primary_scale_factor,
        fallback_scale_factors: Vec::new(),
        primary_bytes,
        fallback_bytes: Vec::new(),
    }
}

/// Load game fonts from a single VFS, falling back to a system font when the
/// VFS ships no TrueType face.
pub fn load_game_fonts(vfs: &VfsPath) -> GameFonts {
    game_fonts_from_vfs(vfs).unwrap_or_else(system_game_fonts)
}

/// First `Some` from `primary` then `fallbacks` in order, else `default`.
fn first_available<T>(
    primary: Option<T>,
    fallbacks: impl IntoIterator<Item = Option<T>>,
    default: impl FnOnce() -> T,
) -> T {
    primary.or_else(|| fallbacks.into_iter().flatten().next()).unwrap_or_else(default)
}

/// Load game fonts, trying `vfs` first, then each `fallbacks` VFS in order (dump
/// builds newest-first for old replays that ship no TTF), and a system font only
/// when none carry a usable face.
pub fn load_game_fonts_with_fallbacks(vfs: &VfsPath, fallbacks: &[VfsPath]) -> GameFonts {
    first_available(game_fonts_from_vfs(vfs), fallbacks.iter().map(game_fonts_from_vfs), system_game_fonts)
}

#[cfg(test)]
mod font_fallback_test {
    use super::first_available;

    #[test]
    fn picks_first_available_in_order() {
        // Primary present: use it.
        assert_eq!(first_available(Some(1), [Some(2), Some(3)], || 9), 1);
        // Primary absent: first available fallback (newest-first ordering).
        assert_eq!(first_available(None, [None, Some(2), Some(3)], || 9), 2);
        // Nothing available: default (system font).
        assert_eq!(first_available::<i32>(None, [None, None], || 9), 9);
    }
}

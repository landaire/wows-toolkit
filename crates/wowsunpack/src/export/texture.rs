//! DDS texture loading and conversion for glTF export.

use std::io::Cursor;

use image_dds::image::ExtendedColorType;
use image_dds::image::ImageEncoder;
use image_dds::image::codecs::png::PngEncoder;
use rootcause::Report;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TextureError {
    #[error("failed to parse DDS: {0}")]
    DdsParse(String),
    #[error("failed to decode DDS image: {0}")]
    DdsDecode(String),
    #[error("failed to encode PNG: {0}")]
    PngEncode(String),
}

/// Force all alpha values to 255 in an RGBA8 PNG buffer.
/// Re-decodes and re-encodes the PNG. Used for model textures where the DDS alpha
/// channel stores non-opacity data (height, roughness).
pub fn force_png_opaque(png_bytes: &mut Vec<u8>) {
    use image_dds::image::ImageReader;
    let Ok(reader) = ImageReader::new(Cursor::new(&*png_bytes)).with_guessed_format() else {
        return;
    };
    let Ok(img) = reader.decode() else { return };
    let mut rgba = img.into_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel[3] = 255;
    }
    let mut buf = Vec::new();
    if PngEncoder::new(&mut buf)
        .write_image(rgba.as_raw(), rgba.width(), rgba.height(), ExtendedColorType::Rgba8)
        .is_ok()
    {
        *png_bytes = buf;
    }
}

/// Decode DDS bytes to PNG bytes (RGBA8), optionally downsampling to a max size.
///
/// If `max_size` is `Some(n)`, the image is downsampled using box filtering so
/// that neither dimension exceeds `n`. This is a simple but effective way to
/// reduce texture memory for map-scale visualization.
pub fn dds_to_png_resized(dds_bytes: &[u8], max_size: Option<u32>) -> Result<Vec<u8>, Report<TextureError>> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(dds_bytes))
        .map_err(|e| Report::new(TextureError::DdsParse(e.to_string())))?;

    let rgba_image =
        image_dds::image_from_dds(&dds, 0).map_err(|e| Report::new(TextureError::DdsDecode(e.to_string())))?;

    let (w, h) = (rgba_image.width(), rgba_image.height());

    // Downsample if needed.
    let (out_w, out_h, pixels) = if let Some(max) = max_size
        && (w > max || h > max)
    {
        let scale = (max as f32 / w as f32).min(max as f32 / h as f32);
        let nw = ((w as f32 * scale) as u32).max(1);
        let nh = ((h as f32 * scale) as u32).max(1);
        let src = rgba_image.as_raw();
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        // Box filter: average source pixels that map to each destination pixel.
        for dy in 0..nh {
            let sy0 = (dy as f64 * h as f64 / nh as f64) as u32;
            let sy1 = (((dy + 1) as f64 * h as f64 / nh as f64) as u32).min(h);
            for dx in 0..nw {
                let sx0 = (dx as f64 * w as f64 / nw as f64) as u32;
                let sx1 = (((dx + 1) as f64 * w as f64 / nw as f64) as u32).min(w);
                let mut r = 0u32;
                let mut g = 0u32;
                let mut b = 0u32;
                let mut a = 0u32;
                let mut count = 0u32;
                for sy in sy0..sy1 {
                    for sx in sx0..sx1 {
                        let i = (sy * w + sx) as usize * 4;
                        r += src[i] as u32;
                        g += src[i + 1] as u32;
                        b += src[i + 2] as u32;
                        a += src[i + 3] as u32;
                        count += 1;
                    }
                }
                if count > 0 {
                    let di = (dy * nw + dx) as usize * 4;
                    dst[di] = (r / count) as u8;
                    dst[di + 1] = (g / count) as u8;
                    dst[di + 2] = (b / count) as u8;
                    dst[di + 3] = (a / count) as u8;
                }
            }
        }
        (nw, nh, dst)
    } else {
        (w, h, rgba_image.into_raw())
    };

    let mut png_buf = Vec::new();
    PngEncoder::new(&mut png_buf)
        .write_image(&pixels, out_w, out_h, ExtendedColorType::Rgba8)
        .map_err(|e| Report::new(TextureError::PngEncode(e.to_string())))?;

    Ok(png_buf)
}

/// Decode DDS bytes to PNG bytes (RGBA8).
pub fn dds_to_png(dds_bytes: &[u8]) -> Result<Vec<u8>, Report<TextureError>> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(dds_bytes))
        .map_err(|e| Report::new(TextureError::DdsParse(e.to_string())))?;

    let rgba_image =
        image_dds::image_from_dds(&dds, 0).map_err(|e| Report::new(TextureError::DdsDecode(e.to_string())))?;

    let mut png_buf = Vec::new();
    PngEncoder::new(&mut png_buf)
        .write_image(rgba_image.as_raw(), rgba_image.width(), rgba_image.height(), ExtendedColorType::Rgba8)
        .map_err(|e| Report::new(TextureError::PngEncode(e.to_string())))?;

    Ok(png_buf)
}

/// Bake a tiled camouflage tile texture with color scheme replacement.
///
/// The tile texture is a color-indexed mask where R/G/B/Black zones correspond
/// to color1/color2/color3/color0 from the color scheme. This function replaces
/// each zone with the appropriate color and returns the result as PNG.
pub fn bake_tiled_camo_png(tile_dds_bytes: &[u8], colors: &[[f32; 4]; 4]) -> Result<Vec<u8>, Report<TextureError>> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(tile_dds_bytes))
        .map_err(|e| Report::new(TextureError::DdsParse(e.to_string())))?;

    let mut rgba_image =
        image_dds::image_from_dds(&dds, 0).map_err(|e| Report::new(TextureError::DdsDecode(e.to_string())))?;

    for pixel in rgba_image.pixels_mut() {
        let [r, g, b, _a] = pixel.0;
        // Determine zone by dominant channel. DXT1 compression may blend
        // edge pixels, but dominant-channel detection handles this well.
        let color = if r > g && r > b && r > 30 {
            &colors[1] // Red zone → color1
        } else if g > r && g > b && g > 30 {
            &colors[2] // Green zone → color2
        } else if b > r && b > g && b > 30 {
            &colors[3] // Blue zone → color3
        } else {
            &colors[0] // Black/dark zone → color0
        };
        // Convert linear float [0,1] to sRGB [0,255]
        pixel.0 = [
            (linear_to_srgb(color[0]) * 255.0) as u8,
            (linear_to_srgb(color[1]) * 255.0) as u8,
            (linear_to_srgb(color[2]) * 255.0) as u8,
            (color[3].clamp(0.0, 1.0) * 255.0) as u8,
        ];
    }

    let mut png_buf = Vec::new();
    PngEncoder::new(&mut png_buf)
        .write_image(rgba_image.as_raw(), rgba_image.width(), rgba_image.height(), ExtendedColorType::Rgba8)
        .map_err(|e| Report::new(TextureError::PngEncode(e.to_string())))?;

    Ok(png_buf)
}

/// Convert a linear-space color component to sRGB.
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

const TEXTURE_BASE: &str = "content/gameplay/common/camouflage/textures";

/// Load raw DDS bytes from an absolute VFS path.
pub fn load_dds_from_vfs(vfs: &vfs::VfsPath, path: &str) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut file = vfs.join(path).ok()?.open_file().ok()?;
    std::io::Read::read_to_end(&mut file, &mut data).ok()?;
    if data.is_empty() { None } else { Some(data) }
}

/// MFM name suffixes that don't appear in texture filenames.
///
/// E.g. MFM `AGM034_16in50_Mk7_skinned.mfm` → texture `AGM034_16in50_Mk7_camo_01.dds`.
const MFM_STRIP_SUFFIXES: &[&str] = &["_skinned", "_wire", "_dead", "_blaze", "_alpha"];

/// Derive texture base names from an MFM stem.
///
/// Returns the original stem first, then the stem with known MFM-only suffixes
/// stripped (e.g. `_skinned`). This allows matching both hull-style stems
/// (where `JSB039_Yamato_1945_Hull` IS the texture name) and turret-style stems
/// (where `AGM034_16in50_Mk7_skinned` maps to `AGM034_16in50_Mk7`).
pub fn texture_base_names(mfm_stem: &str) -> Vec<String> {
    let mut names = vec![mfm_stem.to_string()];
    for suffix in MFM_STRIP_SUFFIXES {
        if let Some(stripped) = mfm_stem.strip_suffix(suffix)
            && !names.contains(&stripped.to_string())
        {
            names.push(stripped.to_string());
        }
    }
    names
}

/// Texture channel suffixes that indicate a multi-channel camo scheme.
///
/// When a scheme is discovered as e.g. `GW_a`, the `_a` suffix means it's the albedo
/// channel of scheme `GW`. The `_mg` and `_mgn` suffixes are metallic/gloss channels.
/// These are stripped during discovery to group channels into a single scheme.
const TEXTURE_CHANNEL_SUFFIXES: &[&str] = &["_a", "_mg", "_mgn"];

/// Load the albedo texture for a given MFM stem and camo scheme from the VFS.
///
/// Given an MFM leaf like `JSB039_Yamato_1945_Hull` and scheme like `GW`,
/// tries multiple naming conventions in order:
/// 1. `{stem}_{scheme}_a.dd0/dds` — explicit albedo channel (e.g. `Hull_GW_a.dds`)
/// 2. `{stem}_{scheme}.dd0/dds` — direct replacement (e.g. `Hull_camo_01.dds`)
///
/// Also tries with known MFM suffixes stripped (e.g. `_skinned`) to handle
/// turret models where the texture name differs from the MFM name.
///
/// Returns `(base_name, dds_bytes)` if found, or `None`.
pub fn load_texture_bytes(vfs: &vfs::VfsPath, mfm_stem: &str, scheme: &str) -> Option<(String, Vec<u8>)> {
    for base in texture_base_names(mfm_stem) {
        // Try explicit albedo channel first ({base}_{scheme}_a), then direct ({base}_{scheme}).
        let candidates = [
            format!("{TEXTURE_BASE}/{base}_{scheme}_a.dd0"),
            format!("{TEXTURE_BASE}/{base}_{scheme}_a.dds"),
            format!("{TEXTURE_BASE}/{base}_{scheme}.dd0"),
            format!("{TEXTURE_BASE}/{base}_{scheme}.dds"),
        ];

        for path in &candidates {
            if let Ok(vfs_path) = vfs.join(path)
                && let Ok(mut file) = vfs_path.open_file()
            {
                let mut data = Vec::new();
                if std::io::Read::read_to_end(&mut file, &mut data).is_ok() && !data.is_empty() {
                    return Some((base, data));
                }
            }
        }
    }

    None
}

/// Load the base albedo texture for a hull mesh from the VFS.
///
/// The base albedo is the "default" ship appearance — gray/weathered paint without
/// any camouflage applied. Textures live in a `textures/` sibling directory next to
/// the ship folder, e.g.:
/// `content/gameplay/japan/ship/battleship/textures/JSB039_Yamato_1945_Hull_a.dd0`
///
/// Prefers `.dd0` (highest resolution, typically 4096x4096) over `.dds` (low-res
/// 512x512 mip tail). Falls back to searching the MFM's own directory.
///
/// `mfm_full_path` is the full VFS path to the MFM file (e.g. ending in `.mfm`).
/// Returns DDS bytes if found.
pub fn load_base_albedo_bytes(vfs: &vfs::VfsPath, mfm_full_path: &str) -> Option<Vec<u8>> {
    let dir = mfm_full_path.rsplit_once('/')?.0;
    let mfm_filename = mfm_full_path.rsplit_once('/')?.1;
    let stem = mfm_filename.strip_suffix(".mfm")?;

    // The textures/ sibling directory: go up from the ship dir to the species dir,
    // then into textures/. E.g. .../cruiser/JSC010_Mogami_1944/ -> .../cruiser/textures/
    let tex_sibling_dir = dir.rsplit_once('/').map(|(parent, _)| format!("{parent}/textures"));

    // Albedo suffix priority: `_a` (standard PBS), `_od` (TILEDLAND overlay diffuse).
    let albedo_suffixes = ["_a", "_od"];

    // Search directories: textures/ sibling, MFM's dir, and TILED/ subdirectory
    // (underwater TILEDLAND materials store textures in a TILED/ subdirectory).
    let tiled_subdir = format!("{dir}/TILED");

    for base in texture_base_names(stem) {
        // Build candidate paths: prefer dd0 (high-res) over dds (low-res mip tail).
        let mut candidates = Vec::new();
        for suffix in &albedo_suffixes {
            if let Some(tex_dir) = &tex_sibling_dir {
                candidates.push(format!("{tex_dir}/{base}{suffix}.dd0"));
                candidates.push(format!("{tex_dir}/{base}{suffix}.dds"));
            }
            candidates.push(format!("{dir}/{base}{suffix}.dd0"));
            candidates.push(format!("{dir}/{base}{suffix}.dds"));
            candidates.push(format!("{tiled_subdir}/{base}{suffix}.dd0"));
            candidates.push(format!("{tiled_subdir}/{base}{suffix}.dds"));
        }

        for path in &candidates {
            if let Ok(vfs_path) = vfs.join(path)
                && let Ok(mut file) = vfs_path.open_file()
            {
                let mut data = Vec::new();
                if std::io::Read::read_to_end(&mut file, &mut data).is_ok() && !data.is_empty() {
                    return Some(data);
                }
            }
        }
    }

    None
}

/// Strip texture channel suffixes (`_a`, `_mg`, `_mgn`) from a raw scheme name.
///
/// E.g. `GW_a` → `GW`, `camo_01` → `camo_01` (no channel suffix).
fn strip_channel_suffix(scheme: &str) -> &str {
    for suffix in TEXTURE_CHANNEL_SUFFIXES {
        if let Some(stripped) = scheme.strip_suffix(suffix)
            && !stripped.is_empty()
        {
            return stripped;
        }
    }
    scheme
}

/// Discover available texture schemes for a set of MFM stems by scanning the VFS.
///
/// Multi-channel schemes (e.g. `GW_a` + `GW_mg`) are grouped into a single scheme
/// name (`GW`). Returns sorted, deduplicated scheme names.
pub fn discover_texture_schemes(vfs: &vfs::VfsPath, mfm_stems: &[String]) -> Vec<String> {
    let mut schemes = std::collections::BTreeSet::new();

    let Ok(tex_dir) = vfs.join(TEXTURE_BASE) else {
        return Vec::new();
    };
    let Ok(entries) = tex_dir.read_dir() else {
        return Vec::new();
    };

    // Collect filenames ending in .dds (base mip level — avoids counting .dd0/.dd1/.dd2 dupes).
    let dds_names: Vec<String> = entries
        .filter_map(|entry| {
            let name = entry.filename();
            if name.ends_with(".dds") { Some(name) } else { None }
        })
        .collect();

    for stem in mfm_stems {
        for base in texture_base_names(stem) {
            let prefix = format!("{base}_");
            for name in &dds_names {
                if let Some(rest) = name.strip_prefix(&prefix)
                    && let Some(raw_scheme) = rest.strip_suffix(".dds")
                    && !raw_scheme.is_empty()
                {
                    let scheme = strip_channel_suffix(raw_scheme);
                    schemes.insert(scheme.to_string());
                }
            }
        }
    }

    schemes.into_iter().collect()
}

// ---------------------------------------------------------------------------
// TILEDLAND terrain texture baking
// ---------------------------------------------------------------------------

use std::collections::HashMap;

use image_dds::SurfaceRgba8;
use image_dds::image::RgbaImage;

use crate::models::assets_bin::PrototypeDatabase;
use crate::models::material::MaterialPrototype;
use crate::models::material::{
    self,
};

/// Resolve a texture selfId hash from an MFM property to a VFS path,
/// load the DDS bytes, and return them.
fn load_texture_by_hash(
    vfs: &vfs::VfsPath,
    db: &PrototypeDatabase<'_>,
    self_id_index: &HashMap<u64, usize>,
    texture_hash: u64,
) -> Option<Vec<u8>> {
    let &path_idx = self_id_index.get(&texture_hash)?;
    let full_path = db.reconstruct_path(path_idx, self_id_index);

    // Try .dd0 (high-res) first, then the path as-is (.dds).
    let dd0_path = if full_path.ends_with(".dds") { Some(full_path.replace(".dds", ".dd0")) } else { None };

    for path in dd0_path.iter().chain(std::iter::once(&full_path)) {
        if let Some(data) = load_dds_from_vfs(vfs, path) {
            return Some(data);
        }
    }
    None
}

/// Parse an MFM material from assets.bin given its selfId (material_mfm_path_id).
///
/// Returns the parsed material if the MFM is found and parses successfully.
pub fn parse_mfm_from_db(db: &PrototypeDatabase<'_>, mfm_path_id: u64) -> Option<MaterialPrototype> {
    let r2p_value = db.lookup_r2p(mfm_path_id)?;
    let location = db.decode_r2p_value(r2p_value).ok()?;
    if location.blob_index != material::MATERIAL_BLOB_INDEX {
        return None;
    }
    let record_data = db.get_prototype_data(location, material::MATERIAL_ITEM_SIZE).ok()?;
    material::parse_material(record_data).ok()
}

/// Check if a material is a TILEDLAND terrain material.
///
/// TILEDLAND materials have `AHArray` (tile atlas), `blendMap`, and `g_tilesIndex`.
pub fn is_tiledland_material(mat: &MaterialPrototype) -> bool {
    mat.get_texture_hash("AHArray").is_some()
        && mat.get_texture_hash("blendMap").is_some()
        && mat.get_vec4("g_tilesIndex").is_some()
}

/// Bake a TILEDLAND terrain albedo texture from MFM material properties.
///
/// The TILEDLAND shader composites 4 tile layers from a shared atlas texture,
/// weighted by the RGBA channels of a blend map. Parameters:
/// - `AHArray`: texture array atlas (Albedo/Height), each layer is a tile material
/// - `blendMap`: per-pixel RGBA blend weights selecting which atlas layers to use
/// - `g_tilesIndex`: vec4 of 4 atlas layer indices (one per blend channel)
/// - `g_tilesScale`: float UV tiling scale for atlas sampling
///
/// Note: ODMap is intentionally NOT applied — it requires g_overlayOpacity/g_overlayDepth
/// shader parameters for correct blending, and naive multiplication darkens the result.
///
/// Returns PNG bytes of the baked albedo texture at the blend map's resolution.
pub fn bake_tiledland_albedo(
    mat: &MaterialPrototype,
    vfs: &vfs::VfsPath,
    db: &PrototypeDatabase<'_>,
    self_id_index: &HashMap<u64, usize>,
    max_size: Option<u32>,
) -> Option<Vec<u8>> {
    // Extract material properties.
    let ah_hash = mat.get_texture_hash("AHArray")?;
    let blend_hash = mat.get_texture_hash("blendMap")?;
    let tiles_index = mat.get_vec4("g_tilesIndex")?;
    let tiles_scale = mat.get_float("g_tilesScale").unwrap_or(16.0);

    // Optional sheen tint color — the TILEDLAND shader uses this to add vegetation
    // coloring (e.g. green tint) on top of the otherwise earth-tone atlas tiles.
    let sheen_tint = mat.get_vec4("addSheenTintColor");
    let sheen_amount = mat.get_float("sheen").unwrap_or(0.0);

    // Load and decode the tile atlas (array texture).
    let ah_dds_bytes = load_texture_by_hash(vfs, db, self_id_index, ah_hash)?;
    let ah_dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(&ah_dds_bytes)).ok()?;
    let num_layers = ah_dds.get_num_array_layers().max(1);
    // Decode only mip 0 of all layers.
    let ah_surface = SurfaceRgba8::decode_layers_mipmaps_dds(&ah_dds, 0..num_layers, 0..1).ok()?;

    // Extract the 4 tile layers we need.
    let layer_indices: [u32; 4] =
        [tiles_index[0] as u32, tiles_index[1] as u32, tiles_index[2] as u32, tiles_index[3] as u32];
    let tile_w = ah_surface.width;
    let tile_h = ah_surface.height;

    let tile_layers: Vec<Option<RgbaImage>> =
        layer_indices.iter().map(|&idx| ah_surface.get_image(idx, 0, 0)).collect();

    // Load and decode the blend map.
    let blend_dds_bytes = load_texture_by_hash(vfs, db, self_id_index, blend_hash)?;
    let blend_img = {
        let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(&blend_dds_bytes)).ok()?;
        image_dds::image_from_dds(&dds, 0).ok()?
    };
    let blend_w = blend_img.width();
    let blend_h = blend_img.height();

    // Determine output size: use blend map resolution (typically 512-1024).
    let out_w = blend_w;
    let out_h = blend_h;

    // Bake: for each output pixel, sample blend weights and composite tile layers.
    let mut output = RgbaImage::new(out_w, out_h);

    for py in 0..out_h {
        for px in 0..out_w {
            let blend_pixel = blend_img.get_pixel(px, py);
            let weights = [
                blend_pixel[0] as f32 / 255.0, // R → layer 0
                blend_pixel[1] as f32 / 255.0, // G → layer 1
                blend_pixel[2] as f32 / 255.0, // B → layer 2
                blend_pixel[3] as f32 / 255.0, // A → layer 3
            ];

            // Normalize weights so they sum to 1. If all zero, use equal weights.
            let sum: f32 = weights.iter().sum();
            let norm = if sum > 0.001 {
                [weights[0] / sum, weights[1] / sum, weights[2] / sum, weights[3] / sum]
            } else {
                [0.25, 0.25, 0.25, 0.25]
            };

            // UV in blend map space [0..1], then tile with g_tilesScale.
            let u = px as f32 / out_w as f32;
            let v = py as f32 / out_h as f32;
            let tile_u = (u * tiles_scale).fract();
            let tile_v = (v * tiles_scale).fract();

            // Sample each tile layer and blend.
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;

            for (i, layer_img) in tile_layers.iter().enumerate() {
                if norm[i] < 0.001 {
                    continue;
                }
                if let Some(img) = layer_img {
                    let tx = ((tile_u * tile_w as f32) as u32).min(tile_w - 1);
                    let ty = ((tile_v * tile_h as f32) as u32).min(tile_h - 1);
                    let p = img.get_pixel(tx, ty);
                    r += p[0] as f32 * norm[i];
                    g += p[1] as f32 * norm[i];
                    b += p[2] as f32 * norm[i];
                }
            }

            // Apply addSheenTintColor — the TILEDLAND shader uses this to add
            // vegetation coloring (green tint) on top of the earth-tone atlas tiles.
            // We lerp toward the tint color by the sheen amount.
            if let Some(tint) = sheen_tint
                && sheen_amount > 0.0
            {
                let t = sheen_amount;
                r = r * (1.0 - t) + (tint[0] * 255.0) * t;
                g = g * (1.0 - t) + (tint[1] * 255.0) * t;
                b = b * (1.0 - t) + (tint[2] * 255.0) * t;
            }

            output.put_pixel(
                px,
                py,
                image_dds::image::Rgba([
                    r.clamp(0.0, 255.0) as u8,
                    g.clamp(0.0, 255.0) as u8,
                    b.clamp(0.0, 255.0) as u8,
                    255,
                ]),
            );
        }
    }

    // Downsample if needed.
    let (final_w, final_h, pixels) = if let Some(max) = max_size
        && (out_w > max || out_h > max)
    {
        let scale = (max as f32 / out_w as f32).min(max as f32 / out_h as f32);
        let nw = ((out_w as f32 * scale) as u32).max(1);
        let nh = ((out_h as f32 * scale) as u32).max(1);
        let src = output.as_raw();
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        for dy in 0..nh {
            let sy0 = (dy as f64 * out_h as f64 / nh as f64) as u32;
            let sy1 = (((dy + 1) as f64 * out_h as f64 / nh as f64) as u32).min(out_h);
            for dx in 0..nw {
                let sx0 = (dx as f64 * out_w as f64 / nw as f64) as u32;
                let sx1 = (((dx + 1) as f64 * out_w as f64 / nw as f64) as u32).min(out_w);
                let mut ra = 0u32;
                let mut ga = 0u32;
                let mut ba = 0u32;
                let mut count = 0u32;
                for sy in sy0..sy1 {
                    for sx in sx0..sx1 {
                        let i = (sy * out_w + sx) as usize * 4;
                        ra += src[i] as u32;
                        ga += src[i + 1] as u32;
                        ba += src[i + 2] as u32;
                        count += 1;
                    }
                }
                if count > 0 {
                    let di = (dy * nw + dx) as usize * 4;
                    dst[di] = (ra / count) as u8;
                    dst[di + 1] = (ga / count) as u8;
                    dst[di + 2] = (ba / count) as u8;
                    dst[di + 3] = 255;
                }
            }
        }
        (nw, nh, dst)
    } else {
        (out_w, out_h, output.into_raw())
    };

    // Encode to PNG.
    let mut png_buf = Vec::new();
    PngEncoder::new(&mut png_buf).write_image(&pixels, final_w, final_h, ExtendedColorType::Rgba8).ok()?;

    Some(png_buf)
}

/// Try to load a texture for a model mesh, with TILEDLAND baking support.
///
/// If assets.bin is available, first parses the MFM to check if it's a TILEDLAND
/// terrain material. If so, bakes a composite albedo from the tile atlas + blend
/// map (the correct rendering). Otherwise falls back to simple filename-based
/// texture lookup via `load_base_albedo_bytes`.
///
/// Returns PNG bytes if successful.
pub fn load_or_bake_albedo(
    vfs: &vfs::VfsPath,
    mfm_full_path: &str,
    mfm_path_id: u64,
    db: Option<&PrototypeDatabase<'_>>,
    self_id_index: Option<&HashMap<u64, usize>>,
    max_size: Option<u32>,
) -> Option<Vec<u8>> {
    // Try MFM-based TILEDLAND baking first (terrain materials).
    // This must come before filename-based lookup because _od files exist for
    // TILEDLAND tiles but are overlay maps, not standalone albedo textures.
    if let Some(db) = db
        && let Some(idx) = self_id_index
        && mfm_path_id != 0
        && let Some(mat) = parse_mfm_from_db(db, mfm_path_id)
        && is_tiledland_material(&mat)
    {
        eprintln!("  Baking TILEDLAND texture for: {mfm_full_path}");
        if let Some(png) = bake_tiledland_albedo(&mat, vfs, db, idx, max_size) {
            return Some(png);
        }
        eprintln!("    Warning: TILEDLAND bake failed, falling back to filename lookup");
    }

    // Fall back to simple filename-based lookup (works for standard PBS materials).
    // Force alpha=255 since model albedo textures often store non-opacity data
    // (height, roughness) in the alpha channel which would cause unwanted transparency.
    let dds_bytes = load_base_albedo_bytes(vfs, mfm_full_path)?;
    let mut png = dds_to_png_resized(&dds_bytes, max_size).ok()?;
    force_png_opaque(&mut png);
    Some(png)
}

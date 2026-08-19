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

/// Longest-edge pixel budget for a decoded texture.
///
/// Zero is not representable: a budget of no pixels describes no image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MaxEdge(u32);

impl MaxEdge {
    pub fn new(pixels: u32) -> Option<Self> {
        (pixels > 0).then_some(Self(pixels))
    }

    pub fn pixels(self) -> u32 {
        self.0
    }

    /// Whether an image of `width` by `height` already fits the budget.
    fn fits(self, width: u32, height: u32) -> bool {
        width.max(height) <= self.0
    }
}

/// Requested detail level for a game texture.
///
/// WoWs stores a texture as a ladder of successive halvings: a `.dds` file
/// holding the mip chain from 512 down, plus up to three single-mip files above
/// it (`.dd2`, `.dd1`, `.dd0`), each double the one below. Only the tiers a
/// texture needs exist, and the ones present always form a contiguous chain down
/// to the `.dds` top mip, so `.dd0` is 2x, 4x, or 8x that mip depending on how
/// many tiers there are.
///
/// [`Full`](Self::Full) takes the largest tier. [`Capped`](Self::Capped) takes
/// the largest tier and stored mip that stay within budget, so a small budget
/// skips the multi-megabyte read outright instead of decoding 4096 pixels and
/// scaling the result down. [`Tiers`](Self::Tiers) steps down that authored
/// ladder by rank rather than by pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureLod {
    #[default]
    Full,
    Capped(MaxEdge),
    Tiers(TierDrop),
}

impl TextureLod {
    /// Build from a longest-edge pixel count. `None`, and any count that is not
    /// a valid [`MaxEdge`], mean full detail.
    pub fn from_max_edge(pixels: Option<u32>) -> Self {
        match pixels.and_then(MaxEdge::new) {
            Some(edge) => Self::Capped(edge),
            None => Self::Full,
        }
    }

    /// Match a mesh LOD index by rank: mesh LOD `k` takes the `k`-th texture
    /// tier down. Both ladders are authored by the game, so equal ranks pair
    /// the geometry detail with the texture detail shipped alongside it.
    ///
    /// Pixel sizes cannot do this job: the same rank is 4096 on a hull atlas and
    /// 512 on a small fitting, and a texture only has the tiers it was given.
    pub fn from_mesh_lod(lod: usize) -> Self {
        match TierDrop::new(lod) {
            Some(drop) => Self::Tiers(drop),
            None => Self::Full,
        }
    }

    fn budget(self) -> Option<MaxEdge> {
        match self {
            Self::Full | Self::Tiers(_) => None,
            Self::Capped(edge) => Some(edge),
        }
    }
}

/// How many tiers to step down a texture's authored ladder.
///
/// Zero is not representable: dropping no tiers is [`TextureLod::Full`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TierDrop(usize);

impl TierDrop {
    pub fn new(tiers: usize) -> Option<Self> {
        (tiers > 0).then_some(Self(tiers))
    }

    pub fn tiers(self) -> usize {
        self.0
    }
}

/// Single-mip tier suffixes stacked above the `.dds` mip tail, largest first.
const DDS_TIER_SUFFIXES: [&str; 3] = ["dd0", "dd1", "dd2"];

/// Read a whole file from the VFS, treating an empty file as absent.
fn read_vfs_file(vfs: &vfs::VfsPath, path: &str) -> Option<Vec<u8>> {
    let vfs_path = vfs.join(path).ok()?;
    let mut file = vfs_path.open_file().ok()?;
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut data).ok()?;
    (!data.is_empty()).then_some(data)
}

fn vfs_file_exists(vfs: &vfs::VfsPath, path: &str) -> bool {
    vfs.join(path).and_then(|p| p.exists()).unwrap_or(false)
}

/// Top-mip dimensions of a DDS, without decoding any pixels.
fn dds_top_dims(dds_bytes: &[u8]) -> Option<(u32, u32)> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(dds_bytes)).ok()?;
    Some((dds.get_width(), dds.get_height()))
}

/// Top-mip longest edge of a DDS buffer, without decoding any pixels.
fn dds_top_edge(dds_bytes: &[u8]) -> Option<u32> {
    let (w, h) = dds_top_dims(dds_bytes)?;
    Some(w.max(h))
}

/// The authored resolution ladder for a `.dds` path, largest tier first.
///
/// Reads only the tail (the cheapest file) plus one existence check per tier:
/// tiers double going up, so the tail's top mip sizes the whole ladder with no
/// decode and no read of the large files.
pub fn texture_ladder(vfs: &vfs::VfsPath, path: &str) -> Option<Vec<(u32, u32)>> {
    let stem = path.strip_suffix(".dds")?;
    let tail = read_vfs_file(vfs, path)?;
    let (tw, th) = dds_top_dims(&tail)?;
    let present = DDS_TIER_SUFFIXES.into_iter().filter(|s| vfs_file_exists(vfs, &format!("{stem}.{s}"))).count();

    let mut tiers = Vec::with_capacity(present + 1);
    for doublings in (1..=present as u32).rev() {
        tiers.push((tw.checked_shl(doublings).unwrap_or(u32::MAX), th.checked_shl(doublings).unwrap_or(u32::MAX)));
    }
    tiers.push((tw, th));
    Some(tiers)
}

/// Read the highest-resolution tier available for `stem`, falling back down the
/// ladder to the `.dds` tail.
fn read_highest_tier(vfs: &vfs::VfsPath, stem: &str, tail_path: &str) -> Option<Vec<u8>> {
    for suffix in DDS_TIER_SUFFIXES {
        if let Some(data) = read_vfs_file(vfs, &format!("{stem}.{suffix}")) {
            return Some(data);
        }
    }
    read_vfs_file(vfs, tail_path)
}

/// Read the tier `drop` steps below the top of this texture's own ladder.
///
/// The ladder is the tiers the texture actually has, largest first, ending at
/// the `.dds` tail. A drop past the end clamps to the tail, which is the lowest
/// separately authored level; going below it is a pixel budget's job, not a
/// rank's. Existence checks size the ladder, so exactly one file is read.
fn read_tier_below_top(vfs: &vfs::VfsPath, stem: &str, tail_path: &str, drop: TierDrop) -> Option<Vec<u8>> {
    let present: Vec<&str> =
        DDS_TIER_SUFFIXES.into_iter().filter(|s| vfs_file_exists(vfs, &format!("{stem}.{s}"))).collect();

    // Walk down from the requested rank so a tier that fails to read falls back
    // to the next smaller one rather than aborting the load.
    for suffix in present.into_iter().skip(drop.tiers()) {
        if let Some(data) = read_vfs_file(vfs, &format!("{stem}.{suffix}")) {
            return Some(data);
        }
    }
    read_vfs_file(vfs, tail_path)
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

/// Box-filter `image` down so that neither dimension exceeds `edge`.
fn downsample(image: &RgbaImage, edge: MaxEdge) -> RgbaImage {
    let (w, h) = (image.width(), image.height());
    let max = edge.pixels();
    let scale = (max as f32 / w as f32).min(max as f32 / h as f32);
    let nw = ((w as f32 * scale) as u32).max(1);
    let nh = ((h as f32 * scale) as u32).max(1);

    let src = image.as_raw();
    let mut dst = vec![0u8; (nw as usize) * (nh as usize) * 4];
    // Average the source pixels covered by each destination pixel.
    for dy in 0..nh {
        let sy0 = (dy as f64 * h as f64 / nh as f64) as u32;
        let sy1 = (((dy + 1) as f64 * h as f64 / nh as f64) as u32).min(h);
        for dx in 0..nw {
            let sx0 = (dx as f64 * w as f64 / nw as f64) as u32;
            let sx1 = (((dx + 1) as f64 * w as f64 / nw as f64) as u32).min(w);
            let mut acc = [0u32; 4];
            let mut count = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let i = (sy * w + sx) as usize * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += src[i + c] as u32;
                    }
                    count += 1;
                }
            }
            if count > 0 {
                let di = (dy * nw + dx) as usize * 4;
                for (c, a) in acc.iter().enumerate() {
                    dst[di + c] = (a / count) as u8;
                }
            }
        }
    }

    RgbaImage::from_raw(nw, nh, dst).unwrap_or_else(|| RgbaImage::new(nw, nh))
}

/// Largest stored mip level of `dds` that fits `lod`'s budget.
///
/// Returns the smallest stored mip when even that exceeds the budget; the caller
/// box-filters the remainder.
fn best_mip(dds: &image_dds::ddsfile::Dds, lod: TextureLod) -> u32 {
    let Some(edge) = lod.budget() else {
        return 0;
    };
    let levels = dds.get_num_mipmap_levels().max(1);
    let (mut w, mut h) = (dds.get_width().max(1), dds.get_height().max(1));
    for mip in 0..levels {
        if edge.fits(w, h) {
            return mip;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    levels - 1
}

/// Decode DDS bytes to an RGBA8 image at no more than `lod`'s budget.
///
/// Prefers a stored mip over box filtering, so a texture whose own chain can
/// serve the request never pays for a full-size decode.
fn decode_dds(dds_bytes: &[u8], lod: TextureLod) -> Result<RgbaImage, Report<TextureError>> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(dds_bytes))
        .map_err(|e| Report::new(TextureError::DdsParse(e.to_string())))?;

    let image = image_dds::image_from_dds(&dds, best_mip(&dds, lod))
        .map_err(|e| Report::new(TextureError::DdsDecode(e.to_string())))?;

    Ok(match lod.budget() {
        Some(edge) if !edge.fits(image.width(), image.height()) => downsample(&image, edge),
        _ => image,
    })
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, Report<TextureError>> {
    let mut png_buf = Vec::new();
    PngEncoder::new(&mut png_buf)
        .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
        .map_err(|e| Report::new(TextureError::PngEncode(e.to_string())))?;
    Ok(png_buf)
}

/// Decode DDS bytes to PNG bytes (RGBA8) at no more than `lod`'s budget.
pub fn dds_to_png(dds_bytes: &[u8], lod: TextureLod) -> Result<Vec<u8>, Report<TextureError>> {
    encode_png(&decode_dds(dds_bytes, lod)?)
}

/// Bake a color-indexed camouflage mask into RGBA using a color scheme.
///
/// The mask's R/G/B zones map to color1/color2/color3. The black zone maps to color0, but its
/// meaning depends on `black_passthrough`:
/// - A per-ship painted mask (the camo maps 1:1 to the ship's UV atlas, `tiled=false`) leaves the
///   parts it does not paint black to mean "no camo here". Those texels pass through to the base
///   albedo (keeping the red anti-fouling below the waterline and the stock detail in unpainted
///   areas), so they are baked transparent (alpha 0) and the caller composites over the base. The
///   RGB is left at color0 so DXT edge blends fringe with the camo's own dark tone, not black.
/// - A repeating tile (`tiled=true`, e.g. an ERDL/dazzle pattern) uses black as a real pattern
///   color: the whole ship is covered, so black bakes to opaque color0 with no passthrough.
pub fn bake_tiled_camo_png(
    tile_dds_bytes: &[u8],
    colors: &[[f32; 4]; 4],
    black_passthrough: bool,
    lod: TextureLod,
) -> Result<Vec<u8>, Report<TextureError>> {
    let mut rgba_image = decode_dds(tile_dds_bytes, lod)?;

    let black_alpha = if black_passthrough { 0 } else { 255 };
    for pixel in rgba_image.pixels_mut() {
        let [r, g, b, _a] = pixel.0;
        // Determine zone by dominant channel. DXT1 compression may blend
        // edge pixels, but dominant-channel detection handles this well.
        let (color, alpha) = if r > g && r > b && r > 30 {
            (&colors[1], 255) // Red zone → color1
        } else if g > r && g > b && g > 30 {
            (&colors[2], 255) // Green zone → color2
        } else if b > r && b > g && b > 30 {
            (&colors[3], 255) // Blue zone → color3
        } else {
            (&colors[0], black_alpha) // Black zone → color0 (opaque tile) or passthrough (per-ship mask)
        };
        // Convert linear float [0,1] to sRGB [0,255]
        pixel.0 = [
            (linear_to_srgb(color[0]) * 255.0) as u8,
            (linear_to_srgb(color[1]) * 255.0) as u8,
            (linear_to_srgb(color[2]) * 255.0) as u8,
            alpha,
        ];
    }

    encode_png(&rgba_image)
}

/// Fraction of pixels that look like a colorizable zone mask: red/green/blue-dominant
/// (dominant channel > 30, the same dominance conditions as `bake_tiled_camo_png`) or
/// near-black (all channels <= 30). A high fraction (>= 0.9) means the texture is a zone
/// mask to colorize with a color scheme; a low fraction means a real painted albedo that
/// must render raw.
///
/// Judges the top mip of whatever tier the caller loaded. Compression blends zone
/// edges, so a lower tier scores marginally lower; a mask near the threshold can
/// therefore classify differently across [`TextureLod`] settings.
pub fn zone_mask_fraction(dds_bytes: &[u8]) -> Option<f32> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(dds_bytes)).ok()?;
    let img = image_dds::image_from_dds(&dds, 0).ok()?;
    let mut total = 0usize;
    let mut zone = 0usize;
    for pixel in img.pixels() {
        let [r, g, b, _a] = pixel.0;
        total += 1;
        let is_zone = (r > g && r > b && r > 30)
            || (g > r && g > b && g > 30)
            || (b > r && b > g && b > 30)
            || (r <= 30 && g <= 30 && b <= 30);
        if is_zone {
            zone += 1;
        }
    }
    if total == 0 {
        return Some(0.0);
    }
    Some(zone as f32 / total as f32)
}

/// Convert a linear-space color component to sRGB.
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

const TEXTURE_BASE: &str = "content/gameplay/common/camouflage/textures";

/// Load raw DDS bytes for a `.dds` path, choosing the tier `lod` asks for.
///
/// A capped request reads the `.dds` tail first: it is the cheapest file and its
/// top mip anchors the ladder, so the size of every tier above follows from how
/// many of them exist. That lets the largest tier within budget be read directly,
/// with no speculative reads of the tiers above it.
pub fn load_dds_from_vfs(vfs: &vfs::VfsPath, path: &str, lod: TextureLod) -> Option<Vec<u8>> {
    let Some(stem) = path.strip_suffix(".dds") else {
        return read_vfs_file(vfs, path);
    };

    if let TextureLod::Tiers(drop) = lod {
        return read_tier_below_top(vfs, stem, path, drop);
    }

    let Some(budget) = lod.budget() else {
        return read_highest_tier(vfs, stem, path);
    };

    // Without the tail there is nothing to size the ladder against.
    let Some(tail) = read_vfs_file(vfs, path) else {
        return read_highest_tier(vfs, stem, path);
    };
    let Some(tail_edge) = dds_top_edge(&tail) else {
        return Some(tail);
    };
    if budget.pixels() <= tail_edge {
        // The tail's own chain covers the request; the decode picks the mip.
        return Some(tail);
    }

    let present: Vec<&str> =
        DDS_TIER_SUFFIXES.into_iter().filter(|s| vfs_file_exists(vfs, &format!("{stem}.{s}"))).collect();

    // Tiers double going up, so the i-th of `n` present tiers (largest first)
    // sits `n - i` doublings above the tail. Largest first means the first tier
    // that fits the budget is the best one that does.
    for (i, suffix) in present.iter().enumerate() {
        let doublings = (present.len() - i) as u32;
        let edge = tail_edge.checked_shl(doublings).unwrap_or(u32::MAX);
        if edge <= budget.pixels()
            && let Some(data) = read_vfs_file(vfs, &format!("{stem}.{suffix}"))
        {
            return Some(data);
        }
    }

    Some(tail)
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
pub fn load_texture_bytes(
    vfs: &vfs::VfsPath,
    mfm_stem: &str,
    scheme: &str,
    lod: TextureLod,
) -> Option<(String, Vec<u8>)> {
    // Tries every candidate for real: a resolved path that merely *exists* can
    // still fail to load (e.g. a zero-length placeholder shadowing a later,
    // real candidate), and the loader must not stop there.
    for (base, path) in scheme_texture_candidates(mfm_stem, scheme) {
        if let Some(data) = load_dds_from_vfs(vfs, &path, lod) {
            return Some((base, data));
        }
    }
    None
}

/// Ordered `(base_name, path)` candidates a scheme's texture might resolve to
/// for `mfm_stem`: explicit albedo channel first, then direct replacement, for
/// every name [`texture_base_names`] yields. Shared by [`load_texture_bytes`]
/// and [`scheme_texture_path`] so the two never drift on candidate order.
fn scheme_texture_candidates(mfm_stem: &str, scheme: &str) -> Vec<(String, String)> {
    let mut candidates = Vec::new();
    for base in texture_base_names(mfm_stem) {
        // Try explicit albedo channel first ({base}_{scheme}_a), then direct ({base}_{scheme}).
        let albedo = format!("{TEXTURE_BASE}/{base}_{scheme}_a.dds");
        let direct = format!("{TEXTURE_BASE}/{base}_{scheme}.dds");
        candidates.push((base.clone(), albedo));
        candidates.push((base, direct));
    }
    candidates
}

/// The DDS path a scheme's texture resolves to for `mfm_stem`, without reading
/// it. Pricing an export must not pay for a decode, so this is a cheap
/// existence check rather than [`load_texture_bytes`]'s "did it actually load"
/// — a same-named zero-length file could in principle shadow a later, real
/// candidate here and price it as absent, which is an acceptable miss for a
/// best-effort price (see `size_model`'s "contributes zero" policy) but must
/// never happen in [`load_texture_bytes`], which does not share this shortcut.
pub fn scheme_texture_path(vfs: &vfs::VfsPath, mfm_stem: &str, scheme: &str) -> Option<String> {
    scheme_texture_candidates(mfm_stem, scheme)
        .into_iter()
        .find(|(_, path)| vfs_file_exists(vfs, path))
        .map(|(_, path)| path)
}

/// Load the base albedo texture for a hull mesh from the VFS.
///
/// The base albedo is the "default" ship appearance — gray/weathered paint without
/// any camouflage applied. Textures live in a `textures/` sibling directory next to
/// the ship folder, e.g.:
/// `content/gameplay/japan/ship/battleship/textures/JSB039_Yamato_1945_Hull_a.dd0`
///
/// The tier within each candidate is chosen by `lod`. Falls back to searching the
/// MFM's own directory.
///
/// `mfm_full_path` is the full VFS path to the MFM file (e.g. ending in `.mfm`).
/// Returns DDS bytes if found.
pub fn load_base_albedo_bytes(vfs: &vfs::VfsPath, mfm_full_path: &str, lod: TextureLod) -> Option<Vec<u8>> {
    // Tries every candidate for real: a resolved path that merely *exists* can
    // still fail to load (e.g. a zero-length placeholder shadowing a later,
    // real candidate), and the loader must not stop there.
    for path in base_albedo_candidates(mfm_full_path)? {
        if let Some(data) = load_dds_from_vfs(vfs, &path, lod) {
            return Some(data);
        }
    }
    None
}

/// Ordered candidate paths for a hull mesh's base albedo: the `textures/`
/// sibling directory, the MFM's own directory, and its `TILED/` subdirectory,
/// each tried with the `_a` then `_od` albedo suffix, for every name
/// [`texture_base_names`] yields. Shared by [`load_base_albedo_bytes`] and
/// [`base_albedo_path`] so the two never drift on candidate order.
///
/// `mfm_full_path` is the full VFS path to the MFM file (e.g. ending in `.mfm`).
fn base_albedo_candidates(mfm_full_path: &str) -> Option<Vec<String>> {
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

    let mut candidates = Vec::new();
    for base in texture_base_names(stem) {
        for suffix in &albedo_suffixes {
            if let Some(tex_dir) = &tex_sibling_dir {
                candidates.push(format!("{tex_dir}/{base}{suffix}.dds"));
            }
            candidates.push(format!("{dir}/{base}{suffix}.dds"));
            candidates.push(format!("{tiled_subdir}/{base}{suffix}.dds"));
        }
    }
    Some(candidates)
}

/// The base albedo DDS path for a hull mesh's MFM, without reading it.
/// Pricing an export must not pay for a decode, so this is a cheap existence
/// check rather than [`load_base_albedo_bytes`]'s "did it actually load" — a
/// same-named zero-length file could in principle shadow a later, real
/// candidate here and price it as absent, which is an acceptable miss for a
/// best-effort price (see `size_model`'s "contributes zero" policy) but must
/// never happen in [`load_base_albedo_bytes`], which does not share this
/// shortcut.
///
/// `mfm_full_path` is the full VFS path to the MFM file (e.g. ending in `.mfm`).
pub fn base_albedo_path(vfs: &vfs::VfsPath, mfm_full_path: &str) -> Option<String> {
    base_albedo_candidates(mfm_full_path)?.into_iter().find(|path| vfs_file_exists(vfs, path))
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
pub fn discover_texture_schemes(
    vfs: &vfs::VfsPath,
    mfm_stems: &[String],
    exclude_paths: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut schemes = std::collections::BTreeSet::new();

    let Ok(tex_dir) = vfs.join(TEXTURE_BASE) else {
        return Vec::new();
    };
    let Ok(entries) = tex_dir.read_dir() else {
        return Vec::new();
    };

    // Collect filenames ending in .dds (base mip level - avoids counting .dd0/.dd1/.dd2 dupes).
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
                    // Skip files that are camouflages.xml zone masks; the mat-camo path
                    // surfaces those correctly (colorized), so a raw copy here is a wrong duplicate.
                    // Check the albedo channel variant too, since the scheme name is channel-stripped
                    // but camouflages.xml references the `_a` file directly.
                    let base_path = format!("{TEXTURE_BASE}/{base}_{scheme}.dds");
                    let albedo_path = format!("{TEXTURE_BASE}/{base}_{scheme}_a.dds");
                    if exclude_paths.contains(&base_path) || exclude_paths.contains(&albedo_path) {
                        continue;
                    }
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
use crate::models::material;
use crate::models::material::MaterialPrototype;

/// Resolve a texture selfId hash from an MFM property to a VFS path,
/// load the DDS bytes, and return them.
fn load_texture_by_hash(
    vfs: &vfs::VfsPath,
    db: &PrototypeDatabase<'_>,
    self_id_index: &HashMap<u64, usize>,
    texture_hash: u64,
    lod: TextureLod,
) -> Option<Vec<u8>> {
    let &path_idx = self_id_index.get(&texture_hash)?;
    let full_path = db.reconstruct_path(path_idx, self_id_index);
    load_dds_from_vfs(vfs, &full_path, lod)
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
    lod: TextureLod,
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
    let ah_dds_bytes = load_texture_by_hash(vfs, db, self_id_index, ah_hash, lod)?;
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

    // Load and decode the blend map. It sets the output resolution, so decoding it
    // within budget is what keeps the bake itself within budget.
    let blend_dds_bytes = load_texture_by_hash(vfs, db, self_id_index, blend_hash, lod)?;
    let blend_img = decode_dds(&blend_dds_bytes, lod).ok()?;
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

    // The bake writes opaque pixels throughout, so averaging alpha keeps it at 255.
    let final_image = match lod.budget() {
        Some(edge) if !edge.fits(out_w, out_h) => downsample(&output, edge),
        _ => output,
    };

    encode_png(&final_image).ok()
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
    lod: TextureLod,
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
        if let Some(png) = bake_tiledland_albedo(&mat, vfs, db, idx, lod) {
            return Some(png);
        }
        eprintln!("    Warning: TILEDLAND bake failed, falling back to filename lookup");
    }

    // Fall back to simple filename-based lookup (works for standard PBS materials).
    // Force alpha=255 since model albedo textures often store non-opacity data
    // (height, roughness) in the alpha channel which would cause unwanted transparency.
    let dds_bytes = load_base_albedo_bytes(vfs, mfm_full_path, lod)?;
    let mut png = dds_to_png(&dds_bytes, lod).ok()?;
    force_png_opaque(&mut png);
    Some(png)
}

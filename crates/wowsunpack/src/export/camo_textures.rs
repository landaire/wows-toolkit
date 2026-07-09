//! Lazy camo texture support: scheme identity, metadata, and on-demand decode.

use std::collections::HashMap;

use crate::export::camouflage;
use crate::export::camouflage::UvTransform;
use crate::export::gltf_export::CamoOrigin;
use crate::export::ship::MfmInfo;
use crate::export::texture;

/// Stable identity of a camo scheme within a single loaded ship. It indexes the
/// ordered `CamoSchemeInfo` list the ship produces and is never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CamoSchemeId(pub usize);

#[derive(Debug, thiserror::Error)]
pub enum CamoDecodeError {
    #[error("no camo scheme with id {0:?}")]
    UnknownId(CamoSchemeId),
}

/// Decoded textures for one scheme: mfm stem to PNG bytes. Empty is a valid,
/// explicit result (a scheme that covers no loaded part), distinct from
/// not-yet-decoded (absence from any cache).
pub type SchemeTextures = HashMap<String, Vec<u8>>;

/// Cheap per-scheme metadata for the picker; carries no decoded textures.
#[derive(Debug, Clone)]
pub struct CamoSchemeInfo {
    pub id: CamoSchemeId,
    pub display_name: String,
    pub origin: CamoOrigin,
    pub use_color_scheme: bool,
    /// mfm stem to UV scale/offset for tiled schemes; absent stem means identity.
    pub uv_transforms: HashMap<String, UvTransform>,
}

/// Decode one filename-scanned (LegacyScan) scheme: the textures WoWs already
/// stores pre-colored per part. No baking; a missing part texture is skipped.
pub(crate) fn decode_legacy_scheme(
    vfs: &crate::vfs::VfsPath,
    unique_infos: &[&MfmInfo],
    scheme: &str,
) -> SchemeTextures {
    let mut out = SchemeTextures::new();
    for info in unique_infos {
        let Some((_base_name, dds_bytes)) = texture::load_texture_bytes(vfs, &info.stem, scheme) else {
            continue;
        };
        match texture::dds_to_png(&dds_bytes) {
            Ok(png) => {
                out.insert(info.stem.clone(), png);
            }
            Err(e) => eprintln!("  Warning: failed to decode camo texture {}_{scheme}: {e}", info.stem),
        }
    }
    out
}

/// Borrow view of the fields a mat-based scheme needs to decode, so ship.rs's
/// owned `MatCamoScheme` and the lazy source can both drive one decoder.
pub struct MatCamoSchemeView<'a> {
    pub textures: &'a HashMap<String, String>,
    pub tiled: bool,
    pub color_scheme_colors: Option<&'a [[f32; 4]; 4]>,
    pub uv_transforms: &'a HashMap<String, UvTransform>,
}

/// Decode one material-based scheme (ship-specific, universal, or expendable):
/// resolve each part's texture, bake a zone mask that carries a color scheme,
/// force-opaque a raw painted albedo. Returns stem to PNG.
pub(crate) fn decode_mat_scheme(
    vfs: &crate::vfs::VfsPath,
    scheme: &MatCamoSchemeView<'_>,
    stems: &[String],
) -> SchemeTextures {
    let mut decoded: HashMap<String, Vec<u8>> = HashMap::new();
    let mut out = SchemeTextures::new();
    for stem in stems {
        let cat = camouflage::classify_part_category(stem);
        let Some(path) = crate::export::ship::resolve_part_texture(scheme.textures, cat, scheme.tiled) else {
            continue;
        };
        if !decoded.contains_key(path) {
            let Some(dds) = texture::load_dds_from_vfs(vfs, path) else {
                continue;
            };
            // Bake (colorize) only a zone mask that has a color scheme; a real painted
            // albedo (even one that carries a color scheme) must render raw.
            let bake = scheme.color_scheme_colors.is_some() && texture::zone_mask_fraction(&dds).unwrap_or(0.0) >= 0.90;
            let png = if bake {
                // A per-ship painted mask (tiled=false) uses black as passthrough to the
                // base; a repeating tile uses black as an opaque pattern color (full cover).
                let black_passthrough = !scheme.tiled;
                match texture::bake_tiled_camo_png(&dds, scheme.color_scheme_colors.unwrap(), black_passthrough) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("  Warning: failed to bake camo {path}: {e}");
                        continue;
                    }
                }
            } else {
                // A raw painted camo is a full opaque replacement; its DDS alpha holds
                // material data (gloss/height), not camo coverage. Force it opaque so
                // the only textures carrying sub-255 alpha are baked passthrough masks,
                // which lets the compositor identify passthrough reliably.
                match texture::dds_to_png(&dds) {
                    Ok(mut p) => {
                        texture::force_png_opaque(&mut p);
                        p
                    }
                    Err(e) => {
                        eprintln!("  Warning: failed to decode camo texture {path}: {e}");
                        continue;
                    }
                }
            };
            decoded.insert(path.clone(), png);
        }
        if let Some(png) = decoded.get(path) {
            out.insert(stem.clone(), png.clone());
        }
    }
    out
}

/// Owned copy of a mat-based scheme's decode inputs, so the source survives the
/// `ShipModelContext` that built it.
#[derive(Debug, Clone)]
pub struct OwnedMatScheme {
    pub display_name: String,
    pub textures: HashMap<String, String>,
    pub tiled: bool,
    pub use_color_scheme: bool,
    pub color_scheme_colors: Option<[[f32; 4]; 4]>,
    pub uv_transforms: HashMap<String, UvTransform>,
    pub origin: CamoOrigin,
}

impl OwnedMatScheme {
    fn view(&self) -> MatCamoSchemeView<'_> {
        MatCamoSchemeView {
            textures: &self.textures,
            tiled: self.tiled,
            color_scheme_colors: self.color_scheme_colors.as_ref(),
            uv_transforms: &self.uv_transforms,
        }
    }
}

enum CamoSchemeKind {
    Legacy(usize), // index into legacy_schemes
    Mat(usize),    // index into mat_schemes
}

/// Everything needed to enumerate scheme metadata and decode any one scheme,
/// without retaining the ship context or re-parsing assets.bin.
pub struct CamoTextureSource {
    vfs: crate::vfs::VfsPath,
    unique_infos: Vec<MfmInfo>,
    unique_stems: Vec<String>,
    legacy_schemes: Vec<String>,
    mat_schemes: Vec<OwnedMatScheme>,
    kinds: Vec<CamoSchemeKind>,
}

impl CamoTextureSource {
    pub(crate) fn new(
        vfs: crate::vfs::VfsPath,
        unique_infos: Vec<MfmInfo>,
        legacy_schemes: Vec<String>,
        mat_schemes: Vec<OwnedMatScheme>,
    ) -> Self {
        let unique_stems = unique_infos.iter().map(|i| i.stem.clone()).collect();
        let mut kinds = Vec::with_capacity(legacy_schemes.len() + mat_schemes.len());
        for i in 0..legacy_schemes.len() {
            kinds.push(CamoSchemeKind::Legacy(i));
        }
        for i in 0..mat_schemes.len() {
            kinds.push(CamoSchemeKind::Mat(i));
        }
        Self { vfs, unique_infos, unique_stems, legacy_schemes, mat_schemes, kinds }
    }

    pub fn scheme_infos(&self) -> Vec<CamoSchemeInfo> {
        self.kinds
            .iter()
            .enumerate()
            .map(|(idx, kind)| {
                let id = CamoSchemeId(idx);
                match kind {
                    CamoSchemeKind::Legacy(i) => CamoSchemeInfo {
                        id,
                        display_name: self.legacy_schemes[*i].clone(),
                        origin: CamoOrigin::LegacyScan,
                        use_color_scheme: false,
                        uv_transforms: HashMap::new(),
                    },
                    CamoSchemeKind::Mat(i) => {
                        let s = &self.mat_schemes[*i];
                        CamoSchemeInfo {
                            id,
                            display_name: s.display_name.clone(),
                            origin: s.origin,
                            use_color_scheme: s.use_color_scheme,
                            uv_transforms: self.mat_uv_by_stem(s),
                        }
                    }
                }
            })
            .collect()
    }

    /// Per-stem UV transforms, mirroring the eager path's `tiled_uv_transforms`
    /// (resolve the part category, fall back to the "tile" transform when tiled).
    fn mat_uv_by_stem(&self, s: &OwnedMatScheme) -> HashMap<String, UvTransform> {
        let mut out = HashMap::new();
        for stem in &self.unique_stems {
            let cat = camouflage::classify_part_category(stem);
            let xform = s.uv_transforms.get(cat).or_else(|| if s.tiled { s.uv_transforms.get("tile") } else { None });
            if let Some(x) = xform
                && (x.scale != [1.0, 1.0] || x.offset != [0.0, 0.0])
            {
                out.insert(stem.clone(), x.clone());
            }
        }
        out
    }

    pub fn decode(&self, id: CamoSchemeId) -> Result<SchemeTextures, CamoDecodeError> {
        let kind = self.kinds.get(id.0).ok_or(CamoDecodeError::UnknownId(id))?;
        let unique_refs: Vec<&MfmInfo> = self.unique_infos.iter().collect();
        Ok(match kind {
            CamoSchemeKind::Legacy(i) => decode_legacy_scheme(&self.vfs, &unique_refs, &self.legacy_schemes[*i]),
            CamoSchemeKind::Mat(i) => decode_mat_scheme(&self.vfs, &self.mat_schemes[*i].view(), &self.unique_stems),
        })
    }
}

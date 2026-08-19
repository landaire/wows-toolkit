//! CPU-side camo compositing shared by the armor viewer and the model exporter.
//!
//! Classification and the recolor/opaque paths are resolved here so both callers
//! agree on how a camo texture reaches a surface. `CompositeOverBase` stays a
//! deferred instruction: the viewer hands it to its fragment shader (compositing
//! coverage alpha on the CPU once stalled for 18 seconds), while `flatten_over_base`
//! gives the exporter, which has no shader, the same result on the CPU.

use std::collections::HashMap;

use crate::export::camo_textures::SchemeTextures;
use crate::export::camouflage::UvTransform;

/// Hermite smoothstep from 0 at `e0` to 1 at `e1`.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear-sample RGBA `data` (`w` x `h`) at (`u`, `v`) in [0,1) with wrap. Returns `[f32; 4]`.
fn bilinear_rgba(data: &[u8], w: u32, h: u32, u: f32, v: f32) -> [f32; 4] {
    let fx = (u - u.floor()) * w as f32 - 0.5;
    let fy = (v - v.floor()) * h as f32 - 0.5;
    let (x0, y0) = (fx.floor(), fy.floor());
    let (dx, dy) = (fx - x0, fy - y0);
    let px = |xi: i64, yi: i64| -> [f32; 4] {
        let x = xi.rem_euclid(w as i64) as usize;
        let y = yi.rem_euclid(h as i64) as usize;
        let i = (y * w as usize + x) * 4;
        [data[i] as f32, data[i + 1] as f32, data[i + 2] as f32, data[i + 3] as f32]
    };
    let (x0i, y0i) = (x0 as i64, y0 as i64);
    let c00 = px(x0i, y0i);
    let c10 = px(x0i + 1, y0i);
    let c01 = px(x0i, y0i + 1);
    let c11 = px(x0i + 1, y0i + 1);
    let mut o = [0.0f32; 4];
    for k in 0..4 {
        let a = c00[k] * (1.0 - dx) + c10[k] * dx;
        let b = c01[k] * (1.0 - dx) + c11[k] * dx;
        o[k] = a * (1.0 - dy) + b * dy;
    }
    o
}

/// A coarse (heavily downsampled) luminance grid used as a low-frequency background: sampling it
/// and dividing the full-res luminance by it isolates fine albedo detail (the baked hull number,
/// panel seams) from large-scale shading.
struct LowFreqLuma {
    w: u32,
    h: u32,
    data: Vec<f32>,
}

impl LowFreqLuma {
    /// Bilinear-sample the grid at (`u`, `v`) in [0,1) with wrap. Bilinear (not nearest) keeps the
    /// high-pass detail ratio smooth so a flat hull doesn't band at grid-cell boundaries.
    fn sample(&self, u: f32, v: f32) -> f32 {
        let fx = (u - u.floor()) * self.w as f32 - 0.5;
        let fy = (v - v.floor()) * self.h as f32 - 0.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (dx, dy) = (fx - x0, fy - y0);
        let at = |xi: i64, yi: i64| -> f32 {
            let x = xi.rem_euclid(self.w as i64) as usize;
            let y = yi.rem_euclid(self.h as i64) as usize;
            self.data[y * self.w as usize + x]
        };
        let (x0i, y0i) = (x0 as i64, y0 as i64);
        let a = at(x0i, y0i) * (1.0 - dx) + at(x0i + 1, y0i) * dx;
        let b = at(x0i, y0i + 1) * (1.0 - dx) + at(x0i + 1, y0i + 1) * dx;
        a * (1.0 - dy) + b * dy
    }
}

/// Box-downsample the luminance of RGBA `srgba` (`sw` x `sh`) into a ~32x-smaller grid.
fn downsampled_luminance(srgba: &[u8], sw: u32, sh: u32) -> LowFreqLuma {
    let (w, h) = ((sw / 32).max(1), (sh / 32).max(1));
    let mut sum = vec![0f32; (w * h) as usize];
    let mut cnt = vec![0u32; (w * h) as usize];
    for y in 0..sh {
        let ly = (y * h / sh).min(h - 1);
        for x in 0..sw {
            let i = ((y * sw + x) * 4) as usize;
            let l = 0.2126 * srgba[i] as f32 + 0.7152 * srgba[i + 1] as f32 + 0.0722 * srgba[i + 2] as f32;
            let lx = (x * w / sw).min(w - 1);
            let li = (ly * w + lx) as usize;
            sum[li] += l;
            cnt[li] += 1;
        }
    }
    for (s, c) in sum.iter_mut().zip(cnt.iter()) {
        if *c > 0 {
            *s /= *c as f32;
        }
    }
    LowFreqLuma { w, h, data: sum }
}

/// Recolor a tiled camo over the base albedo (useColorScheme=True, e.g. Patches):
/// the game recolors the ship over its base rather than replacing it, so the
/// base's fine detail (the baked hull number, panel lines) survives. Bake the
/// tile over the base and modulate it by the base's high-frequency albedo detail:
/// flat hull keeps the full camo color, while the number and seams show through
/// the recolor. A tiled camo with useColorScheme=False (e.g. Spring Sky) is
/// instead an opaque replacement.
///
/// The tiling transform is baked in here, so the result samples with plain UVs.
fn recolor_tile_over_base(camo: &image::RgbaImage, srgba: &[u8], sw: u32, sh: u32, t: &UvTransform) -> Vec<u8> {
    let (cw, ch) = (camo.width(), camo.height());
    // The flat hull is recolored with the camo; strong base-albedo markings (the hull
    // number, hard painted decals) show in their TRUE color so they stay readable
    // regardless of the pattern underneath, rather than being tinted/broken up by it.
    // Blend toward the base where the local luminance deviates hard from its neighborhood.
    let low = downsampled_luminance(srgba, sw, sh);
    let mut out = vec![0u8; (cw * ch * 4) as usize];
    for y in 0..ch {
        for x in 0..cw {
            let bu = (x as f32 + 0.5) / cw as f32;
            let bv = (y as f32 + 0.5) / ch as f32;
            let base = bilinear_rgba(srgba, sw, sh, bu, bv);
            let base_l = 0.2126 * base[0] + 0.7152 * base[1] + 0.0722 * base[2];
            let smooth_l = low.sample(bu, bv).max(1.0);
            // The hull number/insignia are bright markings painted over the ship; the game
            // keeps them on top of the recolor. Reveal the true base where it is brighter
            // than its local neighborhood (signed, positive only) at full strength, so the
            // number sits on top of the camo. Dark base detail (panel lines, shadows) stays
            // camo, and the flat hull (dev ~ 0) stays fully recolored.
            let dev = ((base_l - smooth_l) / smooth_l).clamp(0.0, 1.0);
            let reveal = smoothstep(0.10, 0.30, dev);
            let cu = bu * t.scale[0] + t.offset[0];
            let cv = bv * t.scale[1] + t.offset[1];
            let cc = bilinear_rgba(camo.as_raw(), cw, ch, cu, cv);
            let o = (y * cw + x) as usize * 4;
            for k in 0..3 {
                out[o + k] = (cc[k] * (1.0 - reveal) + base[k] * reveal).clamp(0.0, 255.0) as u8;
            }
            out[o + 3] = 255;
        }
    }
    out
}

/// Decoded RGBA8 image data. Width and height travel with the pixels because a
/// camo and the base it covers are routinely different sizes.
pub struct RgbaImageData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// How one part's camo texture reaches the surface.
pub enum CamoApplication {
    /// Carries coverage alpha over a base albedo. Left unresolved: the viewer
    /// composites it in the fragment shader, the exporter flattens it on the CPU.
    CompositeOverBase { texture: RgbaImageData, uv: UvTransform },
    /// Fully resolved. Sample it directly with `uv`.
    Replace { texture: RgbaImageData, uv: UvTransform },
}

/// Classify each part's camo texture. `base_albedos` is keyed by MFM stem; a stem
/// with no base has nothing to show through, which changes the classification.
///
/// Zone-mask camos carry transparent (alpha 0) texels where the mask is black, which the game
/// reads as "no camo": those texels show the base albedo, so the red anti-fouling stays below
/// the waterline and stock detail stays in the parts the camo does not paint. That is entirely a
/// property of the camo texture, so there is no waterline geometry involved here.
pub fn apply_scheme(
    textures: &SchemeTextures,
    uv_transforms: &HashMap<String, UvTransform>,
    use_color_scheme: bool,
    base_albedos: &HashMap<String, RgbaImageData>,
) -> HashMap<String, CamoApplication> {
    let mut out: HashMap<String, CamoApplication> = HashMap::new();

    // The same PNG backs many stems (46 stems over 3 images on Smaland), so
    // decode each distinct image once.
    let mut decoded: HashMap<&Vec<u8>, image::RgbaImage> = HashMap::new();

    for (stem, png) in textures {
        let camo = match decoded.get(png) {
            Some(img) => img.clone(),
            None => {
                let Ok(img) = image::load_from_memory(png) else {
                    continue;
                };
                let img = img.to_rgba8();
                decoded.insert(png, img.clone());
                img
            }
        };
        let (cw, ch) = (camo.width(), camo.height());
        let has_coverage = camo.pixels().any(|p| p.0[3] < 250);

        // An absent entry is an identity transform: `mat_uv_by_stem` only records
        // transforms that differ from identity.
        let t = uv_transforms.get(stem).cloned().unwrap_or_default();
        let is_tiled = t.scale != [1.0, 1.0] || t.offset != [0.0, 0.0];
        let base = base_albedos.get(stem);

        if has_coverage && base.is_some() {
            let texture = RgbaImageData { width: cw, height: ch, pixels: camo.into_raw() };
            out.insert(stem.clone(), CamoApplication::CompositeOverBase { texture, uv: t });
            continue;
        }

        if has_coverage {
            // A zone-mask camo carries alpha-0 passthrough texels but there is no base
            // albedo to show through; forcing it opaque below loses the anti-fouling and
            // stock reveal. Surface it instead of failing silently.
            tracing::warn!("camo stem {stem} has passthrough texels but no base albedo; passthrough dropped");
        }

        let (pixels, uv) = if is_tiled
            && use_color_scheme
            && let Some(base) = base
        {
            (recolor_tile_over_base(&camo, &base.pixels, base.width, base.height, &t), UvTransform::default())
        } else {
            // Opaque replacement camo (uniform painted, e.g. Steel; or no base to
            // composite): force alpha opaque, keep tiling for the caller's sampler.
            let mut rgba = camo.into_raw();
            for px in rgba.chunks_exact_mut(4) {
                px[3] = 255;
            }
            (rgba, t)
        };

        out.insert(
            stem.clone(),
            CamoApplication::Replace { texture: RgbaImageData { width: cw, height: ch, pixels }, uv },
        );
    }
    out
}

/// Resolve a coverage camo against its base on the CPU, at the base's resolution.
/// The exporter has no shader, so it needs the composite as flat opaque pixels.
pub fn flatten_over_base(camo: &RgbaImageData, uv: &UvTransform, base: &RgbaImageData) -> RgbaImageData {
    let mut pixels = vec![0u8; (base.width * base.height * 4) as usize];
    for y in 0..base.height {
        for x in 0..base.width {
            let u = (x as f32 + 0.5) / base.width as f32;
            let v = (y as f32 + 0.5) / base.height as f32;
            let b = bilinear_rgba(&base.pixels, base.width, base.height, u, v);
            let cu = u * uv.scale[0] + uv.offset[0];
            let cv = v * uv.scale[1] + uv.offset[1];
            let c = bilinear_rgba(&camo.pixels, camo.width, camo.height, cu, cv);
            let a = c[3] / 255.0;
            let o = (y * base.width + x) as usize * 4;
            for k in 0..3 {
                pixels[o + k] = (c[k] * a + b[k] * (1.0 - a)).clamp(0.0, 255.0) as u8;
            }
            pixels[o + 3] = 255;
        }
    }
    RgbaImageData { width: base.width, height: base.height, pixels }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::export::camouflage::UvTransform;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImageData {
        let pixels = rgba.iter().copied().cycle().take((width * height * 4) as usize).collect();
        RgbaImageData { width, height, pixels }
    }

    fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).expect("encode png");
        out.into_inner()
    }

    #[test]
    fn a_coverage_camo_over_a_base_is_left_for_the_caller_to_composite() {
        let mut textures = HashMap::new();
        textures.insert("HULL".to_string(), png(2, 2, [10, 20, 30, 0]));
        let mut bases = HashMap::new();
        bases.insert("HULL".to_string(), solid(2, 2, [200, 200, 200, 255]));

        let out = apply_scheme(&textures, &HashMap::new(), false, &bases);
        assert!(matches!(out.get("HULL"), Some(CamoApplication::CompositeOverBase { .. })));
    }

    #[test]
    fn an_opaque_camo_replaces_and_keeps_its_tiling() {
        let mut textures = HashMap::new();
        textures.insert("HULL".to_string(), png(2, 2, [10, 20, 30, 255]));
        let mut uvs = HashMap::new();
        uvs.insert("HULL".to_string(), UvTransform { scale: [4.0, 4.0], offset: [0.0, 0.0] });

        let out = apply_scheme(&textures, &uvs, false, &HashMap::new());
        let Some(CamoApplication::Replace { uv, .. }) = out.get("HULL") else {
            panic!("an opaque camo replaces the albedo");
        };
        assert_eq!(uv.scale, [4.0, 4.0], "tiling is left for the sampler");
    }

    #[test]
    fn a_coverage_camo_without_a_base_is_forced_opaque() {
        let mut textures = HashMap::new();
        textures.insert("HULL".to_string(), png(2, 2, [10, 20, 30, 0]));

        let out = apply_scheme(&textures, &HashMap::new(), false, &HashMap::new());
        let Some(CamoApplication::Replace { texture, .. }) = out.get("HULL") else {
            panic!("with no base to show through there is nothing to composite over");
        };
        assert!(texture.pixels.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn a_recolored_tile_bakes_its_tiling_and_reports_identity_uvs() {
        let mut textures = HashMap::new();
        textures.insert("HULL".to_string(), png(4, 4, [10, 20, 30, 255]));
        let mut uvs = HashMap::new();
        uvs.insert("HULL".to_string(), UvTransform { scale: [2.0, 2.0], offset: [0.0, 0.0] });
        let mut bases = HashMap::new();
        bases.insert("HULL".to_string(), solid(4, 4, [128, 128, 128, 255]));

        let out = apply_scheme(&textures, &uvs, true, &bases);
        let Some(CamoApplication::Replace { uv, .. }) = out.get("HULL") else {
            panic!("a recolor resolves on the cpu");
        };
        assert_eq!(uv, &UvTransform::default(), "the tiling is baked in, so sampling is identity");
    }

    #[test]
    fn flattening_yields_an_opaque_image_at_the_base_resolution() {
        let camo = solid(2, 2, [10, 20, 30, 255]);
        let base = solid(8, 8, [200, 200, 200, 255]);

        let flat = flatten_over_base(&camo, &UvTransform::default(), &base);

        assert_eq!((flat.width, flat.height), (8, 8), "the base sets the resolution");
        assert!(flat.pixels.chunks_exact(4).all(|p| p[3] == 255), "a flattened camo is opaque");
        assert_eq!(&flat.pixels[0..3], &[10, 20, 30], "a fully opaque camo covers the base");
    }

    #[test]
    fn a_fully_transparent_camo_texel_shows_the_base() {
        let camo = solid(2, 2, [10, 20, 30, 0]);
        let base = solid(2, 2, [200, 210, 220, 255]);

        let flat = flatten_over_base(&camo, &UvTransform::default(), &base);

        assert_eq!(&flat.pixels[0..4], &[200, 210, 220, 255], "passthrough shows the base");
    }
}

//! Camo compositing: bakes a decoded camo scheme's per-stem PNGs against the
//! hull's base albedo into GPU-ready RGBA. Ports `armor_viewer::common::
//! build_active_camo` (`common.rs:718-831`) and its pure pixel-math helpers
//! (`smoothstep`, `bilinear_rgba`, `LowFreqLuma`/`downsampled_luminance`,
//! `common.rs:625-707`) near-verbatim -- no egui involved, so this is testable
//! with synthetic in-memory textures.
//!
//! Three cases, keyed off a decoded camo texture's own alpha channel and UV
//! transform (see `build_active_camo`'s doc for the full rationale):
//! - **Zone-mask** (has alpha-0 coverage texels, and a stock base exists):
//!   composite the camo over the stock base at camo resolution, alpha-0
//!   texels pass through to the base; the UV entry is removed (the tiling is
//!   baked into the composited texture).
//! - **Recoloring tiled** (`use_color_scheme` and the UV is non-identity, and
//!   a stock base exists): bake the tile over the base, modulated by the
//!   base's high-frequency luminance detail so the hull number/insignia
//!   reveal through; UV entry removed.
//! - **Opaque replacement** (everything else): force alpha to 255 and keep
//!   the UV entry so the GPU tiles it directly.

use std::collections::HashMap;

use wowsunpack::export::camo_textures::SchemeTextures;
use wowsunpack::export::camouflage::UvTransform;

use super::upload_hull::mfm_stem;

/// Hermite smoothstep from 0 at `e0` to 1 at `e1`. Ports `common.rs:625-628`.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear-sample RGBA `data` (`w` x `h`) at (`u`, `v`) in [0,1) with wrap.
/// Returns `[f32; 4]`. Ports `common.rs:631-654`.
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

/// A coarse (heavily downsampled) luminance grid used as a low-frequency
/// background: sampling it and dividing the full-res luminance by it isolates
/// fine albedo detail (the baked hull number, panel seams) from large-scale
/// shading. Ports `common.rs:659-663`.
struct LowFreqLuma {
    w: u32,
    h: u32,
    data: Vec<f32>,
}

impl LowFreqLuma {
    /// Bilinear-sample the grid at (`u`, `v`) in [0,1) with wrap. Bilinear
    /// (not nearest) keeps the high-pass detail ratio smooth so a flat hull
    /// doesn't band at grid-cell boundaries. Ports `common.rs:668-682`.
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

/// Box-downsample the luminance of RGBA `srgba` (`sw` x `sh`) into a
/// ~32x-smaller grid. Ports `common.rs:686-707`.
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

/// Decode a camo scheme's per-stem textures into GPU-ready RGBA, compositing
/// camos that carry a coverage alpha over the stock ship albedo (so the ship
/// shows through the gaps and the hull is opaque). Opaque camos are passed
/// through unchanged (they tile on the GPU via the returned UV map). Returns
/// (active_camo_textures: stem -> (w,h,rgba), active_camo_uvs: stem ->
/// UvTransform). Ports `common.rs:718-831` verbatim.
///
/// Zone-mask camos carry transparent (alpha 0) texels where the mask is
/// black, which the game reads as "no camo": those texels composite through
/// to the base albedo, so the red anti-fouling stays below the waterline and
/// stock detail stays in the parts the camo does not paint. That is entirely
/// a property of the camo texture, so there is no waterline geometry involved
/// here.
#[allow(clippy::type_complexity)]
pub(crate) fn build_active_camo(
    textures: &SchemeTextures,
    uv_transforms: &HashMap<String, UvTransform>,
    use_color_scheme: bool,
    hull_textures: &HashMap<String, (u32, u32, Vec<u8>)>,
) -> (HashMap<String, (u32, u32, Vec<u8>)>, HashMap<String, UvTransform>) {
    let stock_by_stem: HashMap<&str, &(u32, u32, Vec<u8>)> =
        hull_textures.iter().map(|(p, t)| (mfm_stem(p), t)).collect();

    let mut out_textures: HashMap<String, (u32, u32, Vec<u8>)> = HashMap::new();
    let mut uvs = uv_transforms.clone();

    for (stem, png) in textures {
        let Ok(img) = image::load_from_memory(png) else {
            continue;
        };
        let camo = img.to_rgba8();
        let (cw, ch) = (camo.width(), camo.height());
        let has_coverage = camo.pixels().any(|p| p.0[3] < 250);

        let t = uv_transforms.get(stem).cloned().unwrap_or_default();
        let is_tiled = t.scale != [1.0, 1.0] || t.offset != [0.0, 0.0];
        let stock = stock_by_stem.get(stem.as_str()).copied();

        if has_coverage && stock.is_none() {
            // A zone-mask camo carries alpha-0 passthrough texels but there is no base albedo to
            // show through; the fallback below force-opaques them, losing the anti-fouling/stock
            // reveal. Surface it instead of failing silently.
            tracing::warn!("camo stem {stem} has passthrough texels but no base albedo; passthrough dropped");
        }

        let pixels: Vec<u8> = if has_coverage && let Some((sw, sh, srgba)) = stock {
            // Zone-mask camo: composite camo over stock at camo resolution (baking the tiling
            // transform), so alpha-0 (black-zone) texels pass through to the base albedo.
            let mut out = vec![0u8; (cw * ch * 4) as usize];
            for y in 0..ch {
                for x in 0..cw {
                    let bu = (x as f32 + 0.5) / cw as f32;
                    let bv = (y as f32 + 0.5) / ch as f32;
                    let stock_px = bilinear_rgba(srgba, *sw, *sh, bu, bv);
                    let cu = bu * t.scale[0] + t.offset[0];
                    let cv = bv * t.scale[1] + t.offset[1];
                    let cc = bilinear_rgba(camo.as_raw(), cw, ch, cu, cv);
                    let a = cc[3] / 255.0;
                    let o = (y * cw + x) as usize * 4;
                    out[o] = (stock_px[0] * (1.0 - a) + cc[0] * a) as u8;
                    out[o + 1] = (stock_px[1] * (1.0 - a) + cc[1] * a) as u8;
                    out[o + 2] = (stock_px[2] * (1.0 - a) + cc[2] * a) as u8;
                    out[o + 3] = 255;
                }
            }
            uvs.remove(stem);
            out
        } else if is_tiled
            && use_color_scheme
            && let Some((sw, sh, srgba)) = stock
        {
            // Recoloring tiled camo (useColorScheme=True, e.g. Patches): the game recolors the ship
            // over its base rather than replacing it, so the base's fine detail (the baked hull
            // number, panel lines) survives. Bake the tile over the base and modulate it by the
            // base's high-frequency albedo detail: flat hull keeps the full camo color, while the
            // number and seams show through the recolor. A tiled camo with useColorScheme=False
            // (e.g. Spring Sky) is instead an opaque replacement (below).
            //
            // The flat hull is recolored with the camo; strong base-albedo markings (the hull
            // number, hard painted decals) show in their TRUE color so they stay readable
            // regardless of the pattern underneath, rather than being tinted/broken up by it.
            // Blend toward the base where the local luminance deviates hard from its neighborhood.
            let low = downsampled_luminance(srgba, *sw, *sh);
            let mut out = vec![0u8; (cw * ch * 4) as usize];
            for y in 0..ch {
                for x in 0..cw {
                    let bu = (x as f32 + 0.5) / cw as f32;
                    let bv = (y as f32 + 0.5) / ch as f32;
                    let base = bilinear_rgba(srgba, *sw, *sh, bu, bv);
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
            uvs.remove(stem);
            out
        } else {
            // Opaque replacement camo (uniform painted, e.g. Steel; or no stock to composite):
            // force alpha opaque, keep GPU tiling.
            let mut rgba = camo.into_raw();
            for px in rgba.chunks_exact_mut(4) {
                px[3] = 255;
            }
            rgba
        };

        out_textures.insert(stem.clone(), (cw, ch, pixels));
    }
    (out_textures, uvs)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use wowsunpack::export::camouflage::UvTransform;

    use super::bilinear_rgba;
    use super::build_active_camo;
    use super::smoothstep;

    /// A single-pixel PNG (repeated as the whole image) so `image::
    /// load_from_memory` has real bytes to decode.
    fn solid_png(pixel: [u8; 4]) -> Vec<u8> {
        let mut img = image::RgbaImage::new(2, 2);
        for p in img.pixels_mut() {
            *p = image::Rgba(pixel);
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn solid_rgba(pixel: [u8; 4], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
        (w, h, pixel.iter().copied().cycle().take((w * h * 4) as usize).collect())
    }

    #[test]
    fn smoothstep_clamps_below_and_above_the_edges() {
        assert_eq!(smoothstep(0.1, 0.3, 0.0), 0.0);
        assert_eq!(smoothstep(0.1, 0.3, 1.0), 1.0);
    }

    #[test]
    fn smoothstep_is_half_at_the_midpoint() {
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bilinear_rgba_returns_the_solid_color_of_a_uniform_texture() {
        let (w, h, data) = solid_rgba([10, 20, 30, 255], 4, 4);
        let sample = bilinear_rgba(&data, w, h, 0.3, 0.7);
        assert_eq!(sample, [10.0, 20.0, 30.0, 255.0]);
    }

    #[test]
    fn opaque_replacement_forces_alpha_255_and_keeps_uv() {
        // A camo texture with no coverage (fully opaque, non-tiled UV) and no
        // matching stock base falls into the opaque-replacement branch: alpha
        // forced to 255, its UV transform entry survives (GPU tiling).
        let mut textures = HashMap::new();
        textures.insert("Hull_A".to_string(), solid_png([200, 50, 50, 255]));
        let mut uv_transforms = HashMap::new();
        uv_transforms.insert("Hull_A".to_string(), UvTransform { scale: [2.0, 2.0], offset: [0.0, 0.0] });
        let hull_textures = HashMap::new();

        let (out_tex, out_uv) = build_active_camo(&textures, &uv_transforms, false, &hull_textures);

        let (_, _, rgba) = out_tex.get("Hull_A").expect("expected an output texture for Hull_A");
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
        assert!(out_uv.contains_key("Hull_A"), "opaque replacement must keep its UV transform");
    }

    #[test]
    fn zone_mask_passes_through_the_base_where_camo_alpha_is_zero_and_drops_the_uv() {
        // A camo texture with a fully-transparent texel (alpha 0) and a
        // matching stock base composites through to the base color at that
        // texel, and its UV transform entry is removed (baked into the output).
        let mut img = image::RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255])); // opaque camo red
        img.put_pixel(1, 0, image::Rgba([0, 0, 0, 0])); // transparent -> base shows through
        img.put_pixel(0, 1, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let mut textures = HashMap::new();
        textures.insert("Hull_A".to_string(), png);
        let uv_transforms = HashMap::new();
        let mut hull_textures = HashMap::new();
        hull_textures.insert("content/Hull_A.mfm".to_string(), solid_rgba([0, 255, 0, 255], 2, 2));

        let (out_tex, out_uv) = build_active_camo(&textures, &uv_transforms, false, &hull_textures);

        let (_, _, rgba) = out_tex.get("Hull_A").expect("expected an output texture for Hull_A");
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255), "zone-mask output must be fully opaque");
        assert!(!out_uv.contains_key("Hull_A"), "zone-mask output must have its UV baked (no tiling transform left)");
    }
}

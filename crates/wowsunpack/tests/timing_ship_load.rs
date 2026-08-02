//! Ad-hoc profiling harness: times each model-load step with game data resident.
//! Run: WOWS_DIR=E:\WoWs\World_of_Warships SHIP=BSB009_Conqueror_1949 \
//!   cargo test -p wowsunpack --test timing_ship_load -- --ignored --nocapture

use std::path::PathBuf;
use std::time::Instant;

use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::game_params::types::GameParamProvider;

#[test]
#[ignore = "requires a World of Warships install; profiling only"]
fn time_ship_load() {
    let game_dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    let ship = std::env::var("SHIP").unwrap_or_else(|_| "BSB009_Conqueror_1949".to_string());

    let t = Instant::now();
    let assets = ShipAssets::from_game_dir(&game_dir).expect("load ship assets");
    eprintln!("[from_game_dir] {:?}", t.elapsed());

    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(&ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle matching {ship}"));

    let opts = ShipExportOptions { lod: 0, textures: false, ..Default::default() };
    let t = Instant::now();
    let ctx = assets.load_ship_from_vehicle(&vehicle, &opts).expect("ctx");
    eprintln!("[load_ship_from_vehicle] {:?}", t.elapsed());

    let t = Instant::now();
    let hull_meshes = ctx.interactive_hull_meshes().expect("hull meshes");
    eprintln!("[interactive_hull_meshes] {:?} ({} meshes)", t.elapsed(), hull_meshes.len());

    let t = Instant::now();
    let albedos = ctx.hull_base_albedos(&hull_meshes);
    eprintln!("[hull_base_albedos] {:?} ({} textures)", t.elapsed(), albedos.len());

    let t = Instant::now();
    let source = ctx.camo_texture_source().expect("camo source");
    let infos = source.scheme_infos();
    eprintln!("[camo_texture_source + scheme_infos] {:?} ({} schemes, 0 decoded)", t.elapsed(), infos.len());
}

/// Times one camo scheme decode, then the UI-side work the armor viewer does on
/// top of it: PNG decode of the camo and the stock albedo, and the per-pixel
/// composite in `build_active_camo`.
/// Run: WOWS_DIR=... SHIP=WSD011_Smaland_1955 CAMO=camo_permanent_1 \
///   cargo test --release -p wowsunpack --test timing_ship_load -- --ignored --nocapture time_camo_decode
#[test]
#[ignore = "requires a World of Warships install; profiling only"]
fn time_camo_decode() {
    let game_dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    let ship = std::env::var("SHIP").unwrap_or_else(|_| "WSD011_Smaland_1955".to_string());
    let camo = std::env::var("CAMO").unwrap_or_else(|_| "camo_permanent_1".to_string());

    let assets = ShipAssets::from_game_dir(&game_dir).expect("load ship assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(&ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle matching {ship}"));

    let opts = ShipExportOptions { lod: 0, textures: false, ..Default::default() };
    let ctx = assets.load_ship_from_vehicle(&vehicle, &opts).expect("ctx");
    let source = ctx.camo_texture_source().expect("camo source");
    let infos = source.scheme_infos();

    let info = infos.iter().find(|i| i.display_name == camo).unwrap_or_else(|| {
        panic!("no scheme named {camo}; have {:?}", infos.iter().map(|i| &i.display_name).take(20).collect::<Vec<_>>())
    });

    let t = Instant::now();
    let textures = source.decode(info.id).expect("decode");
    let decode_elapsed = t.elapsed();
    let png_bytes: usize = textures.values().map(|v| v.len()).sum();
    let unique: std::collections::HashSet<&Vec<u8>> = textures.values().collect();
    eprintln!(
        "[decode {camo}] {decode_elapsed:?} ({} textures, {} unique images, {:.1} MiB of PNG)",
        textures.len(),
        unique.len(),
        png_bytes as f64 / (1024.0 * 1024.0)
    );

    // Replay the UI-side compositing the toolkit does after decode: PNG decode of
    // both the camo and the stock albedo, then the per-pixel bilinear composite.
    let hull_meshes = ctx.interactive_hull_meshes().expect("hull meshes");
    let t = Instant::now();
    let albedos = ctx.hull_base_albedos(&hull_meshes);
    eprintln!("[hull_base_albedos] {:?} ({} textures)", t.elapsed(), albedos.len());

    let stock_by_stem: std::collections::HashMap<String, &Vec<u8>> = albedos
        .iter()
        .filter_map(|(path, png)| {
            let stem = path.rsplit('/').next()?.strip_suffix(".mfm")?;
            Some((stem.to_string(), png))
        })
        .collect();

    let mut png_decode_total = std::time::Duration::ZERO;
    let mut composite_total = std::time::Duration::ZERO;
    let mut composited = 0usize;
    let mut passthrough = 0usize;
    for (stem, png) in &textures {
        let t = Instant::now();
        let camo_img = image::load_from_memory(png).expect("camo png decode").to_rgba8();
        png_decode_total += t.elapsed();
        let (cw, ch) = (camo_img.width(), camo_img.height());
        let has_coverage = camo_img.pixels().any(|p| p.0[3] < 250);

        let Some(stock_png) = stock_by_stem.get(stem) else {
            eprintln!("  {stem} {cw}x{ch} coverage={has_coverage} (no stock albedo)");
            continue;
        };
        let t = Instant::now();
        let stock_img = image::load_from_memory(stock_png).expect("stock png decode").to_rgba8();
        png_decode_total += t.elapsed();

        if !has_coverage {
            passthrough += 1;
            eprintln!("  {stem} {cw}x{ch} opaque -> GPU tile path (no composite)");
            continue;
        }

        let (sw, sh) = (stock_img.width(), stock_img.height());
        let src = stock_img.as_raw();
        let camo_raw = camo_img.as_raw();
        let t = Instant::now();
        let mut out = vec![0u8; (cw * ch * 4) as usize];
        for y in 0..ch {
            for x in 0..cw {
                let bu = (x as f32 + 0.5) / cw as f32;
                let bv = (y as f32 + 0.5) / ch as f32;
                let sp = bilinear(src, sw, sh, bu, bv);
                let cc = bilinear(camo_raw, cw, ch, bu, bv);
                let a = cc[3] / 255.0;
                let o = (y * cw + x) as usize * 4;
                out[o] = (sp[0] * (1.0 - a) + cc[0] * a) as u8;
                out[o + 1] = (sp[1] * (1.0 - a) + cc[1] * a) as u8;
                out[o + 2] = (sp[2] * (1.0 - a) + cc[2] * a) as u8;
                out[o + 3] = 255;
            }
        }
        let el = t.elapsed();
        composite_total += el;
        composited += 1;
        eprintln!("  {stem} {cw}x{ch} over stock {sw}x{sh}: composite={el:?}");
    }
    eprintln!(
        "[ui-side totals] png_decode={png_decode_total:?} composite={composite_total:?} ({composited} composited, {passthrough} opaque)"
    );
}

/// Same bilinear sampler the toolkit's `build_active_camo` uses.
fn bilinear(data: &[u8], w: u32, h: u32, u: f32, v: f32) -> [f32; 4] {
    let fx = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
    let fy = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
    let x0 = fx.floor() as u32;
    let y0 = fy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let px = |x: u32, y: u32| -> [f32; 4] {
        let i = (y * w + x) as usize * 4;
        [data[i] as f32, data[i + 1] as f32, data[i + 2] as f32, data[i + 3] as f32]
    };
    let (p00, p10, p01, p11) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut out = [0.0f32; 4];
    for k in 0..4 {
        let a = p00[k] * (1.0 - tx) + p10[k] * tx;
        let b = p01[k] * (1.0 - tx) + p11[k] * tx;
        out[k] = a * (1.0 - ty) + b * ty;
    }
    out
}

/// sRGB transfer function (IEC 61966-2-1), `c` normalized to 0..1.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Q1/Q2 from the camo-GPU-compositing measurement: do any `LegacyScan` camo
/// schemes carry continuous (non-binary) alpha, and if so, how large is the
/// straight-byte-lerp (old CPU compositor) vs sRGB-aware (new GPU shader,
/// `Rgba8UnormSrgb` decode/blend/encode) colour difference against the ship's
/// stock albedo.
///
/// `decode_mat_scheme` force-opaques raw painted albedos (their DDS alpha is
/// gloss/height, not coverage), so mat-based schemes are not the concern here.
/// `decode_legacy_scheme` does not force-opaque, so if any filename-scanned
/// texture's alpha channel is itself continuous material data, the two blends
/// disagree everywhere alpha is strictly between 0 and 255, not just on a
/// one-texel filtering fringe.
///
/// Run: WOWS_DIR=E:\WoWs\World_of_Warships SHIP=WSD011_Smaland_1955 \
///   cargo test --release -p wowsunpack --test timing_ship_load -- --ignored --nocapture \
///   verify_legacy_camo_alpha_and_blend
///
/// `SAMPLE` caps how many LegacyScan schemes are decoded (default 40, 0 = all).
#[test]
#[ignore = "requires a World of Warships install; profiling only"]
fn verify_legacy_camo_alpha_and_blend() {
    use wowsunpack::export::gltf_export::CamoOrigin;

    let game_dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    let ship = std::env::var("SHIP").unwrap_or_else(|_| "WSD011_Smaland_1955".to_string());
    let sample_limit: usize = std::env::var("SAMPLE").ok().and_then(|s| s.parse().ok()).unwrap_or(40);

    let assets = ShipAssets::from_game_dir(&game_dir).expect("load ship assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(&ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle matching {ship}"));

    let opts = ShipExportOptions { lod: 0, textures: false, ..Default::default() };
    let ctx = assets.load_ship_from_vehicle(&vehicle, &opts).expect("ctx");

    let hull_meshes = ctx.interactive_hull_meshes().expect("hull meshes");
    let albedos = ctx.hull_base_albedos(&hull_meshes);
    let stock_by_stem: std::collections::HashMap<String, &Vec<u8>> = albedos
        .iter()
        .filter_map(|(path, png)| {
            let stem = path.rsplit('/').next()?.strip_suffix(".mfm")?;
            Some((stem.to_string(), png))
        })
        .collect();

    let source = ctx.camo_texture_source().expect("camo source");
    let infos = source.scheme_infos();
    let legacy: Vec<_> = infos.iter().filter(|i| i.origin == CamoOrigin::LegacyScan).collect();
    eprintln!("[schemes] {} total, {} LegacyScan", infos.len(), legacy.len());

    let take_n = if sample_limit == 0 { legacy.len() } else { sample_limit.min(legacy.len()) };
    eprintln!("[sampling] decoding {take_n} of {} LegacyScan schemes", legacy.len());

    let mut continuous_found = false;
    let mut checked_textures = 0usize;

    for info in legacy.iter().take(take_n) {
        let textures = match source.decode(info.id) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("  decode failed for {}: {e}", info.display_name);
                continue;
            }
        };
        for (stem, png) in &textures {
            let img = match image::load_from_memory(png) {
                Ok(i) => i.to_rgba8(),
                Err(e) => {
                    eprintln!("  {}/{stem}: png decode failed: {e}", info.display_name);
                    continue;
                }
            };
            checked_textures += 1;
            let (w, h) = (img.width(), img.height());
            let total = (w * h) as u64;
            let (mut zero, mut full, mut mid) = (0u64, 0u64, 0u64);
            for p in img.pixels() {
                match p.0[3] {
                    0 => zero += 1,
                    255 => full += 1,
                    _ => mid += 1,
                }
            }
            let mid_frac = mid as f64 / total as f64;
            if mid_frac == 0.0 {
                continue;
            }
            eprintln!(
                "  {} / {stem} {w}x{h}: alpha0={:.4} alpha255={:.4} mid={:.4} ({mid}/{total} texels)",
                info.display_name,
                zero as f64 / total as f64,
                full as f64 / total as f64,
                mid_frac
            );

            // Only schemes with a material (not one-texel-fringe) fraction of
            // continuous alpha are candidates for Q2's blend comparison.
            if mid_frac < 0.01 {
                continue;
            }
            continuous_found = true;

            let Some(stock_png) = stock_by_stem.get(stem.as_str()) else {
                eprintln!("    (no stock albedo for stem {stem}; skipping blend comparison)");
                continue;
            };
            let stock_img = image::load_from_memory(stock_png).expect("stock png decode").to_rgba8();
            let (sw, sh) = (stock_img.width(), stock_img.height());
            let stock_raw = stock_img.as_raw();
            let raw = img.as_raw();

            let mut sum_max_delta = 0f64;
            let mut max_delta = 0u8;
            let (mut gt2, mut gt8, mut gt32) = (0u64, 0u64, 0u64);
            let mut n = 0u64;
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize * 4;
                    let ca = raw[i + 3] as f32 / 255.0;
                    if raw[i + 3] == 0 || raw[i + 3] == 255 {
                        continue;
                    }
                    let bu = (x as f32 + 0.5) / w as f32;
                    let bv = (y as f32 + 0.5) / h as f32;
                    let sp = bilinear(stock_raw, sw, sh, bu, bv);

                    let mut texel_max = 0u8;
                    for k in 0..3 {
                        let c = raw[i + k] as f32;
                        let s = sp[k].round().clamp(0.0, 255.0);

                        let old = (s * (1.0 - ca) + c * ca).round().clamp(0.0, 255.0) as u8;

                        let s_lin = srgb_to_linear(s / 255.0);
                        let c_lin = srgb_to_linear(c / 255.0);
                        let blended_lin = s_lin * (1.0 - ca) + c_lin * ca;
                        let new = (linear_to_srgb(blended_lin) * 255.0).round().clamp(0.0, 255.0) as u8;

                        let delta = old.abs_diff(new);
                        texel_max = texel_max.max(delta);
                    }
                    sum_max_delta += texel_max as f64;
                    max_delta = max_delta.max(texel_max);
                    if texel_max > 2 {
                        gt2 += 1;
                    }
                    if texel_max > 8 {
                        gt8 += 1;
                    }
                    if texel_max > 32 {
                        gt32 += 1;
                    }
                    n += 1;
                }
            }
            if n > 0 {
                eprintln!(
                    "    blend delta over {n} continuous-alpha texels: mean_max={:.2} max={} gt2={:.4} gt8={:.4} gt32={:.4}",
                    sum_max_delta / n as f64,
                    max_delta,
                    gt2 as f64 / n as f64,
                    gt8 as f64 / n as f64,
                    gt32 as f64 / n as f64
                );
            }
        }
    }

    eprintln!("[summary] checked {checked_textures} decoded textures; continuous alpha found = {continuous_found}");
}

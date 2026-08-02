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

/// Times a single camo scheme decode and breaks the per-texture cost into its
/// VFS read / DDS decode / PNG encode / PNG decode stages.
/// Run: WOWS_DIR=... SHIP=WSD011_Smaland_1955 CAMO=camo_permanent_1 \
///   cargo test --release -p wowsunpack --test timing_ship_load -- --ignored --nocapture time_camo_decode
#[test]
#[ignore = "requires a World of Warships install; profiling only"]
fn time_camo_decode() {
    use wowsunpack::export::texture;

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

    let info = infos
        .iter()
        .find(|i| i.display_name == camo)
        .unwrap_or_else(|| panic!("no scheme named {camo}; have {:?}", infos.iter().map(|i| &i.display_name).take(20).collect::<Vec<_>>()));

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

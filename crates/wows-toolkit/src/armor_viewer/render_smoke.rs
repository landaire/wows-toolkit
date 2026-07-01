//! Headless render smoke test: renders Smaland through the real armor-viewer
//! wgpu pipeline (same shader, same `upload_hull_meshes_to_viewport` with R8
//! brightness + camo textures) and writes PNGs so camos can be verified in 3D.
//!
//! Ignored by default: requires a GPU and a World of Warships install. Run with:
//!   OUT_DIR_CUSTOM=<dir> cargo test -p wows_toolkit --lib \
//!     armor_viewer::render_smoke::render_smaland_camos -- --ignored --nocapture

use std::path::PathBuf;

use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::GameParamProvider;

use crate::armor_viewer::common::ShipLoadOptions;
use crate::armor_viewer::common::load_ship_armor;
use crate::armor_viewer::state::ArmorPane;
use crate::armor_viewer::ui::tab::upload_hull_meshes_to_viewport;
use crate::viewport_3d::ArcballCamera;
use crate::viewport_3d::GpuPipeline;

const SHIP_NAME: &str = "WSD011_Smaland_1955";
const RENDER_SIZE: (u32, u32) = (1024, 512);

/// (raw scheme name to match, friendly label for the output file).
const SCHEMES: &[(&str, &str)] = &[
    ("camo_permanent_1", "Default"),
    ("camo_SwedenArc_style", "Traditions"),
    ("mat_SteelStyle2021", "MadeOfSteel"),
    ("mat_Snow_2025_tile", "ArcticLights"),
];

fn out_dir() -> PathBuf {
    std::env::var_os("OUT_DIR_CUSTOM").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

/// Fraction of pixels that are near-white on all channels (blowout metric).
fn near_white_fraction(rgba: &[u8]) -> f32 {
    let total = rgba.len() / 4;
    if total == 0 {
        return 0.0;
    }
    let blown = rgba.chunks_exact(4).filter(|px| px[0] >= 250 && px[1] >= 250 && px[2] >= 250).count();
    blown as f32 / total as f32
}

#[test]
#[ignore = "requires GPU + game install"]
fn render_smaland_camos() {
    let out = out_dir();
    std::fs::create_dir_all(&out).expect("create output dir");

    // Headless wgpu device + queue (HighPerformance adapter).
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    let (device, queue) = rt.block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .expect("request headless adapter");
        eprintln!("adapter: {:?}", adapter.get_info());
        adapter
            .request_device(&wgpu::DeviceDescriptor { label: Some("render_smoke_device"), ..Default::default() })
            .await
            .expect("request device")
    });

    let pipeline = GpuPipeline::new(&device, &queue);

    // Load Smaland via the real ship-asset + armor pipeline.
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("load ship assets from game dir");
    // Resolve the Vehicle by scanning params for a matching model path (the
    // by-name key is a localization id, not the model dir).
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(SHIP_NAME)).unwrap_or(false))
        .cloned()
        .or_else(|| assets.metadata().game_param_by_name(SHIP_NAME).and_then(|p| p.vehicle().cloned()))
        .unwrap_or_else(|| panic!("no vehicle for {SHIP_NAME}"));

    let mut armor = load_ship_armor(
        &vehicle,
        &assets,
        ShipLoadOptions { display_name: "Smaland".into(), lod: 0, ..Default::default() },
    )
    .expect("load ship armor");

    eprintln!(
        "loaded Smaland: {} hull meshes, {} camo schemes: {:?}",
        armor.hull_meshes.len(),
        armor.camo_schemes.len(),
        armor.camo_schemes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
    );

    for (raw, label) in SCHEMES {
        // Match by exact name or a contains() either way (localized vs raw names).
        let matched = armor
            .camo_schemes
            .iter()
            .position(|s| s.name == *raw || s.name.contains(raw) || raw.contains(s.name.as_str()));
        let Some(idx) = matched else {
            eprintln!("scheme {raw} ({label}) not found - skipping");
            continue;
        };

        let scheme = &armor.camo_schemes[idx];
        let scheme_name = scheme.name.clone();
        let (active_camo_textures, active_camo_uvs) =
            crate::armor_viewer::common::build_active_camo(scheme, &armor.hull_textures);
        armor.active_camo_textures = active_camo_textures;
        armor.active_camo_uvs = active_camo_uvs;

        // Build a pane with this camo selected and all hull parts visible + opaque.
        let mut pane = ArmorPane::empty(0);
        pane.hull_opaque = true;
        pane.selected_camo = Some(scheme_name);
        for (_group, names) in &armor.hull_part_groups {
            for name in names {
                pane.hull_visibility.insert(name.clone(), true);
            }
        }

        // Frame the camera on the whole ship: look at the bounds center from a
        // distance that fits the diagonal, with a mild yaw to show the side.
        let (min, max) = armor.bounds;
        let center = (min + max) * 0.5;
        let diagonal = (max - min).norm();
        let mut camera = ArcballCamera::from_bounds(min, max);
        camera.target = center;
        camera.distance = 1.5 * diagonal;
        camera.azimuth = 0.9;
        camera.elevation = 0.35;
        camera.near = camera.distance * 0.01;
        camera.far = camera.distance * 100.0;
        pane.viewport.camera = camera;

        // Upload hull meshes through the real function, then render headless.
        upload_hull_meshes_to_viewport(&mut pane, &armor, &device, &queue, &pipeline);
        pane.viewport.mark_dirty();

        let (w, h, rgba) =
            pane.viewport.render_offscreen_rgba(&device, &queue, &pipeline, RENDER_SIZE).expect("offscreen render");
        assert_eq!((w, h), RENDER_SIZE);
        assert!(!rgba.is_empty(), "empty render for {label}");

        let path = out.join(format!("render_{label}.png"));
        let img = image::RgbaImage::from_raw(w, h, rgba.clone()).expect("build image");
        img.save(&path).expect("save png");

        let blowout = near_white_fraction(&rgba) * 100.0;
        eprintln!("wrote {} ({}x{}) near-white={:.2}%", path.display(), w, h, blowout);
    }
}

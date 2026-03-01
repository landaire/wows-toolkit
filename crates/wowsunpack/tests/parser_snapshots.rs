//! Snapshot tests for binary format parsers.
//!
//! These tests load known game files from the VFS, parse them, and snapshot
//! the output with insta. Each parser gets its own snapshot subdirectory.
//!
//! Requires game data to be available — ignored otherwise.

use std::collections::BTreeMap;
use std::io::Read;

use serde::Serialize;
use wowsunpack::data::assets_bin_vfs::{AssetsBinVfs, PrototypeType};
use wowsunpack::models::{geometry, material, model, terrain, visual};
use wowsunpack::vfs::VfsPath;

fn game_vfs() -> VfsPath {
    wows_game_data_dl::latest_build()
        .expect("game data should be available")
        .1
}

fn load_assets_bin_vfs(vfs: &VfsPath) -> AssetsBinVfs {
    let mut data = Vec::new();
    vfs.join("content/assets.bin")
        .unwrap()
        .open_file()
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    AssetsBinVfs::new(data).unwrap()
}

// ── Assets.bin VFS structure ────────────────────────────────────────────────

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn assets_bin_vfs_file_and_dir_counts() {
    let vfs = game_vfs();
    let abvfs = load_assets_bin_vfs(&vfs);

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/assets_bin_vfs"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!("file_count", abvfs.file_count());
        insta::assert_yaml_snapshot!("dir_count", abvfs.dir_count());
    });
}

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn assets_bin_vfs_prototype_type_distribution() {
    let vfs = game_vfs();
    let abvfs = load_assets_bin_vfs(&vfs);

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, _, proto_type) in abvfs.files_with_type() {
        *counts.entry(format!("{proto_type:?}")).or_default() += 1;
    }

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/assets_bin_vfs"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!("type_distribution", counts);
    });
}

// ── Material parser (.mfm) ─────────────────────────────────────────────────

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn parse_material_snapshot() {
    let vfs = game_vfs();
    let abvfs = load_assets_bin_vfs(&vfs);

    // Find first material file under a ship gameplay directory (stable across versions)
    let mut material_files: Vec<_> = abvfs
        .files_with_type()
        .filter(|(_, _, t)| *t == PrototypeType::Material)
        .filter(|(p, _, _)| p.contains("/gameplay/"))
        .map(|(p, _, _)| p.to_string())
        .collect();
    material_files.sort();
    let path = material_files.first().expect("should find a material file");

    let mut data = Vec::new();
    let abvfs_path: VfsPath = abvfs.into();
    abvfs_path
        .join(path)
        .unwrap()
        .open_file()
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();

    let parsed = material::parse_material(&data).expect("should parse material");

    let name = path.rsplit('/').next().unwrap_or(path);
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/material"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!(name.to_string(), parsed);
    });
}

// ── Visual parser (.visual) ─────────────────────────────────────────────────

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn parse_visual_snapshot() {
    let vfs = game_vfs();
    let abvfs = load_assets_bin_vfs(&vfs);

    let mut visual_files: Vec<_> = abvfs
        .files_with_type()
        .filter(|(_, _, t)| *t == PrototypeType::Visual)
        .filter(|(p, _, _)| p.contains("/gameplay/"))
        .map(|(p, _, _)| p.to_string())
        .collect();
    visual_files.sort();
    let path = visual_files.first().expect("should find a visual file");

    let mut data = Vec::new();
    let abvfs_path: VfsPath = abvfs.into();
    abvfs_path
        .join(path)
        .unwrap()
        .open_file()
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();

    let parsed = visual::parse_visual(&data).expect("should parse visual");

    let name = path.rsplit('/').next().unwrap_or(path);
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/visual"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!(name.to_string(), parsed);
    });
}

// ── Model parser (.model) ───────────────────────────────────────────────────

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn parse_model_snapshot() {
    let vfs = game_vfs();
    let abvfs = load_assets_bin_vfs(&vfs);

    let mut model_files: Vec<_> = abvfs
        .files_with_type()
        .filter(|(_, _, t)| *t == PrototypeType::Model)
        .filter(|(p, _, _)| p.contains("/gameplay/"))
        .map(|(p, _, _)| p.to_string())
        .collect();
    model_files.sort();
    let path = model_files.first().expect("should find a model file");

    let mut data = Vec::new();
    let abvfs_path: VfsPath = abvfs.into();
    abvfs_path
        .join(path)
        .unwrap()
        .open_file()
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();

    let parsed = model::parse_model(&data).expect("should parse model");

    let name = path.rsplit('/').next().unwrap_or(path);
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/model"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!(name.to_string(), parsed);
    });
}

// ── Geometry parser (.geometry) ─────────────────────────────────────────────

#[derive(Serialize)]
struct GeometrySummary {
    vertices_mapping_count: usize,
    indices_mapping_count: usize,
    merged_vertices_count: usize,
    merged_indices_count: usize,
    collision_model_count: usize,
    armor_model_count: usize,
    armor_triangle_counts: Vec<usize>,
}

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn parse_geometry_snapshot() {
    let vfs = game_vfs();

    // Geometry files live in the PKG VFS, not assets.bin.
    // Walk content/gameplay/ to find a .geometry file.
    let gameplay = vfs.join("content/gameplay").expect("gameplay dir");
    let geom_path = find_file_recursive(&gameplay, ".geometry")
        .expect("should find a .geometry file");

    let mut data = Vec::new();
    geom_path.open_file().unwrap().read_to_end(&mut data).unwrap();

    let parsed = geometry::parse_geometry(&data).expect("should parse geometry");

    let summary = GeometrySummary {
        vertices_mapping_count: parsed.vertices_mapping.len(),
        indices_mapping_count: parsed.indices_mapping.len(),
        merged_vertices_count: parsed.merged_vertices.len(),
        merged_indices_count: parsed.merged_indices.len(),
        collision_model_count: parsed.collision_models.len(),
        armor_model_count: parsed.armor_models.len(),
        armor_triangle_counts: parsed.armor_models.iter().map(|m| m.triangles.len()).collect(),
    };

    let name = geom_path.filename();
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/geometry"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!(name, summary);
    });
}

// ── Terrain parser ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct TerrainSummary {
    width: u32,
    height: u32,
    tile_size: u16,
    tiles_per_axis: u16,
    heightmap_len: usize,
    heightmap_min: f32,
    heightmap_max: f32,
}

#[test]
#[cfg_attr(not(has_game_data), ignore)]
fn parse_terrain_snapshot() {
    let vfs = game_vfs();

    let spaces = vfs.join("spaces").expect("spaces dir");
    let terrain_path = find_file_recursive(&spaces, "terrain.bin")
        .expect("should find a terrain.bin file");

    let mut data = Vec::new();
    terrain_path
        .open_file()
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();

    let parsed = terrain::parse_terrain(&data).expect("should parse terrain");

    let (min, max) = parsed.heightmap.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| {
        (lo.min(v), hi.max(v))
    });

    let summary = TerrainSummary {
        width: parsed.width,
        height: parsed.height,
        tile_size: parsed.tile_size,
        tiles_per_axis: parsed.tiles_per_axis,
        heightmap_len: parsed.heightmap.len(),
        heightmap_min: min,
        heightmap_max: max,
    };

    // Use the space directory name as the snapshot name
    let space_name = terrain_path
        .parent()
        .filename();
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/terrain"),
    );
    settings.bind(|| {
        insta::assert_yaml_snapshot!(space_name, summary);
    });
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Recursively search for a file matching the given suffix.
fn find_file_recursive(dir: &VfsPath, suffix: &str) -> Option<VfsPath> {
    let entries = dir.read_dir().ok()?;
    let mut dirs = Vec::new();
    for entry in entries {
        let name = entry.filename();
        if name.ends_with(suffix) {
            return Some(entry);
        }
        if entry.is_dir().unwrap_or(false) {
            dirs.push(entry);
        }
    }
    // Recurse into subdirectories (sorted for determinism)
    dirs.sort_by(|a, b| a.filename().cmp(&b.filename()));
    for sub in dirs {
        if let Some(found) = find_file_recursive(&sub, suffix) {
            return Some(found);
        }
    }
    None
}

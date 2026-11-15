// Each test crate uses a subset of these helpers.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use wows_game_settings::BuildNumber;
use wows_game_settings::GameActivity;

pub const BUILD: BuildNumber = BuildNumber(13533378);

pub struct Stopped;

impl GameActivity for Stopped {
    type Hold<'state> = ();

    fn hold_stopped(&self) -> Option<()> {
        Some(())
    }
}

pub fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

pub fn engine_config_default(root: &Path) -> PathBuf {
    root.join("bin").join(BUILD.to_string()).join("res").join("engine_config.xml")
}

pub fn engine_config_override(root: &Path) -> PathBuf {
    root.join("bin").join(BUILD.to_string()).join("res_mods").join("engine_config.xml")
}

/// Lays the fixtures out as an install.
pub fn install(root: &Path) {
    for name in ["preferences.xml", "user_preferences.xml", "game_info.xml"] {
        fs::write(root.join(name), fixture(name)).unwrap();
    }

    let engine_config = engine_config_default(root);
    fs::create_dir_all(engine_config.parent().unwrap()).unwrap();
    fs::write(engine_config, fixture("engine_config.xml")).unwrap();
}

/// The lines that differ between two texts with the same line structure.
pub fn changed_lines(before: &str, after: &str) -> String {
    assert_eq!(before.lines().count(), after.lines().count(), "an edit added or removed lines");

    before
        .lines()
        .zip(after.lines())
        .filter(|(old, new)| old != new)
        .map(|(old, new)| format!("- {}\n+ {}", old.trim(), new.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

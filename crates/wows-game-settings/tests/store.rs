mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;

use wows_game_settings::DirectFiles;
use wows_game_settings::FrameRateLimit;
use wows_game_settings::GameSettingsStore;
use wows_game_settings::GameStopped;
use wows_game_settings::ReplayConfigSource;
use wows_game_settings::ReplayCount;
use wows_game_settings::Resolution;
use wows_game_settings::SaveError;
use wows_game_settings::Section;
use wows_game_settings::SettingsChanges;
use wows_game_settings::SettingsFiles;
use wows_game_settings::Volume;
use wows_game_settings::VolumeChannel;
use wows_game_settings::WindowMode;

use crate::common::BUILD;
use crate::common::Stopped;
use crate::common::changed_lines;
use crate::common::engine_config_default;
use crate::common::engine_config_override;
use crate::common::fixture;
use crate::common::install;

/// An overlay over a read-only lower directory: reads prefer the upper one,
/// writes go only there.
struct TestOverlay {
    lower: PathBuf,
    upper: PathBuf,
}

impl SettingsFiles for TestOverlay {
    fn read_path(&self, relative: &Path) -> io::Result<Option<PathBuf>> {
        let upper = self.upper.join(relative);
        if upper.exists() {
            return Ok(Some(upper));
        }

        let lower = self.lower.join(relative);
        Ok(lower.exists().then_some(lower))
    }

    fn write_path(&self, relative: &Path) -> io::Result<PathBuf> {
        let path = self.upper.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;

        Ok(path)
    }
}

fn store(root: &Path) -> GameSettingsStore<DirectFiles> {
    GameSettingsStore::new(DirectFiles::new(root.to_owned()), Some(BUILD))
}

fn sample_changes() -> SettingsChanges {
    SettingsChanges {
        window_mode: Some(WindowMode::Fullscreen),
        fullscreen_resolution: Some(Resolution::new(1920, 1200).unwrap()),
        battle_frame_rate: Some(FrameRateLimit::limited(144).unwrap()),
        skip_intro_video: Some(false),
        master_volume: Some(Volume::new(0.5).unwrap()),
        channel_volumes: BTreeMap::from([(VolumeChannel::Gunfire, Volume::new(0.25).unwrap())]),
        replays_kept: Some(ReplayCount::new(NonZeroU32::new(200).unwrap())),
        versioned_replays: Some(true),
        ..SettingsChanges::default()
    }
}

fn backups(directory: &Path, file_name: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_str().unwrap();
            name.starts_with(&format!("{file_name}.")) && name.ends_with(".bak")
        })
        .collect();
    found.sort();

    found
}

#[test]
fn reads_typed_settings() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());

    let snapshot = store(root.path()).read().unwrap();

    insta::assert_debug_snapshot!(snapshot.settings());
}

#[test]
fn save_patches_only_changed_values() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let store = store(root.path());
    let snapshot = store.read().unwrap();

    let saved = store.save(&snapshot, &sample_changes(), GameStopped::check(&Stopped).unwrap()).unwrap();

    let preferences = fs::read_to_string(root.path().join("preferences.xml")).unwrap();
    let user_preferences = fs::read_to_string(root.path().join("user_preferences.xml")).unwrap();
    let engine_config = fs::read_to_string(engine_config_override(root.path())).unwrap();
    insta::assert_snapshot!(
        "save_diff",
        [
            changed_lines(&fixture("preferences.xml"), &preferences),
            changed_lines(&fixture("user_preferences.xml"), &user_preferences),
            changed_lines(&fixture("engine_config.xml"), &engine_config),
        ]
        .join("\n\n")
    );

    assert!(user_preferences.starts_with('\u{FEFF}'));
    assert_eq!(preferences.matches("\r\n").count(), fixture("preferences.xml").matches("\r\n").count());
    assert_eq!(fs::read_to_string(engine_config_default(root.path())).unwrap(), fixture("engine_config.xml"));

    let replays = saved.settings().replays.available().unwrap();
    assert_eq!(replays.source, ReplayConfigSource::Override);
    assert_eq!(
        saved.settings().frame_rate.available().unwrap().battle.known(),
        Some(&FrameRateLimit::limited(144).unwrap())
    );
}

#[test]
fn save_backs_up_replaced_files_only() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let store = store(root.path());
    let snapshot = store.read().unwrap();

    store.save(&snapshot, &sample_changes(), GameStopped::check(&Stopped).unwrap()).unwrap();

    let preferences_backups = backups(root.path(), "preferences.xml");
    assert_eq!(preferences_backups.len(), 1);
    assert_eq!(fs::read_to_string(&preferences_backups[0]).unwrap(), fixture("preferences.xml"));
    assert_eq!(backups(root.path(), "user_preferences.xml").len(), 1);

    // The override is new, so there is nothing to back up.
    let res_mods = engine_config_override(root.path()).parent().unwrap().to_owned();
    assert!(backups(&res_mods, "engine_config.xml").is_empty());
}

#[test]
fn save_keeps_the_newest_backups() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let store = store(root.path());

    for fps in 30..37 {
        let snapshot = store.read().unwrap();
        let changes = SettingsChanges {
            battle_frame_rate: Some(FrameRateLimit::limited(fps).unwrap()),
            ..SettingsChanges::default()
        };
        store.save(&snapshot, &changes, GameStopped::check(&Stopped).unwrap()).unwrap();
    }

    let kept = backups(root.path(), "preferences.xml");
    assert_eq!(kept.len(), 5);
    let newest = kept
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .filter(|text| text.contains("<maxFrameRate>\t35\t</maxFrameRate>"))
        .count();
    assert_eq!(newest, 1);
}

#[test]
fn save_merges_unrelated_game_rewrites() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let store = store(root.path());
    let snapshot = store.read().unwrap();

    let path = root.path().join("preferences.xml");
    let rewritten = fixture("preferences.xml").replacen("<locale>\ten\t</locale>", "<locale>\tde\t</locale>", 1);
    fs::write(&path, &rewritten).unwrap();

    let changes = SettingsChanges {
        battle_frame_rate: Some(FrameRateLimit::unlimited()),
        port_frame_rate: Some(FrameRateLimit::limited(60).unwrap()),
        ..SettingsChanges::default()
    };
    store.save(&snapshot, &changes, GameStopped::check(&Stopped).unwrap()).unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(
        saved,
        rewritten.replacen(
            "<maxDockFrameRate>\t75\t</maxDockFrameRate>",
            "<maxDockFrameRate>\t60\t</maxDockFrameRate>",
            1
        )
    );
}

#[test]
fn save_skips_values_already_on_disk() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let store = store(root.path());
    let snapshot = store.read().unwrap();

    let path = root.path().join("preferences.xml");
    let rewritten = fixture("preferences.xml").replacen(
        "<maxFrameRate>\t0\t</maxFrameRate>",
        "<maxFrameRate>\t60\t</maxFrameRate>",
        1,
    );
    fs::write(&path, &rewritten).unwrap();

    let changes = SettingsChanges {
        battle_frame_rate: Some(FrameRateLimit::limited(60).unwrap()),
        master_volume: Some(Volume::new(0.38).unwrap()),
        ..SettingsChanges::default()
    };
    store.save(&snapshot, &changes, GameStopped::check(&Stopped).unwrap()).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), rewritten);
    assert_eq!(fs::read_to_string(root.path().join("user_preferences.xml")).unwrap(), fixture("user_preferences.xml"));
    assert!(backups(root.path(), "preferences.xml").is_empty());
    assert!(backups(root.path(), "user_preferences.xml").is_empty());
}

#[test]
fn save_refuses_when_the_game_rewrote_a_changed_value() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let store = store(root.path());
    let snapshot = store.read().unwrap();

    let path = root.path().join("preferences.xml");
    let rewritten = fixture("preferences.xml").replacen(
        "<maxFrameRate>\t0\t</maxFrameRate>",
        "<maxFrameRate>\t60\t</maxFrameRate>",
        1,
    );
    fs::write(&path, &rewritten).unwrap();

    let changes = SettingsChanges {
        battle_frame_rate: Some(FrameRateLimit::limited(144).unwrap()),
        ..SettingsChanges::default()
    };
    let error = store.save(&snapshot, &changes, GameStopped::check(&Stopped).unwrap()).unwrap_err();

    assert!(matches!(error, SaveError::ChangedOnDisk { path: changed } if changed == path));
    assert_eq!(fs::read_to_string(&path).unwrap(), rewritten);
}

#[test]
fn unknown_section_versions_are_read_only() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let path = root.path().join("user_preferences.xml");
    let patched = fixture("user_preferences.xml")
        .replacen(r#"<device version="2""#, r#"<device version="3""#, 1)
        .replacen(r#"mixingPreset="CUSTOM""#, r#"mixingPreset="BATTLE""#, 1);
    fs::write(&path, &patched).unwrap();
    let store = store(root.path());
    let snapshot = store.read().unwrap();

    let display = SettingsChanges { window_mode: Some(WindowMode::Windowed), ..SettingsChanges::default() };
    let channel = SettingsChanges {
        channel_volumes: BTreeMap::from([(VolumeChannel::World, Volume::new(0.1).unwrap())]),
        ..SettingsChanges::default()
    };
    let errors = [display, channel]
        .map(|changes| store.save(&snapshot, &changes, GameStopped::check(&Stopped).unwrap()).unwrap_err().to_string());

    assert!(matches!(snapshot.settings().display, Section::Unavailable(_)));
    insta::assert_debug_snapshot!(errors);
    assert_eq!(fs::read_to_string(&path).unwrap(), patched);
}

#[test]
fn overlay_writes_go_to_the_upper_directory() {
    let lower = tempfile::tempdir().unwrap();
    let upper = tempfile::tempdir().unwrap();
    install(lower.path());
    let store = GameSettingsStore::new(
        TestOverlay { lower: lower.path().to_owned(), upper: upper.path().to_owned() },
        Some(BUILD),
    );
    let snapshot = store.read().unwrap();

    let saved = store.save(&snapshot, &sample_changes(), GameStopped::check(&Stopped).unwrap()).unwrap();

    assert_eq!(fs::read_to_string(lower.path().join("preferences.xml")).unwrap(), fixture("preferences.xml"));
    assert!(!engine_config_override(lower.path()).exists());
    assert!(engine_config_override(upper.path()).exists());
    assert_eq!(backups(upper.path(), "preferences.xml").len(), 1);
    assert!(saved.source_paths().all(|path| path.starts_with(upper.path()) || path.ends_with("game_info.xml")));
}

//! Loading, judging, and installing the `constants.json` files that decode a
//! replay's server-provided battle results.

use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;
use wowsunpack::data::Version;

/// Whether a set of replay constants was produced for the build it is being
/// used with. Results decoded through mismatched constants read the wrong
/// indices, so they are never persisted (see `replay_index::map_rows`).
///
/// Shared with the GPUI port, which judges the same files by the same rule.
pub use wows_toolkit_viewmodel::index_rows::ConstantsFit;

/// Judge `constants` against the build it is about to decode.
///
/// An exact build number settles it. Otherwise the file fits when it names the
/// same release, which is how the resolver pairs a regional client with another
/// region's file for the same release. Anything else is a mismatch, including
/// the nearest-older file used when nothing better is published: still worth
/// decoding with, never worth persisting results from.
pub fn constants_fit(constants: &Value, build: u32, version: Option<Version>) -> ConstantsFit {
    wows_toolkit_viewmodel::index_rows::constants_fit(constants, build, version)
}

/// A `constants.json` that has been read and identified.
#[derive(Debug, Clone)]
pub struct ParsedConstants {
    /// `VERSION.BUILD`: the build this file was dumped for.
    pub build: u32,
    /// `VERSION.VERSION`, the game version as `"15.2"`, when the file carries it.
    pub version: Option<String>,
    pub data: Value,
}

/// Where an imported file landed.
#[derive(Debug, Clone)]
pub struct InstalledConstants {
    pub build: u32,
    pub version: Option<String>,
    pub storage_path: PathBuf,
    /// The per-build dump copy, when the builds index knows that build.
    pub dump_path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("failed to read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("file is not valid JSON: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
    },
    #[error("file has no VERSION.BUILD field; is it a constants.json?")]
    MissingBuild,
    #[error("failed to write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("no application storage directory is available")]
    NoStorageDir,
}

/// Read a user-picked `constants.json` and take the build it belongs to from
/// its own `VERSION.BUILD`.
///
/// A file that does not say which build it is for is rejected rather than
/// assumed to match the loaded build: installing constants under the wrong
/// build number is the failure this import exists to fix.
pub fn read_constants_file(path: &Path) -> Result<ParsedConstants, ImportError> {
    let bytes = std::fs::read(path).map_err(|source| ImportError::Read { path: path.to_path_buf(), source })?;
    let data: Value = serde_json::from_slice(&bytes).map_err(|source| ImportError::Parse { source })?;

    let version_block = data.get("VERSION");
    let build = version_block
        .and_then(|v| v.get("BUILD"))
        .and_then(Value::as_u64)
        .and_then(|build| u32::try_from(build).ok())
        .ok_or(ImportError::MissingBuild)?;
    let version = version_block.and_then(|v| v.get("VERSION")).and_then(Value::as_str).map(str::to_owned);

    Ok(ParsedConstants { build, version, data })
}

/// Install `parsed` at every path a build reads constants from: the versioned
/// storage cache, and the build's dump directory when one exists.
///
/// The dump copy is overwritten unconditionally. `BuildData::from_dump` prefers
/// the dump-local file, so leaving a stale one in place would make the import a
/// no-op for any build that has been dumped.
pub fn install_constants(
    parsed: &ParsedConstants,
    storage_dir: Option<&Path>,
    dump_base: Option<&Path>,
) -> Result<InstalledConstants, ImportError> {
    let storage_dir = storage_dir.ok_or(ImportError::NoStorageDir)?;
    let storage_path = storage_dir.join(format!("constants_{}.json", parsed.build));
    let bytes = serde_json::to_vec(&parsed.data).map_err(|source| ImportError::Parse { source })?;
    std::fs::write(&storage_path, &bytes)
        .map_err(|source| ImportError::Write { path: storage_path.clone(), source })?;

    let dump_path = dump_base.and_then(|base| {
        let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
        index.find_by_build(parsed.build).map(|entry| base.join(&entry.dir).join("constants.json"))
    });

    if let Some(path) = &dump_path {
        let pretty = serde_json::to_vec_pretty(&parsed.data).map_err(|source| ImportError::Parse { source })?;
        std::fs::write(path, &pretty).map_err(|source| ImportError::Write { path: path.clone(), source })?;
    }

    Ok(InstalledConstants { build: parsed.build, version: parsed.version.clone(), storage_path, dump_path })
}

/// Shared with the GPUI port, which picks a build's constants the same way.
pub use wows_toolkit_viewmodel::index_rows::resolve_replay_constants;

#[cfg(test)]
mod tests {
    use super::ConstantsFit;
    use super::ImportError;
    use super::constants_fit;
    use super::install_constants;
    use super::read_constants_file;
    use super::resolve_replay_constants;
    use serde_json::json;
    use wowsunpack::data::Version;

    fn version(major: u32, minor: u32) -> Version {
        Version { major, minor, patch: 0, build: std::num::NonZeroU32::new(12116141) }
    }

    #[test]
    fn a_dump_local_file_is_preferred_and_judged() {
        let dump = json!({ "VERSION": { "VERSION": "15.2", "BUILD": 12116141 } });
        let cached = json!({ "VERSION": { "VERSION": "15.1", "BUILD": 11965230 } });
        let fallback = json!({ "VERSION": { "VERSION": "15.0", "BUILD": 1 } });

        let (chosen, fit) =
            resolve_replay_constants(Some(dump.clone()), Some(cached), &fallback, 12116141, Some(version(15, 2)));

        assert_eq!(chosen, dump);
        assert_eq!(fit, ConstantsFit::Matched);
    }

    #[test]
    fn a_dump_local_file_from_another_build_is_still_used_but_does_not_fit() {
        // The dump copy is what the rest of the app decodes with, so it is
        // still returned; only its trustworthiness changes.
        let dump = json!({ "VERSION": { "VERSION": "15.1", "BUILD": 11965230 } });
        let fallback = json!({ "VERSION": { "VERSION": "15.0", "BUILD": 1 } });

        let (chosen, fit) =
            resolve_replay_constants(Some(dump.clone()), None, &fallback, 12116141, Some(version(15, 2)));

        assert_eq!(chosen, dump);
        assert_eq!(fit, ConstantsFit::Mismatched);
    }

    #[test]
    fn the_disk_cache_is_used_when_there_is_no_dump_copy() {
        let cached = json!({ "VERSION": { "VERSION": "15.2", "BUILD": 12116141 } });
        let fallback = json!({ "VERSION": { "VERSION": "15.0", "BUILD": 1 } });

        let (chosen, fit) =
            resolve_replay_constants(None, Some(cached.clone()), &fallback, 12116141, Some(version(15, 2)));

        assert_eq!(chosen, cached);
        assert_eq!(fit, ConstantsFit::Matched);
    }

    #[test]
    fn the_fallback_is_used_last_and_rarely_fits() {
        let fallback = json!({ "VERSION": { "VERSION": "15.0", "BUILD": 11565330 } });

        let (chosen, fit) = resolve_replay_constants(None, None, &fallback, 12116141, Some(version(15, 2)));

        assert_eq!(chosen, fallback);
        assert_eq!(fit, ConstantsFit::Mismatched);
    }

    #[test]
    fn a_matching_build_number_fits() {
        let constants = json!({ "VERSION": { "VERSION": "15.2", "BUILD": 12116141 } });
        assert_eq!(constants_fit(&constants, 12116141, Some(version(15, 2))), ConstantsFit::Matched);
    }

    #[test]
    fn a_different_build_number_does_not_fit() {
        let constants = json!({ "VERSION": { "VERSION": "15.2", "BUILD": 12116141 } });
        assert_eq!(constants_fit(&constants, 11965230, Some(version(15, 1))), ConstantsFit::Mismatched);
    }

    #[test]
    fn a_different_build_of_the_same_release_fits() {
        // Cross-region: same release, different build number, interchangeable
        // files. The fetcher resolves these by friendly version and stores them
        // under the target build, so the file keeps its origin build number.
        let constants = json!({ "VERSION": { "VERSION": "14.7", "PATCH": 1.0, "BUILD": 24477 } });
        let cn_build = Version { major: 14, minor: 7, patch: 1, build: std::num::NonZeroU32::new(25588) };
        assert_eq!(constants_fit(&constants, 25588, Some(cn_build)), ConstantsFit::Matched);
    }

    #[test]
    fn an_older_release_used_as_a_last_resort_does_not_fit() {
        // Nearest-older fallback: usable as a data source when nothing better
        // exists, never trusted enough to persist results from.
        let constants = json!({ "VERSION": { "VERSION": "14.7", "PATCH": 0.0, "BUILD": 10374819 } });
        let build = Version { major: 14, minor: 7, patch: 1, build: std::num::NonZeroU32::new(10448683) };
        assert_eq!(constants_fit(&constants, 10448683, Some(build)), ConstantsFit::Mismatched);
    }

    #[test]
    fn a_missing_patch_field_reads_as_patch_zero() {
        // The upstream manifest defaults its patch component to 0; a constants
        // file that omits PATCH is read the same way.
        let constants = json!({ "VERSION": { "VERSION": "15.2", "BUILD": 12116141 } });
        let build = Version { major: 15, minor: 2, patch: 0, build: std::num::NonZeroU32::new(99) };
        assert_eq!(constants_fit(&constants, 99, Some(build)), ConstantsFit::Matched);
    }

    #[test]
    fn without_a_build_number_the_game_version_decides() {
        let constants = json!({ "VERSION": { "VERSION": "0.10.7" } });
        assert_eq!(constants_fit(&constants, 3747819, Some(version(0, 10))), ConstantsFit::Mismatched);

        let constants = json!({ "VERSION": { "VERSION": "15.2" } });
        assert_eq!(constants_fit(&constants, 12116141, Some(version(15, 2))), ConstantsFit::Matched);
        assert_eq!(constants_fit(&constants, 12116141, Some(version(15, 1))), ConstantsFit::Mismatched);
    }

    #[test]
    fn an_unknown_game_version_cannot_confirm_a_fit() {
        let constants = json!({ "VERSION": { "VERSION": "15.2" } });
        assert_eq!(constants_fit(&constants, 12116141, None), ConstantsFit::Mismatched);
    }

    #[test]
    fn constants_without_a_version_block_never_fit() {
        assert_eq!(constants_fit(&json!({}), 12116141, Some(version(15, 2))), ConstantsFit::Mismatched);
    }

    fn write_json(path: &std::path::Path, value: &serde_json::Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn sample_constants() -> serde_json::Value {
        json!({ "VERSION": { "VERSION": "15.2", "BUILD": 12116141 }, "SHIP_TYPES": {} })
    }

    /// A dump base with `builds.toml` naming one build directory, matching what
    /// `wows_data_mgr::builds::BuildsIndex::load` expects.
    fn dump_base_with_build(root: &std::path::Path, build: u32, dir: &str) -> std::path::PathBuf {
        let base = root.join("dumps");
        std::fs::create_dir_all(base.join(dir)).unwrap();
        std::fs::write(
            base.join("builds.toml"),
            format!(
                "[[builds]]\nbuild = {build}\ndir = \"{dir}\"\nversion = \"15.2.0\"\ndumped_at = \"2026-08-10T00:00:00Z\"\n"
            ),
        )
        .unwrap();
        base
    }

    #[test]
    fn reading_a_file_takes_its_build_and_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("constants.json");
        write_json(&path, &sample_constants());

        let parsed = read_constants_file(&path).expect("reads");
        assert_eq!(parsed.build, 12116141);
        assert_eq!(parsed.version.as_deref(), Some("15.2"));
    }

    #[test]
    fn a_file_without_a_build_number_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("constants.json");
        write_json(&path, &json!({ "VERSION": { "VERSION": "15.2" } }));

        assert!(matches!(read_constants_file(&path), Err(ImportError::MissingBuild)));
    }

    #[test]
    fn a_file_that_is_not_json_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("constants.json");
        std::fs::write(&path, b"not json at all").unwrap();

        assert!(matches!(read_constants_file(&path), Err(ImportError::Parse { .. })));
    }

    #[test]
    fn a_missing_file_reports_its_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nope.json");

        match read_constants_file(&path) {
            Err(ImportError::Read { path: reported, .. }) => assert_eq!(reported, path),
            other => panic!("expected a read error, got {other:?}"),
        }
    }

    #[test]
    fn installing_writes_the_storage_copy_and_the_dump_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let dump_base = dump_base_with_build(tmp.path(), 12116141, "15.2.0_12116141");
        let source = tmp.path().join("picked.json");
        write_json(&source, &sample_constants());
        let parsed = read_constants_file(&source).unwrap();

        let installed = install_constants(&parsed, Some(&storage), Some(&dump_base)).expect("installs");

        assert_eq!(installed.build, 12116141);
        assert_eq!(installed.storage_path, storage.join("constants_12116141.json"));
        assert_eq!(installed.dump_path, Some(dump_base.join("15.2.0_12116141").join("constants.json")));
        for path in [installed.storage_path.clone(), installed.dump_path.clone().unwrap()] {
            let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(written, sample_constants());
        }
    }

    #[test]
    fn installing_overwrites_an_existing_dump_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let dump_base = dump_base_with_build(tmp.path(), 12116141, "15.2.0_12116141");
        let stale = dump_base.join("15.2.0_12116141").join("constants.json");
        write_json(&stale, &json!({ "VERSION": { "VERSION": "15.0", "BUILD": 11965230 } }));
        let source = tmp.path().join("picked.json");
        write_json(&source, &sample_constants());
        let parsed = read_constants_file(&source).unwrap();

        install_constants(&parsed, Some(&storage), Some(&dump_base)).expect("installs");

        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&stale).unwrap()).unwrap();
        assert_eq!(written, sample_constants());
    }

    #[test]
    fn installing_for_a_build_with_no_dump_writes_storage_only() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = tmp.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let dump_base = dump_base_with_build(tmp.path(), 11965230, "15.1.0_11965230");
        let source = tmp.path().join("picked.json");
        write_json(&source, &sample_constants());
        let parsed = read_constants_file(&source).unwrap();

        let installed = install_constants(&parsed, Some(&storage), Some(&dump_base)).expect("installs");

        assert_eq!(installed.dump_path, None);
        assert!(installed.storage_path.exists());
        assert!(!dump_base.join("15.1.0_11965230").join("constants.json").exists());
    }

    #[test]
    fn installing_without_a_storage_dir_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("picked.json");
        write_json(&source, &sample_constants());
        let parsed = read_constants_file(&source).unwrap();

        assert!(matches!(install_constants(&parsed, None, None), Err(ImportError::NoStorageDir)));
    }
}

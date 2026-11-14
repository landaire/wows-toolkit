//! Game data sourced the way the CLI flags describe: a live install (`-g`),
//! an extracted or dump directory (`-e`), and a constants JSON (`-c`).

use std::borrow::Cow;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rootcause::prelude::*;
use wows_replays::context::GameDataContext;
use wows_replays::context::GameDataContextError;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::DataFileWithCallback;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::rpc::entitydefs::parse_scripts;
use wowsunpack::vfs::VfsPath;

/// Game data from a live install (`-g`), an extracted or dump directory
/// (`-e`), and the build's constants JSON (see [`versioned_constants`])
/// merged over whatever constants the game files provide.
pub struct CliGameDataContext<'a> {
    game_dir: Option<&'a Path>,
    extracted: Option<&'a Path>,
    constants_path: Option<&'a Path>,
}

impl<'a> CliGameDataContext<'a> {
    pub fn new(game_dir: Option<&'a Path>, extracted: Option<&'a Path>, constants_path: Option<&'a Path>) -> Self {
        Self { game_dir, extracted, constants_path }
    }
}

impl GameDataContext for CliGameDataContext<'_> {
    fn entity_specs(&self, version: &Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError> {
        load_game_data(self.game_dir, self.extracted, version)
            .map(Arc::new)
            .map_err(|e| GameDataContextError::new("entity specs", *version, e))
    }

    fn game_constants(&self, version: &Version) -> Result<Arc<GameConstants>, GameDataContextError> {
        let vfs = open_build_vfs(self.game_dir, self.extracted, version);
        let overrides = versioned_constants(self.constants_path, self.extracted, version)
            .map_err(|e| GameDataContextError::new("game constants", *version, e))?;

        Ok(Arc::new(GameConstants::for_build(vfs.as_ref(), overrides.as_ref(), Some(*version))))
    }

    fn metadata_provider(&self, version: &Version) -> Result<Arc<GameMetadataProvider>, GameDataContextError> {
        load_metadata_provider(self.game_dir, self.extracted, version)
            .map(Arc::new)
            .map_err(|e| GameDataContextError::new("metadata provider", *version, e))
    }
}

struct ExtractedMetadata {
    version: String,
    build: u32,
}

fn read_metadata(path: &Path) -> Option<ExtractedMetadata> {
    let contents = std::fs::read_to_string(path.join("metadata.toml")).ok()?;
    let table: toml::Table = contents.parse().ok()?;
    Some(ExtractedMetadata {
        version: table.get("version")?.as_str()?.to_string(),
        build: table.get("build")?.as_integer()? as u32,
    })
}

/// The wows-constants JSON for `version`'s build: the -c file, else the
/// resolved dump's `constants.json`. Decoding merges it over the game's own
/// constants, and battle results read their field names from it. `None` when
/// neither exists, which leaves the server's results out of an analysis.
pub fn versioned_constants(
    constants_path: Option<&Path>,
    extracted: Option<&Path>,
    version: &Version,
) -> rootcause::Result<Option<serde_json::Value>> {
    if let Some(path) = constants_path {
        return read_constants_file(path).map(Some);
    }

    let Some(extracted) = extracted else { return Ok(None) };
    let Ok(dir) = resolve_extracted_dir(extracted, version) else { return Ok(None) };
    let Ok(text) = std::fs::read_to_string(dir.join("constants.json")) else { return Ok(None) };

    Ok(serde_json::from_str(&text).ok())
}

/// Read and parse a -c/--constants file.
pub fn read_constants_file(path: &Path) -> rootcause::Result<serde_json::Value> {
    let data =
        std::fs::read_to_string(path).context_with(|| format!("failed to read constants file {}", path.display()))?;

    Ok(serde_json::from_str(&data).context_with(|| format!("failed to parse constants JSON {}", path.display()))?)
}

/// Resolve the extracted data directory. If the user passed a parent directory
/// containing version subdirectories (e.g. `15.1.0_11965230/`), auto-detect the
/// right one. If they passed the version dir itself, use it directly.
/// Falls back to the legacy `<extracted>/<version>` layout.
pub fn resolve_extracted_dir(path: &Path, replay_version: &Version) -> rootcause::Result<PathBuf> {
    if !path.exists() {
        bail!("Extracted data directory does not exist: {}", path.display());
    }

    let replay_build =
        replay_version.build_number().ok_or_else(|| report!("replay version carries no build number"))?;

    // If the path itself contains metadata.toml, it's already the version dir
    if let Some(meta) = read_metadata(path) {
        if meta.build == replay_build {
            return Ok(path.to_path_buf());
        }
        // Cross-region: another server's build of the same friendly version
        // ships interchangeable files.
        let version_str = format!("{}.{}.{}", replay_version.major, replay_version.minor, replay_version.patch);
        if meta.version == version_str || meta.version.starts_with(&format!("{version_str}.")) {
            eprintln!("No exact data for build {}; using {} (build {})", replay_build, meta.version, meta.build);
            return Ok(path.to_path_buf());
        }
        bail!(
            "Extracted data is build {} ({}) but replay is build {} ({}). \
             Entity definitions will not match. Use extracted data for the correct version.",
            meta.build,
            meta.version,
            replay_build,
            version_str
        );
    }

    // Scan for version subdirectories with metadata.toml
    let mut candidates: Vec<(PathBuf, ExtractedMetadata)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let sub = entry.path();
            if let Some(meta) = read_metadata(&sub) {
                candidates.push((sub, meta));
            }
        }
    }

    if !candidates.is_empty() {
        // Try to match by build number
        if let Some(matched) = candidates.iter().find(|(_, m)| m.build == replay_build) {
            return Ok(matched.0.clone());
        }

        // Cross-region fallback via BuildsIndex: same major.minor.patch, different build.
        let builds_path = path.join("builds.toml");
        if builds_path.exists() {
            let version_str = format!("{}.{}.{}", replay_version.major, replay_version.minor, replay_version.patch);
            let index = wows_data_mgr::builds::BuildsIndex::load(&builds_path)?;
            if let Some((entry, _exact)) = index.resolve_build(replay_build, Some(&version_str)) {
                eprintln!("No exact data for build {}; using {} (build {})", replay_build, entry.version, entry.build);
                return Ok(path.join(&entry.dir));
            }
        }

        // Last-ditch: same version string in candidates.
        let version_str = format!("{}.{}.{}", replay_version.major, replay_version.minor, replay_version.patch);
        if let Some(matched) = candidates.iter().find(|(_, m)| m.version == version_str) {
            eprintln!(
                "No exact data for build {}; using {} (build {})",
                replay_build, matched.1.version, matched.1.build
            );
            return Ok(matched.0.clone());
        }

        if candidates.len() == 1 {
            let (_, ref meta) = candidates[0];
            bail!(
                "No exact build match for replay (build {}). Only available: {} (build {}). \
                 Download or extract the correct build.",
                replay_build,
                meta.version,
                meta.build
            );
        }

        let available: Vec<String> =
            candidates.iter().map(|(_, m)| format!("{} (build {})", m.version, m.build)).collect();
        bail!(
            "No extracted data matches replay build {}. Available versions in {}: {}",
            replay_build,
            path.display(),
            available.join(", ")
        );
    }

    // Legacy fallback: <extracted>/<major.minor.patch>/
    let legacy = path.join(replay_version.to_path());
    if legacy.exists() {
        return Ok(legacy);
    }

    Err(report!(
        "No extracted game data found in {}. Expected a version directory \
         (containing metadata.toml) or legacy layout (<extracted>/<version>/).",
        path.display()
    ))
}

fn load_game_data(
    game_dir: Option<&Path>,
    extracted_dir: Option<&Path>,
    replay_version: &Version,
) -> rootcause::Result<Vec<EntitySpec>> {
    let specs = match (game_dir, extracted_dir) {
        (Some(game_dir), _) => {
            let resources = game_data::load_game_resources(game_dir, replay_version).map_err(|e| report!("{}", e))?;
            resources.specs
        }
        (None, Some(extracted)) => {
            // Dumps store game files content-addressed; Dump::open resolves
            // both CAS layouts and materialized vfs/ directories.
            let dir = resolve_extracted_dir(extracted, replay_version)?;
            let dump = wows_data_mgr::Dump::open(&dir);
            if dump.has_game_files() {
                let vfs = dump.vfs();
                let loader = DataFileWithCallback::new(|path| {
                    let mut file_data = Vec::new();
                    vfs.join(path)
                        .map_err(wowsunpack::error::GameDataError::from)?
                        .open_file()
                        .map_err(wowsunpack::error::GameDataError::from)?
                        .read_to_end(&mut file_data)
                        .map_err(wowsunpack::error::GameDataError::from)?;
                    Ok(Cow::Owned(file_data))
                });
                parse_scripts(&loader).map_err(|e| report!("failed to parse entity defs: {e:?}"))?
            } else {
                // Legacy hand-extracted layout: game files sit directly under
                // the version dir with no dump manifest.
                let loader = DataFileWithCallback::new(|path| {
                    let file_data = std::fs::read(dir.join(path)).map_err(wowsunpack::error::GameDataError::from)?;
                    Ok(Cow::Owned(file_data))
                });
                parse_scripts(&loader).map_err(|e| report!("failed to parse entity defs: {e:?}"))?
            }
        }
        (None, None) => {
            bail!("Game directory or extracted files directory must be supplied");
        }
    };

    Ok(specs)
}

/// Open the game-file VFS for a build from whichever source the CLI flags
/// name. `None` when neither flag is given or the build's files are absent.
fn open_build_vfs(game_dir: Option<&Path>, extracted_dir: Option<&Path>, version: &Version) -> Option<VfsPath> {
    match (game_dir, extracted_dir) {
        (Some(game_dir), _) => game_data::load_game_resources(game_dir, version).ok().map(|resources| resources.vfs),
        (None, Some(extracted)) => {
            let dir = resolve_extracted_dir(extracted, version).ok()?;
            let dump = wows_data_mgr::Dump::open(&dir);
            if dump.has_game_files() { Some(dump.vfs()) } else { None }
        }
        (None, None) => None,
    }
}

/// Load the game metadata provider (game params + entity specs + translations)
/// for the given replay version. The provider resolves ship param ids to
/// localized names; the entity specs drive the packet parser.
fn load_metadata_provider(
    game_dir: Option<&Path>,
    extracted_dir: Option<&Path>,
    version: &Version,
) -> rootcause::Result<GameMetadataProvider> {
    match (game_dir, extracted_dir) {
        (Some(game_dir), _) => {
            let resources = game_data::load_game_resources(game_dir, version).map_err(|e| report!("{}", e))?;
            let mut provider = GameMetadataProvider::from_vfs(&resources.vfs)
                .map_err(|e| report!("failed to load game params: {e:?}"))?;
            let build = version.build_number().ok_or_else(|| report!("replay version carries no build number"))?;
            let mo_path = game_data::translations_path(game_dir, build);
            apply_translations(Some(&mo_path), &mut provider);
            Ok(provider)
        }
        (None, Some(extracted)) => {
            let dir = resolve_extracted_dir(extracted, version)?;
            let dump = wows_data_mgr::Dump::open(&dir);
            if !dump.has_game_files() {
                bail!("no game files found in {}", dir.display());
            }
            let vfs = dump.vfs();

            // Prefer the prebuilt rkyv cache; fall back to parsing GameParams.data.
            let cached =
                dump.derived_path("game_params.rkyv").and_then(|path| wowsunpack::game_params::cache::load(&path));
            let mut provider = match cached {
                Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs)
                    .map_err(|e| report!("failed to build game metadata: {e:?}"))?,
                None => GameMetadataProvider::from_vfs(&vfs)
                    .map_err(|e| report!("failed to load game params from VFS: {e:?}"))?,
            };
            let mo_path = dump.derived_path("translations/en/LC_MESSAGES/global.mo");
            apply_translations(mo_path.as_deref(), &mut provider);
            Ok(provider)
        }
        (None, None) => Err(report!("Game directory or extracted files directory must be supplied")),
    }
}

/// Load a `.mo` translation catalog into the provider. Ship names are
/// unavailable (param indices are shown instead) when this fails.
fn apply_translations(mo_path: Option<&Path>, provider: &mut GameMetadataProvider) {
    let Some(mo_path) = mo_path else {
        eprintln!("# warning: no translation catalog for this build (ship names unavailable)");
        return;
    };
    match File::open(mo_path) {
        Ok(file) => match gettext::Catalog::parse(file) {
            Ok(catalog) => provider.set_translations(catalog),
            Err(e) => eprintln!("# warning: failed to parse translations {}: {e}", mo_path.display()),
        },
        Err(_) => {
            eprintln!("# warning: translations not found at {} (ship names unavailable)", mo_path.display());
        }
    }
}

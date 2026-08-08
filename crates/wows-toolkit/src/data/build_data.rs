//! Per-build game data: what one World of Warships build provides to the
//! rest of the app.
//!
//! [`BuildData`] is the bundle (VFS, metadata provider, constants, GUI
//! assets); its constructors load it from a live install or a dump.
//! [`crate::data::wows_data::BuildDataCache`] owns caching and resolution
//! across builds.

use std::collections::HashMap;
use std::fs::File;
use std::fs::read_dir;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use gettext::Catalog;
use language_tags::LanguageTag;
use parking_lot::RwLock;
use rootcause::prelude::*;
use tracing::debug;
use tracing::instrument;
use tracing::warn;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::data::idx;
use wowsunpack::data::idx_vfs::IdxVfs;
use wowsunpack::data::wrappers::mmap::MmapPkgSource;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::task::networking::load_versioned_constants_from_disk_with_fallback;
use crate::task::replays::load_ribbon_icons;
use crate::task::replays::load_ship_icons;
use crate::task::replays::parse_dotted_version;
use crate::task::replays::write_params_override;
use crate::util::game_params::load_game_params;

pub struct GameAsset {
    pub path: String,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for GameAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameAsset").field("path", &self.path).field("data", &"...").finish()
    }
}

pub type SharedBuildData = Arc<RwLock<Box<BuildData>>>;

/// GUI assets for one build: icon maps loaded from the game files. Kept
/// apart from the parsing-facing data so library-shaped consumers never
/// touch them.
#[derive(Default)]
pub struct BuildAssets {
    /// Signal-flag icons, lazy-loaded and cached. Keyed by PCEF name.
    /// Modernization (upgrade) icons, lazy-loaded and cached. Keyed by PCM name.
    /// Captain-skill icons, lazy-loaded and cached. Keyed by skill name.
    /// Consumable icons, lazy-loaded and cached. Keyed by PCY name
    /// (e.g. `"PCY009_CrashCrewPremium"`).
    /// Achievement icons, lazy-loaded and cached. Keyed by achievement name (lowercase).
    /// Subribbon icons keyed by ribbon name (e.g., "ribbon_main_caliber")
    /// Ribbon icons keyed by ribbon name (e.g., "ribbon_main_caliber")
    pub ship_icons: HashMap<Species, Arc<GameAsset>>,
    pub ribbon_icons: HashMap<String, Arc<GameAsset>>,
    pub subribbon_icons: HashMap<String, Arc<GameAsset>>,
    pub achievement_icons: HashMap<String, Arc<GameAsset>>,
    pub consumable_icons: HashMap<String, Arc<GameAsset>>,
    pub crew_skill_icons: HashMap<CrewSkillName, Arc<GameAsset>>,
    pub modernization_icons: HashMap<String, Arc<GameAsset>>,
    pub signal_flag_icons: HashMap<String, Arc<GameAsset>>,
}

pub struct BuildData {
    /// Per-build GUI assets (icons), loaded from this build's VFS.
    pub assets: BuildAssets,
    pub vfs: VfsPath,

    /// We may fail to load game params
    pub game_metadata: Option<Arc<GameMetadataProvider>>,

    /// Cached game constants loaded from game files.
    pub game_constants: Arc<GameConstants>,

    /// Version-matched replay constants (from wows-constants repo).
    pub replay_constants: Arc<RwLock<serde_json::Value>>,

    /// Whether the replay constants are an exact match for this build,
    /// or a fallback from a previous build.
    pub replay_constants_exact_match: bool,

    pub full_version: Option<Version>,
    pub patch_version: usize,

    /// The build number this data was loaded for.
    pub build_number: u32,

    pub replays_dir: PathBuf,

    #[allow(dead_code)]
    pub build_dir: PathBuf,

    /// If this data was loaded from a dump directory (not the live install),
    /// this holds the dump path for translation reloading.
    pub dump_dir: Option<PathBuf>,
}

impl BuildData {
    /// The full `major.minor.patch` game version for this data, if known. The
    /// resolver branches on this (never the build number, which differs across
    /// servers); `None` means use the newest layout with its fallbacks.
    pub fn version(&self) -> Option<&Version> {
        self.full_version.as_ref()
    }

    /// Load a GUI asset by what it is, letting the resolver pick the right path
    /// for this build's version. Returns `None` when the asset isn't present.
    fn load_gui_asset(&self, asset: wowsunpack::game_assets::GuiAsset<'_>) -> Option<Arc<GameAsset>> {
        let resolved = asset.resolve(&self.vfs, self.version())?;
        let path = resolved.as_str().trim_start_matches('/').to_owned();
        let mut data = Vec::new();
        resolved.open_file().ok()?.read_to_end(&mut data).ok()?;
        Some(Arc::new(GameAsset { path, data }))
    }

    /// Look up a cached achievement icon (read-only, no loading).
    pub fn cached_achievement_icon(&self, icon_key: &str) -> Option<Arc<GameAsset>> {
        self.assets.achievement_icons.get(icon_key).cloned()
    }

    /// Load and cache an achievement icon from the game files.
    /// Only call this on a cache miss (when `cached_achievement_icon` returns None).
    pub fn load_achievement_icon(&mut self, icon_key: &str) -> Option<Arc<GameAsset>> {
        // Double-check in case another call populated it
        if let Some(icon) = self.assets.achievement_icons.get(icon_key) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::Achievement(icon_key))?;
        self.assets.achievement_icons.insert(icon_key.to_string(), asset.clone());
        Some(asset)
    }

    /// Look up a cached consumable icon (read-only, no loading).
    pub fn cached_consumable_icon(&self, icon_key: &str) -> Option<Arc<GameAsset>> {
        self.assets.consumable_icons.get(icon_key).cloned()
    }

    /// Load and cache a consumable icon by PCY identifier.
    /// Only call this on a cache miss (when `cached_consumable_icon` returns None).
    pub fn load_consumable_icon(&mut self, icon_key: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.assets.consumable_icons.get(icon_key) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::Consumable(icon_key))?;
        self.assets.consumable_icons.insert(icon_key.to_string(), asset.clone());
        Some(asset)
    }

    /// Look up a cached crew-skill icon (read-only, no loading).
    pub fn cached_crew_skill_icon(&self, name: &CrewSkillName) -> Option<Arc<GameAsset>> {
        self.assets.crew_skill_icons.get(name).cloned()
    }

    /// Load and cache a crew-skill icon by skill name.
    /// Only call this on a cache miss (when `cached_crew_skill_icon` returns None).
    pub fn load_crew_skill_icon(&mut self, name: &CrewSkillName) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.assets.crew_skill_icons.get(name) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::CrewSkill { name })?;
        self.assets.crew_skill_icons.insert(name.clone(), asset.clone());
        Some(asset)
    }

    /// Look up a cached modernization icon (read-only, no loading).
    pub fn cached_modernization_icon(&self, name: &str) -> Option<Arc<GameAsset>> {
        self.assets.modernization_icons.get(name).cloned()
    }

    /// Load and cache a modernization icon by PCM name.
    /// Only call this on a cache miss (when `cached_modernization_icon` returns None).
    pub fn load_modernization_icon(&mut self, name: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.assets.modernization_icons.get(name) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::Modernization(name))?;
        self.assets.modernization_icons.insert(name.to_string(), asset.clone());
        Some(asset)
    }

    /// Look up a cached signal-flag icon (read-only, no loading).
    pub fn cached_signal_flag_icon(&self, name: &str) -> Option<Arc<GameAsset>> {
        self.assets.signal_flag_icons.get(name).cloned()
    }

    /// Load and cache a signal-flag icon by PCEF name.
    /// Only call this on a cache miss (when `cached_signal_flag_icon` returns None).
    pub fn load_signal_flag_icon(&mut self, name: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.assets.signal_flag_icons.get(name) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::SignalFlag(name))?;
        self.assets.signal_flag_icons.insert(name.to_string(), asset.clone());
        Some(asset)
    }

    /// Rebuild this data from scratch after constants have changed.
    /// Retains: build_dir, replays_dir, game_metadata, pkg_loader, file_tree,
    /// full_version, patch_version, build_number.
    /// Regenerates everything else (icons, game_constants, replay_constants, etc.).
    /// Returns `false` if versioned constants could not be found on disk.
    #[instrument(skip(self), fields(build = self.build_number))]
    pub fn rebuild_with_new_constants(&mut self) -> bool {
        use crate::task::load_versioned_constants_from_disk_with_fallback;

        debug!("Rebuilding BuildData for build {}", self.build_number);

        // Reload version-matched replay constants from disk only (no network I/O).
        // If not on disk, use our current constants as fallback (better than failing).
        let (new_replay_constants, exact_match) =
            match load_versioned_constants_from_disk_with_fallback(self.build_number) {
                Some((data, exact)) => (data, exact),
                None => {
                    debug!(
                        "No cached versioned constants for build {} during rebuild, using current constants",
                        self.build_number
                    );
                    (self.replay_constants.read().clone(), false)
                }
            };

        // Rebuild game constants from VFS + new replay constants
        let new_game_constants =
            GameConstants::for_build(Some(&self.vfs), Some(&new_replay_constants), self.full_version);

        // Reload all icons from game files
        let version = self.full_version.as_ref();
        let new_ship_icons = crate::task::load_ship_icons(&self.vfs, version);
        let new_ribbon_icons =
            crate::task::load_ribbon_icons(&self.vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, version);
        let new_subribbon_icons =
            crate::task::load_ribbon_icons(&self.vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, version);

        // Apply all regenerated fields
        self.assets.ship_icons = new_ship_icons;
        self.assets.ribbon_icons = new_ribbon_icons;
        self.assets.subribbon_icons = new_subribbon_icons;
        self.assets.achievement_icons = HashMap::new();
        self.assets.consumable_icons = HashMap::new();
        self.assets.crew_skill_icons = HashMap::new();
        self.assets.modernization_icons = HashMap::new();
        self.assets.signal_flag_icons = HashMap::new();
        self.game_constants = Arc::new(new_game_constants);
        *self.replay_constants.write() = new_replay_constants;
        self.replay_constants_exact_match = exact_match;

        debug!("Rebuild complete for build {}", self.build_number);
        true
    }
}

impl BuildData {
    /// Load game resources for a specific build number. This can be called for any build
    /// that has a directory in `bin/`. Used both at startup (for the latest build) and
    /// lazily when a replay from a different version is loaded.
    #[instrument(skip(fallback_constants))]
    pub fn from_live_install(
        wows_directory: &Path,
        build: u32,
        locale: &str,
        fallback_constants: &serde_json::Value,
        version: Option<Version>,
    ) -> Result<BuildData, Report> {
        let game_patch = build as usize;
        let build_dir = wows_directory.join("bin").join(format!("{build}"));

        debug!("Loading game data for build {}", build);

        // Parse IDX files and build VFS
        let mut idx_files = Vec::new();
        for file in read_dir(build_dir.join("idx")).context("failed to read idx directory")? {
            let file = file.context("failed to read idx directory entry")?;
            if file.file_type().context("failed to get file type for idx entry")?.is_file() {
                let path = file.path();
                let file_data =
                    std::fs::read(&path).context_with(|| format!("failed to read idx file {}", path.display()))?;
                idx_files.push(
                    idx::parse(&file_data).context_with(|| format!("failed to parse idx file {}", path.display()))?,
                );
            }
        }

        let pkgs_path = wows_directory.join("res_packages");
        if !pkgs_path.exists() {
            Err(crate::util::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
                .context("res_packages directory not found")?;
        }

        let pkg_source = MmapPkgSource::new(&pkgs_path);
        let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
        let vfs = VfsPath::new(idx_vfs);

        // Load translations
        // WoWs locale codes use underscores (e.g. "zh_tw", "pt_br") but BCP 47
        // language tags use hyphens. Normalize before parsing.
        let bcp47 = locale.replace('_', "-");
        let primary_lang = bcp47
            .parse::<LanguageTag>()
            .map(|tag| tag.primary_language().to_string())
            .unwrap_or_else(|_| locale.to_string());
        let attempted_dirs = [locale, &primary_lang, "en"];
        let mut found_catalog = None;
        for dir in attempted_dirs {
            let localization_path = wows_directory.join(format!("bin/{build}/res/texts/{dir}/LC_MESSAGES/global.mo"));
            if !localization_path.exists() {
                continue;
            }
            let global = File::open(&localization_path)
                .context_with(|| format!("failed to open localization file {}", localization_path.display()))?;
            let catalog = Catalog::parse(global)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}")))
                .context_with(|| format!("failed to parse localization catalog {}", localization_path.display()))?;
            found_catalog = Some(catalog);
            break;
        }

        debug!("Loading GameParams for build {}", build);
        let metadata_provider = load_game_params(&vfs, game_patch).ok().map(|metadata_provider| {
            if let Some(catalog) = found_catalog {
                metadata_provider.set_translations(catalog)
            }
            Arc::new(metadata_provider)
        });

        debug!("Loading icons for build {}", build);
        // The semantic version isn't resolved at this point for a live install, so
        // the resolver gets `None` and picks the newest layout with its fallbacks.
        let icons = load_ship_icons(&vfs, None);
        let ribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, None);
        let subribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, None);

        // Load version-matched constants from disk cache only (no network I/O).
        // If not cached, use fallback constants. The networking thread will fetch
        // updated constants from GitHub in the background.
        debug!("Loading versioned constants for build {}", build);
        let (replay_constants, replay_constants_exact_match) =
            match load_versioned_constants_from_disk_with_fallback(build) {
                Some((data, exact)) => (data, exact),
                None => (fallback_constants.clone(), false),
            };

        let game_constants = GameConstants::for_build(Some(&vfs), Some(&replay_constants), version);
        let game_constants = Arc::new(game_constants);

        // Try to determine full version from preferences or leave as None for non-latest builds
        let full_version = version; // Set by caller (replay version) when known

        Ok(BuildData {
            game_metadata: metadata_provider,
            vfs,
            patch_version: game_patch,
            full_version,
            build_number: build,
            assets: BuildAssets { ship_icons: icons, ribbon_icons, subribbon_icons, ..Default::default() },
            game_constants,
            replay_constants: Arc::new(RwLock::new(replay_constants)),
            replay_constants_exact_match,
            replays_dir: PathBuf::new(), // Set by caller
            build_dir,
            dump_dir: None,
        })
    }

    pub fn from_dump(
        dump_dir: &Path,
        build: u32,
        locale: &str,
        fallback_constants: &serde_json::Value,
        replay_version: Option<Version>,
    ) -> Result<BuildData, Report> {
        use wowsunpack::game_params::provider::GameMetadataProvider;

        debug!("Loading game data from dump: {}", dump_dir.display());

        let mut cas = wows_data_mgr::cas_vfs::BuildCas::open(dump_dir)
            .ok_or_else(|| report!("metadata.toml not found in dump: {}", dump_dir.display()))?;

        // Build numbers are per-server: the China client ships a different one for
        // the same major.minor.patch, so resolution keys on the friendly version and
        // a near-neighbour dump is expected to serve. Record which dump actually
        // answered, since it is otherwise invisible when reading logs.
        let dump_build = cas.metadata().build;
        if dump_build != build {
            tracing::info!(
                requested_build = build,
                dump_build,
                version = %cas.metadata().version,
                "serving build from a different dump of the same version"
            );
        }

        // Key the override directory on the dump's own build rather than its
        // version: a version does not identify a dump (18 of the 109 versions in
        // the reference archive have more than one build), so a version-keyed cache
        // would let one server's params answer for another's.
        if let Some(root) = crate::util::game_params::build_override_root(dump_build) {
            cas.set_override_root(root);
        }
        let vfs = cas.vfs();

        // Recover the semantic version from the dump's metadata so version-aware
        // asset resolution works. Build numbers differ across servers (e.g. the
        // China client) for the same major.minor.patch, so the version is the
        // server-independent key.
        let full_version = parse_dotted_version(&cas.metadata().version, build);

        // Load translations from dump
        let bcp47 = locale.replace('_', "-");
        let primary_lang = bcp47
            .parse::<LanguageTag>()
            .map(|tag| tag.primary_language().to_string())
            .unwrap_or_else(|_| locale.to_string());
        let attempted_dirs = [locale, &primary_lang, "en"];
        let mut found_catalog = None;
        for dir in attempted_dirs {
            let Some(mo_path) = cas.derived_path(&format!("translations/{dir}/LC_MESSAGES/global.mo")) else {
                continue;
            };
            if let Ok(file) = File::open(&mo_path)
                && let Ok(catalog) = Catalog::parse(file)
            {
                found_catalog = Some(catalog);
                break;
            }
        }

        // Load GameParams: prefer the dump's rkyv cache when it carries a current
        // header, otherwise fall back to re-parsing from the dump's VFS (this
        // happens for caches generated before the current cache format version,
        // including any pre-WUGP-magic dumps in wows-replay-data).
        let rkyv_path = cas.derived_path("game_params.rkyv");
        let cached_params = rkyv_path.as_deref().and_then(wowsunpack::game_params::cache::load);
        let metadata_provider = match cached_params {
            Some(params) => {
                debug!("Loaded GameParams from rkyv cache");
                // Pair the fast rkyv params with entity specs parsed from the dump's
                // VFS (`scripts/entity_defs`). The specs are required to parse replay
                // packets (BasePlayerCreate indexes them by entity type); without them
                // any dump-loaded replay -- i.e. any build not in the live install --
                // panics in the packet parser.
                GameMetadataProvider::from_params_with_vfs(params, &vfs)
                    .map_err(|e| report!("Failed to build GameMetadataProvider from dump: {e:?}"))?
            }
            None => {
                debug!("Falling back to GameParams.data in dump VFS (rkyv missing or stale)");
                let provider = GameMetadataProvider::from_vfs(&vfs)
                    .map_err(|e| report!("Failed to load GameParams from dump VFS: {e:?}"))?;
                write_params_override(&cas, &provider);
                provider
            }
        };
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog);
        }
        let metadata_provider = Some(Arc::new(metadata_provider));

        // Load icons, using the semantic version recovered from the dump if present.
        let version = full_version.as_ref();
        let icons = load_ship_icons(&vfs, version);
        let ribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, version);
        let subribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, version);

        // Load constants: try dump dir first, then disk cache, then fallback
        let (replay_constants, replay_constants_exact_match) = {
            let dump_constants_path = dump_dir.join("constants.json");
            if dump_constants_path.exists() {
                if let Ok(data) = std::fs::read(&dump_constants_path)
                    && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data)
                {
                    (json, true)
                } else {
                    (fallback_constants.clone(), false)
                }
            } else {
                match load_versioned_constants_from_disk_with_fallback(build) {
                    Some((data, exact)) => (data, exact),
                    None => (fallback_constants.clone(), false),
                }
            }
        };
        // Prefer the replay's own version; fall back to the version recovered from the dump.
        let constants_version = replay_version.or(full_version);
        let game_constants = GameConstants::for_build(Some(&vfs), Some(&replay_constants), constants_version);

        Ok(BuildData {
            game_metadata: metadata_provider,
            vfs,
            patch_version: build as usize,
            full_version,
            build_number: build,
            assets: BuildAssets { ship_icons: icons, ribbon_icons, subribbon_icons, ..Default::default() },
            game_constants: Arc::new(game_constants),
            replay_constants: Arc::new(RwLock::new(replay_constants)),
            replay_constants_exact_match,
            replays_dir: PathBuf::new(),
            build_dir: dump_dir.to_path_buf(),
            dump_dir: Some(dump_dir.to_path_buf()),
        })
    }
}

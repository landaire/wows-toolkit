//! Fixture loading for the pipeline benchmarks.
//!
//! Resolves a replay's build the same two ways the app does, in the same order:
//! a dumped-build archive first, then a live game install. Builds available in
//! neither are skipped, so the benchmark degrades to "no cases" on a machine
//! without local game data instead of failing.
//!
//! Paths come from the environment:
//!   WOWS_BUILDS_DIR   dump archive root (default `G:\wows_builds`)
//!   WOWS_DIR          live install root (default `E:\WoWs\World_of_Warships`)
//!   WOWS_BENCH_REPLAYS  directory of replays (default `<WOWS_DIR>\replays`)

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::vfs::VfsPath;

#[derive(Clone, Copy)]
struct BuildResources {
    provider: &'static GameMetadataProvider,
    constants: &'static GameConstants,
}

/// One benchmark case: a replay plus the game data its build needs.
pub struct Case {
    pub name: String,
    /// Raw file bytes, so `from_bytes` can be measured without disk I/O.
    pub bytes: Vec<u8>,
    pub replay: ReplayFile,
    pub provider: &'static GameMetadataProvider,
    pub constants: &'static GameConstants,
    pub specs: &'static [EntitySpec],
    pub version: Version,
}

fn env_path(key: &str, default: &str) -> PathBuf {
    PathBuf::from(std::env::var(key).unwrap_or_else(|_| default.to_string()))
}

fn build_cache() -> &'static Mutex<HashMap<u32, Option<BuildResources>>> {
    static CACHE: OnceLock<Mutex<HashMap<u32, Option<BuildResources>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Game data from the dump archive, via the same `BuildsIndex` lookup the app
/// uses. Returns `None` when the archive has no entry for the build.
fn from_dump(build: u32, version: &Version) -> Option<(VfsPath, Option<PathBuf>)> {
    let base = env_path("WOWS_BUILDS_DIR", r"G:\wows_builds");
    let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
    let version_str = format!("{}.{}.{}", version.major, version.minor, version.patch);
    let (entry, _exact) = index.resolve_build(build, Some(&version_str))?;
    let cas = wows_data_mgr::cas_vfs::BuildCas::open(&base.join(&entry.dir))?;
    let params = cas.derived_path("game_params.rkyv");
    Some((cas.vfs(), params))
}

/// Game data from a live install: an `IdxVfs` over the build's `idx/` plus the
/// app's own params cache in APPDATA, mirroring `load_wows_data_for_build`.
fn from_install(build: u32) -> Option<(VfsPath, Option<PathBuf>)> {
    use wowsunpack::data::idx;
    use wowsunpack::data::idx_vfs::IdxVfs;
    use wowsunpack::data::wrappers::mmap::MmapPkgSource;

    let wows_dir = env_path("WOWS_DIR", r"E:\WoWs\World_of_Warships");
    let idx_dir = wows_dir.join("bin").join(build.to_string()).join("idx");
    if !idx_dir.exists() {
        return None;
    }

    let mut idx_files = Vec::new();
    for entry in std::fs::read_dir(&idx_dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_file() {
            idx_files.push(idx::parse(&std::fs::read(&path).ok()?).ok()?);
        }
    }
    let vfs = VfsPath::new(IdxVfs::new(MmapPkgSource::new(wows_dir.join("res_packages")), &idx_files));

    let params = std::env::var("APPDATA")
        .ok()
        .map(|appdata| {
            PathBuf::from(appdata).join("WoWs Toolkit").join("data").join(format!("game_params_{build}.bin"))
        })
        .filter(|p| p.exists());
    Some((vfs, params))
}

/// Resources for a build, cached (including the negative answer, so a missing
/// build is looked for once).
fn resources_for(version: &Version) -> Option<BuildResources> {
    let build = version.build_number()?;
    if let Some(cached) = build_cache().lock().unwrap().get(&build) {
        return *cached;
    }

    let resolved = from_dump(build, version).or_else(|| from_install(build)).and_then(|(vfs, params_path)| {
        let provider = match params_path.as_deref().and_then(wowsunpack::game_params::cache::load) {
            Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs).ok()?,
            None => GameMetadataProvider::from_vfs(&vfs).ok()?,
        };
        Some(BuildResources {
            provider: Box::leak(Box::new(provider)),
            constants: Box::leak(Box::new(GameConstants::from_vfs(&vfs))),
        })
    });

    if resolved.is_none() {
        eprintln!("no game data for build {build}; skipping its replays");
    }
    build_cache().lock().unwrap().insert(build, resolved);
    resolved
}

fn load_case(path: &Path) -> Option<Case> {
    let bytes = std::fs::read(path).ok()?;
    let replay = ReplayFile::from_bytes(&bytes).ok()?;
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)?;
    let res = resources_for(&version)?;
    Some(Case {
        name: path.file_stem()?.to_string_lossy().into_owned(),
        bytes,
        replay,
        provider: res.provider,
        constants: res.constants,
        specs: res.provider.entity_specs(),
        version,
    })
}

/// Benchmark cases spanning the packet-stream size range: the smallest, the
/// median, and the largest resolvable replay. Three keeps a run quick while
/// still showing how cost scales with stream length.
pub fn cases() -> Vec<Case> {
    let dir = std::env::var("WOWS_BENCH_REPLAYS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env_path("WOWS_DIR", r"E:\WoWs\World_of_Warships").join("replays"));

    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no replay directory at {}", dir.display());
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wowsreplay")))
        .collect();
    paths.sort_by_key(|p| p.metadata().map(|m| m.len()).unwrap_or(0));

    if paths.is_empty() {
        eprintln!("no .wowsreplay files under {}", dir.display());
        return Vec::new();
    }

    let picks = [0, paths.len() / 2, paths.len() - 1];
    let mut seen = Vec::new();
    picks
        .iter()
        .filter(|i| {
            let fresh = !seen.contains(*i);
            seen.push(**i);
            fresh
        })
        .filter_map(|i| load_case(&paths[*i]))
        .collect()
}

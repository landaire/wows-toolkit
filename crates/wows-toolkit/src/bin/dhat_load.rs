//! Headless, single-threaded heap profile of the game-data load. Run under dhat:
//! because this process is single-threaded and quiescent at exit, dhat's
//! Profiler::drop converges and writes dhat-heap.json (unlike the GUI, where
//! concurrent allocation prevents it).
//!
//! Mirrors the live app startup VFS (task/replays.rs): a bare IdxVfs over mmap'd
//! pkgs with NO assets.bin overlay. The app loads assets.bin only on demand for
//! the armor viewer / file browser, so it is deliberately excluded here.
//!
//! Usage: dhat_load [wows_dir] [build]
//!   cargo run --profile profiling --features dhat-heap --bin dhat_load

#[cfg(all(feature = "dhat-heap", not(target_arch = "wasm32")))]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(feature = "dhat-heap")]
fn main() {
    use std::path::Path;
    use std::path::PathBuf;

    use wowsunpack::data::idx;
    use wowsunpack::data::idx_vfs::IdxVfs;
    use wowsunpack::data::wrappers::mmap::MmapPkgSource;
    use wowsunpack::vfs::VfsPath;

    let profiler = dhat::Profiler::builder().trim_backtraces(Some(24)).build();

    let wows_dir = std::env::args().nth(1).unwrap_or_else(|| r"E:\WoWs\World_of_Warships".to_string());
    let build: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(12506899);

    eprintln!("building bare IdxVfs: dir={wows_dir} build={build}");
    let build_dir = Path::new(&wows_dir).join("bin").join(build.to_string());
    let mut idx_files = Vec::new();
    for entry in std::fs::read_dir(build_dir.join("idx")).expect("read idx dir") {
        let path = entry.expect("idx entry").path();
        if path.is_file() {
            let bytes = std::fs::read(&path).expect("read idx file");
            idx_files.push(idx::parse(&bytes).expect("parse idx file"));
        }
    }
    let pkg_source = MmapPkgSource::new(Path::new(&wows_dir).join("res_packages"));
    let vfs = VfsPath::new(IdxVfs::new(pkg_source, &idx_files));

    let after_vfs = dhat::HeapStats::get();
    eprintln!("after vfs build: curr_bytes={} curr_blocks={}", after_vfs.curr_bytes, after_vfs.curr_blocks);

    // The file browser's flat list, built eagerly at startup (task/replays.rs):
    // one (Arc<PathBuf>, VfsPath) per file across all ~416K files.
    let file_map = idx::build_file_tree(&idx_files);
    let filtered_files: Vec<(std::sync::Arc<PathBuf>, VfsPath)> = file_map
        .iter()
        .filter(|(_, e)| matches!(e, wowsunpack::data::idx::VfsEntry::File { .. }))
        .filter_map(|(p, _)| Some((std::sync::Arc::new(PathBuf::from(p)), vfs.join(p).ok()?)))
        .collect();
    drop(file_map);
    let after_filtered = dhat::HeapStats::get();
    eprintln!(
        "after filtered_files ({} entries): curr_bytes={} curr_blocks={}",
        filtered_files.len(),
        after_filtered.curr_bytes,
        after_filtered.curr_blocks
    );

    // The raw idx metadata is transient (the app drops it after construction).
    drop(idx_files);
    std::hint::black_box(&filtered_files);

    let appdata = std::env::var("APPDATA").expect("APPDATA");
    let cache_path = PathBuf::from(appdata).join("WoWs Toolkit").join("data").join(format!("game_params_{build}.bin"));
    eprintln!("loading params cache: {}", cache_path.display());
    let params = wowsunpack::game_params::cache::load(&cache_path).expect("load params cache");
    eprintln!("decoded {} params", params.len());

    let after_params = dhat::HeapStats::get();
    eprintln!("after params decode: curr_bytes={} curr_blocks={}", after_params.curr_bytes, after_params.curr_blocks);

    let provider = wowsunpack::game_params::provider::GameMetadataProvider::from_params_with_vfs(params, &vfs)
        .expect("build provider");

    let after_provider = dhat::HeapStats::get();
    eprintln!(
        "after provider build: curr_bytes={} curr_blocks={}",
        after_provider.curr_bytes, after_provider.curr_blocks
    );

    std::hint::black_box(&provider);
    eprintln!("dropping profiler (writes dhat-heap.json)...");
    drop(profiler);
    eprintln!("done");
}

#[cfg(not(feature = "dhat-heap"))]
fn main() {
    eprintln!("rebuild with --features dhat-heap");
}

//! Headless heap profile of the GameParams *parse* path (cold start, no cache).
//!
//! Where `dhat_load` in wows-toolkit measures the warm rkyv cache decode, this
//! measures the expensive cold path: read `content/GameParams.data`, zlib
//! decompress, unpickle to a `pickled::Value` tree, and convert that tree into
//! the strongly-typed `Vec<Param>` (`params_from_data`). The pickled value tree
//! and the parser memo dominate transient memory, so this is the binary to run
//! when investigating parse-time memory pressure.
//!
//! Because the process is single-threaded and quiescent at exit, dhat's
//! `Profiler::drop` converges and writes dhat-heap.json (open it at
//! <https://nnethercote.github.io/dh_view/dh_view.html> and sort by "at t-gmax"
//! to see what is resident at the global peak).
//!
//! Usage: dhat_parse [wows_dir] [build]
//!   cargo run --profile profiling --features dhat-heap --bin dhat_parse

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(feature = "dhat-heap")]
fn main() {
    use std::io::Read;
    use std::path::Path;

    use vfs::VfsPath;
    use wowsunpack::data::idx;
    use wowsunpack::data::idx_vfs::IdxVfs;
    use wowsunpack::data::wrappers::mmap::MmapPkgSource;
    use wowsunpack::game_params::provider::GameMetadataProvider;

    let profiler = dhat::Profiler::builder().trim_backtraces(Some(32)).build();

    let wows_dir = std::env::args().nth(1).unwrap_or_else(|| r"E:\WoWs\World_of_Warships".to_string());
    let build: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(12506899);

    let stat = |label: &str| {
        let s = dhat::HeapStats::get();
        eprintln!("{label:<28} curr={:>5.1} MiB  peak={:>5.1} MiB  blocks={}", mib(s.curr_bytes), mib(s.max_bytes), s.curr_blocks);
    };

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
    drop(idx_files);
    stat("after vfs build");

    let mut game_params_data = Vec::new();
    vfs.join("content/GameParams.data")
        .expect("join GameParams.data")
        .open_file()
        .expect("open GameParams.data")
        .read_to_end(&mut game_params_data)
        .expect("read GameParams.data");
    eprintln!("GameParams.data: {:.1} MiB compressed", mib(game_params_data.len()));
    stat("after read data");

    // Optional: dump the decompressed raw pickle so it can be fed to pickled's
    // `mem_profile` example for a node/dict breakdown. Set DUMP_PICKLE=<path>.
    if let Ok(out) = std::env::var("DUMP_PICKLE") {
        use flate2::read::ZlibDecoder;
        let mut rev = game_params_data.clone();
        rev.reverse();
        let mut raw = Vec::new();
        ZlibDecoder::new(std::io::Cursor::new(rev)).read_to_end(&mut raw).expect("inflate");
        std::fs::write(&out, &raw).expect("write pickle");
        eprintln!("wrote decompressed pickle ({:.1} MiB) to {out}", mib(raw.len()));
        return;
    }

    // Phase A: unpickle only. Isolates the public Value tree (pickled's internal
    // de-tree + memo are freed when value_from_reader returns) so the staged
    // numbers separate "pickle tree" from "Param vec".
    {
        let pickle = wowsunpack::game_params::convert::game_params_to_pickle(game_params_data.clone())
            .expect("unpickle GameParams");
        stat("after unpickle (value tree)");
        std::hint::black_box(&pickle);
    }
    stat("after drop value tree");

    // Phase B: the real path. params_from_data unpickles again and converts to
    // Vec<Param>; the global peak (t-gmax in the json) lands inside here.
    let params = GameMetadataProvider::params_from_data(game_params_data).expect("parse params");
    eprintln!("parsed {} params", params.len());
    stat("after params_from_data");

    let encoded = wowsunpack::game_params::cache::encode(&params).expect("encode cache");
    eprintln!("rkyv cache: {:.1} MiB", mib(encoded.len()));
    stat("after rkyv encode");

    std::hint::black_box(&params);
    std::hint::black_box(&encoded);
    eprintln!("dropping profiler (writes dhat-heap.json)...");
    drop(profiler);
    eprintln!("done");
}

#[cfg(feature = "dhat-heap")]
fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(not(feature = "dhat-heap"))]
fn main() {
    eprintln!("rebuild with --features dhat-heap");
}

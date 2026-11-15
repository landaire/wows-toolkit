//! Timing of the replay load path, for profiling.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use serde::Serialize;
use wows_replays::ReplayFile;
use wows_replays::context::GameDataContext;
use wowsunpack::data::Version;

/// What [`bench`] measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchMode {
    /// JSON deserialization of preloaded headers into owned types.
    MetaJson,
    /// JSON deserialization of preloaded headers, borrowing.
    MetaJsonRef,
    /// Reading each file's header, borrowing.
    MetaRef,
    /// Reading each file's header into owned types.
    MetaFile,
    /// `ReplayFile::from_file`: read, decrypt and inflate.
    File,
    /// `File` and parsing every packet. Needs game data.
    Full,
}

impl BenchMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::MetaJson => "meta-json",
            Self::MetaJsonRef => "meta-json-ref",
            Self::MetaRef => "meta-ref",
            Self::MetaFile => "meta-file",
            Self::File => "file",
            Self::Full => "full",
        }
    }
}

impl FromStr for BenchMode {
    type Err = UnknownBenchMode;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mode = match text {
            "meta-json" => Self::MetaJson,
            "meta-json-ref" => Self::MetaJsonRef,
            "meta-ref" => Self::MetaRef,
            "meta-file" => Self::MetaFile,
            "file" => Self::File,
            "full" => Self::Full,
            _ => return Err(UnknownBenchMode { mode: text.to_owned() }),
        };

        Ok(mode)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown bench mode: {mode}")]
pub struct UnknownBenchMode {
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BenchReport {
    /// Only the preloaded modes read every header before timing starts.
    pub preload: Option<Preload>,
    pub parsed: usize,
    pub failed: usize,
    pub bytes: usize,

    /// A checksum of the parse results, so they are not optimized out.
    pub sink: usize,
    pub elapsed: Duration,
}

impl BenchReport {
    /// Time per successful parse. `None` when nothing parsed.
    pub fn per_parse(&self) -> Option<Duration> {
        let parsed = u32::try_from(self.parsed).ok().filter(|parsed| *parsed > 0)?;

        Some(self.elapsed / parsed)
    }

    /// Bytes parsed per second, in MiB. `None` when no time passed.
    pub fn throughput_mib_s(&self) -> Option<f64> {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            return None;
        }

        Some(self.bytes as f64 / (1024.0 * 1024.0) / secs)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preload {
    pub blobs: usize,
    pub elapsed: Duration,
}

/// The `.wowsreplay` files under `roots` in file name order, at most `limit`.
pub fn collect_replay_paths(roots: &[PathBuf], limit: Option<usize>) -> Result<Vec<PathBuf>, walkdir::Error> {
    let mut paths = Vec::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
            let entry = entry?;
            if !entry.path().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|e| e.to_str()) != Some("wowsreplay") {
                continue;
            }
            paths.push(entry.path().to_path_buf());
            if let Some(limit) = limit
                && paths.len() >= limit
            {
                return Ok(paths);
            }
        }
    }

    Ok(paths)
}

/// Runs `mode` over `paths` `iterations` times. A replay that fails to load
/// counts as failed.
pub fn bench(mode: BenchMode, iterations: usize, paths: &[PathBuf], ctx: &dyn GameDataContext) -> BenchReport {
    let mut parsed = 0usize;
    let mut failed = 0usize;
    let mut bytes = 0usize;
    let mut sink = 0usize;
    let mut preload = None;

    let mut start = Instant::now();
    match mode {
        BenchMode::MetaJson | BenchMode::MetaJsonRef => {
            let blobs: Vec<Vec<u8>> = paths.iter().filter_map(|p| ReplayFile::read_meta_blob(p).ok()).collect();
            preload = Some(Preload { blobs: blobs.len(), elapsed: start.elapsed() });

            start = Instant::now();
            for _ in 0..iterations {
                for blob in &blobs {
                    let vehicles = if mode == BenchMode::MetaJsonRef {
                        wows_replays::ReplayMetaRef::from_slice(blob).ok().map(|m| m.vehicles.len())
                    } else {
                        serde_json::from_slice::<wows_replays::ReplayMeta>(blob).ok().map(|m| m.vehicles.len())
                    };
                    match vehicles {
                        Some(n) => {
                            parsed += 1;
                            bytes += blob.len();
                            sink = sink.wrapping_add(n);
                        }
                        None => failed += 1,
                    }
                }
            }
        }
        BenchMode::MetaFile => {
            for _ in 0..iterations {
                for path in paths {
                    match ReplayFile::meta_from_file(path) {
                        Ok(meta) => {
                            parsed += 1;
                            bytes += meta.playerName.len();
                            sink = sink.wrapping_add(meta.vehicles.len());
                        }
                        Err(_) => failed += 1,
                    }
                }
            }
        }
        BenchMode::MetaRef => {
            for _ in 0..iterations {
                for path in paths {
                    match ReplayFile::read_meta_blob(path) {
                        Ok(blob) => match wows_replays::ReplayMetaRef::from_slice(&blob) {
                            Ok(meta) => {
                                parsed += 1;
                                bytes += blob.len();
                                sink = sink.wrapping_add(meta.vehicles.len());
                            }
                            Err(_) => failed += 1,
                        },
                        Err(_) => failed += 1,
                    }
                }
            }
        }
        BenchMode::File => {
            for _ in 0..iterations {
                for path in paths {
                    match ReplayFile::from_file(path) {
                        Ok(replay) => {
                            parsed += 1;
                            bytes += replay.packet_data().len();
                            sink = sink.wrapping_add(replay.meta.vehicles.len());
                        }
                        Err(_) => failed += 1,
                    }
                }
            }
        }
        BenchMode::Full => {
            for _ in 0..iterations {
                for path in paths {
                    let Some(packet_bytes) = parse_all_packets(path, ctx, &mut sink) else {
                        failed += 1;
                        continue;
                    };
                    parsed += 1;
                    bytes += packet_bytes;
                }
            }
        }
    }

    BenchReport { preload, parsed, failed, bytes, sink, elapsed: start.elapsed() }
}

/// Reads and parses every packet of the replay at `path`, adding packet
/// sizes to `sink`. The packet data's length, or `None` on any failure.
fn parse_all_packets(path: &std::path::Path, ctx: &dyn GameDataContext, sink: &mut usize) -> Option<usize> {
    let replay = ReplayFile::from_file(path).ok()?;
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)?;
    let specs = ctx.entity_specs(&version).ok()?;

    // The parser still has panic paths on malformed input; treat those as
    // failures, not aborts.
    let mut local_sink = 0usize;
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut parser = wows_replays::packet2::Parser::new(&specs);
        let mut remaining = replay.packet_data();
        while !remaining.is_empty() {
            match parser.parse_packet(&mut remaining) {
                Ok(packet) => local_sink = local_sink.wrapping_add(packet.packet_size as usize),
                Err(_) => return false,
            }
        }
        true
    }))
    .unwrap_or(false);
    *sink = sink.wrapping_add(local_sink);

    ok.then(|| replay.packet_data().len())
}

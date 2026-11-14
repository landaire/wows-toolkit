//! replayshark's command line: the argument definitions and their dispatch,
//! shared by the `replayshark` binary and `wows_toolkit replayshark`.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use clap::Subcommand;
use rootcause::prelude::*;
use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::context::CachedContext;
use wows_replays::context::GameDataContext;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

use crate::battle_results::ResultsFormat;
use crate::error::CommandError;
use crate::game_data::CliGameDataContext;
use crate::game_data::versioned_constants;

// No doc comment: clap would take it as the about text of whichever command
// flattens this, replacing that command's own.
#[derive(Clone, Debug, clap::Args)]
pub struct Args {
    #[command(flatten)]
    pub game_data: GameDataArgs,

    #[command(subcommand)]
    pub command: Command,
}

// Where game data comes from, for every command. No doc comment, as above.
#[derive(Clone, Debug, clap::Args)]
pub struct GameDataArgs {
    /// Path to your game directory (e.g. E:\WoWs\World_of_Warships\)
    #[arg(short = 'g', long = "game")]
    pub game_dir: Option<PathBuf>,

    /// Path to extracted game files
    #[arg(short = 'e', long = "extracted")]
    pub extracted_dir: Option<PathBuf>,

    /// Path to a constants JSON file (from wows-constants repo) to override
    /// consumable IDs, battle stages, etc.
    #[arg(short = 'c', long = "constants")]
    pub constants: Option<PathBuf>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    /// Runs the parser against a directory of replays to validate the parser
    Survey {
        /// Don't run the decoder
        #[arg(long)]
        skip_decode: bool,

        /// The replay files to use
        #[arg(required = true)]
        replays: Vec<PathBuf>,
    },
    /// Print the chat log of the given game
    Chat {
        /// The replay file to use
        replay: PathBuf,
    },
    /// Generate summary statistics of the game
    Summary {
        /// The replay file to use
        replay: PathBuf,
    },
    /// Dump the packets to console
    Dump {
        /// Output filename to dump to
        #[arg(short, long)]
        output: Option<String>,

        /// Don't output the metadata as first line
        #[arg(long)]
        no_meta: bool,

        /// The replay file to use
        replay: PathBuf,
    },
    /// Dump the scripts specifications to console
    Spec {
        /// Version to dump. Must be comma-delimited: major,minor,patch,build
        version: String,
    },
    /// Audit which game def semantic type names have domain newtypes.
    AuditTypes {
        /// Version to load. Comma-delimited: major,minor,patch,build
        version: String,
    },
    /// Decrypt a replay file and dump the meta and packet data to disk
    Decrypt {
        /// Output path for the JSON metadata
        #[arg(short = 'm', long)]
        meta_output: PathBuf,

        /// Output path for the decrypted packet data
        #[arg(short = 'p', long)]
        packets_output: PathBuf,

        /// The replay file to use
        replay: PathBuf,
    },
    /// Search a directory full of replays
    Search {
        /// The replay files to use
        #[arg(required = true)]
        replays: Vec<PathBuf>,
    },
    /// Extract specific fields from a replay or batch of replays
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },
    /// Benchmark replay parsing (for profiling)
    Bench {
        /// What to measure: meta-json / meta-json-ref (JSON deserialize only,
        /// owned vs zero-copy, preloaded), meta-ref (zero-copy meta per file),
        /// meta-file (owned meta per file), file
        /// (ReplayFile::from_file: read + decrypt + inflate), full
        /// (file + packet parse; requires game data)
        #[arg(long, default_value = "file")]
        mode: String,

        /// Number of passes over the replay set
        #[arg(long, default_value_t = 1)]
        iterations: usize,

        /// Max number of replay files to use
        #[arg(long)]
        limit: Option<usize>,

        /// The replay files/directories to use
        #[arg(required = true)]
        replays: Vec<PathBuf>,
    },
    /// Tools designed for reverse-engineering packets
    Investigate {
        /// Output the metadata as first line
        #[arg(long)]
        meta: bool,

        /// hh:mm:ss offset to render clock values with
        #[arg(long)]
        timestamp: Option<String>,

        /// If specified, only return packets of the given packet_type
        #[arg(long)]
        filter_packet: Option<String>,

        /// If specified, only return method calls for the given method
        #[arg(long)]
        filter_method: Option<String>,

        /// Entity ID to apply to other filters if applicable
        #[arg(long)]
        entity_id: Option<String>,

        /// The replay file to use
        replay: PathBuf,
    },
    /// Dump battle results (raw, merged, or normalized) for one or more replays.
    BattleResults {
        #[arg(long, value_enum, default_value_t = ResultsFormat::Normalized)]
        format: ResultsFormat,
        /// Write one auto-named <stem>.json per replay into this directory.
        #[arg(long, conflicts_with = "out_file")]
        out_dir: Option<PathBuf>,
        /// Write a single JSON file (object for one replay, array for many).
        #[arg(long, conflicts_with = "out_dir")]
        out_file: Option<PathBuf>,
        /// Use nearest-available constants when the exact build is missing (merged/normalized).
        #[arg(long)]
        allow_approximate_constants: bool,
        /// Optional PR expected-values JSON to populate personal_rating (normalized only).
        #[arg(long)]
        pr_expected_values: Option<PathBuf>,
        #[arg(required = true)]
        replays: Vec<PathBuf>,
    },
    /// Print the match, players with results, kills and chat as JSON.
    Analyze {
        /// The replay file to use
        replay: PathBuf,
    },
    /// Print decoded packets as JSON lines, one object per packet.
    Packets {
        /// Payload kind to keep, such as `Chat` or `DamageReceived`. Repeatable.
        #[arg(long = "kind")]
        kinds: Vec<String>,

        /// Keep packets that name this entity id.
        #[arg(long)]
        entity_id: Option<u32>,

        /// Earliest game clock in seconds since the recording started.
        #[arg(long)]
        from: Option<f32>,

        /// Latest game clock in seconds since the recording started.
        #[arg(long)]
        to: Option<f32>,

        /// The replay file to use
        replay: PathBuf,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum QueryCommand {
    /// Print the arena id of each replay (one line per file). Useful for
    /// pairing up replays from the same match: every player recording the
    /// same battle observes the same arena id. Files that fail to parse or
    /// never reach the arena-state packet print `-`.
    ArenaId {
        /// Replay files or directories to walk
        #[arg(required = true)]
        replays: Vec<PathBuf>,
    },
    /// Print the players in a replay along with their in-replay ship entity id,
    /// ship param id, and localized ship name. Resolves player name <-> entity
    /// id <-> ship in any direction via the filter flags. Requires game data
    /// (`-g` or `-e`) to resolve ship names.
    Players {
        /// The replay file to use
        replay: PathBuf,

        /// Only show players whose name contains this (case-insensitive)
        #[arg(long)]
        name: Option<String>,

        /// Only show the player owning this ship entity id
        #[arg(long)]
        entity_id: Option<u32>,

        /// Only show players whose localized ship name contains this (case-insensitive)
        #[arg(long)]
        ship: Option<String>,

        /// Emit newline-delimited JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Print the game version of each replay (one line per file) as
    /// `version<TAB>path`, where version is `major.minor.patch.build`. The
    /// version is read from the plaintext metadata header, so this needs no
    /// game data and works even when a replay's packet stream is corrupt.
    /// Files that fail to parse print `-`.
    GameVersion {
        /// Replay files or directories to walk
        #[arg(required = true)]
        replays: Vec<PathBuf>,
    },
    /// Print each replay's roster by ship class, resolved from the plaintext
    /// metadata header. Unlike `players` this never touches the packet stream,
    /// so it is fast enough to sweep a large replay archive. Requires game data
    /// (`-g` or `-e`) to map ship ids to a class.
    Roster {
        /// Replay files or directories to walk
        #[arg(required = true)]
        replays: Vec<PathBuf>,

        /// Only print replays whose roster contains this class (case-insensitive,
        /// e.g. `Submarine`). Repeatable.
        #[arg(long = "has-class")]
        has_class: Vec<String>,

        /// Only print replays where the recording player's own ship is NOT one
        /// of the `--has-class` classes. Use with `--has-class Submarine` to
        /// find matches where someone else brought the submarine.
        #[arg(long)]
        exclude_own: bool,

        /// Emit newline-delimited JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

struct InvestigativePrinter {
    filter_packet: Option<u32>,
    filter_method: Option<String>,
    timestamp: Option<f32>,
    entity_id: Option<EntityId>,
    meta: bool,
    packet_decoder: wows_replays::analyzer::decoder::PacketDecoder<'static>,
}

impl wows_replays::analyzer::Analyzer for InvestigativePrinter {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &wows_replays::packet2::Packet<'_, '_>) {
        let decoded = self.packet_decoder.decode(packet);

        if self.meta {
            match &decoded.payload {
                wows_replays::analyzer::decoder::DecodedPacketPayload::OnArenaStateReceived {
                    player_states: players,
                    bot_states: bots,
                    ..
                } => {
                    for player in players.iter().chain(bots.iter()) {
                        match player.player_id() {
                            Some(player_id) => println!(
                                "{} {} ({:x?})",
                                player.username(),
                                player_id,
                                (player_id.raw() as u32).to_le_bytes(),
                            ),
                            None => println!("{} (no player id)", player.username()),
                        }
                    }
                }
                _ => {
                    // Nop
                }
            }
        }

        if let Some(n) = self.filter_packet
            && n != decoded.packet_type.raw()
        {
            return;
        }
        if let Some(s) = self.filter_method.as_ref() {
            match &packet.payload {
                wows_replays::packet2::PacketType::EntityMethod(method) => {
                    if method.method != s {
                        return;
                    }
                    if let Some(eid) = self.entity_id
                        && method.entity_id != eid
                    {
                        return;
                    }
                }
                _ => {
                    return;
                }
            }
        }
        if let Some(t) = self.timestamp {
            let clock = (decoded.clock.seconds() + t) as u32;
            let s = clock % 60;
            let clock = (clock - s) / 60;
            let m = clock % 60;
            let clock = (clock - m) / 60;
            let h = clock;
            let encoded = if self.filter_method.is_some() {
                match &packet.payload {
                    wows_replays::packet2::PacketType::EntityMethod(method) => serde_json::to_string(&method).unwrap(),
                    _ => panic!(),
                }
            } else if self.filter_packet.is_some() {
                match &packet.payload {
                    wows_replays::packet2::PacketType::Unknown(x) => {
                        let v: Vec<_> = x.iter().map(|n| format!("{:02x}", n)).collect();
                        format!("0x[{}]", v.join(","))
                    }
                    _ => serde_json::to_string(&packet).unwrap(),
                }
            } else {
                serde_json::to_string(&decoded).unwrap()
            };
            println!("{:02}:{:02}:{:02}: {}", h, m, s, encoded);
        } else {
            let encoded = serde_json::to_string(&decoded).unwrap();
            println!("{}", encoded);
        }
    }
}

fn build_investigative_printer(
    meta: &wows_replays::ReplayMeta,
    no_meta: bool,
    filter_packet: Option<&str>,
    filter_method: Option<&str>,
    timestamp: Option<&str>,
    entity_id: Option<&str>,
    game_constants: &'static GameConstants,
) -> Box<dyn Analyzer> {
    let version = Version::from_client_exe(&meta.clientVersionFromExe);
    let decoder = InvestigativePrinter {
        packet_decoder: wows_replays::analyzer::decoder::PacketDecoder::builder()
            .version(version)
            .audit(true)
            .battle_constants(game_constants.battle())
            .common_constants(game_constants.common())
            .ships_constants(game_constants.ships())
            .build(),
        filter_packet: filter_packet.map(|s| parse_int::parse::<u32>(s).unwrap()),
        filter_method: filter_method.map(|s| s.to_string()),
        timestamp: timestamp.map(|s| {
            let ts_parts: Vec<_> = s.split("+").collect();
            let offset = ts_parts[1].parse::<u32>().unwrap();
            let parts: Vec<_> = ts_parts[0].split(":").collect();
            if parts.len() == 3 {
                let h = parts[0].parse::<u32>().unwrap();
                let m = parts[1].parse::<u32>().unwrap();
                let s = parts[2].parse::<u32>().unwrap();
                (h * 3600 + m * 60 + s) as f32 - offset as f32
            } else {
                panic!("Expected hh:mm:ss+offset as timestamp");
            }
        }),
        entity_id: entity_id.map(|s| EntityId::from(parse_int::parse::<u32>(s).unwrap())),
        meta: !no_meta,
    };
    if !no_meta {
        println!("{}", serde_json::to_string(&meta).unwrap());
    }
    Box::new(decoder)
}

fn audit_types(specs: &[EntitySpec]) {
    use std::collections::BTreeSet;
    use wowsunpack::rpc::newtype_registry::newtype_for;

    let mut names: BTreeSet<String> = BTreeSet::new();
    for spec in specs {
        for p in spec.properties.iter().chain(&spec.internal_properties).chain(&spec.base_properties) {
            p.prop_type.collect_semantic_names(&mut names);
        }
        for m in spec.client_methods.iter().chain(&spec.base_methods).chain(&spec.cell_methods) {
            for arg in &m.args {
                arg.collect_semantic_names(&mut names);
            }
        }
    }

    let mut covered = 0usize;
    println!("{:<40} {:<10} RUST TYPE", "DEF NAME", "NEWTYPE?");
    for name in &names {
        match newtype_for(name) {
            Some(nt) => {
                covered += 1;
                println!("{:<40} {:<10} {}", name, "yes", nt.rust_type_name());
            }
            None => println!("{:<40} {:<10} -", name, "no"),
        }
    }
    println!("\n{} of {} semantic names have a domain newtype", covered, names.len());
}

fn parse_replay<F>(replay: &std::path::Path, ctx: &dyn GameDataContext, build: F) -> rootcause::Result<(), ParseError>
where
    F: FnOnce(&wows_replays::ReplayMeta) -> Box<dyn Analyzer>,
{
    let replay_file = ReplayFile::from_file(replay)?;

    let replay_version = Version::from_client_exe(replay_file.meta.clientVersionFromExe.as_str());
    let specs = ctx.entity_specs(&replay_version).expect("failed to load game specs");

    let mut analyzer = build(&replay_file.meta);

    let mut parser = wows_replays::packet2::Parser::with_version(&specs, replay_version);
    let mut remaining = replay_file.packet_data();
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).map_err(|e| rootcause::report!(ParseError::from(e)))?;
        analyzer.process(&packet);
    }
    let diagnostics = parser.drain_diagnostics();
    if !diagnostics.is_empty() {
        eprintln!("payload validation: {} property payload(s) under-consumed", diagnostics.len());
        for d in &diagnostics {
            eprintln!(
                "  {} (def {:?}): consumed {} of {} bytes",
                d.context, d.semantic_name, d.consumed, d.payload_len
            );
        }
    }
    analyzer.finish();
    Ok(())
}

fn truncate_string(s: &str, length: usize) -> &str {
    match s.char_indices().nth(length) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

fn printspecs(specs: &[wowsunpack::rpc::entitydefs::EntitySpec]) {
    println!("Have {} entities", specs.len());
    for entity in specs.iter() {
        println!();
        println!(
            "{} has {} properties ({} internal) and {}/{}/{} base/cell/client methods",
            entity.name,
            entity.properties.len(),
            entity.internal_properties.len(),
            entity.base_methods.len(),
            entity.cell_methods.len(),
            entity.client_methods.len()
        );

        println!("Properties:");
        for (i, property) in entity.properties.iter().enumerate() {
            println!(" - {}: {} flag={:?} type={:?}", i, property.name, property.flags, property.prop_type);
        }
        println!("Internal properties:");
        for (i, property) in entity.internal_properties.iter().enumerate() {
            println!(" - {}: {} type={:?}", i, property.name, property.prop_type);
        }
        println!("Client methods:");
        for (i, method) in entity.client_methods.iter().enumerate() {
            println!(" - {}: {}", i, method.name);
            for arg in method.args.iter() {
                println!("      - {:?}", arg);
            }
        }
    }
}

enum SurveyResult {
    /// npackets, ninvalid
    Success((String, String, usize, usize, Vec<String>)),
    UnsupportedVersion(String),
    ParseFailure(String),
}

struct SurveyResults {
    version_failures: usize,
    parse_failures: usize,
    successes: usize,
    successes_with_invalids: usize,
    total: usize,
    invalid_versions: HashMap<String, usize>,
    audits: HashMap<String, (String, Vec<String>)>,
}

impl SurveyResults {
    fn empty() -> Self {
        Self {
            version_failures: 0,
            parse_failures: 0,
            successes: 0,
            successes_with_invalids: 0,
            total: 0,
            invalid_versions: HashMap::new(),
            audits: HashMap::new(),
        }
    }

    fn add(&mut self, result: SurveyResult) {
        self.total += 1;
        match result {
            SurveyResult::Success((hash, datetime, _npacks, ninvalid, audits)) => {
                self.successes += 1;
                if ninvalid > 0 {
                    self.successes_with_invalids += 1;
                }
                if !audits.is_empty() {
                    self.audits.insert(hash, (datetime, audits));
                }
            }
            SurveyResult::UnsupportedVersion(version) => {
                self.version_failures += 1;
                if !self.invalid_versions.contains_key(&version) {
                    self.invalid_versions.insert(version.clone(), 0);
                }
                *self.invalid_versions.get_mut(&version).unwrap() += 1;
            }
            SurveyResult::ParseFailure(_error) => {
                self.parse_failures += 1;
            }
        }
    }

    fn print(&self) {
        let mut audits: Vec<_> = self.audits.iter().collect();
        audits.sort_by_key(|(_, (tm, _))| jiff::civil::DateTime::strptime("%d.%m.%Y %H:%M:%S", tm).unwrap());
        for (k, (tm, v)) in audits.iter() {
            println!();
            println!("{} ({}) has {} audits:", truncate_string(k, 20), tm, v.len());
            for (cnt, audit) in v.iter().enumerate() {
                if cnt >= 10 {
                    println!("...truncating");
                    break;
                }
                println!(" - {}", audit);
            }
        }
        println!();
        println!("Found {} replay files", self.total);
        println!("- {} ({:.0}%) were parsed", self.successes, 100. * self.successes as f64 / self.total as f64);
        println!(
            "  - Of which {} ({:.0}%) contained invalid packets",
            self.successes_with_invalids,
            100. * self.successes_with_invalids as f64 / self.successes as f64
        );
        println!(
            "- {} ({:.0}%) had a parse error",
            self.parse_failures,
            100. * self.parse_failures as f64 / self.total as f64
        );
        println!(
            "- {} ({:.0}%) are an unrecognized version",
            self.version_failures,
            100. * self.version_failures as f64 / self.total as f64
        );
        if !self.invalid_versions.is_empty() {
            for (k, v) in self.invalid_versions.iter() {
                println!("  - Version {} appeared {} times", k, v);
            }
        }
    }
}

fn survey_file(skip_decode: bool, ctx: &dyn GameDataContext, replay: std::path::PathBuf) -> SurveyResult {
    let filename = replay.file_name().unwrap().to_str().unwrap();
    let filename = filename.to_string();

    print!("Parsing {}: ", truncate_string(&filename, 20));
    std::io::stdout().flush().unwrap();

    let survey_stats = std::rc::Rc::new(std::cell::RefCell::new(wows_replays::analyzer::survey::SurveyStats::new()));
    let stats_clone = survey_stats.clone();
    match parse_replay(&replay, ctx, |meta| {
        let version = Version::from_client_exe(&meta.clientVersionFromExe);
        wows_replays::analyzer::survey::SurveyBuilder::new(stats_clone, skip_decode)
            .game_constants(static_game_constants(ctx, &version))
            .build(meta)
    }) {
        Ok(_) => {
            let stats = survey_stats.borrow();
            if stats.invalid_packets > 0 {
                println!("OK ({} packets, {} invalid)", stats.total_packets, stats.invalid_packets);
            } else {
                println!("OK ({} packets)", stats.total_packets);
            }
            SurveyResult::Success((
                filename.to_string(),
                stats.date_time.clone(),
                stats.total_packets,
                stats.invalid_packets,
                stats.audits.clone(),
            ))
        }
        Err(ref e) if matches!(e.current_context(), ParseError::UnsupportedReplayVersion { .. }) => {
            if let ParseError::UnsupportedReplayVersion { version } = e.current_context() {
                println!("Unsupported version {}", version);
                SurveyResult::UnsupportedVersion(version.clone())
            } else {
                unreachable!()
            }
        }
        Err(e) => {
            println!("Parse error: {:?}", e);
            SurveyResult::ParseFailure(format!("{:?}", e))
        }
    }
}

fn collect_replay_paths(replays: &[PathBuf], limit: Option<usize>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for replay_path in replays {
        for entry in walkdir::WalkDir::new(replay_path).sort_by_file_name() {
            let entry = entry.expect("Error unwrapping entry");
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
                return paths;
            }
        }
    }
    paths
}

fn run_bench(
    mode: &str,
    iterations: usize,
    limit: Option<usize>,
    replays: &[PathBuf],
    ctx: &dyn GameDataContext,
) -> Result<(), Report<CommandError>> {
    let paths = collect_replay_paths(replays, limit);
    println!("bench mode={} files={} iterations={}", mode, paths.len(), iterations);

    let mut parsed = 0usize;
    let mut failed = 0usize;
    let mut bytes = 0usize;
    // Checksum accumulator so parse results are observable and not optimized out.
    let mut sink = 0usize;

    let start = std::time::Instant::now();
    match mode {
        "meta-json" | "meta-json-ref" => {
            let blobs: Vec<Vec<u8>> = paths.iter().filter_map(|p| ReplayFile::read_meta_blob(p).ok()).collect();
            let preload_elapsed = start.elapsed();
            println!("preloaded {} metadata blobs in {:?}", blobs.len(), preload_elapsed);
            let zero_copy = mode == "meta-json-ref";
            let start = std::time::Instant::now();
            for _ in 0..iterations {
                for blob in &blobs {
                    let vehicles = if zero_copy {
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
            report_bench(parsed, failed, bytes, sink, start.elapsed());
            return Ok(());
        }
        "meta-file" => {
            for _ in 0..iterations {
                for path in &paths {
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
        "meta-ref" => {
            for _ in 0..iterations {
                for path in &paths {
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
        "file" => {
            for _ in 0..iterations {
                for path in &paths {
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
        "full" => {
            for _ in 0..iterations {
                for path in &paths {
                    let Ok(replay) = ReplayFile::from_file(path) else {
                        failed += 1;
                        continue;
                    };
                    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
                    let Ok(specs) = ctx.entity_specs(&version) else {
                        failed += 1;
                        continue;
                    };
                    let specs_ref = &*specs;
                    // The parser still has panic paths on malformed input;
                    // treat those as failures, not aborts.
                    let mut local_sink = 0usize;
                    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut parser = wows_replays::packet2::Parser::new(specs_ref);
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
                    sink = sink.wrapping_add(local_sink);
                    if ok {
                        parsed += 1;
                        bytes += replay.packet_data().len();
                    } else {
                        failed += 1;
                    }
                }
            }
        }
        other => {
            return Err(report!(CommandError::UnknownBenchMode { mode: other.to_owned() }));
        }
    }
    report_bench(parsed, failed, bytes, sink, start.elapsed());

    Ok(())
}

fn report_bench(parsed: usize, failed: usize, bytes: usize, sink: usize, elapsed: std::time::Duration) {
    let secs = elapsed.as_secs_f64();
    let per_parse_us = if parsed > 0 { secs * 1e6 / parsed as f64 } else { 0.0 };
    let mib_s = if secs > 0.0 { bytes as f64 / (1024.0 * 1024.0) / secs } else { 0.0 };
    println!(
        "parsed={} failed={} bytes={} elapsed={:?} per_parse={:.1}us throughput={:.1}MiB/s sink={}",
        parsed, failed, bytes, elapsed, per_parse_us, mib_s, sink
    );
}


/// `version`'s decode constants, for analyzers that borrow them for `'static`.
///
/// Each build's set is leaked once and reused, so a survey of many replays
/// holds one copy per build rather than one per replay.
fn static_game_constants(ctx: &dyn GameDataContext, version: &Version) -> &'static GameConstants {
    static LEAKED: std::sync::LazyLock<std::sync::Mutex<HashMap<Version, &'static GameConstants>>> =
        std::sync::LazyLock::new(Default::default);

    let mut leaked = LEAKED.lock().expect("constants cache poisoned");
    if let Some(constants) = leaked.get(version).copied() {
        return constants;
    }

    let constants = ctx.game_constants(version).expect("failed to load game constants");
    let constants: &'static std::sync::Arc<GameConstants> = Box::leak(Box::new(constants));
    leaked.insert(*version, constants);

    constants
}

/// Resolve every replay's roster to ship classes using only the plaintext
/// metadata header, so a large archive can be swept without decoding packets.
fn run_roster_query(
    replays: &[PathBuf],
    ctx: &dyn GameDataContext,
    has_class: &[String],
    exclude_own: bool,
    as_json: bool,
) -> rootcause::Result<()> {
    let wanted: Vec<String> = has_class.iter().map(|c| c.to_lowercase()).collect();

    for replay_path in replays {
        for entry in walkdir::WalkDir::new(replay_path) {
            let entry = entry.expect("Error walking replays");
            let path = entry.path();
            if !path.is_file() || path.extension().is_none_or(|e| e != "wowsreplay") {
                continue;
            }
            let Ok(meta) = ReplayFile::meta_from_file(path) else { continue };
            let Some(version) = Version::try_from_client_exe(&meta.clientVersionFromExe) else { continue };

            // Loading game params is the expensive step; the context caches
            // one provider per version across the whole sweep.
            let Ok(provider) = ctx.metadata_provider(&version) else { continue };

            // A ship id absent from this version's params (removed test ship,
            // partial dump) has no class to report, so it is grouped as such
            // rather than silently dropped from the roster.
            let class_of = |ship_id| {
                provider
                    .game_param_by_id(ship_id)
                    .and_then(|p| p.species().and_then(|s| s.known().map(|s| format!("{s:?}"))))
                    .unwrap_or_else(|| "Unknown".to_string())
            };

            let mut counts: HashMap<String, usize> = HashMap::new();
            for vehicle in &meta.vehicles {
                *counts.entry(class_of(vehicle.shipId)).or_default() += 1;
            }
            let own_class = meta.vehicles.iter().find(|v| v.name == meta.playerName).map(|v| class_of(v.shipId));

            if !wanted.is_empty() {
                let present = counts.keys().any(|c| wanted.contains(&c.to_lowercase()));
                if !present {
                    continue;
                }
                if exclude_own && own_class.as_ref().is_some_and(|c| wanted.contains(&c.to_lowercase())) {
                    continue;
                }
            }

            let mut classes: Vec<(&String, &usize)> = counts.iter().collect();
            classes.sort();
            if as_json {
                let value = serde_json::json!({
                    "path": path.display().to_string(),
                    "version": meta.clientVersionFromExe,
                    "own_class": own_class,
                    "classes": counts,
                });
                println!("{value}");
            } else {
                let rendered: Vec<String> = classes.iter().map(|(c, n)| format!("{c}={n}")).collect();
                println!(
                    "{}\t{}\t{}\t{}",
                    meta.clientVersionFromExe,
                    own_class.as_deref().unwrap_or("-"),
                    rendered.join(","),
                    path.display()
                );
            }
        }
    }
    Ok(())
}

/// A resolved player row for the `query players` command.
struct PlayerRow {
    player_name: String,
    entity_id: u32,
    avatar_id: Option<u32>,
    account_id: i64,
    ship_id: u64,
    ship_index: String,
    ship_name: String,
    team_id: i64,
    relation: &'static str,
    is_bot: bool,
}

/// Resolve every player in a replay to their in-replay ship entity id, ship
/// param id, and localized ship name, applying the requested filters.
fn run_players_query(
    replay: &Path,
    ctx: &dyn GameDataContext,
    name_filter: Option<&str>,
    entity_filter: Option<u32>,
    ship_filter: Option<&str>,
    as_json: bool,
) -> rootcause::Result<()> {
    let replay_file = ReplayFile::from_file(replay).map_err(|e| report!("failed to read replay: {e:?}"))?;
    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let provider = ctx.metadata_provider(&version)?;
    let report = wows_battle_world::process::battle_report_for(
        &replay_file,
        ctx,
        wows_battle_world::process::ProcessOptions::default(),
    )
    .map_err(|e| report!("failed to process replay: {e}"))?;

    let name_lc = name_filter.map(str::to_lowercase);
    let ship_lc = ship_filter.map(str::to_lowercase);

    let mut rows: Vec<PlayerRow> = Vec::new();
    for player in report.players() {
        let state = player.initial_state();
        let vehicle = player.vehicle();
        let ship_name = provider.localized_name_from_param(vehicle).unwrap_or_else(|| vehicle.index().to_string());

        if let Some(n) = &name_lc
            && !state.username().to_lowercase().contains(n)
        {
            continue;
        }
        if let Some(eid) = entity_filter
            && state.entity_id().raw() != eid
        {
            continue;
        }
        if let Some(s) = &ship_lc
            && !ship_name.to_lowercase().contains(s)
        {
            continue;
        }

        rows.push(PlayerRow {
            player_name: state.username().to_string(),
            entity_id: state.entity_id().raw(),
            avatar_id: state.avatar_id().map(|a| a.raw()),
            account_id: state.db_id().raw(),
            ship_id: vehicle.id().raw(),
            ship_index: vehicle.index().to_string(),
            ship_name,
            team_id: state.team_id(),
            relation: player.relation().name(),
            is_bot: player.is_bot(),
        });
    }

    rows.sort_by(|a, b| {
        a.team_id.cmp(&b.team_id).then_with(|| a.player_name.to_lowercase().cmp(&b.player_name.to_lowercase()))
    });

    if as_json {
        for r in &rows {
            let value = serde_json::json!({
                "player_name": r.player_name,
                "entity_id": r.entity_id,
                "avatar_id": r.avatar_id,
                "account_id": r.account_id,
                "ship_id": r.ship_id,
                "ship_index": r.ship_index,
                "ship_name": r.ship_name,
                "team_id": r.team_id,
                "relation": r.relation,
                "is_bot": r.is_bot,
            });
            println!("{value}");
        }
    } else {
        println!("{:<9} {:<20} {:<4} {:<6} {:<22} {:<11} BOT", "ENTITY", "PLAYER", "TEAM", "REL", "SHIP", "SHIP_ID");
        for r in &rows {
            println!(
                "{:<9} {:<20} {:<4} {:<6} {:<22} {:<11} {}",
                r.entity_id,
                truncate_string(&r.player_name, 20),
                r.team_id,
                r.relation,
                truncate_string(&r.ship_name, 22),
                r.ship_id,
                if r.is_bot { "bot" } else { "" }
            );
        }
    }

    Ok(())
}

/// Runs one replayshark command, writing its output to stdout.
pub fn run(args: Args) -> Result<(), Report<CommandError>> {
    let game_dir = args.game_data.game_dir.as_deref();
    let extracted = args.game_data.extracted_dir.as_deref();
    let constants_path = args.game_data.constants.as_deref();

    // Per-version caching so directory-scale commands (survey, query,
    // battle-results) pay each build's load once instead of once per replay.
    let ctx = CachedContext::new(CliGameDataContext::new(game_dir, extracted, constants_path));
    let ctx: &dyn GameDataContext = &ctx;

    match args.command {
        Command::Dump { output, no_meta, replay } => {
            let replay_file = read_replay(&replay)?;
            let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
            let gc = static_game_constants(ctx, &version);
            parse_replay(&replay, ctx, |meta| {
                wows_replays::analyzer::decoder::DecoderBuilder::new(false, no_meta, output.as_deref())
                    .game_constants(gc)
                    .build(meta)
            })
            .context(CommandError::ParseReplay { path: replay.clone() })?;
        }
        Command::Investigate { meta, timestamp, filter_packet, filter_method, entity_id, replay } => {
            let replay_file = read_replay(&replay)?;
            let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
            let gc = static_game_constants(ctx, &version);
            let no_meta = !meta;
            parse_replay(&replay, ctx, |meta| {
                build_investigative_printer(
                    meta,
                    no_meta,
                    filter_packet.as_deref(),
                    filter_method.as_deref(),
                    timestamp.as_deref(),
                    entity_id.as_deref(),
                    gc,
                )
            })
            .context(CommandError::ParseReplay { path: replay.clone() })?;
        }
        Command::Spec { version } => {
            let target_version = Version::from_client_exe(&version);
            let specs =
                ctx.entity_specs(&target_version).context(CommandError::GameData { version: target_version })?;
            printspecs(&specs);
        }
        Command::AuditTypes { version } => {
            let target_version = Version::from_client_exe(&version);
            let specs =
                ctx.entity_specs(&target_version).context(CommandError::GameData { version: target_version })?;
            audit_types(&specs);
        }
        Command::Decrypt { meta_output, packets_output, replay } => {
            let replay_file = read_replay(&replay)?;
            std::fs::write(&meta_output, &replay_file.raw_meta)
                .context(CommandError::Write { path: meta_output.clone() })?;
            std::fs::write(&packets_output, replay_file.packet_data())
                .context(CommandError::Write { path: packets_output.clone() })?;

            println!("Wrote {} bytes of metadata to {:?}", replay_file.raw_meta.len(), meta_output);
            println!("Wrote {} bytes of packet data to {:?}", replay_file.packet_data().len(), packets_output);
        }
        Command::Summary { replay } => {
            parse_replay(&replay, ctx, |meta| wows_replays::analyzer::summary::SummaryBuilder::new().build(meta))
                .context(CommandError::ParseReplay { path: replay.clone() })?;
        }
        Command::Chat { replay } => {
            parse_replay(&replay, ctx, |meta| wows_replays::analyzer::chat::ChatLoggerBuilder::new().build(meta))
                .context(CommandError::ParseReplay { path: replay.clone() })?;
        }
        Command::Survey { skip_decode, replays } => {
            let mut survey_result = SurveyResults::empty();
            for replay_path in &replays {
                for entry in walkdir::WalkDir::new(replay_path) {
                    let entry = entry.context(CommandError::Walk { path: replay_path.clone() })?;
                    if !entry.path().is_file() {
                        continue;
                    }
                    let replay = entry.path().to_path_buf();
                    let result = survey_file(skip_decode, ctx, replay);
                    survey_result.add(result);
                }
            }
            survey_result.print();
        }
        Command::Bench { mode, iterations, limit, replays } => {
            run_bench(&mode, iterations, limit, &replays, ctx)?;
        }
        Command::Search { replays: replay_paths } => {
            let mut replays = vec![];
            for replay_path in &replay_paths {
                for entry in walkdir::WalkDir::new(replay_path) {
                    let entry = entry.context(CommandError::Walk { path: replay_path.clone() })?;
                    if !entry.path().is_file() {
                        continue;
                    }
                    let replay = entry.path().to_path_buf();
                    let replay_path = replay.clone();

                    let replay = match ReplayFile::from_file(&replay) {
                        Ok(replay) => replay,
                        Err(_) => {
                            continue;
                        }
                    };
                    replays.push((replay_path, replay.meta));

                    if replays.len() % 100 == 0 {
                        println!("Parsed {} games...", replays.len());
                    }
                }
            }
            replays.sort_by_key(|replay| {
                match jiff::civil::DateTime::strptime("%d.%m.%Y %H:%M:%S", &replay.1.dateTime) {
                    Ok(x) => x,
                    Err(e) => {
                        println!("Couldn't parse '{}' because {:?}", replay.1.dateTime, e);
                        jiff::civil::DateTime::strptime("%d.%m.%Y %H:%M:%S", "05.05.1995 01:02:03").unwrap()
                    }
                }
            });
            println!("Found {} games", replays.len());
            for i in 0..10 {
                let idx = replays.len() - i - 1;
                println!(
                    "{:?} {} {} {} {}",
                    replays[idx].0,
                    replays[idx].1.playerName,
                    replays[idx].1.dateTime,
                    replays[idx].1.mapDisplayName,
                    replays[idx].1.playerVehicle
                );
            }
        }
        Command::Analyze { replay } => {
            let replay_file = read_replay(&replay)?;
            let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
            let results_constants =
                versioned_constants(constants_path, extracted, &version).context(CommandError::GameData { version })?;
            let analysis = crate::analyze(&replay_file, ctx, results_constants.as_ref())
                .context(CommandError::Analyze { path: replay.clone() })?;
            println!("{}", serde_json::to_string_pretty(&analysis).expect("an analysis serializes"));
        }
        Command::Packets { kinds, entity_id, from, to, replay } => {
            let replay_file = read_replay(&replay)?;
            let filter = crate::PacketFilter {
                kinds,
                entity: entity_id.map(EntityId::from),
                from: from.map(GameClock),
                to: to.map(GameClock),
            };
            let mut stdout = std::io::stdout().lock();
            let scan = crate::scan_packets(&replay_file, ctx, &filter, |record| {
                let written = serde_json::to_writer(&mut stdout, &record).map_err(std::io::Error::from);
                match written.and_then(|()| writeln!(stdout)) {
                    Ok(()) => std::ops::ControlFlow::Continue(()),
                    Err(_) => std::ops::ControlFlow::Break(()),
                }
            })
            .context(CommandError::Analyze { path: replay.clone() })?;
            eprintln!("{} packets, {} matched", scan.packets, scan.matched);
        }
        Command::BattleResults {
            format,
            out_dir,
            out_file,
            allow_approximate_constants,
            pr_expected_values,
            replays,
        } => {
            crate::battle_results::run(
                ctx,
                game_dir,
                extracted,
                constants_path,
                format,
                out_dir,
                out_file,
                allow_approximate_constants,
                pr_expected_values,
                replays,
            )
            .context(CommandError::BattleResults)?;
        }
        Command::Query { command } => match command {
            QueryCommand::ArenaId { replays } => {
                for replay_path in &replays {
                    for entry in walkdir::WalkDir::new(replay_path) {
                        let entry = entry.context(CommandError::Walk { path: replay_path.clone() })?;
                        if !entry.path().is_file() {
                            continue;
                        }
                        let path = entry.path();
                        let arena_id = match ReplayFile::from_file(path) {
                            Ok(rf) => {
                                let version = Version::from_client_exe(&rf.meta.clientVersionFromExe);
                                match ctx.entity_specs(&version) {
                                    Ok(specs) => {
                                        wows_replays::analyzer::arena_scan::scan_arena_id(&specs, version, &rf)
                                    }
                                    Err(e) => {
                                        eprintln!("# {} (game data: {})", path.display(), e);
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("# {} (parse: {:?})", path.display(), e);
                                None
                            }
                        };
                        let id_str = arena_id.map(|id| id.to_string()).unwrap_or_else(|| "-".to_string());
                        println!("{}\t{}", id_str, path.display());
                    }
                }
            }
            QueryCommand::Players { replay, name, entity_id, ship, json } => {
                run_players_query(&replay, ctx, name.as_deref(), entity_id, ship.as_deref(), json)
                    .context(CommandError::Analyze { path: replay.clone() })?;
            }
            QueryCommand::GameVersion { replays } => {
                for replay_path in &replays {
                    for entry in walkdir::WalkDir::new(replay_path) {
                        let entry = entry.context(CommandError::Walk { path: replay_path.clone() })?;
                        if !entry.path().is_file() {
                            continue;
                        }
                        let path = entry.path();
                        let version = match ReplayFile::meta_from_file(path) {
                            Ok(meta) => Version::try_from_client_exe(&meta.clientVersionFromExe)
                                .map(|v| {
                                    format!(
                                        "{}.{}.{}.{}",
                                        v.major,
                                        v.minor,
                                        v.patch,
                                        v.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
                                    )
                                })
                                .unwrap_or_else(|| "-".to_string()),
                            Err(e) => {
                                eprintln!("# {} (parse: {:?})", path.display(), e);
                                "-".to_string()
                            }
                        };
                        println!("{}\t{}", version, path.display());
                    }
                }
            }
            QueryCommand::Roster { replays, has_class, exclude_own, json } => {
                run_roster_query(&replays, ctx, &has_class, exclude_own, json)
                    .context(CommandError::Query)?;
            }
        },
    }

    Ok(())
}

fn read_replay(path: &Path) -> Result<ReplayFile, Report<CommandError>> {
    ReplayFile::from_file(path).context(CommandError::ReadReplay { path: path.to_path_buf() })
}
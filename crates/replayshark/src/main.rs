use anyhow::Context;
use anyhow::anyhow;
use clap::Parser;
use clap::Subcommand;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use wowsunpack::data::DataFileWithCallback;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::rpc::entitydefs::parse_scripts;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;

use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;

/// Parses & processes World of Warships replay files
#[derive(Parser)]
#[command(author = "Lane Kolbly <lane@rscheme.org>, Lander Brandt <landaire>")]
struct Args {
    /// Path to your game directory (e.g. E:\WoWs\World_of_Warships\)
    #[arg(short = 'g', long = "game")]
    game_dir: Option<PathBuf>,

    /// Path to extracted game files
    #[arg(short = 'e', long = "extracted")]
    extracted_dir: Option<PathBuf>,

    /// Path to a constants JSON file (from wows-constants repo) to override
    /// consumable IDs, battle stages, etc.
    #[arg(short = 'c', long = "constants")]
    constants: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
        command: QueryCommands,
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
}

#[derive(Subcommand)]
enum QueryCommands {
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
                        println!(
                            "{} {} ({:x?})",
                            player.username(),
                            player.meta_ship_id(),
                            (player.meta_ship_id().raw() as u32).to_le_bytes(),
                        );
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
            println!("{}", &encoded);
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
        println!("{}", &serde_json::to_string(&meta).unwrap());
    }
    Box::new(decoder)
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

/// Resolve the extracted data directory. If the user passed a parent directory
/// containing version subdirectories (e.g. `15.1.0_11965230/`), auto-detect the
/// right one. If they passed the version dir itself, use it directly.
/// Falls back to the legacy `<extracted>/<version>` layout.
fn resolve_extracted_dir(path: &Path, replay_version: &Version) -> anyhow::Result<PathBuf> {
    if !path.exists() {
        return Err(anyhow!("Extracted data directory does not exist: {}", path.display()));
    }

    // If the path itself contains metadata.toml, it's already the version dir
    if let Some(meta) = read_metadata(path) {
        if meta.build != replay_version.build {
            return Err(anyhow!(
                "Extracted data is build {} ({}) but replay is build {}. \
                 Entity definitions will not match. Use extracted data for the correct build.",
                meta.build,
                meta.version,
                replay_version.build
            ));
        }
        return Ok(path.to_path_buf());
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
        if let Some(matched) = candidates.iter().find(|(_, m)| m.build == replay_version.build) {
            return Ok(matched.0.clone());
        }

        // Cross-region fallback via BuildsIndex: same major.minor.patch, different build.
        let builds_path = path.join("builds.toml");
        if builds_path.exists() {
            let version_str = format!("{}.{}.{}", replay_version.major, replay_version.minor, replay_version.patch);
            let index = wows_data_mgr::builds::BuildsIndex::load(&builds_path);
            if let Some((entry, _exact)) = index.resolve_build(replay_version.build, Some(&version_str)) {
                eprintln!(
                    "No exact data for build {}; using {} (build {})",
                    replay_version.build, entry.version, entry.build
                );
                return Ok(path.join(&entry.dir));
            }
        }

        // Last-ditch: same version string in candidates.
        let version_str = format!("{}.{}.{}", replay_version.major, replay_version.minor, replay_version.patch);
        if let Some(matched) = candidates.iter().find(|(_, m)| m.version == version_str) {
            eprintln!(
                "No exact data for build {}; using {} (build {})",
                replay_version.build, matched.1.version, matched.1.build
            );
            return Ok(matched.0.clone());
        }

        if candidates.len() == 1 {
            let (_, ref meta) = candidates[0];
            return Err(anyhow!(
                "No exact build match for replay (build {}). Only available: {} (build {}). \
                 Download or extract the correct build.",
                replay_version.build,
                meta.version,
                meta.build
            ));
        }

        let available: Vec<String> =
            candidates.iter().map(|(_, m)| format!("{} (build {})", m.version, m.build)).collect();
        return Err(anyhow!(
            "No extracted data matches replay build {}. Available versions in {}: {}",
            replay_version.build,
            path.display(),
            available.join(", ")
        ));
    }

    // Legacy fallback: <extracted>/<major.minor.patch>/
    let legacy = path.join(replay_version.to_path());
    if legacy.exists() {
        return Ok(legacy);
    }

    Err(anyhow!(
        "No extracted game data found in {}. Expected a version directory \
         (containing metadata.toml) or legacy layout (<extracted>/<version>/).",
        path.display()
    ))
}

fn load_game_data(
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    replay_version: &Version,
) -> anyhow::Result<Vec<EntitySpec>> {
    let specs = match (game_dir, extracted_dir) {
        (Some(game_dir), _) => {
            let resources =
                game_data::load_game_resources(Path::new(game_dir), replay_version).map_err(|e| anyhow!("{}", e))?;
            resources.specs
        }
        (None, Some(extracted)) => {
            let extracted_dir = resolve_extracted_dir(Path::new(extracted), replay_version)?;
            let vfs_dir = extracted_dir.join("vfs");
            let scripts_dir = if vfs_dir.exists() { vfs_dir } else { extracted_dir };
            let loader = DataFileWithCallback::new(|path| {
                let path = Path::new(path);

                let file_data = std::fs::read(scripts_dir.join(path))
                    .with_context(|| format!("failed to read game file from extracted dir: {:?}", path))
                    .unwrap();

                Ok(Cow::Owned(file_data))
            });
            parse_scripts(&loader).unwrap()
        }
        (None, None) => {
            return Err(anyhow!("Game directory or extracted files directory must be supplied"));
        }
    };

    Ok(specs)
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

fn parse_replay<F>(
    replay: &std::path::Path,
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    build: F,
) -> rootcause::Result<(), ParseError>
where
    F: FnOnce(&wows_replays::ReplayMeta) -> Box<dyn Analyzer>,
{
    let replay_file = ReplayFile::from_file(replay)?;

    let replay_version = Version::from_client_exe(replay_file.meta.clientVersionFromExe.as_str());
    let specs = load_game_data(game_dir, extracted_dir, &replay_version).expect("failed to load game specs");

    let mut analyzer = build(&replay_file.meta);

    let mut parser = wows_replays::packet2::Parser::with_version(&specs, replay_version);
    let mut remaining = &replay_file.packet_data[..];
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

fn survey_file(
    skip_decode: bool,
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    game_constants: &'static GameConstants,
    replay: std::path::PathBuf,
) -> SurveyResult {
    let filename = replay.file_name().unwrap().to_str().unwrap();
    let filename = filename.to_string();

    print!("Parsing {}: ", truncate_string(&filename, 20));
    std::io::stdout().flush().unwrap();

    let survey_stats = std::rc::Rc::new(std::cell::RefCell::new(wows_replays::analyzer::survey::SurveyStats::new()));
    let stats_clone = survey_stats.clone();
    match parse_replay(&replay, game_dir, extracted_dir, |meta| {
        wows_replays::analyzer::survey::SurveyBuilder::new(stats_clone, skip_decode)
            .game_constants(game_constants)
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

/// Load a constants JSON file and merge it into a `GameConstants`, returning a
/// `&'static` reference (leaked, since the CLI is short-lived).
fn load_game_constants(constants_path: Option<&Path>, build: u32) -> &'static GameConstants {
    let mut gc = GameConstants::defaults();
    if let Some(path) = constants_path {
        let data = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read constants file {}: {e}", path.display()));
        let json: serde_json::Value =
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("Failed to parse constants JSON: {e}"));
        gc.merge_replay_constants(&json, build);
    }
    Box::leak(Box::new(gc))
}

/// Load the game metadata provider (game params + entity specs + translations)
/// and battle constants for the given replay version. The provider resolves
/// ship param ids to localized names; the entity specs drive the packet parser.
fn load_metadata_provider_and_constants(
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    version: &Version,
) -> anyhow::Result<(GameMetadataProvider, GameConstants)> {
    match (game_dir, extracted_dir) {
        (Some(game_dir), _) => {
            let resources =
                game_data::load_game_resources(Path::new(game_dir), version).map_err(|e| anyhow!("{}", e))?;
            let mut provider = GameMetadataProvider::from_vfs(&resources.vfs)
                .map_err(|e| anyhow!("failed to load game params: {e:?}"))?;
            let constants = GameConstants::from_vfs(&resources.vfs);
            let mo_path = game_data::translations_path(Path::new(game_dir), version.build);
            apply_translations(&mo_path, &mut provider);
            Ok((provider, constants))
        }
        (None, Some(extracted)) => {
            let dir = resolve_extracted_dir(Path::new(extracted), version)?;
            let vfs_root = dir.join("vfs");
            if !vfs_root.exists() {
                return Err(anyhow!("VFS directory not found: {}", vfs_root.display()));
            }
            let vfs = VfsPath::new(PhysicalFS::new(&vfs_root));

            // Prefer the prebuilt rkyv cache; fall back to parsing GameParams.data.
            let rkyv_path = dir.join("game_params.rkyv");
            let mut provider = match wowsunpack::game_params::cache::load(&rkyv_path) {
                Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs)
                    .map_err(|e| anyhow!("failed to build game metadata: {e:?}"))?,
                None => GameMetadataProvider::from_vfs(&vfs)
                    .map_err(|e| anyhow!("failed to load game params from VFS: {e:?}"))?,
            };
            let constants = GameConstants::from_vfs(&vfs);
            let mo_path = dir.join("translations/en/LC_MESSAGES/global.mo");
            apply_translations(&mo_path, &mut provider);
            Ok((provider, constants))
        }
        (None, None) => Err(anyhow!("Game directory or extracted files directory must be supplied")),
    }
}

/// Load a `.mo` translation catalog into the provider. Ship names are
/// unavailable (param indices are shown instead) when this fails.
fn apply_translations(mo_path: &Path, provider: &mut GameMetadataProvider) {
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
    game_dir: Option<&str>,
    extracted_dir: Option<&str>,
    name_filter: Option<&str>,
    entity_filter: Option<u32>,
    ship_filter: Option<&str>,
    as_json: bool,
) -> anyhow::Result<()> {
    let replay_file = ReplayFile::from_file(replay).map_err(|e| anyhow!("failed to read replay: {e:?}"))?;
    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let (provider, constants) = load_metadata_provider_and_constants(game_dir, extracted_dir, &version)?;

    let mut world = BattleWorld::new(&replay_file.meta, &provider, Some(&constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = wows_replays::packet2::Parser::with_version(provider.entity_specs(), version);
    let mut remaining = replay_file.packet_data.as_slice();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
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
    world.finish();

    let report = world.into_report();

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

fn main() {
    let args = Args::parse();

    let game_dir = args.game_dir.as_deref().and_then(|p| p.to_str());
    let extracted = args.extracted_dir.as_deref().and_then(|p| p.to_str());
    let constants_path = args.constants.as_deref();

    match args.command {
        Commands::Dump { output, no_meta, replay } => {
            let replay_file = ReplayFile::from_file(&replay).unwrap();
            let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
            let gc = load_game_constants(constants_path, version.build);
            parse_replay(&replay, game_dir, extracted, |meta| {
                wows_replays::analyzer::decoder::DecoderBuilder::new(false, no_meta, output.as_deref())
                    .game_constants(gc)
                    .build(meta)
            })
            .unwrap();
        }
        Commands::Investigate { meta, timestamp, filter_packet, filter_method, entity_id, replay } => {
            let replay_file = ReplayFile::from_file(&replay).unwrap();
            let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
            let gc = load_game_constants(constants_path, version.build);
            let no_meta = !meta;
            parse_replay(&replay, game_dir, extracted, |meta| {
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
            .unwrap();
        }
        Commands::Spec { version } => {
            let target_version = Version::from_client_exe(&version);
            let specs = load_game_data(game_dir, extracted, &target_version).expect("failed to load game data");
            printspecs(&specs);
        }
        Commands::AuditTypes { version } => {
            let target_version = Version::from_client_exe(&version);
            let specs = load_game_data(game_dir, extracted, &target_version).expect("failed to load game data");
            audit_types(&specs);
        }
        Commands::Decrypt { meta_output, packets_output, replay } => {
            let replay_file = ReplayFile::from_file(&replay).unwrap();
            std::fs::write(&meta_output, &replay_file.raw_meta).unwrap();
            std::fs::write(&packets_output, &replay_file.packet_data).unwrap();

            println!("Wrote {} bytes of metadata to {:?}", replay_file.raw_meta.len(), meta_output);
            println!("Wrote {} bytes of packet data to {:?}", replay_file.packet_data.len(), packets_output);
        }
        Commands::Summary { replay } => {
            parse_replay(&replay, game_dir, extracted, |meta| {
                wows_replays::analyzer::summary::SummaryBuilder::new().build(meta)
            })
            .unwrap();
        }
        Commands::Chat { replay } => {
            parse_replay(&replay, game_dir, extracted, |meta| {
                wows_replays::analyzer::chat::ChatLoggerBuilder::new().build(meta)
            })
            .unwrap();
        }
        Commands::Survey { skip_decode, replays } => {
            // For survey, we use build 0 since we don't know the build ahead of time.
            // The constants override is still useful for consumable ID mapping.
            let gc = load_game_constants(constants_path, 0);
            let mut survey_result = SurveyResults::empty();
            for replay_path in &replays {
                for entry in walkdir::WalkDir::new(replay_path) {
                    let entry = entry.expect("Error unwrapping entry");
                    if !entry.path().is_file() {
                        continue;
                    }
                    let replay = entry.path().to_path_buf();
                    let result = survey_file(skip_decode, game_dir, extracted, gc, replay);
                    survey_result.add(result);
                }
            }
            survey_result.print();
        }
        Commands::Search { replays: replay_paths } => {
            let mut replays = vec![];
            for replay_path in &replay_paths {
                for entry in walkdir::WalkDir::new(replay_path) {
                    let entry = entry.expect("Error unwrapping entry");
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
        Commands::Query { command } => match command {
            QueryCommands::ArenaId { replays } => {
                for replay_path in &replays {
                    for entry in walkdir::WalkDir::new(replay_path) {
                        let entry = entry.expect("Error walking replays");
                        if !entry.path().is_file() {
                            continue;
                        }
                        let path = entry.path();
                        let arena_id = match ReplayFile::from_file(path) {
                            Ok(rf) => {
                                let version = Version::from_client_exe(&rf.meta.clientVersionFromExe);
                                match load_game_data(game_dir, extracted, &version) {
                                    Ok(specs) => wows_replays::analyzer::battle_controller::merged::scan_arena_id(
                                        &specs, version, &rf,
                                    ),
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
            QueryCommands::Players { replay, name, entity_id, ship, json } => {
                run_players_query(&replay, game_dir, extracted, name.as_deref(), entity_id, ship.as_deref(), json)
                    .expect("failed to query players");
            }
            QueryCommands::GameVersion { replays } => {
                for replay_path in &replays {
                    for entry in walkdir::WalkDir::new(replay_path) {
                        let entry = entry.expect("Error walking replays");
                        if !entry.path().is_file() {
                            continue;
                        }
                        let path = entry.path();
                        let version = match ReplayFile::meta_from_file(path) {
                            Ok(meta) => Version::try_from_client_exe(&meta.clientVersionFromExe)
                                .map(|v| format!("{}.{}.{}.{}", v.major, v.minor, v.patch, v.build))
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
        },
    }
}

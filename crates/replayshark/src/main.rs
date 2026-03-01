use anyhow::Context;
use anyhow::anyhow;
use clap::Parser;
use clap::Subcommand;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use wowsunpack::data::DataFileWithCallback;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::rpc::entitydefs::parse_scripts;

use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
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
            && n != decoded.packet_type
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
) -> Box<dyn Analyzer> {
    let version = Version::from_client_exe(&meta.clientVersionFromExe);
    let decoder = InvestigativePrinter {
        packet_decoder: wows_replays::analyzer::decoder::PacketDecoder::builder().version(version).audit(true).build(),
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
            let extracted_dir = Path::new(extracted).join(replay_version.to_path());
            if !extracted_dir.exists() {
                return Err(anyhow!(
                    "Missing scripts for game version {}. Expected to be at {:?}",
                    replay_version.to_path(),
                    &extracted_dir
                ));
            }
            let loader = DataFileWithCallback::new(|path| {
                let path = Path::new(path);

                let file_data = std::fs::read(extracted_dir.join(path))
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

    let specs = load_game_data(
        game_dir,
        extracted_dir,
        &Version::from_client_exe(replay_file.meta.clientVersionFromExe.as_str()),
    )
    .expect("failed to load game specs");

    let mut analyzer = build(&replay_file.meta);

    let mut parser = wows_replays::packet2::Parser::new(&specs);
    let mut remaining = &replay_file.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).map_err(|e| rootcause::report!(ParseError::from(e)))?;
        analyzer.process(&packet);
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
    replay: std::path::PathBuf,
) -> SurveyResult {
    let filename = replay.file_name().unwrap().to_str().unwrap();
    let filename = filename.to_string();

    print!("Parsing {}: ", truncate_string(&filename, 20));
    std::io::stdout().flush().unwrap();

    let survey_stats = std::rc::Rc::new(std::cell::RefCell::new(wows_replays::analyzer::survey::SurveyStats::new()));
    let stats_clone = survey_stats.clone();
    match parse_replay(&replay, game_dir, extracted_dir, |meta| {
        wows_replays::analyzer::survey::SurveyBuilder::new(stats_clone, skip_decode).build(meta)
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

fn main() {
    let args = Args::parse();

    let game_dir = args.game_dir.as_deref().and_then(|p| p.to_str());
    let extracted = args.extracted_dir.as_deref().and_then(|p| p.to_str());

    match args.command {
        Commands::Dump { output, no_meta, replay } => {
            parse_replay(&replay, game_dir, extracted, |meta| {
                wows_replays::analyzer::decoder::DecoderBuilder::new(false, no_meta, output.as_deref()).build(meta)
            })
            .unwrap();
        }
        Commands::Investigate { meta, timestamp, filter_packet, filter_method, entity_id, replay } => {
            let no_meta = !meta;
            parse_replay(&replay, game_dir, extracted, |meta| {
                build_investigative_printer(
                    meta,
                    no_meta,
                    filter_packet.as_deref(),
                    filter_method.as_deref(),
                    timestamp.as_deref(),
                    entity_id.as_deref(),
                )
            })
            .unwrap();
        }
        Commands::Spec { version } => {
            let target_version = Version::from_client_exe(&version);
            let specs = load_game_data(None, extracted, &target_version).expect("failed to load game data");
            printspecs(&specs);
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
            let mut survey_result = SurveyResults::empty();
            for replay_path in &replays {
                for entry in walkdir::WalkDir::new(replay_path) {
                    let entry = entry.expect("Error unwrapping entry");
                    if !entry.path().is_file() {
                        continue;
                    }
                    let replay = entry.path().to_path_buf();
                    let result = survey_file(skip_decode, game_dir, extracted, replay);
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
    }
}

//! replayshark's command line: the argument definitions and their dispatch,
//! shared by the `replayshark` binary and `wows_toolkit replayshark`.

mod battle_results;
mod investigate;
mod print;

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use clap::Subcommand;
use rootcause::prelude::*;
use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::analyzer::decoder::DecoderBuilder;
use wows_replays::context::CachedContext;
use wows_replays::context::GameDataContext;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::Version;

use crate::analysis::replay_version;
use crate::battle_results::ResultsFormat;
use crate::bench::BenchMode;
use crate::error::AnalysisError;
use crate::error::CommandError;
use crate::game_data::CliGameDataContext;
use crate::game_data::versioned_constants;
use crate::parse::parse_into;
use crate::query::PlayerFilter;
use crate::survey::SurveyTotals;
use investigate::ClockOffset;
use investigate::InvestigateFilter;
use investigate::InvestigativePrinter;

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
        #[arg(value_parser = parse_version)]
        version: Version,
    },
    /// Audit which game def semantic type names have domain newtypes.
    AuditTypes {
        /// Version to load. Comma-delimited: major,minor,patch,build
        #[arg(value_parser = parse_version)]
        version: Version,
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
        #[arg(long, value_parser = investigate::parse_clock_offset)]
        timestamp: Option<ClockOffset>,

        /// If specified, only return packets of the given packet_type
        #[arg(long, value_parser = parse_number)]
        filter_packet: Option<u32>,

        /// If specified, only return method calls for the given method
        #[arg(long)]
        filter_method: Option<String>,

        /// Entity ID to apply to other filters if applicable
        #[arg(long, value_parser = parse_number)]
        entity_id: Option<u32>,

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
            let constants = game_constants(ctx, &replay_file).context(analyze_error(&replay))?;
            let mut decoder = DecoderBuilder::new(false, no_meta, output.as_deref())
                .game_constants(&constants)
                .build(&replay_file.meta);
            let diagnostics = parse_into(&replay_file, ctx, &mut *decoder).context(analyze_error(&replay))?;
            print::diagnostics(&diagnostics);
        }
        Command::Investigate { meta, timestamp, filter_packet, filter_method, entity_id, replay } => {
            let replay_file = read_replay(&replay)?;
            let constants = game_constants(ctx, &replay_file).context(analyze_error(&replay))?;
            let filter = InvestigateFilter {
                packet_type: filter_packet,
                method: filter_method,
                entity_id: entity_id.map(EntityId::from),
                timestamp,
                meta,
            };
            let mut printer = InvestigativePrinter::new(&replay_file.meta, filter, &constants);
            let diagnostics = parse_into(&replay_file, ctx, &mut printer).context(analyze_error(&replay))?;
            print::diagnostics(&diagnostics);
        }
        Command::Spec { version } => {
            let specs = ctx.entity_specs(&version).context(CommandError::GameData { version })?;
            print::spec(&specs);
        }
        Command::AuditTypes { version } => {
            let specs = ctx.entity_specs(&version).context(CommandError::GameData { version })?;
            print::type_audit(&crate::audit::audit_types(&specs));
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
            let replay_file = read_replay(&replay)?;
            let summary = crate::summary::summarize(&replay_file, ctx).context(analyze_error(&replay))?;
            print::diagnostics(&summary.diagnostics);
            print::summary(&summary.value);
        }
        Command::Chat { replay } => {
            let replay_file = read_replay(&replay)?;
            let chat = crate::chat::chat_log(&replay_file, ctx).context(analyze_error(&replay))?;
            print::diagnostics(&chat.diagnostics);
            print::chat(&chat.value);
        }
        Command::Survey { skip_decode, replays } => {
            let mut totals = SurveyTotals::new();
            for_each_file(&replays, |path| {
                survey_file(path, ctx, skip_decode, &mut totals);
                Ok(())
            })?;
            print::survey_totals(&totals);
        }
        Command::Bench { mode, iterations, limit, replays } => {
            let paths = crate::bench::collect_replay_paths(&replays, limit).context(CommandError::Walk)?;
            println!("bench mode={} files={} iterations={}", mode, paths.len(), iterations);
            let mode =
                BenchMode::from_str(&mode).map_err(|e| report!(CommandError::UnknownBenchMode { mode: e.mode }))?;
            print::bench(&crate::bench::bench(mode, iterations, &paths, ctx));
        }
        Command::Search { replays } => {
            let hits = crate::search::search(&replays, |count| println!("Parsed {count} games..."))
                .context(CommandError::Walk)?;
            print::search(&hits);
        }
        Command::Analyze { replay } => {
            let replay_file = read_replay(&replay)?;
            let version = replay_version(&replay_file).context(analyze_error(&replay))?;
            let results_constants =
                versioned_constants(constants_path, extracted, &version).context(CommandError::GameData { version })?;
            let analysis =
                crate::analyze(&replay_file, ctx, results_constants.as_ref()).context(analyze_error(&replay))?;
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
            .context(analyze_error(&replay))?;
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
            let options = battle_results::BatchOptions {
                format,
                out_dir,
                out_file,
                allow_approximate_constants,
                pr_expected_values,
                replays,
            };
            battle_results::run(ctx, game_dir.is_some() || extracted.is_some(), constants_path, options)?;
        }
        Command::Query { command } => run_query(command, ctx)?,
    }

    Ok(())
}

fn run_query(command: QueryCommand, ctx: &dyn GameDataContext) -> Result<(), Report<CommandError>> {
    match command {
        QueryCommand::ArenaId { replays } => for_each_file(&replays, |path| {
            let arena_id = match ReplayFile::from_file(path) {
                Ok(replay) => match crate::query::arena_id(&replay, ctx) {
                    Ok(arena_id) => arena_id,
                    Err(e) => {
                        eprintln!("# {} (game data: {})", path.display(), e);
                        None
                    }
                },
                Err(e) => {
                    eprintln!("# {} (parse: {:?})", path.display(), e);
                    None
                }
            };
            let id_str = arena_id.map_or_else(|| "-".to_string(), |id| id.to_string());
            println!("{}\t{}", id_str, path.display());
            Ok(())
        }),
        QueryCommand::Players { replay, name, entity_id, ship, json } => {
            let replay_file = read_replay(&replay)?;
            let filter = PlayerFilter { name, entity: entity_id.map(EntityId::from), ship };
            let rows = crate::query::players(&replay_file, ctx, &filter).context(analyze_error(&replay))?;
            print::players(&rows, json);
            Ok(())
        }
        QueryCommand::GameVersion { replays } => for_each_file(&replays, |path| {
            let version = match ReplayFile::meta_from_file(path) {
                Ok(meta) => crate::query::game_version(&meta).map_or_else(|| "-".to_string(), print::version),
                Err(e) => {
                    eprintln!("# {} (parse: {:?})", path.display(), e);
                    "-".to_string()
                }
            };
            println!("{}\t{}", version, path.display());
            Ok(())
        }),
        QueryCommand::Roster { replays, has_class, exclude_own, json } => for_each_file(&replays, |path| {
            if path.extension().is_none_or(|e| e != "wowsreplay") {
                return Ok(());
            }

            // A replay whose header or game data does not load has no
            // roster to show.
            let Ok(meta) = ReplayFile::meta_from_file(path) else { return Ok(()) };
            let Ok(roster) = crate::query::roster(&meta, ctx) else { return Ok(()) };
            if roster.matches(&has_class, exclude_own) {
                print::roster(path, &roster, json);
            }
            Ok(())
        }),
    }
}

/// Surveys the replay at `path`, printing a line for it and counting it in
/// `totals`.
fn survey_file(path: &Path, ctx: &dyn GameDataContext, skip_decode: bool, totals: &mut SurveyTotals) {
    let file_name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    print::survey_start(&file_name);

    let surveyed = match ReplayFile::from_file(path) {
        Ok(replay) => crate::survey::survey_replay(&replay, ctx, skip_decode),
        Err(report) => match report.current_context() {
            ParseError::UnsupportedReplayVersion { version } => {
                Err(AnalysisError::Parse(ParseError::UnsupportedReplayVersion { version: version.clone() }))
            }
            _ => {
                println!("Parse error: {report:?}");
                totals.add_parse_failure();
                return;
            }
        },
    };
    match surveyed {
        Ok(survey) => {
            print::diagnostics(&survey.diagnostics);
            print::survey_parsed(&survey.value);
            totals.add_parsed(&file_name, &survey.value);
        }
        Err(AnalysisError::Parse(ParseError::UnsupportedReplayVersion { version })) => {
            println!("Unsupported version {version}");
            totals.add_unsupported_version(&version);
        }
        Err(e) => {
            println!("Parse error: {e:?}");
            totals.add_parse_failure();
        }
    }
}

/// Calls `visit` with every file under `roots`.
fn for_each_file(
    roots: &[PathBuf],
    mut visit: impl FnMut(&Path) -> Result<(), Report<CommandError>>,
) -> Result<(), Report<CommandError>> {
    for root in roots {
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry.context(CommandError::Walk)?;
            if entry.path().is_file() {
                visit(entry.path())?;
            }
        }
    }

    Ok(())
}

fn read_replay(path: &Path) -> Result<ReplayFile, Report<CommandError>> {
    ReplayFile::from_file(path).context(CommandError::ReadReplay { path: path.to_path_buf() })
}

fn analyze_error(path: &Path) -> CommandError {
    CommandError::Analyze { path: path.to_path_buf() }
}

fn game_constants(
    ctx: &dyn GameDataContext,
    replay: &ReplayFile,
) -> Result<std::sync::Arc<wows_replays::game_constants::GameConstants>, AnalysisError> {
    let version = replay_version(replay)?;

    Ok(ctx.game_constants(&version)?)
}

/// A number in any base `parse_int` reads, such as `0x2a`.
fn parse_number(text: &str) -> Result<u32, String> {
    parse_int::parse::<u32>(text).map_err(|e| e.to_string())
}

/// A version as the replay header writes it: `major,minor,patch,build`.
fn parse_version(text: &str) -> Result<Version, String> {
    Version::try_from_client_exe(text).ok_or_else(|| "expected major,minor,patch,build".to_owned())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }

    fn parse(argv: &[&str]) -> Result<Args, clap::Error> {
        Cli::try_parse_from(std::iter::once("replayshark").chain(argv.iter().copied())).map(|cli| cli.args)
    }

    #[test]
    fn investigate_reads_numbers_in_any_base_and_a_clock_offset() {
        let args = parse(&[
            "investigate",
            "--filter-packet",
            "0x2a",
            "--entity-id",
            "17",
            "--timestamp",
            "01:02:03+30",
            "a.wowsreplay",
        ])
        .expect("investigate flags");

        let Command::Investigate { filter_packet, entity_id, timestamp, .. } = args.command else {
            panic!("expected investigate, got {:?}", args.command);
        };
        assert_eq!(filter_packet, Some(42));
        assert_eq!(entity_id, Some(17));
        assert_eq!(timestamp, Some(ClockOffset(3693.0)));
    }

    #[test]
    fn malformed_values_are_refused_at_parse_time() {
        assert!(parse(&["investigate", "--timestamp", "01:02:03", "a.wowsreplay"]).is_err());
        assert!(parse(&["investigate", "--filter-packet", "nope", "a.wowsreplay"]).is_err());
        assert!(parse(&["spec", "15.1.0"]).is_err());
    }

    #[test]
    fn game_data_flags_precede_the_command() {
        let args = parse(&["-e", "/data", "-c", "c.json", "query", "players", "--json", "a.wowsreplay"])
            .expect("query players");

        assert_eq!(args.game_data.extracted_dir.as_deref(), Some(Path::new("/data")));
        assert_eq!(args.game_data.constants.as_deref(), Some(Path::new("c.json")));
        assert!(matches!(args.command, Command::Query { command: QueryCommand::Players { json: true, .. } }));
    }
}

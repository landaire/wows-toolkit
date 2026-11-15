//! Replay analysis shared by the replayshark CLI and programs that read
//! replays with game data loaded ahead of time.
//!
//! Each analysis returns serializable data and prints nothing. [`cli`] is
//! the command line, which prints them. [`game_data`] is the command line's
//! game data source and reports build fallbacks and missing translations on
//! stderr.

pub mod analysis;
pub mod audit;
pub mod battle_results;
pub mod bench;
pub mod chat;
pub mod cli;
pub mod error;
pub mod game_data;
pub mod loaded_build;
pub mod packets;
pub mod parse;
pub mod query;
pub mod search;
pub mod summary;
pub mod survey;

pub use analysis::ReplayAnalysis;
pub use analysis::analyze;
pub use error::AnalysisError;
pub use error::BattleResultsError;
pub use error::CommandError;
pub use loaded_build::LoadedBuild;
pub use packets::PacketFilter;
pub use packets::PacketRecord;
pub use packets::PacketScan;
pub use packets::scan_packets;
pub use parse::Parsed;

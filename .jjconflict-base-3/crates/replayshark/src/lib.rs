//! Replay analysis shared by the replayshark CLI and programs that read
//! replays with game data loaded ahead of time.

pub mod analysis;
pub mod error;
pub mod loaded_build;
pub mod packets;

pub use analysis::ReplayAnalysis;
pub use analysis::analyze;
pub use error::AnalysisError;
pub use loaded_build::LoadedBuild;
pub use packets::PacketFilter;
pub use packets::PacketRecord;
pub use packets::PacketScan;
pub use packets::scan_packets;

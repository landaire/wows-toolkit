//! ECS-backed reconstruction of World of Warships battle state from replay
//! packet streams. Replaces wows-replays' BattleController.

pub mod components;
pub mod ids;
pub mod read;
pub mod report;
pub mod resources;
pub mod units;
pub mod world;

mod ingest;

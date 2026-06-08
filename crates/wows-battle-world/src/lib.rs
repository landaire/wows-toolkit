//! ECS-backed reconstruction of World of Warships battle state from replay
//! packet streams. Replaces wows-replays' BattleController.

pub mod components;
pub mod ids;
pub mod merged;
pub mod read;
pub mod report;
pub mod resources;
pub mod scan;
pub mod units;
pub mod view;
pub mod world;

mod ingest;

pub use scan::EntityTrack;
pub use scan::PositionTimeline;
pub use scan::SalvoFlightTimes;
pub use scan::SampledPos;
pub use view::ActiveSecondaryShot;
pub use world::BattleWorld;

//! Resolved-results damage/hit key constants and description tables. These now
//! live in wows-replay-insights (shared by the builder and this UI) and are
//! re-exported here so existing `use damage_types::*` sites keep working. Only
//! the constants the UI references directly are re-exported; the full key set
//! backs the description tables inside insights.

pub use wows_replay_insights::battle_report::DAMAGE_ATBA_CS;
pub use wows_replay_insights::battle_report::DAMAGE_ATBA_HE;
pub use wows_replay_insights::battle_report::DAMAGE_DESCRIPTIONS;
pub use wows_replay_insights::battle_report::DAMAGE_FIRE;
pub use wows_replay_insights::battle_report::DAMAGE_FLOOD;
pub use wows_replay_insights::battle_report::DAMAGE_MAIN_AP;
pub use wows_replay_insights::battle_report::DAMAGE_MAIN_CS;
pub use wows_replay_insights::battle_report::DAMAGE_MAIN_HE;
pub use wows_replay_insights::battle_report::DAMAGE_TPD_DEEP;
pub use wows_replay_insights::battle_report::DAMAGE_TPD_NORMAL;
pub use wows_replay_insights::battle_report::HITS_ATBA_AP_MANUAL;
pub use wows_replay_insights::battle_report::HITS_ATBA_CS;
pub use wows_replay_insights::battle_report::HITS_ATBA_CS_MANUAL;
pub use wows_replay_insights::battle_report::HITS_ATBA_HE;
pub use wows_replay_insights::battle_report::HITS_ATBA_HE_MANUAL;
pub use wows_replay_insights::battle_report::HITS_DESCRIPTIONS;
pub use wows_replay_insights::battle_report::HITS_MAIN_AP;
pub use wows_replay_insights::battle_report::HITS_MAIN_CS;
pub use wows_replay_insights::battle_report::HITS_MAIN_HE;
pub use wows_replay_insights::battle_report::HITS_ROCKET;
pub use wows_replay_insights::battle_report::HITS_ROCKET_AIRSUPPORT;
pub use wows_replay_insights::battle_report::HITS_SKIP;
pub use wows_replay_insights::battle_report::HITS_SKIP_AIRSUPPORT;
pub use wows_replay_insights::battle_report::HITS_SKIP_ALT;
pub use wows_replay_insights::battle_report::HITS_TPD_NORMAL;
pub use wows_replay_insights::battle_report::POTENTIAL_DAMAGE_DESCRIPTIONS;
pub use wows_replay_insights::battle_report::RECEIVED_DAMAGE_DESCRIPTIONS;

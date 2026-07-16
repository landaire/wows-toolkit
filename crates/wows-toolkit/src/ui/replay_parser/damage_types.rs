//! Resolved-results damage/hit description tables. These live in
//! wows-replay-insights (shared by the builder and this UI) and are re-exported
//! here so existing `use damage_types::*` sites keep working. The UI now only
//! needs the description tables to rebuild hover text; the per-key constants
//! back those tables inside insights.

pub use wows_replay_insights::battle_report::DAMAGE_DESCRIPTIONS;
pub use wows_replay_insights::battle_report::HITS_DESCRIPTIONS;
pub use wows_replay_insights::battle_report::POTENTIAL_DAMAGE_DESCRIPTIONS;
pub use wows_replay_insights::battle_report::RECEIVED_DAMAGE_DESCRIPTIONS;

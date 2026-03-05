// Type re-exports that are always available (no `parsing` feature needed).
// These come from lightweight modules in wowsunpack that don't require parsing.
pub use wowsunpack::game_types::BatteryState;
pub use wowsunpack::game_types::BattleStage;
pub use wowsunpack::game_types::BuoyancyState;
pub use wowsunpack::game_types::CameraMode;
pub use wowsunpack::game_types::CollisionType;
pub use wowsunpack::game_types::Consumable;
pub use wowsunpack::game_types::DeathCause;
pub use wowsunpack::game_types::FinishType;
pub use wowsunpack::game_types::Ribbon;
pub use wowsunpack::game_types::ShellHitType;
pub use wowsunpack::game_types::VoiceLine;
pub use wowsunpack::game_types::WeaponType;
pub use wowsunpack::recognized::Recognized;

// The full decoder implementation (packet decoding, analysis, etc.) requires
// heavy dependencies gated behind the `parsing` feature.
#[cfg(feature = "parsing")]
mod decode;
#[cfg(feature = "parsing")]
pub use decode::*;

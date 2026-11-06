//! Ship stats for a build: the port stat card with captain skills, upgrades
//! and modules applied, the stats derived from it, and the unit and better
//! direction of each.

pub mod build;
pub mod derived;
pub mod error;
pub mod fires;
pub mod mounts;
pub mod resistance;
pub mod stat;

pub use build::Build;
pub use build::BuildCard;
pub use build::BuildEffects;
pub use build::Triggers;
pub use derived::Derivation;
pub use derived::DerivedRow;
pub use derived::DerivedStat;
pub use derived::DerivedValue;
pub use derived::StockRanges;
pub use derived::mount_salvo;
pub use error::BuildError;
pub use fires::FireArmament;
pub use fires::FireInputs;
pub use fires::FireSource;
pub use fires::total_fires;
pub use resistance::FireResistance;
pub use resistance::Flooding;
pub use stat::Direction;
pub use stat::Unit;

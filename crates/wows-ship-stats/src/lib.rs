//! Ship stats for a build: the port stat card with captain skills, upgrades
//! and modules applied, the stats derived from it, and the unit and better
//! direction of each.

pub mod bearing;
pub mod build;
pub mod derived;
pub mod error;
pub mod fires;
pub mod mounts;
pub mod resistance;
pub mod stat;

pub use bearing::Bearing;
pub use bearing::MountCoverage;
pub use bearing::PerBearing;
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
pub use fires::FireBreakdown;
pub use fires::FireInputs;
pub use fires::FireSource;
pub use fires::fire_breakdown;
pub use fires::secondary_fires_by_bearing;
pub use fires::total_fires_all_mounts;
pub use fires::total_fires_by_bearing;
pub use resistance::FireResistance;
pub use resistance::Flooding;
pub use stat::Direction;
pub use stat::Unit;

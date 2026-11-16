//! Submarine dive capacity on the build players run.
//!
//! The battery holds `SubmarineBattery.capacity * batteryCapacityCoeff`
//! (Modifiers/ModifiersApply.py:679-680). At periscope depth and below
//! (`BATTERY_SPENDING_STATES`, BuoyancyCommon.py:37) it drains
//! `BASE_BATTERY_SPEND_RATE` = 1 per second (m63fc2621.py:99) whatever the
//! depth and speed, and the submarine must surface when it is empty
//! (BuoyancyOwner.py:405-412). On the surface it regains
//! `regenRate * batteryRegenCoeff` per second (ModifiersApply.py:898-899).
//! So the capacity is the seconds a full battery lasts underwater.

use serde::Serialize;
use wows_core::units::Seconds;
use wowsunpack::game_params::ttx::model::DiveCapacityRate;

use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;

/// Dive capacity of a submarine and how fast it refills.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryBuilds {
    pub capacity: BuildStat<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub regeneration: Option<BuildStat<DiveCapacityRate>>,

    /// Capacity over regeneration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recharge_time: Option<BuildStat<Seconds>>,
}

impl GameData {
    /// `None` for ships without a battery.
    pub(crate) fn battery_builds(&self, planner: &Planner<'_>) -> Result<Option<BatteryBuilds>, QueryError> {
        let capacity = planner.plan(&StatPlan::new(
            "dive capacity: seconds a full battery lasts at periscope depth or deeper, draining 1 per second at any depth and speed".to_owned(),
            Better::Higher,
            &|outcome| {
                let capacity = outcome.stats.battery.as_ref()?.capacity?;
                Some(Seconds::new(round(capacity.value())))
            },
        ))?;
        let Some(capacity) = capacity else {
            return Ok(None);
        };

        let regeneration = planner.plan(&StatPlan::new(
            "dive capacity recharge rate: seconds of dive capacity regained per second on the surface".to_owned(),
            Better::Higher,
            &|outcome| {
                let regeneration = outcome.stats.battery.as_ref()?.regeneration?;
                Some(DiveCapacityRate::from(round(regeneration.value())))
            },
        ))?;

        let recharge_time = planner.plan(&StatPlan::new(
            "dive capacity recharge time: seconds on the surface to refill an empty battery, capacity / regeneration"
                .to_owned(),
            Better::Lower,
            &|outcome| {
                let battery = outcome.stats.battery.as_ref()?;
                let regeneration = battery.regeneration?.value();
                if regeneration <= 0.0 {
                    return None;
                }

                Some(Seconds::new(round(battery.capacity?.value() / regeneration)))
            },
        ))?;

        Ok(Some(BatteryBuilds { capacity, regeneration, recharge_time }))
    }
}

//! Ship abilities a player switches on in battle: Combat Instructions
//! (GameParams `RageMode` in the hull's `specials` component) and the main
//! battery's alternative fire mode (`SwitchableModeArtilleryModule` in an
//! artillery component).

use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::types::CrewSkillModifier;
use crate::game_params::types::Meters;

/// Combat Instructions: a meter that in-battle events fill; when full, it
/// applies `modifiers` for `duration` (`RageModeComponentCommon`). Each use
/// empties the meter. No field limits the number of uses.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CombatInstructions {
    /// `rageModeName`, which names the title key
    /// `IDS_DOCK_RAGE_MODE_TITLE_<NAME>`.
    pub name: String,
    pub activation: Activation,

    /// `boostDuration`.
    pub duration: Seconds,

    /// `boostPreparation`: delay between a full meter and the ability being
    /// ready. `None` when zero.
    pub preparation: Option<Seconds>,

    /// `None` when the meter never loses progress.
    pub decay: Option<ProgressDecay>,

    pub modifiers: Vec<CrewSkillModifier>,

    /// Events that fill the meter, in GameParams order.
    pub triggers: Vec<ProgressTrigger>,
}

/// How Combat Instructions start once the meter is full (`isAutoUsage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum Activation {
    /// The player presses the ability key (`CMD_ACTIVATE_RAGE_MODE`).
    Manual,
    Automatic,
}

/// Percent of the Combat Instructions meter, 0 to 100.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ProgressPercent(pub f32);

/// Progress lost while no event adds any: after `delay`, `loss` per `period`
/// (`decrementDelay`, `decrementCount`, `decrementPeriod`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ProgressDecay {
    pub delay: Seconds,
    pub loss: ProgressPercent,
    pub period: Seconds,
}

/// One `GameLogicTrigger` whose action adds meter progress
/// (`RageModeProgressAction`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ProgressTrigger {
    /// `progressName`; `"default"` when the event alone names the trigger.
    /// The client labels it `IDS_RAGE_MODE_TRIGGER_<progressName or
    /// activator type>`.
    pub progress_name: String,

    /// Progress added each time the event happens `required_count` times.
    pub gain: ProgressPercent,
    pub event: ProgressEvent,
    pub required_count: u32,

    /// `maxActivations`; `None` when unlimited.
    pub max_activations: Option<u32>,

    /// `startEnabled`: false when another trigger switches this one on, such
    /// as staying undetected.
    pub starts_enabled: bool,
}

/// The activator of a progress trigger.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ProgressEvent {
    /// Earning any of the listed ribbons or sub-ribbons, by client id.
    Ribbon { ribbons: Vec<i64>, sub_ribbons: Vec<i64> },

    /// Each `per` HP of potential damage received.
    PotentialDamage { per: Hp },

    /// Every `period`.
    Timer { period: Seconds, repeating: bool },

    /// Using one of the listed consumables (`ConsumableUseActivator`).
    ConsumableUse { consumables: Vec<String> },

    /// A main battery salvo landing within `radius` of the target
    /// (`AdjustmentShotActivator`).
    AdjustmentShot { radius: Meters },

    /// Becoming detected (`visible`) or undetected.
    VisibilityChanged { visible: bool },

    /// An activator type this crate does not read.
    Other { activator: String },
}

/// The main battery's alternative fire mode, toggled in battle
/// (`CMD_TOGGLE_FIRE_MODE`). With `shots` above one, each turret fires that
/// many salvos `shot_interval` apart and then reloads for `full_reload`;
/// otherwise each shot reloads for `full_reload` (`m148110fe` burst logic).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AlternativeFireMode {
    /// `shotsCount`.
    pub shots: BurstShots,

    /// `burstReloadTime`: reload between the shots of one burst.
    pub shot_interval: Seconds,

    /// `fullReloadTime`: reload after the last shot of a burst, or after
    /// every shot without a burst.
    pub full_reload: Seconds,

    /// Applied only while the mode is on.
    pub modifiers: Vec<CrewSkillModifier>,

    /// `secondaryAmmoList` with its `secondaryAmmoPool`. Empty when the mode
    /// fires the standard shells.
    pub ammo: Vec<AlternativeAmmo>,
}

/// Salvos one turret fires per burst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BurstShots(pub u32);

/// A shell the alternative fire mode loads.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AlternativeAmmo {
    /// Projectile param name.
    pub name: String,

    /// Shells per battle; `None` when unlimited (`-1`).
    pub limit: Option<u32>,
}

impl AlternativeFireMode {
    /// Whether turrets fire bursts rather than single salvos.
    pub fn is_burst(&self) -> bool {
        self.shots.0 > 1
    }
}

//! Buffs that game modes hand out in battle.
//!
//! GameParams holds each buff drop (`maxBuffLevel`, and a `payload` naming
//! the `Modifier` params it applies, one per ship class) and each modifier's
//! values per stack level, but nothing ties a drop to a game mode. Arms Race
//! names its drops in the client script constant `ARMS_RACE_DROP_PARAMS`
//! (`mab132439/mb75a0472.py:44` in 15.10). The scripts are compiled bytecode
//! that the client re-obfuscates every build, and the game data wows-query
//! loads does not include them, so the drop names are kept here; their
//! values are read from GameParams.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::names::search_key;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::text::clean;

/// Arguments for `game_mode_buffs`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameModeBuffsArgs {
    /// Game mode, e.g. "Arms Race". Omit for every mode with buffs.
    pub mode: Option<String>,

    /// Only the effects for this ship's class, e.g. "Akizuki".
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameModeBuffsOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipSummary>,

    pub modes: Vec<GameModeBuffs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameModeBuffs {
    pub mode: String,

    /// How the mode hands out buffs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<String>,

    pub buffs: Vec<ModeBuff>,
}

/// A buff drop. Picking one up buffs every ship of the team; picking up the
/// same buff again adds a stack, up to `max_stacks`.
#[derive(Debug, Clone, Serialize)]
pub struct ModeBuff {
    pub name: String,
    pub max_stacks: u32,

    /// Effects per number of stacks, prefixed by class when they differ
    /// between classes.
    pub stacks: Vec<BuffStack>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuffStack {
    pub stacks: u32,
    pub effects: Vec<String>,
}

/// A game mode and the buff drops it hands out.
struct ModeDrops {
    /// Translation key of the mode's name.
    name: &'static str,

    /// Translation key of how the mode hands out buffs.
    rules: &'static str,

    /// Drop param names, in the client's order.
    drops: &'static [&'static str],
}

const MODES: &[ModeDrops] = &[ModeDrops {
    name: "IDS_ARMSRACE",
    rules: "IDS_TIPS_ARMSRACE_DESCRIPTION",
    drops: &["PCOD002_Regeneration", "PCOD041_AR_AddHeath", "PCOD003_ShotDelay", "PCOD042_AR_AddWeaponDamage"],
}];

impl GameData {
    /// The buffs of each game mode that hands them out, per class and stack.
    pub fn game_mode_buffs(&self, args: &GameModeBuffsArgs) -> Result<GameModeBuffsOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };

        let modes: Vec<&ModeDrops> = match args.mode.as_deref() {
            Some(query) => {
                let key = search_key(query);
                let matching: Vec<&ModeDrops> =
                    MODES.iter().filter(|mode| search_key(&self.mode_name(mode)).contains(&key)).collect();
                if matching.is_empty() {
                    return Err(QueryError::GameModeNotFound {
                        query: query.to_owned(),
                        modes: MODES.iter().map(|mode| self.mode_name(mode)).collect(),
                    });
                }
                matching
            }
            None => MODES.iter().collect(),
        };

        let class = ship.map(|ship| ship.class);
        Ok(GameModeBuffsOutput {
            interpretations,
            ship: ship.map(Ship::summary),
            modes: modes.into_iter().map(|mode| self.mode_buffs(mode, class)).collect::<Result<_, _>>()?,
        })
    }

    /// The buffs of every mode for `class` that restore HP.
    pub(crate) fn mode_heals(&self, class: ShipClass) -> Result<Vec<GameModeBuffs>, QueryError> {
        let mut modes = Vec::new();
        for mode in MODES {
            let heals = self.mode_drops(mode, &[class], |modifiers| {
                crate::buffs::buff_heal_rate(modifiers, class.species()).is_some()
            })?;
            if !heals.is_empty() {
                modes.push(GameModeBuffs {
                    mode: self.mode_name(mode),
                    rules: self.translate(mode.rules),
                    buffs: heals,
                });
            }
        }

        Ok(modes)
    }

    fn mode_buffs(&self, mode: &ModeDrops, class: Option<ShipClass>) -> Result<GameModeBuffs, QueryError> {
        let classes = class.map_or_else(|| ShipClass::ALL.to_vec(), |class| vec![class]);

        Ok(GameModeBuffs {
            mode: self.mode_name(mode),
            rules: self.translate(mode.rules),
            buffs: self.mode_drops(mode, &classes, |_| true)?,
        })
    }

    /// The drops of `mode` with effects for `classes`, keeping those whose
    /// first stack `keep` accepts for some class.
    fn mode_drops(
        &self,
        mode: &ModeDrops,
        classes: &[ShipClass],
        keep: impl Fn(&[CrewSkillModifier]) -> bool,
    ) -> Result<Vec<ModeBuff>, QueryError> {
        let mut buffs = Vec::new();
        for name in mode.drops {
            let Some(param) = self.provider().game_param_by_name(name) else {
                continue;
            };
            let Some(drop) = param.drop_data() else {
                continue;
            };
            let max_stacks = drop
                .max_buff_level()
                .ok_or_else(|| QueryError::BuffDropWithoutStackLimit { drop: (*name).to_owned() })?;

            // The modifiers of `stack` for one class: every payload modifier
            // that applies to it.
            let modifiers = |stack: u32, species: Species| {
                drop.buffs()
                    .iter()
                    .filter_map(|buff| self.buff_modifiers(buff, stack as usize, species))
                    .flatten()
                    .collect::<Vec<_>>()
            };
            if !classes.iter().any(|class| keep(&modifiers(1, class.species()))) {
                continue;
            }

            let stacks = (1..=max_stacks)
                .map(|stack| BuffStack {
                    stacks: stack,
                    effects: self
                        .per_class_lines(classes, |species| self.buff_lines(&modifiers(stack, species), species, None)),
                })
                .collect();

            buffs.push(ModeBuff { name: self.drop_name(drop.marker_name_active(), name), max_stacks, stacks });
        }

        Ok(buffs)
    }

    fn mode_name(&self, mode: &ModeDrops) -> String {
        self.translate(mode.name).unwrap_or_else(|| mode.name.to_owned())
    }

    /// The name the client gives a drop in quick commands
    /// (`QCmdTarget_DropZone`), without its exclamation mark.
    fn drop_name(&self, marker: &str, param: &str) -> String {
        self.provider()
            .localized_name_from_id(&TranslationKey::new(format!("IDS_CMD_QUICK_DROP_{}", marker.to_uppercase())))
            .map(|name| clean(&name).trim_end_matches('!').to_owned())
            .unwrap_or_else(|| param.to_owned())
    }
}

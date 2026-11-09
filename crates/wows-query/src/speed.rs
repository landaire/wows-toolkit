//! Top speed at each depth on the build players run.
//!
//! The game multiplies a ship's top speed by `speedCoef` everywhere, by
//! `speedCoefBattery` on the surface and at periscope depth, and by
//! `speedCoefUW` below periscope depth (`getShipSpeedCoef`,
//! ModifiersApply.py:531-546). Captain skills and upgrades come from the
//! build planner. What holds only part of a battle is listed as an
//! alternative, never in the best-fit build: signal flags carrying one of
//! these modifiers (the port lets any ship mount them), the ship's special
//! ability, the commander's talents and speed boost consumables such as
//! Engine Boost.
//!
//! A speed boost adds to that coefficient instead of multiplying it:
//! `Engine.getBoosterSpeedCoef` is the additive identity 0.0 without a boost
//! and `(boostCoeff + boostCoeffForsage) * engineForsageCoef` with one
//! (Components/__init__.py:326, CommonConsumables/Logic/Ship.py:56).
//! Replays agree: the server top speed of a Marceau with Sierra Mike rises
//! by 1.25 / 1.05 when its +20% Engine Boost starts, not by 1.2.
//!
//! The fastest speed a ship reaches holds every condition that can hold at
//! once. Conditional skills stack only when their triggers can hold at the
//! same moment: Swift in Silence and Brisk need the ship undetected
//! (`entityIsInvisibleTrigger`), Dazzle starts at detection
//! (`activationOnDetectTrigger`, 15 s), so Dazzle never stacks with them
//! although a captain can learn both within 21 points.

use std::iter;

use serde::Serialize;
use wowsunpack::game_params::ttx::model::Knots;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::GameParamProvider;

use crate::builds::Better;
use crate::builds::BuildAlternative;
use crate::builds::BuildStat;
use crate::builds::BuildValue;
use crate::builds::Item;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::commanders::activation_counts;
use crate::commanders::talent_modifiers;
use crate::consumables::SPEED_BOOSTER;
use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AbilityModifiers;

/// Top speed on the surface and, for submarines, at periscope depth and
/// submerged.
#[derive(Debug, Clone, Serialize)]
pub struct SpeedBuilds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<BuildStat<Knots>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub periscope_depth: Option<BuildStat<Knots>>,

    /// At the fastest of operating and maximum depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submerged: Option<BuildStat<Knots>>,
}

/// A depth with a top speed of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpeedDepth {
    Surface,
    Periscope,
    Submerged,
}

impl SpeedDepth {
    fn label(self) -> &'static str {
        match self {
            Self::Surface => "top speed on the surface",
            Self::Periscope => "top speed at periscope depth, just below the surface; not the underwater speed",
            Self::Submerged => "submerged speed: top speed fully underwater, at maximum depth",
        }
    }

    /// Top speed on the stat card of `outcome`.
    fn speed(self, outcome: &Outcome) -> Option<Knots> {
        match self {
            Self::Surface => outcome.stats.mobility.as_ref()?.speed,
            Self::Periscope => outcome.stats.underwater_mobility.as_ref()?.periscope_speed,
            Self::Submerged => outcome.stats.underwater_mobility.as_ref()?.speed,
        }
    }

    /// `getShipSpeedCoef` at this depth, the coefficient the card speed
    /// carries.
    fn coefficient(self, bundle: &ModifierBundle) -> f32 {
        let depth = match self {
            Self::Surface | Self::Periscope => bundle.coef("speedCoefBattery"),
            Self::Submerged => bundle.coef("speedCoefUW"),
        };

        bundle.coef("speedCoef") * depth
    }
}

/// Share of the hull's top speed a speed boost consumable adds while
/// active, its `logic.boostCoeff`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BoostCoefficient(f32);

/// A consumable of the ship that raises top speed while active.
struct SpeedBooster {
    name: String,
    boost: BoostCoefficient,
}

impl SpeedBooster {
    /// Top speed at `depth` on `outcome` with the booster active. Its
    /// `availableBuoyancyStates` are empty, so it works at every depth.
    fn speed(&self, depth: SpeedDepth, outcome: &Outcome) -> Option<Knots> {
        let speed = depth.speed(outcome)?;
        let bundle = &outcome.bundle;
        let coefficient = depth.coefficient(bundle);
        let added = (self.boost.0 + bundle.bonus("boostCoeffForsage")) * bundle.coef("engineForsageCoef");

        Some(Knots::from(speed.value() * (coefficient + added) / coefficient))
    }

    fn note(&self) -> String {
        format!(
            "Only while {} is active. It adds {}% of the hull's top speed; other speed bonuses add to it instead of multiplying it.",
            self.name,
            round(self.boost.0 * 100.0)
        )
    }
}

/// Modifiers switched on while a condition holds, with the builds that
/// carry them.
struct Switched<'a> {
    planner: Planner<'a>,

    /// The condition in a label, e.g. "Sierra Mike signal".
    phrase: String,

    /// The condition in `assumed`.
    assumed: String,

    note: Option<String>,
}

/// The conditions of one ship that may raise its top speed.
struct Conditions<'a> {
    switched: Vec<Switched<'a>>,
    boosters: Vec<SpeedBooster>,
}

/// A special ability that changes top speed, with its effects.
struct SpeedAbility {
    /// e.g. "Combat Instructions: Consumable Operational Readiness".
    names: String,

    modifiers: Vec<AbilityModifiers>,
}

/// One condition held: modifiers switched on, a speed booster active, or
/// both.
#[derive(Clone, Copy)]
struct Held<'c, 'a> {
    switched: Option<&'c Switched<'a>>,
    booster: Option<&'c SpeedBooster>,
}

impl Held<'_, '_> {
    /// e.g. "Engine Boost active and Sierra Mike signal".
    fn phrase(self) -> String {
        let booster = self.booster.map(|booster| format!("{} active", booster.name));
        let switched = self.switched.map(|switched| switched.phrase.clone());
        let phrases: Vec<String> = booster.into_iter().chain(switched).collect();

        phrases.join(" and ")
    }

    fn assumed(self) -> Vec<String> {
        let booster = self.booster.map(|booster| booster.name.clone());
        let switched = self.switched.map(|switched| switched.assumed.clone());

        booster.into_iter().chain(switched).collect()
    }

    fn note(self) -> Option<String> {
        let booster = self.booster.map(SpeedBooster::note);
        let switched = self.switched.and_then(|switched| switched.note.clone());
        let notes: Vec<String> = booster.into_iter().chain(switched).collect();

        (!notes.is_empty()).then(|| notes.join(" "))
    }
}

impl GameData {
    pub(crate) fn speed_builds(&self, planner: &Planner<'_>) -> Result<SpeedBuilds, QueryError> {
        let conditions = self.speed_conditions(planner)?;

        Ok(SpeedBuilds {
            surface: speed_plan(planner, &conditions, SpeedDepth::Surface)?,
            periscope_depth: speed_plan(planner, &conditions, SpeedDepth::Periscope)?,
            submerged: speed_plan(planner, &conditions, SpeedDepth::Submerged)?,
        })
    }

    /// The builds of the top speed at `depth`, as in [`Self::speed_builds`].
    pub(crate) fn speed_build(
        &self,
        planner: &Planner<'_>,
        depth: SpeedDepth,
    ) -> Result<Option<BuildStat<Knots>>, QueryError> {
        let conditions = self.speed_conditions(planner)?;

        speed_plan(planner, &conditions, depth)
    }

    fn speed_conditions<'a>(&self, planner: &Planner<'a>) -> Result<Conditions<'a>, QueryError> {
        let switch_on = |ability: AbilityModifiers| {
            let mut abilities = planner.abilities().to_vec();
            abilities.push(ability);
            planner.with_abilities_on(abilities)
        };

        let mut switched: Vec<Switched<'a>> = self
            .speed_signals()
            .into_iter()
            .map(|signal| {
                let phrase = format!("{} signal", signal.label);
                Switched { planner: switch_on(signal), assumed: phrase.clone(), phrase, note: None }
            })
            .collect();

        if let Some(ability) = self.speed_ability(planner)? {
            let mut abilities = planner.abilities().to_vec();
            abilities.extend(ability.modifiers);
            switched.push(Switched {
                planner: planner.with_abilities_on(abilities),
                phrase: format!("{} active", ability.names),
                assumed: ability.names.clone(),
                note: Some(format!("Only while {} is active.", ability.names)),
            });
        }

        if let Some(commander) = planner.commander() {
            let species = planner.ship().class.species();
            for talent in commander.random_battle_talents().filter(|talent| talent.allowed_classes().contains(&species))
            {
                let key = talent.translation_key(&commander.index);
                let name = self.talent_name(&key);
                let counts = activation_counts(talent);
                let multiple = counts.len() > 1 || talent.max_activations().is_some_and(|limit| limit > 1);
                for count in counts {
                    let modifiers = talent_modifiers(talent, count, species, self);
                    if !modifiers.iter().any(is_speed_modifier) {
                        continue;
                    }

                    let (phrase, assumed) = if multiple {
                        let activations =
                            if count == 1 { "1 activation".to_owned() } else { format!("{count} activations") };
                        (format!("{name} talent active after {activations}"), format!("{name} talent ({activations})"))
                    } else {
                        (format!("{name} talent active"), format!("{name} talent"))
                    };
                    let condition = self.talent_condition(&key, talent);
                    let activation = multiple.then(|| self.activation_condition(&key, talent, count)).flatten();
                    let notes: Vec<String> = condition.into_iter().chain(activation).collect();
                    let ability = AbilityModifiers { source: name.clone(), label: name.clone(), modifiers };
                    switched.push(Switched {
                        planner: switch_on(ability),
                        phrase,
                        assumed,
                        note: (!notes.is_empty()).then(|| notes.join(" ")),
                    });
                }
            }
        }

        Ok(Conditions { switched, boosters: self.speed_boosters(planner) })
    }

    /// The fastest top speed at `depth` with every condition that can hold
    /// at once: each speed signal mounted, the special ability active, the
    /// strongest speed booster active and the conditional skills whose
    /// triggers can hold together met (see
    /// [`Planner::triggers_hold_together`]). Of combinations equally fast,
    /// the one with fewer timed triggers, then fewer skills, is named.
    /// Commander talents are not counted. `None` when the ship lacks the
    /// speed or nothing raises its always-on best.
    pub(crate) fn fastest_speed(
        &self,
        planner: &Planner<'_>,
        depth: SpeedDepth,
    ) -> Result<Option<BuildAlternative<Knots>>, QueryError> {
        let knots = |value: Knots| Knots::from(round(value.value()));
        let card = |outcome: &Outcome| depth.speed(outcome).map(knots);
        let Some(always_on) = planner.best(&StatPlan::new(depth.label().to_owned(), Better::Higher, &card))? else {
            return Ok(None);
        };

        let signals = self.speed_signals();
        let ability = self.speed_ability(planner)?;
        let mut abilities = planner.abilities().to_vec();
        abilities.extend(signals.iter().cloned());
        if let Some(ability) = &ability {
            abilities.extend(ability.modifiers.iter().cloned());
        }
        let held = planner.with_abilities_on(abilities);
        let booster = self
            .speed_boosters(planner)
            .into_iter()
            .reduce(|strongest, booster| if booster.boost.0 > strongest.boost.0 { booster } else { strongest });

        let raised = |skill: Item| -> Result<bool, QueryError> {
            let idle = depth.speed(&*held.with_items(&[skill])?);
            let active = held.with_skill_triggered(skill)?.and_then(|outcome| depth.speed(&outcome));
            Ok(idle.zip(active).is_some_and(|(idle, active)| Better::Higher.improves(active.value(), idle.value())))
        };
        let mut skills: Vec<Item> = Vec::new();
        for &skill in held.triggered_items() {
            if raised(skill)? {
                skills.push(skill);
            }
        }

        let speed = |outcome: &Outcome| {
            let speed = match &booster {
                Some(booster) => booster.speed(depth, outcome),
                None => depth.speed(outcome),
            };
            speed.map(knots)
        };
        let plan = StatPlan::new(depth.label().to_owned(), Better::Higher, &speed);
        let mut fastest: Option<(Vec<Item>, BuildValue<Knots>)> = None;
        for mask in 0..1_usize << skills.len() {
            let subset: Vec<Item> = skills
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, skill)| *skill)
                .collect();
            if !held.triggers_hold_together(&subset) {
                continue;
            }
            let Some(value) = held.best_triggered(&plan, &subset)? else {
                continue;
            };

            let timed = |skills: &[Item]| skills.iter().filter(|skill| held.trigger_is_timed(**skill)).count();
            let replaces = match &fastest {
                None => true,
                Some((best_skills, best)) => {
                    Better::Higher.improves(value.value.value(), best.value.value())
                        || (!Better::Higher.improves(best.value.value(), value.value.value())
                            && (timed(&subset), subset.len()) < (timed(best_skills), best_skills.len()))
                }
            };
            if replaces {
                fastest = Some((subset, value));
            }
        }

        let Some((subset, best)) = fastest else {
            return Ok(None);
        };
        if !Better::Higher.improves(best.value.value(), always_on.best.value.value()) {
            return Ok(None);
        }

        let mut phrases: Vec<String> = subset.iter().map(|skill| format!("{} active", held.name(*skill))).collect();
        let mut assumed = best.assumed;
        let mut notes: Vec<String> = subset.iter().filter_map(|skill| held.trigger_sentence(*skill)).collect();
        if let Some(booster) = &booster {
            phrases.push(format!("{} active", booster.name));
            assumed.push(booster.name.clone());
            notes.push(booster.note());
        }
        for signal in &signals {
            let phrase = format!("{} signal", signal.label);
            phrases.push(phrase.clone());
            assumed.push(phrase);
        }
        if let Some(ability) = ability {
            phrases.push(format!("{} active", ability.names));
            notes.push(format!("Only while {} is active.", ability.names));
            assumed.push(ability.names);
        }

        Ok(Some(BuildAlternative {
            label: format!("with {}", phrases.join(" and ")),
            value: best.value,
            assumed,
            tier_fillers: best.tier_fillers,
            note: (!notes.is_empty()).then(|| notes.join(" ")),
        }))
    }

    /// The ship's special ability when it changes top speed and is not
    /// switched on in every build. Another fire mode does not count: it
    /// changes the guns, not the hull.
    fn speed_ability(&self, planner: &Planner<'_>) -> Result<Option<SpeedAbility>, QueryError> {
        if planner.has_ability() {
            return Ok(None);
        }

        match self.active_ability(planner.ship()) {
            Ok(ability) if ability.switched.is_none() && ability.modifiers.iter().any(changes_speed) => {
                Ok(Some(SpeedAbility { names: ability.names.join(", "), modifiers: ability.modifiers }))
            }
            Ok(_) | Err(QueryError::NoSpecialAbility { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Signal flags whose modifiers change top speed.
    fn speed_signals(&self) -> Vec<AbilityModifiers> {
        self.provider()
            .params()
            .iter()
            .filter_map(|param| {
                let exterior = param.exterior().filter(|exterior| !exterior.hidden())?;
                if !exterior.modifiers().iter().any(is_speed_modifier) {
                    return None;
                }
                // Exteriors are named by param name, e.g. IDS_PCEF005_SM_SIGNALFLAG.
                let name = self.translate(&format!("IDS_{}", param.name().to_uppercase()))?;
                Some(AbilityModifiers {
                    source: param.name().to_owned(),
                    label: name,
                    modifiers: exterior.modifiers().to_vec(),
                })
            })
            .collect()
    }

    /// Speed boost consumables of the planner's ship, one per name.
    fn speed_boosters(&self, planner: &Planner<'_>) -> Vec<SpeedBooster> {
        let ship = planner.ship();
        let slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();
        let mut found: Vec<SpeedBooster> = Vec::new();

        for (ability, variant) in slots.iter().flatten() {
            let Some(param) = self.provider().game_param_by_name(ability) else {
                continue;
            };
            let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                continue;
            };
            if category.consumable_type_raw() != SPEED_BOOSTER {
                continue;
            }
            let Some(&boost) = category.effect_fields().get("boostCoeff").filter(|boost| **boost > 0.0) else {
                continue;
            };

            let name = self.consumable_name(ability, category);
            if !found.iter().any(|known| known.name == name) {
                found.push(SpeedBooster { name, boost: BoostCoefficient(boost) });
            }
        }

        found
    }
}

/// Whether `label`, the label of a speed build alternative, names the
/// conditions `query` names. Case, "with", "active" and "signal" do not
/// matter, nor the order of conditions joined by "and".
pub(crate) fn names_condition(label: &str, query: &str) -> bool {
    fn conditions(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let words: Vec<&str> = lower
            .split_whitespace()
            .enumerate()
            .filter(|(index, word)| !(*index == 0 && *word == "with") && !matches!(*word, "active" | "signal"))
            .map(|(_, word)| word)
            .collect();
        let mut conditions: Vec<String> =
            words.join(" ").split(" and ").map(|condition| condition.trim().to_owned()).collect();
        conditions.sort_unstable();

        conditions
    }

    conditions(label) == conditions(query)
}

/// The builds of the top speed at `depth`, with each condition that raises
/// the best-fit value as an alternative, alone and together with each
/// conditional skill that raises it further. Speed boosters count alone and
/// with each set of switched modifiers. `None` when the ship lacks the
/// speed.
fn speed_plan(
    planner: &Planner<'_>,
    conditions: &Conditions<'_>,
    depth: SpeedDepth,
) -> Result<Option<BuildStat<Knots>>, QueryError> {
    let knots = |value: Knots| Knots::from(round(value.value()));
    let card = |outcome: &Outcome| depth.speed(outcome).map(knots);
    let Some(mut stat) = planner.plan(&StatPlan::new(depth.label().to_owned(), Better::Higher, &card))? else {
        return Ok(None);
    };

    let switched = conditions.switched.iter().map(|switched| Held { switched: Some(switched), booster: None });
    let boosted = conditions.boosters.iter().flat_map(|booster| {
        iter::once(Held { switched: None, booster: Some(booster) }).chain(
            conditions.switched.iter().map(move |switched| Held { switched: Some(switched), booster: Some(booster) }),
        )
    });
    for held in switched.chain(boosted) {
        let planner = held.switched.map_or(planner, |switched| &switched.planner);
        let speed = |outcome: &Outcome| {
            let speed = match held.booster {
                Some(booster) => booster.speed(depth, outcome),
                None => depth.speed(outcome),
            };
            speed.map(knots)
        };
        let Some(with_condition) = planner.plan(&StatPlan::new(depth.label().to_owned(), Better::Higher, &speed))?
        else {
            continue;
        };
        let best = with_condition.builds.best;
        if !Better::Higher.improves(best.value.value(), stat.builds.best.value.value()) {
            continue;
        }

        let phrase = held.phrase();
        let condition = held.assumed();
        let combined: Vec<BuildAlternative<Knots>> = with_condition
            .builds
            .alternatives
            .into_iter()
            .filter(|alternative| Better::Higher.improves(alternative.value.value(), best.value.value()))
            .map(|mut alternative| {
                alternative.label = format!("{} and {phrase}", alternative.label);
                alternative.assumed.extend(condition.iter().cloned());
                alternative
            })
            .collect();

        let mut assumed = best.assumed;
        assumed.extend(condition);
        stat.builds.alternatives.push(BuildAlternative {
            label: format!("with {phrase}"),
            value: best.value,
            assumed,
            tier_fillers: best.tier_fillers,
            note: held.note(),
        });
        stat.builds.alternatives.extend(combined);
    }

    Ok(Some(stat))
}

fn is_speed_modifier(modifier: &CrewSkillModifier) -> bool {
    const SPEED_MODIFIERS: [&str; 3] = ["speedCoef", "speedCoefBattery", "speedCoefUW"];

    SPEED_MODIFIERS.contains(&modifier.name())
}

fn changes_speed(ability: &AbilityModifiers) -> bool {
    ability.modifiers.iter().any(is_speed_modifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_names_ignore_case_order_and_filler_words() {
        let label = "with Swift in Silence active and Engine Boost active and Sierra Mike signal";

        assert!(names_condition(label, "sierra mike and engine boost and swift in silence"));
        assert!(names_condition("with Engine Boost active", "Engine Boost"));
        assert!(!names_condition(label, "Engine Boost"));
        assert!(!names_condition("with Engine Boost active and Sierra Mike signal", "Engine Boost"));
    }
}

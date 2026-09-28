//! What a shell cast through an armor model ended up doing.
//!
//! The simulation itself is `wowsunpack::ballistics`; this reads its result back
//! as a sentence both front ends can put beside the drawn arc, so the armor
//! viewers say the same thing about the same cast.

use std::collections::HashMap;

use wowsunpack::ballistics::PlateOutcome;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::ShellSimResult;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::game_params::types::Degrees;
use wowsunpack::game_params::types::Millimeters;

/// One plate along the cast, as far as reading the outcome is concerned.
///
/// Neither front end's own hit type reaches here: each carries a position in its
/// own vector type, and none of that says anything about the outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcPlate {
    /// The armor zone the plate belongs to, as the model names it.
    pub zone: String,
    pub thickness: Millimeters,
    /// The material key, which the caller translates for display.
    pub material: String,
}

/// The plate a shell's run ended at.
#[derive(Clone, Debug, PartialEq)]
pub struct StoppingPlate {
    /// Its place along the cast, counting from one, as the viewers number them.
    pub number: usize,
    pub thickness: Millimeters,
    pub material: String,
}

/// How a cast shell's run ended.
#[derive(Clone, Debug, PartialEq)]
pub enum ArcOutcome {
    /// The fuse went off. `zone` is the volume it went off inside, and `None`
    /// means it was already past every zone.
    Detonated {
        zone: Option<String>,
    },
    Ricocheted {
        plate: Option<StoppingPlate>,
    },
    Shattered {
        plate: Option<StoppingPlate>,
    },
    /// Stopped without ricocheting or shattering: the shell ran out of velocity.
    Stopped {
        plate: Option<StoppingPlate>,
    },
    /// Through everything. `fuse_armed` is false for a shell whose fuse never
    /// armed at all, which is a different thing from one that armed too late.
    Overpenetrated {
        fuse_armed: bool,
    },
    /// No shell was cast, so there is nothing to say about one.
    NotSimulated,
}

/// The zone the shell is inside after crossing the plates up to `up_to`.
///
/// Every crossing of a zone's boundary toggles whether the shell is within it,
/// so a zone crossed an odd number of times has been entered and not left. The
/// innermost such zone is the one the shell sits in; `None` once it is clear of
/// every zone.
pub fn enclosing_zone<'a>(zones: &[&'a str], up_to: usize) -> Option<&'a str> {
    let crossed = &zones[..zones.len().min(up_to)];

    let mut crossings: HashMap<&str, usize> = HashMap::new();
    for zone in crossed {
        *crossings.entry(zone).or_default() += 1;
    }

    crossed.iter().rev().copied().find(|zone| crossings[zone] % 2 == 1)
}

/// Reads `sim` back as what the shell did to `plates`.
pub fn describe_arc(sim: Option<&ShellSimResult>, plates: &[ArcPlate]) -> ArcOutcome {
    let Some(sim) = sim else { return ArcOutcome::NotSimulated };

    if let Some(detonated) = sim.detonated_at {
        let zones: Vec<&str> = plates.iter().map(|plate| plate.zone.as_str()).collect();
        let zone = enclosing_zone(&zones, detonated.number()).map(str::to_owned);
        return ArcOutcome::Detonated { zone };
    }

    if let Some(stopped) = sim.stopped_at {
        let plate = plates.get(stopped.value()).map(|plate| StoppingPlate {
            number: stopped.number(),
            thickness: plate.thickness,
            material: plate.material.clone(),
        });
        return match sim.plates.last().map(|plate| &plate.outcome) {
            Some(PlateOutcome::Ricochet) => ArcOutcome::Ricocheted { plate },
            Some(PlateOutcome::Shatter) => ArcOutcome::Shattered { plate },
            _ => ArcOutcome::Stopped { plate },
        };
    }

    ArcOutcome::Overpenetrated { fuse_armed: sim.detonation.is_some() }
}

/// One zone of an armor model, as isolating plates needs to see it: the parts it
/// holds and, per part, the plate thicknesses in tenths of a millimetre.
pub struct ZoneShape<'a> {
    pub name: &'a str,
    pub parts: Vec<(&'a str, &'a [i32])>,
}

/// A plate, by the three things that tell one from another.
pub type PlateKeyParts = (String, String, i32);

/// Which parts and plates a viewer shows after isolating what a cast hit.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Isolation {
    /// Keyed by `(zone, part)`. `true` shows the part.
    pub parts: HashMap<(String, String), bool>,
    /// Keyed by plate. `false` hides that plate; a plate with no entry is shown.
    pub plates: HashMap<PlateKeyParts, bool>,
}

/// Hides everything a cast did not reach.
///
/// A zone that was hit is shown whole, because the question being asked of a
/// zone is what the shell went through to reach it. A zone that was not hit is
/// shown only as the individual plates the shell crossed, and its other parts
/// are hidden outright. With nothing hit, nothing is hidden: an empty isolation
/// is what puts the whole ship back.
pub fn isolate(
    zones: &[ZoneShape<'_>],
    hit_zones: &std::collections::HashSet<String>,
    hit_plates: &std::collections::HashSet<PlateKeyParts>,
) -> Isolation {
    let mut isolation = Isolation::default();
    if hit_zones.is_empty() && hit_plates.is_empty() {
        return isolation;
    }

    for zone in zones {
        let whole_zone_hit = hit_zones.contains(zone.name);
        for (part, thicknesses) in &zone.parts {
            let part_key = (zone.name.to_owned(), (*part).to_owned());
            if whole_zone_hit {
                isolation.parts.insert(part_key, true);
                continue;
            }
            let part_was_hit = thicknesses
                .iter()
                .any(|thickness| hit_plates.contains(&(zone.name.to_owned(), (*part).to_owned(), *thickness)));
            if !part_was_hit {
                isolation.parts.insert(part_key, false);
                continue;
            }
            isolation.parts.insert(part_key, true);
            for thickness in *thicknesses {
                let plate = (zone.name.to_owned(), (*part).to_owned(), *thickness);
                if !hit_plates.contains(&plate) {
                    isolation.plates.insert(plate, false);
                }
            }
        }
    }
    isolation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_crossed_twice_has_been_left_again() {
        let zones = ["hull", "citadel", "citadel", "hull"];
        assert_eq!(enclosing_zone(&zones, 4), None);
        assert_eq!(enclosing_zone(&zones, 2), Some("citadel"));
        assert_eq!(enclosing_zone(&zones, 1), Some("hull"));
    }

    #[test]
    fn isolating_nothing_hides_nothing() {
        let zones = [ZoneShape { name: "hull", parts: vec![("plating", &[100][..])] }];
        let isolation = isolate(&zones, &Default::default(), &Default::default());
        assert_eq!(isolation, Isolation::default());
    }

    #[test]
    fn a_zone_that_was_hit_is_shown_whole_and_the_rest_is_hidden() {
        let zones = [
            ZoneShape { name: "citadel", parts: vec![("belt", &[380][..])] },
            ZoneShape { name: "hull", parts: vec![("plating", &[250, 320][..])] },
        ];
        let hit_zones = std::collections::HashSet::from(["citadel".to_owned()]);
        let hit_plates = std::collections::HashSet::from([("hull".to_owned(), "plating".to_owned(), 250)]);

        let isolation = isolate(&zones, &hit_zones, &hit_plates);

        assert_eq!(isolation.parts.get(&("citadel".to_owned(), "belt".to_owned())), Some(&true));
        assert_eq!(isolation.parts.get(&("hull".to_owned(), "plating".to_owned())), Some(&true));
        // The plating the shell did not cross is hidden; the one it did is not.
        assert_eq!(isolation.plates.get(&("hull".to_owned(), "plating".to_owned(), 320)), Some(&false));
        assert_eq!(isolation.plates.get(&("hull".to_owned(), "plating".to_owned(), 250)), None);
    }

    #[test]
    fn a_cast_with_no_shell_says_nothing_about_one() {
        assert_eq!(describe_arc(None, &[]), ArcOutcome::NotSimulated);
    }
}

/// The first plate a shell met, which is what a ricochet is decided on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    /// How square the strike was, measured from the plate's normal.
    pub angle: Degrees,
    pub thickness: Millimeters,
}

/// What the simulation says became of the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimOutcome {
    Citadel,
    Penetration,
    Overpenetration,
    Ricochet,
    Shatter,
    /// Stopped in the armor without an identified ricochet or shatter.
    Stopped,
}

impl SimOutcome {
    /// The key naming this outcome, for a front end that translates it.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Citadel => "ui.armor.outcome.citadel",
            Self::Penetration => "ui.armor.outcome.penetration",
            Self::Overpenetration => "ui.armor.outcome.overpenetration",
            Self::Ricochet => "ui.armor.outcome.ricochet",
            Self::Shatter => "ui.armor.outcome.shatter",
            Self::Stopped => "ui.armor.outcome.stopped",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Citadel => "Citadel",
            Self::Penetration => "Penetration",
            Self::Overpenetration => "Overpenetration",
            Self::Ricochet => "Ricochet",
            Self::Shatter => "Shatter",
            Self::Stopped => "Stopped",
        }
    }
}

/// The simulation's side of a disagreement with the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimVerdict {
    /// The simulation ran to this outcome.
    Outcome(SimOutcome),
    /// The plate is overmatched, which rules a ricochet out entirely.
    OvermatchRulesOutRicochet,
    /// The strike sits in the band where a ricochet is certain.
    AlwaysRicochetBand,
    /// The strike is too shallow for a ricochet to be possible at all.
    RicochetAngleTooLow,
}

impl SimVerdict {
    /// The key naming this verdict, for a front end that translates it. An
    /// outcome names itself, so that case reads the outcome's own key.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Outcome(outcome) => outcome.label_key(),
            Self::OvermatchRulesOutRicochet => "ui.armor.sim.overmatch_rules_out_ricochet",
            Self::AlwaysRicochetBand => "ui.armor.sim.always_ricochet_band",
            Self::RicochetAngleTooLow => "ui.armor.sim.ricochet_angle_too_low",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Outcome(outcome) => outcome.display_name(),
            Self::OvermatchRulesOutRicochet => "Overmatch (can't ricochet)",
            Self::AlwaysRicochetBand => "Ricochet (always-ricochet zone)",
            Self::RicochetAngleTooLow => "No ricochet possible (angle too low)",
        }
    }
}

/// How our simulation compares to the server.
#[derive(Clone, Debug)]
pub enum ComparisonVerdict {
    /// Simulation matches server.
    Match,
    /// Angle is in the ricochet RNG zone; server's call is valid either way.
    RicochetRngDefer { angle: Degrees, ricochet_start: Degrees, always_ricochet: Degrees },
    /// Simulation disagrees with server.
    Mismatch { sim: SimVerdict, server: crate::armor::incoming::ServerOutcome },
}

/// Classify the simulation's outcome for the shell.
pub fn sim_outcome(sim: &ShellSimResult, plates: &[ArcPlate]) -> SimOutcome {
    // A detonation takes priority even if the shell shattered or ricocheted on a
    // later plate: the fragments still explode.
    if sim.detonation.is_some() {
        let Some(detonated_at) = sim.detonated_at else {
            return SimOutcome::Overpenetration;
        };
        let zones: Vec<&str> = plates.iter().map(|plate| plate.zone.as_str()).collect();
        let inside_citadel =
            enclosing_zone(&zones, detonated_at.number()).is_some_and(|zone| zone.to_lowercase().contains("citadel"));
        return if inside_citadel { SimOutcome::Citadel } else { SimOutcome::Penetration };
    }

    let Some(stopped_at) = sim.stopped_at else {
        return SimOutcome::Overpenetration;
    };
    match sim.plates.get(stopped_at.value()).map(|plate| plate.outcome) {
        Some(wowsunpack::ballistics::PlateOutcome::Ricochet) => SimOutcome::Ricochet,
        Some(wowsunpack::ballistics::PlateOutcome::Shatter) => SimOutcome::Shatter,
        _ => SimOutcome::Stopped,
    }
}

/// Compare a shell simulation result against the server's authoritative outcome.
///
/// Ricochet reasoning needs the first plate the shell struck, so this returns
/// `None` when the ray produced no hits at all.
pub fn compare_with_server(
    sim: &ShellSimResult,
    plates: &[ArcPlate],
    strike: Strike,
    server_outcome: &crate::armor::incoming::ServerOutcome,
    params: &ShellParams,
) -> Option<ComparisonVerdict> {
    // A ray that crossed nothing says nothing about a ricochet, which is read
    // off the first plate the shell met.
    if plates.is_empty() {
        return None;
    }
    let strike_angle = strike.angle;
    let overmatched = is_overmatch(params.caliber, strike.thickness);
    let ricochet_start = params.ricochet_angle.to_degrees();
    let always_ricochet = params.always_ricochet_angle.to_degrees();

    let mismatch = |sim_verdict| ComparisonVerdict::Mismatch { sim: sim_verdict, server: server_outcome.clone() };

    if *server_outcome == crate::armor::incoming::ServerOutcome::Ricochet {
        if overmatched {
            return Some(mismatch(SimVerdict::OvermatchRulesOutRicochet));
        }
        if strike_angle >= always_ricochet {
            return Some(ComparisonVerdict::Match);
        }
        if strike_angle >= ricochet_start {
            return Some(ComparisonVerdict::RicochetRngDefer { angle: strike_angle, ricochet_start, always_ricochet });
        }
        return Some(mismatch(SimVerdict::RicochetAngleTooLow));
    }

    // Server didn't ricochet. Check if we think it should have.
    if !overmatched && strike_angle >= always_ricochet {
        return Some(mismatch(SimVerdict::AlwaysRicochetBand));
    }

    let outcome = sim_outcome(sim, plates);
    let agrees = match server_outcome {
        crate::armor::incoming::ServerOutcome::Penetration => outcome == SimOutcome::Penetration,
        crate::armor::incoming::ServerOutcome::Citadel => outcome == SimOutcome::Citadel,
        crate::armor::incoming::ServerOutcome::Shatter => outcome == SimOutcome::Shatter,
        crate::armor::incoming::ServerOutcome::Overpenetration => outcome == SimOutcome::Overpenetration,
        // The simulation models neither of these, so there is nothing to agree
        // about. Refused rather than reported as agreement: a reader counting
        // agreements to decide whether to trust these figures would be counting
        // shells nothing was checked on.
        crate::armor::incoming::ServerOutcome::Underwater | crate::armor::incoming::ServerOutcome::Unknown(_) => {
            return None;
        }
        crate::armor::incoming::ServerOutcome::Ricochet => false,
    };

    Some(if agrees { ComparisonVerdict::Match } else { mismatch(SimVerdict::Outcome(outcome)) })
}

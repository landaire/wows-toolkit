//! The game art a table and its rows' detail draw.

use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::model::PlayerRow;
use wowsunpack::game_params::types::Species;

use crate::game_art;
use crate::game_art::Art;
use crate::game_art::ArtKey;
use crate::game_art::ClassTone;
use crate::game_art::GameArt;

/// Every picture `row` and its detail draw.
pub fn row_keys(row: &PlayerRow) -> impl Iterator<Item = ArtKey> + '_ {
    let build = row.translated_build.as_ref();
    let modules = build
        .into_iter()
        .flat_map(|build| build.modernization_slots.iter().flatten())
        .map(|slot| ArtKey::Modernization(slot.game_params_name.clone()));
    let signals = build
        .into_iter()
        .flat_map(|build| &build.signals)
        .map(|signal| ArtKey::SignalFlag(signal.game_params_name.clone()));
    let skills = build
        .into_iter()
        .flat_map(|build| build.captain_skills.iter().flatten().flat_map(|tier| &tier.skills))
        .map(|skill| ArtKey::CrewSkill(skill.internal_name.clone()));

    [ArtKey::ShipClass(row.ship_class)]
        .into_iter()
        .chain(row.ship_nation.clone().map(ArtKey::NationFlag))
        .chain(row.achievements.iter().map(|achievement| ArtKey::Achievement(achievement.icon_key.clone())))
        .chain(row.ribbons.iter().map(|ribbon| match ribbon.is_subribbon {
            true => ArtKey::SubRibbon(ribbon.icon_key.clone()),
            false => ArtKey::Ribbon(ribbon.icon_key.clone()),
        }))
        .chain(row.consumables.iter().map(|consumable| ArtKey::Consumable(consumable.icon_key.clone())))
        .chain(modules)
        .chain(signals)
        .chain(skills)
}

/// `species`' icon tinted for `kind`, as SVG markup. Reactive.
pub(super) fn class_icon(art: GameArt, species: Species, kind: PlayerColorKind) -> Option<String> {
    match art.get(&ArtKey::ShipClass(species))? {
        Art::Vector(svg) => Some(game_art::tinted_svg(&svg, ClassTone::from(kind).rgb())),
        Art::Raster(_) => None,
    }
}

/// The `src` of `nation`'s flag. Reactive.
pub(super) fn flag(art: GameArt, nation: &str) -> Option<String> {
    art.get(&ArtKey::NationFlag(nation.to_owned()))?.url()
}

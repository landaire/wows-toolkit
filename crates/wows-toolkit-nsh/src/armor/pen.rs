//! The penetration checker: the ships being compared, the one a trajectory
//! cast fires, and what each of their shells does to the plate under the
//! pointer.

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use rust_i18n::t;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wows_toolkit_viewmodel::armor::penetration::Ifhe;
use wows_toolkit_viewmodel::armor::penetration::PenResult;
use wows_toolkit_viewmodel::armor::penetration::check_penetration;
use wows_toolkit_viewmodel::armor::penetration::he_penetration;
use wows_toolkit_viewmodel::armor::penetration::sap_penetration;
use wows_toolkit_viewmodel::ship_catalog::ShipEntry;
use wows_toolkit_viewmodel::ship_catalog::tier_roman;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;

use crate::js_newtype::js_newtype;

use super::catalog::fold_query;
use super::catalog::ship_matches;

/// A shell's position in its ship's shell list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellIndex(u32);

js_newtype!(ShellIndex, u32);

impl ShellIndex {
    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// A ship's position in the list of ships being compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenShipIndex(u32);

js_newtype!(PenShipIndex, u32);

impl PenShipIndex {
    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// What one shell does to the plate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Verdict {
    Penetrates,
    Shatters,
    AngleDependent,
    NoData,
    /// No plate has been under the pointer yet.
    #[default]
    AwaitingPlate,
}

/// One shell row.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PenShellRow {
    pub index: ShellIndex,
    /// `"AP 460mm"`.
    pub label: String,
    /// `"AP 460 mm"`, as the trajectory shell picker reads.
    pub picker_label: String,
    /// The flat penetration (`"25mm"`), or `"-"` for AP and a shell without one.
    pub pen: String,
    pub verdict: Verdict,
    pub verdict_text: String,
    /// The shell a trajectory cast fires.
    pub firing: bool,
}

/// One ship being compared.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PenShipRow {
    pub param_index: String,
    /// `"X Yamato"`.
    pub label: String,
    pub firing: bool,
    pub shells: Vec<PenShellRow>,
    /// Said when the ship carries no main battery shells.
    pub no_shells: Option<String>,
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PenResultRow {
    pub param_index: String,
    pub label: String,
}

/// The plate the verdicts are read against.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct PenPlate {
    pub mm: f32,
    /// The zone, translated.
    pub zone: String,
}

/// The checker as its panel and the trajectory panel show it.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct PenView {
    pub ifhe: bool,
    pub search: String,
    pub results: Vec<PenResultRow>,
    pub plate: Option<PenPlate>,
    pub ships: Vec<PenShipRow>,
    pub firing_ship: Option<String>,
    pub firing_shell: Option<ShellIndex>,
}

/// The checker's own state, shared by every pane.
#[derive(Debug, Default)]
pub(super) struct PenState {
    /// In the order they were added.
    pub ships: Vec<ComparisonShip>,
    pub firing_ship: Option<String>,
    /// The firing ship's shell, when one was chosen.
    pub firing_shell: Option<ShellIndex>,
    pub ifhe: bool,
    pub search: String,
}

impl PenState {
    pub fn holds(&self, param_index: &str) -> bool {
        self.ships.iter().any(|ship| ship.param_index == param_index)
    }

    /// Adds a ship, clearing the search: the ship asked for is now listed
    /// below it. The first ship becomes the firing ship.
    pub fn add(&mut self, ship: ComparisonShip) {
        if self.holds(&ship.param_index) {
            return;
        }
        if self.ships.is_empty() {
            self.firing_ship = Some(ship.param_index.clone());
            self.firing_shell = None;
        }
        self.ships.push(ship);
        self.search.clear();
    }

    pub fn remove(&mut self, index: PenShipIndex) {
        let index = index.get();
        if index >= self.ships.len() {
            return;
        }
        self.ships.remove(index);
        if !self.firing_ship.as_deref().is_some_and(|firing| self.holds(firing)) {
            self.firing_ship = None;
            self.firing_shell = None;
        }
    }

    pub fn clear(&mut self) {
        self.ships.clear();
        self.firing_ship = None;
        self.firing_shell = None;
    }

    pub fn set_firing_ship(&mut self, param_index: &str) {
        if !self.holds(param_index) {
            return;
        }
        self.firing_ship = Some(param_index.to_owned());
        self.firing_shell = None;
    }

    pub fn set_firing_shell(&mut self, index: ShellIndex) {
        let Some(ship) = self.firing() else { return };
        if index.get() >= ship.shells.len() {
            return;
        }
        self.firing_shell = Some(index);
    }

    /// The shell a cast from the firing ship fires: the chosen one, else its
    /// AP shell, else its first.
    pub fn active_shell(&self) -> Option<ShellIndex> {
        let ship = self.firing()?;
        let index = self
            .firing_shell
            .map(ShellIndex::get)
            .filter(|index| *index < ship.shells.len())
            .or_else(|| ship.shells.iter().position(|shell| shell.ammo_type == AmmoType::AP))
            .or_else(|| (!ship.shells.is_empty()).then_some(0))?;

        Some(ShellIndex(index as u32))
    }

    fn firing(&self) -> Option<&ComparisonShip> {
        let firing = self.firing_ship.as_deref()?;
        self.ships.iter().find(|ship| ship.param_index == firing)
    }

    /// The ship a cast fires: the chosen one, else the first. Records it as
    /// chosen, as the trajectory panel shows it.
    pub fn cast_ship(&mut self) -> (Option<ComparisonShip>, Option<ShellIndex>) {
        let ship = self.firing().or_else(|| self.ships.first()).cloned();
        self.firing_ship = ship.as_ref().map(|ship| ship.param_index.clone());

        (ship, self.active_shell())
    }

    /// The ships a search offers, skipping those already compared.
    pub fn results<'a>(&self, catalog: impl Iterator<Item = &'a ShipEntry>) -> Vec<PenResultRow> {
        const SEARCH_RESULTS: usize = 10;

        let Some(needle) = fold_query(&self.search) else { return Vec::new() };

        catalog
            .filter(|ship| ship_matches(ship, &needle))
            .filter(|ship| !self.holds(&ship.param_index))
            .take(SEARCH_RESULTS)
            .map(|ship| PenResultRow {
                param_index: ship.param_index.clone(),
                label: format!("{} {}", tier_roman(ship.tier), ship.display_name),
            })
            .collect()
    }

    /// The ship blocks, read against `thickness`.
    pub fn ship_rows(&self, thickness: Option<Millimeters>) -> Vec<PenShipRow> {
        let ifhe = Ifhe::from_enabled(self.ifhe);
        let active_shell = self.active_shell();
        self.ships
            .iter()
            .map(|ship| {
                let firing = self.firing_ship.as_deref() == Some(ship.param_index.as_str());
                let shells = ship
                    .shells
                    .iter()
                    .enumerate()
                    .map(|(index, shell)| {
                        let index = ShellIndex(index as u32);
                        let verdict = verdict(shell, thickness, ifhe);
                        PenShellRow {
                            index,
                            label: shell_label(shell),
                            picker_label: format!("{} {:.0} mm", shell.ammo_type.display_name(), shell.caliber.value()),
                            pen: pen_label(shell, ifhe),
                            verdict,
                            verdict_text: verdict_text(verdict),
                            firing: firing && active_shell == Some(index),
                        }
                    })
                    .collect::<Vec<_>>();
                PenShipRow {
                    param_index: ship.param_index.clone(),
                    label: format!("{} {}", tier_roman(ship.tier), ship.display_name),
                    firing,
                    no_shells: shells
                        .is_empty()
                        .then(|| t!("ui.armor.pen.no_shells", ship = ship.display_name.clone()).into_owned()),
                    shells,
                }
            })
            .collect()
    }
}

fn verdict(shell: &ShellInfo, thickness: Option<Millimeters>, ifhe: Ifhe) -> Verdict {
    let Some(thickness) = thickness else { return Verdict::AwaitingPlate };
    match check_penetration(shell, thickness, ifhe) {
        Some(PenResult::Penetrates) => Verdict::Penetrates,
        Some(PenResult::Bounces) => Verdict::Shatters,
        Some(PenResult::AngleDependent) => Verdict::AngleDependent,
        None => Verdict::NoData,
    }
}

fn verdict_text(verdict: Verdict) -> String {
    let key = match verdict {
        Verdict::Penetrates => "ui.armor.pen.penetrates",
        Verdict::Shatters => "ui.armor.pen.shatters",
        Verdict::AngleDependent => "ui.armor.pen.angle_dependent",
        Verdict::NoData => "ui.armor.pen.no_data",
        Verdict::AwaitingPlate => "ui.armor.pen.awaiting_plate",
    };
    t!(key).into_owned()
}

/// The ammo type and calibre, which tells two of a ship's shells apart.
pub(super) fn shell_label(shell: &ShellInfo) -> String {
    let ammo = match &shell.ammo_type {
        AmmoType::AP => "AP",
        AmmoType::HE => "HE",
        AmmoType::SAP => "SAP",
        AmmoType::Unknown(_) => "?",
    };
    format!("{ammo} {:.0}mm", shell.caliber.value())
}

/// A flat figure for HE and SAP; AP is measured by overmatch instead.
fn pen_label(shell: &ShellInfo, ifhe: Ifhe) -> String {
    let flat = match &shell.ammo_type {
        AmmoType::HE => he_penetration(shell, ifhe),
        AmmoType::SAP => sap_penetration(shell),
        AmmoType::AP | AmmoType::Unknown(_) => None,
    };
    match flat {
        Some(pen) => format!("{:.0}mm", pen.value()),
        None => "-".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use wowsunpack::game_params::types::Species;

    use super::*;

    fn shell(ammo_type: AmmoType, caliber: f32, he_pen_mm: Option<f32>) -> ShellInfo {
        ShellInfo {
            name: "PTEST".to_owned(),
            ammo_type,
            caliber: Millimeters::from(caliber),
            he_pen_mm,
            sap_pen_mm: None,
            alpha_damage: 0.0,
            muzzle_velocity: 0.0,
            mass_kg: 0.0,
            krupp: 0.0,
            ricochet_angle: 0.0,
            always_ricochet_angle: 0.0,
            fuse_time: 0.0,
            fuse_threshold: None,
            burn_prob: 0.0,
            air_drag: 0.0,
            normalization: None,
            cap: true,
        }
    }

    fn ship(param_index: &str, name: &str) -> ComparisonShip {
        ComparisonShip {
            param_index: param_index.to_owned(),
            display_name: name.to_owned(),
            tier: 10,
            nation: "japan".to_owned(),
            species: Species::Battleship,
            shells: vec![shell(AmmoType::AP, 460.0, None), shell(AmmoType::HE, 460.0, Some(76.0))],
        }
    }

    fn entry(param_index: &str, display_name: &str) -> ShipEntry {
        ShipEntry {
            param_index: param_index.to_owned(),
            display_name: display_name.to_owned(),
            search_name: unidecode::unidecode(display_name).to_lowercase(),
            tier: 10,
        }
    }

    #[test]
    fn the_first_ship_added_fires_and_the_search_clears() {
        let mut pen = PenState { search: "yam".to_owned(), ..PenState::default() };
        pen.add(ship("PJSB018", "Yamato"));
        pen.add(ship("PASB018", "Montana"));

        assert_eq!(pen.firing_ship.as_deref(), Some("PJSB018"));
        assert!(pen.search.is_empty());
        assert_eq!(pen.ships.len(), 2);
    }

    #[test]
    fn removing_the_firing_ship_lets_the_first_fire() {
        let mut pen = PenState::default();
        pen.add(ship("PJSB018", "Yamato"));
        pen.add(ship("PASB018", "Montana"));
        pen.set_firing_shell(ShellIndex(1));
        pen.remove(PenShipIndex(0));

        assert_eq!(pen.firing_ship, None);
        assert_eq!(pen.firing_shell, None);
        let (cast, shell) = pen.cast_ship();
        assert_eq!(cast.map(|ship| ship.param_index), Some("PASB018".to_owned()));
        assert_eq!(shell, Some(ShellIndex(0)), "the AP shell fires until another is chosen");
        assert_eq!(pen.firing_ship.as_deref(), Some("PASB018"));
    }

    #[test]
    fn a_chosen_shell_fires_else_the_ap_shell_else_the_first() {
        let mut pen = PenState::default();
        let mut he_only = ship("PJSB018", "Yamato");
        he_only.shells.reverse();
        pen.add(he_only);
        assert_eq!(pen.active_shell(), Some(ShellIndex(1)), "the AP shell is found where it sits");

        pen.set_firing_shell(ShellIndex(0));
        assert_eq!(pen.active_shell(), Some(ShellIndex(0)));

        pen.ships[0].shells.retain(|shell| shell.ammo_type != AmmoType::AP);
        pen.firing_shell = None;
        assert_eq!(pen.active_shell(), Some(ShellIndex(0)));

        pen.ships[0].shells.clear();
        assert_eq!(pen.active_shell(), None);
    }

    #[test]
    fn the_search_folds_accents_and_skips_ships_already_compared() {
        let mut pen = PenState::default();
        pen.add(ship("PWSD110", "Sm\u{e5}land"));
        pen.search = "SMA".to_owned();
        let catalog =
            [entry("PWSD110", "Sm\u{e5}land"), entry("PWSD111", "Sm\u{e5}land B"), entry("PJSB018", "Yamato")];

        let results = pen.results(catalog.iter());

        assert_eq!(results, [PenResultRow { param_index: "PWSD111".to_owned(), label: "X Sm\u{e5}land B".to_owned() }]);
    }

    #[test]
    fn verdicts_wait_for_a_plate() {
        let mut pen = PenState::default();
        pen.add(ship("PJSB018", "Yamato"));

        let waiting = pen.ship_rows(None);
        let thin = pen.ship_rows(Some(Millimeters::from(25.0)));

        assert!(waiting[0].shells.iter().all(|row| row.verdict == Verdict::AwaitingPlate));
        assert_eq!(thin[0].shells[0].verdict, Verdict::Penetrates);
        assert_eq!(thin[0].shells[1].verdict, Verdict::Penetrates);
        assert_eq!(thin[0].shells[0].label, "AP 460mm");
        assert_eq!(thin[0].shells[0].pen, "-");
        assert_eq!(thin[0].shells[1].pen, "76mm");
    }
}

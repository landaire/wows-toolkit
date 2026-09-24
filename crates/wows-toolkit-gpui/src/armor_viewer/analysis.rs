//! Penetration checker: what a chosen ship's shells do to a plate of a given
//! thickness.
//!
//! The egui app asks the same question from its Analysis window's Penetration
//! tab (`armor_viewer/ui/analysis.rs`), against the plate under the pointer.
//! Here the thickness is a field, seeded from the last plate the pointer was
//! over, because the popover holds the pointer while it is open.
//!
//! The verdicts themselves come from
//! `wows_toolkit_viewmodel::armor::penetration`, which both front ends read.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wows_toolkit_viewmodel::armor::penetration::Ifhe;
use wows_toolkit_viewmodel::armor::penetration::PenResult;
use wows_toolkit_viewmodel::armor::penetration::check_penetration;
use wows_toolkit_viewmodel::armor::penetration::he_penetration;
use wows_toolkit_viewmodel::armor::penetration::sap_penetration;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;

use super::catalog::ShipCatalog;
use super::catalog::tier_roman;
use super::pane::ArmorViewerPane;

/// The panel's own width, and the range it can be dragged over. Wide enough
/// for a shell name beside its verdict.
pub const PANEL_WIDTH: Pixels = px(320.);
pub const PANEL_MIN_WIDTH: Pixels = px(260.);
pub const PANEL_MAX_WIDTH: Pixels = px(480.);

/// How far the search results run before they scroll, so they never push the
/// ships being compared off the panel.
const RESULTS_MAX_HEIGHT: Pixels = px(150.);

const SHELL_COLUMN_WIDTH: Pixels = px(120.);
const PEN_COLUMN_WIDTH: Pixels = px(64.);

/// One ship in the attacker combo.
#[derive(Clone)]
pub struct ShipItem {
    label: SharedString,
    param_index: SharedString,
}

impl SearchableListItem for ShipItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.param_index
    }
}

/// How many search results the list offers at once, as the egui panel does.
/// Enough to find a ship by a few letters, few enough not to become the panel.
const SEARCH_RESULTS: usize = 10;

/// One ship the reader can search for.
#[derive(Clone)]
pub struct CatalogEntry {
    pub param_index: String,
    pub display_name: String,
    /// The name folded for searching: accents removed, lowercased, as the
    /// egui selector folds it.
    pub search_name: String,
    pub tier: u32,
}

/// Everything the checker keeps between renders. Owned by `ArmorViewerPane`.
pub struct PenetrationState {
    /// The attackers being compared, in the order they were added.
    ///
    /// A list rather than one ship: the question the checker answers is which
    /// of several ships gets through a plate, which one at a time cannot ask.
    pub ships: Vec<ComparisonShip>,
    /// Bumped whenever the list changes, so anything caching a result against
    /// it can tell that it is stale.
    pub version: u64,
    pub ifhe: bool,
    /// The plate the pointer was last over: its zone and how thick it is.
    /// `None` until the pointer has been on the hull.
    pub plate: Option<PlateUnderPointer>,
    /// What has been typed into the search field.
    pub search: Entity<InputState>,
    /// Every ship that can be searched, flattened from the catalog once.
    pub catalog: Vec<CatalogEntry>,
}

/// The plate the pointer is over, as the checker names it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateUnderPointer {
    pub zone: SharedString,
    pub thickness: Millimeters,
}

impl PenetrationState {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.armor.pen.search_hint").to_string()));
        Self { ships: Vec::new(), version: 0, ifhe: false, plate: None, search, catalog: Vec::new() }
    }

    /// Flattens the catalog into the list the search runs over.
    pub fn set_catalog(&mut self, catalog: &ShipCatalog) {
        self.catalog = catalog
            .nations
            .iter()
            .flat_map(|nation| nation.classes.iter())
            .flat_map(|class| class.ships.iter())
            .map(|ship| CatalogEntry {
                param_index: ship.param_index.clone(),
                display_name: ship.display_name.clone(),
                search_name: ship.search_name.clone(),
                tier: ship.tier,
            })
            .collect();
    }

    /// The ships a search for `needle` offers, skipping those already added.
    ///
    /// Folded the same way the catalog folded its names, so a search without
    /// accents still finds an accented ship.
    pub fn matches(&self, needle: &str) -> Vec<&CatalogEntry> {
        let needle = unidecode::unidecode(needle.trim()).to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.catalog
            .iter()
            .filter(|entry| entry.search_name.contains(&needle))
            .filter(|entry| !self.ships.iter().any(|ship| ship.param_index == entry.param_index))
            .take(SEARCH_RESULTS)
            .collect()
    }

    /// Whether `param_index` is already being compared.
    pub fn holds(&self, param_index: &str) -> bool {
        self.ships.iter().any(|ship| ship.param_index == param_index)
    }

    pub fn add(&mut self, ship: ComparisonShip) {
        if self.holds(&ship.param_index) {
            return;
        }
        self.ships.push(ship);
        self.version = self.version.wrapping_add(1);
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.ships.len() {
            self.ships.remove(index);
            self.version = self.version.wrapping_add(1);
        }
    }

    pub fn clear(&mut self) {
        if self.ships.is_empty() {
            return;
        }
        self.ships.clear();
        self.version = self.version.wrapping_add(1);
    }
}

/// The checker's panel: the skill, the search, and one block per ship being
/// compared.
///
/// A panel rather than a popover. The verdicts are read against the plate the
/// pointer is on, so the surface carrying them has to stay up while the
/// pointer is on the hull; a popover both closes on the first click and eats
/// the pointer moves that would update the plate.
/// `view` is the pane itself rather than its entity: this is called from
/// inside that pane's own render, where reading the entity would panic. The
/// entity is captured only for the callbacks, which run later.
pub fn render_panel(view: &ArmorViewerPane, pane: &Entity<ArmorViewerPane>, cx: &mut App) -> AnyElement {
    let (ships, ifhe_on, plate, search, matches) = {
        let state = view.penetration();
        let typed = state.search.read(cx).value().to_string();
        let matches: Vec<(String, String, u32)> = state
            .matches(&typed)
            .into_iter()
            .map(|entry| (entry.param_index.clone(), entry.display_name.clone(), entry.tier))
            .collect();
        (state.ships.clone(), state.ifhe, state.plate.clone(), state.search.clone(), matches)
    };
    let ifhe = Ifhe::from_enabled(ifhe_on);

    let header = h_flex()
        .gap_2()
        .items_center()
        .child(div().flex_1().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.pen_check_label").to_string()))
        .child(
            Checkbox::new("armor-pen-ifhe")
                .label(t!("ui.armor.pen.ifhe").to_string())
                .checked(ifhe_on)
                .tooltip(t!("ui.armor.pen.ifhe_tooltip").to_string())
                .on_click({
                    let pane = pane.clone();
                    move |checked: &bool, _window, cx: &mut App| {
                        let checked = *checked;
                        pane.update(cx, |pane, cx| pane.set_pen_ifhe(checked, cx));
                    }
                }),
        );

    // What the verdicts below are being read against. Live: it follows the
    // pointer over the hull rather than being typed in.
    let against: AnyElement = match &plate {
        Some(plate) => h_flex()
            .gap_1()
            .items_center()
            .child(div().text_xs().font_weight(FontWeight::BOLD).child(format!("{:.0} mm", plate.thickness.value())))
            .child(div().text_xs().text_color(crate::theme::text_dim()).child(plate.zone.to_string()))
            .into_any_element(),
        None => hint(t!("ui.armor.pen.hover_a_plate").as_ref(), cx),
    };

    let results: Option<AnyElement> = (!matches.is_empty()).then(|| {
        v_flex()
            .id("armor-pen-results")
            .gap_0()
            .max_h(RESULTS_MAX_HEIGHT)
            .overflow_y_scroll()
            .children(matches.into_iter().enumerate().map(|(ix, (param_index, name, tier))| {
                let pane = pane.clone();
                Button::new(("armor-pen-add", ix))
                    .label(format!("{} {}", tier_roman(tier), name))
                    .compact()
                    .w_full()
                    .on_click(move |_event, window, cx: &mut App| {
                        let param_index = param_index.clone();
                        pane.update(cx, |pane, cx| pane.add_comparison_ship(&param_index, window, cx));
                    })
                    .into_any_element()
            }))
            .into_any_element()
    });

    let body: AnyElement = if ships.is_empty() {
        hint(t!("ui.armor.pen.add_a_ship").as_ref(), cx)
    } else {
        v_flex()
            .gap_2()
            .children(
                ships.iter().enumerate().map(|(index, ship)| ship_block(pane, index, ship, plate.as_ref(), ifhe, cx)),
            )
            .into_any_element()
    };

    v_flex()
        .size_full()
        .gap_2()
        .p_2()
        .child(header)
        .child(against)
        .child(Input::new(&search).id("armor-pen-search").small().w_full())
        .children(results)
        .child(div().id("armor-pen-ships").flex_1().min_h(px(0.)).overflow_y_scroll().child(body))
        .when(!ships.is_empty(), |this| {
            let pane = pane.clone();
            this.child(
                Button::new("armor-pen-clear").label(t!("ui.armor.pen.clear_all").to_string()).compact().on_click(
                    move |_event, _window, cx: &mut App| {
                        pane.update(cx, |pane, cx| pane.clear_comparison_ships(cx));
                    },
                ),
            )
        })
        .into_any_element()
}

/// One ship being compared: its name, a way to drop it, and its shells
/// against whatever the pointer is on.
fn ship_block(
    pane: &Entity<ArmorViewerPane>,
    index: usize,
    ship: &ComparisonShip,
    plate: Option<&PlateUnderPointer>,
    ifhe: Ifhe,
    cx: &App,
) -> AnyElement {
    let pane = pane.clone();
    let rows: AnyElement = if ship.shells.is_empty() {
        hint(t!("ui.armor.pen.no_shells", ship = ship.display_name.clone()).as_ref(), cx)
    } else {
        v_flex()
            .gap_0()
            .children(ship.shells.iter().enumerate().map(|(ix, shell)| {
                shell_row(ix, shell, plate.map(|plate| plate.thickness), ifhe, cx).into_any_element()
            }))
            .into_any_element()
    };

    v_flex()
        .gap_0()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(div().flex_1().text_xs().font_weight(FontWeight::BOLD).truncate().child(format!(
                    "{} {}",
                    tier_roman(ship.tier),
                    ship.display_name
                )))
                .child(
                    Button::new(("armor-pen-remove", index))
                        .child(crate::icons::icon(crate::icons::X))
                        .compact()
                        .tooltip(t!("ui.armor.pen.remove").to_string())
                        .on_click(move |_event, _window, cx: &mut App| {
                            pane.update(cx, |pane, cx| pane.remove_comparison_ship(index, cx));
                        }),
                ),
        )
        .child(rows)
        .into_any_element()
}

/// The thickness the field holds, or `None` while it is empty.
fn thickness_mm(input: &Entity<InputState>, cx: &App) -> Option<Millimeters> {
    let text = input.read(cx).value();
    let value: f32 = text.trim().parse().ok()?;
    (value > 0.0).then(|| Millimeters::from(value))
}

fn hint(text: &str, cx: &App) -> AnyElement {
    let _ = cx;
    div().py_1().text_xs().text_color(crate::theme::text_dim()).child(text.to_string()).into_any_element()
}

/// One shell against the plate: what it is, what it brings, and what happens.
fn shell_row(ix: usize, shell: &ShellInfo, thickness: Option<Millimeters>, ifhe: Ifhe, cx: &App) -> impl IntoElement {
    let semantic = crate::theme::semantic();
    // No plate under the pointer is no question yet, which reads differently
    // from a shell the data carries no figure for.
    let (verdict, color) = match thickness.map(|thickness| check_penetration(shell, thickness, ifhe)) {
        None => (t!("ui.armor.pen.awaiting_plate"), rgb(semantic.text_dim)),
        Some(Some(PenResult::Penetrates)) => (t!("ui.armor.pen.penetrates"), rgb(semantic.ok)),
        Some(Some(PenResult::Bounces)) => (t!("ui.armor.pen.shatters"), rgb(semantic.error)),
        Some(Some(PenResult::AngleDependent)) => (t!("ui.armor.pen.angle_dependent"), rgb(semantic.warn)),
        Some(None) => (t!("ui.armor.pen.no_data"), rgb(semantic.text_dim)),
    };

    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .px_1()
        .py(px(1.))
        .when_some(crate::ui::stripe(ix, cx), |row, background| row.bg(background))
        .child(div().w(SHELL_COLUMN_WIDTH).text_xs().truncate().child(shell_label(shell)))
        .child(div().w(PEN_COLUMN_WIDTH).text_xs().text_color(crate::theme::text_dim()).child(pen_label(shell, ifhe)))
        .child(div().flex_1().text_xs().text_color(color).child(verdict.to_string()))
}

/// The ammo type and calibre, which is what tells two of a ship's shells
/// apart on screen.
fn shell_label(shell: &ShellInfo) -> String {
    let ammo = match &shell.ammo_type {
        AmmoType::AP => "AP",
        AmmoType::HE => "HE",
        AmmoType::SAP => "SAP",
        AmmoType::Unknown(_) => "?",
    };
    format!("{ammo} {:.0}mm", shell.caliber.value())
}

/// What the shell brings to the plate: a flat figure for HE and SAP, and the
/// calibre for AP, which is what overmatch is measured from.
fn pen_label(shell: &ShellInfo, ifhe: Ifhe) -> String {
    let flat = match &shell.ammo_type {
        AmmoType::HE => he_penetration(shell, ifhe),
        AmmoType::SAP => sap_penetration(shell),
        AmmoType::AP | AmmoType::Unknown(_) => None,
    };
    match flat {
        Some(pen) => format!("{:.0}mm", pen.value()),
        None => "-".to_string(),
    }
}

// The test module imports by name: `use super::*` would pull in the glob
// `gpui_kit::*` re-export of GPUI's own `test` macro, which shadows Rust's
// `#[test]`.
#[cfg(test)]
mod tests {
    use super::pen_label;
    use super::shell_label;
    use wows_toolkit_viewmodel::armor::penetration::Ifhe;
    use wowsunpack::game_params::types::AmmoType;
    use wowsunpack::game_params::types::Millimeters;
    use wowsunpack::game_params::types::ShellInfo;

    fn shell(ammo_type: AmmoType, caliber: f32, he_pen_mm: Option<f32>, sap_pen_mm: Option<f32>) -> ShellInfo {
        ShellInfo {
            name: "PTEST".to_string(),
            ammo_type,
            caliber: Millimeters::from(caliber),
            he_pen_mm,
            sap_pen_mm,
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

    #[test]
    fn a_row_names_the_ammo_type_and_calibre() {
        assert_eq!(shell_label(&shell(AmmoType::AP, 406.0, None, None)), "AP 406mm");
        assert_eq!(shell_label(&shell(AmmoType::HE, 152.0, Some(25.0), None)), "HE 152mm");
        assert_eq!(shell_label(&shell(AmmoType::SAP, 203.0, None, Some(55.0))), "SAP 203mm");
    }

    #[test]
    fn only_a_flat_penetrator_carries_a_figure() {
        let he = shell(AmmoType::HE, 152.0, Some(25.0), None);
        assert_eq!(pen_label(&he, Ifhe::NotApplied), "25mm");
        assert_eq!(pen_label(&he, Ifhe::Applied), "31mm");

        // AP is measured by overmatch, not by a figure of its own.
        assert_eq!(pen_label(&shell(AmmoType::AP, 406.0, None, None), Ifhe::NotApplied), "-");
        // A shell the data has no figure for reads as absent, not as zero.
        assert_eq!(pen_label(&shell(AmmoType::HE, 152.0, None, None), Ifhe::Applied), "-");
    }
}

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
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
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

/// The popover's box. Wide enough for a shell name and its verdict.
pub const PANEL_MIN_WIDTH: Pixels = px(300.);
pub const PANEL_MAX_WIDTH: Pixels = px(380.);

const SHELL_COLUMN_WIDTH: Pixels = px(120.);
const THICKNESS_FIELD_WIDTH: Pixels = px(70.);
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

/// Everything the checker keeps between renders. Owned by `ArmorViewerPane`.
pub struct PenetrationState {
    /// The attacker, resolved once when it is chosen rather than per render.
    pub ship: Option<ComparisonShip>,
    pub ifhe: bool,
    /// The plate the pointer was last over, which seeds the thickness field
    /// and names what the verdicts are against.
    pub plate: Option<SharedString>,
    pub thickness: Entity<InputState>,
    pub ship_select: Entity<SelectState<SearchableVec<ShipItem>>>,
}

impl PenetrationState {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let thickness = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("32")
                .pattern(regex::Regex::new(r"^\d*$").expect("a digits-only pattern is a valid regex"))
        });
        let ship_select = cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        Self { ship: None, ifhe: false, plate: None, thickness, ship_select }
    }

    /// Fills the attacker combo from the catalog, once it has loaded.
    pub fn set_catalog(&self, catalog: &ShipCatalog, window: &mut Window, cx: &mut App) {
        let items: Vec<ShipItem> = catalog
            .nations
            .iter()
            .flat_map(|nation| nation.classes.iter())
            .flat_map(|class| class.ships.iter())
            .map(|ship| ShipItem {
                label: SharedString::from(format!("{} {}", tier_roman(ship.tier), ship.display_name)),
                param_index: SharedString::from(ship.param_index.clone()),
            })
            .collect();
        self.ship_select.update(cx, |state, cx| state.set_items(SearchableVec::new(items), window, cx));
    }
}

/// The popover's content: the attacker, the skill, the plate, and one row per
/// shell.
pub fn render_panel(pane: &Entity<ArmorViewerPane>, cx: &mut App) -> AnyElement {
    // Everything the panel draws is read out first: the builders below take
    // `cx` mutably, so nothing may still be borrowed from the pane.
    let (ship, ifhe_on, plate, ship_select, thickness_input) = {
        let state = pane.read(cx).penetration();
        (state.ship.clone(), state.ifhe, state.plate.clone(), state.ship_select.clone(), state.thickness.clone())
    };
    let thickness = thickness_mm(&thickness_input, cx);
    let ifhe = Ifhe::from_enabled(ifhe_on);

    let rows: AnyElement = match (&ship, thickness) {
        (None, _) => hint(t!("ui.armor.pen.pick_ship").as_ref(), cx),
        (Some(_), None) => hint(t!("ui.armor.pen.enter_thickness").as_ref(), cx),
        (Some(ship), Some(_)) if ship.shells.is_empty() => {
            hint(t!("ui.armor.pen.no_shells", ship = ship.display_name).as_ref(), cx)
        }
        (Some(ship), Some(thickness_mm)) => v_flex()
            .gap_0()
            .children(
                ship.shells
                    .iter()
                    .enumerate()
                    .map(|(ix, shell)| shell_row(ix, shell, thickness_mm, ifhe, cx).into_any_element()),
            )
            .into_any_element(),
    };

    v_flex()
        .min_w(PANEL_MIN_WIDTH)
        .max_w(PANEL_MAX_WIDTH)
        .gap_1()
        .p_1()
        .child(
            Select::new(&ship_select)
                .id("armor-pen-ship")
                .accessibility_label(t!("ui.armor.pen.ship").to_string())
                .small()
                .w_full(),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.armor.pen.plate").to_string()))
                .child(div().w(THICKNESS_FIELD_WIDTH).child(NumberInput::new(&thickness_input).small()))
                .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.armor.pen.mm").to_string()))
                .child(div().flex_1())
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
                ),
        )
        .when_some(plate, |this, plate| {
            this.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.armor.pen.last_hovered", plate = plate).to_string()),
            )
        })
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
fn shell_row(ix: usize, shell: &ShellInfo, thickness: Millimeters, ifhe: Ifhe, cx: &App) -> impl IntoElement {
    let semantic = crate::theme::semantic();
    let (verdict, color) = match check_penetration(shell, thickness, ifhe) {
        Some(PenResult::Penetrates) => (t!("ui.armor.pen.penetrates"), rgb(semantic.ok)),
        Some(PenResult::Bounces) => (t!("ui.armor.pen.shatters"), rgb(semantic.error)),
        Some(PenResult::AngleDependent) => (t!("ui.armor.pen.angle_dependent"), rgb(semantic.warn)),
        None => (t!("ui.armor.pen.no_data"), rgb(semantic.text_dim)),
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

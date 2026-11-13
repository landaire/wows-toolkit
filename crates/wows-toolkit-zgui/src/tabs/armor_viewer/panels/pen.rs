//! The penetration checker: what the shells of the ships being compared do to the plate under the
//! pointer.
//!
//! It is a section of the options rail, beside the hull rather than over it, so it stays up while
//! the pointer sweeps the plates it is read against. The verdicts come from
//! `wows_toolkit_viewmodel::armor::penetration`, which every front end reads.

use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::EscapeClears;
use landaire_ui::MarkedTextProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::GameClock;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::catalog::tier_roman;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wows_toolkit_viewmodel::armor::penetration::Ifhe;
use wows_toolkit_viewmodel::armor::penetration::PenResult;
use wows_toolkit_viewmodel::armor::penetration::check_penetration;
use wows_toolkit_viewmodel::armor::penetration::he_penetration;
use wows_toolkit_viewmodel::armor::penetration::resolve_ship_shells;
use wows_toolkit_viewmodel::armor::penetration::sap_penetration;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::armor_viewer::panels::incoming::IncomingFire;
use crate::tabs::armor_viewer::panels::incoming::IncomingLogProps;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::text_marks::marks_within;

const SHEET: &str = "wt-armor-pen";

pub(super) const CSS: &str = css!(
    ".wt-armor-pen { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
    .wt-armor-pen__header { display: flex; align-items: center; gap: 8px; }
    .wt-armor-pen__against { flex: 1; display: flex; align-items: center; gap: 4px; min-width: 0; }
    .wt-armor-pen__mm { font-size: 12px; font-weight: 700; }
    .wt-armor-pen__dim { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-pen__hint { padding: 4px 0; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-pen__check { display: flex; align-items: center; gap: 6px; font-size: 12px; }
    .wt-armor-pen__search { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-pen__results { display: flex; flex-direction: column; max-height: 150px; overflow-y: auto; }
    .wt-armor-pen__result { justify-content: flex-start; }
    .wt-armor-pen__ships-slot { display: flex; flex-direction: column; }
    .wt-armor-pen__ships { display: flex; flex-direction: column; gap: 8px; }
    .wt-armor-pen__ship { display: flex; flex-direction: column; }
    .wt-armor-pen__ship-head { display: flex; align-items: center; gap: 4px; }
    .wt-armor-pen__ship-name { flex: 1; min-width: 0; font-size: 12px; font-weight: 700; overflow: hidden;
        white-space: nowrap; text-overflow: ellipsis; }
    .wt-armor-pen__shell { display: flex; align-items: center; gap: 8px; width: 100%; padding: 1px 4px; }
    .wt-armor-pen__shell[data-stripe] { background-color: var(--lui-color-widget); }
    .wt-armor-pen__shell-name { flex: none; width: 120px; font-size: 12px; overflow: hidden; white-space: nowrap;
        text-overflow: ellipsis; }
    .wt-armor-pen__shell-pen { flex: none; width: 64px; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-pen__verdict { flex: 1; font-size: 12px; }
    .wt-armor-pen__verdict[data-tone=ok] { color: var(--wt-color-ok); }
    .wt-armor-pen__verdict[data-tone=error] { color: var(--wt-color-error); }
    .wt-armor-pen__verdict[data-tone=warn] { color: var(--wt-color-warn); }
    .wt-armor-pen__verdict[data-tone=dim] { color: var(--wt-color-text-dim); }"
);

/// How many search results the list offers at once, as the egui panel does: enough to find a ship
/// by a few letters, few enough not to become the panel.
const SEARCH_RESULTS: usize = 10;

/// One ship the reader can search for.
#[derive(Clone)]
pub struct CatalogEntry {
    pub param_index: String,
    pub display_name: String,
    /// The name folded once for the search.
    pub key: FilterKey,
    pub tier: u32,
}

impl CatalogEntry {
    /// The tier, then the name, as a result row reads.
    fn label(&self) -> String {
        format!("{} {}", tier_roman(self.tier), self.display_name)
    }
}

/// A catalog's ships as the search reads them, kept with the catalog they were read from.
struct FlattenedCatalog {
    catalog: Arc<ShipCatalog>,
    entries: Rc<Vec<CatalogEntry>>,
}

/// Every ship of `catalog`, flattened once for the search.
pub fn catalog_entries(catalog: &ShipCatalog) -> Vec<CatalogEntry> {
    catalog
        .nations
        .iter()
        .flat_map(|nation| nation.classes.iter())
        .flat_map(|class| class.ships.iter())
        .map(|ship| CatalogEntry {
            param_index: ship.param_index.clone(),
            display_name: ship.display_name.clone(),
            key: FilterKey::new(&ship.display_name),
            tier: ship.tier,
        })
        .collect()
}

/// The plate the pointer is over, as the checker names it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateUnderPointer {
    pub zone: String,
    pub thickness: Millimeters,
}

/// What the checker keeps: the ships being compared, which of them a trajectory cast fires, and
/// the plate the verdicts are read against. One per pane, owned by the tab.
#[derive(Clone, Default)]
pub struct PenetrationState {
    /// The attackers being compared, in the order they were added. A list rather than one ship:
    /// the question is which of several ships gets through a plate.
    pub ships: Vec<ComparisonShip>,
    /// The ship whose shell a trajectory cast fires.
    pub firing_ship: Option<String>,
    /// The shell in that ship's list, when one was chosen.
    pub firing_shell: Option<usize>,
    pub ifhe: bool,
    /// The plate the pointer was last over. `None` until the pointer has been on the hull.
    pub plate: Option<PlateUnderPointer>,
}

impl PenetrationState {
    /// Whether `param_index` is already being compared.
    pub fn holds(&self, param_index: &str) -> bool {
        self.ships.iter().any(|ship| ship.param_index == param_index)
    }

    /// Adds a ship. The first one added is the one a cast fires.
    pub fn add(&mut self, ship: ComparisonShip) {
        if self.holds(&ship.param_index) {
            return;
        }
        if self.ships.is_empty() {
            self.firing_ship = Some(ship.param_index.clone());
            self.firing_shell = None;
        }
        self.ships.push(ship);
    }

    /// Drops the ship at `index`, and the cast's ship with it when it was that one.
    pub fn remove(&mut self, index: usize) {
        if index >= self.ships.len() {
            return;
        }
        self.ships.remove(index);
        if self.firing_ship.as_deref().is_none_or(|ship| !self.holds(ship)) {
            self.firing_ship = None;
            self.firing_shell = None;
        }
    }

    pub fn clear(&mut self) {
        self.ships.clear();
        self.firing_ship = None;
        self.firing_shell = None;
    }

    /// Fires casts with `param_index`'s ship, from its default shell.
    pub fn set_firing_ship(&mut self, param_index: &str) {
        if !self.holds(param_index) {
            return;
        }
        self.firing_ship = Some(param_index.to_owned());
        self.firing_shell = None;
    }

    /// Fires casts with the firing ship's shell at `shell_index`.
    pub fn set_firing_shell(&mut self, shell_index: usize) {
        let Some(ship) = self.firing() else { return };
        if shell_index < ship.shells.len() {
            self.firing_shell = Some(shell_index);
        }
    }

    /// The ship a cast fires, when there is one.
    pub fn firing(&self) -> Option<&ComparisonShip> {
        let firing = self.firing_ship.as_deref()?;
        self.ships.iter().find(|ship| ship.param_index == firing)
    }

    /// The ships `query` finds in `catalog`, best first, without those already compared. Empty for
    /// a query with nothing in it.
    pub fn matches<'a>(&self, catalog: &'a [CatalogEntry], query: &FilterQuery) -> Vec<&'a CatalogEntry> {
        if !query.is_active() {
            return Vec::new();
        }
        let addable = catalog.iter().filter(|entry| !self.holds(&entry.param_index));

        query.rank(addable, |entry| &entry.key).into_iter().take(SEARCH_RESULTS).collect()
    }
}

/// The checker's section: the plate being read against and the IFHE skill, the search, one block
/// per ship being compared, and the incoming fire log of a ship opened from a replay.
#[component]
pub fn PenCheckPanel(
    pen: RwSignal<PenetrationState, LocalStorage>,
    #[prop(into)] catalog: Signal<Option<Arc<ShipCatalog>>, LocalStorage>,
    #[prop(into)] assets: Signal<Option<Arc<ShipAssets>>, LocalStorage>,
    incoming: RwSignal<IncomingFire, LocalStorage>,
    #[prop(into)] playback_at: Signal<Option<GameClock>, LocalStorage>,
    viewport: ArmorViewportState,
    /// Asked to move playback to a moment, from the health strip or a salvo.
    #[prop(optional)]
    on_seek: Option<UnsyncCallback<GameClock>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;

    let against = move || match pen.with(|pen| pen.plate.clone()) {
        Some(plate) => AnyView::new(view! {
            row(class = "wt-armor-pen__against", attr:data-testid = "armor-pen-plate") {
                text(class = "wt-armor-pen__mm") {{format!("{:.0} mm", plate.thickness.value())}}
                text(class = "wt-armor-pen__dim") {{plate.zone.clone()}}
            }
        }),
        None => AnyView::new(view! {
            row(class = "wt-armor-pen__against") {
                text(class = "wt-armor-pen__hint") {{locale.text("ui.armor.pen.hover_a_plate")}}
            }
        }),
    };
    let ifhe = Signal::derive_local(move || if pen.with(|pen| pen.ifhe) { Checked::Yes } else { Checked::No });

    let ships = move || {
        let (ships, firing, plate, ifhe) = pen.with(|pen| {
            (pen.ships.clone(), pen.firing_ship.clone(), pen.plate.as_ref().map(|plate| plate.thickness), pen.ifhe)
        });
        if ships.is_empty() {
            return AnyView::new(view! {
                text(class = "wt-armor-pen__hint") {{locale.text("ui.armor.pen.add_a_ship")}}
            });
        }
        let ifhe = Ifhe::from_enabled(ifhe);
        let blocks: Vec<AnyView> = ships
            .into_iter()
            .enumerate()
            .map(|(index, ship)| {
                let firing = firing.as_deref() == Some(ship.param_index.as_str());
                ship_block(pen, index, ship, firing, plate, ifhe)
            })
            .collect();
        AnyView::new(view! {
            column(class = "wt-armor-pen__ships", attr:data-testid = "armor-pen-ships") {
                {blocks}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "armor-pen-clear",
                    on:click = move |_| pen.update(PenetrationState::clear)
                ) {
                    {locale.text("ui.armor.pen.clear_all")}
                }
            }
        })
    };

    view! {
        column(class = "wt-armor-pen", attr:data-testid = "armor-pen") {
            row(class = "wt-armor-pen__header") {
                {against}
                Tooltip {
                    TooltipTrigger {
                        row(class = "wt-armor-pen__check") {
                            Checkbox(
                                checked = Binding::controlled(ifhe, move |checked: Checked| {
                                    pen.update(|pen| pen.ifhe = checked == Checked::Yes)
                                }),
                                attr:data-testid = "armor-pen-ifhe",
                                a11y:label = locale.text("ui.armor.pen.ifhe")
                            )
                            text {{locale.text("ui.armor.pen.ifhe")}}
                        }
                    }
                    TooltipContent {{locale.text("ui.armor.pen.ifhe_tooltip")}}
                }
            }
            ShipSearch(pen = pen, catalog = catalog, assets = assets)
            column(class = "wt-armor-pen__ships-slot") {
                {ships}
            }
            IncomingLog(incoming = incoming, playback_at = playback_at, viewport = viewport, on_seek = on_seek)
        }
    }
}

/// The search that adds a ship to the comparison list, which both the checker and the trajectory
/// casts draw their attackers from.
#[component]
fn ShipSearch(
    pen: RwSignal<PenetrationState, LocalStorage>,
    catalog: Signal<Option<Arc<ShipCatalog>>, LocalStorage>,
    assets: Signal<Option<Arc<ShipAssets>>, LocalStorage>,
) -> impl IntoView {
    let query = RwSignal::new_local(String::new());
    let escape = EscapeClears::new(query);
    // Flattened once per catalog: thousands of names are folded for the search.
    let flattened: StoredValue<Option<FlattenedCatalog>, LocalStorage> = StoredValue::new_local(None);
    let entries = move || -> Option<Rc<Vec<CatalogEntry>>> {
        let catalog = catalog.get()?;
        let held = flattened.with_value(|held| {
            held.as_ref().filter(|held| Arc::ptr_eq(&held.catalog, &catalog)).map(|held| Rc::clone(&held.entries))
        });
        if held.is_some() {
            return held;
        }
        let built = Rc::new(catalog_entries(&catalog));
        flattened.set_value(Some(FlattenedCatalog { catalog, entries: Rc::clone(&built) }));
        Some(built)
    };
    // The search field is cleared once a ship is added, as the egui panel clears it: the ship is
    // in the list below now, and leaving the text would keep offering it.
    let add = move |param_index: &str| {
        let Some(assets) = assets.get() else { return };
        let Some(ship) = resolve_ship_shells(assets.metadata(), param_index) else { return };
        pen.update(|pen| pen.add(ship));
        query.set(String::new());
    };

    let results = move || {
        let typed = FilterQuery::new(&query.get());
        let Some(entries) = entries() else { return Vec::new() };
        let found: Vec<CatalogEntry> = pen.with(|pen| pen.matches(&entries, &typed).into_iter().cloned().collect());
        found
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let label = entry.label();
                let marks = marks_within(&typed, &label, &entry.display_name);
                let param_index = entry.param_index;
                AnyView::new(view! {
                    Button(
                        class = "wt-armor-pen__result",
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Sm,
                        a11y:label = label.clone(),
                        attr:data-testid = format!("armor-pen-add-{index}"),
                        on:click = move |_| add(&param_index)
                    ) {
                        MarkedText(text = label, ranges = marks)
                    }
                })
            })
            .collect::<Vec<_>>()
    };

    view! {
        column(class = "wt-armor-pen__search") {
            Input(
                value = query,
                placeholder = t!("ui.armor.pen.search_hint").into_owned(),
                label = t!("ui.armor.pen.search_hint").into_owned(),
                attr:data-testid = "armor-pen-search",
                on:key_down = move |ev| {
                    escape.key_down(ev);
                }
            )
            column(class = "wt-armor-pen__results", attr:data-testid = "armor-pen-results") {
                {results}
            }
        }
    }
}

/// One ship being compared: its name, whether casts fire it, a way to drop it, and its shells
/// against the plate under the pointer.
fn ship_block(
    pen: RwSignal<PenetrationState, LocalStorage>,
    index: usize,
    ship: ComparisonShip,
    firing: bool,
    plate: Option<Millimeters>,
    ifhe: Ifhe,
) -> AnyView {
    let name = format!("{} {}", tier_roman(ship.tier), ship.display_name);
    let rows: AnyView = if ship.shells.is_empty() {
        AnyView::new(view! {
            text(class = "wt-armor-pen__hint") {{t!("ui.armor.pen.no_shells", ship = ship.display_name.as_str()).into_owned()}}
        })
    } else {
        let rows: Vec<AnyView> =
            ship.shells.iter().enumerate().map(|(row, shell)| shell_row(row, shell, plate, ifhe)).collect();
        AnyView::new(view! { column {{rows}} })
    };
    let param_index = ship.param_index.clone();
    let use_label = t!("ui.armor.pen.use_for_trajectory").into_owned();
    let remove_label = t!("ui.armor.pen.remove").into_owned();
    let remove_tip = remove_label.clone();

    AnyView::new(view! {
        column(class = "wt-armor-pen__ship", attr:data-testid = format!("armor-pen-ship-{index}")) {
            row(class = "wt-armor-pen__ship-head") {
                text(class = "wt-armor-pen__ship-name") {{name}}
                Tooltip {
                    TooltipTrigger {
                        Toggle(
                            size = ToggleSize::Sm,
                            pressed = Binding::controlled(Signal::stored_local(firing), move |_: bool| {
                                pen.update(|pen| pen.set_firing_ship(&param_index))
                            }),
                            label = use_label.clone(),
                            attr:data-testid = format!("armor-pen-firing-{index}")
                        ) {
                            {use_label.clone()}
                        }
                    }
                    TooltipContent {{t!("ui.armor.pen.firing_ship_tooltip").into_owned()}}
                }
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::IconSm,
                            a11y:label = remove_label.clone(),
                            attr:data-testid = format!("armor-pen-remove-{index}"),
                            on:click = move |_| pen.update(|pen| pen.remove(index))
                        ) {
                            AppIcon(glyph = Glyph::X)
                        }
                    }
                    TooltipContent {{remove_tip.clone()}}
                }
            }
            {rows}
        }
    })
}

/// How a verdict is toned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Ok,
    Error,
    Warn,
    Dim,
}

impl Tone {
    fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Dim => "dim",
        }
    }
}

/// What a shell does to a plate of `thickness`, and the tone it reads in. No plate under the
/// pointer is no question yet, which reads differently from a shell the data has no figure for.
fn verdict(shell: &ShellInfo, thickness: Option<Millimeters>, ifhe: Ifhe) -> (&'static str, Tone) {
    match thickness.map(|thickness| check_penetration(shell, thickness, ifhe)) {
        None => ("ui.armor.pen.awaiting_plate", Tone::Dim),
        Some(Some(PenResult::Penetrates)) => ("ui.armor.pen.penetrates", Tone::Ok),
        Some(Some(PenResult::Bounces)) => ("ui.armor.pen.shatters", Tone::Error),
        Some(Some(PenResult::AngleDependent)) => ("ui.armor.pen.angle_dependent", Tone::Warn),
        Some(None) => ("ui.armor.pen.no_data", Tone::Dim),
    }
}

/// One shell against the plate: what it is, what it brings, and what happens.
fn shell_row(row: usize, shell: &ShellInfo, thickness: Option<Millimeters>, ifhe: Ifhe) -> AnyView {
    let (key, tone) = verdict(shell, thickness, ifhe);
    let (name, pen) = (shell_label(shell), pen_label(shell, ifhe));

    AnyView::new(view! {
        row(class = "wt-armor-pen__shell", attr:data-stripe = (row % 2 == 1).then(|| "true".to_owned())) {
            text(class = "wt-armor-pen__shell-name") {{name}}
            text(class = "wt-armor-pen__shell-pen") {{pen}}
            text(class = "wt-armor-pen__verdict", attr:data-tone = tone.name()) {{t!(key).into_owned()}}
        }
    })
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

/// What the shell brings to the plate: a flat figure for HE and SAP. AP is measured by overmatch
/// from its calibre, so it has none.
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
    use wows_toolkit_armor::catalog::ClassGroup;
    use wows_toolkit_armor::catalog::NationGroup;
    use wows_toolkit_armor::catalog::ShipCatalog;
    use wows_toolkit_armor::catalog::ShipEntry;
    use wows_toolkit_armor::filter_fields::FilterFields;
    use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
    use wows_toolkit_viewmodel::armor::penetration::Ifhe;
    use wows_toolkit_viewmodel::text_filter::FilterQuery;
    use wowsunpack::game_params::types::AmmoType;
    use wowsunpack::game_params::types::Millimeters;
    use wowsunpack::game_params::types::ShellInfo;
    use wowsunpack::game_params::types::Species;

    use super::PenetrationState;
    use super::Tone;
    use super::catalog_entries;
    use super::pen_label;
    use super::shell_label;
    use super::verdict;
    use crate::text_marks::marks_within;

    fn shell(ammo_type: AmmoType, caliber: f32, he_pen_mm: Option<f32>, sap_pen_mm: Option<f32>) -> ShellInfo {
        ShellInfo {
            name: "PTEST".to_owned(),
            ammo_type,
            caliber: Millimeters::from(caliber),
            he_pen_mm,
            sap_pen_mm,
            alpha_damage: 0.0,
            muzzle_velocity: 0.0,
            mass_kg: 0.0,
            krupp: 0.0,
            ricochet_angle: None,
            always_ricochet_angle: None,
            fuse_time: None,
            fuse_threshold: None,
            burn_prob: None,
            air_drag: 0.0,
            normalization: None,
            cap: true,
        }
    }

    fn ship(param_index: &str) -> ComparisonShip {
        ComparisonShip {
            param_index: param_index.to_owned(),
            display_name: param_index.to_owned(),
            tier: 10,
            nation: "usa".to_owned(),
            species: Species::Battleship,
            shells: vec![shell(AmmoType::HE, 152.0, Some(25.0), None), shell(AmmoType::AP, 406.0, None, None)],
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

    #[test]
    fn no_plate_awaits_one_rather_than_reading_as_missing_data() {
        let he = shell(AmmoType::HE, 152.0, Some(25.0), None);
        assert_eq!(verdict(&he, None, Ifhe::NotApplied), ("ui.armor.pen.awaiting_plate", Tone::Dim));
        assert_eq!(verdict(&he, Some(Millimeters::from(19.0)), Ifhe::NotApplied).1, Tone::Ok);
        assert_eq!(verdict(&he, Some(Millimeters::from(32.0)), Ifhe::NotApplied).1, Tone::Error);
    }

    fn catalog() -> ShipCatalog {
        let ship = |display_name: &str, tier: u32| ShipEntry {
            param_index: format!("P{display_name}"),
            display_name: display_name.to_owned(),
            filter_fields: FilterFields::new(),
            tier,
        };

        ShipCatalog {
            nations: vec![NationGroup {
                nation: "usa".to_owned(),
                display_name: "U.S.A.".to_owned(),
                classes: vec![ClassGroup {
                    species: Species::Battleship,
                    ships: vec![ship("S. Carolina", 3), ship("Sm\u{e5}land", 10), ship("Iowa", 9)],
                }],
            }],
            hidden: Vec::new(),
        }
    }

    #[test]
    fn the_attacker_search_folds_ranks_and_marks_its_results() {
        let entries = catalog_entries(&catalog());
        let state = PenetrationState::default();
        let found = |typed: &str| -> Vec<String> {
            state
                .matches(&entries, &FilterQuery::new(typed))
                .into_iter()
                .map(|entry| entry.display_name.clone())
                .collect()
        };

        assert_eq!(found("i"), ["Iowa", "S. Carolina"], "a name starting with it ranks first");
        assert_eq!(found("s carolina"), ["S. Carolina"]);
        assert_eq!(found("smaland"), ["Sm\u{e5}land"]);
        assert!(found(" ").is_empty(), "nothing typed offers nothing");

        let query = FilterQuery::new("smal");
        let matched = state.matches(&entries, &query);
        let label = matched[0].label();
        let marks = marks_within(&query, &label, &matched[0].display_name);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0], 2..7);
    }

    #[test]
    fn a_compared_ship_is_not_offered_again() {
        let entries = catalog_entries(&catalog());
        let mut state = PenetrationState::default();
        state.add(ship("PIowa"));

        let found: Vec<&str> = state
            .matches(&entries, &FilterQuery::new("i"))
            .into_iter()
            .map(|entry| entry.display_name.as_str())
            .collect();
        assert_eq!(found, ["S. Carolina"]);
    }

    #[test]
    fn the_first_ship_fires_and_dropping_it_clears_the_cast() {
        let mut state = PenetrationState::default();
        state.add(ship("A"));
        state.add(ship("B"));
        state.add(ship("A"));
        assert_eq!(state.ships.len(), 2, "a ship is compared once");
        assert_eq!(state.firing_ship.as_deref(), Some("A"));

        state.set_firing_shell(1);
        assert_eq!(state.firing_shell, Some(1));
        state.set_firing_shell(5);
        assert_eq!(state.firing_shell, Some(1), "a shell the ship lacks is refused");

        state.set_firing_ship("B");
        assert_eq!((state.firing_ship.as_deref(), state.firing_shell), (Some("B"), None));
        state.set_firing_ship("C");
        assert_eq!(state.firing_ship.as_deref(), Some("B"), "a ship not compared cannot fire");

        state.remove(1);
        assert_eq!((state.firing_ship.as_deref(), state.firing_shell), (None, None));
        assert_eq!(state.ships.len(), 1);
    }
}

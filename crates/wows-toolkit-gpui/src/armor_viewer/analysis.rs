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
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
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
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;

use std::collections::HashSet;

use wows_replays::analyzer::decoder::HitType;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::ComparisonVerdict;
use wows_toolkit_viewmodel::armor::arc::StoppingPlate;
use wows_toolkit_viewmodel::armor::incoming::IncomingSalvo;
use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;
use wows_toolkit_viewmodel::armor::incoming::group_incoming;

use super::catalog::ShipCatalog;
use super::catalog::tier_roman;
use super::pane::ArmorViewerPane;
use super::viewport_view::ArcSummary;
use super::viewport_view::Isolate;
use super::viewport_view::ViewportView;

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
        .child(render_arcs(view, cx))
        .child(render_incoming(view, pane, cx))
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

/// What was fired at this ship, salvo by salvo.
///
/// The egui app's Incoming Fire panel (`replay/realtime_armor_viewer.rs`), in
/// the panel this port already reads its armor questions in. Empty for a ship
/// opened from the catalogue, which nobody was shooting at.
fn render_incoming(view: &ArmorViewerPane, pane: &Entity<ArmorViewerPane>, cx: &App) -> AnyElement {
    let incoming = view.incoming();
    if incoming.context.taken.is_empty() && incoming.context.attackers.is_empty() {
        return div().into_any_element();
    }

    let enemies: HashSet<EntityId> = incoming.context.attackers.keys().copied().collect();
    let salvos = group_incoming(&incoming.context.taken, &incoming.filter, &enemies, &incoming.context.main_battery);
    let shells: usize = salvos.iter().map(|salvo| salvo.shells.len()).sum();
    let chosen = incoming.filter.attacker;
    // Read once for the whole log rather than per line: every line reaches for
    // the same viewport.
    let viewport = view.active_viewport(cx);
    let verdicts = viewport.read(cx);

    v_flex()
        .gap_1()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.realtime.incoming_fire").to_string()))
        .children(incoming.context.health.clone().map(|strip| render_health_strip(pane, strip, view.playback_at())))
        .child(
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.armor.realtime.tracked", salvos = salvos.len(), shells = shells).to_string()),
        )
        .children((verdicts.sim_unchecked() > 0).then(|| {
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.armor.realtime.sim_unchecked", count = verdicts.sim_unchecked()).to_string())
        }))
        .child(
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.armor.realtime.attacker_filter").to_string()),
        )
        .child(
            h_flex()
                .flex_wrap()
                .gap_1()
                .child(attacker_button(pane, None, t!("ui.armor.realtime.all_enemies").into_owned(), chosen))
                .children(
                    incoming
                        .context
                        .attackers
                        .iter()
                        .map(|(entity_id, named)| attacker_button(pane, Some(*entity_id), named.clone(), chosen)),
                ),
        )
        .child(
            Checkbox::new("armor-incoming-secondaries")
                .label(t!("ui.armor.realtime.show_secondaries").to_string())
                .checked(incoming.filter.secondaries)
                .on_click({
                    let pane = pane.clone();
                    move |checked: &bool, _window, cx: &mut App| {
                        let checked = *checked;
                        pane.update(cx, |pane, cx| pane.set_incoming_secondaries(checked, cx));
                    }
                }),
        )
        .child(if salvos.is_empty() {
            hint(t!("ui.armor.realtime.nothing_landed").as_ref(), cx)
        } else {
            v_flex()
                .id("armor-incoming-log")
                .gap_1()
                .max_h(LOG_MAX_HEIGHT)
                .overflow_y_scroll()
                .children(
                    salvos.iter().enumerate().map(|(index, salvo)| salvo_block(view, pane, index, salvo, verdicts)),
                )
                .into_any_element()
        })
        .into_any_element()
}

/// This ship's health over the battle, with what landed on it and where
/// playback has reached. A press along it moves playback there.
///
/// The egui panel draws the same strip above its own log
/// (`replay/realtime_armor_viewer.rs`'s `draw_health_timeline`); the shape comes
/// from the reading both apps share.
fn render_health_strip(
    pane: &Entity<ArmorViewerPane>,
    strip: wows_toolkit_viewmodel::armor::health_strip::HealthStrip,
    at: Option<GameClock>,
) -> AnyElement {
    let pressing = pane.clone();
    let seeking = strip.clone();

    div()
        .id("armor-incoming-health")
        .h(STRIP_HEIGHT)
        .w_full()
        .relative()
        .child(
            canvas(|_bounds, _window, _cx| {}, {
                let strip = strip.clone();
                move |bounds, _state, window, _cx| paint_health_strip(&strip, at, bounds, window)
            })
            .absolute()
            .inset_0(),
        )
        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _window, cx: &mut App| {
            // Where along the strip the press landed, which is the moment it
            // names. The strip fills its element, so its own width is the one
            // the press is measured against.
            let Some(bounds) = STRIP_BOUNDS.with(|held| held.get()) else { return };
            let width = bounds.size.width.as_f32();
            if width <= 0.0 {
                return;
            }
            let along = (event.position.x.as_f32() - bounds.origin.x.as_f32()) / width;
            let clock = seeking.clock_at(along);
            pressing.update(cx, |pane, cx| pane.seek_to(clock, cx));
        })
        .into_any_element()
}

/// How tall the health strip is drawn.
const STRIP_HEIGHT: Pixels = px(60.);

thread_local! {
    /// Where the strip was last painted, so a press on it can be measured.
    /// One board's strip at a time, which is what one window draws.
    static STRIP_BOUNDS: std::cell::Cell<Option<Bounds<Pixels>>> = const { std::cell::Cell::new(None) };
}

/// Draws the strip: what landed, the health line, and where playback is.
fn paint_health_strip(
    strip: &wows_toolkit_viewmodel::armor::health_strip::HealthStrip,
    at: Option<GameClock>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    STRIP_BOUNDS.with(|held| held.set(Some(bounds)));

    let semantic = crate::theme::semantic();
    let (left, top) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
    let (width, height) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
    // The ticks sit in a band along the bottom, and the line is drawn above it.
    let tick_height = height * 0.2;
    let line_height = height - tick_height;

    window.paint_quad(fill(bounds, rgb(semantic.text_faint).opacity(0.15)));

    for along in &strip.hits {
        let x = left + along * width;
        window.paint_quad(fill(
            Bounds { origin: point(px(x), px(top + height - tick_height)), size: size(px(1.), px(tick_height)) },
            rgb(semantic.armor_pen).opacity(0.55),
        ));
    }

    // Drawn as a run of short bars rather than a stroked path: what the strip
    // says is where the health stepped, and a bar per step says it without a
    // path type this viewport has no other use for.
    for pair in strip.line.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let (x0, x1) = (left + from.0 * width, left + to.0 * width);
        let y = top + line_height - from.1 * line_height;
        window.paint_quad(fill(
            Bounds { origin: point(px(x0), px(y)), size: size(px((x1 - x0).max(1.0)), px(1.5)) },
            rgb(semantic.ok),
        ));
        // The step down to the next reading, so a shell that took a third of
        // the ship reads as a drop rather than a slope.
        let next_y = top + line_height - to.1 * line_height;
        let (high, low) = if next_y < y { (next_y, y) } else { (y, next_y) };
        window.paint_quad(fill(
            Bounds { origin: point(px(x1), px(high)), size: size(px(1.5), px((low - high).max(1.0))) },
            rgb(semantic.ok),
        ));
    }

    if let Some(at) = at {
        let x = left + strip.along(at) * width;
        window.paint_quad(fill(
            Bounds { origin: point(px(x), px(top)), size: size(px(1.5), px(height)) },
            rgb(semantic.text_strong),
        ));
    }
}

/// How far the salvo log runs before it scrolls.
const LOG_MAX_HEIGHT: Pixels = px(220.);

/// One attacker the log can be narrowed to.
fn attacker_button(
    pane: &Entity<ArmorViewerPane>,
    attacker: Option<EntityId>,
    label: String,
    chosen: Option<EntityId>,
) -> AnyElement {
    let pane = pane.clone();
    let selected = attacker == chosen;
    // An element id per attacker, with the "every enemy" row on its own key
    // rather than a reserved number.
    let id: SharedString = match attacker {
        Some(attacker) => format!("armor-incoming-attacker-{}", attacker.raw()).into(),
        None => "armor-incoming-attacker-all".into(),
    };
    crate::ui::selectable(
        ElementId::from(id.clone()),
        selected,
        Button::new(id).label(label).compact().selected(selected).on_click(move |_event, _window, cx: &mut App| {
            pane.update(cx, |pane, cx| pane.set_incoming_attacker(attacker, cx));
        }),
    )
    .into_any_element()
}

/// One salvo: who fired it, when its first shell landed, and what each did.
fn salvo_block(
    view: &ArmorViewerPane,
    pane: &Entity<ArmorViewerPane>,
    index: usize,
    salvo: &IncomingSalvo,
    verdicts: &ViewportView,
) -> AnyElement {
    // A ship the roster did not name is one the frame had no row for, which
    // reads as its entity rather than as a blank.
    let who = view
        .incoming()
        .context
        .attackers
        .get(&salvo.attacker)
        .cloned()
        .unwrap_or_else(|| t!("ui.armor.realtime.unknown_attacker", id = salvo.attacker.raw()).into_owned());

    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().flex_1().text_xs().child(who))
                .child({
                    let pane = pane.clone();
                    let clock = salvo.first_clock;
                    Button::new(("armor-incoming-seek", index))
                        .label(clock_label(clock))
                        .compact()
                        .tooltip(t!("ui.armor.realtime.seek_to_salvo").to_string())
                        .on_click(move |_event, _window, cx: &mut App| {
                            pane.update(cx, |pane, cx| pane.seek_to(clock, cx));
                        })
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.armor.realtime.shells", count = salvo.shells.len()).to_string()),
                ),
        )
        .children(salvo.shells.iter().enumerate().map(|(shell_index, shell)| {
            let agreed = verdicts.sim_agreement(shell.shot_id);
            h_flex()
                .id(SharedString::from(format!("armor-incoming-shell-{index}-{shell_index}")))
                .gap_1()
                .pl(px(12.))
                .text_xs()
                .child(div().text_color(crate::theme::text_dim()).child(format!(
                    "{}  {}",
                    clock_label(shell.clock),
                    hit_label(&shell.hit_type)
                )))
                .children(agreed.map(|said| div().text_color(rgb(said.standing.color())).child(said.label.clone())))
        }))
        .into_any_element()
}

/// What this app's own simulation made of a shell, worded for the log.
///
/// Worded when the comparison is run rather than per frame: a battleship's log
/// runs to hundreds of lines, and each would otherwise translate and format
/// itself again on every draw.
pub(crate) struct SimAgreement {
    pub(crate) label: SharedString,
    pub(crate) standing: Agreement,
}

/// Whether the two agree, which is what colours the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Agreement {
    Agrees,
    /// The angle falls in the band where the game rolls for a ricochet, so
    /// either call is right and there is nothing to disagree about.
    Deferred,
    Disagrees,
}

impl Agreement {
    fn color(self) -> u32 {
        let semantic = crate::theme::semantic();
        match self {
            Self::Agrees => semantic.ok,
            Self::Deferred => semantic.warn,
            Self::Disagrees => semantic.error,
        }
    }
}

/// The reading beside a shell: whether this app's ballistics reach the same
/// answer the server did.
///
/// The egui viewer says the same three things about the same shell
/// (`replay/realtime_armor_viewer.rs`), in the panel beside its own hull.
pub(crate) fn agreement(verdict: &ComparisonVerdict) -> SimAgreement {
    match verdict {
        ComparisonVerdict::Match => {
            SimAgreement { label: t!("ui.armor.realtime.sim_agrees").into_owned().into(), standing: Agreement::Agrees }
        }
        ComparisonVerdict::RicochetRngDefer { angle, ricochet_start, always_ricochet } => SimAgreement {
            label: t!(
                "ui.armor.realtime.sim_rng_zone",
                angle = format!("{:.1}", angle.value()),
                from = format!("{:.1}", ricochet_start.value()),
                to = format!("{:.1}", always_ricochet.value()),
            )
            .into_owned()
            .into(),
            standing: Agreement::Deferred,
        },
        ComparisonVerdict::Mismatch { sim, server } => SimAgreement {
            label: t!("ui.armor.realtime.sim_disagrees", sim = t!(sim.label_key()), server = t!(server.label_key()),)
                .into_owned()
                .into(),
            standing: Agreement::Disagrees,
        },
    }
}

/// A game clock as the log prints it: minutes and seconds into the battle.
fn clock_label(clock: GameClock) -> String {
    let seconds = clock.seconds().max(0.0);
    format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
}

/// What the server said a shell did.
fn hit_label(hit: &HitType) -> String {
    let outcome = ServerOutcome::from_shell_hit_type(&hit.shell_hit);
    match &outcome {
        ServerOutcome::Unknown(raw) => t!(outcome.label_key(), raw = raw.clone()).into_owned(),
        _ => t!(outcome.label_key()).into_owned(),
    }
}

/// What the shells cast at the hull did.
///
/// The egui Analysis window's Trajectory tab, in the panel this port already
/// puts the penetration checker in: both answer the same question about the same
/// plates, and reading them side by side is the point.
fn render_arcs(view: &ArmorViewerPane, cx: &mut App) -> AnyElement {
    let viewport = view.active_viewport(cx);
    let arcs = viewport.read(cx).arcs();
    if arcs.is_empty() {
        return div().into_any_element();
    }

    v_flex()
        .gap_1()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div().flex_1().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.trajectory").to_string()),
                )
                .child({
                    let viewport = viewport.clone();
                    Button::new("armor-arcs-isolate-plates")
                        .label(t!("ui.armor.arc_isolate_plates").to_string())
                        .compact()
                        .tooltip(t!("ui.armor.arc_isolate_plates_tooltip").to_string())
                        .on_click(move |_event, _window, cx: &mut App| {
                            viewport.update(cx, |view, cx| view.isolate_all_arcs(Isolate::Plates, cx));
                        })
                })
                .child({
                    let viewport = viewport.clone();
                    Button::new("armor-arcs-isolate-zones")
                        .label(t!("ui.armor.arc_isolate_zones").to_string())
                        .compact()
                        .tooltip(t!("ui.armor.arc_isolate_zones_tooltip").to_string())
                        .on_click(move |_event, _window, cx: &mut App| {
                            viewport.update(cx, |view, cx| view.isolate_all_arcs(Isolate::Zones, cx));
                        })
                }),
        )
        // The simulation is read off the game's own data but is not the game's
        // own code, which is worth saying where its verdicts are read.
        .child(
            div()
                .text_xs()
                .text_color(rgb(crate::theme::semantic().warn))
                .child(t!("ui.armor.arc_simulation_caveat").to_string()),
        )
        .child(render_angle_legend())
        .children(arcs.iter().enumerate().map(|(index, arc)| arc_block(view, &viewport, index, arc)))
        .into_any_element()
}

/// What the impact markers' colours mean, which is how square each strike was.
fn render_angle_legend() -> impl IntoElement {
    let semantic = crate::theme::semantic();
    h_flex().gap_2().items_center().children(
        [
            (semantic.armor_angle_good, "ui.armor.angle_good"),
            (semantic.armor_angle_mid, "ui.armor.angle_mid"),
            (semantic.armor_angle_bad, "ui.armor.angle_bad"),
        ]
        .into_iter()
        .map(|(color, key)| {
            h_flex()
                .gap_1()
                .items_center()
                .child(div().size_2().rounded_full().bg(rgb(color)))
                .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!(key).to_string()))
        }),
    )
}

/// One cast arc: what it crossed, what became of the shell, and the two things
/// the reader can do to it.
fn arc_block(view: &ArmorViewerPane, viewport: &Entity<ViewportView>, index: usize, arc: &ArcSummary) -> AnyElement {
    let swatch = rgba(
        (((arc.color[0] * 255.0) as u32) << 24)
            | (((arc.color[1] * 255.0) as u32) << 16)
            | (((arc.color[2] * 255.0) as u32) << 8)
            | 0xff,
    );

    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().size_2().rounded_full().bg(swatch))
                .child(div().flex_1().text_xs().child(t!("ui.armor.arc_heading", number = index + 1).to_string()))
                .child(
                    div().text_xs().text_color(crate::theme::text_dim()).child(
                        t!(
                            "ui.armor.arc_summary",
                            hits = arc.hits,
                            armor = format!("{:.0}", arc.total_armor.value()),
                            range = format!("{:.1}", arc.range.value()),
                        )
                        .to_string(),
                    ),
                )
                .child({
                    let viewport = viewport.clone();
                    Button::new(("armor-arc-delete", index))
                        .child(crate::icons::icon(crate::icons::TRASH))
                        .compact()
                        .tooltip(t!("ui.armor.arc_delete").to_string())
                        .on_click(move |_event, _window, cx: &mut App| {
                            viewport.update(cx, |view, cx| view.remove_trajectory(index, cx));
                        })
                }),
        )
        .child(
            h_flex()
                .gap_1()
                .child({
                    let viewport = viewport.clone();
                    let on = arc.isolating_plates;
                    crate::ui::selectable(
                        ("armor-arc-plates", index),
                        on,
                        Button::new(("armor-arc-plates-button", index))
                            .label(t!("ui.armor.arc_isolate_plates").to_string())
                            .compact()
                            .selected(on)
                            .on_click(move |_event, _window, cx: &mut App| {
                                viewport.update(cx, |view, cx| view.isolate_arc(index, Isolate::Plates, !on, cx));
                            }),
                    )
                })
                .child(arc_range_step(viewport, index, arc.range, -RANGE_STEP, "armor-arc-range-down", "-"))
                .child(arc_range_step(viewport, index, arc.range, RANGE_STEP, "armor-arc-range-up", "+"))
                .child({
                    let viewport = viewport.clone();
                    let on = arc.isolating_zones;
                    crate::ui::selectable(
                        ("armor-arc-zones", index),
                        on,
                        Button::new(("armor-arc-zones-button", index))
                            .label(t!("ui.armor.arc_isolate_zones").to_string())
                            .compact()
                            .selected(on)
                            .on_click(move |_event, _window, cx: &mut App| {
                                viewport.update(cx, |view, cx| view.isolate_arc(index, Isolate::Zones, !on, cx));
                            }),
                    )
                }),
        )
        .child(div().text_xs().text_color(outcome_color(&arc.outcome)).child(describe_outcome(&arc.outcome, view)))
        .into_any_element()
}

/// How far one press moves an arc's firing range, and the span it is held to.
///
/// The same span the cast-range slider offers, so an arc cannot be stepped to a
/// range the viewport would not fire from.
const RANGE_STEP: f32 = 0.5;
const RANGE_MIN: f32 = 0.0;
const RANGE_MAX: f32 = 30.0;

/// One press of an arc's range, up or down.
///
/// A stepper rather than a slider: a slider would need state of its own per arc,
/// and the arcs come and go as the reader casts.
fn arc_range_step(
    viewport: &Entity<ViewportView>,
    index: usize,
    range: Km,
    by: f32,
    id: &'static str,
    label: &'static str,
) -> impl IntoElement + use<> {
    let viewport = viewport.clone();
    let stepped = (range.value() + by).clamp(RANGE_MIN, RANGE_MAX);
    Button::new((id, index))
        .label(label)
        .compact()
        .tooltip(t!("ui.armor.arc_range_tooltip").to_string())
        .disabled((stepped - range.value()).abs() < f32::EPSILON)
        .on_click(move |_event, _window, cx: &mut App| {
            viewport.update(cx, |view, cx| view.set_trajectory_range(index, Km::new(stepped), cx));
        })
}

/// What an outcome reads as.
fn describe_outcome(outcome: &ArcOutcome, view: &ArmorViewerPane) -> String {
    match outcome {
        ArcOutcome::NotSimulated => t!("ui.armor.arc_no_shell").into_owned(),
        ArcOutcome::Detonated { zone } => match zone {
            Some(zone) => t!("ui.armor.arc_detonated_in", zone = zone.clone()).into_owned(),
            None => t!("ui.armor.arc_detonated_past_armor").into_owned(),
        },
        ArcOutcome::Ricocheted { plate } => {
            t!("ui.armor.arc_ricochet", plate = describe_plate(plate, view)).into_owned()
        }
        ArcOutcome::Shattered { plate } => t!("ui.armor.arc_shatter", plate = describe_plate(plate, view)).into_owned(),
        ArcOutcome::Stopped { plate } => t!("ui.armor.arc_stopped", plate = describe_plate(plate, view)).into_owned(),
        ArcOutcome::Overpenetrated { fuse_armed: true } => t!("ui.armor.arc_overpen").into_owned(),
        ArcOutcome::Overpenetrated { fuse_armed: false } => t!("ui.armor.arc_overpen_unarmed").into_owned(),
    }
}

/// The plate a shell stopped at, or that it stopped at one nothing is known
/// about, which a cast whose plate list is shorter than its simulation reports.
fn describe_plate(plate: &Option<StoppingPlate>, view: &ArmorViewerPane) -> String {
    match plate {
        // Named as the reader knows it: the material key is a mesh name, and
        // the build has a translation for it.
        Some(plate) => {
            let named = view.translate_part(&plate.material);
            format!("#{} {:.0}mm {}", plate.number, plate.thickness.value(), named)
        }
        None => t!("ui.armor.arc_plate_unknown").into_owned(),
    }
}

/// The tone an outcome is read in: the armor palette both viewers paint their
/// verdicts in, where a detonation is the result the reader was after.
fn outcome_color(outcome: &ArcOutcome) -> Hsla {
    let semantic = crate::theme::semantic();
    match outcome {
        ArcOutcome::Detonated { .. } => rgb(semantic.armor_pen).into(),
        ArcOutcome::Overpenetrated { .. } => rgb(semantic.armor_overpen).into(),
        ArcOutcome::Ricocheted { .. } => rgb(semantic.armor_ricochet).into(),
        ArcOutcome::Shattered { .. } | ArcOutcome::Stopped { .. } => rgb(semantic.armor_shatter).into(),
        ArcOutcome::NotSimulated => crate::theme::text_dim(),
    }
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

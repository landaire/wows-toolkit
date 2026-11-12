//! The drawing tools popover on the transport.

use std::rc::Rc;

use landaire_ui::MarkedTextProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::annotate::Annotator;
use wows_toolkit_replay_data::replay_render::annotate::ENEMY_TINT;
use wows_toolkit_replay_data::replay_render::annotate::FRIENDLY_TINT;
use wows_toolkit_replay_data::replay_render::annotate::INKS;
use wows_toolkit_replay_data::replay_render::annotate::MAX_NIB;
use wows_toolkit_replay_data::replay_render::annotate::MIN_NIB;
use wows_toolkit_replay_data::replay_render::annotate::PlaceableShip;
use wows_toolkit_replay_data::replay_render::annotate::SPECIES;
use wows_toolkit_replay_data::replay_render::annotate::ShipCatalog;
use wows_toolkit_replay_data::replay_render::annotate::TOOLS;
use wows_toolkit_replay_data::replay_render::annotate::ToolButton;
use wows_toolkit_replay_data::replay_render::annotate::places_ship;
use wows_toolkit_replay_data::replay_render::annotate::same_tool;
use wows_toolkit_replay_data::replay_render::annotate::ship_tool;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SeparatorProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::text_marks::marks_within;
use crate::windows::renderer::session::RendererSession;

const SHEET: &str = "wt-renderer-tools";

const CSS: &str = css!(
    ".wt-renderer-tools { display: flex; flex-direction: column; gap: 8px; width: 230px; padding: 8px; }
    .wt-renderer-tools__row { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
    .wt-renderer-tools__chooser { display: flex; flex-direction: column; gap: 4px; }
    .wt-renderer-tools__dim { flex: 1; font-size: 12px; color: var(--lui-color-text-dim); }
    .wt-renderer-tools__nib { width: 16px; font-size: 12px; }
    .wt-renderer-tools__ink { width: 18px; height: 18px; border-radius: 3px; cursor: pointer; }
    .wt-renderer-tools__ink[data-chosen] { border: 2px solid #ffffff; }"
);

/// The drawing tools, on the transport beside the gear.
///
/// Refused without a session: an annotation is something everyone in one sees, and there is
/// nowhere to put one otherwise.
#[component]
pub fn ToolsPopover(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let refused = Signal::derive_local(move || !session.collab.with(CollabLink::is_active));
    // Kept with the transport, so the catalogue is read once rather than each time the popover opens.
    let catalog: StoredValue<Option<Rc<ShipCatalog>>, LocalStorage> = StoredValue::new_local(None);
    let named = t!("ui.renderer.annotations.title").into_owned();
    let label = named.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                Popover {
                    PopoverTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "replay-renderer-tools-toggle",
                        state:disabled = move || refused.get(),
                        a11y:disabled = move || refused.get(),
                        a11y:label = label.clone()
                    ) {
                        AppIcon(glyph = Glyph::NotebookPen)
                    }
                    PopoverContent(attr:data-testid = "replay-renderer-tools") {
                        ToolsPanel(session = session, catalog = catalog)
                    }
                }
            }
            TooltipContent {{named.clone()}}
        }
    }
}

/// What the popover holds.
#[component]
fn ToolsPanel(session: RendererSession, catalog: StoredValue<Option<Rc<ShipCatalog>>, LocalStorage>) -> impl IntoView {
    let undo_refused = Signal::derive_local(move || !session.annotator.with(Annotator::can_undo));
    // A peer's change can rename or remove the picked ship, so it is read again as the session
    // redraws; the memo keeps the chooser, and what is typed in it, until the answer changes.
    let picked_ship = Memo::new(move |_| {
        let _ = session.collab_frame.get();
        session.collab.with(|link| session.annotator.with(|annotator| annotator.picked_ship(link)))
    });
    let chooser = move || {
        let named = picked_ship.get()?;
        Some(AnyView::new(view! { ShipChooser(session = session, named = named, catalog = catalog) }))
    };
    let tools: Vec<AnyView> =
        TOOLS.iter().map(|button| AnyView::new(view! { ToolToggle(session = session, button = button) })).collect();
    let inks: Vec<AnyView> = INKS
        .iter()
        .map(|&(id, ink)| AnyView::new(view! { InkSwatch(session = session, id = id, ink = ink) }))
        .collect();

    view! {
        column(class = "wt-renderer-tools") {
            row(class = "wt-renderer-tools__row") {
                {tools}
            }
            {chooser}
            Separator()
            SpeciesRow(session = session, friendly = true)
            SpeciesRow(session = session, friendly = false)
            Separator()
            row {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Sm,
                    disabled = undo_refused,
                    attr:data-testid = "replay-renderer-annotations-undo",
                    on:click = move |_| {
                        session.annotate(|annotator, link| annotator.undo(link));
                    }
                ) {
                    AppIcon(glyph = Glyph::RotateCcw)
                    text {{t!("ui.renderer.annotations.undo").into_owned()}}
                }
            }
            Separator()
            NibRow(session = session)
            row(class = "wt-renderer-tools__row") {
                {inks}
            }
        }
    }
}

/// One tool on the popover, chosen while it is the one in hand.
#[component]
fn ToolToggle(session: RendererSession, button: &'static ToolButton) -> impl IntoView {
    let make = button.tool;
    let on = Signal::derive_local(move || {
        session.annotator.with(|annotator| same_tool(annotator.drawing().tool(), &make()))
    });
    let named = match button.chord {
        Some(chord) => format!("{} ({chord})", t!(button.label_key)),
        None => t!(button.label_key).into_owned(),
    };
    let label = named.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                Toggle(
                    size = ToggleSize::Sm,
                    pressed = Binding::controlled(on, move |_: bool| session.take_up(make())),
                    label = label.clone(),
                    attr:data-testid = button.id
                ) {
                    AppIcon(glyph = button.glyph)
                }
            }
            TooltipContent {{named.clone()}}
        }
    }
}

/// A row of ships to place, for one side, tinted by it.
#[component]
fn SpeciesRow(session: RendererSession, friendly: bool) -> impl IntoView {
    let tint = format!("#{:06x}", if friendly { FRIENDLY_TINT } else { ENEMY_TINT });
    let side = if friendly { "friendly" } else { "enemy" };
    let ships: Vec<AnyView> = SPECIES
        .iter()
        .map(|(short, species)| {
            let on = Signal::derive_local(move || {
                session.annotator.with(|annotator| places_ship(annotator.drawing().tool(), species, friendly))
            });
            let tint = tint.clone();
            AnyView::new(view! {
                Toggle(
                    size = ToggleSize::Sm,
                    pressed = Binding::controlled(on, move |_: bool| session.take_up(ship_tool(species, friendly))),
                    attr:data-testid = format!("replay-renderer-place-{side}-{short}"),
                    style:color = move || Some(tint.clone())
                ) {
                    {*short}
                }
            })
        })
        .collect();

    view! {
        row(class = "wt-renderer-tools__row") {
            {ships}
        }
    }
}

/// How thick what is drawn is, and the two steps either way. The bracket keys move the same value.
#[component]
fn NibRow(session: RendererSession) -> impl IntoView {
    let nib = move || session.annotator.with(|annotator| annotator.drawing().width());
    let step = move |by: f32| {
        session.annotate(|annotator, _| annotator.step_nib(by));
    };
    let shown = move || format!("{:.0}", nib());

    view! {
        row(class = "wt-renderer-tools__row") {
            text(class = "wt-renderer-tools__dim") {{t!("ui.renderer.annotations.nib").into_owned()}}
            NibStep(
                label_key = "ui.renderer.annotations.thinner",
                test_id = "replay-renderer-nib-thinner",
                glyph = "-",
                disabled = Signal::derive_local(move || nib() <= MIN_NIB),
                on_press = Rc::new(move || step(-1.0))
            )
            text(class = "wt-renderer-tools__nib", attr:data-testid = "replay-renderer-nib", a11y:label = shown) {{shown}}
            NibStep(
                label_key = "ui.renderer.annotations.thicker",
                test_id = "replay-renderer-nib-thicker",
                glyph = "+",
                disabled = Signal::derive_local(move || nib() >= MAX_NIB),
                on_press = Rc::new(move || step(1.0))
            )
        }
    }
}

/// One step of the nib, named by its tooltip.
#[component]
fn NibStep(
    label_key: &'static str,
    test_id: &'static str,
    glyph: &'static str,
    disabled: Signal<bool, LocalStorage>,
    on_press: Rc<dyn Fn()>,
) -> impl IntoView {
    let named = t!(label_key).into_owned();
    let label = named.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    disabled = disabled,
                    attr:data-testid = test_id,
                    a11y:label = label.clone(),
                    on:click = move |_| on_press()
                ) {
                    {glyph}
                }
            }
            TooltipContent {{named.clone()}}
        }
    }
}

/// One ink a tool can draw with, outlined while it is the one in use.
#[component]
fn InkSwatch(session: RendererSession, id: &'static str, ink: [u8; 4]) -> impl IntoView {
    let [red, green, blue, _] = ink;
    let chosen =
        move || session.annotator.with(|annotator| annotator.drawing().color() == ink).then(|| "true".to_owned());

    view! {
        box(
            class = "wt-renderer-tools__ink",
            attr:data-testid = id,
            attr:data-chosen = chosen,
            style:background-color = format!("#{red:02x}{green:02x}{blue:02x}"),
            on:click = move |_| {
                session.annotate(|annotator, _| annotator.set_ink(ink));
            }
        )
    }
}

/// Where a placed ship is given an identity, shown only while one is picked out: a ship's ranges
/// are read from its own game data, so until one is chosen there is nothing to read.
#[component]
fn ShipChooser(
    session: RendererSession,
    named: Option<String>,
    catalog: StoredValue<Option<Rc<ShipCatalog>>, LocalStorage>,
) -> impl IntoView {
    let query = RwSignal::new_local(String::new());
    // Built the first time something is typed, and again after that only while no build is loaded.
    let ships = move || -> Option<Rc<ShipCatalog>> {
        if let Some(built) = catalog.with_value(Option::clone) {
            return Some(built);
        }
        let built = Rc::new(ShipCatalog::from_game_data(&session.game_data()?)?);
        catalog.update_value(|held| *held = Some(Rc::clone(&built)));

        Some(built)
    };
    let choose = move |ship: &PlaceableShip| {
        let Some(game_data) = session.game_data() else { return };
        let (species, param_index, display_name) = (ship.species, ship.param_index.clone(), ship.display_name.clone());
        session.annotate(|annotator, link| {
            annotator.name_picked_ship(link, &game_data, species, &param_index, display_name)
        });
        query.set(String::new());
    };
    let matches = move || {
        let typed = FilterQuery::new(&query.get());
        let Some(ships) = ships() else { return Vec::new() };
        ships
            .matching(&typed)
            .into_iter()
            .map(|ship| {
                let ship = ship.clone();
                let test_id = format!("replay-renderer-ship-{}", ship.param_index);
                let label = ship.label();
                let marks = marks_within(&typed, &label, &ship.display_name);
                AnyView::new(view! {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Sm,
                        a11y:label = label.clone(),
                        attr:data-testid = test_id,
                        on:click = move |_| choose(&ship)
                    ) {
                        MarkedText(text = label, ranges = marks)
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    let named = named.unwrap_or_else(|| t!("ui.renderer.annotations.no_ship").into_owned());

    view! {
        column(class = "wt-renderer-tools__chooser", attr:data-testid = "replay-renderer-ship-chooser") {
            Separator()
            text(class = "wt-renderer-tools__dim", attr:data-testid = "replay-renderer-ship-named") {{named}}
            Input(
                value = query,
                placeholder = t!("ui.renderer.annotations.ship_hint").into_owned(),
                label = t!("ui.renderer.annotations.ship_hint").into_owned(),
                attr:data-testid = "replay-renderer-ship-search"
            )
            column {
                {matches}
            }
        }
    }
}

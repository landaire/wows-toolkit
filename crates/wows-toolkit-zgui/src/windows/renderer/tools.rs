//! The drawing tools popover on the transport.

use std::rc::Rc;

use landaire_ui::EscapeClears;
use landaire_ui::MarkedTextProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
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
use wows_toolkit_replay_data::replay_render::annotate::is_fillable;
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
    .wt-renderer-tools__ink[data-chosen] { border: 2px solid #ffffff; }
    .wt-renderer-tools__clear.wt-renderer-tools__clear { color: var(--lui-color-danger); }
    .wt-renderer-tools__toggle[data-locked] { color: var(--wt-color-warn); }"
);

/// The drawing tools, on the transport beside the gear. Offered alone as in a session: a reader
/// draws on their own replay whether or not anyone else sees it. Refused with a lock while the
/// session's host has locked what is drawn, as the egui toolbar is.
#[component]
pub fn ToolsPopover(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    // The host's permissions change under the session, so they are read again as it redraws.
    let locked = Signal::derive_local(move || {
        let _ = session.collab_frame.get();
        session.collab.with(CollabLink::annotations_locked)
    });
    // Kept with the transport, so the catalogue is read once rather than each time the popover opens.
    let catalog: StoredValue<Option<Rc<ShipCatalog>>, LocalStorage> = StoredValue::new_local(None);
    // The ship chooser's search, held here so Escape in it empties it before it closes the popover.
    let query = RwSignal::new_local(String::new());
    let escape = EscapeClears::new(query);
    let named = move || {
        let key = if locked.get() { "ui.renderer.annotations.locked" } else { "ui.renderer.annotations.title" };
        t!(key).into_owned()
    };
    let glyph = move || {
        let glyph = if locked.get() { Glyph::Lock } else { Glyph::NotebookPen };
        view! { AppIcon(glyph = glyph) }
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Popover {
                    PopoverTrigger(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::IconXs,
                        class = "wt-renderer-tools__toggle",
                        attr:data-testid = "replay-renderer-tools-toggle",
                        attr:data-locked = move || locked.get().then(String::new),
                        state:disabled = move || locked.get(),
                        a11y:disabled = move || locked.get(),
                        a11y:label = named
                    ) {
                        {glyph}
                    }
                    PopoverContent(dismiss_on_escape = escape.dismiss_on_escape(), attr:data-testid = "replay-renderer-tools") {
                        ToolsPanel(session = session, catalog = catalog, query = query, escape = escape)
                    }
                }
            }
            TooltipContent {{named}}
        }
    }
}

/// What the popover holds.
#[component]
fn ToolsPanel(
    session: RendererSession,
    catalog: StoredValue<Option<Rc<ShipCatalog>>, LocalStorage>,
    query: RwSignal<String, LocalStorage>,
    escape: EscapeClears,
) -> impl IntoView {
    let undo_refused = Signal::derive_local(move || !session.annotator.with(Annotator::can_undo));
    // A peer can draw or erase, so the count is read again as the session redraws.
    let nothing_drawn = Signal::derive_local(move || {
        let _ = session.collab_frame.get();
        session.annotator.with(|_| ());
        session.collab.with(CollabLink::annotation_count) == 0
    });
    // A peer's change can rename or remove the picked ship, so it is read again as the session
    // redraws; the memo keeps the chooser, and what is typed in it, until the answer changes.
    let picked_ship = Memo::new(move |_| {
        let _ = session.collab_frame.get();
        session.collab.with(|link| session.annotator.with(|annotator| annotator.picked_ship(link)))
    });
    let chooser = move || {
        let named = picked_ship.get()?;
        Some(AnyView::new(
            view! { ShipChooser(session = session, named = named, catalog = catalog, query = query, escape = escape) },
        ))
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
            row(class = "wt-renderer-tools__row") {
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
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Sm,
                    class = "wt-renderer-tools__clear",
                    disabled = nothing_drawn,
                    attr:data-testid = "replay-renderer-annotations-clear",
                    on:click = move |_| {
                        session.annotate(|annotator, link| annotator.clear_all(link));
                    }
                ) {
                    AppIcon(glyph = Glyph::Trash)
                    text {{t!("ui.renderer.annotations.clear_all").into_owned()}}
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

/// One tool on the popover, chosen while it is the one in hand. Pressed again, a shape's button
/// fills the shape or empties it.
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
    let fillable = is_fillable(&make());
    let tip = move || {
        if fillable && on.get() {
            format!("{named} - {}", t!("ui.renderer.annotations.toggle_fill"))
        } else {
            named.clone()
        }
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Toggle(
                    size = ToggleSize::Sm,
                    pressed = Binding::controlled(on, move |_: bool| {
                        session.annotate(|annotator, _| annotator.choose(make()));
                    }),
                    label = label.clone(),
                    attr:data-testid = button.id
                ) {
                    AppIcon(glyph = button.glyph)
                }
            }
            TooltipContent {{tip.clone()}}
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
    query: RwSignal<String, LocalStorage>,
    escape: EscapeClears,
) -> impl IntoView {
    // Each chooser starts empty, as the ship it names may have changed.
    query.set(String::new());
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
                node_ref = escape.node_ref(),
                attr:data-testid = "replay-renderer-ship-search",
                on:key_down = move |ev| {
                    escape.key_down(ev);
                }
            )
            column {
                {matches}
            }
        }
    }
}

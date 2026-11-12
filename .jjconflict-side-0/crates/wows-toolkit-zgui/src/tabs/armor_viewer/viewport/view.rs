//! The viewport element: the scene's surface, the overlay over it, what stands in the scene's
//! place while there is none, the hover tooltips, the readouts and the context menu.

use std::time::Instant;

use landaire_ui::OverlaySurfaceProps;
use rust_i18n::t;
use wows_toolkit_armor::catalog::translate_name;
use wows_toolkit_armor::hull_picking::MeshTooltip;
use wows_toolkit_armor::load_ship::ArmorTriangleTooltip;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::viewport::types::Vec2;
use wowsunpack::export::gltf_export::thickness_to_color;
use zgui::component;
use zgui::css;
use zgui::custom::CustomHandle;
use zgui::elements::Focus;
use zgui::elements::surface;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::prelude::Fonts;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::use_context;
use zgui::reactive::use_local_context;
use zgui::surface::SurfaceElementExt;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui::SpinnerProps;

use crate::i18n::Locale;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::ModifierKey;

use super::host::ViewportRenderer;
use super::input::KeyModifiers;
use super::input::KeyOutcome;
use super::input::ViewKey;
use super::model::GpuState;
use super::overlay::OverlayElement;
use super::state::ArmorViewportState;

const SHEET: &str = "wt-armor-viewport";

const CSS: &str = css!(
    ".wt-armor-viewport__trigger {
        display: flex;
        flex-direction: column;
        flex: 1 1 0;
        min-width: 0;
        min-height: 0;
    }
    .wt-armor-viewport {
        position: relative;
        flex: 1 1 0;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }
    .wt-armor-viewport__scene {
        position: absolute;
        inset: 0;
        display: block;
        width: 100%;
        height: 100%;
    }
    .wt-armor-viewport__overlay {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        pointer-events: none;
    }
    .wt-armor-viewport__shade {
        position: absolute;
        pointer-events: none;
    }
    .wt-armor-viewport__shade--left {
        left: 0; top: 0; bottom: 0; width: 10px;
        background: linear-gradient(90deg, rgba(0, 0, 0, 0.3), rgba(0, 0, 0, 0));
    }
    .wt-armor-viewport__shade--right {
        right: 0; top: 0; bottom: 0; width: 10px;
        background: linear-gradient(270deg, rgba(0, 0, 0, 0.3), rgba(0, 0, 0, 0));
    }
    .wt-armor-viewport__shade--top {
        left: 0; right: 0; top: 0; height: 10px;
        background: linear-gradient(180deg, rgba(0, 0, 0, 0.3), rgba(0, 0, 0, 0));
    }
    .wt-armor-viewport__shade--bottom {
        left: 0; right: 0; bottom: 0; height: 10px;
        background: linear-gradient(0deg, rgba(0, 0, 0, 0.3), rgba(0, 0, 0, 0));
    }
    .wt-armor-viewport__said {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 4px;
        font-size: var(--zui-type-size-sm);
        pointer-events: none;
    }
    .wt-armor-viewport__said .zui-spinner {
        width: 14px;
        height: 14px;
    }
    .wt-armor-viewport__dim {
        color: var(--wt-color-text-dim);
    }
    .wt-armor-viewport__readout {
        position: absolute;
        left: 8px;
        padding: 4px 8px;
        border-radius: var(--zui-radius-md);
        background-color: color-mix(in srgb, var(--zui-color-background) 85%, transparent);
        border: 1px solid var(--zui-color-border);
        font-size: var(--zui-type-size-xs);
        pointer-events: none;
    }
    .wt-armor-viewport__readout--top { top: 8px; }
    .wt-armor-viewport__readout--log {
        top: 40px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .wt-armor-viewport__readout--bottom { bottom: 8px; }
    .wt-armor-viewport__log-line {
        display: flex;
        flex-direction: row;
        gap: 8px;
    }
    .wt-armor-viewport__tooltip {
        position: absolute;
        padding: 4px 8px;
        pointer-events: none;
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-width: 320px;
    }
    .wt-armor-viewport__tooltip-head {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        font-size: 14px;
        font-weight: var(--zui-type-weight-bold);
    }
    .wt-armor-viewport__tooltip-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        font-size: var(--zui-type-size-sm);
    }
    .wt-armor-viewport__tooltip-row[data-this] {
        font-weight: var(--zui-type-weight-bold);
    }
    .wt-armor-viewport__tooltip-text {
        font-size: var(--zui-type-size-sm);
    }
    .wt-armor-viewport__tooltip-muted {
        font-size: var(--zui-type-size-xs);
        color: var(--zui-color-muted-foreground);
    }
    .wt-armor-viewport__tooltip-rule {
        height: 1px;
        background-color: var(--zui-color-border);
    }
    .wt-armor-viewport__swatch {
        flex: none;
        border-radius: 2px;
    }
    .wt-armor-viewport__backdrop {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: rgba(0, 0, 0, 0.35);
    }
    .wt-armor-viewport__confirm {
        width: 360px;
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 16px;
    }
    .wt-armor-viewport__confirm-title {
        font-weight: var(--zui-type-weight-bold);
    }
    .wt-armor-viewport__confirm-body {
        font-size: var(--zui-type-size-sm);
    }
    .wt-armor-viewport__confirm-actions {
        display: flex;
        flex-direction: row;
        gap: 8px;
        justify-content: flex-end;
    }"
);

/// How far the tooltips sit from the pointer, so they do not sit under it.
const TOOLTIP_CURSOR_OFFSET: f32 = 16.0;

/// The part of the viewport past which a tooltip opens toward the pointer rather than away from
/// it, so it stays inside the viewport.
const TOOLTIP_FLIP: f32 = 0.6;

/// How tall a line of wheel scrolling is, in CSS pixels.
const WHEEL_LINE: f32 = 20.0;

/// The viewport showing `state`: the scene with its gizmo and labels, the pointer and keyboard
/// driving its camera, hover tooltips and the plate menu.
#[component]
pub fn ArmorViewport(state: ArmorViewportState) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    // Absent outside the main window, where nothing can be exported to.
    let shell = use_local_context::<Shell>();
    let area = NodeRef::new();
    let overlay = overlay_element(state);

    // Where `position`, in window CSS pixels, sits in the viewport.
    let local = move |position: zgui::geom::Point<CssPx, Css>| -> Option<Vec2> {
        let bounds = area.window_bounds()?;
        let scale = area.scale();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        Some(Vec2::new(position.x.0 - bounds.origin.x.0 / scale, position.y.0 - bounds.origin.y.0 / scale))
    };

    let on_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
        let Some(button) = ev.button else { return };
        let Some(at) = local(ev.position) else { return };
        area.focus();
        let when = ev.timestamp;
        let drag = state.update(|model, cx| model.pointer_down(at, button, when, cx)).unwrap_or(false);
        if drag {
            ev.capture_pointer();
        }
    };
    let on_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
        let Some(at) = local(ev.position) else { return };
        state.update(|model, cx| model.pointer_move(at, cx));
    };
    let on_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
        let Some(button) = ev.button else { return };
        ev.release_pointer();
        let shift = ev.modifiers.shift();
        match local(ev.position) {
            Some(at) => state.update(|model, cx| model.pointer_up(at, button, shift, cx)),
            None => state.update(|model, _| model.pointer_cancelled()),
        };
    };
    let on_key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
        let lowered;
        let key = match &ev.key_without_modifiers {
            Key::Named(NamedKey::ArrowUp) => ViewKey::Up,
            Key::Named(NamedKey::ArrowDown) => ViewKey::Down,
            Key::Named(NamedKey::ArrowLeft) => ViewKey::Left,
            Key::Named(NamedKey::ArrowRight) => ViewKey::Right,
            Key::Character(text) => {
                lowered = text.to_lowercase();
                ViewKey::Character(&lowered)
            }
            _ => return,
        };
        let modifiers = KeyModifiers { primary: ev.modifiers.primary(), shift: ev.modifiers.shift() };
        let repeat = ev.repeat;
        let outcome =
            state.update(|model, cx| model.key_down(key, modifiers, repeat, cx)).unwrap_or(KeyOutcome::Ignored);
        match outcome {
            KeyOutcome::Handled => ev.prevent_default(),
            KeyOutcome::ToggleOptions => {
                if let Some(menu) = state.pane_menu() {
                    menu.toggle_options.run(());
                }
                ev.prevent_default();
            }
            KeyOutcome::Ignored => {}
        }
    };
    let on_key_up = move |ev: &mut EventCx<'_, events::KeyUp>| {
        let lowered;
        let key = match &ev.key_without_modifiers {
            Key::Named(NamedKey::ArrowUp) => ViewKey::Up,
            Key::Named(NamedKey::ArrowDown) => ViewKey::Down,
            Key::Named(NamedKey::ArrowLeft) => ViewKey::Left,
            Key::Named(NamedKey::ArrowRight) => ViewKey::Right,
            Key::Character(text) => {
                lowered = text.to_lowercase();
                ViewKey::Character(&lowered)
            }
            _ => return,
        };
        state.update(|model, _| model.key_up(key));
    };

    let renderer = ViewportRenderer::new(state);
    let invalidator = state.invalidator().unwrap_or_default();
    let scene = surface().class("wt-armor-viewport__scene").renderer_with_invalidator(renderer, &invalidator);

    view! {
        ContextMenu {
            ContextMenuTrigger(class = "wt-armor-viewport__trigger") {
                box(
                    class = SHEET,
                    node_ref = area,
                    attr:data-testid = "armor-viewport",
                    tabindex = {Focus::Programmatic},
                    on:pointer_down = on_down,
                    on:pointer_move = on_move,
                    on:pointer_up = on_up,
                    on:pointer_cancel = move |ev| {
                        ev.release_pointer();
                        state.update(|model, _| model.pointer_cancelled());
                    },
                    on:pointer_enter = move |ev| {
                        if let Some(at) = local(ev.position) {
                            state.update(|model, _| model.pointer_entered(at));
                        }
                    },
                    on:pointer_leave = move |_| {
                        state.update(|model, _| model.pointer_left());
                    },
                    on:wheel = move |ev| {
                        let notches = ev.delta.to_pixels(CssPx(WHEEL_LINE)).height.0;
                        if notches != 0.0 {
                            // A wheel turned away from the reader reports a negative delta, which
                            // brings the camera closer.
                            state.update(|model, cx| model.wheel(-notches, cx));
                            ev.prevent_default();
                        }
                    },
                    on:key_down = on_key_down,
                    on:key_up = on_key_up,
                    on:focus_out = move |_| {
                        state.update(|model, _| model.release_keys());
                    }
                ) {
                    {scene}
                    Show(when = move || state.read(|model| model.scene_is_shown()).unwrap_or(false)) {
                        box(class = "wt-armor-viewport__shade wt-armor-viewport__shade--left")
                        box(class = "wt-armor-viewport__shade wt-armor-viewport__shade--right")
                        box(class = "wt-armor-viewport__shade wt-armor-viewport__shade--top")
                        box(class = "wt-armor-viewport__shade wt-armor-viewport__shade--bottom")
                    }
                    {overlay.class("wt-armor-viewport__overlay")}
                    {move || stand_in(state, locale)}
                    {move || readouts(state, locale)}
                    {move || tooltips(state, locale)}
                    {move || export_confirm(state, locale, shell)}
                }
            }
            ContextMenuContent {
                {move || menu_items(state)}
            }
        }
    }
}

/// The overlay element, handed the scene to draw whenever the viewport changes.
fn overlay_element(state: ArmorViewportState) -> zgui::elements::Element<zgui::custom::Custom> {
    let fonts = use_context::<Fonts>().expect("every window provides its fonts");
    let (element, handle) = zgui::custom::custom(OverlayElement::new(fonts.shaper()));
    let handle: StoredValue<CustomHandle<OverlayElement>, LocalStorage> = StoredValue::new_local(handle);
    let effect = RenderEffect::new(move |_| {
        let Some(scene) = state.read_overlay(|model| model.overlay_scene(Instant::now())) else { return };
        handle.with_value(|handle| {
            let changed = handle.update(|element| {
                let changed = element.scene != scene;
                element.scene = scene;
                changed
            });
            if changed {
                handle.repaint();
            }
        });
    });
    on_cleanup_local(move || drop(effect));

    element
}

/// What stands where the scene would be: the subject loading, the device being prepared, why the
/// scene cannot be drawn, or the invitation to pick something.
#[derive(Clone, Debug, PartialEq)]
enum StandIn {
    Loading(String),
    Initializing,
    Problem { headline: String, explanation: String },
    Message(String),
}

fn stand_in(state: ArmorViewportState, locale: Locale) -> Option<AnyView> {
    locale.track();
    let said = state
        .read(|model| {
            let status = match &model.gpu {
                GpuState::Waiting => Some(t!("ui.armor.initializing").into_owned()),
                GpuState::Failed(reason) => Some(t!("ui.armor.init_failed", reason = reason).into_owned()),
                GpuState::Ready(_) => None,
            };
            // A load in flight replaces whatever is drawn, so the subject on screen is never
            // mistaken for the one being loaded.
            if let Some(name) = &model.ship_loading {
                return Some(StandIn::Loading(name.clone()));
            }
            if matches!(model.gpu, GpuState::Waiting) {
                return Some(StandIn::Initializing);
            }
            if let Some(problem) = model.scene_problem.as_ref().filter(|_| status.is_none()) {
                return Some(StandIn::Problem {
                    headline: problem.headline(),
                    explanation: problem.reason.explanation(),
                });
            }
            if model.current_armor.is_none() {
                return Some(StandIn::Message(status.unwrap_or_else(|| t!("ui.armor.select_ship").into_owned())));
            }
            status.map(StandIn::Message)
        })
        .flatten()?;

    Some(match said {
        StandIn::Loading(name) => {
            let label = t!("ui.armor.loading_named_ship", ship = name).into_owned();
            AnyView::new(view! {
                box(class = "wt-armor-viewport__said", attr:data-testid = "armor-viewport-loading") {
                    Spinner(label = label.clone())
                    text {{label}}
                }
            })
        }
        StandIn::Initializing => {
            let label = t!("ui.armor.initializing").into_owned();
            AnyView::new(view! {
                box(class = "wt-armor-viewport__said", attr:data-testid = "armor-viewport-initializing") {
                    Spinner(label = label.clone())
                    text {{label}}
                }
            })
        }
        StandIn::Problem { headline, explanation } => AnyView::new(view! {
            box(class = "wt-armor-viewport__said", attr:data-testid = "armor-viewport-problem") {
                text {{headline}}
                text(class = "wt-armor-viewport__dim") {{explanation}}
            }
        }),
        StandIn::Message(message) => AnyView::new(view! {
            box(class = "wt-armor-viewport__said", attr:data-testid = "armor-viewport-message") {
                text(class = "wt-armor-viewport__dim") {{message}}
            }
        }),
    })
}

/// The readouts over the hull: the replay's health and hits, its shot log, and the model's size.
fn readouts(state: ArmorViewportState, locale: Locale) -> Vec<AnyView> {
    locale.track();
    let Some((health, log, length)) = state.read(|model| {
        let log = if model.hits.is_empty() { Vec::new() } else { model.shot_log() };
        let length = model.length_readout().filter(|_| model.scene_is_shown());
        (model.hit_readout(), log, length)
    }) else {
        return Vec::new();
    };

    let mut views = Vec::new();
    if let Some(readout) = health {
        views.push(AnyView::new(view! {
            box(class = "wt-armor-viewport__readout wt-armor-viewport__readout--top", attr:data-testid = "armor-hit-readout") {
                text {{readout}}
            }
        }));
    }
    if !log.is_empty() {
        let lines: Vec<AnyView> = log
            .into_iter()
            .map(|line| {
                AnyView::new(view! {
                    row(class = "wt-armor-viewport__log-line") {
                        text(class = "wt-armor-viewport__dim") {{line.at}}
                        text {{line.outcome}}
                    }
                })
            })
            .collect();
        views.push(AnyView::new(view! {
            box(class = "wt-armor-viewport__readout wt-armor-viewport__readout--log", attr:data-testid = "armor-shot-log") {
                {lines}
            }
        }));
    }
    if let Some(readout) = length {
        views.push(AnyView::new(view! {
            box(class = "wt-armor-viewport__readout wt-armor-viewport__readout--bottom", attr:data-testid = "armor-length-readout") {
                text {{readout}}
            }
        }));
    }
    views
}

/// What a tooltip shows.
#[derive(Clone)]
enum Hover {
    /// A camera orbit, which takes the tooltip from the plate behind it.
    Ring(String),
    Plate {
        tooltip: ArmorTriangleTooltip,
        zone: String,
        material: String,
    },
    Mesh(MeshTooltip),
}

/// The tooltip for what the pointer rests on, anchored beside the pointer and kept inside the
/// viewport.
fn tooltips(state: ArmorViewportState, locale: Locale) -> Option<AnyView> {
    locale.track();
    let (hover, cursor, size) = state
        .read_overlay(|model| {
            let size = model.viewport_size();
            if let Some((label, cursor)) = model.ring_hover() {
                return Some((Hover::Ring(label), cursor, size));
            }
            if let Some((tooltip, cursor)) = model.plate_hover() {
                // A zone is named the way the game names it, as the egui tooltip does.
                let zone = match model.scene_source.as_ref() {
                    Some(source) => translate_name(source.assets().metadata(), &tooltip.zone),
                    None => tooltip.zone.clone(),
                };
                let material = model.material_label(tooltip.material);
                return Some((Hover::Plate { tooltip, zone, material }, cursor, size));
            }
            let (tooltip, cursor) = model.mesh_hover()?;
            Some((Hover::Mesh(tooltip), cursor, size))
        })
        .flatten()?;

    let (width, height) = size;
    let (horizontal, x) = if cursor.x > width * TOOLTIP_FLIP {
        ("right", width - cursor.x + TOOLTIP_CURSOR_OFFSET)
    } else {
        ("left", cursor.x + TOOLTIP_CURSOR_OFFSET)
    };
    let (vertical, y) = if cursor.y > height * TOOLTIP_FLIP {
        ("bottom", height - cursor.y + TOOLTIP_CURSOR_OFFSET)
    } else {
        ("top", cursor.y + TOOLTIP_CURSOR_OFFSET)
    };
    let placed = format!("{horizontal}: {x}px; {vertical}: {y}px;");

    Some(match hover {
        Hover::Ring(label) => {
            let lines: Vec<AnyView> = label
                .lines()
                .map(|line| AnyView::new(view! { text(class = "wt-armor-viewport__tooltip-text") {{line.to_owned()}} }))
                .collect();
            AnyView::new(view! {
                OverlaySurface(class = "wt-armor-viewport__tooltip", attr:style = placed, attr:data-testid = "armor-ring-tooltip") {
                    {lines}
                }
            })
        }
        Hover::Plate { tooltip, zone, material } => {
            let hint = t!("ui.armor.cast_here_hint", key = ModifierKey::Shift.label()).into_owned();
            plate_tooltip(&tooltip, &zone, &material, &hint, placed)
        }
        Hover::Mesh(tooltip) => {
            let mut lines = vec![
                AnyView::new(
                    view! { box(class = "wt-armor-viewport__tooltip-head") { text {{tooltip.name.clone()}} } },
                ),
                AnyView::new(view! { text(class = "wt-armor-viewport__tooltip-text") {{tooltip.group_label()}} }),
            ];
            for muted in [tooltip.model_label(), tooltip.hardpoint_label()].into_iter().flatten() {
                lines.push(AnyView::new(view! { text(class = "wt-armor-viewport__tooltip-muted") {{muted}} }));
            }
            AnyView::new(view! {
                OverlaySurface(class = "wt-armor-viewport__tooltip", attr:style = placed, attr:data-testid = "armor-mesh-tooltip") {
                    {lines}
                }
            })
        }
    })
}

/// A small filled colour swatch, `size` pixels square.
fn swatch(color: [f32; 4], size: f32) -> AnyView {
    // Truncated to bytes as the legend's swatches are.
    let byte = |channel: f32| (channel * 255.0) as u8;
    let alpha = f32::from(byte(color[3])) / 255.0;
    let style = format!(
        "width: {size}px; height: {size}px; background-color: rgba({}, {}, {}, {alpha});",
        byte(color[0]),
        byte(color[1]),
        byte(color[2])
    );
    AnyView::new(view! { box(class = "wt-armor-viewport__swatch", attr:style = style) })
}

/// The thickness tooltip, as the egui app's `show_armor_tooltip` lays it out: a swatch and the
/// thickness, the zone and material, the layers of the part when there are several, and what a
/// click there can do.
fn plate_tooltip(tooltip: &ArmorTriangleTooltip, zone: &str, material: &str, hint: &str, placed: String) -> AnyView {
    let mut lines = vec![
        AnyView::new(view! {
            box(class = "wt-armor-viewport__tooltip-head") {
                {swatch(tooltip.color, 12.0)}
                text {{format!("{:.0} mm", tooltip.thickness_mm)}}
            }
        }),
        AnyView::new(view! { text(class = "wt-armor-viewport__tooltip-text") {{format!("{zone} / {material}")}} }),
    ];
    if tooltip.layers.len() > 1 {
        lines.push(AnyView::new(view! { box(class = "wt-armor-viewport__tooltip-rule") }));
        lines.push(AnyView::new(view! {
            text(class = "wt-armor-viewport__tooltip-muted") {{format!("{} layers for this part:", tooltip.layers.len())}}
        }));
        for &layer_mm in &tooltip.layers {
            let this = ((layer_mm - tooltip.thickness_mm).abs() < 0.1).then(String::new);
            lines.push(AnyView::new(view! {
                box(class = "wt-armor-viewport__tooltip-row", attr:data-this = this) {
                    {swatch(thickness_to_color(layer_mm), 10.0)}
                    text {{format!("{layer_mm:.0} mm")}}
                }
            }));
        }
    }
    lines.push(AnyView::new(view! { text(class = "wt-armor-viewport__tooltip-muted") {{hint.to_owned()}} }));

    AnyView::new(view! {
        OverlaySurface(class = "wt-armor-viewport__tooltip", attr:style = placed, attr:data-testid = "armor-plate-tooltip") {
            {lines}
        }
    })
}

/// The export-confirm panel, over the viewport while it is open.
fn export_confirm(state: ArmorViewportState, locale: Locale, shell: Option<Shell>) -> Option<AnyView> {
    locale.track();
    let display_name =
        state.read(|model| model.export_confirm.as_ref().map(|confirm| confirm.display_name.clone())).flatten()?;
    let body = format!(
        "Export {display_name} to a glTF Binary (.glb) file. These 3D models and textures are IP of Wargaming and any usage of these models should be in compliance with your local laws."
    );

    Some(AnyView::new(view! {
        box(
            class = "wt-armor-viewport__backdrop",
            attr:data-testid = "armor-export-confirm",
            on:pointer_down = move |ev| ev.stop_propagation(),
            on:pointer_up = move |ev| ev.stop_propagation(),
            on:pointer_move = move |ev| ev.stop_propagation(),
            on:wheel = move |ev| ev.stop_propagation()
        ) {
            OverlaySurface(class = "wt-armor-viewport__confirm") {
                text(class = "wt-armor-viewport__confirm-title") {{t!("ui.armor.export_model").into_owned()}}
                text(class = "wt-armor-viewport__confirm-body") {{body}}
                row(class = "wt-armor-viewport__confirm-actions") {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        attr:data-testid = "armor-export-confirm-cancel",
                        on:click = move |_| state.cancel_export()
                    ) {
                        {t!("ui.buttons.cancel").into_owned()}
                    }
                    Button(
                        size = ButtonSize::Sm,
                        attr:data-testid = "armor-export-confirm-export",
                        on:click = move |_| {
                            if let Some(shell) = shell {
                                state.confirm_export(shell);
                            }
                        }
                    ) {
                        {t!("ui.armor.export_button").into_owned()}
                    }
                }
            }
        }
    }))
}

/// The context menu: hide or show the plate under the pointer, show every hidden plate, disable
/// the plate's material, then the pane's quick toggles.
fn menu_items(state: ArmorViewportState) -> Vec<AnyView> {
    let item = |label: String, test_id: &'static str, run: Box<dyn Fn()>| {
        AnyView::new(view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| run()), attr:data-testid = test_id) {
                {label}
            }
        })
    };
    let separator = || AnyView::new(view! { MenuSeparator() });
    let quick = state.pane_menu().map(|menu| menu.quick_toggles.run(())).unwrap_or_default();

    let hovered = state
        .read_untracked(|model| {
            let hover = model.hovered.clone()?;
            let label = model.material_label(hover.tooltip.material);
            let hidden = model.plate_visibility.get(&hover.key).copied().unwrap_or(false);
            Some((hover.key, label, hidden, model.plate_visibility.len()))
        })
        .flatten();
    let Some((key, label, hidden, hidden_count)) = hovered else { return quick };

    let PlateKey { zone, material_name, thickness_tenths } = key.clone();
    let thickness_mm = thickness_tenths as f32 / 10.0;
    let toggle_label = if hidden {
        format!("Show {thickness_mm:.0} mm {label}")
    } else {
        format!("Hide {thickness_mm:.0} mm {label}")
    };

    let mut items =
        vec![item(toggle_label, "armor-menu-toggle-plate", Box::new(move || state.toggle_plate(key.clone())))];
    if hidden_count > 0 {
        items.push(item(
            format!("Show all hidden plates ({hidden_count})"),
            "armor-menu-show-all",
            Box::new(move || state.show_all_plates()),
        ));
    }
    items.push(separator());
    items.push(item(
        format!("Disable {label}"),
        "armor-menu-disable-material",
        Box::new(move || state.disable_material(&zone, &material_name)),
    ));
    if !quick.is_empty() {
        items.push(separator());
        items.extend(quick);
    }
    items
}

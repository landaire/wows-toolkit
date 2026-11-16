//! The edit popup beside a single picked shape, as the egui renderer offers it: the shape's size,
//! ink and fill, a placed ship's side, and a button that erases it.

use landaire_ui::ColorPickerProps;
use landaire_ui::Decimals;
use landaire_ui::NumberInputProps;
use landaire_ui::NumberRange;
use landaire_ui::Rgb;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::annotate::ENEMY_TINT;
use wows_toolkit_replay_data::replay_render::annotate::FRIENDLY_TINT;
use wows_toolkit_replay_data::replay_render::annotate::PickedShape;
use wows_toolkit_replay_data::replay_render::annotate::ShapeEdit;
use wows_toolkit_replay_data::replay_render::annotate::SizeUnit;
use wows_toolkit_replay_data::replay_render::annotate::shape_color;
use wows_toolkit_replay_data::replay_render::annotate::shape_filled;
use wows_toolkit_replay_data::replay_render::annotate::shape_size;
use wt_collab_client::types::Annotation;
use zgui::component;
use zgui::css;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::windows::renderer::session::RendererSession;

const SHEET: &str = "wt-renderer-shape-edit";

const CSS: &str = css!(
    ".wt-renderer-shape-edit {
        position: absolute;
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 6px;
        border-radius: 6px;
        border: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-panel);
        font-size: 12px;
        pointer-events: auto;
    }
    .wt-renderer-shape-edit__field { display: flex; align-items: center; gap: 6px; }
    .wt-renderer-shape-edit__label { flex: 1; color: var(--lui-color-text-dim); }
    .wt-renderer-shape-edit__size { width: 104px; }
    .wt-renderer-shape-edit__delete.wt-renderer-shape-edit__delete { color: var(--lui-color-danger); }"
);

/// How far right of the shape the popup sits, in element pixels: the egui popup's gap.
const GAP: f32 = 8.0;

/// The popup, over the map beside the single picked shape. A control rather than part of the map,
/// so a press in it is kept from the map under it.
#[component]
pub fn ShapeEditPopup(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let picked = Memo::new(move |_| {
        // A peer's change can move, change or remove the picked shape.
        let _ = session.collab_frame.get();
        let origin = session.map_origin.get();
        session.annotator.with(|annotator| {
            session.map.with(|map| session.collab.with(|link| annotator.picked_shape(link, map, origin)))
        })
    });
    // The fields are built again only for another shape or another kind of shape, so a field being
    // typed in is not rebuilt under the reader as each edit lands.
    let kind = Memo::new(move |_| {
        picked.with(|picked| picked.as_ref().map(|picked| (picked.index, std::mem::discriminant(&picked.annotation))))
    });
    let fields = move || {
        kind.get()?;
        let annotation = picked.with_untracked(|picked| picked.as_ref().map(|picked| picked.annotation.clone()))?;

        Some(AnyView::new(view! { ShapeFields(session = session, picked = picked, annotation = annotation) }))
    };
    let left = move || picked.with(|picked| picked.as_ref().map(|picked| format!("{}px", picked.beside.x + GAP)));
    let top = move || picked.with(|picked| picked.as_ref().map(|picked| format!("{}px", picked.beside.y)));

    view! {
        Show(when = move || kind.with(Option::is_some)) {
            box(
                class = "wt-renderer-shape-edit",
                attr:data-testid = "replay-renderer-shape-edit",
                a11y:label = t!("ui.renderer.annotations.edit").into_owned(),
                style:left = left,
                style:top = top,
                on:pointer_down = move |ev| ev.stop_propagation(),
                on:pointer_up = move |ev| ev.stop_propagation(),
                on:click = move |ev| ev.stop_propagation(),
                on:double_click = move |ev| ev.stop_propagation(),
                on:wheel = move |ev| ev.stop_propagation()
            ) {
                {fields}
            }
        }
    }
}

/// The fields `annotation`'s kind of shape offers, each reading the picked shape as it now is.
#[component]
fn ShapeFields(session: RendererSession, picked: Memo<Option<PickedShape>>, annotation: Annotation) -> impl IntoView {
    let size = shape_size(&annotation).map(|_| {
        let space_size = session.track_untracked().map(|track| track.space_size).filter(|size| *size > 0.0);
        let unit = SizeUnit::for_shape(&annotation, space_size);
        AnyView::new(view! { SizeField(session = session, picked = picked, unit = unit) })
    });
    let color =
        shape_color(&annotation).map(|_| AnyView::new(view! { ColorField(session = session, picked = picked) }));
    let filled =
        shape_filled(&annotation).map(|_| AnyView::new(view! { FilledField(session = session, picked = picked) }));
    let side = matches!(annotation, Annotation::Ship { .. })
        .then(|| AnyView::new(view! { SideField(session = session, picked = picked) }));
    let delete = t!("ui.renderer.annotations.action_delete").into_owned();
    let label = delete.clone();

    view! {
        {size}
        {color}
        {filled}
        {side}
        row(class = "wt-renderer-shape-edit__field") {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        class = "wt-renderer-shape-edit__delete",
                        a11y:label = label.clone(),
                        attr:data-testid = "replay-renderer-shape-delete",
                        on:click = move |_| {
                            session.annotate(|annotator, link| annotator.erase_picked(link));
                        }
                    ) {
                        AppIcon(glyph = Glyph::Trash)
                    }
                }
                TooltipContent {{delete.clone()}}
            }
        }
    }
}

/// How big the shape is, in `unit`.
#[component]
fn SizeField(session: RendererSession, picked: Memo<Option<PickedShape>>, unit: SizeUnit) -> impl IntoView {
    let shown = Signal::derive_local(move || {
        let size = picked.with(|picked| picked.as_ref().and_then(|picked| shape_size(&picked.annotation)));
        // Nothing to show once the shape has gone, and the popup goes with it.
        size.map_or(0.0, |size| f64::from(unit.shown(size)))
    });
    let range = unit.range();
    let bounds =
        NumberRange::new(f64::from(unit.step())).with_min(f64::from(*range.start())).with_max(f64::from(*range.end()));
    let named = t!("ui.renderer.annotations.size").into_owned();

    view! {
        row(class = "wt-renderer-shape-edit__field") {
            text(class = "wt-renderer-shape-edit__label") {{named.clone()}}
            NumberInput(
                value = Binding::controlled(shown, move |next: f64| edit(session, ShapeEdit::Size(unit.size(next as f32)))),
                range = bounds,
                decimals = Decimals(unit.decimals()),
                label = named,
                class = "wt-renderer-shape-edit__size",
                attr:data-testid = "replay-renderer-shape-size"
            )
        }
    }
}

/// The ink the shape is drawn in. Opaque, as the egui picker is.
#[component]
fn ColorField(session: RendererSession, picked: Memo<Option<PickedShape>>) -> impl IntoView {
    let ink = Signal::derive_local(move || {
        let color = picked.with(|picked| picked.as_ref().and_then(|picked| shape_color(&picked.annotation)));
        // Nothing to show once the shape has gone, and the popup goes with it.
        let [red, green, blue, _] = color.unwrap_or_default();
        Rgb::from_channels(red, green, blue)
    });
    let named = t!("ui.renderer.annotations.color").into_owned();

    view! {
        row(class = "wt-renderer-shape-edit__field") {
            text(class = "wt-renderer-shape-edit__label") {{named.clone()}}
            ColorPicker(
                value = Binding::controlled(ink, move |chosen: Rgb| {
                    edit(session, ShapeEdit::Color([chosen.red(), chosen.green(), chosen.blue(), u8::MAX]));
                }),
                label = named,
                attr:data-testid = "replay-renderer-shape-color"
            )
        }
    }
}

/// Whether the shape is filled.
#[component]
fn FilledField(session: RendererSession, picked: Memo<Option<PickedShape>>) -> impl IntoView {
    let state = Signal::derive_local(move || {
        let filled = picked.with(|picked| picked.as_ref().and_then(|picked| shape_filled(&picked.annotation)));
        if filled == Some(true) { Checked::Yes } else { Checked::No }
    });
    let named = t!("ui.renderer.annotations.filled").into_owned();

    view! {
        row(class = "wt-renderer-shape-edit__field") {
            Checkbox(
                checked = Binding::controlled(state, move |next: Checked| edit(session, ShapeEdit::Filled(next.is_checked()))),
                a11y:label = named.clone(),
                attr:data-testid = "replay-renderer-shape-filled"
            )
            text(class = "wt-renderer-shape-edit__label") {{named}}
        }
    }
}

/// Which side a placed ship is on, tinted by it; a press moves it to the other.
#[component]
fn SideField(session: RendererSession, picked: Memo<Option<PickedShape>>) -> impl IntoView {
    let friendly = move || {
        picked.with(|picked| {
            matches!(picked, Some(PickedShape { annotation: Annotation::Ship { friendly: true, .. }, .. }))
        })
    };
    let tint = move || Some(format!("#{:06x}", if friendly() { FRIENDLY_TINT } else { ENEMY_TINT }));
    let named = move || {
        let key = if friendly() { "ui.tactics.side_friendly" } else { "ui.tactics.side_enemy" };
        t!(key).into_owned()
    };

    view! {
        row(class = "wt-renderer-shape-edit__field") {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "replay-renderer-shape-side",
                style:color = tint,
                on:click = move |_| edit(session, ShapeEdit::Friendly(!friendly()))
            ) {
                text {{named}}
            }
        }
    }
}

/// Changes the picked shape, kept for undo, and redraws.
fn edit(session: RendererSession, change: ShapeEdit) {
    session.annotate(|annotator, link| annotator.edit_picked(link, change));
}

//! What is drawn over the frame for the drawing tools and a collab session: the shortcut sheet,
//! peers' cursors and pings, and the rotation handle.
//!
//! Elements rather than part of the frame: none of it is part of the battle, it moves between
//! rasters, and it stays out of an exported video.

use rust_i18n::t;
use wows_toolkit_replay_data::replay_render::annotate::ANNOTATION_ACTIONS;
use wows_toolkit_replay_data::replay_render::annotate::HANDLE_RADIUS;
use wows_toolkit_replay_data::replay_render::annotate::TOOLS;
use wows_toolkit_replay_data::replay_render::collab::ping_ring;
use zgui::component;
use zgui::css;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;

use crate::windows::renderer::session::RendererSession;

const SHEET: &str = "wt-renderer-annotations";

const CSS: &str = css!(
    ".wt-renderer-annotations {
        position: absolute;
        left: 0;
        top: 0;
        right: 0;
        bottom: 0;
        pointer-events: none;
    }
    .wt-renderer-annotations__cursor {
        position: absolute;
        display: flex;
        align-items: center;
        gap: 4px;
        white-space: nowrap;
    }
    .wt-renderer-annotations__dot { flex: none; width: 8px; height: 8px; border-radius: 50%; }
    .wt-renderer-annotations__name { font-size: 12px; }
    .wt-renderer-annotations__ping { position: absolute; border-radius: 50%; border-width: 2px; border-style: solid; }
    .wt-renderer-annotations__handle { position: absolute; }
    .wt-renderer-annotations__knob { border-radius: 50%; background-color: #ffff64; }
    .wt-renderer-annotations__stem { position: absolute; width: 1px; background-color: #ffff64; }
    .wt-renderer-annotations__corner {
        position: absolute;
        left: 0;
        top: 0;
        right: 0;
        bottom: 0;
        display: flex;
        justify-content: flex-end;
        align-items: flex-end;
        padding: 8px;
    }
    .wt-renderer-annotations__sheet {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px;
        border-radius: 6px;
        border: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-panel);
        font-size: 12px;
    }
    .wt-renderer-annotations__heading { color: var(--lui-color-text-dim); }
    .wt-renderer-annotations__chord { display: flex; justify-content: space-between; gap: 8px; }
    .wt-renderer-annotations__does { color: var(--lui-color-text-dim); }"
);

/// Everything drawn over the frame, over the whole map element.
#[component]
pub fn AnnotationOverlay(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        box(class = "wt-renderer-annotations") {
            RotationHandle(session = session)
            PeerMarks(session = session)
            Show(when = move || session.ctrl_held.get()) {
                ShortcutSheet()
            }
        }
    }
}

/// The handle a single picked shape is turned by: a control rather than something drawn on the
/// map, so it keeps its size at every zoom.
#[component]
fn RotationHandle(session: RendererSession) -> impl IntoView {
    let handle = move || {
        // A peer's change can move or remove the picked shape.
        let _ = session.collab_frame.get();
        let origin = session.map_origin.get();
        let handle = session.annotator.with(|annotator| {
            session.map.with(|map| session.collab.with(|link| annotator.rotation_handle(link, map, origin)))
        })?;
        let diameter = format!("{}px", HANDLE_RADIUS * 2.0);

        Some(view! {
            box(
                class = "wt-renderer-annotations__handle",
                attr:data-testid = "replay-renderer-rotation-handle",
                style:left = format!("{}px", handle.handle.x - HANDLE_RADIUS),
                style:top = format!("{}px", handle.handle.y - HANDLE_RADIUS)
            ) {
                box(class = "wt-renderer-annotations__knob", style:width = diameter.clone(), style:height = diameter)
                // The stem back to the shape, so the handle reads as belonging to it.
                box(
                    class = "wt-renderer-annotations__stem",
                    style:left = format!("{HANDLE_RADIUS}px"),
                    style:top = format!("{HANDLE_RADIUS}px"),
                    style:height = format!("{}px", handle.anchor.y - handle.handle.y)
                )
            }
        })
    };

    view! {
        box {
            {handle}
        }
    }
}

/// Where the peers' pointers are and the pings they dropped, read again each time a running
/// session redraws.
#[component]
fn PeerMarks(session: RendererSession) -> impl IntoView {
    let marks = move || {
        let _ = session.collab_frame.get();
        let origin = session.map_origin.get();
        let (cursors, pings) = session.collab.with(|link| (link.peer_cursors(), link.peer_pings()));

        session.map.with(|map| {
            let mut marks: Vec<AnyView> = Vec::new();
            for cursor in cursors {
                let Some([x, y]) = cursor.pos else { continue };
                let Some(at) = map.element_point((x, y), origin) else { continue };
                let [red, green, blue] = cursor.color;
                let color = format!("#{red:02x}{green:02x}{blue:02x}");
                let name = cursor.name;
                marks.push(AnyView::new(view! {
                    row(
                        class = "wt-renderer-annotations__cursor",
                        attr:data-testid = format!("replay-renderer-peer-cursor-{}", cursor.user_id),
                        style:left = format!("{}px", at.x),
                        style:top = format!("{}px", at.y)
                    ) {
                        box(class = "wt-renderer-annotations__dot", style:background-color = color.clone())
                        text(class = "wt-renderer-annotations__name", style:color = color) {{name}}
                    }
                }));
            }
            for ping in pings {
                let Some(at) = map.element_point((ping.pos[0], ping.pos[1]), origin) else { continue };
                let Some(ring) = ping_ring(ping.time.elapsed()) else { continue };
                let [red, green, blue] = ping.color;
                let alpha = (ring.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                let diameter = format!("{}px", ring.radius * 2.0);
                marks.push(AnyView::new(view! {
                    box(
                        class = "wt-renderer-annotations__ping",
                        attr:data-testid = format!("replay-renderer-peer-ping-{}", ping.user_id),
                        style:left = format!("{}px", at.x - ring.radius),
                        style:top = format!("{}px", at.y - ring.radius),
                        style:width = diameter.clone(),
                        style:height = diameter,
                        style:border-color = format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
                    )
                }));
            }

            marks
        })
    };

    view! {
        box {
            {marks}
        }
    }
}

/// What the keys do, in the corner while the primary modifier is held. None of the chords appear
/// anywhere else, so this is how a reader finds them.
#[component]
fn ShortcutSheet() -> impl IntoView {
    let chord = |chord: &'static str, does: String| {
        AnyView::new(view! {
            row(class = "wt-renderer-annotations__chord") {
                text {{chord}}
                text(class = "wt-renderer-annotations__does") {{does}}
            }
        })
    };
    let tools: Vec<AnyView> = TOOLS
        .iter()
        .filter_map(|button| button.chord.map(|keys| chord(keys, t!(button.label_key).into_owned())))
        .collect();
    let actions: Vec<AnyView> =
        ANNOTATION_ACTIONS.iter().map(|(keys, label_key)| chord(keys, t!(*label_key).into_owned())).collect();

    view! {
        box(class = "wt-renderer-annotations__corner") {
            column(class = "wt-renderer-annotations__sheet", attr:data-testid = "replay-renderer-shortcuts") {
                text {{t!("ui.renderer.annotations.shortcuts_title").into_owned()}}
                text(class = "wt-renderer-annotations__heading") {{t!("ui.renderer.annotations.shortcuts_tools").into_owned()}}
                {tools}
                text(class = "wt-renderer-annotations__heading") {{t!("ui.renderer.annotations.shortcuts_actions").into_owned()}}
                {actions}
            }
        }
    }
}

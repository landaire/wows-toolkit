//! The renderer's view: the map, what is drawn over it, and the transport under it.

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_minimap_renderer::viewport::MAX_ZOOM;
use wows_minimap_renderer::viewport::MIN_ZOOM;
use wows_minimap_renderer::viewport::MapViewport;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::export::ExportTarget;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::playback::SEEK_STEP;
use wows_toolkit_replay_data::replay_render::playback::SPEEDS;
use wows_toolkit_replay_data::replay_render::playback::speed_index;
use wows_toolkit_replay_data::replay_render::playback::speed_label;
use wows_toolkit_replay_data::replay_render::track::FrameIndex;
use wows_toolkit_replay_data::replay_render::view::ElementBounds;
use wows_toolkit_replay_data::replay_render::view::WindowPoint;
use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ProgressProps;
use zgui_ui::SelectContentProps;
use zgui_ui::SelectItemProps;
use zgui_ui::SelectProps;
use zgui_ui::SelectTriggerProps;
use zgui_ui::SelectValueProps;
use zgui_ui::SliderProps;
use zgui_ui::SpinnerProps;

use crate::frames::FrameViewProps;
use crate::icons::AppIconProps;
use crate::shell::tabs::ModeHint;
use crate::shell::tabs::ModifierKey;
use crate::windows::renderer::export::ExportNoticeProps;
use crate::windows::renderer::export::export_asking_first;
use crate::windows::renderer::hover::HoverReadingsProps;
use crate::windows::renderer::options::OptionsPopoverProps;
use crate::windows::renderer::overlays::AnnotationOverlayProps;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::session::Status;
use crate::windows::renderer::ship_menu::ShipMenuViewProps;
use crate::windows::renderer::timeline::TimelinePopoverProps;
use crate::windows::renderer::tools::ToolsPopoverProps;

const SHEET: &str = "wt-renderer";

const CSS: &str = css!(
    ".wt-renderer {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
    }
    .wt-renderer__body { position: relative; display: flex; flex: 1; min-height: 0; }
    .wt-renderer__map { position: relative; flex: 1; min-width: 0; min-height: 0; }
    .wt-renderer__map[data-zoomed] { cursor: grab; }
    .wt-renderer__frame { position: absolute; left: 0; top: 0; right: 0; bottom: 0; }
    .wt-renderer__state {
        display: flex;
        flex-direction: column;
        flex: 1;
        align-items: center;
        justify-content: center;
        gap: 4px;
        font-size: 14px;
    }
    .wt-renderer__state-title { font-weight: 700; }
    .wt-renderer__state-reason { color: var(--wt-color-text-dim); }
    .wt-renderer__export {
        position: absolute;
        top: 8px;
        left: 0;
        right: 0;
        display: flex;
        justify-content: center;
        pointer-events: none;
    }
    .wt-renderer__export-body {
        display: flex;
        flex-direction: column;
        gap: 4px;
        width: 300px;
        padding: 8px 12px;
        border-radius: var(--lui-radius);
        border: 1px solid var(--lui-color-border);
        background-color: color-mix(in srgb, var(--lui-color-surface) 85%, transparent);
        font-size: 12px;
    }
    .wt-renderer__export-waiting { display: flex; align-items: center; gap: 8px; }
    .wt-renderer__transport {
        display: flex;
        flex-direction: column;
        flex: none;
        background-color: var(--lui-color-surface);
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-renderer__failure { padding: 4px 8px 0 8px; font-size: 12px; color: var(--wt-color-error); }
    .wt-renderer__scrubber { flex: none; padding: 4px 13px 0 13px; }
    .wt-renderer__track { position: relative; }
    .wt-renderer__mark {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 1.5px;
        background-color: var(--lui-color-text-dim);
        pointer-events: none;
    }
    .wt-renderer__controls {
        display: flex;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 0 8px 4px 8px;
    }
    .wt-renderer__clock { flex: none; width: 88px; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-renderer__spacer { flex: 1; min-width: 0; }
    .wt-renderer__zoom { flex: none; width: 90px; }
    .wt-renderer__speed { flex: none; width: 72px; }
    .wt-renderer__rule { flex: none; width: 1px; align-self: stretch; margin: 4px 0; background-color: var(--lui-color-border); }"
);

/// How much one line of wheel travel is worth, for a wheel that reports lines rather than pixels.
const SCROLL_LINE: f32 = 20.0;

/// What a docked renderer names on the status bar: the shortcut sheet its key shows while held.
pub const SHORTCUTS_MODE: ModeHint =
    ModeHint { key: ModifierKey::Primary, name_key: "ui.renderer.annotations.shortcuts_title" };

/// A replay played back on its minimap, with its transport. `slot_tools` and the other slots are
/// the transport's popovers, placed before the pop-out control.
#[component]
pub fn ReplayRenderer(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let map = NodeRef::new();
    let root = NodeRef::new();

    // The frame's placement follows the element's size and the frame's. Observed once: each call
    // makes another observer.
    let border = map.observe_border_box();
    let placing = RenderEffect::new(move |_| {
        let _ = session.frame_size.get();
        let _ = border.get();
        place(session, map);
    });
    on_cleanup_local(move || drop(placing));

    let body = move || match session.status.get() {
        Status::Baking => AnyView::new(view! {
            column(class = "wt-renderer__state", attr:data-testid = "replay-renderer-baking") {
                Spinner(label = t!("ui.replay.renderer.baking").into_owned())
                text {{t!("ui.replay.renderer.baking").into_owned()}}
            }
        }),
        Status::Failed(reason) => AnyView::new(view! {
            column(class = "wt-renderer__state", attr:data-testid = "replay-renderer-failed") {
                text(class = "wt-renderer__state-title") {{t!("ui.replay.renderer.failed").into_owned()}}
                text(class = "wt-renderer__state-reason") {{reason}}
            }
        }),
        Status::Ready(_) => AnyView::new(view! { MapArea(session = session, map = map, root = root) }),
    };

    view! {
        column(
            class = "wt-renderer",
            node_ref = root,
            attr:data-testid = "replay-renderer",
            tabindex = {Focus::Programmatic},
            on:key_down = move |ev| {
                session.ctrl_held.set(ev.modifiers.primary());
                if session.key(&ev.key, &ev.key_without_modifiers, ev.modifiers) {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            },
            on:key_up = move |ev| session.ctrl_held.set(ev.modifiers.primary()),
            on:focus_out = move |_| session.ctrl_held.set(false)
        ) {
            row(class = "wt-renderer__body") {
                {body}
            }
            Transport(session = session)
            ShipMenuView(session = session)
            ExportNotice(session = session)
        }
    }
}

/// Records where the map element sits, in the window's CSS pixels.
fn place(session: RendererSession, map: NodeRef) {
    let Some(rect) = map.window_bounds() else { return };
    let scale = map.scale();
    session.place(ElementBounds {
        origin: WindowPoint { x: rect.origin.x.0 / scale, y: rect.origin.y.0 / scale },
        width: rect.size.width.0 / scale,
        height: rect.size.height.0 / scale,
    });
}

pub(super) fn window_point(position: zgui::geom::Point<zgui::geom::CssPx, zgui::geom::Css>) -> WindowPoint {
    WindowPoint { x: position.x.0, y: position.y.0 }
}

/// The map, the frame on it, and what is drawn over the frame.
#[component]
fn MapArea(session: RendererSession, map: NodeRef, root: NodeRef) -> impl IntoView {
    let sink = session.sink();
    let pressed = StoredValue::new_local(false);
    // The frame playback is on, which a view built again after a pop-out shows without moving.
    session.draw();
    let zoomed = move || session.map.with(|map| !map.view().is_whole_map()).then(|| "true".to_owned());

    view! {
        box(
            class = "wt-renderer__map",
            node_ref = map,
            attr:data-testid = "replay-renderer-viewport",
            attr:data-zoomed = zoomed,
            on:pointer_down = move |ev| {
                place(session, map);
                let position = window_point(ev.position);
                match ev.button {
                    // Not captured: a captured release is not over the pressed element, so no click
                    // would follow it. Leaving the map ends the press instead.
                    Some(PointerButton::Primary) => {
                        pressed.set_value(true);
                        root.focus();
                        session.pointer_down(position);
                    }
                    Some(PointerButton::Middle) => session.ping(position),
                    Some(PointerButton::Secondary) => session.open_ship_menu(position),
                    _ => {}
                }
            },
            on:pointer_move = move |ev| {
                let held = pressed.get_value();
                session.pointer_move(window_point(ev.position), held, ev.modifiers.shift());
            },
            on:pointer_up = move |ev| {
                if ev.button == Some(PointerButton::Primary) {
                    pressed.set_value(false);
                    session.pointer_up(window_point(ev.position));
                }
            },
            on:pointer_cancel = move |ev| {
                pressed.set_value(false);
                session.pointer_up(window_point(ev.position));
            },
            on:pointer_leave = move |ev| {
                if pressed.get_value() {
                    pressed.set_value(false);
                    session.pointer_up(window_point(ev.position));
                }
                session.pointer_left();
            },
            on:click = move |ev| session.click(window_point(ev.position), ev.modifiers.shift(), 1),
            on:double_click = move |ev| session.click(window_point(ev.position), ev.modifiers.shift(), 2),
            on:context_menu = move |ev| ev.prevent_default(),
            on:wheel = move |ev| {
                place(session, map);
                let delta = ev.delta.to_pixels(zgui::geom::CssPx(SCROLL_LINE)).height.0;
                // A wheel turned away from the reader scrolls down, which zooms out.
                session.wheel(window_point(ev.position), -delta);
                ev.prevent_default();
            }
        ) {
            FrameView(sink = sink, class = "wt-renderer__frame")
            ExportOverlay(session = session)
            AnnotationOverlay(session = session)
            HoverReadings(session = session)
        }
    }
}

/// How far an export has got, over the video: the transport's controls all refuse while one runs,
/// and a number among them is not where a reader looks.
#[component]
fn ExportOverlay(session: RendererSession) -> impl IntoView {
    let progress = move || session.export.with(|export| export.progress);
    let body = move || {
        let progress = progress()?;
        Some(match progress.fraction() {
            None => AnyView::new(view! {
                row(class = "wt-renderer__export-waiting") {
                    Spinner(label = t!("ui.renderer.preparing_export").into_owned())
                    text {{t!("ui.renderer.preparing_export").into_owned()}}
                }
            }),
            Some(done) => AnyView::new(view! {
                column {
                    text {{format!("{} ({}/{})", t!(progress.stage.label_key()), progress.done, progress.total)}}
                    Progress(value = Signal::stored_local(Some(f64::from(done) * 100.0)))
                }
            }),
        })
    };

    view! {
        Show(when = move || progress().is_some()) {
            box(class = "wt-renderer__export", attr:data-testid = "replay-renderer-export-overlay") {
                column(class = "wt-renderer__export-body") {
                    {body}
                }
            }
        }
    }
}

/// One icon button on the transport, named by its tooltip.
#[component]
pub(super) fn TransportButton(
    glyph: Glyph,
    label_key: &'static str,
    test_id: &'static str,
    #[prop(into)] disabled: Signal<bool, LocalStorage>,
    on_press: UnsyncCallback<()>,
) -> impl IntoView {
    let label = t!(label_key).into_owned();
    let named = label.clone();
    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    disabled = disabled,
                    attr:data-testid = test_id,
                    a11y:label = label.clone(),
                    on:click = move |_| zgui::reactive::Callable::run(&on_press, ())
                ) {
                    AppIcon(glyph = glyph)
                }
            }
            TooltipContent {{named.clone()}}
        }
    }
}

/// Play or pause, whichever playback is not doing.
#[component]
fn PlayButton(session: RendererSession, refused: Signal<bool, LocalStorage>) -> impl IntoView {
    let playing = move || session.playback.with(Playback::is_playing);
    move || {
        let (glyph, label_key) = if playing() {
            (Glyph::Pause, "ui.replay.renderer.pause")
        } else {
            (Glyph::Play, "ui.replay.renderer.play")
        };
        view! {
            TransportButton(glyph = glyph, label_key = label_key, test_id = "replay-renderer-play",
                disabled = refused, on_press = UnsyncCallback::new(move |()| session.toggle_playing()))
        }
    }
}

/// The scrubber, and the row of controls under it.
#[component]
fn Transport(session: RendererSession) -> impl IntoView {
    let ready = move || session.is_ready() && session.export.with(|export| export.progress.is_none());
    let refused = Signal::derive_local(move || !ready());
    let press = move |act: fn(RendererSession)| UnsyncCallback::new(move |()| act(session));
    let last = move || session.track().map_or(0, |track| track.last().get());
    let has_previous = move || session.previous_event_at().is_some();
    let has_next = move || session.next_event_at().is_some();
    let clock = move || {
        let track = session.track()?;
        Some(session.playback.with(|playback| playback.clock_and_length(&track)))
    };
    let at = Signal::derive_local(move || session.playback.with(|playback| playback.at().get() as f64));
    let zoom = Signal::derive_local(move || f64::from(session.map.with(|map| map.view().zoom())));
    let whole_map = move || session.map.with(|map| map.view().is_whole_map());
    let speed = move || speed_index(session.playback.with(Playback::speed)).to_string();
    let scrubber = move || {
        // The range is fixed when a slider is built, so it is built again over the baked track.
        let last = last();
        view! {
            Slider(
                value = zgui_ui_primitives::Binding::controlled(at, move |value: f64| session.seek_to_slider(value as f32)),
                min = 0.0,
                max = last.max(1) as f64,
                step = 1.0,
                disabled = refused
            )
        }
    };

    view! {
        column(class = "wt-renderer__transport", attr:data-testid = "replay-renderer-transport") {
            Show(when = move || session.export.with(|export| export.failure.is_some())) {
                text(class = "wt-renderer__failure", attr:data-testid = "replay-renderer-export-failure") {
                    {move || session.export.with(|export| export.failure.clone().unwrap_or_default())}
                }
            }
            box(class = "wt-renderer__scrubber", attr:data-testid = "replay-renderer-scrubber") {
                // The marks are placed along the slider's width, inside the scrubber's inset.
                box(class = "wt-renderer__track") {
                    {scrubber}
                    for mark in move || session.track().map(|track| track.battle_marks()).unwrap_or_default(),
                        key = |mark: &f32| mark.to_bits() {
                        box(class = "wt-renderer__mark", style:left = format!("calc({}% - 0.75px)", mark * 100.0))
                    }
                }
            }
            row(class = "wt-renderer__controls") {
                TransportButton(glyph = Glyph::SkipBack, label_key = "ui.renderer.controls.jump_to_start",
                    test_id = "replay-renderer-jump-to-start", disabled = refused,
                    on_press = press(|session| session.go_to(FrameIndex::FIRST)))
                TransportButton(glyph = Glyph::Rewind, label_key = "ui.renderer.controls.previous_event",
                    test_id = "replay-renderer-previous-event",
                    disabled = Signal::derive_local(move || !ready() || !has_previous()),
                    on_press = press(RendererSession::jump_to_previous_event))
                TransportButton(glyph = Glyph::RotateCcw, label_key = "ui.renderer.controls.back_10s",
                    test_id = "replay-renderer-back-10s", disabled = refused,
                    on_press = press(|session| session.seek_by(-SEEK_STEP)))
                PlayButton(session = session, refused = refused)
                TransportButton(glyph = Glyph::RotateCw, label_key = "ui.renderer.controls.forward_10s",
                    test_id = "replay-renderer-forward-10s", disabled = refused,
                    on_press = press(|session| session.seek_by(SEEK_STEP)))
                TransportButton(glyph = Glyph::FastForward, label_key = "ui.renderer.controls.next_event",
                    test_id = "replay-renderer-next-event",
                    disabled = Signal::derive_local(move || !ready() || !has_next()),
                    on_press = press(RendererSession::jump_to_next_event))
                TransportButton(glyph = Glyph::SkipForward, label_key = "ui.renderer.controls.jump_to_end",
                    test_id = "replay-renderer-jump-to-end", disabled = refused,
                    on_press = press(|session| {
                        if let Some(track) = session.track_untracked() {
                            session.go_to(track.last());
                        }
                    }))
                text(class = "wt-renderer__clock", attr:data-testid = "replay-renderer-clock") {{clock}}
                box(class = "wt-renderer__spacer")
                AppIcon(glyph = Glyph::Search)
                box(class = "wt-renderer__zoom") {
                    Slider(
                        value = zgui_ui_primitives::Binding::controlled(zoom, move |value: f64| session.zoom_to(value as f32)),
                        min = f64::from(MIN_ZOOM),
                        max = f64::from(MAX_ZOOM),
                        step = 0.1,
                        disabled = refused
                    )
                }
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || !ready() || whole_map()),
                    attr:data-testid = "replay-renderer-zoom-reset",
                    on:click = move |_| session.set_view(MapViewport::default())
                ) {
                    {t!("ui.buttons.reset").into_owned()}
                }
                box(class = "wt-renderer__rule")
                box(class = "wt-renderer__speed", attr:data-testid = "replay-renderer-speed") {
                    Select(
                        value = zgui_ui_primitives::Binding::controlled(Signal::derive_local(speed), move |index: String| {
                            if let Some(speed) = index.parse::<usize>().ok().and_then(|index| SPEEDS.get(index)) {
                                session.set_speed(*speed);
                            }
                        })
                    ) {
                        SelectTrigger(a11y:label = t!("ui.renderer.controls.speed").into_owned()) {
                            SelectValue()
                        }
                        SelectContent {
                            for index in move || (0..SPEEDS.len()).collect::<Vec<_>>(), key = |index: &usize| *index {
                                SelectItem(value = index.to_string()) {{speed_label(SPEEDS[index])}}
                            }
                        }
                    }
                }
                box(class = "wt-renderer__rule")
                TransportButton(glyph = Glyph::Download, label_key = "ui.replay.renderer.export_video",
                    test_id = "replay-renderer-export", disabled = refused,
                    on_press = UnsyncCallback::new(move |()| export_asking_first(session, ExportTarget::File)))
                ToolsPopover(session = session)
                OptionsPopover(session = session)
                TimelinePopover(session = session)
                Show(when = move || !session.popped_out.get()) {
                    TransportButton(glyph = Glyph::SquareArrowOutUpRight, label_key = "ui.replay.renderer.pop_out",
                        test_id = "replay-renderer-pop-out", disabled = Signal::stored_local(false),
                        on_press = UnsyncCallback::new(move |()| {
                            crate::windows::renderer::pop_out(session);
                        }))
                }
                TransportButton(glyph = Glyph::Clipboard, label_key = "ui.replay.renderer.export_clipboard",
                    test_id = "replay-renderer-clipboard", disabled = refused,
                    on_press = UnsyncCallback::new(move |()| export_asking_first(session, ExportTarget::Clipboard)))
            }
        }
    }
}

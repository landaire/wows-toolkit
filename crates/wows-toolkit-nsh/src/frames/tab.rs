use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use nsh_core::element::Image;
use nsh_core::element::ResponderEvent;
use nsh_core::element::TouchClaim;
use nsh_core::prelude::*;
use tokio::sync::watch;

use crate::frames::FRAME_SIDE;
use crate::frames::FrameMode;
use crate::frames::InputKind;
use crate::frames::UriBuffering;
use crate::frames::UriFrame;
use crate::frames::hub;
use crate::ui;

/// How producer frames reach the engine thread, from `WTN_DELIVERY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    /// A task awaiting the producer's watch channel.
    Channel,
    /// An 8 ms `set_interval` looking at the same channel.
    Poll,
}

impl Delivery {
    fn from_env() -> Self {
        match std::env::var("WTN_DELIVERY").as_deref() {
            Ok("poll") => Delivery::Poll,
            // Unset or anything else: the push path, which the spike recommends.
            _ => Delivery::Channel,
        }
    }
}

pub fn frames_tab(initial: FrameMode) -> impl IntoElement {
    let mode = signal(initial);
    let uri = signal(None::<UriFrame>);
    let report = signal(hub().report());

    hub().set_mode(initial);
    hub().set_active(true);
    cleanup(|| hub().set_active(false));

    deliver_frames(Delivery::from_env(), hub().subscribe(), uri);
    set_interval(Duration::from_secs(1), move || report.set(hub().report()));

    let last_drag = signal((0.0f32, 0.0f32));
    let side = px(FRAME_SIDE as f32);

    view()
        .flex_1()
        .gap(px(8.0))
        .p(px(8.0))
        .child(view().flex_row().gap(px(8.0)).items_center().children(FrameMode::ALL.map(|choice| {
            ui::button(
                choice.label(),
                move || mode.get() == choice,
                move || {
                    hub().set_mode(choice);
                    mode.set(choice);
                },
            )
        })))
        .child(text(move || report.get()).text_size(px(12.0)).font_family("monospace"))
        .child(
            view()
                .w(side)
                .h(side)
                .bg(rgb(0x000000))
                .claim_touches(TouchClaim::on_start())
                .on_responder_grant(move |_: &ResponderEvent| last_drag.set((0.0, 0.0)))
                .on_responder_move(move |event: &ResponderEvent| {
                    let (lx, ly) = last_drag.get_untracked();
                    let (dx, dy) = (event.gesture.delta.x.0, event.gesture.delta.y.0);
                    last_drag.set((dx, dy));
                    hub().pan_by(dx - lx, dy - ly);
                    hub().stats.record_input(InputKind::NshResponderMove);
                })
                .on_mouse_move(move |_: &MouseEvent| hub().stats.record_input(InputKind::NshMouseMove))
                .child(dynamic(move || match mode.get() {
                    FrameMode::Native => native_frame().into_element(),
                    FrameMode::Image(_, UriBuffering::Single) => uri_frame(uri).into_element(),
                    FrameMode::Image(_, UriBuffering::Swap) => swapped_uri_frame(uri).into_element(),
                })),
        )
}

/// Moves each new producer frame into `uri` on the engine thread, whose
/// reactive runtime the producer thread cannot write.
fn deliver_frames(delivery: Delivery, receiver: watch::Receiver<Option<UriFrame>>, uri: Signal<Option<UriFrame>>) {
    let last_seq = Rc::new(Cell::new(0u64));
    let accept = move |frame: Option<UriFrame>| {
        let Some(frame) = frame else {
            return;
        };

        let previous = last_seq.replace(frame.seq.0);
        if previous != 0 && frame.seq.0 > previous + 1 {
            hub().stats.record_skipped(frame.seq.0 - previous - 1);
        }
        uri.set(Some(frame));
    };

    match delivery {
        Delivery::Channel => {
            let mut receiver = receiver;
            spawn(async move {
                // The hub's sender is static, so `changed` only ends when the task is cancelled.
                while receiver.changed().await.is_ok() {
                    let frame = receiver.borrow_and_update().clone();
                    accept(frame);
                }
            });
        }
        Delivery::Poll => {
            let receiver = RefCell::new(receiver);
            set_interval(Duration::from_millis(8), move || {
                let mut receiver = receiver.borrow_mut();
                if matches!(receiver.has_changed(), Ok(true)) {
                    let frame = receiver.borrow_and_update().clone();
                    accept(frame);
                }
            });
        }
    }
}

#[cfg(target_os = "linux")]
fn native_frame() -> impl IntoElement {
    native(crate::frames::native::COMPONENT)
        .on_native_event(|event: &NativeEvent| {
            log::info!(target: "frames", "native event {} reached the engine: {}", event.name, event.data);
        })
        .flex_1()
}

#[cfg(not(target_os = "linux"))]
fn native_frame() -> impl IntoElement {
    text("native frames are implemented for Linux only")
}

fn uri_frame(uri: Signal<Option<UriFrame>>) -> impl IntoElement {
    when(move || uri.with(Option::is_some), move || uri_image(uri, || {}).flex_1())
}

/// Two stacked images: the hidden one takes each new source and comes to the
/// front once loaded. A single `image()` drops its picture when the source
/// changes and shows nothing until the new one decodes.
fn swapped_uri_frame(uri: Signal<Option<UriFrame>>) -> impl IntoElement {
    let slots = [signal(None::<UriFrame>), signal(None::<UriFrame>)];
    let front = signal(0usize);

    effect(move || {
        if let Some(frame) = uri.get() {
            slots[1 - front.get_untracked()].set(Some(frame));
        }
    });

    view().flex_1().children([0usize, 1].map(|slot| {
        when(
            move || slots[slot].with(Option::is_some),
            move || {
                uri_image(slots[slot], move || front.set(slot))
                    .absolute()
                    .inset_0()
                    .opacity(move || if front.get() == slot { 1.0_f32 } else { 0.0_f32 })
            },
        )
    }))
}

/// An `image()` of the frame in `uri`; `loaded` runs after the stats and
/// file cleanup on each load event.
fn uri_image(uri: Signal<Option<UriFrame>>, loaded: impl Fn() + 'static) -> Image {
    let load_start = Rc::new(Cell::new(None::<Instant>));
    let start_cell = Rc::clone(&load_start);

    // Shown only under a `when` that checked the slot holds a frame.
    image(move || {
        uri.with(|frame| frame.as_ref().map(|frame| SharedString::from(Arc::clone(&frame.uri)))).unwrap_or_default()
    })
    .on_load_start(move || start_cell.set(Some(Instant::now())))
    .on_load(move |_: &Size<DevicePixels>| {
        if let Some(start) = load_start.take() {
            hub().stats.record_load(start.elapsed());
        }
        hub().stats.record_display(None);
        if let Some(seq) = uri.with_untracked(|frame| frame.as_ref().map(|frame| frame.seq)) {
            hub().frame_shown(seq);
        }
        loaded();
    })
    .on_error(|message: &str| {
        hub().stats.record_display_error();
        log::warn!(target: "frames", "frame image failed: {message}");
    })
}

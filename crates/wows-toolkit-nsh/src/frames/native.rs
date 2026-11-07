use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use nsh_core::JsonValue;
use nsh_platform_linux::ComponentContext;
use nsh_platform_linux::Components;
use nsh_platform_linux::NativeComponent;
use nsh_platform_linux::gtk4 as gtk;
use serde_json::json;

use crate::frames::FRAME_SIDE;
use crate::frames::InputKind;
use crate::frames::hub;

pub const COMPONENT: &str = "wows.frame";

pub fn register(components: &Components) {
    components.register(
        COMPONENT,
        Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> { Box::new(FrameView::new(context)) }),
    );
}

/// Shows the producer's latest RGBA frame, polled on the widget's frame clock.
struct FrameView {
    picture: gtk::Picture,
}

impl FrameView {
    fn new(context: ComponentContext) -> Self {
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);

        picture.add_tick_callback(|picture, _clock| {
            let Some(frame) = hub().take_rgba() else {
                return glib::ControlFlow::Continue;
            };

            let start = Instant::now();
            let bytes = glib::Bytes::from_owned(frame.pixels);
            let texture = gdk::MemoryTexture::new(
                FRAME_SIDE as i32,
                FRAME_SIDE as i32,
                gdk::MemoryFormat::R8g8b8a8Premultiplied,
                &bytes,
                FRAME_SIDE as usize * 4,
            );
            picture.set_paintable(Some(&texture));
            hub().stats.record_display(Some(start.elapsed()));

            glib::ControlFlow::Continue
        });

        // GestureDrag offsets are cumulative; pan by the step since the last update.
        let drag = gtk::GestureDrag::new();
        let last = Rc::new(Cell::new((0.0f64, 0.0f64)));
        let begin_last = Rc::clone(&last);
        drag.connect_drag_begin(move |_, x, y| {
            log::info!(target: "frames", "gtk drag begin at {x:.0},{y:.0}");
            begin_last.set((0.0, 0.0));
        });
        drag.connect_drag_update(move |_, dx, dy| {
            let (px, py) = last.replace((dx, dy));
            hub().pan_by((dx - px) as f32, (dy - py) as f32);
            hub().stats.record_input(InputKind::GtkDragUpdate);
        });
        picture.add_controller(drag);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let wheel_context = context.clone();
        scroll.connect_scroll(move |_, dx, dy| {
            log::info!(target: "frames", "gtk scroll dx={dx:.2} dy={dy:.2}");
            hub().stats.record_input(InputKind::GtkScroll);
            wheel_context.emit("wheel", json!({ "dx": dx, "dy": dy }));
            glib::Propagation::Stop
        });
        picture.add_controller(scroll);

        let zoom = gtk::GestureZoom::new();
        zoom.connect_scale_changed(move |_, scale| {
            log::info!(target: "frames", "gtk zoom scale={scale:.3}");
            hub().stats.record_input(InputKind::GtkZoom);
        });
        picture.add_controller(zoom);

        Self { picture }
    }
}

impl NativeComponent for FrameView {
    fn widget(&self) -> gtk::Widget {
        self.picture.clone().upcast()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        log::debug!(target: "frames", "native frame prop {name} = {value:?}");
    }
}

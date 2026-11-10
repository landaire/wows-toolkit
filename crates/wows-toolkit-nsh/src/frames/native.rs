use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use nsh_core::JsonValue;
use nsh_platform_linux::ComponentContext;
use nsh_platform_linux::Components;
use nsh_platform_linux::NativeComponent;
use nsh_platform_linux::gtk4 as gtk;
use serde_json::json;

use super::AlphaMode;
use super::FrameGeneration;
use super::Modifiers;
use super::PinchScale;
use super::PixelPoint;
use super::PixelSize;
use super::PointerButton;
use super::ScaleFactor;
use super::ScrollSteps;
use super::Surface;
use super::SurfaceId;
use super::SurfaceInput;
use super::ViewSize;

/// Name the TSX side passes to `nativeComponent`.
pub const COMPONENT: &str = "wows.frame";

pub fn register(components: &Components) {
    components.register(
        COMPONENT,
        Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> { Box::new(FrameView::new(context)) }),
    );
}

/// The view's binding: the surface it shows and what it last told it.
#[derive(Default)]
struct Binding {
    surface: Option<Arc<Surface>>,
    shown_generation: Option<FrameGeneration>,
    reported: Option<ViewSize>,
}

type SharedBinding = Rc<RefCell<Binding>>;

/// Shows a surface's latest frame, polled on the widget's frame clock, and
/// forwards pointer, wheel, pinch, key and focus input to it.
struct FrameView {
    picture: gtk::Picture,
    binding: SharedBinding,
}

fn modifiers(state: gdk::ModifierType) -> Modifiers {
    Modifiers {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        control: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        meta: state.contains(gdk::ModifierType::META_MASK) || state.contains(gdk::ModifierType::SUPER_MASK),
    }
}

fn button(number: u32) -> PointerButton {
    match number {
        gdk::BUTTON_PRIMARY => PointerButton::Primary,
        gdk::BUTTON_MIDDLE => PointerButton::Middle,
        gdk::BUTTON_SECONDARY => PointerButton::Secondary,
        other => PointerButton::Other(other),
    }
}

fn send(binding: &SharedBinding, input: SurfaceInput) {
    if let Some(surface) = &binding.borrow().surface {
        surface.send_input(input);
    }
}

impl FrameView {
    fn new(context: ComponentContext) -> Self {
        let picture = gtk::Picture::new();
        // Producers render at the reported view size, so the frame maps 1:1.
        picture.set_content_fit(gtk::ContentFit::Fill);
        picture.set_can_shrink(true);
        picture.set_focusable(true);
        picture.set_can_focus(true);
        picture.set_hexpand(true);
        picture.set_vexpand(true);

        let binding: SharedBinding = Rc::default();
        let scale = |widget: &gtk::Picture| widget.scale_factor().max(1) as f32;
        let to_pixels = move |widget: &gtk::Picture, x: f64, y: f64| {
            let factor = scale(widget);
            PixelPoint { x: x as f32 * factor, y: y as f32 * factor }
        };

        let tick_binding = Rc::clone(&binding);
        picture.add_tick_callback(move |picture, _clock| {
            let mut binding = tick_binding.borrow_mut();
            let Some(surface) = binding.surface.clone() else {
                return glib::ControlFlow::Continue;
            };

            let factor = scale(picture);
            let size = PixelSize {
                width: (picture.width().max(0) as f32 * factor).round() as u32,
                height: (picture.height().max(0) as f32 * factor).round() as u32,
            };
            let view = ViewSize { size, scale: ScaleFactor::new(factor) };
            if binding.reported != Some(view) && !size.is_empty() {
                binding.reported = Some(view);
                surface.send_input(SurfaceInput::Resized { size, scale: view.scale });
            }

            let generation = surface.generation();
            if binding.shown_generation == Some(generation) {
                return glib::ControlFlow::Continue;
            }
            binding.shown_generation = Some(generation);
            match surface.latest() {
                Some(frame) => {
                    let format = match frame.alpha {
                        AlphaMode::Premultiplied => gdk::MemoryFormat::R8g8b8a8Premultiplied,
                        AlphaMode::Straight => gdk::MemoryFormat::R8g8b8a8,
                    };
                    let bytes = glib::Bytes::from(&frame.pixels[..]);
                    let texture = gdk::MemoryTexture::new(
                        frame.size.width as i32,
                        frame.size.height as i32,
                        format,
                        &bytes,
                        frame.size.width as usize * 4,
                    );
                    picture.set_paintable(Some(&texture));
                }
                None => picture.set_paintable(gdk::Paintable::NONE),
            }
            glib::ControlFlow::Continue
        });

        let last_pointer = Rc::new(Cell::new(None::<PixelPoint>));

        let click = gtk::GestureClick::new();
        click.set_button(0);
        let press_binding = Rc::clone(&binding);
        let press_context = context.clone();
        click.connect_pressed(move |gesture, clicks, x, y| {
            let Some(widget) = gesture.widget().and_downcast::<gtk::Picture>() else { return };
            widget.grab_focus();
            let pressed = button(gesture.current_button());
            let mods = modifiers(gesture.current_event_state());
            send(
                &press_binding,
                SurfaceInput::PointerDown {
                    at: to_pixels(&widget, x, y),
                    button: pressed,
                    clicks: clicks.max(1) as u32,
                    modifiers: mods,
                },
            );
            if pressed == PointerButton::Secondary {
                press_context.emit("secondaryPress", json!({ "x": x, "y": y }));
            }
        });
        let release_binding = Rc::clone(&binding);
        click.connect_released(move |gesture, _clicks, x, y| {
            let Some(widget) = gesture.widget().and_downcast::<gtk::Picture>() else { return };
            send(
                &release_binding,
                SurfaceInput::PointerUp {
                    at: to_pixels(&widget, x, y),
                    button: button(gesture.current_button()),
                    modifiers: modifiers(gesture.current_event_state()),
                },
            );
        });
        picture.add_controller(click);

        let motion = gtk::EventControllerMotion::new();
        let motion_binding = Rc::clone(&binding);
        let motion_last = Rc::clone(&last_pointer);
        motion.connect_motion(move |controller, x, y| {
            let Some(widget) = controller.widget().and_downcast::<gtk::Picture>() else { return };
            let at = to_pixels(&widget, x, y);
            motion_last.set(Some(at));
            send(&motion_binding, SurfaceInput::PointerMoved { at, modifiers: modifiers(controller.current_event_state()) });
        });
        let leave_binding = Rc::clone(&binding);
        let leave_last = Rc::clone(&last_pointer);
        motion.connect_leave(move |_| {
            leave_last.set(None);
            send(&leave_binding, SurfaceInput::PointerLeft);
        });
        picture.add_controller(motion);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let scroll_binding = Rc::clone(&binding);
        let scroll_last = Rc::clone(&last_pointer);
        scroll.connect_scroll(move |controller, dx, dy| {
            send(
                &scroll_binding,
                SurfaceInput::Wheel {
                    at: scroll_last.get(),
                    dx: ScrollSteps::new(dx as f32),
                    dy: ScrollSteps::new(dy as f32),
                    modifiers: modifiers(controller.current_event_state()),
                },
            );
            glib::Propagation::Stop
        });
        picture.add_controller(scroll);

        let zoom = gtk::GestureZoom::new();
        let zoom_binding = Rc::clone(&binding);
        zoom.connect_scale_changed(move |_, scale| {
            send(&zoom_binding, SurfaceInput::Pinch { scale: PinchScale::new(scale as f32) });
        });
        picture.add_controller(zoom);

        let keys = gtk::EventControllerKey::new();
        let down_binding = Rc::clone(&binding);
        keys.connect_key_pressed(move |_, key, _code, state| {
            let Some(name) = key.name() else { return glib::Propagation::Proceed };
            send(&down_binding, SurfaceInput::Key { key: name.to_string(), pressed: true, modifiers: modifiers(state) });
            glib::Propagation::Stop
        });
        let up_binding = Rc::clone(&binding);
        keys.connect_key_released(move |_, key, _code, state| {
            if let Some(name) = key.name() {
                send(&up_binding, SurfaceInput::Key { key: name.to_string(), pressed: false, modifiers: modifiers(state) });
            }
        });
        picture.add_controller(keys);

        let focus = gtk::EventControllerFocus::new();
        let enter_binding = Rc::clone(&binding);
        focus.connect_enter(move |_| send(&enter_binding, SurfaceInput::Focus { focused: true }));
        let focus_leave_binding = Rc::clone(&binding);
        focus.connect_leave(move |_| send(&focus_leave_binding, SurfaceInput::Focus { focused: false }));
        picture.add_controller(focus);

        Self { picture, binding }
    }
}

impl NativeComponent for FrameView {
    fn widget(&self) -> gtk::Widget {
        self.picture.clone().upcast()
    }

    fn set_prop(&mut self, name: &str, value: Option<&JsonValue>) {
        if name != "surface" {
            return;
        }
        let mut binding = self.binding.borrow_mut();
        *binding = Binding::default();
        binding.surface = value.and_then(JsonValue::as_str).map(|name| super::surface(&SurfaceId::new(name)));
        if binding.surface.is_none() {
            self.picture.set_paintable(gdk::Paintable::NONE);
        }
    }
}

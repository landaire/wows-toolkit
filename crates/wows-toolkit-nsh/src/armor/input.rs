//! What a frame surface's input means to an armor scene.
//!
//! The surface reports device pixels, GDK key names, wheel steps and a pinch
//! scale measured from the start of the gesture. The scene wants logical
//! pixels, lowercase key names, wheel pixels and a pinch factor against the
//! previous report.

use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use wows_toolkit_armor::scene::Button;
use wows_toolkit_armor::viewport::types::Vec2;

use crate::frames::PinchScale;
use crate::frames::PixelPoint;
use crate::frames::PointerButton;
use crate::frames::ScaleFactor;
use crate::frames::ScrollSteps;
use crate::frames::SurfaceInput;
use crate::frames::ViewSize;

/// One surface event, in the scene's terms.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum SceneInput {
    /// The view's size in device pixels and its device pixels per point.
    Resize {
        view: ViewSize,
    },
    Down {
        at: Vec2,
        button: Button,
        clicks: u32,
    },
    Up {
        at: Vec2,
        button: Button,
        shift: bool,
    },
    Moved {
        at: Vec2,
    },
    Left,
    /// Logical pixels; positive zooms in.
    Wheel {
        dy: f32,
    },
    /// Above 1 brings the model closer.
    Pinch {
        factor: f32,
    },
    KeyDown {
        key: String,
        command: bool,
        shift: bool,
    },
    KeyUp {
        key: String,
    },
    ReleaseKeys,
}

/// The pinch gesture in progress: the scale last reported and when.
#[derive(Debug, Clone, Copy)]
struct PinchGesture {
    previous: PinchScale,
    at: Instant,
}

/// Turns one surface's input into scene input, keeping what that needs
/// between events.
#[derive(Debug)]
pub(super) struct InputMapper {
    /// Device pixels per logical pixel, from the last resize.
    scale: ScaleFactor,
    pinch: Option<PinchGesture>,
    /// Keys held, so an auto-repeat is told from a fresh press.
    keys_down: HashSet<String>,
}

impl InputMapper {
    pub fn new() -> Self {
        // A view not yet sized is drawn at one device pixel per point.
        Self { scale: ScaleFactor::new(1.0), pinch: None, keys_down: HashSet::new() }
    }

    pub fn map(&mut self, input: SurfaceInput, now: Instant) -> Option<SceneInput> {
        match input {
            SurfaceInput::Resized { size, scale } => {
                // A scale the view could not measure leaves the last one.
                if scale.get().is_finite() && scale.get() > 0.0 {
                    self.scale = scale;
                }
                Some(SceneInput::Resize { view: ViewSize { size, scale: self.scale } })
            }
            SurfaceInput::PointerDown { at, button, clicks, .. } => {
                self.pinch = None;
                let button = scene_button(button)?;
                Some(SceneInput::Down { at: self.logical(at), button, clicks })
            }
            SurfaceInput::PointerUp { at, button, modifiers } => {
                self.pinch = None;
                let button = scene_button(button)?;
                Some(SceneInput::Up { at: self.logical(at), button, shift: modifiers.shift })
            }
            SurfaceInput::PointerMoved { at, .. } => {
                self.pinch = None;
                Some(SceneInput::Moved { at: self.logical(at) })
            }
            SurfaceInput::PointerLeft => {
                self.pinch = None;
                Some(SceneInput::Left)
            }
            SurfaceInput::Wheel { dy, .. } => wheel_pixels(dy).map(|dy| SceneInput::Wheel { dy }),
            SurfaceInput::Pinch { scale } => self.pinch(scale, now),
            SurfaceInput::Key { key, pressed, modifiers } => {
                let key = key_name(&key);
                if pressed {
                    // An auto-repeat would toggle a Ctrl shortcut once per repeat.
                    if !self.keys_down.insert(key.clone()) {
                        return None;
                    }
                    return Some(SceneInput::KeyDown { key, command: modifiers.control, shift: modifiers.shift });
                }
                self.keys_down.remove(&key);
                Some(SceneInput::KeyUp { key })
            }
            SurfaceInput::Focus { focused: true } => None,
            SurfaceInput::Focus { focused: false } => {
                self.keys_down.clear();
                Some(SceneInput::ReleaseKeys)
            }
        }
    }

    fn logical(&self, at: PixelPoint) -> Vec2 {
        let scale = self.scale.get();
        Vec2::new(at.x / scale, at.y / scale)
    }

    fn pinch(&mut self, scale: PinchScale, now: Instant) -> Option<SceneInput> {
        const IDLE: Duration = Duration::from_millis(250);

        if !scale.get().is_finite() || scale.get() <= 0.0 {
            return None;
        }

        // A gesture idle this long has ended; this report starts the next one.
        let previous = match self.pinch {
            Some(gesture) if now.saturating_duration_since(gesture.at) <= IDLE => gesture.previous,
            _ => PinchScale::new(1.0),
        };
        self.pinch = Some(PinchGesture { previous: scale, at: now });

        Some(SceneInput::Pinch { factor: scale.get() / previous.get() })
    }
}

/// The button the viewport reacts to; extra mouse buttons do nothing.
fn scene_button(button: PointerButton) -> Option<Button> {
    match button {
        PointerButton::Primary => Some(Button::Primary),
        PointerButton::Middle => Some(Button::Middle),
        PointerButton::Secondary => Some(Button::Secondary),
        PointerButton::Other(_) => None,
    }
}

/// Wheel steps as scene pixels: a step down zooms out.
fn wheel_pixels(steps: ScrollSteps) -> Option<f32> {
    const PIXELS_PER_STEP: f32 = 40.0;

    let steps = steps.get();
    if steps == 0.0 || !steps.is_finite() {
        return None;
    }

    Some(-steps * PIXELS_PER_STEP)
}

/// A GDK key name as the scene names keys (`"Up"` is `"up"`, a shifted
/// `"W"` is `"w"`).
fn key_name(gdk: &str) -> String {
    gdk.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::Modifiers;
    use crate::frames::PixelSize;

    fn point(x: f32, y: f32) -> PixelPoint {
        PixelPoint { x, y }
    }

    fn mapper_at_scale(scale: f32) -> InputMapper {
        let mut mapper = InputMapper::new();
        mapper.map(
            SurfaceInput::Resized { size: PixelSize { width: 400, height: 300 }, scale: ScaleFactor::new(scale) },
            Instant::now(),
        );
        mapper
    }

    fn wheel(dx: f32, dy: f32) -> SurfaceInput {
        SurfaceInput::Wheel {
            at: None,
            dx: ScrollSteps::new(dx),
            dy: ScrollSteps::new(dy),
            modifiers: Modifiers::default(),
        }
    }

    fn pinch(scale: f32) -> SurfaceInput {
        SurfaceInput::Pinch { scale: PinchScale::new(scale) }
    }

    fn key(name: &str, pressed: bool, modifiers: Modifiers) -> SurfaceInput {
        SurfaceInput::Key { key: name.to_owned(), pressed, modifiers }
    }

    #[test]
    fn a_wheel_step_down_zooms_out_by_forty_pixels() {
        let mut mapper = InputMapper::new();
        let now = Instant::now();

        let down = mapper.map(wheel(0.0, 1.0), now);
        let up = mapper.map(wheel(0.0, -0.5), now);
        let flat = mapper.map(wheel(3.0, 0.0), now);

        assert_eq!(down, Some(SceneInput::Wheel { dy: -40.0 }));
        assert_eq!(up, Some(SceneInput::Wheel { dy: 20.0 }));
        assert_eq!(flat, None);
    }

    #[test]
    fn device_pixels_are_divided_by_the_scale() {
        let mut mapper = mapper_at_scale(2.0);
        let now = Instant::now();

        let moved =
            mapper.map(SurfaceInput::PointerMoved { at: point(100.0, 50.0), modifiers: Modifiers::default() }, now);
        let down = mapper.map(
            SurfaceInput::PointerDown {
                at: point(30.0, 8.0),
                button: PointerButton::Middle,
                clicks: 2,
                modifiers: Modifiers::default(),
            },
            now,
        );
        let up = mapper.map(
            SurfaceInput::PointerUp {
                at: point(30.0, 8.0),
                button: PointerButton::Primary,
                modifiers: Modifiers { shift: true, ..Modifiers::default() },
            },
            now,
        );

        assert_eq!(moved, Some(SceneInput::Moved { at: Vec2::new(50.0, 25.0) }));
        assert_eq!(down, Some(SceneInput::Down { at: Vec2::new(15.0, 4.0), button: Button::Middle, clicks: 2 }));
        assert_eq!(up, Some(SceneInput::Up { at: Vec2::new(15.0, 4.0), button: Button::Primary, shift: true }));
    }

    #[test]
    fn an_unmeasured_scale_keeps_the_last_one() {
        let mut mapper = mapper_at_scale(1.5);
        let now = Instant::now();

        let resized = mapper.map(
            SurfaceInput::Resized { size: PixelSize { width: 10, height: 10 }, scale: ScaleFactor::new(0.0) },
            now,
        );
        let moved =
            mapper.map(SurfaceInput::PointerMoved { at: point(3.0, 6.0), modifiers: Modifiers::default() }, now);

        assert_eq!(
            resized,
            Some(SceneInput::Resize {
                view: ViewSize { size: PixelSize { width: 10, height: 10 }, scale: ScaleFactor::new(1.5) }
            })
        );
        assert_eq!(moved, Some(SceneInput::Moved { at: Vec2::new(2.0, 4.0) }));
    }

    #[test]
    fn extra_mouse_buttons_are_ignored() {
        let mut mapper = InputMapper::new();
        let down = mapper.map(
            SurfaceInput::PointerDown {
                at: point(1.0, 1.0),
                button: PointerButton::Other(8),
                clicks: 1,
                modifiers: Modifiers::default(),
            },
            Instant::now(),
        );

        assert_eq!(down, None);
    }

    #[test]
    fn key_names_are_lowercased_and_control_is_the_command_key() {
        let mut mapper = InputMapper::new();
        let now = Instant::now();
        let control = Modifiers { control: true, ..Modifiers::default() };

        let up = mapper.map(key("Up", true, Modifiers::default()), now);
        let shifted = mapper.map(key("W", true, Modifiers { shift: true, ..Modifiers::default() }), now);
        let save = mapper.map(key("s", true, control), now);

        assert_eq!(up, Some(SceneInput::KeyDown { key: "up".to_owned(), command: false, shift: false }));
        assert_eq!(shifted, Some(SceneInput::KeyDown { key: "w".to_owned(), command: false, shift: true }));
        assert_eq!(save, Some(SceneInput::KeyDown { key: "s".to_owned(), command: true, shift: false }));
    }

    #[test]
    fn a_repeat_of_a_held_key_is_not_dispatched_again() {
        let mut mapper = InputMapper::new();
        let now = Instant::now();
        let control = Modifiers { control: true, ..Modifiers::default() };

        let first = mapper.map(key("t", true, control), now);
        let repeat = mapper.map(key("t", true, control), now);
        let released = mapper.map(key("T", false, control), now);
        let again = mapper.map(key("t", true, control), now);

        assert_eq!(first, Some(SceneInput::KeyDown { key: "t".to_owned(), command: true, shift: false }));
        assert_eq!(repeat, None);
        assert_eq!(released, Some(SceneInput::KeyUp { key: "t".to_owned() }));
        assert_eq!(again, first);
    }

    #[test]
    fn losing_focus_releases_every_key() {
        let mut mapper = InputMapper::new();
        let now = Instant::now();

        mapper.map(key("w", true, Modifiers::default()), now);
        let lost = mapper.map(SurfaceInput::Focus { focused: false }, now);
        let pressed_again = mapper.map(key("w", true, Modifiers::default()), now);
        let gained = mapper.map(SurfaceInput::Focus { focused: true }, now);

        assert_eq!(lost, Some(SceneInput::ReleaseKeys));
        assert_eq!(pressed_again, Some(SceneInput::KeyDown { key: "w".to_owned(), command: false, shift: false }));
        assert_eq!(gained, None);
    }

    #[test]
    fn a_pinch_reports_the_factor_since_the_previous_report() {
        let mut mapper = InputMapper::new();
        let start = Instant::now();

        let first = mapper.map(pinch(1.2), start);
        let second = mapper.map(pinch(1.5), start + Duration::from_millis(16));

        assert_eq!(first, Some(SceneInput::Pinch { factor: 1.2 }));
        assert_eq!(second, Some(SceneInput::Pinch { factor: 1.5 / 1.2 }));
    }

    #[test]
    fn a_pinch_starts_over_after_idling() {
        let mut mapper = InputMapper::new();
        let start = Instant::now();

        mapper.map(pinch(2.0), start);
        let later = mapper.map(pinch(1.1), start + Duration::from_millis(300));

        assert_eq!(later, Some(SceneInput::Pinch { factor: 1.1 }));
    }

    #[test]
    fn a_pointer_event_ends_the_pinch() {
        let mut mapper = InputMapper::new();
        let start = Instant::now();

        mapper.map(pinch(2.0), start);
        mapper.map(SurfaceInput::PointerMoved { at: point(1.0, 1.0), modifiers: Modifiers::default() }, start);
        let next = mapper.map(pinch(0.8), start + Duration::from_millis(10));
        let bad = mapper.map(pinch(f32::NAN), start + Duration::from_millis(20));

        assert_eq!(next, Some(SceneInput::Pinch { factor: 0.8 }));
        assert_eq!(bad, None);
    }
}

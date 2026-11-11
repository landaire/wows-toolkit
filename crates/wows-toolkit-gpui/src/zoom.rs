//! The interface zoom: how large the main window draws its text and controls,
//! and the shortcuts that step it.

use gpui_kit::Action;
use gpui_kit::App;
use gpui_kit::KeyBinding;
use gpui_kit::actions;
use rust_i18n::t;
use serde::Serialize;

use crate::status_bar::ModifierKey;

actions!(zoom, [ZoomIn, ZoomOut, ResetZoom]);

/// How large the interface is drawn, as a multiple of its unscaled size.
/// Always inside [`ZoomFactor::MIN`] and [`ZoomFactor::MAX`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ZoomFactor(f32);

impl ZoomFactor {
    /// The range the egui settings tab's slider offers.
    pub const MIN: Self = Self(0.5);
    pub const MAX: Self = Self(2.0);

    /// A never-saved setting and the Reset button, as the egui app has it.
    pub const DEFAULT: Self = Self(1.15);

    /// Unscaled, which the reset shortcut returns to, as egui's own does.
    pub const ACTUAL_SIZE: Self = Self(1.0);

    /// Shortcut steps per unit of zoom: each step is a tenth.
    const STEPS_PER_UNIT: f32 = 10.0;

    /// How far from a step a factor may be and still count as on it: `1.1`
    /// is `10.999999` tenths in `f32`.
    const STEP_TOLERANCE: f32 = 1e-3;

    /// `value` moved into the range.
    pub fn new(value: f32) -> Self {
        Self(value.clamp(Self::MIN.0, Self::MAX.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// The next step up. Steps land on tenths, so a factor set between them
    /// by the slider rejoins the shortcut's sequence rather than carrying its
    /// offset along.
    pub fn zoomed_in(self) -> Self {
        let steps = (self.0 * Self::STEPS_PER_UNIT + Self::STEP_TOLERANCE).floor() + 1.0;

        Self::new(steps / Self::STEPS_PER_UNIT)
    }

    /// The next step down, on tenths as [`Self::zoomed_in`].
    pub fn zoomed_out(self) -> Self {
        let steps = (self.0 * Self::STEPS_PER_UNIT - Self::STEP_TOLERANCE).ceil() - 1.0;

        Self::new(steps / Self::STEPS_PER_UNIT)
    }
}

/// Binds the zoom shortcuts, to Ctrl or to Cmd on macOS. Without a context,
/// so they reach whatever has focus, a dialog or a text field included: none
/// of the kit's own bindings use these keys.
pub fn bind_keys(cx: &mut App) {
    let shortcuts: [(&str, Box<dyn Action>); 7] = [
        ("secondary-=", Box::new(ZoomIn)),
        // Shift and `=` arrives as `+` on some platforms and as itself on others.
        ("secondary-+", Box::new(ZoomIn)),
        ("secondary-shift-=", Box::new(ZoomIn)),
        ("secondary--", Box::new(ZoomOut)),
        // The keypad's keys, as Linux names them. Windows and macOS report
        // them as `+`, `-` and `0`.
        ("secondary-add", Box::new(ZoomIn)),
        ("secondary-subtract", Box::new(ZoomOut)),
        ("secondary-0", Box::new(ResetZoom)),
    ];
    let bindings: Vec<KeyBinding> = shortcuts
        .into_iter()
        .map(|(keystrokes, action)| {
            KeyBinding::load(keystrokes, action, None, false, None, cx).expect("a zoom shortcut is a valid keystroke")
        })
        .collect();

    cx.bind_keys(bindings);
}

/// The zoom shortcuts, with the platform's own modifier name.
pub fn shortcuts_hint(cx: &App) -> String {
    let modifier = ModifierKey::Secondary.label(cx);

    t!(
        "ui.settings.panel.zoom_shortcuts",
        zoom_in = format!("{modifier}+="),
        zoom_out = format!("{modifier}+-"),
        reset = format!("{modifier}+0")
    )
    .into_owned()
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::Bounds;
    use gpui_kit::Entity;
    use gpui_kit::Modifiers;
    use gpui_kit::MouseButton;
    use gpui_kit::MouseDownEvent;
    use gpui_kit::MouseMoveEvent;
    use gpui_kit::MouseUpEvent;
    use gpui_kit::Pixels;
    use gpui_kit::PlatformInput;
    use gpui_kit::Point;
    use gpui_kit::TestAppContext;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::ActiveTheme as _;
    use gpui_kit::component::Root;
    use gpui_kit::point;
    use gpui_kit::test::TestWindowExt as _;
    use wows_toolkit_viewmodel::settings::keys;

    use super::*;
    use crate::app::Setting;
    use crate::app::SettingsSection;
    use crate::interaction_tests::open_app_in_root;
    use crate::interaction_tests::show_settings;
    use crate::interaction_tests::test_settings;

    #[test]
    fn steps_land_on_tenths_and_stop_at_the_ends() {
        assert_eq!(ZoomFactor::ACTUAL_SIZE.zoomed_in(), ZoomFactor::new(1.1));
        assert_eq!(ZoomFactor::new(1.1).zoomed_in(), ZoomFactor::new(1.2));
        assert_eq!(ZoomFactor::new(1.1).zoomed_out(), ZoomFactor::ACTUAL_SIZE);
        assert_eq!(ZoomFactor::DEFAULT.zoomed_in(), ZoomFactor::new(1.2));
        assert_eq!(ZoomFactor::DEFAULT.zoomed_out(), ZoomFactor::new(1.1));

        assert_eq!(ZoomFactor::MAX.zoomed_in(), ZoomFactor::MAX);
        assert_eq!(ZoomFactor::MIN.zoomed_out(), ZoomFactor::MIN);
        assert_eq!(ZoomFactor::new(5.0), ZoomFactor::MAX);
    }

    /// The app in its `Root`, with the shortcuts bound as `main` binds them.
    struct Harness {
        window: WindowHandle<Root>,
        app: Entity<crate::app::App>,
        /// The theme's text size at 1x, which every other zoom scales.
        unscaled_text: f32,
    }

    impl Harness {
        /// Opens the app with its settings read, and resets it to 1x, which
        /// is what the drawn zoom is measured against.
        fn open(cx: &mut TestAppContext) -> Self {
            let (window, app) = open_app_in_root(cx);
            cx.update(bind_keys);
            cx.update_window(window.into(), |_, window, cx| {
                app.update(cx, |app, cx| app.apply_settings(test_settings(), window, cx));
                window.render_frame(cx);
            })
            .expect("the test window stays open");
            cx.simulate_keystrokes(window.into(), "secondary-0");
            assert_eq!(saved_zoom(), Some(1.0), "the reset shortcut saves 1x");

            Self { window, app, unscaled_text: text_size(cx) }
        }

        /// The zoom the window is drawn at.
        fn drawn_zoom(&self, cx: &mut TestAppContext) -> f32 {
            text_size(cx) / self.unscaled_text
        }

        /// Presses `keys` and checks that the zoom is drawn at, saved as and
        /// announced as `expected`.
        fn press(&self, keys: &str, expected: f32, cx: &mut TestAppContext) {
            cx.simulate_keystrokes(self.window.into(), keys);

            let drawn = self.drawn_zoom(cx);
            assert!((drawn - expected).abs() < 1e-4, "{keys} draws at {drawn}, not {expected}");
            assert_eq!(saved_zoom(), Some(expected), "{keys} saves {expected}");
            let said = crate::toast::said_for_test();
            assert_eq!(said.last().map(String::as_str), Some(format!("Zoom: {expected:.1}x").as_str()));
        }

        fn dispatch(&self, event: PlatformInput, cx: &mut TestAppContext) {
            cx.update_window(self.window.into(), |_, window, cx| {
                window.dispatch_event(event, cx);
                window.render_frame(cx);
            })
            .expect("the test window stays open");
        }
    }

    /// The text size the theme hands the widgets, which the zoom scales.
    fn text_size(cx: &mut TestAppContext) -> f32 {
        cx.update(|cx| f32::from(cx.theme().font_size))
    }

    /// The newest zoom handed to the settings store.
    fn saved_zoom() -> Option<f32> {
        crate::settings_store::saved_for_test()
            .into_iter()
            .rev()
            .find(|(key, _)| *key == keys::ZOOM_FACTOR)
            .map(|(_, json)| json.parse().expect("the zoom is saved as a number"))
    }

    #[gpui_kit::test]
    fn the_shortcuts_step_clamp_and_reset_the_zoom(cx: &mut TestAppContext) {
        let app = Harness::open(cx);

        app.press("secondary-=", 1.1, cx);
        app.press("secondary-+", 1.2, cx);
        app.press("secondary-shift-=", 1.3, cx);
        app.press("secondary-add", 1.4, cx);
        app.press("secondary--", 1.3, cx);
        app.press("secondary-subtract", 1.2, cx);
        app.press("secondary-0", 1.0, cx);

        app.press(&["secondary-="; 12].join(" "), 2.0, cx);
        app.press("secondary-=", 2.0, cx);
        app.press(&["secondary--"; 16].join(" "), 0.5, cx);
        app.press("secondary--", 0.5, cx);
        app.press("secondary-0", 1.0, cx);
    }

    /// A dialog `Root` hosts holds focus outside the app view, in the
    /// palette's case in a text field; the shortcuts still reach the zoom.
    #[gpui_kit::test]
    fn the_shortcuts_reach_the_zoom_from_the_palette(cx: &mut TestAppContext) {
        let app = Harness::open(cx);
        cx.update_window(app.window.into(), |_, window, cx| {
            app.app.update(cx, |app, cx| app.open_palette(window, cx));
        })
        .expect("the test window stays open");
        cx.run_until_parked();
        cx.update_window(app.window.into(), |_, window, cx| window.render_frame(cx))
            .expect("the test window stays open");

        app.press("secondary-=", 1.1, cx);
        app.press("secondary--", 1.0, cx);
    }

    fn moved(position: Point<Pixels>, pressed_button: Option<MouseButton>) -> PlatformInput {
        PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button, modifiers: Modifiers::default() })
    }

    /// Where along `track` the slider puts `value`.
    fn at_value(track: Bounds<Pixels>, value: f32) -> Point<Pixels> {
        let fraction = (value - ZoomFactor::MIN.get()) / (ZoomFactor::MAX.get() - ZoomFactor::MIN.get());

        point(track.left() + track.size.width * fraction, track.center().y)
    }

    /// A drag leaves the window's scale alone until the release, then
    /// Before the settings are read, a shortcut changes nothing: the stored
    /// zoom is about to replace it, and there is nowhere to save it yet.
    #[gpui_kit::test]
    fn the_shortcuts_wait_for_the_settings(cx: &mut TestAppContext) {
        let (window, _app) = open_app_in_root(cx);
        cx.update(bind_keys);
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
        let before = text_size(cx);

        cx.simulate_keystrokes(window.into(), "secondary-= secondary-0");

        assert_eq!(text_size(cx), before);
        assert_eq!(saved_zoom(), None);
    }

    /// Opens the drawer at the zoom row, presses the slider's thumb at 1x
    /// and drags it to 1.55, checking that no step rescales the window or
    /// saves anything. Returns where the pointer was let go.
    fn drag_to_1_55(app: &Harness, cx: &mut TestAppContext) -> Point<Pixels> {
        // The reset's note sits over the gear that opens the drawer.
        cx.executor().advance_clock(std::time::Duration::from_secs(30));
        cx.run_until_parked();
        let track = cx
            .update_window(app.window.into(), |_, window, cx| {
                show_settings(window, SettingsSection::Appearance, cx);
                window.within(("settings-row", Setting::Zoom as usize)).find("slider-bar-container").bounds()
            })
            .expect("the test window stays open");
        let saves = crate::settings_store::saved_for_test().len();

        let start = at_value(track, ZoomFactor::ACTUAL_SIZE.get());
        app.dispatch(moved(start, None), cx);
        let pressed = MouseDownEvent {
            button: MouseButton::Left,
            position: start,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        };
        app.dispatch(PlatformInput::MouseDown(pressed), cx);
        let end = at_value(track, 1.55);
        for step in 1..=10 {
            let x = start.x + (end.x - start.x) * (step as f32 / 10.0);
            app.dispatch(moved(point(x, start.y), Some(MouseButton::Left)), cx);
            assert_eq!(app.drawn_zoom(cx), 1.0, "step {step} leaves the scale alone");
        }
        assert_eq!(crate::settings_store::saved_for_test().len(), saves, "nothing is saved mid-drag");

        end
    }

    fn release(app: &Harness, position: Point<Pixels>, cx: &mut TestAppContext) {
        let released =
            MouseUpEvent { button: MouseButton::Left, position, modifiers: Modifiers::default(), click_count: 1 };
        app.dispatch(PlatformInput::MouseUp(released), cx);
    }

    fn assert_zoom_applied_and_saved(app: &Harness, expected: f32, cx: &mut TestAppContext) {
        let drawn = app.drawn_zoom(cx);
        assert!((drawn - expected).abs() < 1e-4, "drawn at {drawn}, not {expected}");
        let saved = saved_zoom().expect("the zoom is saved");
        assert!((saved - expected).abs() < 1e-4, "{saved} is saved, not {expected}");
    }

    /// A drag leaves the window's scale alone until the release, then
    /// applies and saves the value under the pointer as the track stood when
    /// the drag began. Applying it per step rescales the track under the
    /// pointer, and the value then follows the rescaling instead.
    #[gpui_kit::test]
    fn dragging_the_zoom_slider_applies_the_zoom_on_release(cx: &mut TestAppContext) {
        let app = Harness::open(cx);
        let end = drag_to_1_55(&app, cx);

        release(&app, end, cx);

        assert_zoom_applied_and_saved(&app, 1.55, cx);
    }

    /// A drawer shut by a key mid-drag takes the slider, and its own release
    /// listener, out of the frame; the release still ends the drag.
    #[gpui_kit::test]
    fn a_drag_ends_at_the_release_after_the_drawer_shuts(cx: &mut TestAppContext) {
        let app = Harness::open(cx);
        let end = drag_to_1_55(&app, cx);
        cx.simulate_keystrokes(app.window.into(), "escape");
        cx.update_window(app.window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("settings-row", Setting::Zoom as usize)).is_none(), "the drawer is shut");
        })
        .expect("the test window stays open");

        release(&app, end, cx);

        assert_zoom_applied_and_saved(&app, 1.55, cx);
        cx.update_window(app.window.into(), |_, window, cx| {
            show_settings(window, SettingsSection::Appearance, cx);
        })
        .expect("the test window stays open");
        cx.simulate_keystrokes(app.window.into(), "secondary--");
        assert_zoom_applied_and_saved(&app, 1.5, cx);
    }
}

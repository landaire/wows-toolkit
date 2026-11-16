//! The interface zoom: how large the main window draws its text and controls,
//! and the shortcuts that step it.

use gpui_kit::Action;
use gpui_kit::App;
use gpui_kit::KeyBinding;
use gpui_kit::actions;
use rust_i18n::t;

use crate::status_bar::ModifierKey;

actions!(zoom, [ZoomIn, ZoomOut, ResetZoom]);

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
    use gpui_kit::ScrollDelta;
    use gpui_kit::TestAppContext;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::ActiveTheme as _;
    use gpui_kit::component::Root;
    use gpui_kit::point;
    use gpui_kit::px;
    use gpui_kit::test::TestWindowExt as _;
    use wows_toolkit_viewmodel::settings::ZoomFactor;
    use wows_toolkit_viewmodel::settings::keys;

    use super::*;
    use crate::app::Setting;
    use crate::app::SettingsSection;
    use crate::interaction_tests::open_app_in_root;
    use crate::interaction_tests::show_settings;
    use crate::interaction_tests::test_settings;

    const ZOOM_ROW: (&str, usize) = ("settings-row", Setting::Zoom as usize);
    const SCROLL: &str = "settings-drawer-scroll";

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

    /// Opens the drawer at the zoom row and drags its slider to 1.55.
    fn drag_to_1_55(app: &Harness, cx: &mut TestAppContext) -> Point<Pixels> {
        show_zoom_row(app, cx);

        drag_shown_slider_to_1_55(app, cx)
    }

    fn show_zoom_row(app: &Harness, cx: &mut TestAppContext) {
        // The reset's note sits over the gear that opens the drawer.
        cx.executor().advance_clock(std::time::Duration::from_secs(30));
        cx.run_until_parked();
        cx.update_window(app.window.into(), |_, window, cx| show_settings(window, SettingsSection::Appearance, cx))
            .expect("the test window stays open");
    }

    /// Presses the drawn zoom slider's thumb at 1x and drags it to 1.55,
    /// checking that no step rescales the window or saves anything. Returns
    /// where the pointer was let go.
    fn drag_shown_slider_to_1_55(app: &Harness, cx: &mut TestAppContext) -> Point<Pixels> {
        let track = cx
            .update_window(app.window.into(), |_, window, _| {
                window.within(ZOOM_ROW).find("slider-bar-container").bounds()
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
            assert!(window.try_find(ZOOM_ROW).is_none(), "the drawer is shut");
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

    fn draw(app: &Harness, cx: &mut TestAppContext) {
        cx.update_window(app.window.into(), |_, window, cx| window.render_frame(cx))
            .expect("the test window stays open");
    }

    /// The zoom slider's top in the window, in the frame last drawn.
    fn zoom_slider_top(app: &Harness, cx: &mut TestAppContext) -> Pixels {
        cx.update_window(app.window.into(), |_, window, _| {
            window.within(ZOOM_ROW).find("slider-bar-container").bounds().top()
        })
        .expect("the test window stays open")
    }

    fn assert_slider_kept(app: &Harness, before: Pixels, change: &str, cx: &mut TestAppContext) {
        let after = zoom_slider_top(app, cx);
        assert!((after - before).abs() < px(1.), "{change} moves the zoom slider from {before:?} to {after:?}");
    }

    /// Each way of changing the zoom keeps the zoom slider where it was
    /// drawn, as far as the list can scroll.
    #[gpui_kit::test]
    fn a_zoom_change_keeps_the_zoom_slider_in_place(cx: &mut TestAppContext) {
        let app = Harness::open(cx);
        show_zoom_row(&app, cx);
        cx.update_window(app.window.into(), |_, window, cx| {
            let list = window.find(SCROLL).bounds();
            let slider = window.within(ZOOM_ROW).find("slider-bar-container").bounds();
            window.scroll(SCROLL, ScrollDelta::Pixels(point(px(0.), list.center().y - slider.top())), cx);
        })
        .expect("the test window stays open");
        let start = zoom_slider_top(&app, cx);
        let list = cx
            .update_window(app.window.into(), |_, window, _| window.find(SCROLL).bounds())
            .expect("the test window stays open");
        assert!((start - list.center().y).abs() < px(1.), "the slider starts mid-list");

        app.press(&["secondary-="; 5].join(" "), 1.5, cx);
        draw(&app, cx);
        assert_slider_kept(&app, start, "zooming in to 1.5", cx);
        app.press(&["secondary-="; 5].join(" "), 2.0, cx);
        draw(&app, cx);
        assert_slider_kept(&app, start, "zooming in to 2.0", cx);

        // At 0.5 the rows over it are too short to keep it there, so the list
        // stops at the top of its range, which scrolling up then shows.
        app.press(&["secondary--"; 15].join(" "), 0.5, cx);
        draw(&app, cx);
        let shrunk = zoom_slider_top(&app, cx);
        assert!(shrunk < start - px(1.), "the slider rises to {shrunk:?} from {start:?}");
        cx.update_window(app.window.into(), |_, window, cx| {
            window.scroll(SCROLL, ScrollDelta::Pixels(point(px(0.), px(1000.))), cx);
        })
        .expect("the test window stays open");
        assert_slider_kept(&app, shrunk, "scrolling up at the top of the range", cx);

        app.press("secondary-0", 1.0, cx);
        draw(&app, cx);
        let reset = zoom_slider_top(&app, cx);

        let end = drag_shown_slider_to_1_55(&app, cx);
        assert_slider_kept(&app, reset, "the drag before its release", cx);
        // Draws the one frame after the release: the first at the new zoom.
        release(&app, end, cx);
        assert_zoom_applied_and_saved(&app, 1.55, cx);
        assert_slider_kept(&app, reset, "a slider drag", cx);
    }

    /// A zoom shortcut pressed while the drawer shows another section leaves
    /// the drawer where it is rather than bringing the zoom slider back.
    #[gpui_kit::test]
    fn a_zoom_change_does_not_bring_back_a_zoom_slider_out_of_view(cx: &mut TestAppContext) {
        let app = Harness::open(cx);
        show_zoom_row(&app, cx);
        let in_view = |window: &mut gpui_kit::Window| {
            let list = window.find(SCROLL).bounds();
            window.within(ZOOM_ROW).find("slider-bar-container").bounds().intersects(&list)
        };
        cx.update_window(app.window.into(), |_, window, cx| {
            window.scroll(SCROLL, ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
            assert!(!in_view(window), "the slider is scrolled out of view");
        })
        .expect("the test window stays open");

        app.press("secondary-=", 1.1, cx);
        draw(&app, cx);

        cx.update_window(app.window.into(), |_, window, _| assert!(!in_view(window), "the slider stays out of view"))
            .expect("the test window stays open");
    }
}

//! The value inputs in a real document, typed into and stepped over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::ColorPickerProps;
use landaire_ui::NumberInputProps;
use landaire_ui::NumberPattern;
use landaire_ui::NumberRange;
use landaire_ui::Rgb;
use landaire_ui::SliderFieldProps;
use landaire_ui::SliderRange;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::prelude::Binding;

const SIZE: WindowSize = WindowSize { width: 480.0, height: 480.0 };

/// The signal a test's view bound, once the view is built.
type Held<T> = Rc<Cell<Option<RwSignal<T, LocalStorage>>>>;

/// A window over `view`, and the signal `view` binds, read back after each step.
fn open<T, F, V>(initial: T, view: F) -> (HeadlessWindow, Held<T>)
where
    T: Clone + 'static,
    F: Fn(RwSignal<T, LocalStorage>) -> V + 'static,
    V: IntoView,
{
    let held = Rc::new(Cell::new(None));
    let window = {
        let held = Rc::clone(&held);
        HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
            let value = RwSignal::new_local(initial.clone());
            held.set(Some(value));
            view(value)
        })
        .expect("opens")
    };

    (window, held)
}

fn value<T: Clone + 'static>(held: &Held<T>) -> T {
    held.get().expect("the view was built").get_untracked()
}

fn text_of(window: &mut HeadlessWindow, field: Element) -> Option<String> {
    window.a11y(field).and_then(|state| state.value)
}

/// Replaces whatever `field` holds with `text` and presses Enter.
fn retype(window: &mut HeadlessWindow, field: Element, text: &str) {
    window.click(field).expect("the field has a box");
    window.press_character('a', Modifiers::PRIMARY);
    window.type_text(text);
    window.press_key(NamedKey::Enter, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
}

#[test]
fn a_typed_number_is_kept_inside_the_range_and_text_that_is_not_one_is_put_back() {
    let (mut window, held) = open(5.0, |value| {
        view! {
            ThemeRoot {
                NumberInput(value = value, range = NumberRange::new(1.0).with_min(0.0).with_max(10.0), label = "Count")
            }
        }
    });
    let field = window.by_role_and_label(Role::TextInput, "Count").expect("the field is announced");

    retype(&mut window, field, "42");
    assert_eq!(value(&held), 10.0);
    assert_eq!(text_of(&mut window, field).as_deref(), Some("10"));

    retype(&mut window, field, "1-2");
    assert_eq!(value(&held), 10.0, "text that is not a number leaves the value alone");
    assert_eq!(text_of(&mut window, field).as_deref(), Some("10"));

    retype(&mut window, field, "2,5");
    assert_eq!(value(&held), 2.5);
}

#[test]
fn arrows_and_buttons_step_and_stop_at_the_ends() {
    let (mut window, held) = open(9.0, |value| {
        view! {
            ThemeRoot {
                NumberInput(value = value, range = NumberRange::new(1.0).with_min(0.0).with_max(10.0), label = "Count")
            }
        }
    });
    let field = window.by_role_and_label(Role::TextInput, "Count").expect("the field is announced");
    window.click(field).expect("the field has a box");

    window.press_key(NamedKey::ArrowUp, Modifiers::NONE);
    window.press_key(NamedKey::ArrowUp, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), 10.0);

    let decrease = window.by_role_and_label(Role::Button, "Decrease").expect("the stepper is announced");
    window.click(decrease).expect("the stepper has a box");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), 9.0);
    assert_eq!(text_of(&mut window, field).as_deref(), Some("9"));
}

#[test]
fn a_digits_field_refuses_every_other_character() {
    let (mut window, held) = open(0.0, |value| {
        view! {
            ThemeRoot {
                NumberInput(value = value, pattern = NumberPattern::Digits, label = "Count")
            }
        }
    });
    let field = window.by_role_and_label(Role::TextInput, "Count").expect("the field is announced");

    retype(&mut window, field, "1a.-2");

    assert_eq!(value(&held), 12.0);
}

#[test]
fn the_slider_and_the_field_follow_each_other() {
    let (mut window, held) = open(0.5, |value| {
        view! {
            ThemeRoot {
                SliderField(value = value, range = SliderRange::new(0.0, 1.0, 0.05), label = "Opacity")
            }
        }
    });
    let slider = window.by_role_and_label(Role::Slider, "Opacity").expect("the slider is announced");
    let field = window.by_role_and_label(Role::TextInput, "Opacity").expect("the field is announced");
    assert_eq!(text_of(&mut window, field).as_deref(), Some("0.50"));

    window.click(slider).expect("the slider has a box");
    window.press_key(NamedKey::ArrowRight, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), 0.55);
    assert_eq!(text_of(&mut window, field).as_deref(), Some("0.55"));

    retype(&mut window, field, "0.333");
    assert_eq!(value(&held), 0.35, "a typed value lands on one of the slider's steps");
    assert_eq!(text_of(&mut window, field).as_deref(), Some("0.35"));
}

#[test]
fn hex_typed_into_the_picker_sets_the_color_and_anything_else_is_put_back() {
    let (mut window, held) = open(Rgb::BLACK, |value| {
        view! {
            ThemeRoot {
                ColorPicker(value = value, label = "Tint", presets = vec![Rgb::new(0x3b82f6)])
            }
        }
    });
    let trigger = window.by_role_and_label(Role::Button, "Tint").expect("the swatch is announced");
    window.click(trigger).expect("the swatch has a box");
    window.advance(Duration::from_millis(300));
    let hex = window.by_role_and_label(Role::TextInput, "Hex").expect("the picker opened");

    retype(&mut window, hex, "#f80");
    assert_eq!(value(&held), Rgb::new(0xff8800));
    assert_eq!(text_of(&mut window, hex).as_deref(), Some("#ff8800"));

    retype(&mut window, hex, "#12345z");
    assert_eq!(value(&held), Rgb::new(0xff8800));
    assert_eq!(text_of(&mut window, hex).as_deref(), Some("#ff8800"));

    let preset = window.by_role_and_label(Role::Button, "#3b82f6").expect("the preset is announced");
    window.click(preset).expect("the preset has a box");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), Rgb::new(0x3b82f6));
    assert_eq!(text_of(&mut window, hex).as_deref(), Some("#3b82f6"));
}

#[test]
fn a_picker_whose_caller_refuses_a_color_goes_on_showing_the_one_it_has() {
    let (mut window, held) = open(Rgb::BLACK, |value| {
        let refused = Binding::controlled(value, |_: Rgb| {});
        view! {
            ThemeRoot {
                ColorPicker(value = refused, label = "Tint")
            }
        }
    });
    let trigger = window.by_role_and_label(Role::Button, "Tint").expect("the swatch is announced");
    window.click(trigger).expect("the swatch has a box");
    window.advance(Duration::from_millis(300));
    let area = window.by_role_and_label(Role::Slider, "Saturation and brightness").expect("the picker opened");

    window.click(area).expect("the area has a box");
    window.advance(Duration::from_millis(16));

    assert_eq!(value(&held), Rgb::BLACK);
    assert_eq!(text_of(&mut window, area).as_deref(), Some("0%, 0%"));
}

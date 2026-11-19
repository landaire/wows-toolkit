//! The value inputs in a real document, typed into and stepped over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::CheckboxFieldProps;
use landaire_ui::ColorPickerProps;
use landaire_ui::EscapeClears;
use landaire_ui::FocusText;
use landaire_ui::NumberInputProps;
use landaire_ui::NumberPattern;
use landaire_ui::NumberRange;
use landaire_ui::PasswordInputProps;
use landaire_ui::PasswordInputStrings;
use landaire_ui::Rgb;
use landaire_ui::SliderFieldProps;
use landaire_ui::SliderLabelPlacement;
use landaire_ui::SliderRange;
use landaire_ui::SwitchFieldProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::focus_when;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui::vocab::Toggled;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
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
fn the_slider_label_is_drawn_before_the_slider_unless_the_caller_draws_it() {
    let (window, _) = open(0.5, |value| {
        view! {
            ThemeRoot {
                SliderField(value = value, range = SliderRange::new(0.0, 1.0, 0.05), label = "Opacity")
            }
        }
    });
    assert_eq!(window.texts_by_class("lui-slider-field__label"), ["Opacity"]);
    let label = window.by_class("lui-slider-field__label").expect("the label is in the document");
    let slider = window.by_class("lui-slider-field__slider").expect("the slider is in the document");
    let (label, slider) = (window.bounds(label).expect("a box"), window.bounds(slider).expect("a box"));
    assert!(label.size.width.0 > 0.0, "the label takes room: {label:?}");
    assert!(label.origin.x.0 + label.size.width.0 <= slider.origin.x.0, "{label:?} before {slider:?}");
    drop(window);

    let (window, _) = open(0.5, |value| {
        view! {
            ThemeRoot {
                SliderField(
                    value = value,
                    range = SliderRange::new(0.0, 1.0, 0.05),
                    label = "Opacity",
                    label_placement = SliderLabelPlacement::Outside
                )
            }
        }
    });
    assert!(window.texts_by_class("lui-slider-field__label").is_empty());
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

/// Escape in a filter field inside a popover empties the field and leaves the popover open, and
/// Escape in the empty field closes the popover.
#[test]
fn escape_empties_a_filter_field_before_it_closes_the_popover_around_it() {
    let shown = Rc::new(Cell::new(None));
    let (mut window, held) = {
        let shown = Rc::clone(&shown);
        open(String::new(), move |value| {
            let open = RwSignal::new_local(true);
            shown.set(Some(open));
            let escape = EscapeClears::new(value);
            view! {
                ThemeRoot {
                    Popover(open = open) {
                        PopoverTrigger {"Ships"}
                        PopoverContent(dismiss_on_escape = escape.dismiss_on_escape()) {
                            Input(
                                node_ref = escape.node_ref(),
                                value = value,
                                label = "Filter",
                                on:key_down = move |ev| {
                                    escape.key_down(ev);
                                }
                            )
                        }
                    }
                }
            }
        })
    };
    let open = shown.get().expect("the view was built");
    window.advance(Duration::from_millis(300));
    let field = window.by_role_and_label(Role::TextInput, "Filter").expect("the popover is open");
    window.click(field).expect("the field has a box");
    window.type_text("yam");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), "yam");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), "", "Escape empties the field");
    assert!(open.get_untracked(), "and leaves the popover open");
    window.type_text("k");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), "k", "the field keeps focus");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(300));
    assert!(!open.get_untracked(), "Escape in the empty field closes the popover");
}

#[test]
fn a_press_on_a_switch_fields_label_turns_the_switch() {
    let (mut window, held) = open(false, |on| {
        view! {
            ThemeRoot {
                SwitchField(checked = on, label = "Sound")
            }
        }
    });
    let switch = window.by_role_and_label(Role::Switch, "Sound").expect("the label names the switch");
    let label = window.by_class("lui-toggle-field__label").expect("the label is in the document");

    window.click(label).expect("the label has a box");
    window.advance(Duration::from_millis(16));
    assert!(value(&held));
    assert_eq!(window.a11y(switch).and_then(|state| state.checked), Some(Toggled::True));

    window.click(label).expect("the label has a box");
    window.advance(Duration::from_millis(16));
    assert!(!value(&held));
    assert_eq!(window.a11y(switch).and_then(|state| state.checked), Some(Toggled::False));
}

#[test]
fn a_press_on_a_fields_control_turns_it_once() {
    let (mut window, held) = open(false, |on| {
        view! {
            ThemeRoot {
                SwitchField(checked = on, label = "Sound")
            }
        }
    });
    let switch = window.by_role_and_label(Role::Switch, "Sound").expect("the label names the switch");

    window.click(switch).expect("the switch has a box");
    window.advance(Duration::from_millis(16));
    assert!(value(&held));

    window.click(switch).expect("the switch has a box");
    window.advance(Duration::from_millis(16));
    assert!(!value(&held));
}

#[test]
fn a_press_on_a_checkbox_fields_label_ticks_the_checkbox_and_a_mixed_one_becomes_ticked() {
    let (mut window, held) = open(Checked::Mixed, |checked| {
        view! {
            ThemeRoot {
                CheckboxField(checked = checked, label = "Every ship")
            }
        }
    });
    let checkbox = window.by_role_and_label(Role::CheckBox, "Every ship").expect("the label names the checkbox");
    let label = window.by_class("lui-toggle-field__label").expect("the label is in the document");

    window.click(label).expect("the label has a box");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), Checked::Yes);
    assert_eq!(window.a11y(checkbox).and_then(|state| state.checked), Some(Toggled::True));

    window.click(label).expect("the label has a box");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), Checked::No);
}

#[test]
fn a_press_on_a_disabled_fields_label_or_control_changes_nothing() {
    let (mut window, held) = open(false, |on| {
        view! {
            ThemeRoot {
                SwitchField(checked = on, disabled = true, label = "Sound")
                CheckboxField(default_checked = Checked::No, disabled = true, label = "Every ship")
            }
        }
    });
    let switch = window.by_role_and_label(Role::Switch, "Sound").expect("the label names the switch");
    let checkbox = window.by_role_and_label(Role::CheckBox, "Every ship").expect("the label names the checkbox");
    let labels = window.all_by_class("lui-toggle-field__label");
    assert_eq!(labels.len(), 2);

    for element in labels.into_iter().chain([switch, checkbox]) {
        window.click(element).expect("the element has a box");
    }
    window.advance(Duration::from_millis(16));

    assert!(!value(&held));
    assert_eq!(window.a11y(switch).and_then(|state| state.checked), Some(Toggled::False));
    assert_eq!(window.a11y(checkbox).and_then(|state| state.checked), Some(Toggled::False));
}

#[test]
fn the_eye_button_shows_the_password_and_masks_it_again() {
    let (mut window, _) = open("hunter2".to_owned(), |value| {
        view! {
            ThemeRoot {
                PasswordInput(value = value, a11y:label = "Password")
            }
        }
    });
    let field = window.by_label("Password").expect("the field takes the forwarded name");
    let masked = window.a11y(field).expect("the field is announced");
    assert_eq!(masked.role, Role::PasswordInput);
    assert_ne!(masked.value.as_deref(), Some("hunter2"), "a masked field keeps its text from a reader");

    let show = window.by_role_and_label(Role::Button, "Show password").expect("the button is announced");
    window.click(show).expect("the button has a box");
    window.advance(Duration::from_millis(16));
    let shown = window.a11y(field).expect("the field is announced");
    assert_eq!(shown.role, Role::TextInput);
    assert_eq!(shown.value.as_deref(), Some("hunter2"));
    let hide = window.by_role_and_label(Role::Button, "Hide password").expect("the button is renamed");
    let pressed = window.a11y(hide).and_then(|state| state.checked);
    assert_eq!(pressed, None, "the name says what a press does, so the button reports no pressed state");

    window.click(hide).expect("the button has a box");
    window.advance(Duration::from_millis(16));
    assert_eq!(window.a11y(field).map(|state| state.role), Some(Role::PasswordInput));
}

#[test]
fn the_eye_button_is_named_in_the_callers_words() {
    let (mut window, _) = open(String::new(), |value| {
        let strings = PasswordInputStrings { show: "Afficher".into(), hide: "Masquer".into() };
        view! {
            ThemeRoot {
                PasswordInput(value = value, strings = strings)
            }
        }
    });
    let show = window.by_role_and_label(Role::Button, "Afficher").expect("the button is announced");

    window.click(show).expect("the button has a box");
    window.advance(Duration::from_millis(16));

    assert!(window.by_role_and_label(Role::Button, "Masquer").is_some());
}

/// The field is focused with its text selected as soon as it is built, and again each time the
/// request is made after the focus left it.
#[test]
fn focus_when_focuses_a_field_once_it_is_built_and_again_on_request() {
    let asked = Rc::new(Cell::new(None));
    let taken = Rc::new(Cell::new(0_u32));
    let (mut window, held) = {
        let asked = Rc::clone(&asked);
        let taken = Rc::clone(&taken);
        open("Yamato".to_owned(), move |value| {
            let wanted = RwSignal::new_local(true);
            asked.set(Some(wanted));
            let field = NodeRef::new();
            let taken = Rc::clone(&taken);
            focus_when(
                field,
                move || wanted.get(),
                FocusText::SelectAll,
                move || {
                    taken.set(taken.get() + 1);
                    wanted.set(false);
                },
            );

            view! {
                ThemeRoot {
                    Input(node_ref = field, value = value, label = "Name")
                    Input(label = "Other")
                }
            }
        })
    };
    let wanted = asked.get().expect("the view was built");
    window.advance(Duration::from_millis(16));
    let name = window.by_role_and_label(Role::TextInput, "Name").expect("the field is announced");
    assert!(window.a11y(name).is_some_and(|state| state.focused), "the field is focused once built");
    assert_eq!(taken.get(), 1);

    window.type_text("M");
    window.advance(Duration::from_millis(16));
    assert_eq!(value(&held), "M", "the text was selected, so typing replaced it");

    let other = window.by_role_and_label(Role::TextInput, "Other").expect("the field is announced");
    window.click(other).expect("the field has a box");
    window.advance(Duration::from_millis(16));
    assert!(window.a11y(name).is_some_and(|state| !state.focused));

    wanted.set(true);
    window.advance(Duration::from_millis(16));
    assert!(window.a11y(name).is_some_and(|state| state.focused), "a new request moves the focus back");
    assert_eq!(taken.get(), 2);
}

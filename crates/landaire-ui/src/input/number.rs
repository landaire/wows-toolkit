//! A field for one number, typed or stepped.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::InputProps;
use zgui_ui::support::activate_on_press_keeping_focus;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::mark::MINUS;
use zgui_ui_icons::set::mark::PLUS;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Controllable;

/// What the field rules are installed under.
const SHEET: &str = "landaire-ui-number";

const CSS: &str = css!(
    ".lui-number {
        display: flex;
        flex-direction: row;
        align-items: stretch;
        box-sizing: border-box;
        height: 26px;
        min-width: 0;
        overflow: hidden;
        border: 1px solid var(--zui-color-input);
        border-radius: var(--lui-radius);
        color: var(--lui-color-text);
        font-family: var(--lui-font-family);
        font-size: var(--lui-font-size);
        transition: border-color var(--lui-motion-fade) var(--lui-motion-ease);
    }
    .lui-number:focus-within { border-color: var(--zui-color-ring); }
    .lui-number.lui-number-invalid { border-color: var(--zui-color-destructive); }
    .lui-number.lui-number-disabled { opacity: 0.5; pointer-events: none; }
    .lui-number__step {
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 24px;
        color: var(--lui-color-text);
        background-color: transparent;
    }
    .lui-number__step:hover { background-color: color-mix(in srgb, var(--lui-color-border) 40%, transparent); }
    .lui-number__step:active { background-color: color-mix(in srgb, var(--lui-color-border) 60%, transparent); }
    .lui-number .lui-number__field.lui-number__field {
        flex: 1 1 auto;
        height: auto;
        min-width: 0;
        padding: 0 4px;
        border: none;
        border-radius: 0;
        box-shadow: none;
        justify-content: center;
        text-align: center;
        font-variant-numeric: tabular-nums;
    }
    .lui-number .lui-number__field.lui-number__field:focus-visible,
    .lui-number .lui-number__field.lui-number__field:invalid:focus-visible { box-shadow: none; }"
);

/// The values a [`NumberInput`] accepts and how far a step moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberRange {
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
}

impl NumberRange {
    /// Any finite number, stepped by `step`.
    ///
    /// # Panics
    ///
    /// Panics unless `step` is finite and positive, at compile time in a constant.
    pub const fn new(step: f64) -> Self {
        assert!(step.is_finite() && step > 0.0, "a number step is a positive amount");

        Self { min: None, max: None, step }
    }

    /// # Panics
    ///
    /// Panics unless `min` is finite and not above the maximum.
    pub const fn with_min(self, min: f64) -> Self {
        assert!(min.is_finite(), "a number range starts at a finite value");
        if let Some(max) = self.max {
            assert!(min <= max, "a number range runs from a lower to a higher value");
        }

        Self { min: Some(min), ..self }
    }

    /// # Panics
    ///
    /// Panics unless `max` is finite and not below the minimum.
    pub const fn with_max(self, max: f64) -> Self {
        assert!(max.is_finite(), "a number range ends at a finite value");
        if let Some(min) = self.min {
            assert!(min <= max, "a number range runs from a lower to a higher value");
        }

        Self { max: Some(max), ..self }
    }

    pub const fn min(self) -> Option<f64> {
        self.min
    }

    pub const fn max(self) -> Option<f64> {
        self.max
    }

    pub const fn step(self) -> f64 {
        self.step
    }

    /// `value` moved inside the range.
    pub fn clamp(self, value: f64) -> f64 {
        let value = self.min.map_or(value, |min| value.max(min));

        self.max.map_or(value, |max| value.min(max))
    }

    pub fn contains(self, value: f64) -> bool {
        self.clamp(value) == value
    }

    /// One step from `from` in `direction`, inside the range. `None` when the step would not move
    /// the value that way, as at the end of the range or from outside it on the far side.
    pub fn stepped(self, from: f64, direction: StepDirection) -> Option<f64> {
        // Rounded to the places the step and the start carry, so 0.1 + 0.2 lands on 0.3.
        let places = decimals_of(self.step).max(decimals_of(from));
        let raw = match direction {
            StepDirection::Up => from + self.step,
            StepDirection::Down => from - self.step,
        };
        let next = self.clamp(round_to(raw, places));
        let moved = match direction {
            StepDirection::Up => next > from,
            StepDirection::Down => next < from,
        };

        moved.then_some(next)
    }
}

/// Which way a step moves a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepDirection {
    Up,
    Down,
}

/// What a [`NumberInput`] lets be typed into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NumberPattern {
    /// A signed number with a decimal point or a decimal comma.
    #[default]
    Decimal,
    /// The digits 0 to 9 and nothing else: a count.
    Digits,
}

impl NumberPattern {
    /// Whether `text` may be typed at all. A key that would insert anything else is refused.
    fn admits(self, text: &str) -> bool {
        match self {
            Self::Decimal => text.chars().all(|character| matches!(character, '0'..='9' | '.' | ',' | '-' | '+')),
            Self::Digits => text.chars().all(|character| character.is_ascii_digit()),
        }
    }
}

/// What the text in a number field says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberEntry {
    Empty,
    Number(f64),
    /// Text that is not a number under the field's pattern.
    Malformed,
}

impl NumberEntry {
    pub fn parse(text: &str, pattern: NumberPattern) -> Self {
        let text = text.trim();
        if text.is_empty() {
            return Self::Empty;
        }
        if !pattern.admits(text) {
            return Self::Malformed;
        }

        match text.replace(',', ".").parse::<f64>() {
            Ok(number) if number.is_finite() => Self::Number(number),
            _ => Self::Malformed,
        }
    }
}

/// How many places after the point a number is shown with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Decimals(pub u8);

/// Whether a [`NumberInput`] draws its decrement and increment buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StepButtons {
    #[default]
    Shown,
    Hidden,
}

/// A text field for one number in a [`NumberRange`], with a button either side that steps it.
///
/// Typed text is taken on <kbd>Enter</kbd> or when focus leaves: a number is moved inside the range
/// and kept, and anything else puts the last value back. Until then the field is marked invalid
/// while its text is not a number in the range. <kbd>ArrowUp</kbd> and <kbd>ArrowDown</kbd> step
/// from whatever the field shows.
#[component]
pub fn NumberInput(
    /// The value, when the caller holds it.
    #[prop(into, optional)]
    value: Binding<f64>,
    /// What it starts as, when the field owns it.
    #[prop(default = 0.0)]
    default_value: f64,
    /// Told whenever the value changes, whoever owns it.
    #[prop(optional)]
    on_change: Option<UnsyncCallback<f64>>,
    #[prop(default = NumberRange::new(1.0))] range: NumberRange,
    #[prop(default = NumberPattern::Decimal)] pattern: NumberPattern,
    /// Places shown after the point. Left out, a value shows as many as it has.
    #[prop(optional)]
    decimals: Option<Decimals>,
    #[prop(default = StepButtons::Shown)] step_buttons: StepButtons,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    /// What the number is, for a reader.
    #[prop(into, optional)]
    label: Option<String>,
    #[prop(into, default = "Decrease".to_owned())] decrement_label: String,
    #[prop(into, default = "Increase".to_owned())] increment_label: String,
    #[prop(into, optional)] class: Classes,
    /// Forwarded to the frame around the field and its buttons.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let held = Controllable::new(value, range.clamp(default_value), on_change);
    let show = move |number: f64| format_number(number, decimals);
    let text = RwSignal::new_local(show(held.get_untracked()));
    // Set by typing and cleared by a commit or a step: while set, the text is the user's and is not
    // replaced when the value moves.
    let typed = RwSignal::new_local(false);

    let following = RenderEffect::new(move |_| {
        let number = held.get();
        if !typed.get_untracked() {
            text.set(show(number));
        }
    });
    on_cleanup_local(move || drop(following));

    let commit = move || {
        if !typed.get_untracked() {
            return;
        }

        typed.set(false);
        if let NumberEntry::Number(number) = text.with_untracked(|text| NumberEntry::parse(text, pattern)) {
            held.set(range.clamp(number));
        }
        text.set(show(held.get_untracked()));
    };
    let step = move |direction: StepDirection| {
        if disabled.get_untracked() {
            return;
        }

        let from = match text.with_untracked(|text| NumberEntry::parse(text, pattern)) {
            NumberEntry::Number(number) if typed.get_untracked() => number,
            _ => held.get_untracked(),
        };
        typed.set(false);
        if let Some(next) = range.stepped(from, direction) {
            held.set(next);
        }
        text.set(show(held.get_untracked()));
    };
    let invalid = Signal::derive_local(move || {
        text.with(|text| match NumberEntry::parse(text, pattern) {
            NumberEntry::Empty => false,
            NumberEntry::Number(number) => !range.contains(number),
            NumberEntry::Malformed => true,
        })
    });

    let field = view! {
        Input(
            class = "lui-number__field",
            value = text,
            on_change = UnsyncCallback::new(move |_| typed.set(true)),
            invalid = invalid,
            disabled = disabled,
            label = label,
            on:key_down = move |ev| {
                match &ev.key {
                    Key::Named(NamedKey::ArrowUp) => step(StepDirection::Up),
                    Key::Named(NamedKey::ArrowDown) => step(StepDirection::Down),
                    Key::Named(NamedKey::Enter) => {
                        commit();
                        return;
                    }
                    key => {
                        let refused = key.inserted_text().is_some_and(|inserted| !pattern.admits(inserted));
                        let shortcut = ev.modifiers.control() || ev.modifiers.meta() || ev.modifiers.alt();
                        if !refused || shortcut {
                            return;
                        }
                    }
                }
                ev.prevent_default();
            },
            on:focus_out = move |_| commit()
        )
    };

    let buttons = match step_buttons {
        StepButtons::Shown => Some((
            step_button(MINUS, decrement_label, move || step(StepDirection::Down)),
            step_button(PLUS, increment_label, move || step(StepDirection::Up)),
        )),
        StepButtons::Hidden => None,
    };
    let (decrement, increment) = buttons.unzip();

    view! {
        row(
            class = "lui-number",
            class:lui-number-invalid = move || invalid.get(),
            class:lui-number-disabled = move || disabled.get(),
            {..attrs},
            class = class
        ) {
            {decrement}
            {field}
            {increment}
        }
    }
}

/// A stepper beside the field. It takes no focus, so the caret stays where the user is typing.
fn step_button(icon: IconData, label: String, pressed: impl Fn() + 'static) -> AnyView {
    AnyView::new(view! {
        control(
            class = "lui-number__step",
            a11y:role = Role::Button,
            a11y:label = label,
            on:pointer_down = activate_on_press_keeping_focus(),
            on:click = move |_| pressed()
        ) {
            Icon(icon = icon, size = IconSize::Sm)
        }
    })
}

/// `value` with `decimals` places, or with as few as write it exactly.
pub(crate) fn format_number(value: f64, decimals: Option<Decimals>) -> String {
    let places = usize::from(decimals.unwrap_or_else(|| decimals_of(value)).0);

    format!("{value:.places$}")
}

/// The fewest places after the point that write `value`, up to six.
pub(crate) fn decimals_of(value: f64) -> Decimals {
    // Past six places an f64 that came from typed text prints rounding noise.
    const MOST: u8 = 6;

    let places = (0..MOST)
        .find(|&places| {
            let scaled = value * 10f64.powi(i32::from(places));
            (scaled - scaled.round()).abs() <= scaled.abs().max(1.0) * 1e-9
        })
        .unwrap_or(MOST);

    Decimals(places)
}

fn round_to(value: f64, places: Decimals) -> f64 {
    let scale = 10f64.powi(i32::from(places.0));

    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_lands_on_the_decimal_it_should_and_stops_at_the_ends() {
        let range = NumberRange::new(0.1).with_min(0.0).with_max(1.0);

        assert_eq!(range.stepped(0.2, StepDirection::Up), Some(0.3));
        assert_eq!(range.stepped(0.95, StepDirection::Up), Some(1.0));
        assert_eq!(range.stepped(1.0, StepDirection::Up), None);
        assert_eq!(range.stepped(0.0, StepDirection::Down), None);
    }

    #[test]
    fn a_step_from_outside_the_range_never_moves_away_from_it() {
        let range = NumberRange::new(1.0).with_min(10.0).with_max(100.0);

        assert_eq!(range.stepped(5.0, StepDirection::Down), None);
        assert_eq!(range.stepped(5.0, StepDirection::Up), Some(10.0));
        assert_eq!(range.stepped(1000.0, StepDirection::Up), None);
        assert_eq!(range.stepped(1000.0, StepDirection::Down), Some(100.0));
    }

    #[test]
    fn text_is_read_under_the_pattern() {
        assert_eq!(NumberEntry::parse(" 2,5 ", NumberPattern::Decimal), NumberEntry::Number(2.5));
        assert_eq!(NumberEntry::parse("-3", NumberPattern::Decimal), NumberEntry::Number(-3.0));
        assert_eq!(NumberEntry::parse("", NumberPattern::Decimal), NumberEntry::Empty);
        assert_eq!(NumberEntry::parse("1-2", NumberPattern::Decimal), NumberEntry::Malformed);
        assert_eq!(NumberEntry::parse("inf", NumberPattern::Decimal), NumberEntry::Malformed);
        assert_eq!(NumberEntry::parse("12", NumberPattern::Digits), NumberEntry::Number(12.0));
        assert_eq!(NumberEntry::parse("1.5", NumberPattern::Digits), NumberEntry::Malformed);
        assert_eq!(NumberEntry::parse("-1", NumberPattern::Digits), NumberEntry::Malformed);
    }

    #[test]
    fn a_number_is_written_with_the_places_it_has_unless_told_otherwise() {
        assert_eq!(format_number(2.0, None), "2");
        assert_eq!(format_number(0.1 + 0.2, None), "0.3");
        assert_eq!(format_number(2.25, None), "2.25");
        assert_eq!(format_number(0.5, Some(Decimals(2))), "0.50");
    }
}

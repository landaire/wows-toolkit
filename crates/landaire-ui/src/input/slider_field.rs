//! A slider whose value can also be typed, and put back to where it started.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::SliderProps;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Controllable;

use crate::input::number::Decimals;
use crate::input::number::NumberInputProps;
use crate::input::number::NumberRange;
use crate::input::number::StepButtons;
use crate::input::number::decimals_of;
use crate::tooltip::TooltipContentProps;
use crate::tooltip::TooltipProps;
use crate::tooltip::TooltipTriggerProps;

/// What the field rules are installed under.
const SHEET: &str = "landaire-ui-slider-field";

const CSS: &str = css!(
    ".lui-slider-field {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
    }
    .lui-slider-field__slider { flex: 1 1 auto; min-width: 0; }
    .lui-slider-field__number { flex: none; width: 52px; height: 22px; }
    .lui-slider-field__reset.lui-slider-field__reset { flex: none; width: 22px; height: 22px; padding: 0; }"
);

/// A counter-clockwise turn, for putting a value back.
const ROTATE_BACK: IconData = IconData::new(
    "rotate-back",
    24.0,
    "M12 4 C16.418 4 20 7.582 20 12 C20 16.418 16.418 20 12 20 C7.582 20 4 16.418 4 12 L6 12 \
     C6 15.314 8.686 18 12 18 C15.314 18 18 15.314 18 12 C18 8.686 15.314 6 12 6 Z \
     M12 1.5 L12 8.5 L7.5 5 Z",
);

/// The values a [`SliderField`] can take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderRange {
    min: f64,
    max: f64,
    step: f64,
}

impl SliderRange {
    /// # Panics
    ///
    /// Panics unless `min < max` and `step` is positive, all three finite, at compile time in a
    /// constant.
    pub const fn new(min: f64, max: f64, step: f64) -> Self {
        assert!(min.is_finite() && max.is_finite() && min < max, "a slider range runs from a lower to a higher value");
        assert!(step.is_finite() && step > 0.0, "a slider step is a positive amount");

        Self { min, max, step }
    }

    pub const fn min(self) -> f64 {
        self.min
    }

    pub const fn max(self) -> f64 {
        self.max
    }

    pub const fn step(self) -> f64 {
        self.step
    }

    /// `value` inside the range and on a step from its start.
    pub fn snap(self, value: f64) -> f64 {
        let stepped = self.min + ((value - self.min) / self.step).round() * self.step;
        let scale = 10f64.powi(i32::from(self.decimals().0));

        ((stepped * scale).round() / scale).clamp(self.min, self.max)
    }

    /// As many places as the step or the start carries, so the text never rounds off a value the
    /// slider can reach.
    pub fn decimals(self) -> Decimals {
        decimals_of(self.step).max(decimals_of(self.min))
    }

    fn number_range(self) -> NumberRange {
        NumberRange::new(self.step).with_min(self.min).with_max(self.max)
    }
}

/// A slider, the number it is at as a field that can be typed into, and optionally a button that
/// puts the value back.
///
/// The slider and the field show one value: a drag or an arrow key rewrites the field, and a typed
/// number moves the slider when it is taken, snapped to the slider's steps.
#[component]
pub fn SliderField(
    /// The value, when the caller holds it.
    #[prop(into, optional)]
    value: Binding<f64>,
    /// What it starts as, when the field owns it.
    #[prop(default = 0.0)]
    default_value: f64,
    /// Told whenever the value changes, whoever owns it.
    #[prop(optional)]
    on_change: Option<UnsyncCallback<f64>>,
    range: SliderRange,
    /// Where the reset button puts the value. Left out, there is no reset button.
    #[prop(optional)]
    reset_to: Option<f64>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    /// What the value measures, for a reader. Names both the slider and the field.
    #[prop(into)]
    label: String,
    #[prop(into, default = "Reset".to_owned())] reset_label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    // The seed is not snapped: a value nobody touched is reported back only once somebody does.
    let held = Controllable::new(value, default_value, on_change);
    let write = move |next: f64| held.set(range.snap(next));
    let to_slider = Binding::controlled(held.signal(), write);
    let to_number = Binding::controlled(held.signal(), write);

    let reset_button = reset_to.map(|reset_to| {
        let tip = reset_label.clone();
        let at_reset = Signal::derive_local(move || disabled.get() || range.snap(reset_to) == range.snap(held.get()));
        AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    Button(
                        class = "lui-slider-field__reset",
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        disabled = at_reset,
                        a11y:label = reset_label,
                        on:click = move |_| write(reset_to)
                    ) {
                        Icon(icon = ROTATE_BACK, size = IconSize::Sm)
                    }
                }
                TooltipContent {{tip.clone()}}
            }
        })
    });

    let number = view! {
        NumberInput(
            class = "lui-slider-field__number",
            value = to_number,
            range = range.number_range(),
            decimals = range.decimals(),
            step_buttons = StepButtons::Hidden,
            disabled = disabled,
            label = label.clone()
        )
    };

    let slider = view! {
        Slider(
            class = "lui-slider-field__slider",
            value = to_slider,
            min = range.min,
            max = range.max,
            step = range.step,
            disabled = disabled,
            label = label
        )
    };

    view! {
        row(class = "lui-slider-field", {..attrs}, class = class) {
            {slider}
            {number}
            {reset_button}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_snaps_to_the_range_and_its_steps() {
        let opacity = SliderRange::new(0.05, 1.0, 0.01);
        assert_eq!(opacity.snap(0.333), 0.33);
        assert_eq!(opacity.snap(4.0), 1.0);
        assert_eq!(opacity.snap(-1.0), 0.05);

        let roll = SliderRange::new(-15.0, 15.0, 0.5);
        assert_eq!(roll.snap(3.3), 3.5);
        assert_eq!(roll.decimals(), Decimals(1));
    }

    #[test]
    #[should_panic(expected = "a slider step is a positive amount")]
    fn a_range_with_no_step_is_refused() {
        SliderRange::new(0.0, 1.0, 0.0);
    }
}

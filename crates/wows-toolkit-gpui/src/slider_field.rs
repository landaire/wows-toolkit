//! A slider whose value can also be typed in, and put back to its default.
//!
//! The slider is drawn by whoever lays out the row (see [`SliderField::slider`]);
//! this view draws the number beside it and the reset button.

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::EventEmitter;
use gpui_kit::Focusable as _;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Render;
use gpui_kit::Styled as _;
use gpui_kit::Subscription;
use gpui_kit::Window;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::SliderEvent;
use gpui_kit::component::slider::SliderState;
use gpui_kit::px;
use rust_i18n::t;

/// The values a [`SliderField`] can take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderRange {
    min: f32,
    max: f32,
    step: f32,
}

impl SliderRange {
    /// Panics unless `min < max` and `step` is positive, all three finite.
    /// Every range is a literal, so build one in a `const` block and a bad
    /// range fails the build.
    pub const fn new(min: f32, max: f32, step: f32) -> Self {
        assert!(min.is_finite() && max.is_finite() && min < max, "a slider range runs from a lower to a higher value");
        assert!(step.is_finite() && step > 0.0, "a slider step is a positive amount");
        Self { min, max, step }
    }

    /// `value` inside the range and on a step from its start.
    fn snap(self, value: f32) -> f32 {
        let stepped = self.min + ((value - self.min) / self.step).round() * self.step;
        stepped.clamp(self.min, self.max)
    }

    /// How many decimals a value on the steps needs: as many as the step or
    /// the start of the range carries, so the text never rounds off a value
    /// the slider can reach.
    fn decimals(self) -> usize {
        decimals_of(self.step).max(decimals_of(self.min))
    }
}

/// The value moved, by the slider, the number field or the reset button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderFieldChange(pub f32);

pub struct SliderField {
    range: SliderRange,
    slider: Entity<SliderState>,
    /// Made on the first draw, which is where a window first is: an input
    /// cannot be built without one.
    number: Option<Entity<InputState>>,
    /// The reader has typed into the number since it was last committed, so
    /// the text is theirs and is not replaced by the slider's value.
    editing: bool,
    reset_to: f32,
    disabled: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SliderFieldChange> for SliderField {}

impl SliderField {
    pub fn new(range: SliderRange, value: f32, reset_to: f32, cx: &mut Context<Self>) -> Self {
        // The seed is not snapped: the viewport draws the stored value, and
        // reporting a snapped one would rewrite settings nobody touched.
        let slider = cx.new(|_| SliderState::new().min(range.min).max(range.max).step(range.step).default_value(value));
        let dragged = cx.subscribe(&slider, |this, _slider, event: &SliderEvent, cx| {
            if let SliderEvent::Change(value) = event {
                // A drag replaces whatever was typed and not yet committed.
                this.editing = false;
                cx.emit(SliderFieldChange(value.end()));
                cx.notify();
            }
        });
        Self { range, slider, number: None, editing: false, reset_to, disabled: false, _subscriptions: vec![dragged] }
    }

    /// The slider, for the row that draws it.
    pub fn slider(&self) -> &Entity<SliderState> {
        &self.slider
    }

    pub fn value(&self, cx: &App) -> f32 {
        self.slider.read(cx).value().end()
    }

    /// Greys the number and the reset out along with the slider.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    fn text_for(&self, value: f32) -> String {
        format!("{value:.*}", self.range.decimals())
    }

    /// Moves the slider to `value` and reports it, as a drag would. A value
    /// that snaps to the current one only rewrites the text. Either way the
    /// text is replaced, so nothing typed is left to commit.
    fn set(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.range.snap(value);
        self.editing = false;
        if let Some(number) = &self.number {
            let text = self.text_for(value);
            number.update(cx, |number, cx| number.set_value(text, window, cx));
        }

        if value == self.value(cx) {
            return;
        }

        self.slider.update(cx, |slider, cx| slider.set_value(value, window, cx));
        cx.emit(SliderFieldChange(value));
        cx.notify();
    }

    /// Takes what was typed, or puts the shown value back if it is not a
    /// number. Either decimal mark is read.
    fn commit_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(number) = self.number.clone() else { return };

        self.editing = false;
        let typed = number.read(cx).value().trim().replace(',', ".");
        match typed.parse::<f32>() {
            Ok(value) if value.is_finite() => self.set(value, window, cx),
            _ => {
                let text = self.text_for(self.value(cx));
                number.update(cx, |number, cx| number.set_value(text, window, cx));
            }
        }
    }

    fn number(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(number) = &self.number {
            return number.clone();
        }

        let text = self.text_for(self.value(cx));
        let number = cx.new(|cx| InputState::new(window, cx).default_value(text));
        // `set_value` emits no `Change`, so a `Change` is always the reader's.
        let typed = cx.subscribe_in(&number, window, |this, _number, event: &InputEvent, window, cx| match event {
            InputEvent::Change => this.editing = true,
            InputEvent::PressEnter { .. } | InputEvent::Blur if this.editing => this.commit_typed(window, cx),
            _ => {}
        });
        self._subscriptions.push(typed);
        self.number = Some(number.clone());
        number
    }
}

impl Render for SliderField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let number = self.number(window, cx);
        let value = self.value(cx);

        // Follows the slider, except while the reader is in the field or has
        // typed a value not yet committed: focus leaves before the blur that
        // commits the text arrives.
        let shown = self.text_for(value);
        let focused = number.read(cx).focus_handle(cx).is_focused(window);
        if !self.editing && !focused && number.read(cx).value() != shown.as_str() {
            number.update(cx, |number, cx| number.set_value(shown, window, cx));
        }
        let at_default = self.range.snap(self.reset_to) == self.range.snap(value);

        h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .child(Input::new(&number).id("slider-field-number").xsmall().w(px(52.)).disabled(self.disabled))
            .child(
                Button::new("slider-field-reset")
                    .icon(crate::icons::Glyph::RotateCcw)
                    .ghost()
                    .xsmall()
                    .tooltip(t!("ui.buttons.reset").to_string())
                    .disabled(self.disabled || at_default)
                    .on_click(cx.listener(|this, _event, window, cx| this.set(this.reset_to, window, cx))),
            )
    }
}

/// The fewest decimals that write `value` to within `f32` precision.
fn decimals_of(value: f32) -> usize {
    // An f32 carries about seven significant digits, so more places than
    // this only print noise.
    const MOST: usize = 6;

    let value = f64::from(value);
    (0..MOST)
        .find(|&places| {
            let scaled = value * 10f64.powi(places as i32);
            (scaled - scaled.round()).abs() <= scaled.abs() * 1e-6
        })
        .unwrap_or(MOST)
}

#[cfg(test)]
mod tests {
    use super::SliderRange;

    /// A typed value lands inside the range and on the slider's own steps, so
    /// the number shown and the thumb agree.
    #[test]
    fn a_typed_value_snaps_to_the_range_and_its_steps() {
        let opacity = SliderRange::new(0.05, 1.0, 0.01);
        assert!((opacity.snap(0.333) - 0.33).abs() < 1e-5);
        assert_eq!(opacity.snap(4.0), 1.0);
        assert_eq!(opacity.snap(-1.0), 0.05);

        let roll = SliderRange::new(-15.0, 15.0, 0.5);
        assert_eq!(roll.snap(3.3), 3.5);
        assert_eq!(roll.decimals(), 1);
    }

    /// Every value a step reaches reads back as itself, so focusing the
    /// number and committing it unchanged does not move the slider.
    #[test]
    fn the_shown_text_reads_back_as_the_value_on_every_step() {
        for range in [
            SliderRange::new(0.0, 1.0, 0.05),
            SliderRange::new(0.0, 2.0, 0.25),
            SliderRange::new(0.05, 1.0, 0.1),
            SliderRange::new(0.0, 360.0, 1.0),
        ] {
            let steps = ((range.max - range.min) / range.step).round() as usize;
            for step in 0..=steps {
                let value = range.snap(range.min + step as f32 * range.step);
                let text = format!("{value:.*}", range.decimals());
                let read = range.snap(text.parse::<f32>().expect("the shown text is a number"));
                assert_eq!(read, value, "{range:?} shows {value} as {text}");
            }
        }
    }

    #[test]
    #[should_panic(expected = "a slider step is a positive amount")]
    fn a_range_with_no_step_is_refused() {
        SliderRange::new(0.0, 1.0, 0.0);
    }
}

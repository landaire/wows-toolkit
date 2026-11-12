//! A span of values chosen with two thumbs on one track.

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui::vocab::Role;
use zgui::vocab::UiState;
use zgui_ui::slider::Move;
use zgui_ui::slider::key_move;
use zgui_ui::support::Bound;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Controllable;

use crate::input::slider_field::SliderRange;

/// What the range slider rules are installed under.
const SHEET: &str = "landaire-ui-range-slider";

const CSS: &str = css!(
    ".lui-range-slider {
        position: relative;
        display: flex;
        flex-direction: row;
        align-items: center;
        height: 16px;
        width: 100%;
        user-select: none;
    }
    .lui-range-slider__track {
        position: relative;
        height: 6px;
        width: 100%;
        overflow: hidden;
        border-radius: var(--zui-radius-full);
        background-color: var(--zui-color-muted);
    }
    .lui-range-slider__range {
        position: absolute;
        top: 0;
        height: 100%;
        background-color: var(--zui-color-primary);
    }
    .lui-range-slider__thumb {
        position: absolute;
        width: 16px;
        height: 16px;
        margin-left: -8px;
        border: 1px solid var(--zui-color-primary);
        border-radius: var(--zui-radius-full);
        background-color: oklch(1 0 0);
        box-shadow: var(--zui-shadow-sm);
        outline: none;
        transition-property: box-shadow;
        transition-duration: var(--zui-motion-duration-normal);
        transition-timing-function: var(--zui-motion-ease-standard);
    }
    .lui-range-slider__thumb:hover, .lui-range-slider__thumb:focus-visible {
        box-shadow: 0 0 0 4px color-mix(in oklab, var(--zui-color-ring) 50%, transparent), var(--zui-shadow-sm);
    }
    .lui-range-slider:disabled { opacity: 0.5; pointer-events: none; }"
);

/// The two ends of a span on a [`RangeSlider`], the lower first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderSpan {
    low: f64,
    high: f64,
}

impl SliderSpan {
    /// The span between `a` and `b`, in either order.
    pub fn new(a: f64, b: f64) -> Self {
        Self { low: a.min(b), high: a.max(b) }
    }

    pub fn low(self) -> f64 {
        self.low
    }

    pub fn high(self) -> f64 {
        self.high
    }
}

/// One of a [`RangeSlider`]'s thumbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Thumb {
    Low,
    High,
}

impl Thumb {
    /// The thumb a press at `value` moves: the nearer one, or the one on the press's side when
    /// both stand on the same value.
    fn nearer(span: SliderSpan, value: f64) -> Self {
        let to_low = (value - span.low).abs();
        let to_high = (value - span.high).abs();
        if to_high < to_low || (to_high == to_low && value > span.high) { Self::High } else { Self::Low }
    }

    /// `span` with this thumb at `value`. A thumb stops at the other.
    fn moved(self, span: SliderSpan, value: f64) -> SliderSpan {
        match self {
            Self::Low => SliderSpan { low: value.min(span.high), high: span.high },
            Self::High => SliderSpan { low: span.low, high: value.max(span.low) },
        }
    }

    fn value(self, span: SliderSpan) -> f64 {
        match self {
            Self::Low => span.low,
            Self::High => span.high,
        }
    }
}

/// Two thumbs on one track choosing a span of `range`. Neither thumb passes the other.
///
/// `value` follows every step of a drag; `on_release` is told the span once a drag ends or a key
/// has moved a thumb, for work worth doing only once per choice.
///
/// # Keyboard
///
/// Each thumb is a tab stop and moves as a slider's thumb does: the arrow keys one step, page up
/// and page down ten, home and end as far as the other thumb or the range allows.
#[component]
pub fn RangeSlider(
    /// The span, held by the caller.
    #[prop(into)]
    value: Binding<SliderSpan>,
    range: SliderRange,
    #[prop(optional)] on_release: Option<UnsyncCallback<SliderSpan>>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    /// What the lower thumb sets, for a reader.
    #[prop(into)]
    low_label: String,
    /// What the higher thumb sets, for a reader.
    #[prop(into)]
    high_label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let bound = Bound::new(range.min(), range.max(), range.step());
    let held = Controllable::new(value, SliderSpan::new(range.min(), range.max()), None);
    let track = NodeRef::new();
    let dragging: RwSignal<Option<Thumb>, LocalStorage> = RwSignal::new_local(None);
    let release = move || {
        if let Some(on_release) = on_release {
            on_release.run(held.get_untracked());
        }
    };

    // A pointer reports where it is in CSS pixels and in the window; the track is asked for its
    // window box in device pixels, so the scale relates the two.
    let value_at = move |x_css: f32| {
        let bounds = track.window_bounds()?;
        if bounds.size.width.0 <= 0.0 {
            return None;
        }
        let fraction = f64::from((x_css * track.scale() - bounds.origin.x.0) / bounds.size.width.0);

        Some(range.snap(bound.at(fraction)))
    };
    let percent = move |value: f64| Some(format!("{:.4}%", bound.fraction(value) * 100.0));

    let thumb = move |which: Thumb, label: String| {
        let semantics = A11yBinding::new(Role::Slider)
            .label(label)
            .numeric_value(move || which.value(held.get()))
            .disabled(move || disabled.get())
            .step(move |a11y| a11y.numeric_range(bound.min, bound.max).numeric_step(bound.step));
        let own = Attrs::new().a11y_from(semantics);
        let class = match which {
            Thumb::Low => "lui-range-slider__thumb lui-range-slider__thumb--low",
            Thumb::High => "lui-range-slider__thumb lui-range-slider__thumb--high",
        };

        view! {
            control(
                class = class,
                tabindex = {Focus::Sequential},
                style:left = move || percent(which.value(held.get())),
                on:key_down = move |ev| {
                    if disabled.get_untracked() {
                        return;
                    }
                    let Some(asked) = key_move(&ev.key, bound) else { return };
                    let span = held.get_untracked();
                    let next = match asked {
                        Move::By(by) => which.value(span) + by,
                        Move::To(to) => to,
                    };
                    held.set(which.moved(span, range.snap(next)));
                    release();
                    // Claimed, so the same key does not also scroll what holds the slider.
                    ev.prevent_default();
                    ev.stop_propagation();
                },
                {..own}
            )
        }
    };
    let low_thumb = thumb(Thumb::Low, low_label);
    let high_thumb = thumb(Thumb::High, high_label);

    let own = Attrs::new()
        .class_toggle(zgui::view::ClassName::new("lui-range-slider"), true)
        .state(UiState::DISABLED, move || disabled.get());

    view! {
        box(
            on:pointer_down = move |ev| {
                if disabled.get_untracked() {
                    return;
                }
                let Some(value) = value_at(ev.position.x.0) else { return };
                let span = held.get_untracked();
                let which = Thumb::nearer(span, value);
                ev.capture_pointer();
                dragging.set(Some(which));
                held.set(which.moved(span, value));
            },
            on:pointer_move = move |ev| {
                let Some(which) = dragging.get_untracked() else { return };
                if let Some(value) = value_at(ev.position.x.0) {
                    held.set(which.moved(held.get_untracked(), value));
                }
            },
            on:pointer_up = move |ev| {
                ev.release_pointer();
                if dragging.get_untracked().is_some() {
                    dragging.set(None);
                    release();
                }
            },
            on:pointer_cancel = move |ev| {
                ev.release_pointer();
                dragging.set(None);
            },
            {..own},
            {..attrs},
            class = class
        ) {
            box(class = "lui-range-slider__track", node_ref = track) {
                box(
                    class = "lui-range-slider__range",
                    style:left = move || percent(held.get().low),
                    style:width = move || {
                        let span = held.get();
                        Some(format!("{:.4}%", (bound.fraction(span.high) - bound.fraction(span.low)) * 100.0))
                    }
                )
            }
            {low_thumb}
            {high_thumb}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_moves_the_nearer_thumb_and_a_thumb_stops_at_the_other() {
        let span = SliderSpan::new(8.0, 3.0);
        assert_eq!((span.low(), span.high()), (3.0, 8.0), "the ends are put in order");

        assert_eq!(Thumb::nearer(span, 4.0), Thumb::Low);
        assert_eq!(Thumb::nearer(span, 7.0), Thumb::High);
        assert_eq!(Thumb::nearer(SliderSpan::new(5.0, 5.0), 9.0), Thumb::High, "a press past both");
        assert_eq!(Thumb::nearer(SliderSpan::new(5.0, 5.0), 1.0), Thumb::Low);

        assert_eq!(Thumb::Low.moved(span, 10.0), SliderSpan::new(8.0, 8.0));
        assert_eq!(Thumb::High.moved(span, 1.0), SliderSpan::new(3.0, 3.0));
    }
}

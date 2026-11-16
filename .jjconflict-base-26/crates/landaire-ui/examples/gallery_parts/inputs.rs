//! The gallery's inputs section: a number input, a slider field and a color picker.

use landaire_ui::ColorPickerProps;
use landaire_ui::NumberInputProps;
use landaire_ui::NumberPattern;
use landaire_ui::NumberRange;
use landaire_ui::Rgb;
use landaire_ui::SliderFieldProps;
use landaire_ui::SliderRange;
use landaire_ui::test_support::WindowSize;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;

use crate::Section;

pub const SECTION: Section =
    Section { name: "inputs", title: "Inputs", page: WindowSize { width: 1280.0, height: 220.0 }, view };

const SHEET: &str = css!(
    ".gallery-inputs { display: flex; flex-direction: column; gap: 8px; width: 300px; }
     .gallery-inputs__row { display: flex; flex-direction: row; gap: 8px; align-items: center; }
     .gallery-inputs__label { width: 56px; flex: none; color: var(--lui-color-text-dim); }
     .gallery-inputs__number { width: 110px; }
     .gallery-inputs__slider { flex: 1 1 auto; }"
);

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-inputs", SHEET);

    let count = RwSignal::new_local(25.0);
    let opacity = RwSignal::new_local(0.6);
    let tint = RwSignal::new_local(Rgb::new(0x3b82f6));
    let presets = [0xe5484d, 0xf76b15, 0xffc53d, 0x46a758, 0x3b82f6, 0x8e4ec6, 0xffffff, 0x000000]
        .into_iter()
        .map(Rgb::new)
        .collect::<Vec<_>>();

    AnyView::new(view! {
        column(class = "gallery-inputs") {
            row(class = "gallery-inputs__row") {
                text(class = "gallery-inputs__label") {"Count"}
                NumberInput(
                    class = "gallery-inputs__number",
                    value = count,
                    range = NumberRange::new(1.0).with_min(0.0).with_max(100.0),
                    pattern = NumberPattern::Digits,
                    label = "Count"
                )
            }
            row(class = "gallery-inputs__row") {
                SliderField(
                    class = "gallery-inputs__slider",
                    value = opacity,
                    range = SliderRange::new(0.0, 1.0, 0.05),
                    reset_to = 1.0,
                    label = "Opacity"
                )
            }
            row(class = "gallery-inputs__row") {
                text(class = "gallery-inputs__label") {"Tint"}
                ColorPicker(value = tint, presets = presets, label = "Tint")
            }
        }
    })
}

//! A swatch that opens a picker for an opaque color.

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SliderProps;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Controllable;

use crate::color::Opacity;
use crate::color::Rgb;

/// What the picker rules are installed under.
const SHEET: &str = "landaire-ui-color-picker";

// The hue strip and the shading over the saturation and brightness area are the color model's own
// colors, the same in every theme.
const CSS: &str = css!(
    ".lui-color__trigger.lui-color__trigger { width: 24px; height: 24px; padding: 0; }
    .lui-color__swatch {
        width: 14px;
        height: 14px;
        border: 1px solid var(--lui-color-border-bright);
        border-radius: var(--lui-radius);
    }
    .lui-color__popover.lui-color__popover { width: 240px; padding: 10px; }
    .lui-color__panel { display: flex; flex-direction: column; gap: 8px; }
    .lui-color__area {
        position: relative;
        height: 150px;
        border-radius: var(--lui-radius);
        outline: none;
        user-select: none;
    }
    .lui-color__area:focus-visible { box-shadow: 0 0 0 2px var(--zui-color-ring); }
    .lui-color__thumb {
        position: absolute;
        width: 12px;
        height: 12px;
        margin-left: -6px;
        margin-top: -6px;
        box-sizing: border-box;
        border: 2px solid rgb(255 255 255);
        border-radius: 999px;
        box-shadow: 0 0 0 1px rgb(0 0 0 / 0.45);
        pointer-events: none;
    }
    .lui-color__hue.lui-color__hue > .zui-slider__track {
        height: 10px;
        border-radius: var(--lui-radius);
        background-color: transparent;
        background-image: linear-gradient(to right, rgb(255 0 0), rgb(255 255 0), rgb(0 255 0),
            rgb(0 255 255), rgb(0 0 255), rgb(255 0 255), rgb(255 0 0));
    }
    .lui-color__hue.lui-color__hue > .zui-slider__track > .zui-slider__range { display: none; }
    .lui-color__hue.lui-color__hue > .zui-slider__thumb {
        width: 8px;
        height: 14px;
        margin-left: -4px;
        border: 2px solid rgb(255 255 255);
        border-radius: var(--lui-radius);
        background-color: transparent;
        box-shadow: 0 0 0 1px rgb(0 0 0 / 0.45);
    }
    .lui-color__entry { display: flex; flex-direction: row; align-items: center; gap: 8px; }
    .lui-color__preview {
        flex: none;
        width: 26px;
        height: 26px;
        box-sizing: border-box;
        border: 1px solid var(--lui-color-border-bright);
        border-radius: var(--lui-radius);
    }
    .lui-color__entry .lui-color__hex.lui-color__hex {
        height: 26px;
        padding: 0 8px;
        font-family: var(--lui-font-family-mono);
    }
    .lui-color__presets { display: flex; flex-direction: row; flex-wrap: wrap; gap: 4px; }
    .lui-color__preset {
        width: 20px;
        height: 20px;
        box-sizing: border-box;
        border: 1px solid var(--lui-color-border-bright);
        border-radius: var(--lui-radius);
        outline: none;
    }
    .lui-color__preset:hover { border-color: var(--lui-color-text-dim); }
    .lui-color__preset:focus-visible { box-shadow: 0 0 0 2px var(--zui-color-ring); }
    .lui-color__preset-selected.lui-color__preset-selected { border: 2px solid var(--lui-color-text-bright); }"
);

/// Hue, saturation and brightness: the coordinates of the picker's area and strip.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hsv {
    /// From 0 to 360.
    hue_degrees: f64,
    /// From 0 to 1.
    saturation: f64,
    /// From 0 to 1.
    brightness: f64,
}

impl Hsv {
    fn from_rgb(rgb: Rgb) -> Self {
        let red = f64::from(rgb.red()) / 255.0;
        let green = f64::from(rgb.green()) / 255.0;
        let blue = f64::from(rgb.blue()) / 255.0;
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        let chroma = max - min;

        let sector = if chroma == 0.0 {
            0.0
        } else if max == red {
            ((green - blue) / chroma).rem_euclid(6.0)
        } else if max == green {
            (blue - red) / chroma + 2.0
        } else {
            (red - green) / chroma + 4.0
        };
        let saturation = if max == 0.0 { 0.0 } else { chroma / max };

        Self { hue_degrees: sector * 60.0, saturation, brightness: max }
    }

    /// `rgb`, keeping this hue where `rgb` has none of its own to say.
    fn follow(self, rgb: Rgb) -> Self {
        let next = Self::from_rgb(rgb);
        if next.saturation == 0.0 || next.brightness == 0.0 {
            return Self { hue_degrees: self.hue_degrees, ..next };
        }

        next
    }

    fn to_rgb(self) -> Rgb {
        let chroma = self.brightness * self.saturation;
        let sector = (self.hue_degrees / 60.0).rem_euclid(6.0);
        let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (red, green, blue) = match sector as u8 {
            0 => (chroma, second, 0.0),
            1 => (second, chroma, 0.0),
            2 => (0.0, chroma, second),
            3 => (0.0, second, chroma),
            4 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let offset = self.brightness - chroma;
        let channel = |value: f64| ((value + offset).clamp(0.0, 1.0) * 255.0).round() as u8;

        Rgb::from_channels(channel(red), channel(green), channel(blue))
    }

    /// The fully saturated, fully bright color of this hue.
    fn pure_hue(self) -> Rgb {
        Self { saturation: 1.0, brightness: 1.0, ..self }.to_rgb()
    }
}

/// A button showing a color that opens a picker for it: an area for saturation and brightness, a
/// strip for hue, the color in hex, and optional preset swatches.
///
/// The area and the strip are operated with the arrow keys as well as the pointer, with
/// <kbd>Shift</kbd> for larger steps. The hex field takes `#rrggbb` or `#rgb` on <kbd>Enter</kbd>
/// or when focus leaves, and puts the color back on anything else.
#[component]
pub fn ColorPicker(
    /// The color, when the caller holds it.
    #[prop(into, optional)]
    value: Binding<Rgb>,
    /// What it starts as, when the picker owns it.
    #[prop(default = Rgb::BLACK)]
    default_value: Rgb,
    /// Told whenever the color changes, whoever owns it.
    #[prop(optional)]
    on_change: Option<UnsyncCallback<Rgb>>,
    /// Colors offered as one-press choices below the editor.
    #[prop(optional)]
    presets: Vec<Rgb>,
    /// Whether the picker is open, when the caller holds it.
    #[prop(into, optional)]
    open: Binding<bool>,
    /// What the color is for, for a reader. Names the button.
    #[prop(into)]
    label: String,
    #[prop(into, default = "Saturation and brightness".to_owned())] area_label: String,
    #[prop(into, default = "Hue".to_owned())] hue_label: String,
    #[prop(into, default = "Hex".to_owned())] hex_label: String,
    #[prop(into, optional)] class: Classes,
    /// Forwarded to the button.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let held = Controllable::new(value, default_value, on_change);
    let hsv = RwSignal::new_local(Hsv::from_rgb(held.get_untracked()));
    let hex = RwSignal::new_local(held.get_untracked().to_string());
    // Set by typing and cleared when the hex is taken: while set the text is the user's.
    let typed = RwSignal::new_local(false);

    let following = RenderEffect::new(move |_| {
        let color = held.get();
        if hsv.with_untracked(|hsv| hsv.to_rgb()) != color {
            hsv.update(|hsv| *hsv = hsv.follow(color));
        }
        if !typed.get_untracked() {
            hex.set(color.to_string());
        }
    });
    on_cleanup_local(move || drop(following));

    // The color is asked for first and the area and strip shown from what was kept, so a caller
    // that refuses a change leaves every part of the picker on the color it kept.
    let choose = move |next: Hsv| {
        typed.set(false);
        held.set(next.to_rgb());

        let kept = held.get_untracked();
        if next.to_rgb() == kept {
            hsv.set(next);
        } else {
            hsv.update(|hsv| *hsv = hsv.follow(kept));
        }
        hex.set(kept.to_string());
    };
    let pick = move |color: Rgb| {
        typed.set(false);
        held.set(color);

        let kept = held.get_untracked();
        if hsv.with_untracked(|hsv| hsv.to_rgb()) != kept {
            hsv.update(|hsv| *hsv = hsv.follow(kept));
        }
        hex.set(kept.to_string());
    };
    let take_hex = move || {
        if !typed.get_untracked() {
            return;
        }

        typed.set(false);
        if let Ok(color) = hex.with_untracked(|text| Rgb::parse(text)) {
            pick(color);
        }
        hex.set(held.get_untracked().to_string());
    };
    let hex_invalid = Signal::derive_local(move || typed.get() && hex.with(|text| Rgb::parse(text).is_err()));

    let swatch = move || Some(held.get().to_string());
    let dialog_label = label.clone();
    let presets = StoredValue::new_local(presets);

    view! {
        Popover(open = open) {
            PopoverTrigger(
                class = "lui-color__trigger",
                variant = ButtonVariant::Outline,
                size = ButtonSize::IconSm,
                a11y:label = label,
                {..attrs},
                class = class
            ) {
                box(class = "lui-color__swatch", style:background-color = swatch)
            }
            PopoverContent(class = "lui-color__popover", a11y:label = dialog_label) {
                column(class = "lui-color__panel") {
                    ShadeArea(hsv = hsv, choose = UnsyncCallback::new(choose), label = area_label.clone())
                    Slider(
                        class = "lui-color__hue",
                        value = Binding::controlled(
                            Signal::derive_local(move || hsv.get().hue_degrees),
                            move |hue_degrees: f64| choose(Hsv { hue_degrees, ..hsv.get_untracked() })
                        ),
                        min = 0.0,
                        max = 360.0,
                        step = 1.0,
                        label = hue_label.clone()
                    )
                    row(class = "lui-color__entry") {
                        box(class = "lui-color__preview", style:background-color = swatch, a11y:hidden = true)
                        Input(
                            class = "lui-color__hex",
                            value = hex,
                            on_change = UnsyncCallback::new(move |_| typed.set(true)),
                            invalid = hex_invalid,
                            label = hex_label.clone(),
                            on:key_down = move |ev| {
                                if ev.key == Key::Named(NamedKey::Enter) {
                                    take_hex();
                                }
                            },
                            on:focus_out = move |_| take_hex()
                        )
                    }
                    {presets.with_value(|presets| preset_row(presets, held, pick))}
                }
            }
        }
    }
}

/// The square that picks saturation across and brightness up, in the strip's hue.
#[component]
fn ShadeArea(hsv: RwSignal<Hsv, LocalStorage>, choose: UnsyncCallback<Hsv>, label: String) -> impl IntoView {
    let area = NodeRef::new();
    let dragging = RwSignal::new_local(false);

    // A pointer reports CSS pixels in the window and the area's box is in device pixels, so the
    // scale relates them, as in zgui-ui's slider.
    let to_point = move |x_css: f32, y_css: f32| {
        let Some(bounds) = area.window_bounds() else {
            return;
        };
        if bounds.size.width.0 <= 0.0 || bounds.size.height.0 <= 0.0 {
            return;
        }

        let scale = area.scale();
        let across = f64::from((x_css * scale - bounds.origin.x.0) / bounds.size.width.0);
        let down = f64::from((y_css * scale - bounds.origin.y.0) / bounds.size.height.0);
        choose.run(Hsv {
            saturation: across.clamp(0.0, 1.0),
            brightness: (1.0 - down).clamp(0.0, 1.0),
            ..hsv.get_untracked()
        });
    };

    let shading = format!(
        "linear-gradient(to top, {}, {}), linear-gradient(to right, {}, {})",
        Rgb::BLACK.with_opacity(Opacity::OPAQUE),
        Rgb::BLACK.with_opacity(Opacity(0.0)),
        Rgb::WHITE.with_opacity(Opacity::OPAQUE),
        Rgb::WHITE.with_opacity(Opacity(0.0)),
    );
    let percent = |fraction: f64| Some(format!("{:.2}%", fraction * 100.0));

    view! {
        box(
            node_ref = area,
            class = "lui-color__area",
            tabindex = {Focus::Sequential},
            a11y:role = Role::Slider,
            a11y:label = label,
            a11y:value = move || {
                let hsv = hsv.get();
                format!("{:.0}%, {:.0}%", hsv.saturation * 100.0, hsv.brightness * 100.0)
            },
            style:background-color = move || Some(hsv.get().pure_hue().to_string()),
            style:background-image = shading,
            on:key_down = move |ev| {
                let by = if ev.modifiers.shift() { 0.1 } else { 0.01 };
                let now = hsv.get_untracked();
                let next = match &ev.key {
                    Key::Named(NamedKey::ArrowLeft) => Hsv { saturation: (now.saturation - by).max(0.0), ..now },
                    Key::Named(NamedKey::ArrowRight) => Hsv { saturation: (now.saturation + by).min(1.0), ..now },
                    Key::Named(NamedKey::ArrowDown) => Hsv { brightness: (now.brightness - by).max(0.0), ..now },
                    Key::Named(NamedKey::ArrowUp) => Hsv { brightness: (now.brightness + by).min(1.0), ..now },
                    _ => return,
                };
                choose.run(next);
                ev.prevent_default();
                ev.stop_propagation();
            },
            on:pointer_down = move |ev| {
                ev.capture_pointer();
                dragging.set(true);
                to_point(ev.position.x.0, ev.position.y.0);
            },
            on:pointer_move = move |ev| {
                if dragging.get_untracked() {
                    to_point(ev.position.x.0, ev.position.y.0);
                }
            },
            on:pointer_up = move |ev| {
                dragging.set(false);
                ev.release_pointer();
            },
            on:pointer_cancel = move |ev| {
                dragging.set(false);
                ev.release_pointer();
            }
        ) {
            box(
                class = "lui-color__thumb",
                style:left = move || percent(hsv.get().saturation),
                style:top = move || percent(1.0 - hsv.get().brightness)
            )
        }
    }
}

fn preset_row(presets: &[Rgb], held: Controllable<Rgb>, pick: impl Fn(Rgb) + Copy + 'static) -> Option<AnyView> {
    if presets.is_empty() {
        return None;
    }

    let swatches: Vec<AnyView> = presets
        .iter()
        .map(|&color| {
            AnyView::new(view! {
                control(
                    class = "lui-color__preset",
                    class:lui-color__preset-selected = move || held.get() == color,
                    tabindex = {Focus::Sequential},
                    a11y:role = Role::Button,
                    a11y:label = color.to_string(),
                    style:background-color = color.to_string(),
                    on:click = move |_| pick(color)
                )
            })
        })
        .collect();

    Some(AnyView::new(view! { row(class = "lui-color__presets") {{swatches}} }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_reads_back_what_a_color_writes() {
        let color = Rgb::new(0x1a2b3c);

        assert_eq!(Rgb::parse(&color.to_string()), Ok(color));
        assert_eq!(Rgb::parse(" 1A2B3C "), Ok(color));
        assert_eq!(Rgb::parse("#f80"), Ok(Rgb::new(0xff8800)));
    }

    #[test]
    fn hex_that_is_not_a_color_says_why() {
        use crate::error::HexColorError;

        assert_eq!(Rgb::parse("#12345"), Err(HexColorError::Length { length: 5 }));
        assert_eq!(Rgb::parse("#12g456"), Err(HexColorError::NotHex { position: 2 }));
        assert_eq!(Rgb::parse(""), Err(HexColorError::Length { length: 0 }));
    }

    #[test]
    fn every_channel_survives_a_trip_through_the_picker_coordinates() {
        for packed in [0x000000, 0xffffff, 0xff0000, 0x00ff00, 0x0000ff, 0x1a2b3c, 0x808080, 0xc7c3b8] {
            let color = Rgb::new(packed);
            assert_eq!(Hsv::from_rgb(color).to_rgb(), color, "{color}");
        }
    }

    #[test]
    fn a_grey_keeps_the_hue_the_strip_was_at() {
        let red = Hsv { hue_degrees: 0.0, saturation: 1.0, brightness: 1.0 };
        let teal = Hsv { hue_degrees: 180.0, ..red };

        assert_eq!(teal.follow(Rgb::new(0x808080)).hue_degrees, 180.0);
        assert_eq!(teal.follow(Rgb::new(0xff0000)).hue_degrees, 0.0);
    }
}

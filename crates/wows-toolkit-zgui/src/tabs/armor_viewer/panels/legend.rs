//! The armor thickness legend: a panel over the viewer that can be dragged, collapsed and closed,
//! with one swatch per thickness band the hull is painted in.
//!
//! The egui window shows its title twice, once as window chrome and once as the first row. Here
//! one title row is both the title and the drag handle.

use std::rc::Rc;

use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_icons::Glyph;
use wowsunpack::export::gltf_export::ArmorLegendEntry;
use wowsunpack::export::gltf_export::armor_color_legend;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::chevron::CHEVRON_DOWN;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::icons::AppIconProps;
use crate::services::Services;

const SHEET: &str = "wt-armor-legend";

const CSS: &str = css!(
    ".wt-armor-legend { position: absolute; display: flex; flex-direction: column; min-width: 140px;
        background-color: var(--lui-color-surface); border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius); }
    .wt-armor-legend__header { display: flex; align-items: center; gap: 4px; padding: 4px; }
    .wt-armor-legend__title { flex: 1; font-size: 12px; font-weight: 700; cursor: grab; }
    .wt-armor-legend__title[data-dragging] { cursor: grabbing; }
    .wt-armor-legend__rows { display: flex; flex-direction: column; gap: 4px; padding: 2px 8px 8px 8px; }
    .wt-armor-legend__row { display: flex; align-items: center; gap: 4px; }
    .wt-armor-legend__swatch { flex: none; width: 16px; height: 12px; border-radius: 2px; }
    .wt-armor-legend__label { font-size: 12px; }"
);

/// Where the panel sits in the viewer, in logical pixels from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegendPos {
    pub x: f32,
    pub y: f32,
}

/// The egui window's default position (`ui/tab.rs`: `egui::pos2(10.0, 100.0)`).
pub const DEFAULT_POS: LegendPos = LegendPos { x: 10.0, y: 100.0 };

/// Whether the legend is shown, whether it is collapsed, and where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegendState {
    pub visible: bool,
    pub collapsed: bool,
    pub pos: LegendPos,
}

impl Default for LegendState {
    /// The egui app's `ArmorViewerDefaults::default()`: shown, expanded, default position.
    fn default() -> Self {
        Self { visible: true, collapsed: false, pos: DEFAULT_POS }
    }
}

impl LegendState {
    /// Reads the saved `armor_viewer_defaults` row. No row, or a position never saved, is the
    /// egui default.
    pub fn from_defaults(row: Option<&ArmorViewerDefaultsRow>) -> Self {
        let Some(row) = row else { return Self::default() };
        let pos = match (row.legend_pos_x, row.legend_pos_y) {
            (Some(x), Some(y)) => LegendPos { x: x as f32, y: y as f32 },
            _ => DEFAULT_POS,
        };

        Self { visible: row.show_legend, collapsed: row.legend_collapsed, pos }
    }
}

/// A legend entry's 0..1 RGBA as bytes, the way the egui legend converts it: each channel times
/// 255, truncated and saturated by the float to integer cast.
pub fn swatch_rgba(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel * 255.0) as u8)
}

/// The colour as a CSS value.
pub fn swatch_css(color: [f32; 4]) -> String {
    let [red, green, blue, alpha] = swatch_rgba(color);
    format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
}

/// `{min}+ mm` for the open top band, else `{min}-{max} mm`, as the egui legend words it.
fn entry_label(entry: &ArmorLegendEntry) -> String {
    if entry.max_mm >= 999.0 {
        format!("{}+ mm", entry.min_mm as u32)
    } else {
        format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32)
    }
}

/// Where a drag started: the pointer and the panel, so the panel follows the pointer's whole
/// displacement rather than adding up per-move steps.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LegendDrag {
    pointer: LegendPos,
    panel: LegendPos,
}

#[cfg(test)]
thread_local! {
    /// How many times the panel and its rows have been built on this thread.
    static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times a legend panel or its rows have been built on this thread so far.
#[cfg(test)]
pub fn builds_for_test() -> usize {
    BUILDS.with(std::cell::Cell::get)
}

/// The legend panel, absolutely placed in the nearest positioned ancestor. Hidden while
/// `state.visible` is false. `on_saved_change` is told after a change worth saving: a collapse, a
/// close, or the end of a drag.
#[component]
pub fn ArmorLegend(
    state: RwSignal<LegendState, LocalStorage>,
    #[prop(optional)] on_saved_change: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let drag: RwSignal<Option<LegendDrag>, LocalStorage> = RwSignal::new_local(None);
    let commit = Rc::new(move || {
        if let Some(on_saved_change) = &on_saved_change {
            on_saved_change.run(());
        }
    });

    // Apart, so a drag moving the panel does not build it or its rows again.
    let visible = Memo::new(move |_| state.get().visible);
    let collapsed = Memo::new(move |_| state.get().collapsed);
    let pos = Memo::new(move |_| state.get().pos);

    let panel = move || {
        if !visible.get() {
            return AnyView::new(());
        }
        #[cfg(test)]
        BUILDS.with(|builds| builds.set(builds.get() + 1));
        let (on_collapse, on_close, on_release) = (Rc::clone(&commit), Rc::clone(&commit), Rc::clone(&commit));
        let rows = move || {
            if collapsed.get() {
                return Vec::new();
            }
            #[cfg(test)]
            BUILDS.with(|builds| builds.set(builds.get() + 1));
            armor_color_legend()
                .iter()
                .map(|entry| {
                    let swatch = swatch_css(entry.color);
                    AnyView::new(view! {
                        row(class = "wt-armor-legend__row") {
                            box(class = "wt-armor-legend__swatch", style:background-color = swatch)
                            text(class = "wt-armor-legend__label") {{entry_label(entry)}}
                        }
                    })
                })
                .collect()
        };
        let chevron = Signal::derive_local(move || if collapsed.get() { CHEVRON_RIGHT } else { CHEVRON_DOWN });

        AnyView::new(view! {
            column(
                class = "wt-armor-legend",
                attr:data-testid = "armor-legend-panel",
                style:left = move || Some(format!("{}px", pos.get().x)),
                style:top = move || Some(format!("{}px", pos.get().y))
            ) {
                row(class = "wt-armor-legend__header") {
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::IconSm,
                                attr:data-testid = "armor-legend-collapse",
                                a11y:label = locale.text("ui.armor.legend_collapse"),
                                on:click = move |_| {
                                    state.update(|state| state.collapsed = !state.collapsed);
                                    on_collapse();
                                }
                            ) {
                                Icon(icon = chevron)
                            }
                        }
                        TooltipContent {{locale.text("ui.armor.legend_collapse")}}
                    }
                    text(
                        class = "wt-armor-legend__title",
                        attr:data-testid = "armor-legend-drag-handle",
                        attr:data-dragging = move || drag.get().map(|_| "true".to_owned()),
                        on:pointer_down = move |ev| {
                            ev.capture_pointer();
                            let pointer = LegendPos { x: ev.position.x.0, y: ev.position.y.0 };
                            drag.set(Some(LegendDrag { pointer, panel: state.get_untracked().pos }));
                        },
                        on:pointer_move = move |ev| {
                            let Some(start) = drag.get_untracked() else { return };
                            let pos = LegendPos {
                                x: start.panel.x + ev.position.x.0 - start.pointer.x,
                                y: start.panel.y + ev.position.y.0 - start.pointer.y,
                            };
                            state.update(|state| state.pos = pos);
                        },
                        on:pointer_up = move |ev| {
                            ev.release_pointer();
                            if drag.get_untracked().is_some() {
                                drag.set(None);
                                on_release();
                            }
                        },
                        on:pointer_cancel = move |ev| {
                            ev.release_pointer();
                            drag.set(None);
                        }
                    ) {
                        {locale.text("ui.armor.armor_thickness_title")}
                    }
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::IconSm,
                                attr:data-testid = "armor-legend-close",
                                a11y:label = locale.text("ui.armor.legend_hide"),
                                on:click = move |_| {
                                    state.update(|state| state.visible = false);
                                    on_close();
                                }
                            ) {
                                AppIcon(glyph = Glyph::X)
                            }
                        }
                        TooltipContent {{locale.text("ui.armor.legend_hide")}}
                    }
                }
                column(class = "wt-armor-legend__rows", attr:data-testid = "armor-legend-rows") {
                    {rows}
                }
            }
        })
    };

    view! {
        {panel}
    }
}

#[cfg(test)]
mod tests {
    use wowsunpack::export::gltf_export::armor_color_legend;

    use super::DEFAULT_POS;
    use super::LegendState;
    use super::entry_label;
    use super::swatch_css;
    use super::swatch_rgba;

    #[test]
    fn labels_match_the_egui_legend() {
        let entries = armor_color_legend();
        assert_eq!(entries.len(), 10, "armor_color_legend() entry count changed; update this pin");

        for entry in &entries[..entries.len() - 1] {
            assert!(entry.max_mm < 999.0, "expected a closed band for {entry:?}");
            assert_eq!(entry_label(entry), format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32));
        }

        let last = entries.last().expect("the legend has entries");
        assert!(last.max_mm >= 999.0, "expected the last band to be open");
        assert_eq!(entry_label(last), format!("{}+ mm", last.min_mm as u32));
    }

    #[test]
    fn swatch_channels_truncate_like_egui() {
        // 0.5 * 255 is 127.5, which truncates to 127.
        assert_eq!(swatch_rgba([0.0, 1.0, 0.5, 0.8]), [0, 255, 127, 204]);
        assert_eq!(swatch_css([0.0, 1.0, 0.5, 0.8]), "#00ff7fcc");
    }

    #[test]
    fn no_saved_row_is_the_egui_default() {
        let state = LegendState::from_defaults(None);
        assert!(state.visible);
        assert!(!state.collapsed);
        assert_eq!(state.pos, DEFAULT_POS);
    }
}

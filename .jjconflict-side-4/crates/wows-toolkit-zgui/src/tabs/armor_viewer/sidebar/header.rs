//! The sidebar's header: its title, the button that opens a comparison pane, and the menu of
//! what every comparison pane shares beside the filters the listing takes.

use landaire_ui::RangeSliderProps;
use landaire_ui::SliderRange;
use landaire_ui::SliderSpan;
use rust_i18n::t;
use wows_toolkit_armor::catalog::TierRange;
use wows_toolkit_armor::catalog::tier_roman;
use wows_toolkit_icons::Glyph;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::view::IntoView;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SeparatorProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::armor_viewer::sidebar::CommonPaneSettings;
use crate::tabs::armor_viewer::sidebar::SidebarEvent;
use crate::tabs::armor_viewer::sidebar::state::SidebarState;
use crate::tabs::armor_viewer::viewport::OpeningLayer;

/// The title row.
#[component]
pub fn SidebarHeader(state: SidebarState, common: Signal<CommonPaneSettings, LocalStorage>) -> impl IntoView {
    let locale = Services::current().locale;
    let compare_tip = t!("ui.armor.compare_tooltip").into_owned();

    view! {
        row(class = "wt-armor-sidebar__header") {
            text(class = "wt-armor-sidebar__title") {{locale.text("ui.armor.ships")}}
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        attr:data-testid = "armor-sidebar-compare",
                        a11y:label = compare_tip,
                        on:click = move |_| state.send(SidebarEvent::CompareSplit(None))
                    ) {
                        AppIcon(glyph = Glyph::LayoutDashboard)
                        text {{locale.text("ui.armor.compare")}}
                    }
                }
                TooltipContent {{locale.text("ui.armor.compare_tooltip")}}
            }
            CommonMenu(state = state, common = common)
        }
    }
}

/// What every comparison pane follows, the tiers the Ships section lists, and whether it lists
/// the ships the catalog leaves out.
///
/// Always in the header, whether or not a comparison is open, so opening one does not move the
/// controls around it; the comparison items are refused while there is nothing to share with.
#[component]
fn CommonMenu(state: SidebarState, common: Signal<CommonPaneSettings, LocalStorage>) -> impl IntoView {
    let locale = Services::current().locale;
    let tip = t!("ui.armor.common_settings").into_owned();
    let not_comparing = Signal::derive_local(move || !common.get().comparing);
    let change = move |apply: fn(&mut CommonPaneSettings, bool)| {
        move |checked: Checked| {
            let mut next = common.get_untracked();
            apply(&mut next, checked == Checked::Yes);
            state.send(SidebarEvent::Common(next));
        }
    };

    let show_armor = Signal::derive_local(move || Checked::from(common.get().opening_layer.shows_armor()));
    let mirror = Signal::derive_local(move || Checked::from(common.get().mirror_cameras));
    let stack = Signal::derive_local(move || Checked::from(common.get().stack_panes));
    let sync = Signal::derive_local(move || Checked::from(common.get().sync_options));
    let hidden = Signal::derive_local(move || Checked::from(state.show_hidden.get()));

    let tiers_text = move || {
        locale.track();
        let tiers = state.tiers.get();
        t!("ui.armor.tier_filter", lowest = tier_roman(tiers.lowest()), highest = tier_roman(tiers.highest()))
            .into_owned()
    };
    let span = Signal::derive_local(move || {
        let tiers = state.tiers.get();
        SliderSpan::new(f64::from(tiers.lowest()), f64::from(tiers.highest()))
    });
    let choose = move |span: SliderSpan| state.choose_tiers(tiers_of(span));
    let commit = move |span: SliderSpan| state.commit_tiers(tiers_of(span));
    let range = SliderRange::new(f64::from(TierRange::LOWEST), f64::from(TierRange::HIGHEST), 1.0);
    // Names what the thumbs choose between, not where they stand, which the slider reports.
    let slider_label =
        t!("ui.armor.tier_filter", lowest = tier_roman(TierRange::LOWEST), highest = tier_roman(TierRange::HIGHEST))
            .into_owned();

    view! {
        Tooltip {
            TooltipTrigger {
                Popover {
                    PopoverTrigger(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "armor-sidebar-common",
                        a11y:label = tip
                    ) {
                        AppIcon(glyph = Glyph::Settings)
                    }
                    PopoverContent(
                        placement = Placement::new(Side::Bottom, Align::End),
                        class = "wt-armor-common",
                        attr:data-testid = "armor-sidebar-common-menu"
                    ) {
                        {check(
                            "armor-pane-show-armor-by-default",
                            "ui.armor.show_armor_by_default",
                            "ui.armor.show_armor_by_default_tooltip",
                            show_armor,
                            Signal::stored_local(false),
                            change(|common, on| common.opening_layer = OpeningLayer::from_show_armor(on)),
                        )}
                        {check(
                            "armor-pane-mirror-cameras",
                            "ui.armor.mirror_cameras",
                            "ui.armor.mirror_cameras_tooltip",
                            mirror,
                            not_comparing,
                            change(|common, on| common.mirror_cameras = on),
                        )}
                        {check(
                            "armor-pane-stack",
                            "ui.armor.stack_panes",
                            "ui.armor.stack_panes_tooltip",
                            stack,
                            not_comparing,
                            change(|common, on| common.stack_panes = on),
                        )}
                        {check(
                            "armor-pane-sync-options",
                            "ui.armor.sync_options",
                            "ui.armor.sync_tooltip",
                            sync,
                            not_comparing,
                            change(|common, on| common.sync_options = on),
                        )}
                        Separator()
                        text(class = "wt-armor-common__tiers") {{tiers_text}}
                        box(class = "wt-armor-common__slider") {
                            RangeSlider(
                                value = Binding::controlled(span, choose),
                                range = range,
                                on_release = UnsyncCallback::new(commit),
                                low_label = slider_label.clone(),
                                high_label = slider_label.clone(),
                                attr:data-testid = "armor-sidebar-tiers"
                            )
                        }
                        {check(
                            "armor-sidebar-show-hidden-ships",
                            "ui.armor.show_hidden_ships",
                            "ui.armor.show_hidden_ships_tooltip",
                            hidden,
                            Signal::stored_local(false),
                            move |checked: Checked| state.choose_show_hidden(checked == Checked::Yes),
                        )}
                    }
                }
            }
            TooltipContent {{locale.text("ui.armor.common_settings")}}
        }
    }
}

/// One labelled checkbox of the menu, with its explanation on hover.
fn check(
    test_id: &'static str,
    label: &'static str,
    hint: &'static str,
    checked: Signal<Checked, LocalStorage>,
    disabled: Signal<bool, LocalStorage>,
    toggle: impl Fn(Checked) + 'static,
) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        Tooltip {
            TooltipTrigger {
                row(class = "wt-armor-common__check", style:opacity = move || disabled.get().then(|| "0.5".to_string())) {
                    Checkbox(
                        checked = Binding::controlled(checked, toggle),
                        disabled = disabled,
                        attr:data-testid = test_id,
                        a11y:label = t!(label).into_owned()
                    )
                    text {{locale.text(label)}}
                }
            }
            TooltipContent {{locale.text(hint)}}
        }
    }
}

/// The tiers a span of the slider stands on. The slider's step keeps each thumb on a whole tier.
fn tiers_of(span: SliderSpan) -> TierRange {
    TierRange::new(span.low().round() as u32, span.high().round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_thumbs_give_whole_tiers_in_order() {
        assert_eq!(tiers_of(SliderSpan::new(3.2, 8.6)), TierRange::new(3, 9));
        assert_eq!(tiers_of(SliderSpan::new(8.0, 3.0)), TierRange::new(3, 8), "crossed thumbs");
        assert_eq!(tiers_of(SliderSpan::new(0.0, 30.0)), TierRange::ALL);
    }
}

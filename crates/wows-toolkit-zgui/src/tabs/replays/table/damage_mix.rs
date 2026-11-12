//! Drawing of a player's damage mix: the stacked bar under a damage figure and its listing.

use wows_toolkit_replay_data::damage_mix::DamageKind;
use wows_toolkit_replay_data::damage_mix::DamageLine;
use wows_toolkit_replay_data::damage_mix::DamageMix;
use wows_toolkit_replay_data::model::separate_number;
use zgui::prelude::AnyView;
use zgui::view;

use crate::theme::damage_kind_name;

/// The kind's colour, the same wherever the kind is drawn.
pub fn color(kind: DamageKind) -> String {
    format!("var(--wt-damage-{})", damage_kind_name(kind))
}

/// The segments across the whole width, each as wide as its kind's part of the damage.
pub fn stacked_bar(mix: &DamageMix) -> AnyView {
    let segments: Vec<AnyView> = mix
        .proportions()
        .enumerate()
        .map(|(index, (kind, proportion))| {
            let grow = proportion.to_string();
            AnyView::new(view! {
                box(
                    class = "wt-pt-mix__segment",
                    attr:data-testid = format!("damage-segment-{index}"),
                    style:flex-grow = grow,
                    style:background-color = color(kind)
                ) {}
            })
        })
        .collect();

    AnyView::new(view! { box(class = "wt-pt-mix") {{segments}} })
}

/// Every line with its colour key, amount and share of the total, for a tooltip.
pub fn listing(mix: &DamageMix, lines: &[DamageLine]) -> AnyView {
    let keys: Vec<AnyView> = lines
        .iter()
        .map(|line| {
            let label = line.label;
            let swatch = color(line.kind);
            AnyView::new(view! {
                box(class = "wt-pt-listing__key") {
                    box(class = "wt-pt-swatch", style:background-color = swatch) {}
                    text {{label}}
                }
            })
        })
        .collect();
    let amounts: Vec<AnyView> =
        lines.iter().map(|line| AnyView::new(view! { text {{separate_number(line.amount)}} })).collect();
    let percents: Vec<AnyView> =
        lines.iter().map(|line| AnyView::new(view! { text {{format!("{:.0}%", mix.percent(line.amount))}} })).collect();

    AnyView::new(view! {
        box(class = "wt-pt-listing") {
            box(class = "wt-pt-listing__column") {{keys}}
            box(class = "wt-pt-listing__column", attr:data-figures = String::new()) {{amounts}}
            box(class = "wt-pt-listing__column", attr:data-figures = String::new()) {{percents}}
        }
    })
}

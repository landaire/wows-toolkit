//! What a pointer resting on the frame reads: a roster's consumable icon, or the advantage label.

use wows_toolkit_replay_data::replay_render::hover::advantage_hover;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::view::ElementPoint;
use wows_toolkit_replay_data::replay_render::view::HudStrip;
use zgui::component;
use zgui::css;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::SeparatorProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;

use crate::windows::renderer::session::HoveredConsumable;
use crate::windows::renderer::session::RendererSession;

const SHEET: &str = "wt-renderer-hover";

const CSS: &str = css!(
    ".wt-renderer-hover__strip { position: absolute; }
    .wt-renderer-hover__strip .zui-tooltip__trigger, .wt-renderer-hover__fill { width: 100%; height: 100%; }
    .wt-renderer-hover__lines { display: flex; flex-direction: column; font-size: 11px; }
    .wt-renderer-hover__reading {
        position: absolute;
        display: flex;
        flex-direction: column;
        gap: 2px;
        width: 280px;
        padding: 8px;
        border-radius: var(--lui-radius);
        border: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-surface);
        font-size: 11px;
        pointer-events: none;
    }
    .wt-renderer-hover__name { font-size: 13px; font-weight: 700; }
    .wt-renderer-hover__description { display: flex; flex-direction: column; gap: 4px; color: var(--wt-color-text-dim); }"
);

/// How wide a consumable reading is, which keeps it inside the viewport.
const READING_WIDTH: f32 = 280.0;

/// How far a reading sits from the pointer, and from the viewport's edges.
const READING_GAP: f32 = 16.0;
const EDGE_MARGIN: f32 = 8.0;

/// The advantage strip and the consumable reading over the map.
#[component]
pub fn HoverReadings(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        AdvantageStrip(session = session)
        ConsumableReading(session = session)
    }
}

/// The strip above the map that the advantage label is drawn in, which holds the label's
/// breakdown as a hover: the label is in the frame's pixels, with no element of its own to rest on.
#[component]
fn AdvantageStrip(session: RendererSession) -> impl IntoView {
    let lines = move || {
        let track = session.track()?;
        let at = session.playback.with(Playback::at);
        advantage_hover(track.commands(at)?)
    };
    let strip = move || session.map.with(|map| map.hud_strip());

    move || {
        let HudStrip { left, top, height } = strip()?;
        lines()?;
        Some(view! {
            box(
                class = "wt-renderer-hover__strip",
                attr:data-testid = "replay-renderer-advantage-hover",
                style:left = format!("{left}px"),
                style:right = format!("{left}px"),
                style:top = format!("{top}px"),
                style:height = format!("{height}px")
            ) {
                Tooltip {
                    TooltipTrigger {
                        box(class = "wt-renderer-hover__fill")
                    }
                    TooltipContent {
                        column(class = "wt-renderer-hover__lines", attr:data-testid = "replay-renderer-advantage-lines") {
                            for line in move || lines().unwrap_or_default().into_iter().enumerate().collect::<Vec<_>>(), key = |line: &(usize, String)| line.clone() {
                                text {{line.1}}
                            }
                        }
                    }
                }
            }
        }.into_view())
    }
}

/// What the consumable icon under the pointer reads.
///
/// Anchored at the pointer rather than at the icon: a roster row's icons are a few pixels apart,
/// and a reading over the row would cover the ones beside the one being read.
#[component]
fn ConsumableReading(session: RendererSession) -> impl IntoView {
    let size = move || {
        session.map.with(|map| map.placement().map(|placement| (placement.bounds.width, placement.bounds.height)))
    };

    move || {
        let Reading { name, charges, timing, active, description, player, at } =
            session.hovered.with(|hovered| hovered.as_ref().map(Reading::of))?;
        let (width, height) = size()?;
        // Kept inside the viewport: beside the pointer where there is room, against the far edge
        // where there is not, and above the pointer in the lower half.
        let left = (at.x + READING_GAP).min(width - READING_WIDTH - EDGE_MARGIN).max(EDGE_MARGIN);
        let (top, bottom) = if at.y > height / 2.0 {
            (None, Some(format!("{}px", height - at.y + READING_GAP)))
        } else {
            (Some(format!("{}px", at.y + READING_GAP)), None)
        };

        Some(
            view! {
                column(
                    class = "wt-renderer-hover__reading",
                    attr:data-testid = "replay-renderer-consumable-reading",
                    style:left = format!("{left}px"),
                    style:top = top,
                    style:bottom = bottom
                ) {
                    text(class = "wt-renderer-hover__name") {{name}}
                    text {{player}}
                    text {{charges}}
                    {timing.map(|line| view! { text {{line}} }.into_view())}
                    {active.map(|line| view! { text {{line}} }.into_view())}
                    {(!description.is_empty()).then(|| view! {
                        column(class = "wt-renderer-hover__description") {
                            Separator()
                            text {{description}}
                        }
                    }.into_view())}
                }
            }
            .into_view(),
        )
    }
}

/// What a consumable reading shows, copied out of the hover so the view owns it.
struct Reading {
    name: String,
    charges: String,
    timing: Option<String>,
    active: Option<String>,
    description: String,
    player: String,
    at: ElementPoint,
}

impl Reading {
    fn of(hovered: &HoveredConsumable) -> Self {
        let lines = &hovered.hover.lines;
        Self {
            name: lines.name.clone(),
            charges: lines.charges.clone(),
            timing: lines.timing.clone(),
            active: lines.active.clone(),
            description: lines.description.clone(),
            player: hovered.hover.player.clone(),
            at: hovered.at,
        }
    }
}

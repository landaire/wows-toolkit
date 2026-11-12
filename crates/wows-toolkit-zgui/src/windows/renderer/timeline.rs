//! The transport's event timeline: what happened, filtered, and clickable.

use landaire_ui::MarkedTextProps;
use landaire_ui::Toasts;
use rust_i18n::t;
use wows_replay_insights::timeline::EventTone;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::kind_label_key;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::playback::mmss;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::timeline::TimelineRow;
use zgui::component;
use zgui::css;
use zgui::prelude::ClipboardKind;
use zgui::prelude::Clipboards;
use zgui::prelude::try_use_clipboard;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SeparatorProps;
use zgui_ui::SpinnerProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::windows::renderer::options::CheckRowProps;
use crate::windows::renderer::session::RendererSession;

const SHEET: &str = "wt-renderer-timeline";

const CSS: &str = css!(
    ".wt-renderer-timeline { display: flex; flex-direction: column; gap: 4px; width: 340px; padding: 8px; }
    .wt-renderer-timeline__header { display: flex; flex-direction: row; align-items: center; justify-content: space-between; }
    .wt-renderer-timeline__title { font-size: 12px; font-weight: 700; }
    .wt-renderer-timeline__filters { display: flex; flex-direction: row; align-items: center; gap: 4px; }
    .wt-renderer-timeline__search { flex: 1; min-width: 0; }
    .wt-renderer-timeline__kinds { display: flex; flex-direction: column; }
    .wt-renderer-timeline__said { display: flex; flex-direction: row; align-items: center; gap: 8px; padding: 8px 0; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-renderer-timeline__list { display: flex; flex-direction: column; max-height: 400px; overflow-y: auto; }
    .wt-renderer-timeline__row { display: flex; flex-direction: row; gap: 8px; padding: 2px 4px; font-size: 12px; cursor: pointer; }
    .wt-renderer-timeline__row:hover { background-color: var(--lui-color-surface); }
    .wt-renderer-timeline__at { flex: none; width: 40px; color: var(--wt-color-text-dim); }
    .wt-renderer-timeline__label { flex: 1; min-width: 0; }
    .wt-renderer-timeline__label[data-tone=friendly] { color: var(--wt-color-win); }
    .wt-renderer-timeline__label[data-tone=enemy] { color: var(--wt-color-loss); }
    .wt-renderer-timeline__label[data-tone=neutral] { color: var(--wt-color-text-dim); }
    .wt-renderer-timeline__hover { display: flex; flex-direction: column; font-size: 11px; }"
);

/// The list button on the transport and the timeline it opens.
#[component]
pub fn TimelinePopover(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let label = t!("ui.renderer.settings.event_timeline").into_owned();
    let named = label.clone();
    let title = label.clone();

    view! {
        Popover {
            PopoverTrigger(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconSm,
                attr:data-testid = "replay-renderer-timeline-toggle",
                a11y:label = label.clone()
            ) {
                Tooltip {
                    TooltipTrigger {
                        AppIcon(glyph = Glyph::List)
                    }
                    TooltipContent {{named.clone()}}
                }
            }
            PopoverContent(
                placement = Placement::new(Side::Top, Align::End),
                class = "wt-renderer-timeline",
                attr:data-testid = "replay-renderer-timeline"
            ) {
                row(class = "wt-renderer-timeline__header") {
                    text(class = "wt-renderer-timeline__title") {{title.clone()}}
                    CopyButton(session = session)
                }
                FilterBar(session = session)
                Separator()
                TimelineRows(session = session)
            }
        }
    }
}

/// Puts the listed events on the clipboard as text.
#[component]
fn CopyButton(session: RendererSession) -> impl IntoView {
    let toasts = Toasts::current();
    let clipboard = StoredValue::new_local(try_use_clipboard());
    let nothing_listed = Signal::derive_local(move || session.events.with(|events| events.visible().is_empty()));
    let copy = move || {
        let text = session.events.with(BattleEvents::copy_text);
        clipboard.with_value(|clipboard: &Option<Clipboards>| {
            // A platform with no clipboard has nowhere to put it.
            let Some(clipboard) = clipboard else { return };
            clipboard.set_text(ClipboardKind::Standard, text);
            if let Some(toasts) = toasts {
                toasts.ok(t!("ui.replay.timeline_copied").into_owned());
            }
        });
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = nothing_listed,
                    attr:data-testid = "replay-renderer-timeline-copy",
                    on:click = move |_| copy()
                ) {
                    {t!("ui.buttons.copy").into_owned()}
                }
            }
            TooltipContent {{t!("ui.replay.timeline_export_tooltip").into_owned()}}
        }
    }
}

/// The kind switches and the search box.
#[component]
fn FilterBar(session: RendererSession) -> impl IntoView {
    let search = Signal::derive_local(move || session.events.with(|events| events.filter().search.clone()));
    let set_every_kind =
        move |on: bool| session.events.update(|events| events.set_filter(|filter| filter.kinds = [on; KIND_COUNT]));

    view! {
        column {
            row(class = "wt-renderer-timeline__filters") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "replay-renderer-timeline-all",
                    on:click = move |_| set_every_kind(true)
                ) {
                    {t!("ui.replay.timeline_filter_all").into_owned()}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "replay-renderer-timeline-none",
                    on:click = move |_| set_every_kind(false)
                ) {
                    {t!("ui.replay.timeline_filter_none").into_owned()}
                }
                box(class = "wt-renderer-timeline__search") {
                    Input(
                        value = Binding::controlled(search, move |text: String| {
                            session.events.update(|events| events.set_filter(|filter| filter.search = text));
                        }),
                        placeholder = t!("ui.replay.timeline_search_hint").into_owned(),
                        attr:data-testid = "replay-renderer-timeline-search"
                    )
                }
            }
            column(class = "wt-renderer-timeline__kinds") {
                for index in move || (0..KIND_COUNT).collect::<Vec<_>>(), key = |index: &usize| *index {
                    CheckRow(
                        label = t!(kind_label_key(index)).into_owned(),
                        test_id = format!("replay-renderer-timeline-kind-{index}"),
                        checked = Signal::derive_local(move || session.events.with(|events| events.filter().kinds[index])),
                        on_change = UnsyncCallback::new(move |on: bool| {
                            session.events.update(|events| events.set_filter(|filter| filter.kinds[index] = on));
                        })
                    )
                }
            }
        }
    }
}

/// What the timeline says while there is no list to show.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// The second walk of the replay has not finished.
    Reading,
    /// The battle had nothing worth listing.
    NothingHappened,
    /// Things happened, and the filter keeps none of them.
    NothingMatches,
    Rows,
}

/// The filtered list, or what to say instead of one.
#[component]
fn TimelineRows(session: RendererSession) -> impl IntoView {
    let listing = move || {
        session.events.with(|events| {
            if !events.is_read() {
                Listing::Reading
            } else if events.events().is_empty() {
                Listing::NothingHappened
            } else if events.visible().is_empty() {
                Listing::NothingMatches
            } else {
                Listing::Rows
            }
        })
    };

    move || match listing() {
        Listing::Reading => AnyView::new(view! {
            row(class = "wt-renderer-timeline__said") {
                Spinner(label = t!("ui.replay.timeline_parsing").into_owned())
                text {{t!("ui.replay.timeline_parsing").into_owned()}}
            }
        }),
        Listing::NothingHappened => AnyView::new(view! {
            text(class = "wt-renderer-timeline__said") {{t!("ui.replay.timeline_no_events").into_owned()}}
        }),
        Listing::NothingMatches => AnyView::new(view! {
            text(class = "wt-renderer-timeline__said") {{t!("ui.replay.timeline_no_matches").into_owned()}}
        }),
        Listing::Rows => AnyView::new(view! {
            column(class = "wt-renderer-timeline__list", attr:data-testid = "replay-renderer-timeline-list") {
                // Keyed by place and wording: the same place holds a different event once the
                // filter moves, and the same event is marked anew as the search changes.
                for entry in move || session.events.with(BattleEvents::rows).into_iter().enumerate().collect::<Vec<_>>(),
                    key = |entry: &(usize, TimelineRow)| (entry.0, entry.1.label.clone(), entry.1.at.to_bits(), entry.1.marks.clone()) {
                    TimelineEntry(session = session, index = entry.0, row = entry.1)
                }
            }
        }),
    }
}

/// One event: when, what, and on hover the detail; a click goes there.
#[component]
fn TimelineEntry(session: RendererSession, index: usize, row: TimelineRow) -> impl IntoView {
    let TimelineRow { at, label, hover, tone, marks } = row;
    let tone = match tone {
        // The two colours this app already paints a friendly and an enemy team in.
        EventTone::Friendly => "friendly",
        EventTone::Enemy => "enemy",
        EventTone::Neutral => "neutral",
    };
    let entry = view! {
        row(
            class = "wt-renderer-timeline__row",
            attr:data-testid = format!("replay-renderer-timeline-row-{index}"),
            on:click = move |_| session.go_to_elapsed(at)
        ) {
            text(class = "wt-renderer-timeline__at") {{mmss(at)}}
            MarkedText(class = "wt-renderer-timeline__label", attr:data-tone = tone, text = label, ranges = marks.into())
        }
    };
    if hover.is_empty() {
        return AnyView::new(entry);
    }

    let lines = StoredValue::new_local(hover.split('\n').map(str::to_owned).enumerate().collect::<Vec<_>>());
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                {entry}
            }
            TooltipContent {
                column(class = "wt-renderer-timeline__hover") {
                    for line in move || lines.get_value(), key = |line: &(usize, String)| line.0 {
                        text {{line.1}}
                    }
                }
            }
        }
    })
}

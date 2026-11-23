//! The event timeline: what happened, filtered, and clickable. The transport's popover and the
//! replay panel's timeline window share its controls.

use landaire_ui::EscapeClears;
use landaire_ui::LoadingLineProps;
use landaire_ui::MarkedTextProps;
use landaire_ui::Toasts;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use wows_replay_insights::timeline::EventTone;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::kind_label_key;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::playback::mmss;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::timeline::TimelineRow;
use wt_translations::t;
use zgui::component;
use zgui::css;
use zgui::prelude::ClipboardKind;
use zgui::prelude::Clipboards;
use zgui::prelude::try_use_clipboard;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
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
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::windows::renderer::options::CheckRowProps;
use crate::windows::renderer::session::RendererSession;

pub(crate) const SHEET: &str = "wt-renderer-timeline";

pub(crate) const CSS: &str = css!(
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

/// What a timeline's controls read, and where a click on a row goes.
#[derive(Clone, Copy)]
pub(crate) struct TimelineCx {
    pub events: RwSignal<BattleEvents, LocalStorage>,
    /// What each control's test id starts with.
    pub test_id: &'static str,
    pub on_pick: UnsyncCallback<PickedEvent>,
}

impl TimelineCx {
    fn test_id(self, part: &str) -> String {
        format!("{}-{part}", self.test_id)
    }

    /// Escape in the search empties it before it closes what holds the timeline.
    pub(crate) fn escape(self) -> EscapeClears {
        let events = self.events;
        EscapeClears::with(
            Signal::derive_local(move || events.with(|events| !events.filter().search.is_empty())),
            move || events.update(|events| events.set_filter(|filter| filter.search = String::new())),
        )
    }
}

/// A row the reader clicked: its place among the listed rows, and when it happened.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PickedEvent {
    pub index: usize,
    /// Seconds into the battle.
    pub at: f32,
}

/// The list button on the transport and the timeline it opens.
#[component]
pub fn TimelinePopover(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let label = t!("ui-renderer-settings-event_timeline").into_owned();
    let named = label.clone();
    let title = label.clone();
    let cx = TimelineCx {
        events: session.events,
        test_id: "replay-renderer-timeline",
        on_pick: UnsyncCallback::new(move |picked: PickedEvent| session.go_to_elapsed(picked.at)),
    };
    let escape = cx.escape();

    view! {
        Popover {
            PopoverTrigger(
                variant = ButtonVariant::Outline,
                size = ButtonSize::IconXs,
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
                dismiss_on_escape = escape.dismiss_on_escape(),
                placement = Placement::new(Side::Top, Align::End),
                class = "wt-renderer-timeline",
                attr:data-testid = "replay-renderer-timeline"
            ) {
                row(class = "wt-renderer-timeline__header") {
                    text(class = "wt-renderer-timeline__title") {{title.clone()}}
                    TimelineCopyButton(cx = cx)
                }
                TimelineFilterBar(cx = cx, escape = escape)
                Separator()
                TimelineRows(cx = cx)
            }
        }
    }
}

/// Puts the listed events on the clipboard as text.
#[component]
pub(crate) fn TimelineCopyButton(cx: TimelineCx) -> impl IntoView {
    let toasts = Toasts::current();
    let clipboard = StoredValue::new_local(try_use_clipboard());
    let events = cx.events;
    let nothing_listed = Signal::derive_local(move || events.with(|events| events.visible().is_empty()));
    let copy = move || {
        let text = events.with(BattleEvents::copy_text);
        clipboard.with_value(|clipboard: &Option<Clipboards>| {
            // A platform with no clipboard has nowhere to put it.
            let Some(clipboard) = clipboard else { return };
            clipboard.set_text(ClipboardKind::Standard, text);
            if let Some(toasts) = toasts {
                toasts.ok(t!("ui-replay-timeline_copied").into_owned());
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
                    attr:data-testid = cx.test_id("copy"),
                    on:click = move |_| copy()
                ) {
                    {t!("ui-buttons-copy").into_owned()}
                }
            }
            TooltipContent {{t!("ui-replay-timeline_export_tooltip").into_owned()}}
        }
    }
}

/// The kind switches and the search box.
#[component]
pub(crate) fn TimelineFilterBar(cx: TimelineCx, escape: EscapeClears) -> impl IntoView {
    let events = cx.events;
    let search = Signal::derive_local(move || events.with(|events| events.filter().search.clone()));
    let set_every_kind =
        move |on: bool| events.update(|events| events.set_filter(|filter| filter.kinds = [on; KIND_COUNT]));

    view! {
        column {
            row(class = "wt-renderer-timeline__filters") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = cx.test_id("all"),
                    on:click = move |_| set_every_kind(true)
                ) {
                    {t!("ui-replay-timeline_filter_all").into_owned()}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = cx.test_id("none"),
                    on:click = move |_| set_every_kind(false)
                ) {
                    {t!("ui-replay-timeline_filter_none").into_owned()}
                }
                box(class = "wt-renderer-timeline__search") {
                    Input(
                        value = Binding::controlled(search, move |text: String| {
                            events.update(|events| events.set_filter(|filter| filter.search = text));
                        }),
                        placeholder = t!("ui-replay-timeline_search_hint").into_owned(),
                        attr:data-testid = cx.test_id("search"),
                        node_ref = escape.node_ref(),
                        on:key_down = move |ev| {
                            escape.key_down(ev);
                        }
                    )
                }
            }
            column(class = "wt-renderer-timeline__kinds") {
                for index in move || (0..KIND_COUNT).collect::<Vec<_>>(), key = |index: &usize| *index {
                    CheckRow(
                        label = t!(kind_label_key(index)).into_owned(),
                        test_id = cx.test_id(&format!("kind-{index}")),
                        checked = Signal::derive_local(move || events.with(|events| events.filter().kinds[index])),
                        on_change = UnsyncCallback::new(move |on: bool| {
                            events.update(|events| events.set_filter(|filter| filter.kinds[index] = on));
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
    /// The walk of the replay that reads its events has not finished.
    Reading,
    /// The battle had nothing worth listing.
    NothingHappened,
    /// Things happened, and the filter keeps none of them.
    NothingMatches,
    Rows,
}

/// The filtered list, or what to say instead of one.
#[component]
pub(crate) fn TimelineRows(cx: TimelineCx) -> impl IntoView {
    let events = cx.events;
    let listing = move || {
        events.with(|events| {
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
            LoadingLine(label = t!("ui-replay-timeline_parsing").into_owned(), class = "wt-renderer-timeline__said")
        }),
        Listing::NothingHappened => AnyView::new(view! {
            text(class = "wt-renderer-timeline__said") {{t!("ui-replay-timeline_no_events").into_owned()}}
        }),
        Listing::NothingMatches => AnyView::new(view! {
            text(class = "wt-renderer-timeline__said") {{t!("ui-replay-timeline_no_matches").into_owned()}}
        }),
        Listing::Rows => AnyView::new(view! {
            column(class = "wt-renderer-timeline__list", attr:data-testid = cx.test_id("list")) {
                // Keyed by place and wording: the same place holds a different event once the
                // filter moves, and the same event is marked anew as the search changes.
                for entry in move || events.with(BattleEvents::rows).into_iter().enumerate().collect::<Vec<_>>(),
                    key = |entry: &(usize, TimelineRow)| (entry.0, entry.1.label.clone(), entry.1.at.to_bits(), entry.1.marks.clone()) {
                    TimelineEntry(cx = cx, index = entry.0, row = entry.1)
                }
            }
        }),
    }
}

/// One event: when, what, and on hover the detail; a click picks it.
#[component]
fn TimelineEntry(cx: TimelineCx, index: usize, row: TimelineRow) -> impl IntoView {
    let TimelineRow { at, label, hover, tone, marks } = row;
    let tone = match tone {
        // The two colours this app already paints a friendly and an enemy team in.
        EventTone::Friendly => "friendly",
        EventTone::Enemy => "enemy",
        EventTone::Neutral => "neutral",
    };
    let on_pick = cx.on_pick;
    let entry = view! {
        row(
            class = "wt-renderer-timeline__row",
            attr:data-testid = cx.test_id(&format!("row-{index}")),
            on:click = move |_| on_pick.run(PickedEvent { index, at })
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

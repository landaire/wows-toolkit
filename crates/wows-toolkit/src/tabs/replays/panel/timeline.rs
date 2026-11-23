//! A battle's timeline beside the player table: every event, with the alt perspectives' events
//! merged in, filtered, copied or saved, and one event copied by clicking it. The events and the
//! filter are kept per tab, so the timeline reopens as it was left.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use wows_replay_insights::timeline::TimelineExtractionResult;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wt_translations::t;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateUntracked;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::SeparatorProps;

use crate::shell::Shell;
use crate::windows::renderer::timeline;
use crate::windows::renderer::timeline::PickedEvent;
use crate::windows::renderer::timeline::TimelineCopyButtonProps;
use crate::windows::renderer::timeline::TimelineCx;
use crate::windows::renderer::timeline::TimelineFilterBarProps;
use crate::windows::renderer::timeline::TimelineRowsProps;

const SHEET: &str = "wt-replay-timeline";

const CSS: &str = css!(
    ".wt-replay-timeline { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: 4px; padding: 8px; }
    .wt-replay-timeline .wt-renderer-timeline__list { flex: 1; max-height: none; }
    .wt-replay-timeline__actions { display: flex; flex-direction: row; justify-content: flex-end; gap: 4px; }
    .wt-replay-timeline__failed { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; color: var(--wt-color-loss); }"
);

/// The battle a timeline reads.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelineSource {
    pub path: PathBuf,
    /// Other recordings of the same battle, whose events are merged in.
    pub alts: Vec<PathBuf>,
    /// The map's name, which a saved timeline is named after.
    pub map: String,
}

/// What reads a battle's events off the UI thread. The reason it gives on failure is shown.
pub type ReadEvents = Arc<dyn Fn(&TimelineSource) -> Result<TimelineExtractionResult, String> + Send + Sync>;

/// Where the read of the events stands.
#[derive(Clone, Debug, PartialEq)]
enum Reading {
    Underway,
    Failed(String),
    Done,
}

/// A tab's timeline: the events read, the filter over them, and the battle they were read from.
/// Kept while the tab is open, so the battle is read once per set of recordings.
#[derive(Clone, Copy)]
pub struct TimelineState {
    events: RwSignal<BattleEvents, LocalStorage>,
    reading: RwSignal<Reading, LocalStorage>,
    /// `None` until the timeline is first shown.
    read_from: StoredValue<Option<TimelineSource>>,
}

impl TimelineState {
    pub fn new() -> Self {
        Self {
            events: RwSignal::new_local(BattleEvents::new()),
            reading: RwSignal::new_local(Reading::Underway),
            read_from: StoredValue::new(None),
        }
    }

    /// Reads the events of `source`, unless they are read or being read already.
    fn show(self, source: &TimelineSource, read: &ReadEvents) {
        if self.read_from.with_value(|from| from.as_ref() == Some(source)) {
            return;
        }

        self.start(source.clone(), Arc::clone(read));
    }

    /// Reads the events of `source` again, dropping what was read before but keeping the
    /// filter, which the reader can change while the read is underway.
    fn start(self, source: TimelineSource, read: ReadEvents) {
        let Self { events, reading, read_from } = self;
        read_from.set_value(Some(source.clone()));
        reading.set(Reading::Underway);
        events.update(BattleEvents::forget_events);

        spawn_local(async move {
            let asked = source.clone();
            let answer = blocking(move || read(&asked)).await;
            // A read of recordings since added or taken out is not shown.
            if read_from.try_with_value(|from| from.as_ref() == Some(&source)) != Some(true) {
                return;
            }
            match answer {
                Ok(answer) => {
                    // The inspector's timeline lists events only; the hits a ship took are the
                    // renderer's.
                    let Some(filter) = events.try_with_untracked(|events| events.filter().clone()) else { return };
                    let mut answer = BattleEvents::from_extraction(answer, HashMap::new());
                    answer.set_filter(|kept| *kept = filter);
                    events.try_set(answer);
                    reading.try_set(Reading::Done);
                }
                Err(reason) => {
                    reading.try_set(Reading::Failed(reason));
                }
            }
        });
    }

    /// Lets go of the events read, as the tab closes.
    pub fn release(self) {
        self.read_from.try_set_value(None);
        self.events.try_update_untracked(BattleEvents::forget_events);
    }

    /// Reads `source` with `read` now, so a panel showing `source` finds its events read.
    #[cfg(test)]
    pub(super) fn read_for_test(self, source: TimelineSource, read: ReadEvents) {
        self.start(source, read);
    }
}

impl Default for TimelineState {
    fn default() -> Self {
        Self::new()
    }
}

/// The timeline of `source`'s battle, read with `read` unless `state` holds it already.
#[component]
pub fn TimelinePanel(state: TimelineState, source: TimelineSource, read: ReadEvents) -> impl IntoView {
    install_stylesheet(timeline::SHEET, timeline::CSS);
    install_stylesheet(SHEET, CSS);
    let shell = Shell::current();
    state.show(&source, &read);

    let TimelineState { events, reading, .. } = state;
    let map = source.map.clone();
    let source = StoredValue::new(source);
    let retry = UnsyncCallback::new(move |()| state.start(source.get_value(), Arc::clone(&read)));
    let cx = TimelineCx {
        events,
        test_id: "replay-timeline",
        on_pick: UnsyncCallback::new(move |picked: PickedEvent| {
            let Some(text) = events.with_untracked(|events| events.event_text(picked.index)) else { return };
            shell.copy_text(text);
            shell.toasts().ok(t!("ui-replay-timeline_event_copied").into_owned());
        }),
    };
    let escape = cx.escape();
    let content = move || match reading.get() {
        Reading::Failed(reason) => AnyView::new(view! {
            column(class = "wt-replay-timeline__failed", attr:data-testid = "replay-timeline-failed") {
                text {{t!("ui-replay-timeline_failed", error = reason).into_owned()}}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "replay-timeline-retry",
                    on:click = move |_| retry.run(())
                ) {
                    {t!("ui-replay-timeline_retry").into_owned()}
                }
            }
        }),
        Reading::Underway | Reading::Done => {
            let map = map.clone();
            AnyView::new(view! {
                row(class = "wt-replay-timeline__actions") {
                    SaveButton(events = events, map = map)
                    TimelineCopyButton(cx = cx)
                }
                TimelineFilterBar(cx = cx, escape = escape)
                Separator()
                TimelineRows(cx = cx)
            })
        }
    };

    view! {
        column(class = "wt-replay-timeline", attr:data-testid = "replay-timeline") {{content}}
    }
}

/// Asks where to write the listed events and writes them there.
#[component]
fn SaveButton(events: RwSignal<BattleEvents, LocalStorage>, map: String) -> impl IntoView {
    let shell = Shell::current();
    let nothing_listed = Signal::derive_local(move || events.with(|events| events.visible().is_empty()));
    let save = move || {
        let text = events.with_untracked(BattleEvents::save_text);
        let asked = wows_toolkit_services::dialog::save_file(
            Some(t!("ui-replay-timeline").as_ref()),
            &format!("{map} - Match Timeline.txt"),
            None,
        );
        spawn_local(async move {
            let path = match asked.await {
                Ok(Some(path)) => path,
                Ok(None) => return,
                Err(err) => {
                    shell.toasts().failed(t!("ui-dialogs-file_dialog_failed", reason = err).into_owned());
                    return;
                }
            };
            let shown = path.display().to_string();
            match blocking(move || std::fs::write(&path, text)).await {
                Ok(()) => {
                    shell.toasts().ok(t!("ui-replay-timeline_saved", path = shown).into_owned());
                }
                Err(err) => {
                    tracing::warn!("replay timeline: the timeline was not saved: {err}");
                    shell.toasts().failed(t!("ui-replay-timeline_save_failed", error = err).into_owned());
                }
            }
        });
    };

    view! {
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            disabled = nothing_listed,
            attr:data-testid = "replay-timeline-save",
            on:click = move |_| save()
        ) {
            {t!("ui-replay-save_to_file").into_owned()}
        }
    }
}

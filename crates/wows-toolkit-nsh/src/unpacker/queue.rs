//! The extraction queue: its contents, the destination, and the run that
//! writes them out, as the GPUI queue panel shows them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use nsh_base::checkbox;
use nsh_base::progress;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::unpacker::extract::ExtractOutcome;
use wows_toolkit_viewmodel::unpacker::extract::ExtractProgress;
use wows_toolkit_viewmodel::unpacker::extract::PrototypeOutput;
use wows_toolkit_viewmodel::unpacker::extract::expand_to_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_root;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::queue::ExtractQueue;
use wowsunpack::vfs::VfsPath;

use crate::icons::Icon;

use super::Tab;
use super::error::UnpackerError;
use super::listing::Shared;
use super::load::save_output_dir;
use super::ui::ButtonStyle;
use super::ui::button;
use super::ui::caption;
use super::ui::class;
use super::ui::icon_button;
use super::ui::text_field;
use super::ui::tip;

/// The queued entries, compared by path so a signal holding them notices
/// an edit.
#[derive(Clone, Default)]
pub struct Queued(pub ExtractQueue);

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.entries().iter().zip(other.0.entries()).all(|(left, right)| left.as_str() == right.as_str())
    }
}

/// What the extraction run is doing.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractState {
    Idle,
    /// Expanding the queue's directories; the file count is not known yet.
    Counting,
    Running(ExtractProgress),
    Failed(Shared<UnpackerError>),
    /// Finished or cancelled, with what actually reached disk.
    Done(ExtractOutcome),
}

impl ExtractState {
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Counting | Self::Running(_))
    }
}

/// The queue panel's state. `Copy`: signal handles.
#[derive(Clone, Copy)]
pub struct QueueState {
    pub queued: Signal<Queued>,
    pub extract: Signal<ExtractState>,
    /// The saved destination, shared with the other front ends.
    pub output_dir: Signal<Option<PathBuf>>,
    /// The destination as typed, saved on Enter or blur so a half-typed
    /// path never reaches the database.
    pub output_dir_input: Signal<String>,
    /// Session state, as in the other front ends, which do not persist it.
    pub prototypes: Signal<PrototypeOutput>,
    /// Set to stop the run in flight; replaced per run.
    pub stop_flag: Stored<Arc<AtomicBool>>,
}

impl QueueState {
    pub fn new() -> Self {
        Self {
            queued: signal(Queued::default()),
            extract: signal(ExtractState::Idle),
            output_dir: signal(None),
            output_dir_input: signal(String::new()),
            prototypes: signal(PrototypeOutput::Raw),
            stop_flag: stored(Arc::new(AtomicBool::new(false))),
        }
    }

    pub fn is_queued(&self, path: &VfsPath) -> bool {
        self.queued.with(|queued| queued.0.contains(path))
    }

    pub fn toggle(&self, path: VfsPath) {
        self.queued.update(|queued| {
            if queued.0.contains(&path) {
                queued.0.remove(&path);
            } else {
                queued.0.push(path);
            }
        });
    }

    pub fn push(&self, path: VfsPath) {
        self.queued.update(|queued| {
            queued.0.push(path);
        });
    }

    pub fn push_listed(&self, entries: &[ListingEntry]) {
        self.queued.update(|queued| {
            queued.0.push_listed_files(entries);
        });
    }

    fn remove(&self, path: &VfsPath) {
        self.queued.update(|queued| queued.0.remove(path));
    }

    fn clear(&self) {
        self.queued.update(|queued| queued.0.clear());
        self.extract.set(ExtractState::Idle);
    }

    /// Adopts the saved destination and seeds the field with it.
    pub fn set_output_dir(&self, output_dir: Option<PathBuf>) {
        // No destination: the field is blank.
        let text = output_dir.as_deref().map(|dir| dir.display().to_string()).unwrap_or_default();
        self.output_dir_input.set(text);
        self.output_dir.set(output_dir);
    }

    fn store_output_dir(&self, output_dir: Option<PathBuf>, tab: Tab) {
        if self.output_dir.with_untracked(|saved| *saved == output_dir) {
            return;
        }

        self.output_dir.set(output_dir.clone());
        spawn(async move {
            let saved = blocking(move || save_output_dir(output_dir.as_deref()))
                .await
                .map_err(|source| UnpackerError::Task { source })
                .and_then(|saved| saved);
            if let Err(error) = saved {
                tab.set_status(error.to_string());
            }
        });
    }

    fn status(&self, tab: Tab) -> Option<String> {
        self.extract.with(|state| match state {
            ExtractState::Idle => None,
            ExtractState::Counting => Some(tab.text("ui.unpacker.counting_files")),
            ExtractState::Running(progress) => Some(tab.text_with(
                "ui.unpacker.extracting",
                &[("done", &progress.written), ("total", &progress.total)],
            )),
            ExtractState::Failed(error) => {
                Some(tab.text_with("ui.unpacker.extract_failed", &[("reason", &**error)]))
            }
            ExtractState::Done(ExtractOutcome::Completed { written }) => {
                Some(tab.text_with("ui.unpacker.extracted", &[("count", written)]))
            }
            ExtractState::Done(ExtractOutcome::Stopped { written }) => {
                Some(tab.text_with("ui.unpacker.cancelled", &[("count", written)]))
            }
        })
    }

    /// Writes the queue under the destination's `res` directory off the
    /// engine thread, reporting progress per file.
    fn start(&self) {
        let busy = self.extract.with_untracked(ExtractState::is_busy);
        let empty = self.queued.with_untracked(|queued| queued.0.is_empty());
        if busy || empty {
            return;
        }
        let Some(output_dir) = self.output_dir.get_untracked() else {
            return;
        };

        let output_dir = extract_root(&output_dir);
        let queued = self.queued.with_untracked(|queued| queued.0.entries().to_vec());
        let prototypes = self.prototypes.get_untracked();
        let stop_flag = Arc::new(AtomicBool::new(false));
        self.stop_flag.replace(Arc::clone(&stop_flag));
        self.extract.set(ExtractState::Counting);

        let extract = self.extract;
        spawn(async move {
            let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<ExtractProgress>();
            let worker = blocking(move || {
                let files = expand_to_files(&queued);
                extract_files(
                    &files,
                    &output_dir,
                    prototypes,
                    |progress| {
                        let _ = progress_tx.send(progress);
                    },
                    || stop_flag.load(Ordering::Relaxed),
                )
            });

            // Drained in batches: a run of small files reports faster than
            // the view repaints, and only the latest count matters.
            while let Some(first) = progress_rx.recv().await {
                let mut latest = first;
                while let Ok(next) = progress_rx.try_recv() {
                    latest = next;
                }
                extract.set(ExtractState::Running(latest));
            }

            let outcome = worker
                .await
                .map_err(|source| UnpackerError::Task { source })
                .and_then(|outcome| outcome.map_err(UnpackerError::from));
            extract.set(match outcome {
                Ok(outcome) => ExtractState::Done(outcome),
                Err(error) => ExtractState::Failed(Shared::new(error)),
            });
        });
    }

    fn cancel(&self) {
        self.stop_flag.with(|flag| flag.store(true, Ordering::Relaxed));
    }

    fn browse(&self, tab: Tab) {
        let queue = *self;
        spawn(async move {
            let picked = pick_paths(OpenDialog::folders().title("Extract to").button_label("Choose")).await;
            match picked {
                Ok(Some(mut paths)) => {
                    if let Some(path) = paths.pop() {
                        queue.output_dir_input.set(path.display().to_string());
                        queue.store_output_dir(Some(path), tab);
                    }
                }
                // Cancelled: the destination stays as it was.
                Ok(None) => {}
                Err(reason) => tab.set_status(UnpackerError::Dialog { reason }.to_string()),
            }
        });
    }
}

/// The queue panel: destination, contents and the run controls.
pub fn queue_view(queue: QueueState, tab: Tab) -> AnyElement {
    let count = move || queue.queued.with(|queued| queued.0.len());
    let busy = move || queue.extract.with(ExtractState::is_busy);

    let destination = view()
        .app_class(class("gap-1 px-2 py-2 border-b border-border"))
        .child(caption(tab.text("ui.unpacker.extraction_output")))
        .child(
            view()
                .app_class(class("flex-row items-center gap-1"))
                .child(
                    text_field(
                        queue.output_dir_input,
                        tab.text("ui.unpacker.output_dir_hint"),
                        tab.text("ui.unpacker.extraction_output"),
                    )
                    .on_submit(move |value: &str| queue.store_output_dir(parse_output_dir(value), tab))
                    .on_blur(move || {
                        let output_dir = queue.output_dir_input.with_untracked(|value| parse_output_dir(value));
                        queue.store_output_dir(output_dir, tab);
                    })
                    .app_class(class(
                        "flex-1 min-w-0 min-h-7 px-2 rounded-(--radius) border border-input bg-background text-xs \
                         text-foreground",
                    )),
                )
                .child(
                    button(tab.text("ui.unpacker.browse"), ButtonStyle::Secondary)
                        .on_press(move |_: &PressEvent| queue.browse(tab)),
                ),
        )
        .child(view().tooltip(tip(tab.text("ui.unpacker.decode_prototypes_tooltip"))).child(
            checkbox()
                .accessibility_label(tab.text("ui.unpacker.decode_prototypes"))
                .checked(move || queue.prototypes.get() == PrototypeOutput::DecodeToJson)
                .on_change(move |checked: &bool| {
                    queue.prototypes.set(if *checked { PrototypeOutput::DecodeToJson } else { PrototypeOutput::Raw });
                })
                .app_class(class("flex-row items-center gap-2"))
                .child(text(tab.text("ui.unpacker.decode_prototypes")).app_class(class("text-xs text-foreground"))),
        ));

    let summary = view()
        .app_class(class("flex-row items-center gap-1 px-2 py-1"))
        .child(
            caption(move || tab.text_with("ui.unpacker.queued_count", &[("count", &count())]))
                .app_class(class("flex-1 text-xs text-muted-foreground")),
        )
        .child(
            button(tab.text("ui.unpacker.clear_all"), ButtonStyle::Ghost)
                .disabled(move || count() == 0)
                .on_press(move |_: &PressEvent| queue.clear()),
        );

    let entries = view()
        .app_class(class("flex-1 min-h-0"))
        .child(when(move || count() == 0, move || caption(tab.text("ui.unpacker.queue_empty")).app_class(class("p-2 text-xs text-muted-foreground"))))
        .child(when(
            move || count() > 0,
            move || {
                scroll().app_class(class("flex-1 min-h-0")).child(
                    virtual_list(count, move |at: Position| queue_row(queue, tab, at))
                        .keys(move || {
                            queue.queued.with(|queued| {
                                queued.0.entries().iter().map(|entry| entry.as_str().to_owned()).collect::<Vec<_>>()
                            })
                        })
                        .estimated_item_size(px(24.0))
                        .viewport(ListViewport::Parent),
                )
            },
        ));

    let extract_label = move || match count() {
        0 => tab.text("ui.unpacker.extract"),
        1 => tab.text("ui.unpacker.extract_one"),
        count => tab.text_with("ui.unpacker.extract_many", &[("count", &count)]),
    };
    let no_destination = move || queue.output_dir.with(Option::is_none);

    let run = view()
        .app_class(class("gap-1 px-2 py-2 border-t border-border"))
        .child(
            view()
                .app_class(class("flex-row items-center gap-1"))
                .child(
                    button(extract_label, ButtonStyle::Primary)
                        .app_class(class(
                            "flex-1 min-h-7 px-3 rounded-(--radius) border border-border bg-primary \
                             text-primary-foreground items-center justify-center hover:opacity-85 active:opacity-70 \
                             disabled:opacity-45",
                        ))
                        .disabled(move || count() == 0 || busy() || no_destination())
                        .tooltip(tip(move || {
                            if no_destination() { tab.text("ui.unpacker.choose_directory") } else { extract_label() }
                        }))
                        .on_press(move |_: &PressEvent| queue.start()),
                )
                .child(when(busy, move || {
                    button(tab.text("ui.buttons.cancel"), ButtonStyle::Secondary)
                        .on_press(move |_: &PressEvent| queue.cancel())
                })),
        )
        .child(
            view()
                .app_class(class("h-4"))
                // Idle: the line is blank but keeps its place.
                .child(caption(move || queue.status(tab).unwrap_or_default()).accessible(true).role(AccessibilityRole::Alert)),
        )
        // Both slots are always there, empty while nothing is being
        // written, so a run starting does not push the queue down.
        .child(view().app_class(class("h-2")).child(when(
            move || queue.extract.with(|state| matches!(state, ExtractState::Running(_))),
            move || {
                progress()
                    .accessibility_label(tab.text("ui.unpacker.show_queue"))
                    .value(move || {
                        queue.extract.with(|state| match state {
                            ExtractState::Running(progress) => progress.fraction() * 100.0,
                            _ => 0.0,
                        })
                    })
                    .track_class("h-2 rounded-full bg-muted")
                    .indicator_class("h-2 rounded-full bg-primary")
            },
        )))
        .child(view().app_class(class("h-4")).child(
            text(move || {
                queue.extract.with(|state| match state {
                    ExtractState::Running(progress) => progress.file.clone(),
                    _ => String::new(),
                })
            })
            .app_class(class("text-xs text-muted-foreground truncate")),
        ));

    view()
        .app_class(class("flex-1 min-h-0"))
        .child(destination)
        .child(summary)
        .child(entries)
        .child(run)
        .into_element()
}

fn queue_row(queue: QueueState, tab: Tab, at: Position) -> AnyElement {
    let entry = computed(move || queue.queued.with(|queued| queued.0.entries().get(at.index()).cloned()));
    let label = move || {
        entry.with(|entry| {
            // A row past the end, while the list catches up with a shorter
            // queue, shows blank.
            entry.as_ref().map(|entry| format!("res/{}", entry.as_str().trim_start_matches('/'))).unwrap_or_default()
        })
    };

    view()
        .app_class(class("flex-row items-center gap-1 h-6 px-2"))
        .child(text(label).app_class(class("flex-1 min-w-0 text-xs text-foreground truncate")))
        .child(
            icon_button(Icon::X, tab.text("ui.unpacker.remove_from_queue"))
                .label(move || format!("{}: {}", tab.text("ui.unpacker.remove_from_queue"), label()))
                .on_press(move |_: &PressEvent| {
                    if let Some(path) = entry.with_untracked(Clone::clone) {
                        queue.remove(&path);
                    }
                }),
        )
        .into_element()
}

/// The directory a typed destination names; a blank field names none.
fn parse_output_dir(text: &str) -> Option<PathBuf> {
    let text = text.trim();

    (!text.is_empty()).then(|| PathBuf::from(text))
}

//! A content-search results panel: the query and its progress above a
//! virtualized list of hits. One panel per search, so an earlier result set
//! survives the next.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use nsh_base::menu;
use nsh_base::menu_item;
use nsh_base::progress;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::search::ContentSearchHit;
use wows_toolkit_viewmodel::unpacker::search::SearchProgress;
use wows_toolkit_viewmodel::unpacker::search::compile_query;
use wows_toolkit_viewmodel::unpacker::search::files_to_scan;
use wows_toolkit_viewmodel::unpacker::search::scan;

use crate::icons::Icon;

use super::Tab;
use super::error::UnpackerError;
use super::listing::BrowserSource;
use super::ui::ButtonStyle;
use super::ui::button;
use super::ui::caption;
use super::ui::class;
use super::ui::icon_button;
use super::ui::status;
use super::ui::tip;

/// One message from a running scan.
enum SearchUpdate {
    Hit(ContentSearchHit),
    Progress(SearchProgress),
}

/// One search and what it has found so far.
#[derive(Clone, Copy)]
pub struct SearchRun {
    pub query: Stored<String>,
    pub source: BrowserSource,
    pub hits: Signal<Vec<ContentSearchHit>>,
    pub progress: Signal<SearchProgress>,
    pub running: Signal<bool>,
    /// Set to end the scan early; the worker polls it per file. Also set
    /// when the panel closes, so closing it stops the scan behind it.
    pub stop_flag: Stored<Arc<AtomicBool>>,
}

impl SearchRun {
    /// Starts scanning `files` for `query`, narrowed by `path_filter`.
    /// Fails when the query compiles neither as a regex nor as a literal.
    pub fn start(
        source: BrowserSource,
        query: String,
        path_filter: String,
        files: Arc<FileList>,
    ) -> Result<Self, UnpackerError> {
        let Some(pattern) = compile_query(&query) else {
            return Err(UnpackerError::SearchQuery { query });
        };

        let run = Self {
            query: stored(query),
            source,
            hits: signal_with(Vec::new(), SignalEquals::Never),
            progress: signal(SearchProgress { scanned: 0, total: 0 }),
            running: signal(true),
            stop_flag: stored(Arc::new(AtomicBool::new(false))),
        };

        let stop_flag = run.stop_flag.get();
        spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SearchUpdate>();
            let worker = blocking(move || {
                let targets = files_to_scan(&files, &path_filter);
                scan(
                    &files,
                    &targets,
                    &pattern,
                    |hit| {
                        let _ = tx.send(SearchUpdate::Hit(hit));
                    },
                    |progress| {
                        let _ = tx.send(SearchUpdate::Progress(progress));
                    },
                    || stop_flag.load(Ordering::Relaxed),
                );
            });

            // Drained in batches: a broad query matches faster than the view
            // repaints, and one hit per wake-up is one relayout per hit.
            while let Some(first) = rx.recv().await {
                let mut hits = Vec::new();
                let mut latest = None;
                let mut pending = Some(first);
                while let Some(update) = pending.take().or_else(|| rx.try_recv().ok()) {
                    match update {
                        SearchUpdate::Hit(hit) => hits.push(hit),
                        SearchUpdate::Progress(progress) => latest = Some(progress),
                    }
                }
                if let Some(progress) = latest {
                    run.progress.set(progress);
                }
                if !hits.is_empty() {
                    run.hits.update(|all| all.extend(hits));
                }
            }

            // The worker's own failure is a panic in the scan; the hits so
            // far stay, and the run ends either way.
            let _ = worker.await;
            run.running.set(false);
        });

        Ok(run)
    }

    pub fn stop(&self) {
        self.stop_flag.with(|flag| flag.store(true, Ordering::Relaxed));
        self.running.set(false);
    }

    pub fn title(&self, tab: Tab) -> String {
        tab.text_with("ui.unpacker.search_title", &[("query", &self.query.get())])
    }
}

pub fn search_view(run: SearchRun, tab: Tab) -> AnyElement {
    let header = view()
        .app_class(class("flex-row items-center gap-2 px-2 py-1 border-b border-border"))
        .child(text(run.query.get()).app_class(class("text-sm text-foreground")))
        .child(caption(tab.text_with("ui.unpacker.search_in", &[("source", &tab.text(run.source.title_key()))])))
        .child(
            caption(move || {
                let progress = run.progress.get();
                format!("{} hits, {} of {} files", run.hits.with(Vec::len), progress.scanned, progress.total)
            })
            .app_class(class("flex-1 text-xs text-muted-foreground")),
        )
        .child(when(
            move || run.running.get(),
            move || {
                view().app_class(class("w-32")).child(
                    progress()
                        .accessibility_label(run.title(tab))
                        .value(move || {
                            let progress = run.progress.get();
                            if progress.total == 0 { 0.0 } else { progress.scanned as f32 / progress.total as f32 * 100.0 }
                        })
                        .track_class("h-2 rounded-full bg-muted")
                        .indicator_class("h-2 rounded-full bg-primary"),
                )
            },
        ))
        .child(
            button(tab.text("ui.buttons.stop"), ButtonStyle::Secondary)
                .label(tab.text("ui.unpacker.stop_search_tooltip"))
                .disabled(move || !run.running.get())
                .on_press(move |_: &PressEvent| run.stop()),
        );

    let empty = move || run.hits.with(Vec::is_empty) && !run.running.get();
    let body = view()
        .app_class(class("flex-1 min-h-0"))
        .child(when(empty, move || status(tab.text("ui.search.no_matches"))))
        .child(when(
            move || !empty(),
            move || {
                scroll().app_class(class("flex-1 min-h-0")).child(
                    virtual_list(move || run.hits.with(Vec::len), move |at: Position| hit_row(run, tab, at))
                        .estimated_item_size(px(36.0))
                        .viewport(ListViewport::Parent),
                )
            },
        ));

    view().app_class(class("flex-1 min-h-0")).child(header).child(body).into_element()
}

fn hit_row(run: SearchRun, tab: Tab, at: Position) -> AnyElement {
    let hit = computed(move || run.hits.with(|hits| hits.get(at.index()).cloned()));
    let path = move || hit.with_untracked(|hit| hit.as_ref().map(|hit| hit.vfs_path.clone()));
    // A row past the end, while the list catches up with the closed panel
    // dropping its hits, shows blank.
    let label = move || hit.with(|hit| hit.as_ref().map(|hit| hit.path.clone()).unwrap_or_default());

    let body = view()
        .accessible(true)
        .label(label)
        .app_class(class("flex-1 min-w-0 justify-center"))
        .on_press(move |press: &PressEvent| {
            if press.clicks < 2 {
                return;
            }
            if let Some(path) = path() {
                tab.open_viewer(path);
            }
        })
        .child(text(label).app_class(class("text-xs text-foreground truncate")))
        .child(
            view()
                .app_class(class("flex-row gap-1"))
                .child(caption(move || {
                    hit.with(|hit| {
                        hit.as_ref()
                            .map(|hit| tab.text_with("ui.unpacker.hit_offset", &[("offset", &hit.offset)]))
                            .unwrap_or_default()
                    })
                }))
                .child(
                    caption(move || hit.with(|hit| hit.as_ref().map(|hit| hit.context.clone()).unwrap_or_default()))
                        .app_class(class("flex-1 min-w-0 text-xs text-muted-foreground truncate")),
                ),
        );

    let item = move |key: &'static str, action: fn(Tab, SearchRun, wowsunpack::vfs::VfsPath)| {
        menu_item()
            .accessibility_label(tab.text(key))
            .app_class(class("min-h-7 px-2 rounded-(--radius) justify-center hover:bg-accent"))
            .on_select(move || {
                if let Some(path) = path() {
                    action(tab, run, path);
                }
            })
            .child(text(tab.text(key)).app_class(class("text-xs text-popover-foreground")))
    };

    let actions = menu()
        .trigger({
            let actions_label = move || format!("Actions for {}", label());
            icon_button(Icon::Ellipsis, String::new()).label(actions_label).tooltip(tip(actions_label))
        })
        .popup_class("min-w-48 p-1 rounded-(--radius) border border-border bg-popover")
        .child(item("ui.unpacker.view_contents", |tab, _, path| tab.open_viewer(path)))
        .child(item("ui.unpacker.queue", |tab, _, path| tab.queue_path(path)))
        .child(item("ui.unpacker.reveal_in_tree", |tab, run, path| tab.reveal(run.source, path)));

    view()
        .app_class(move || {
            Some(SharedString::from(if at.index() % 2 == 1 {
                "flex-row items-center gap-1 h-9 px-2 bg-muted/40 hover:bg-accent"
            } else {
                "flex-row items-center gap-1 h-9 px-2 hover:bg-accent"
            }))
        })
        .child(body)
        .child(actions)
        .into_element()
}

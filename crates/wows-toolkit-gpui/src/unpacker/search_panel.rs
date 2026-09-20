//! A content-search results tab: the query and its progress above a
//! virtualized list of hits. One panel per search, matching the egui app,
//! which opens a new search tab per run rather than replacing the last.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::browser::BrowserSource;
use wows_toolkit_viewmodel::unpacker::listing::FileKind;
use wows_toolkit_viewmodel::unpacker::search::ContentSearchHit;
use wows_toolkit_viewmodel::unpacker::search::SearchProgress;

/// Raised for the Unpacker tab to act on.
#[derive(Clone, Debug)]
pub enum SearchPanelEvent {
    /// Open the file a hit points at.
    View { path: wowsunpack::vfs::VfsPath, kind: FileKind },
}

impl EventEmitter<SearchPanelEvent> for SearchPanel {}
impl EventEmitter<PanelEvent> for SearchPanel {}

const ROW_HEIGHT: Pixels = px(34.);
const LIST_OVERDRAW: Pixels = px(200.);

pub struct SearchPanel {
    query: SharedString,
    source: BrowserSource,
    hits: Vec<ContentSearchHit>,
    progress: SearchProgress,
    running: bool,
    /// Set to end the scan early; the worker polls it per file. Also set when
    /// the panel is dropped, so closing the tab stops the scan behind it.
    stop_flag: Arc<AtomicBool>,
    list_state: ListState,
    focus_handle: FocusHandle,
}

impl SearchPanel {
    pub fn new(query: SharedString, source: BrowserSource, stop_flag: Arc<AtomicBool>, cx: &mut Context<Self>) -> Self {
        Self {
            query,
            source,
            hits: Vec::new(),
            progress: SearchProgress { scanned: 0, total: 0 },
            running: true,
            stop_flag,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Appends a batch of hits.
    ///
    /// The list is spliced rather than reset: `reset` clears the scroll
    /// position and drops scroll events until the next paint, which would
    /// make the results unscrollable for as long as the scan runs.
    pub fn extend_hits(&mut self, hits: impl IntoIterator<Item = ContentSearchHit>, cx: &mut Context<Self>) {
        let before = self.hits.len();
        self.hits.extend(hits);
        let added = self.hits.len() - before;
        if added == 0 {
            return;
        }
        self.list_state.splice(before..before, added);
        cx.notify();
    }

    pub fn set_progress(&mut self, progress: SearchProgress, cx: &mut Context<Self>) {
        self.progress = progress;
        cx.notify();
    }

    pub fn finish(&mut self, cx: &mut Context<Self>) {
        self.running = false;
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.running = false;
        cx.notify();
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for SearchPanel {
    fn panel_name(&self) -> &'static str {
        "UnpackerSearchPanel"
    }
}

impl Panel for SearchPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(format!("Search: {}", self.query))
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;
        let fraction =
            if self.progress.total == 0 { 0.0 } else { self.progress.scanned as f32 / self.progress.total as f32 };

        let header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(Icon::new(IconName::Search))
            .child(div().text_sm().child(self.query.clone()))
            .child(div().text_xs().opacity(0.6).child(format!("in {}", self.source.title())))
            .child(div().flex_1().text_xs().opacity(0.6).child(format!(
                "{} hits, {} of {} files",
                self.hits.len(),
                self.progress.scanned,
                self.progress.total
            )))
            .when(self.running, |this| {
                this.child(div().w(px(120.)).child(Progress::new("unpacker-search-progress").value(fraction * 100.)))
            })
            .child(
                Button::new("unpacker-search-stop")
                    .label("Stop")
                    .compact()
                    .disabled(!self.running)
                    .on_click(cx.listener(|this, _event, _window, cx| this.stop(cx))),
            );

        let entity = cx.entity();
        let render_row = move |ix: usize, _window: &mut Window, cx: &mut App| {
            // Read the hit out of the entity rather than holding a clone of
            // the whole result set in this closure.
            let Some(hit) = entity.read(cx).hits.get(ix).cloned() else {
                return div().into_any_element();
            };
            let entity = entity.clone();
            let path = hit.vfs_path.clone();
            let kind = FileKind::of(&hit.path);
            v_flex()
                .id(ix)
                .w_full()
                .h(ROW_HEIGHT)
                .px_2()
                .justify_center()
                .hover(|this| this.bg(hover_bg))
                .child(div().text_xs().child(hit.path.clone()))
                .child(div().text_xs().opacity(0.6).child(hit.context.clone()))
                .on_click(move |event, _window, cx| {
                    if event.click_count() < 2 {
                        return;
                    }
                    entity.update(cx, |_this, cx| {
                        cx.emit(SearchPanelEvent::View { path: path.clone(), kind });
                    });
                })
                .into_any_element()
        };

        let body: AnyElement = if self.hits.is_empty() && !self.running {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child("No matches"))
                .into_any_element()
        } else {
            div()
                .relative()
                .size_full()
                .child(list(self.list_state.clone(), render_row).size_full())
                .child(Scrollbar::vertical(&self.list_state))
                .into_any_element()
        };

        v_flex().size_full().child(header).child(div().flex_1().min_h(px(0.)).child(body))
    }
}

impl Drop for SearchPanel {
    /// Closing the results tab stops the scan feeding it; the egui pane does
    /// the same from its own `Drop`.
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

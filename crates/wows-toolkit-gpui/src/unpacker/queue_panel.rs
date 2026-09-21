//! The extraction queue as a panel beside the listing, carrying every control
//! that acts on it.
//!
//! The egui tab spreads these over a bottom bar and a popover: the queue
//! behind a toggle, Extract and the output directory in the bar, Clear inside
//! the popover. Gathering them puts the destination, the contents and the
//! button that writes them in one place, and leaves the listing's own chrome
//! to the listing.
//!
//! The panel holds no state of its own. `UnpackerView` owns the queue, the
//! output directory and the run; this renders what it is handed and emits
//! what the reader asked for.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wowsunpack::vfs::VfsPath;

/// What the panel draws, handed down from the tab on every change.
#[derive(Clone, Default)]
pub struct QueueView {
    pub entries: Vec<VfsPath>,
    /// Absent while nothing is running.
    pub progress: Option<(usize, usize)>,
    pub status: Option<SharedString>,
    pub busy: bool,
    pub decode_prototypes: bool,
    /// Whether a destination has been chosen; the Extract button is refused
    /// without one, and says so.
    pub has_output_dir: bool,
}

/// What the reader asked the tab to do.
pub enum QueueEvent {
    Extract,
    Cancel,
    ClearAll,
    Remove(VfsPath),
    Browse,
    SetDecodePrototypes(bool),
}

pub struct QueuePanel {
    view: QueueView,
    /// The tab's own field, shown here because the destination belongs with
    /// the thing being written.
    output_dir: Entity<InputState>,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for QueuePanel {}
impl EventEmitter<QueueEvent> for QueuePanel {}

impl QueuePanel {
    pub fn new(output_dir: Entity<InputState>, cx: &mut Context<Self>) -> Self {
        Self { view: QueueView::default(), output_dir, scroll: ScrollHandle::new(), focus_handle: cx.focus_handle() }
    }

    pub fn set_view(&mut self, view: QueueView, cx: &mut Context<Self>) {
        self.view = view;
        cx.notify();
    }

    /// The queue's contents, one removable row each, or a line saying how to
    /// fill it.
    fn rows(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.view.entries.is_empty() {
            return div()
                .p_2()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.unpacker.queue_empty").to_string())
                .into_any_element();
        }

        let rows: Vec<AnyElement> = self
            .view
            .entries
            .clone()
            .into_iter()
            .enumerate()
            .map(|(ix, entry)| self.row(ix, &entry, cx).into_any_element())
            .collect();

        v_flex()
            .id("unpacker-queue-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(rows)
            .into_any_element()
    }

    fn row(&self, ix: usize, entry: &VfsPath, cx: &mut Context<Self>) -> impl IntoElement {
        let path = entry.as_str().trim_start_matches('/').to_string();
        let remove = entry.clone();

        h_flex()
            .w_full()
            .gap_1()
            .items_center()
            .px_2()
            .py(px(1.))
            .when_some(crate::ui::stripe(ix, cx), |row, color| row.bg(color))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .truncate()
                    .child(crate::ui::selectable_text(("queue-path", ix), format!("res/{path}"))),
            )
            .child(
                Button::new(("unpacker-queue-remove", ix))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip(t!("ui.unpacker.remove_from_queue").to_string())
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        cx.emit(QueueEvent::Remove(remove.clone()));
                    })),
            )
    }
}

impl Focusable for QueuePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for QueuePanel {
    fn panel_name(&self) -> &'static str {
        "UnpackerQueuePanel"
    }

    /// Not closable: it is the only place the extraction controls live.
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl Panel for QueuePanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(t!("ui.unpacker.extraction_queue").to_string())
    }
}

impl Render for QueuePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let count = self.view.entries.len();
        let busy = self.view.busy;

        let destination = v_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.unpacker.extraction_output").to_string()),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(Input::new(&self.output_dir).id("unpacker-output-dir").small().flex_1().min_w(px(0.)))
                    .child(
                        Button::new("unpacker-browse")
                            .icon(IconName::FolderOpen)
                            .small()
                            .tooltip(t!("ui.unpacker.browse").to_string())
                            .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(QueueEvent::Browse))),
                    ),
            )
            .child(
                Checkbox::new("unpacker-decode-prototypes")
                    .label(t!("ui.unpacker.decode_prototypes").to_string())
                    .checked(self.view.decode_prototypes)
                    .tooltip(t!("ui.unpacker.decode_prototypes_tooltip").to_string())
                    .on_click(cx.listener(|_this, checked: &bool, _window, cx| {
                        cx.emit(QueueEvent::SetDecodePrototypes(*checked));
                    })),
            );

        let extract_label = match count {
            0 => t!("ui.unpacker.extract").into_owned(),
            1 => t!("ui.unpacker.extract_one").into_owned(),
            count => t!("ui.unpacker.extract_many", count = count).into_owned(),
        };

        // A fixed strip whether or not anything is running, so the queue below
        // does not jump when a run starts.
        let run = v_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("unpacker-extract")
                            .label(extract_label)
                            .primary()
                            .small()
                            .flex_1()
                            .disabled(count == 0 || busy || !self.view.has_output_dir)
                            .when(!self.view.has_output_dir, |this| {
                                this.tooltip(t!("ui.unpacker.choose_directory").to_string())
                            })
                            .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(QueueEvent::Extract))),
                    )
                    .when(busy, |this| {
                        this.child(
                            Button::new("unpacker-cancel")
                                .label(t!("ui.buttons.cancel").to_string())
                                .small()
                                .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(QueueEvent::Cancel))),
                        )
                    }),
            )
            .child(div().h(px(16.)).when_some(self.view.status.clone(), |this, status| {
                this.child(div().text_xs().text_color(crate::theme::text_dim()).child(status))
            }))
            .when_some(self.view.progress, |this, (done, total)| {
                let fraction = if total == 0 { 0.0 } else { (done as f32 / total as f32) * 100.0 };
                this.child(Progress::new("unpacker-extract-progress").value(fraction))
            });

        v_flex()
            .id("unpacker-queue-panel")
            .size_full()
            .child(destination)
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(crate::theme::text_dim())
                            .child(t!("ui.unpacker.queued_count", count = count).to_string()),
                    )
                    .child(
                        Button::new("unpacker-queue-clear-all")
                            .label(t!("ui.unpacker.clear_all").to_string())
                            .ghost()
                            .xsmall()
                            .disabled(count == 0)
                            .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(QueueEvent::ClearAll))),
                    ),
            )
            .child(div().flex_1().min_h(px(0.)).child(self.rows(cx)))
            .child(run)
    }
}

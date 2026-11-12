use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

use zgui::platform::ClipboardKind;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::runtime::clipboard::Clipboards;
use zgui::view::AnyView;
use zgui::view::FrameHandle;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollPosition;
use zgui::view::ScrollTarget;
use zgui::view::Timers;
use zgui::vocab::Modifiers;
use zgui::vocab::Timestamp;
use zgui_ui::ContextMenuPlace;
use zgui_ui::virtualize::VirtualWindow;

use crate::grid::column::ColumnId;
use crate::grid::column::ColumnSizing;
use crate::grid::column::ColumnWidth;
use crate::grid::column::GridColumn;
use crate::grid::copy;
use crate::grid::layout;
use crate::grid::layout::ColumnLayout;
use crate::grid::mode::RowDetail;
use crate::grid::selection::GridSelectionMode;
use crate::grid::selection::Selection;
use crate::grid::sort::ColumnSort;
use crate::motion::CubicBezier;
use crate::motion::MotionPreference;
use crate::motion::Movement;
use crate::surface::RowHeight;

/// Builds the items of the menu a row opens, for that row's key.
pub type RowMenu<K> = Rc<dyn Fn(K) -> AnyView>;

/// The words a grid shows of its own, in its header menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridStrings {
    pub pin: String,
    pub unpin: String,
    pub reset_width: String,
}

impl GridStrings {
    pub fn new() -> Self {
        Self { pin: "Pin column".to_owned(), unpin: "Unpin column".to_owned(), reset_width: "Reset width".to_owned() }
    }
}

impl Default for GridStrings {
    fn default() -> Self {
        Self::new()
    }
}

/// What a context menu was asked for over.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MenuTarget<K> {
    Header(ColumnId),
    Row(K),
}

/// A header pressed and possibly being carried to a new place.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Carry {
    /// Pressed, but not yet moved far enough to be told from a click.
    Pressed { column: ColumnId, from: f32, grab: f32 },
    /// Following the pointer, `pointer` CSS pixels from the grid's leading edge. `grab` is how
    /// far into the header it was taken, so the floating header keeps that point under the
    /// pointer.
    Moving { column: ColumnId, grab: f32, pointer: f32 },
}

impl Carry {
    pub(crate) fn moving(&self) -> Option<&ColumnId> {
        match self {
            Self::Moving { column, .. } => Some(column),
            Self::Pressed { .. } => None,
        }
    }
}

/// A width drag on a header's grip.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Resize {
    pub(crate) column: ColumnId,
    pub(crate) from: f32,
    pub(crate) start: f32,
    pub(crate) width: ColumnWidth,
}

/// The pinned columns' offsets from the leading edge, as drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Pins {
    left: Vec<(ColumnId, f32)>,
}

impl Pins {
    pub(crate) fn new(left: Vec<(ColumnId, f32)>) -> Self {
        Self { left }
    }

    pub(crate) fn left_of(&self, id: &ColumnId) -> Option<f32> {
        self.left.iter().find(|(pinned, _)| pinned == id).map(|(_, left)| *left)
    }

    pub(crate) fn is_edge(&self, id: &ColumnId) -> bool {
        self.left.last().is_some_and(|(pinned, _)| pinned == id)
    }
}

/// Everything one grid's parts share.
pub(crate) struct GridCx<T: 'static, K: 'static>
where
    K: Eq + Hash,
{
    pub(crate) columns: Rc<[GridColumn<T>]>,
    pub(crate) row_key: Rc<dyn Fn(&T) -> K>,
    pub(crate) rows: Signal<Vec<T>, LocalStorage>,
    /// Every row's key in row order, and each key's index, held rather than derived so a scroll
    /// reads them instead of walking the rows.
    pub(crate) order: RwSignal<Rc<Vec<K>>, LocalStorage>,
    pub(crate) positions: RwSignal<Rc<HashMap<K, usize>>, LocalStorage>,

    pub(crate) layout: RwSignal<ColumnLayout, LocalStorage>,
    /// What a release of the carried header would leave.
    pub(crate) preview: RwSignal<Option<ColumnLayout>, LocalStorage>,
    pub(crate) drawn: Memo<ColumnLayout>,
    pub(crate) drawn_ids: Memo<Vec<ColumnId>>,
    pub(crate) pins: Memo<Pins>,
    pub(crate) carry: RwSignal<Option<Carry>, LocalStorage>,
    /// The carried column alone, so a pointer move during a carry wakes no cell.
    pub(crate) carried: Memo<Option<ColumnId>>,
    /// Whether the press that just ended was answered on its release, so the click that may
    /// follow it does not sort again. A click with no press before it is the keyboard's.
    pub(crate) press_answered: RwSignal<bool, LocalStorage>,
    pub(crate) resize: RwSignal<Option<Resize>, LocalStorage>,
    /// Each column's width as last laid out, in CSS pixels.
    pub(crate) widths: RwSignal<HashMap<ColumnId, f32>, LocalStorage>,
    /// The widest each fitted column has been laid out at since the rows last changed.
    pub(crate) fitted: RwSignal<HashMap<ColumnId, f32>, LocalStorage>,

    /// How far each column is drawn from its place while the columns slide to new places.
    pub(crate) shifts: RwSignal<HashMap<ColumnId, f32>, LocalStorage>,
    /// Holds the shifts without a transition for the frame they are written in.
    pub(crate) instant: RwSignal<bool, LocalStorage>,
    pub(crate) slide: Rc<RefCell<Option<Slide>>>,
    pub(crate) settle: Rc<RefCell<Option<FrameHandle>>>,
    /// Under reduced motion the columns take their new places at once.
    pub(crate) motion: Signal<MotionPreference, LocalStorage>,

    pub(crate) sort: Signal<Option<ColumnSort>, LocalStorage>,
    pub(crate) on_sort: Option<UnsyncCallback<Option<ColumnSort>>>,

    pub(crate) selection_mode: GridSelectionMode,
    pub(crate) selection: RwSignal<Selection<K>, LocalStorage>,
    /// The row the keyboard is on.
    pub(crate) cursor: RwSignal<Option<K>, LocalStorage>,
    pub(crate) expanded: RwSignal<HashSet<K>, LocalStorage>,
    pub(crate) detail: Option<RowDetail<T>>,
    pub(crate) fixed: Option<RowHeight>,
    /// The rows a fixed-height grid builds, and where each row sits.
    pub(crate) window: Option<Signal<VirtualWindow, LocalStorage>>,
    pub(crate) on_activate: Option<UnsyncCallback<K>>,

    pub(crate) row_menu: Option<RowMenu<K>>,
    pub(crate) menu_target: RwSignal<Option<MenuTarget<K>>, LocalStorage>,
    /// Whether a header or row named itself the target of the context menu being asked for. The
    /// menu opens only for a target named by the gesture that asks for it.
    pub(crate) menu_claimed: Cell<bool>,
    /// Where the grid's context menu opens from, known once the region around the grid is built.
    pub(crate) menu_place: Cell<Option<ContextMenuPlace>>,
    pub(crate) on_copy: Option<UnsyncCallback<String>>,
    pub(crate) clipboard: Option<Clipboards>,
    pub(crate) strings: GridStrings,

    pub(crate) port: NodeRef,
    pub(crate) scroll: Signal<ScrollPosition, LocalStorage>,
    /// Each built row's filler cell, whose top and bottom are the row's.
    pub(crate) row_cells: RefCell<HashMap<K, NodeRef>>,
    pub(crate) header_height: RwSignal<f32, LocalStorage>,
    /// How wide the grid's view is, as CSS text, for a detail region to stay that wide and in
    /// view while the columns scroll. Watched only when rows have a detail.
    pub(crate) view_width: Signal<Option<String>, LocalStorage>,
    pub(crate) timers: Option<Timers>,
}

impl<T, K> GridCx<T, K>
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    pub(crate) fn column(&self, id: &ColumnId) -> Option<&GridColumn<T>> {
        self.columns.iter().find(|column| column.id() == id)
    }

    /// The surface's device pixels per CSS pixel.
    pub(crate) fn scale(&self) -> f32 {
        let scale = self.port.scale();
        if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
    }

    /// How far the scrolling columns are scrolled, in CSS pixels.
    pub(crate) fn scroll_x(&self) -> f32 {
        self.scroll.with_untracked(|position| position.offset.x.0) / self.scale()
    }

    /// `x`, a pointer position in window CSS pixels, from the grid's leading edge.
    pub(crate) fn pointer_offset(&self, x: f32) -> Option<f32> {
        let port = self.port.window_bounds()?;

        Some(x - port.origin.x.0 / self.scale())
    }

    /// The width `id` is laid out at, as last measured, or what it is set to before it is.
    pub(crate) fn width_of(&self, id: &ColumnId) -> f32 {
        if let Some(width) = self.widths.with_untracked(|widths| widths.get(id).copied()) {
            return width;
        }

        let Some(column) = self.column(id) else {
            return ColumnWidth::MIN.0;
        };
        match self.layout.with_untracked(|layout| layout.width(id)) {
            Some(width) => column.clamp(width.0).0,
            None => match column.sizing_value() {
                ColumnSizing::Fixed(width) => column.clamp(width.0).0,
                ColumnSizing::Fit => column.min_width_value().0,
            },
        }
    }

    /// The grid's `grid-template-columns`: one track per column as drawn, then one that takes up
    /// whatever room is left so a row's ground reaches the far edge.
    pub(crate) fn tracks(&self) -> String {
        let resize = self.resize.get();
        let fitted = self.fitted.get();
        let mut tracks: Vec<String> = self.drawn.with(|drawn| {
            drawn
                .columns()
                .iter()
                .filter_map(|state| {
                    let column = self.column(state.id())?;
                    let dragged =
                        resize.as_ref().filter(|resize| resize.column == *state.id()).map(|resize| resize.width);
                    let track = match dragged.or_else(|| state.width().map(|width| column.clamp(width.0))) {
                        Some(width) => width.css(),
                        None => match column.sizing_value() {
                            ColumnSizing::Fixed(width) => column.clamp(width.0).css(),
                            ColumnSizing::Fit => {
                                let least = fitted.get(state.id()).copied().unwrap_or(0.0);
                                format!("minmax({}, max-content)", column.clamp(least).css())
                            }
                        },
                    };
                    Some(track)
                })
                .collect()
        });
        tracks.push("minmax(0px, 1fr)".to_owned());

        tracks.join(" ")
    }

    /// Notes the width `id`'s header was laid out at.
    pub(crate) fn measured(&self, id: &ColumnId, width: f32) {
        if self.widths.with_untracked(|widths| widths.get(id) != Some(&width)) {
            self.widths.update(|widths| {
                widths.insert(id.clone(), width);
            });
        }

        let fits = self.column(id).is_some_and(|column| column.sizing_value() == ColumnSizing::Fit);
        let wider = self.fitted.with_untracked(|fitted| fitted.get(id).is_none_or(|known| width > *known));
        if fits && wider {
            self.fitted.update(|fitted| {
                fitted.insert(id.clone(), width);
            });
        }
    }

    /// Whether `id` may be pinned: it allows it, some other column is left to scroll, and the
    /// pinned columns with it leave room in the view to reach the rest of the grid.
    pub(crate) fn may_pin(&self, id: &ColumnId) -> bool {
        let Some(column) = self.column(id) else {
            return false;
        };

        let scrolling_left = self
            .layout
            .with_untracked(|layout| layout.columns().iter().any(|state| !state.is_pinned() && state.id() != id));
        let fits = self.pinned_room(id).is_none_or(|room| self.width_of(id) <= room);

        column.is_pinnable() && scrolling_left && fits
    }

    /// How wide `id` may be as a pinned column: what is left of the share of the view the pinned
    /// columns may take once the other pinned columns have theirs. `None` before the view is laid
    /// out.
    pub(crate) fn pinned_room(&self, id: &ColumnId) -> Option<f32> {
        // The rest of the view is left to the scrolling columns and their scrollbar.
        const PINNED_SHARE: f32 = 0.6;

        let view = self.port.bounds()?.size.width.0 / self.scale();
        let others: f32 = self.layout.with_untracked(|layout| {
            layout
                .columns()
                .iter()
                .filter(|state| state.is_pinned() && state.id() != id)
                .map(|state| self.width_of(state.id()))
                .sum()
        });

        Some(view * PINNED_SHARE - others)
    }

    /// `width` as `id` may be resized to: no narrower than its minimum and, while it is pinned, no
    /// wider than [`pinned_room`](Self::pinned_room) leaves it.
    pub(crate) fn resized_width(&self, id: &ColumnId, width: f32) -> Option<ColumnWidth> {
        let column = self.column(id)?;
        let pinned = self.layout.with_untracked(|layout| layout.is_pinned(id));
        let room = if pinned { self.pinned_room(id) } else { None };

        Some(column.clamp(room.map_or(width, |room| width.min(room))))
    }

    /// Commits `next` as the reader's layout, sliding the columns from where they are drawn.
    pub(crate) fn rearrange(&self, next: ColumnLayout) {
        let drawn = self.drawn.get_untracked();
        self.slide(&drawn, &next);
        self.preview.set(None);
        if self.layout.with_untracked(|layout| *layout != next) {
            self.layout.set(next);
        }
    }

    /// Starts every column whose place differs between `from` and `to` sliding from where it is
    /// drawn, including how far along a slide still under way it is, to its place in `to`. Under
    /// reduced motion every column is drawn at its place in `to` at once.
    pub(crate) fn slide(&self, from: &ColumnLayout, to: &ColumnLayout) {
        if self.motion.get_untracked() == MotionPreference::Reduced {
            self.settle.replace(None);
            self.slide.replace(None);
            if self.instant.get_untracked() {
                self.instant.set(false);
            }
            if self.shifts.with_untracked(|shifts| !shifts.is_empty()) {
                self.shifts.set(HashMap::new());
            }
            return;
        }

        let scroll = self.scroll_x();
        let width = |id: &ColumnId| self.width_of(id);
        let moving = self.slide.borrow().as_ref().map(Slide::offsets).unwrap_or_default();
        let before: HashMap<ColumnId, f32> = layout::drawn_starts(from, scroll, width)
            .into_iter()
            .map(|(id, start)| {
                let offset = moving.get(&id).copied().unwrap_or(0.0);
                (id, start + offset)
            })
            .collect();
        let shifts: HashMap<ColumnId, f32> = layout::drawn_starts(to, scroll, width)
            .into_iter()
            .filter_map(|(id, start)| {
                let shift = before.get(&id)? - start;
                (shift.abs() >= 0.5).then_some((id, shift))
            })
            .collect();
        if shifts.is_empty() {
            return;
        }

        // The shifts are drawn for one frame without a transition, then dropped with one, so each
        // column eases from where it was to where it now is. The frame these writes are drawn in
        // runs its callbacks before it styles them, so the drop waits for the frame after.
        self.instant.set(true);
        self.shifts.set(shifts.clone());
        let Some(timers) = self.timers.clone() else {
            self.instant.set(false);
            self.shifts.set(HashMap::new());
            return;
        };
        let instant = self.instant;
        let shift_signal = self.shifts;
        let slide = Rc::clone(&self.slide);
        let settle = Rc::clone(&self.settle);
        let handle = timers.clone().request_frame(move |_| {
            let drop_shifts = timers.clone().request_frame(move |began| {
                instant.set(false);
                shift_signal.set(HashMap::new());
                slide.replace(Some(Slide { began, latest: began, from: shifts, next: None }));
                follow_slide(timers, slide);
            });
            settle.replace(Some(drop_shifts));
        });
        self.settle.replace(Some(handle));
    }

    pub(crate) fn press_header(&self, id: &ColumnId) {
        let Some(on_sort) = self.on_sort.as_ref() else {
            return;
        };

        let current = self.sort.get_untracked();
        on_sort.run(ColumnSort::after_press(current.as_ref(), id));
    }

    /// Selects `key` the way a press with `modifiers` held does.
    pub(crate) fn press_row(&self, key: &K, modifiers: Modifiers) {
        self.cursor.set(Some(key.clone()));
        match self.selection_mode {
            GridSelectionMode::None => {}
            GridSelectionMode::Single => self.selection.update(|selection| selection.select_only(key.clone())),
            GridSelectionMode::Multiple if modifiers.shift() => {
                let order = self.order.get_untracked();
                self.selection.update(|selection| selection.select_run(key.clone(), &order));
            }
            GridSelectionMode::Multiple if modifiers.primary() => {
                self.selection.update(|selection| selection.toggle(key.clone()));
            }
            GridSelectionMode::Multiple => self.selection.update(|selection| selection.select_only(key.clone())),
        }
    }

    /// Activates `key`: tells the application, or with no one to tell, opens or closes the row.
    pub(crate) fn activate(&self, key: &K) {
        match (&self.on_activate, &self.detail) {
            (Some(activate), _) => activate.run(key.clone()),
            (None, Some(_)) => self.toggle_expanded(key),
            (None, None) => {}
        }
    }

    pub(crate) fn toggle_expanded(&self, key: &K) {
        let open = self.expanded.with_untracked(|expanded| expanded.contains(key));
        self.set_expanded(key, !open);
    }

    pub(crate) fn set_expanded(&self, key: &K, open: bool) {
        if self.detail.is_none() || self.expanded.with_untracked(|expanded| expanded.contains(key)) == open {
            return;
        }

        self.expanded.update(|expanded| {
            if open {
                expanded.insert(key.clone());
            } else {
                expanded.remove(key);
            }
        });
    }

    /// The selected rows as tab-separated text, in row order, to the application or the
    /// clipboard.
    pub(crate) fn copy(&self) {
        let columns: Vec<&GridColumn<T>> =
            self.drawn_ids.with_untracked(|ids| ids.iter().filter_map(|id| self.column(id)).collect());
        let text = self.selection.with_untracked(|selection| {
            self.rows.with_untracked(|rows| {
                let selected = rows.iter().filter(|row| selection.contains(&(self.row_key)(row)));
                copy::tsv(selected, &columns)
            })
        });
        if text.is_empty() {
            return;
        }

        match (&self.on_copy, &self.clipboard) {
            (Some(on_copy), _) => on_copy.run(text),
            (None, Some(clipboard)) => clipboard.set_text(ClipboardKind::Standard, text),
            (None, None) => {}
        }
    }

    /// Moves the keyboard to the row `step` rows from the one it is on, selecting as a press
    /// with `modifiers` does, and brings it into view.
    pub(crate) fn step_cursor(&self, step: Step, modifiers: Modifiers) {
        let order = self.order.get_untracked();
        let Some(last) = order.len().checked_sub(1) else {
            return;
        };

        let current = self.cursor.with_untracked(|cursor| {
            cursor.as_ref().and_then(|key| self.positions.with_untracked(|p| p.get(key).copied()))
        });
        let index = match (step, current) {
            (Step::First, _) => 0,
            (Step::Last, _) => last,
            (Step::Previous, Some(index)) => index.saturating_sub(1),
            (Step::Next, Some(index)) => (index + 1).min(last),
            (Step::Previous | Step::Next, None) => 0,
        };
        let key = order[index].clone();

        // A plain arrow moves the selection with it, as a press does; ctrl moves only the
        // keyboard.
        if modifiers.primary() && self.selection_mode == GridSelectionMode::Multiple {
            self.cursor.set(Some(key.clone()));
        } else {
            self.press_row(&key, modifiers);
        }
        self.reveal(&key, index);
    }

    /// Scrolls the grid up or down so row `key`, at `index`, is in view below the header.
    pub(crate) fn reveal(&self, key: &K, index: usize) {
        let scale = self.scale();
        let header = self.header_height.get_untracked();
        let position = self.scroll.get_untracked();
        let view_top = position.offset.y.0 / scale + header;
        let view_bottom = (position.offset.y.0 + position.scrollport.height.0) / scale;

        let span = match (self.fixed, self.window) {
            (Some(height), Some(window)) => {
                let top = header + window.with_untracked(|window| window.offset_of(index));
                Some((top, top + height.0))
            }
            (Some(_), None) | (None, _) => {
                let cell = self.row_cells.borrow().get(key).copied();
                let port = self.port.window_bounds();
                cell.and_then(|cell| cell.window_bounds()).zip(port).map(|(cell, port)| {
                    let top = (cell.origin.y.0 - port.origin.y.0) / scale + position.offset.y.0 / scale;
                    (top, top + cell.size.height.0 / scale)
                })
            }
        };
        let Some((top, bottom)) = span else {
            return;
        };

        let by = if top < view_top {
            top - view_top
        } else if bottom > view_bottom {
            bottom - view_bottom
        } else {
            return;
        };
        self.port.scroll_to(
            ScrollTarget::By(zgui::geom::Point::new(zgui::geom::DevicePx(0.0), zgui::geom::DevicePx(by * scale))),
            ScrollBehavior::Instant,
        );
    }
}

/// A slide under way: when its transition began, the latest frame drawn since, and how far each
/// column started from its place.
pub(crate) struct Slide {
    began: Timestamp,
    latest: Timestamp,
    from: HashMap<ColumnId, f32>,
    /// The next frame that notes the time, held so dropping the slide stops following it.
    next: Option<FrameHandle>,
}

impl Slide {
    /// How far each column is drawn from its place as of the latest frame.
    fn offsets(&self) -> HashMap<ColumnId, f32> {
        let duration = Movement::Rearrange.full_duration().as_secs_f32();
        let elapsed = self.latest.since_origin().saturating_sub(self.began.since_origin()).as_secs_f32();
        let eased = ease(CubicBezier::STANDARD, (elapsed / duration).min(1.0));

        self.from.iter().map(|(id, offset)| (id.clone(), offset * (1.0 - eased))).collect()
    }
}

/// Notes the time of each frame while `slide` runs, and drops it once it has run its course.
fn follow_slide(timers: Timers, slide: Rc<RefCell<Option<Slide>>>) {
    let following = Rc::clone(&slide);
    let handle = timers.clone().request_frame(move |at| {
        let done = match following.borrow_mut().as_mut() {
            Some(slide) => {
                slide.latest = at;
                at.since_origin() >= slide.began.since_origin() + Movement::Rearrange.full_duration()
            }
            None => return,
        };
        if done {
            following.replace(None);
            return;
        }
        follow_slide(timers, following);
    });
    if let Some(slide) = slide.borrow_mut().as_mut() {
        slide.next = Some(handle);
    }
}

/// How far along `curve` is at `progress` of its time, by bisecting its x for `progress`.
fn ease(curve: CubicBezier, progress: f32) -> f32 {
    let point = |t: f32, first: f32, second: f32| {
        let rest = 1.0 - t;
        3.0 * rest * rest * t * first + 3.0 * rest * t * t * second + t * t * t
    };

    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if point(middle, curve.x1, curve.x2) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }

    point((low + high) / 2.0, curve.y1, curve.y2)
}

/// Where a key moves the keyboard to among the rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    First,
    Previous,
    Next,
    Last,
}

/// The ids of `columns` in their order, for a layout.
pub(crate) fn known_ids<T>(columns: &[GridColumn<T>]) -> Arc<[ColumnId]> {
    columns.iter().map(|column| column.id().clone()).collect()
}

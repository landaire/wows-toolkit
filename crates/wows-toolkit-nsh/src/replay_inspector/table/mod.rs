//! `PlayerTable`: one open replay's players. Rows sort by a header click,
//! expand to each column's detail, and carry a row menu; columns fit their
//! content, resize by their header's trailing edge (kept in the settings),
//! and all but Name and Ship scroll sideways under a bar along the bottom.

mod colors;
mod detail;
mod icons;
mod parts;
mod row;

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use nsh_base::HandleEdge;
use nsh_base::resize_handle;
use nsh_core::keymap::Modifiers;
use nsh_core::prelude::*;
use nsh_core::time::Instant;
use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::player_table::COLUMN_DRAG_MIN;
use wows_toolkit_replay_data::player_table::STICKY_COLUMN_COUNT;
use wows_toolkit_replay_data::player_table::STICKY_MAX_FRACTION;
use wows_toolkit_replay_data::player_table::TextMeasure;
use wows_toolkit_replay_data::player_table::TextWeight;
use wows_toolkit_replay_data::player_table::column_label;
use wows_toolkit_replay_data::player_table::markdown_table;
use wows_toolkit_replay_data::player_table::measure_column_widths;
use wows_toolkit_replay_data::player_table::row_text;
use wows_toolkit_replay_data::player_table::table_text;
use wows_toolkit_replay_data::sort::SortColumn;
use wows_toolkit_replay_data::sort::SortOrder;
use wows_toolkit_replay_data::sort::sort_rows;

use crate::icons::Icon;
use crate::icons::icon;
use crate::replay_inspector::columns::column_from_key;
use crate::replay_inspector::document;
use crate::replay_inspector::document::ReplayDocId;
use crate::replay_inspector::document::ReplayDocument;
use crate::replay_inspector::settings;
use crate::replay_inspector::table::icons::TableIcons;
use crate::replay_inspector::table::parts::class;

/// The props of [`PlayerTable`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct PlayerTableProps {
    /// The open replay to show, by the id `ReplayInspectorService.open`
    /// gave. None shows nothing.
    pub document: Prop<Option<ReplayDocId>>,
    /// Lifts the NDA and the enemy build gate, and offers the raw player
    /// metadata.
    #[prop(default)]
    pub debug: Prop<bool>,
    /// The visible columns by their keys (`columns::column_key`), in
    /// `ReplayColumn::ALL` order.
    #[prop(default)]
    pub columns: Prop<Vec<SharedString>>,
    /// Hides the recording player's own figures (a test ship's NDA).
    #[prop(default)]
    pub hide_self_stats: Prop<bool>,
}

/// The events of a [`PlayerTable`].
#[derive(Debug, Clone, PartialEq, Event)]
pub enum PlayerTableEvent {
    /// A row's raw metadata, pretty-printed, to show in the raw JSON panel.
    #[args(json)]
    ViewRawJson(SharedString),
    /// A copy confirmation for the app to show.
    #[args(text)]
    Notice(SharedString),
}

/// The player table.
#[component]
pub fn PlayerTable(props: PlayerTableProps, cx: Scope<PlayerTableEvent>) -> impl IntoElement {
    let table = Table::new(props, cx);
    table.load_saved_widths();
    table.load_icons();

    view()
        .app_class(class("flex-1 min-h-0 min-w-0"))
        .child(when(move || table.document.with(Option::is_some), move || table.view()))
}

/// An open document, compared by identity.
#[derive(Clone)]
struct DocHandle(Rc<ReplayDocument>);

impl PartialEq for DocHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The document's rows in display order, with the self row's NDA toggle
/// applied. Compared by identity: every sort is a new list.
#[derive(Clone, Default)]
struct Rows(Rc<Vec<PlayerRow>>);

impl PartialEq for Rows {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// A column-width drag in progress: the column and the width it started at.
#[derive(Debug, Clone, Copy)]
struct ColumnDrag {
    column: ReplayColumn,
    from: f32,
}

/// A press on a resize grip that did not move, which a second one soon
/// after turns into a reset.
#[derive(Debug, Clone, Copy)]
struct GripTap {
    column: ReplayColumn,
    at: Instant,
}

/// The table's state. `Copy`: a bundle of signal handles.
#[derive(Clone, Copy)]
struct Table {
    cx: Scope<PlayerTableEvent>,
    debug: Memo<bool>,
    document: Memo<Option<DocHandle>>,
    columns: Memo<Vec<ReplayColumn>>,
    sort: Signal<SortOrder>,
    rows: Memo<Rows>,
    /// Rows showing their detail, by player so a sort keeps them.
    expanded: Signal<HashSet<AccountId>>,
    /// The row a secondary-modifier click picked out; one at a time.
    selected: Signal<Option<AccountId>>,
    /// Alt turns the damage shares around (of the other player's total).
    alt_held: Signal<bool>,
    /// Each column's content-fit width, by `ReplayColumn as usize`.
    fit: Memo<Vec<f32>>,
    /// Widths the reader dragged, by `ReplayColumn as usize`; saved.
    overrides: Signal<Vec<Option<f32>>>,
    drag: Stored<Option<ColumnDrag>>,
    grip_tap: Stored<Option<GripTap>>,
    /// The cell under the pointer, which a row menu's Copy Cell copies.
    hovered: Stored<Option<(AccountId, ReplayColumn)>>,
    /// The table's own width, for the frozen columns' cap.
    viewport_width: Signal<Option<f32>>,
    /// How far the scrolling columns are scrolled.
    scroll_x: Signal<f32>,
    icons: Signal<TableIcons>,
}

/// How far a grip press may move and still be a tap.
const TAP_SLOP: f32 = 4.0;

/// The longest between two grip taps that reset the column.
const DOUBLE_TAP: Duration = Duration::from_millis(400);

/// The cell text size the widths are measured at (`text-sm`), and the
/// spacing scale's root (`gap-1` is a quarter of it).
const CELL_FONT_SIZE: f32 = 14.0;
const REM: f32 = 16.0;

/// The platform text engine, at the cells' size as the user's text scale
/// draws it.
struct EngineMeasure {
    text_scale: f32,
}

impl TextMeasure for EngineMeasure {
    fn width(&mut self, text: &str, weight: TextWeight) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let size = CELL_FONT_SIZE * self.text_scale;
        let font = match weight {
            TextWeight::Regular => format!("{size}px system-ui"),
            TextWeight::Bold => format!("bold {size}px system-ui"),
        };

        measure_text(&font, text).0
    }

    fn rem(&self) -> f32 {
        REM
    }

    fn text_scale(&self) -> f32 {
        self.text_scale
    }
}

/// The platform's secondary modifier: Command on Apple keyboards, Control
/// elsewhere.
fn secondary(modifiers: &Modifiers) -> bool {
    if apple_keys() { modifiers.cmd } else { modifiers.ctrl }
}

impl Table {
    fn new(props: PlayerTableProps, cx: Scope<PlayerTableEvent>) -> Self {
        let debug = computed(move || props.debug.get());
        let document = computed(move || {
            document::get(props.document.get()?).map(DocHandle)
        });
        let columns = computed(move || {
            props
                .columns
                .get()
                .iter()
                .filter_map(|key| {
                    let column = column_from_key(key);
                    if column.is_none() {
                        tracing::warn!(key = key.as_ref(), "player table: unknown column key");
                    }
                    column
                })
                .collect::<Vec<_>>()
        });
        let sort = signal(SortOrder::default());
        let rows = computed(move || {
            let Some(document) = document.get() else {
                return Rows::default();
            };
            let model = &document.0.model;
            let mut rows = model.rows.clone();
            let hidden = props.hide_self_stats.get();
            if let Some(row) = rows.iter_mut().find(|row| row.is_self) {
                row.manual_stat_hide_toggle = hidden;
            }
            sort_rows(&mut rows, model.self_team, sort.get(), debug.get());

            Rows(Rc::new(rows))
        });
        let expanded = signal(HashSet::new());
        let fit = computed(move || {
            let rows = rows.get();
            let columns = columns.get();
            let debug = debug.get();
            let mut measure = EngineMeasure { text_scale: font_scale() };
            expanded.with(|expanded| measure_column_widths(&rows.0, &columns, debug, expanded, &mut measure))
        });

        Self {
            cx,
            debug,
            document,
            columns,
            sort,
            rows,
            expanded,
            selected: signal(None),
            alt_held: signal(false),
            fit,
            overrides: signal(vec![None; ReplayColumn::ALL.len()]),
            drag: stored(None),
            grip_tap: stored(None),
            hovered: stored(None),
            viewport_width: signal(None),
            scroll_x: signal(0.0),
            icons: signal(TableIcons::default()),
        }
    }

    /// Reads the dragged widths back once. A width narrower than the grip
    /// would leave a column that cannot be grabbed again.
    fn load_saved_widths(self) {
        spawn(async move {
            let saved = match blocking(settings::load_column_widths).await {
                Ok(Ok(saved)) => saved,
                Ok(Err(source)) => {
                    tracing::warn!("player table: saved column widths could not be read: {source}");
                    return;
                }
                Err(source) => {
                    tracing::warn!("player table: reading the column widths failed: {source}");
                    return;
                }
            };
            self.overrides.update(|overrides| {
                for (slot, width) in overrides.iter_mut().zip(saved) {
                    *slot = width.map(|width| width.max(COLUMN_DRAG_MIN));
                }
            });
        });
    }

    fn save_widths(self) {
        let widths = self.overrides.get_untracked();
        spawn(async move {
            match blocking(move || settings::save_column_widths(&widths)).await {
                Ok(Ok(())) => {}
                Ok(Err(source)) => tracing::warn!("player table: column widths could not be saved: {source}"),
                Err(source) => tracing::warn!("player table: saving the column widths failed: {source}"),
            }
        });
    }

    /// Reads the document's icons whenever the document changes. A read that
    /// finishes after another document came is dropped.
    fn load_icons(self) {
        effect(move || {
            self.icons.set(TableIcons::default());
            let Some(document) = self.document.get() else {
                return;
            };
            let Some(vfs) = document.0.vfs.clone() else {
                return;
            };
            spawn(async move {
                let icons = icons::load(&document.0.model.rows, vfs).await;
                if self.document.get_untracked().as_ref() == Some(&document) {
                    self.icons.set(icons);
                }
            });
        });
    }

    fn notice(self, text: impl Into<SharedString>) {
        self.cx.emit(PlayerTableEvent::Notice(text.into()));
    }

    /// Puts `text` on the clipboard and says so.
    fn copy(self, text: String) {
        set_clipboard(text);
        self.notice(t!("ui.replay.context.copied").into_owned());
    }

    fn sort_by(self, column: SortColumn) {
        self.sort.update(|sort| {
            sort.update_column(column);
        });
    }

    fn toggle_expanded(self, player: AccountId) {
        self.expanded.update(|expanded| {
            if !expanded.remove(&player) {
                expanded.insert(player);
            }
        });
    }

    fn toggle_selected(self, player: AccountId) {
        self.selected.update(|selected| {
            *selected = if *selected == Some(player) { None } else { Some(player) };
        });
    }

    /// The widest the frozen columns may be drawn. `None` before the first
    /// layout.
    fn sticky_cap(self) -> Option<f32> {
        self.viewport_width.get().map(|width| width * STICKY_MAX_FRACTION)
    }

    /// The width `col` is drawn at: dragged, else fitted. Under the frozen
    /// cap Name gives way first, its long names ellipsized, so the ship
    /// names beside it stay readable and the scrolling columns reachable.
    fn width(self, col: ReplayColumn) -> f32 {
        let width = self.wanted_width(col);
        let (ReplayColumn::Name, Some(cap)) = (col, self.sticky_cap()) else {
            return width;
        };
        let ship = if self.columns.with(|columns| columns.contains(&ReplayColumn::ShipName)) {
            self.wanted_width(ReplayColumn::ShipName)
        } else {
            0.0
        };

        width.min((cap - ship).max(COLUMN_DRAG_MIN))
    }

    /// What `col` asks for: its dragged width, else its fitted one.
    fn wanted_width(self, col: ReplayColumn) -> f32 {
        let fitted = self.fit.with(|fit| fit[col as usize]);
        self.overrides.with(|overrides| overrides[col as usize]).unwrap_or(fitted)
    }

    fn sticky_columns(self) -> Vec<ReplayColumn> {
        self.columns.with(|columns| columns.iter().take(STICKY_COLUMN_COUNT).copied().collect())
    }

    fn scroll_columns(self) -> Vec<ReplayColumn> {
        self.columns.with(|columns| columns.iter().skip(STICKY_COLUMN_COUNT).copied().collect())
    }

    /// The frozen section's drawn width: its columns, up to the cap.
    fn sticky_width(self) -> f32 {
        let total: f32 = self.sticky_columns().into_iter().map(|col| self.width(col)).sum();
        match self.sticky_cap() {
            Some(cap) => total.min(cap),
            None => total,
        }
    }

    fn scroll_width(self) -> f32 {
        self.scroll_columns().into_iter().map(|col| self.width(col)).sum()
    }

    fn start_drag(self, column: ReplayColumn) {
        let from = untracked(|| self.width(column));
        self.drag.replace(Some(ColumnDrag { column, from }));
    }

    fn follow_drag(self, offset: f32) {
        let Some(drag) = self.drag.with(|drag| *drag) else {
            return;
        };
        let width = (drag.from + offset).max(COLUMN_DRAG_MIN);
        self.overrides.update(|overrides| overrides[drag.column as usize] = Some(width));
    }

    /// Ends a drag. One that did not move is a tap, and a second tap on the
    /// same grip soon after puts the column back on its content.
    fn end_drag(self, offset: f32) {
        let Some(drag) = self.drag.replace(None) else {
            return;
        };
        if offset.abs() > TAP_SLOP {
            self.grip_tap.replace(None);
            self.save_widths();
            return;
        }

        let now = nsh_core::time::now();
        match self.grip_tap.replace(None) {
            Some(tap) if tap.column == drag.column && now.duration_since(tap.at) < DOUBLE_TAP => {
                self.reset_width(drag.column);
            }
            _ => {
                self.grip_tap.replace(Some(GripTap { column: drag.column, at: now }));
            }
        }
    }

    fn reset_width(self, column: ReplayColumn) {
        let had = self.overrides.with_untracked(|overrides| overrides[column as usize].is_some());
        if !had {
            return;
        }
        self.overrides.update(|overrides| overrides[column as usize] = None);
        self.save_widths();
    }

    fn row(self, player: AccountId) -> Option<PlayerRow> {
        self.rows.with_untracked(|rows| rows.0.iter().find(|row| row.db_id == player).cloned())
    }

    fn copy_cell(self, player: AccountId, column: ReplayColumn) {
        let Some(row) = self.row(player) else {
            return;
        };
        let debug = self.debug.get_untracked();
        self.copy(wows_toolkit_replay_data::cells::cell_value(&row, column, debug).text);
    }

    fn copy_row(self, player: AccountId) {
        let Some(row) = self.row(player) else {
            return;
        };
        let columns = self.columns.get_untracked();
        self.copy(row_text(&row, &columns, self.debug.get_untracked()));
    }

    fn copy_table(self) {
        let rows = self.rows.get_untracked();
        let columns = self.columns.get_untracked();
        self.copy(table_text(&rows.0, &columns, self.debug.get_untracked()));
    }

    fn copy_row_markdown(self, player: AccountId) {
        let Some(row) = self.row(player) else {
            return;
        };
        let columns = self.columns.get_untracked();
        self.copy(markdown_table([&row], &columns, self.debug.get_untracked()));
    }

    fn copy_table_markdown(self) {
        let rows = self.rows.get_untracked();
        let columns = self.columns.get_untracked();
        self.copy(markdown_table(rows.0.iter(), &columns, self.debug.get_untracked()));
    }

    fn view(self) -> AnyElement {
        let body = virtual_list(move || self.rows.with(|rows| rows.0.len()), move |at: Position| row::row(self, at))
            .keys(move || self.rows.with(|rows| rows.0.iter().map(|row| row.db_id).collect::<Vec<_>>()))
            .estimated_item_size(px(28.0))
            .app_class(class("flex-1 min-h-0"));

        view()
            .app_class(class("flex-1 min-h-0 min-w-0 overflow-hidden"))
            .role(AccessibilityRole::Table)
            .on_layout(move |bounds: &Bounds<Points>| self.viewport_width.set(Some(bounds.size.width.0)))
            // Fires when a modifier changes while the pointer rests here too.
            .on_mouse_move(move |event: &MouseEvent| {
                if self.alt_held.get_untracked() != event.modifiers.alt {
                    self.alt_held.set(event.modifiers.alt);
                }
            })
            // A table the pointer left never hears the alt release.
            .on_mouse_leave(move |_: &MouseEvent| self.alt_held.set(false))
            .child(self.header())
            .child(body)
            .child(self.scroll_strip())
            .into_element()
    }

    fn header(self) -> AnyElement {
        let sticky = dynamic(move || {
            self.sticky_columns().into_iter().map(|col| self.header_cell(col)).collect::<Vec<_>>()
        });
        let scrolling = dynamic(move || {
            self.scroll_columns().into_iter().map(|col| self.header_cell(col)).collect::<Vec<_>>()
        });

        view()
            .app_class(class("flex-row flex-none w-full border-b border-border"))
            .child(
                view()
                    .app_class(class("flex-row flex-none overflow-hidden"))
                    .w(move || Length::from(px(self.sticky_width())))
                    .child(sticky),
            )
            .child(
                view().app_class(class("flex-1 min-w-0 overflow-hidden")).child(
                    view()
                        .app_class(class("flex-row flex-none"))
                        .w(move || Length::from(px(self.scroll_width())))
                        .translate_x(move || DefiniteLength::from(px(-self.scroll_x.get())))
                        .child(scrolling),
                ),
            )
            .into_element()
    }

    /// A column title: its label (a sort button where the column sorts),
    /// the sort direction when it is the sort, and the resize grip.
    fn header_cell(self, col: ReplayColumn) -> AnyElement {
        let label = column_label(col);
        let sort_column = SortColumn::of_column(col);
        let title = view()
            .app_class(class(if sort_column.is_some() {
                "flex-1 min-w-0 flex-row items-center gap-1 px-1 py-1 overflow-hidden hover:bg-accent"
            } else {
                "flex-1 min-w-0 flex-row items-center gap-1 px-1 py-1 overflow-hidden"
            }))
            .accessible(true)
            .label(label.clone())
            .child(text(label).app_class(class("text-sm font-bold text-foreground truncate")));

        let title = match sort_column {
            Some(sort_column) => title
                .role(AccessibilityRole::Button)
                .cursor_pointer()
                .on_press(move |_: &PressEvent| self.sort_by(sort_column))
                .child(dynamic(move || {
                    let sort = self.sort.get();
                    if sort.column() != sort_column {
                        return Vec::new();
                    }
                    let caret = match sort {
                        SortOrder::Asc(_) => Icon::ListSortAscending,
                        SortOrder::Desc(_) => Icon::ListSortDescending,
                    };
                    vec![icon(caret, px(14.0)).app_class(class("flex-none text-foreground")).into_element()]
                })),
            None => title.role(AccessibilityRole::ColumnHeader),
        };

        let grip = resize_handle()
            .edge(HandleEdge::Trailing)
            .accessibility_label(format!("Resize {}", column_label(col)))
            .on_drag_start(move || self.start_drag(col))
            .on_drag_move(move |offset: &Points| self.follow_drag(offset.0))
            .on_drag_end(move |offset: &Points| self.end_drag(offset.0))
            .child(view().app_class(class("w-px h-full group-hover:bg-ring group-active:bg-ring")));

        view()
            .app_class(class("relative flex-row flex-none overflow-hidden"))
            .w(move || Length::from(px(self.width(col))))
            .child(title)
            .child(grip)
            .into_element()
    }

    /// The bar the scrolling columns move by, under them: the frozen columns
    /// to its left do not move.
    fn scroll_strip(self) -> AnyElement {
        view()
            .app_class(class("flex-row flex-none w-full h-3"))
            .child(view().app_class(class("flex-none")).w(move || Length::from(px(self.sticky_width()))))
            .child(
                scroll()
                    .horizontal()
                    .app_class(class("flex-1 min-w-20"))
                    .on_scroll(move |event: &ScrollEvent| self.scroll_x.set(event.offset.x.0))
                    .child(view().app_class(class("h-px")).w(move || Length::from(px(self.scroll_width())))),
            )
            .into_element()
    }
}

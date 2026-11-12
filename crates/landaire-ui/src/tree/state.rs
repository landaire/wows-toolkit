//! What a tree remembers between events: which nodes are open, which are selected, where the
//! keyboard cursor is, and the rows those make visible.

use std::collections::HashSet;
use std::hash::Hash;

use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Modifiers;
use zgui_ui::prelude::Binding;

use crate::surface::RowIndex;
use crate::tree::flat::FlatRows;
use crate::tree::flat::NodeKind;
use crate::tree::keys::Travel;
use crate::tree::model::TreeModel;

/// How many rows a tree lets a person select at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectionMode {
    /// One row. A modified click or key selects the one row as a plain one does.
    Single,
    /// Any rows: the primary modifier adds and removes one, shift selects a run.
    #[default]
    Multiple,
}

/// A set of keys held by the application or by the tree.
pub(crate) enum KeySet<K: 'static> {
    Signal(RwSignal<HashSet<K>, LocalStorage>),
    Controlled { read: Signal<HashSet<K>, LocalStorage>, write: UnsyncCallback<HashSet<K>> },
}

impl<K: 'static> Clone for KeySet<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: 'static> Copy for KeySet<K> {}

impl<K> KeySet<K>
where
    K: Clone + Eq + Hash + 'static,
{
    /// The set `binding` names, or a new empty one the tree keeps when it names none.
    pub fn from_binding(binding: Binding<HashSet<K>>) -> Self {
        match binding {
            Binding::Unbound => Self::Signal(RwSignal::new_local(HashSet::new())),
            Binding::TwoWay(signal) => Self::Signal(signal),
            Binding::Controlled { read, write } => Self::Controlled { read, write },
        }
    }

    /// Whether `key` is in the set, subscribing to it. A set whose scope is gone holds nothing.
    pub fn contains(&self, key: &K) -> bool {
        match self {
            Self::Signal(signal) => signal.try_with(|set| set.contains(key)),
            Self::Controlled { read, .. } => read.try_with(|set| set.contains(key)),
        }
        .unwrap_or(false)
    }

    pub fn contains_untracked(&self, key: &K) -> bool {
        match self {
            Self::Signal(signal) => signal.try_with_untracked(|set| set.contains(key)),
            Self::Controlled { read, .. } => read.try_with_untracked(|set| set.contains(key)),
        }
        .unwrap_or(false)
    }

    /// A copy of the set, subscribing to it.
    pub fn snapshot(&self) -> Option<HashSet<K>> {
        match self {
            Self::Signal(signal) => signal.try_get(),
            Self::Controlled { read, .. } => read.try_get(),
        }
    }

    pub fn update(&self, change: impl FnOnce(&mut HashSet<K>)) {
        match self {
            Self::Signal(signal) => {
                signal.try_update(change);
            }
            Self::Controlled { read, write } => {
                let Some(mut next) = read.try_get_untracked() else {
                    return;
                };
                change(&mut next);
                write.run(next);
            }
        }
    }

    pub fn replace(&self, next: HashSet<K>) {
        match self {
            Self::Signal(signal) => {
                signal.try_set(next);
            }
            Self::Controlled { write, .. } => write.run(next),
        }
    }
}

/// Everything a tree's rows and handlers share.
pub(crate) struct TreeState<M: TreeModel> {
    pub model: Signal<M, LocalStorage>,
    pub rows: RwSignal<FlatRows<M::Key>, LocalStorage>,
    pub expanded: KeySet<M::Key>,
    pub selection: KeySet<M::Key>,
    pub mode: SelectionMode,
    /// The row the keyboard acts on. Its own value, apart from the selection, so the primary
    /// modifier can move it without selecting.
    pub cursor: RwSignal<Option<M::Key>, LocalStorage>,
    /// Where a shift-selected run starts: the row last selected without shift.
    pub anchor: RwSignal<Option<M::Key>, LocalStorage>,
}

impl<M: TreeModel> Clone for TreeState<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M: TreeModel> Copy for TreeState<M> {}

impl<M: TreeModel> TreeState<M> {
    pub fn new(
        model: Signal<M, LocalStorage>,
        expanded: Binding<HashSet<M::Key>>,
        selection: Binding<HashSet<M::Key>>,
        mode: SelectionMode,
    ) -> Self {
        Self {
            model,
            rows: RwSignal::new_local(FlatRows::new()),
            expanded: KeySet::from_binding(expanded),
            selection: KeySet::from_binding(selection),
            mode,
            cursor: RwSignal::new_local(None),
            anchor: RwSignal::new_local(None),
        }
    }

    /// The cursor's row. A cursor inside a collapsed node is on that node's nearest visible
    /// ancestor, the row that hid it.
    pub fn cursor_index(&self) -> Option<RowIndex> {
        let key = self.cursor.get_untracked()?;

        self.visible_index(&key)
    }

    /// The row showing `key`, or else its nearest visible ancestor.
    fn visible_index(&self, key: &M::Key) -> Option<RowIndex> {
        self.rows.with_untracked(|rows| {
            self.model.with_untracked(|model| {
                let mut current = key.clone();
                loop {
                    if let Some(index) = rows.position(&current) {
                        return Some(index);
                    }
                    current = model.parent(&current)?;
                }
            })
        })
    }

    /// Moves a cursor that a collapse or a model change hid onto the row that hid it, or drops
    /// it when no ancestor is visible either.
    pub fn settle_cursor(&self) {
        let Some(key) = self.cursor.get_untracked() else {
            return;
        };
        if self.rows.with_untracked(|rows| rows.position(&key).is_some()) {
            return;
        }

        let settled = self.visible_index(&key).and_then(|index| self.key_at(index));
        self.cursor.set(settled);
    }

    /// Puts a cursor that is on no row where focus arriving at the tree lands: the first visible
    /// selected row, or else the first row. Returns the cursor's row.
    pub fn enter(&self) -> Option<RowIndex> {
        if let Some(index) = self.cursor_index() {
            return Some(index);
        }

        let first = self.rows.with_untracked(|rows| {
            let selected = rows.keys().position(|key| self.selection.contains_untracked(key));
            let index = RowIndex(selected.unwrap_or(0));
            rows.get(index).map(|row| (index, row.key.clone()))
        });
        let (index, key) = first?;
        if self.anchor.get_untracked().is_none() {
            self.anchor.set(Some(key.clone()));
        }
        self.cursor.set(Some(key));

        Some(index)
    }

    pub fn key_at(&self, index: RowIndex) -> Option<M::Key> {
        self.rows.with_untracked(|rows| rows.get(index).map(|row| row.key.clone()))
    }

    pub fn select_only(&self, key: M::Key) {
        self.selection.replace(HashSet::from([key.clone()]));
        self.anchor.set(Some(key.clone()));
        self.cursor.set(Some(key));
    }

    /// Adds `key` to the selection or takes it out.
    pub fn toggle_selected(&self, key: M::Key) {
        if self.mode == SelectionMode::Single {
            self.select_only(key);
            return;
        }

        let toggled = key.clone();
        self.selection.update(move |set| {
            if !set.remove(&toggled) {
                set.insert(toggled);
            }
        });
        self.anchor.set(Some(key.clone()));
        self.cursor.set(Some(key));
    }

    /// Selects the visible rows from the anchor to `to`, in place of the selection or, when
    /// `adding`, beside it.
    pub fn select_run(&self, to: RowIndex, adding: bool) {
        let Some(key) = self.key_at(to) else {
            return;
        };

        if self.mode == SelectionMode::Single {
            self.select_only(key);
            return;
        }

        // With no anchor on screen the run is the one row, as a plain selection is.
        let from =
            self.anchor.get_untracked().and_then(|anchor| self.rows.with_untracked(|rows| rows.position(&anchor)));
        let Some(from) = from else {
            self.select_only(key);
            return;
        };

        let run: HashSet<M::Key> = self.rows.with_untracked(|rows| rows.keys_between(from..=to).cloned().collect());
        if adding {
            self.selection.update(move |set| set.extend(run));
        } else {
            self.selection.replace(run);
        }
        self.cursor.set(Some(key));
    }

    /// What a press on `key` with `modifiers` held selects.
    pub fn press(&self, key: M::Key, modifiers: Modifiers) {
        if modifiers.shift() {
            let index = self.rows.with_untracked(|rows| rows.position(&key));
            match index {
                Some(index) => self.select_run(index, modifiers.primary()),
                None => self.select_only(key),
            }
            return;
        }

        if modifiers.primary() {
            self.toggle_selected(key);
            return;
        }

        self.select_only(key);
    }

    pub fn select_all(&self) {
        if self.mode == SelectionMode::Single {
            return;
        }

        let every: HashSet<M::Key> = self.rows.with_untracked(|rows| rows.keys().cloned().collect());
        self.selection.replace(every);
    }

    /// Moves the cursor by `travel`, `page` rows being one page, and selects as `modifiers`
    /// ask. Returns the row it landed on.
    pub fn travel(&self, travel: Travel, modifiers: Modifiers, page: usize) -> Option<RowIndex> {
        let count = self.rows.with_untracked(FlatRows::len);
        let last = count.checked_sub(1)?;
        let current = self.cursor_index();

        let target = match (travel, current) {
            (Travel::First, _) | (Travel::Next | Travel::PageDown, None) => 0,
            (Travel::Last, _) | (Travel::Previous | Travel::PageUp, None) => last,
            (Travel::Previous, Some(at)) => at.0.saturating_sub(1),
            (Travel::Next, Some(at)) => (at.0 + 1).min(last),
            (Travel::PageUp, Some(at)) => at.0.saturating_sub(page),
            (Travel::PageDown, Some(at)) => (at.0 + page).min(last),
        };
        let target = RowIndex(target);

        self.land(target, modifiers);

        Some(target)
    }

    /// Puts the cursor on `index` and selects as `modifiers` ask: shift extends the run, the
    /// primary modifier leaves the selection alone.
    pub fn land(&self, index: RowIndex, modifiers: Modifiers) {
        let Some(key) = self.key_at(index) else {
            return;
        };

        if modifiers.shift() {
            self.select_run(index, modifiers.primary());
        } else if modifiers.primary() && self.mode == SelectionMode::Multiple {
            self.cursor.set(Some(key));
        } else {
            self.select_only(key);
        }
    }

    /// Expands the cursor's node, or steps to its first child when it is already open. Returns
    /// the row the cursor moved to.
    pub fn open(&self) -> Option<RowIndex> {
        let index = self.cursor_index()?;
        let (key, kind, open) = self
            .rows
            .with_untracked(|rows| rows.get(index).map(|row| (row.key.clone(), row.kind, rows.is_open(index))))?;
        if kind == NodeKind::Leaf {
            return None;
        }

        if !self.expanded.contains_untracked(&key) {
            self.set_expanded(key, true);
            return None;
        }

        if !open {
            return None;
        }

        let child = RowIndex(index.0 + 1);
        self.land(child, Modifiers::NONE);

        Some(child)
    }

    /// Collapses the cursor's node, or steps to its parent when it is closed. Returns the row
    /// the cursor moved to.
    pub fn close(&self) -> Option<RowIndex> {
        let index = self.cursor_index()?;
        let (key, kind) = self.rows.with_untracked(|rows| rows.get(index).map(|row| (row.key.clone(), row.kind)))?;
        if kind == NodeKind::Branch && self.expanded.contains_untracked(&key) {
            self.set_expanded(key, false);
            return None;
        }

        let parent = self.rows.with_untracked(|rows| rows.parent(index))?;
        self.land(parent, Modifiers::NONE);

        Some(parent)
    }

    pub fn set_expanded(&self, key: M::Key, open: bool) {
        if self.expanded.contains_untracked(&key) == open {
            return;
        }

        self.expanded.update(move |set| {
            if open {
                set.insert(key);
            } else {
                set.remove(&key);
            }
        });
    }

    pub fn toggle_expanded(&self, key: M::Key) {
        let open = !self.expanded.contains_untracked(&key);
        self.set_expanded(key, open);
    }

    /// The first row, wrapping round, whose text begins with `search`: from the row after the
    /// cursor, or with `from_cursor` from the cursor's own.
    pub fn find(&self, search: &str, from_cursor: bool) -> Option<RowIndex> {
        let skip = usize::from(!from_cursor);
        let start = self.cursor_index().map_or(0, |at| at.0 + skip);

        self.rows.with_untracked(|rows| {
            self.model.with_untracked(|model| {
                let count = rows.len();
                (0..count).map(|step| RowIndex((start + step) % count)).find(|&index| {
                    rows.get(index).and_then(|row| model.text(&row.key)).is_some_and(|text| begins_with(text, search))
                })
            })
        })
    }

    /// The ancestors of `key` that are not expanded, nearest first.
    pub fn closed_ancestors(&self, key: &M::Key) -> Vec<M::Key> {
        self.model.with_untracked(|model| {
            let mut closed = Vec::new();
            let mut current = model.parent(key);
            while let Some(ancestor) = current {
                current = model.parent(&ancestor);
                if !self.expanded.contains_untracked(&ancestor) {
                    closed.push(ancestor);
                }
            }

            closed
        })
    }
}

/// Whether `text` begins with `search`, ignoring case and leading space.
fn begins_with(text: &str, search: &str) -> bool {
    let mut text = text.trim_start().chars().flat_map(char::to_lowercase);

    search.chars().flat_map(char::to_lowercase).all(|wanted| text.next() == Some(wanted))
}

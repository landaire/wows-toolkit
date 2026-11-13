//! The palette's dialog: the root list, a mode's list under its header, and the field both are
//! searched with.
//!
//! The field narrows the list itself rather than through the list's own filter, which keeps an
//! option only when its text holds the query: a mode's rows are ranked by how well the name
//! matches, read from the replay index, or found by a path the row does not show in full.

use std::rc::Rc;
use std::time::Duration;

use landaire_ui::LoadingPlacement;
use landaire_ui::LoadingProps;
use landaire_ui::MarkedTextProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::palette::PaletteMode;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Track;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::Task;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::ForProps;
use zgui::view::FrameHandle;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;
use zgui::view::install_stylesheet;
use zgui::vocab::HasPopup;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CommandEmptyProps;
use zgui_ui::CommandItemProps;
use zgui_ui::CommandListProps;
use zgui_ui::CommandProps;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::InputProps;
use zgui_ui::Listbox;
use zgui_ui::ListboxEntry;
use zgui_ui::SpinnerProps;
use zgui_ui::prelude::Binding;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::arrow::ARROW_LEFT;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;
use zgui_ui_icons::set::ui::SEARCH;

use crate::icons::AppIconProps;
use crate::shell::Shell;
use crate::shell::palette::Layer;
use crate::shell::palette::ModeRow;
use crate::shell::palette::ModeRows;
use crate::shell::palette::ModeSource;
use crate::shell::palette::ModeSpec;
use crate::shell::palette::Opening;
use crate::shell::palette::RootRow;
use crate::shell::palette::builtin;
use crate::shell::palette::mode_name;
use crate::shell::zoom_shortcut;
use crate::text_marks::Marks;

const SHEET: &str = "wt-palette";

const CSS: &str = css!(
    ".wt-palette { width: 448px; max-width: calc(100vw - 32px); }
    .wt-palette .zui-command__list { max-height: 400px; }
    .wt-palette__head {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--zui-color-border);
    }
    .wt-palette__crumb { color: var(--zui-color-muted-foreground); }
    .wt-palette__hint {
        margin-left: auto;
        font-size: var(--zui-type-size-xs);
        color: var(--zui-color-muted-foreground);
    }
    .wt-palette__field .zui-spinner {
        flex: none;
        width: 14px;
        height: 14px;
    }
    .wt-palette__row {
        display: flex;
        flex-direction: row;
        flex: 1 1 auto;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .wt-palette__row[data-missing] { opacity: 0.5; }
    .wt-palette__row .wt-icon { color: var(--lui-color-text-dim); }
    .wt-palette__row .wt-palette__warn { color: var(--wt-color-warn); }
    .wt-palette__name { flex: none; }
    .wt-palette__detail {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--zui-type-size-xs);
        color: var(--lui-color-text-dim);
    }"
);

/// One row of a mode's list, with its place on the list.
#[derive(Clone)]
struct ListedRow {
    index: usize,
    row: ModeRow,
}

impl ListedRow {
    /// What the row is told apart by: everything it shows, so a row that changes is rebuilt.
    fn key(&self) -> RowKey {
        RowKey {
            index: self.index,
            label: self.row.label.clone(),
            marks: self.row.marks.clone(),
            detail: self.row.detail.clone(),
            detail_marks: self.row.detail_marks.clone(),
            missing: self.row.missing,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RowKey {
    index: usize,
    label: String,
    marks: Marks,
    detail: Option<String>,
    detail_marks: Marks,
    missing: bool,
}

/// How long the field waits after a keystroke before a mode reads the replay index.
const QUERY_PAUSE: Duration = Duration::from_millis(100);

/// The palette's dialog, and what the shell offers on it.
#[component]
pub fn CommandPalette(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    builtin::offer(shell);
    let palette = shell.palette;
    let locale = shell.services().locale;

    let open = Binding::controlled(Signal::derive_local(move || palette.is_open()), move |open: bool| {
        if !open {
            palette.close();
        }
    });
    // What is typed in the list on screen, emptied for each list opened.
    let query = RwSignal::new_local(String::new());
    // Escape empties a typed query first, and a mode answers it itself by going back to the root
    // list.
    let escape_closes =
        Signal::derive_local(move || palette.layer() == Some(Layer::Root) && query.with(String::is_empty));

    // The last list opened, kept while the dialog plays its exit so the list does not vanish first.
    let shown = RwSignal::new_local(None::<Opening>);
    let following = RenderEffect::new(move |_| {
        let opening = palette.opening.get();
        if opening.is_some() && opening != shown.get_untracked() {
            query.set(String::new());
            shown.set(opening);
        }
    });
    on_cleanup_local(move || drop(following));

    view! {
        Dialog(open = open) {
            DialogContent(
                dismiss_on_escape = escape_closes,
                dismiss_control = false,
                class = "zui-command__dialog wt-palette",
                attr:data-testid = "palette",
                on:key_down = move |ev| {
                    let modifiers = ev.modifiers;
                    if zoom_shortcut(shell, ev, modifiers) {
                        ev.prevent_default();
                        ev.stop_propagation();
                    }
                }
            ) {
                DialogTitle {{locale.text("ui.palette.title")}}
                {move || match shown.get() {
                    Some(Opening { layer: Layer::Root, .. }) => {
                        AnyView::new(view! { RootList(shell = shell, query = query) })
                    }
                    Some(Opening { layer: Layer::Mode(mode), offer, .. }) => match offer.and_then(|offer| palette.offered_spec(offer)) {
                        Some(spec) => {
                            AnyView::new(view! { ModeList(shell = shell, mode = mode, spec = spec, query = query) })
                        }
                        None => AnyView::new(view! { RootList(shell = shell, query = query) }),
                    },
                    None => AnyView::new(()),
                }}
            }
        }
    }
}

/// The root list: every command offered, narrowed to the labels that hold the query.
#[component]
fn RootList(shell: Shell, query: RwSignal<String, LocalStorage>) -> impl IntoView {
    let palette = shell.palette;
    let locale = shell.services().locale;
    let rows = Signal::derive_local(move || {
        locale.track();
        palette.root_rows(&query.get(), shell.debug_mode.get())
    });

    view! {
        Command(attr:data-testid = "palette-root") {
            PaletteField(query = query, placeholder = t!("ui.palette.placeholder").into_owned())
            HighlightFirst(rows = Signal::derive_local(move || rows.track()))
            CommandList(label = t!("ui.palette.title").into_owned()) {
                for row in move || rows.get(), key = |row: &RootRow| row.clone() {
                    CommandItem(
                        value = row.item.value(),
                        text = row.label.clone(),
                        on_select = UnsyncCallback::new(move |()| palette.choose_root(shell, row.item)),
                        a11y:label = row.label.clone(),
                        attr:data-testid = format!("palette-item-{}", row.item.value())
                    ) {
                        MarkedText(text = row.label.clone(), ranges = row.marks.clone())
                    }
                }
                CommandEmpty {{locale.text("ui.search.no_matches")}}
            }
        }
    }
}

/// One mode's list, under a header leading back to the root list.
#[component]
fn ModeList(shell: Shell, mode: PaletteMode, spec: ModeSpec, query: RwSignal<String, LocalStorage>) -> impl IntoView {
    let palette = shell.palette;
    let rows = RwSignal::new_local(Rc::new(ModeRows::Ready(Vec::new())));
    let searching = RwSignal::new_local(false);
    match spec.source {
        ModeSource::Local(filter) => follow_filter(filter, query, rows),
        ModeSource::Query(fetch) => follow_query(fetch, query, rows, searching),
    }

    let listing = Signal::derive_local(move || {
        rows.with(|rows| match &**rows {
            ModeRows::Ready(listed) => {
                listed.iter().cloned().enumerate().map(|(index, row)| ListedRow { index, row }).collect()
            }
            ModeRows::Loading(_) | ModeRows::Unavailable(_) => Vec::new(),
        })
    });
    let back = UnsyncCallback::new(move |()| palette.open_root());

    view! {
        Command(attr:data-testid = format!("palette-mode-{}", mode_name(mode))) {
            ModeHeader(mode = mode, back = back)
            PaletteField(
                query = query,
                placeholder = t!(mode.placeholder_key()).into_owned(),
                searching = searching,
                on_back = back
            )
            HighlightFirst(rows = Signal::derive_local(move || rows.track()))
            CommandList(label = t!(mode.title_key()).into_owned()) {
                for listed in move || listing.get(), key = |listed: &ListedRow| listed.key() {
                    CommandItem(
                        value = format!("row-{}", listed.index),
                        text = listed.row.label.clone(),
                        on_select = {
                            let run = listed.row.run.clone();
                            UnsyncCallback::new(move |()| palette.run(shell, run.clone()))
                        },
                        a11y:label = listed.row.label.clone(),
                        attr:data-testid = format!("palette-row-{}", listed.index)
                    ) {
                        ModeRowView(row = listed.row.clone())
                    }
                }
                CommandEmpty {{move || empty_message(&rows.get(), searching.get())}}
            }
        }
    }
}

/// Puts the highlight on the first row whenever `rows` changes, as the list's own filter does,
/// so Enter runs the best match.
#[component]
fn HighlightFirst(rows: Signal<(), LocalStorage>) -> impl IntoView {
    let Some(listbox) = Listbox::current() else { return };
    let timers = Timers::current();
    let pending = StoredValue::new_local(None::<FrameHandle>);
    let first = move || listbox.set_active(listbox.choosable().first().map(ListboxEntry::id));

    let following = RenderEffect::new(move |_| {
        rows.track();
        match &timers {
            // A row can be walked to once its element is in the tree, which is a frame later.
            Some(timers) => pending.set_value(Some(timers.request_frame(move |_| first()))),
            None => first(),
        }
    });
    on_cleanup_local(move || {
        drop(following);
        pending.set_value(None);
    });
}

/// What the list says while it has no rows: nothing while a search is out, else why.
fn empty_message(rows: &ModeRows, searching: bool) -> AnyView {
    if searching {
        return AnyView::new(());
    }

    match rows {
        ModeRows::Loading(message) => {
            let message = message.clone();
            AnyView::new(view! {
                Loading(label = message, placement = LoadingPlacement::Top)
            })
        }
        ModeRows::Unavailable(reason) => AnyView::new(view! { text {{reason.clone()}} }),
        ModeRows::Ready(_) => AnyView::new(view! { text {{t!("ui.search.no_matches").into_owned()}} }),
    }
}

/// Refilters the rows in memory on every change of the query, or of anything the filter reads.
fn follow_filter(
    filter: Rc<dyn Fn(&str) -> ModeRows>,
    query: RwSignal<String, LocalStorage>,
    rows: RwSignal<Rc<ModeRows>, LocalStorage>,
) {
    let filtering = RenderEffect::new(move |_| {
        let found = query.with(|query| filter(query));
        rows.set(Rc::new(found));
    });
    on_cleanup_local(move || drop(filtering));
}

/// Reads the rows once the reader pauses typing, dropping an answer to a query since replaced.
fn follow_query(
    fetch: Rc<dyn Fn(String) -> crate::shell::palette::ModeQuery>,
    query: RwSignal<String, LocalStorage>,
    rows: RwSignal<Rc<ModeRows>, LocalStorage>,
    searching: RwSignal<bool, LocalStorage>,
) {
    let timers = Timers::current();
    let due = RwSignal::new_local(None::<(u64, String)>);
    let pause = StoredValue::new_local(None::<TimeoutHandle>);
    let reading = StoredValue::new_local(None::<Task>);
    let asked = StoredValue::new(0_u64);

    // The generation advances as the query changes, not when the pause ends, so an answer to
    // the previous query arriving during the pause is dropped.
    let pausing = RenderEffect::new(move |_| {
        let needle = query.get();
        asked.update_value(|asked| *asked += 1);
        let this = asked.get_value();
        if let Some(Some(earlier)) = reading.try_update_value(Option::take) {
            earlier.cancel();
        }
        searching.set(true);
        rows.set(Rc::new(ModeRows::Ready(Vec::new())));
        match &timers {
            Some(timers) => {
                pause.set_value(Some(timers.set_timeout(QUERY_PAUSE, move || due.set(Some((this, needle))))));
            }
            None => due.set(Some((this, needle))),
        }
    });

    let fetching = RenderEffect::new(move |_| {
        let Some((this, needle)) = due.get() else { return };
        if asked.get_value() != this {
            return;
        }

        let read = fetch(needle);
        let task = spawn_local(async move {
            let found = read.await;
            if asked.get_value() != this {
                return;
            }
            rows.set(Rc::new(found));
            searching.set(false);
        });
        reading.set_value(Some(task));
    });

    on_cleanup_local(move || {
        drop(pausing);
        drop(fetching);
        pause.set_value(None);
        if let Some(Some(earlier)) = reading.try_update_value(Option::take) {
            earlier.cancel();
        }
    });
}

/// The way back to the root list, and the name of the list below it.
#[component]
fn ModeHeader(mode: PaletteMode, back: UnsyncCallback<()>) -> impl IntoView {
    view! {
        row(class = "wt-palette__head") {
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::Xs,
                on:click = move |_| back.run(()),
                attr:data-testid = "palette-back"
            ) {
                Icon(icon = ARROW_LEFT)
                text {{t!("ui.palette.title").into_owned()}}
            }
            box(class = "wt-palette__crumb") {
                Icon(icon = CHEVRON_RIGHT)
            }
            text(attr:data-testid = "palette-mode-title") {{t!(mode.title_key()).into_owned()}}
            text(class = "wt-palette__hint") {{t!("ui.palette.back_hint").into_owned()}}
        }
    }
}

/// One row of a mode: its icon, its label, and the detail dimmed after it.
#[component]
fn ModeRowView(row: ModeRow) -> impl IntoView {
    let ModeRow { label, marks, detail, detail_marks, glyph, missing, .. } = row;
    let name = AnyView::new(view! { MarkedText(class = "wt-palette__name", text = label, ranges = marks) });

    view! {
        row(class = "wt-palette__row", attr:data-missing = missing.then(|| "true".to_owned())) {
            {glyph.map_or_else(|| AnyView::new(()), |glyph| AnyView::new(view! { AppIcon(glyph = glyph) }))}
            {name}
            {if missing {
                AnyView::new(view! {
                    AppIcon(
                        glyph = Glyph::TriangleAlert,
                        class = "wt-palette__warn",
                        attr:data-testid = "palette-folder-missing"
                    )
                })
            } else {
                AnyView::new(())
            }}
            {detail.map_or_else(
                || AnyView::new(()),
                |detail| AnyView::new(view! { MarkedText(class = "wt-palette__detail", text = detail, ranges = detail_marks) }),
            )}
        }
    }
}

/// The field a list is searched with. It walks and chooses the list's rows as the list's own
/// field does. Escape empties a typed query, and in an empty field Escape or Backspace asks to go
/// back.
#[component]
fn PaletteField(
    query: RwSignal<String, LocalStorage>,
    placeholder: String,
    /// Shows a spinner while a mode's search is out.
    #[prop(into, default = Signal::stored_local(false))]
    searching: Signal<bool, LocalStorage>,
    /// `None` on the root list, where Escape closes the palette.
    #[prop(optional)]
    on_back: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    let listbox = Listbox::current();
    let input = NodeRef::new();
    focus_when_bound(input);

    let mut semantics = A11yBinding::new(Role::ComboBox)
        .has_popup(HasPopup::Listbox)
        .expanded(|| true)
        .label(t!("ui.palette.title").into_owned())
        .active_descendant(move || {
            listbox.map_or(zgui::vocab::NodeId(0), |listbox| {
                zgui::vocab::NodeId(listbox.active_node().get().map_or(0, zgui::view::NodeId::as_u64))
            })
        });
    if let Some(listbox) = listbox {
        semantics = semantics.controls(listbox.surface().content());
    }
    let own = Attrs::new().a11y_from(semantics);

    view! {
        row(
            class = "zui-command__field wt-palette__field",
            on:key_down:capture = move |ev| {
                let escape = ev.key == Key::Named(NamedKey::Escape);
                let backspace = ev.key == Key::Named(NamedKey::Backspace) && ev.modifiers == Modifiers::NONE;
                let typed = !query.with_untracked(String::is_empty);
                if escape && typed {
                    query.set(String::new());
                    ev.prevent_default();
                    ev.stop_propagation();
                    return;
                }
                if (escape || backspace) && !typed && let Some(on_back) = on_back {
                    on_back.run(());
                    ev.prevent_default();
                    ev.stop_propagation();
                    return;
                }

                if listbox.is_some_and(|listbox| listbox.handle(&ev.key)) {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            }
        ) {
            Icon(icon = SEARCH)
            Input(
                node_ref = input,
                value = query,
                placeholder = placeholder,
                class = "zui-command__input",
                attr:data-testid = "palette-field",
                {..own}
            )
            Show(when = move || searching.get(), fallback = || ()) {
                Spinner()
            }
        }
    }
}

/// Puts the caret in `input` once it is bound: a reader who opened a list is about to type.
fn focus_when_bound(input: NodeRef) {
    let focusing = RenderEffect::new(move |focused: Option<bool>| {
        if focused == Some(true) {
            return true;
        }
        let bound = input.get().is_some();
        if bound {
            input.focus();
        }

        bound
    });
    on_cleanup_local(move || drop(focusing));
}

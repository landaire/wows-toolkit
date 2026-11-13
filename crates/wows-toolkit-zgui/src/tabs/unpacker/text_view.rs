//! The viewer's text: highlighted lines in a virtualised list, with character selection by
//! dragging, ctrl+C and ctrl+A, and a find bar on ctrl+F.

use std::collections::HashMap;
use std::rc::Rc;

use landaire_ui::TABULAR_CLASS;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::prelude::Fonts;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::use_context;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::PointerButton;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::VirtualListProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::chevron::CHEVRON_DOWN;
use zgui_ui_icons::set::chevron::CHEVRON_UP;

use crate::icons::AppIconProps;
use crate::shell::Shell;
use crate::tabs::unpacker::text_layout::LineShaper;
use crate::tabs::unpacker::text_layout::TAB_SIZE;
use crate::tabs::unpacker::text_layout::code_font_family;
use crate::tabs::unpacker::text_layout::column_at;
use crate::tabs::unpacker::text_layout::guide_columns;
use crate::tabs::unpacker::text_layout::indents;
use crate::tabs::unpacker::text_select::LineMarks;
use crate::tabs::unpacker::text_select::Marks;
use crate::tabs::unpacker::text_select::Match;
use crate::tabs::unpacker::text_select::Selection;
use crate::tabs::unpacker::text_select::TextPos;
use crate::tabs::unpacker::text_select::find_all;
use crate::tabs::unpacker::text_select::selected_text;
use crate::tabs::unpacker::text_select::spans;
use crate::tabs::unpacker::viewer::CodeLine;
use crate::tabs::unpacker::viewer::TokenClass;

const SHEET: &str = "wt-unpacker-text";

const CSS: &str = css!(
    ".wt-text { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; position: relative; }
    .wt-text:focus-visible { outline: none; }
    .wt-text__code {
        flex: 1 1 auto;
        min-height: 0;
        overflow-x: auto;
        font-size: 12px;
        cursor: text;
        user-select: none;
    }
    .wt-text__find {
        flex: none;
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-text__find-field { width: 240px; }
    .wt-text__find-count {
        min-width: 72px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
    }
    .wt-code-line { display: flex; height: 18px; align-items: center; min-width: max-content; }
    .wt-code-line__body { position: relative; flex: none; display: flex; align-items: center; height: 18px; }
    .wt-code-line__guide {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 1px;
        background-color: color-mix(in srgb, var(--lui-color-border) 85%, transparent);
    }
    .wt-code-line__number {
        flex: none;
        box-sizing: border-box;
        width: 48px;
        padding-right: 12px;
        text-align: right;
        color: var(--wt-color-text-faint);
    }
    .wt-code-line__text { white-space: pre; }
    .wt-code-sel { background-color: var(--lui-color-selection); }
    .wt-code-match { background-color: color-mix(in srgb, var(--wt-color-warn) 30%, transparent); }
    .wt-code-current { background-color: color-mix(in srgb, var(--wt-color-warn) 70%, transparent); }"
);

/// One line of code, and the height every line is drawn at.
pub const LINE_HEIGHT: f32 = 18.0;

/// Where the text starts: the line-number column's border-box width.
const NUMBER_WIDTH: f32 = 48.0;

pub type Lines = RwSignal<Rc<Vec<CodeLine>>, LocalStorage>;

/// Where the viewer's lines are and where their characters sit.
#[derive(Clone, Copy)]
struct Geometry {
    viewport: NodeRef,
    shaper: StoredValue<LineShaper, LocalStorage>,
    /// Each shaped line's character boundaries, by line, until the text changes.
    stops: StoredValue<HashMap<usize, Rc<[f32]>>, LocalStorage>,
}

impl Geometry {
    /// The place in `lines` nearest `position`, which is in window CSS pixels.
    fn text_pos(self, lines: &[CodeLine], position: Point<CssPx, Css>) -> Option<TextPos> {
        let last = lines.len().checked_sub(1)?;
        let rect = self.viewport.window_bounds()?;
        let scale = self.viewport.scale();
        let scroll = self.viewport.scroll_offset();
        let x = (position.x.0 * scale - rect.origin.x.0 + scroll.x.0) / scale - NUMBER_WIDTH;
        let y = (position.y.0 * scale - rect.origin.y.0 + scroll.y.0) / scale;
        let line = ((y / LINE_HEIGHT).floor().max(0.0) as usize).min(last);
        let stops = self.line_stops(lines, line)?;

        Some(TextPos { line, column: column_at(&stops, x) })
    }

    /// `line`'s character boundaries, shaped the first time they are asked for. `None` once the
    /// view is disposed.
    fn line_stops(self, lines: &[CodeLine], line: usize) -> Option<Rc<[f32]>> {
        if let Some(stops) = self.stops.with_value(|stops| stops.get(&line).cloned()) {
            return Some(stops);
        }

        let stops: Rc<[f32]> = self.shaper.try_update_value(|shaper| shaper.stops(&lines[line]))?.into();
        self.stops.update_value(|cache| {
            cache.insert(line, Rc::clone(&stops));
        });

        Some(stops)
    }

    /// Scrolls `line` to the middle of the port, unless it is already in view.
    fn reveal(self, line: usize) {
        let Some(rect) = self.viewport.window_bounds() else { return };
        let scale = self.viewport.scale();
        let scroll = self.viewport.scroll_offset();
        let row = LINE_HEIGHT * scale;
        let top = line as f32 * row;
        let height = rect.size.height.0;
        if top >= scroll.y.0 && top + row <= scroll.y.0 + height {
            return;
        }
        let down = (top - (height - row) / 2.0).max(0.0);
        self.viewport.scroll_to(ScrollTarget::Offset(Point::new(scroll.x, DevicePx(down))), ScrollBehavior::Instant);
    }
}

/// The find bar's state.
#[derive(Clone, Copy)]
struct Find {
    open: RwSignal<bool, LocalStorage>,
    query: RwSignal<String, LocalStorage>,
    matches: RwSignal<Rc<Vec<Match>>, LocalStorage>,
    current: RwSignal<Option<usize>, LocalStorage>,
    field: NodeRef,
}

impl Find {
    /// Moves to the next match, or the previous one, wrapping at either end.
    fn step(self, forward: bool, geometry: Geometry) {
        let total = self.matches.with_untracked(|matches| matches.len());
        if total == 0 {
            return;
        }
        let next = match self.current.get_untracked() {
            None if forward => 0,
            None => total - 1,
            Some(at) if forward => (at + 1) % total,
            Some(at) => (at + total - 1) % total,
        };
        self.current.set(Some(next));
        if let Some(line) = self.matches.with_untracked(|matches| matches.get(next).map(|found| found.line)) {
            geometry.reveal(line);
        }
    }

    /// Opens the bar, or goes back to it, with its query selected for typing over.
    fn show(self) {
        self.open.set(true);
        self.focus_field();
    }

    fn focus_field(self) {
        self.field.focus();
        self.field.select_all();
    }

    fn close(self, root: NodeRef) {
        self.open.set(false);
        root.focus();
    }
}

#[component]
pub fn TextView(lines: Lines, title: String) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shell = Shell::current();
    let root = NodeRef::new();
    let fonts = use_context::<Fonts>().expect("every window provides its fonts");
    let mut shaper = LineShaper::new(fonts.shaper());
    let space_width = shaper.space_width();
    let geometry = Geometry {
        viewport: NodeRef::new(),
        shaper: StoredValue::new_local(shaper),
        stops: StoredValue::new_local(HashMap::new()),
    };
    let selection = RwSignal::new_local(None::<Selection>);
    let dragging = StoredValue::new_local(false);

    // New text is shaped again when it is clicked, and its indent recounted.
    let line_indents = RwSignal::new_local(Rc::new(Vec::new()));
    let relaying = RenderEffect::new(move |_| {
        geometry.stops.update_value(HashMap::clear);
        line_indents.set(Rc::new(lines.with(|lines| indents(lines))));
    });
    on_cleanup_local(move || drop(relaying));

    let find = Find {
        open: RwSignal::new_local(false),
        query: RwSignal::new_local(String::new()),
        matches: RwSignal::new_local(Rc::new(Vec::new())),
        current: RwSignal::new_local(None),
        field: NodeRef::new(),
    };
    // A new query, or text that changed under it, starts at its first match.
    let finding = RenderEffect::new(move |_| {
        let found = lines.with(|lines| find.query.with(|query| find_all(lines, query)));
        let first = found.first().map(|found| found.line);
        find.matches.set(Rc::new(found));
        find.current.set(first.map(|_| 0));
        if let Some(line) = first {
            geometry.reveal(line);
        }
    });
    on_cleanup_local(move || drop(finding));
    // The field is mounted after the key that opens the bar, so it takes focus once it exists.
    let focusing = RenderEffect::new(move |_| {
        if find.field.get().is_some() {
            find.focus_field();
        }
    });
    on_cleanup_local(move || drop(focusing));

    let on_key = move |ev: &mut EventCx<'_, events::KeyDown>| {
        let named = |key: NamedKey| ev.key == Key::Named(key);
        let handled = match (&ev.key_without_modifiers, ev.modifiers.primary(), ev.modifiers.shift()) {
            (Key::Character(text), true, false) if text.eq_ignore_ascii_case("c") => {
                if let Some(chosen) = selection.get_untracked().filter(|chosen| !chosen.is_empty()) {
                    shell.copy_text(lines.with_untracked(|lines| selected_text(lines, chosen)));
                }
                true
            }
            (Key::Character(text), true, false) if text.eq_ignore_ascii_case("a") => {
                selection.set(lines.with_untracked(|lines| Selection::all(lines)));
                true
            }
            (Key::Character(text), true, false) if text.eq_ignore_ascii_case("f") => {
                find.show();
                true
            }
            (_, false, shift) if named(NamedKey::F3) => {
                find.step(!shift, geometry);
                true
            }
            (_, false, false) if named(NamedKey::Escape) && find.open.get_untracked() => {
                find.close(root);
                true
            }
            (_, false, false) if named(NamedKey::Escape) && selection.get_untracked().is_some() => {
                selection.set(None);
                true
            }
            _ => false,
        };
        if handled {
            ev.prevent_default();
            ev.stop_propagation();
        }
    };

    // The field's own keys: Enter steps through the matches, and copy and select-all stay with
    // the field rather than reaching the text below it.
    let on_field_key = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if ev.key == Key::Named(NamedKey::Enter) {
            find.step(!ev.modifiers.shift(), geometry);
            ev.prevent_default();
            ev.stop_propagation();
        } else if ev.modifiers.primary()
            && matches!(&ev.key_without_modifiers, Key::Character(text) if text.eq_ignore_ascii_case("c") || text.eq_ignore_ascii_case("a"))
        {
            ev.stop_propagation();
        }
    };

    let count = move || {
        let total = find.matches.with(|matches| matches.len());
        if find.query.with(String::is_empty) {
            String::new()
        } else if total == 0 {
            t!("ui.unpacker.find_none").into_owned()
        } else {
            let current = find.current.get().map_or(0, |at| at + 1);
            t!("ui.unpacker.find_count", current = current, total = total).into_owned()
        }
    };

    let at_pointer = move |position| lines.with_untracked(|lines| geometry.text_pos(lines, position));

    view! {
        column(class = "wt-text", node_ref = root, tabindex = {Focus::Programmatic}, on:key_down = on_key) {
            Show(when = move || find.open.get()) {
                row(class = "wt-text__find", attr:data-testid = "unpacker-find") {
                    Input(
                        value = find.query,
                        node_ref = find.field,
                        placeholder = t!("ui.unpacker.find").into_owned(),
                        label = t!("ui.unpacker.find").into_owned(),
                        class = "wt-text__find-field",
                        attr:data-testid = "unpacker-find-field",
                        on:key_down = on_field_key
                    )
                    text(class = format!("wt-text__find-count {TABULAR_CLASS}"), attr:data-testid = "unpacker-find-count") {{count}}
                    Button(
                        size = ButtonSize::IconSm,
                        variant = ButtonVariant::Ghost,
                        a11y:label = t!("ui.unpacker.find_previous").into_owned(),
                        attr:data-testid = "unpacker-find-previous",
                        on:click = move |_| find.step(false, geometry)
                    ) {
                        Icon(icon = CHEVRON_UP)
                    }
                    Button(
                        size = ButtonSize::IconSm,
                        variant = ButtonVariant::Ghost,
                        a11y:label = t!("ui.unpacker.find_next").into_owned(),
                        attr:data-testid = "unpacker-find-next",
                        on:click = move |_| find.step(true, geometry)
                    ) {
                        Icon(icon = CHEVRON_DOWN)
                    }
                    Button(
                        size = ButtonSize::IconSm,
                        variant = ButtonVariant::Ghost,
                        a11y:label = t!("ui.unpacker.find_close").into_owned(),
                        attr:data-testid = "unpacker-find-close",
                        on:click = move |_| find.close(root)
                    ) {
                        AppIcon(glyph = Glyph::X)
                    }
                }
            }
            VirtualList(
                count = Signal::derive_local(move || lines.with(|lines| lines.len())),
                row_size = LINE_HEIGHT,
                label = title,
                node_ref = geometry.viewport,
                class = "wt-text__code",
                style:font-family = code_font_family(),
                style:tab-size = TAB_SIZE.to_string(),
                attr:data-testid = "unpacker-viewer-text",
                on:pointer_down = move |ev| {
                    if ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    root.focus();
                    let Some(at) = at_pointer(ev.position) else { return };
                    let anchor = match selection.get_untracked() {
                        Some(chosen) if ev.modifiers.shift() => chosen.anchor,
                        _ => at,
                    };
                    selection.set(Some(Selection { anchor, head: at }));
                    dragging.set_value(true);
                    ev.capture_pointer();
                },
                on:pointer_move = move |ev| {
                    if !dragging.get_value() {
                        return;
                    }
                    let Some(at) = at_pointer(ev.position) else { return };
                    // Only a move to another character redraws the lines.
                    if let Some(chosen) = selection.get_untracked().filter(|chosen| chosen.head != at) {
                        selection.set(Some(Selection { head: at, ..chosen }));
                    }
                },
                on:pointer_up = move |ev| {
                    if dragging.get_value() {
                        dragging.set_value(false);
                        ev.release_pointer();
                    }
                },
                on:pointer_cancel = move |ev| {
                    if dragging.get_value() {
                        dragging.set_value(false);
                        ev.release_pointer();
                    }
                },
                row = move |ix: usize| code_line(lines, ix, selection, find, Guides { indents: line_indents, space_width })
            )
        }
    }
}

/// Where a line's indent guides go: each line's indent in columns, and one column's width.
#[derive(Clone, Copy)]
struct Guides {
    indents: RwSignal<Rc<Vec<usize>>, LocalStorage>,
    space_width: f32,
}

fn code_line(
    lines: Lines,
    ix: usize,
    selection: RwSignal<Option<Selection>, LocalStorage>,
    find: Find,
    guides: Guides,
) -> impl IntoView {
    // Read through the signals, so a line drawn plain is redrawn once the highlighting lands, and
    // a line is redrawn as the selection or the matches over it change.
    let pieces = move || {
        let chosen = selection.get();
        let current_at = find.current.get();
        find.matches
            .with(|matches| {
                let marks = LineMarks { line: ix, selection: chosen, matches: matches.as_slice(), current: current_at };
                lines.with(|lines| lines.get(ix).map(|line| spans(line, &marks)).unwrap_or_default())
            })
            .into_iter()
            .map(|span| {
                let class = span.class.map_or("", TokenClass::class);
                let Marks { selected, matched, current } = span.marks;
                AnyView::new(view! {
                    text(
                        class = class,
                        class:wt-code-sel = selected,
                        class:wt-code-match = matched,
                        class:wt-code-current = current
                    ) {{span.text}}
                })
            })
            .collect::<Vec<AnyView>>()
    };

    let guide_views = move || {
        let indent = guides.indents.with(|indents| indents.get(ix).copied().unwrap_or(0));
        guide_columns(indent)
            .map(|column| {
                let left = format!("{}px", column as f32 * guides.space_width);
                AnyView::new(view! { box(class = "wt-code-line__guide", style:left = left) })
            })
            .collect::<Vec<AnyView>>()
    };

    view! {
        row(class = "wt-code-line") {
            text(class = format!("wt-code-line__number {TABULAR_CLASS}")) {{(ix + 1).to_string()}}
            row(class = "wt-code-line__body") {
                {guide_views}
                text(class = "wt-code-line__text") {{pieces}}
            }
        }
    }
}

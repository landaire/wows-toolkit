//! A debug-mode JSON payload in the side panel: the replay's metadata, its battle results, or one
//! player's metadata.
//!
//! The payload is highlighted as JSON and its text can be selected. The search field finds each
//! place its text occurs and scrolls to them in turn. Only the lines in view are built, since a
//! battle's results run to thousands of lines.

use std::ops::Range;
use std::rc::Rc;

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::geom::DevicePx;
use zgui::geom::Point;
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
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::ScrollAreaContext;
use zgui_ui::ScrollAreaProps;
use zgui_ui::virtualize::Virtualize;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::chevron::CHEVRON_DOWN;
use zgui_ui_icons::set::chevron::CHEVRON_UP;
use zgui_ui_primitives::Orientation;

use crate::icons::AppIconProps;
use crate::shell::Shell;

const SHEET: &str = "wt-raw-json";

/// The payload is read rather than edited, so it is drawn at the app's small size, as the other
/// front end draws it.
const CSS: &str = css!(
    ".wt-rp-json { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-rp-json__toolbar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-rp-json__body { flex: 1 1 auto; min-height: 0; }
    .wt-rp-json__search { display: flex; flex-direction: row; align-items: center; gap: 4px; flex: 1 1 auto; min-width: 0; }
    .wt-rp-json__field { flex: 1 1 auto; min-width: 60px; }
    .wt-rp-json__count { flex: none; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-rp-json__lines { display: flex; flex-direction: column; padding: 0 8px; user-select: text; }
    .wt-rp-json__line {
        flex: none;
        height: 16px;
        line-height: 16px;
        font-family: monospace;
        font-size: 12px;
        white-space: pre;
    }
    .wt-json-key { color: var(--wt-color-syntax-key); }
    .wt-json-string { color: var(--wt-color-syntax-string); }
    .wt-json-number { color: var(--wt-color-syntax-number); }
    .wt-json-literal { color: var(--wt-color-syntax-literal); }
    .wt-json-punctuation { color: var(--wt-color-text-dim); }
    .wt-json-found { background-color: color-mix(in srgb, var(--lui-color-accent) 25%, transparent); }
    .wt-json-current { background-color: color-mix(in srgb, var(--lui-color-accent) 60%, transparent); }"
);

/// Each line's height, in CSS pixels, which the stylesheet sets too.
const LINE_HEIGHT: f32 = 16.0;

/// What a run of a JSON line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    Key,
    String,
    Number,
    /// `true`, `false` or `null`.
    Literal,
    Punctuation,
    Space,
}

impl TokenKind {
    const fn class(self) -> Option<&'static str> {
        match self {
            Self::Key => Some("wt-json-key"),
            Self::String => Some("wt-json-string"),
            Self::Number => Some("wt-json-number"),
            Self::Literal => Some("wt-json-literal"),
            Self::Punctuation => Some("wt-json-punctuation"),
            Self::Space => None,
        }
    }
}

/// A line of the payload, counted from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LineIndex(usize);

/// One place the search text occurs: its line and the bytes of that line it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    line: LineIndex,
    range: Range<usize>,
}

/// Which of the found places is the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FoundIndex(usize);

/// How a piece of a line is marked by the search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    None,
    Found,
    Current,
}

/// A run of one line drawn in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    range: Range<usize>,
    kind: TokenKind,
    mark: Mark,
}

/// Which way Enter steps through the found places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Next,
    Previous,
}

/// The payload and where each of its lines lies in it.
struct Payload {
    content: Rc<str>,
    lines: Rc<[Range<usize>]>,
}

impl Payload {
    fn new(content: Rc<str>) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        for line in content.split_inclusive('\n') {
            let text = line.trim_end_matches('\n').trim_end_matches('\r');
            lines.push(start..start + text.len());
            start += line.len();
        }

        Self { content, lines: Rc::from(lines) }
    }

    fn line(&self, index: LineIndex) -> &str {
        self.lines.get(index.0).map_or("", |range| &self.content[range.clone()])
    }
}

/// One payload with its search field and copy button.
#[component]
pub fn RawJsonPanel(content: Rc<str>) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shell = Shell::current();
    let payload = Rc::new(Payload::new(Rc::clone(&content)));
    let query = RwSignal::new_local(String::new());
    let found = RwSignal::new_local(Rc::<[Found]>::from([]));
    let current = RwSignal::new_local(None::<FoundIndex>);

    // A new search starts at its first place.
    let searched = Rc::clone(&payload);
    let searching = RenderEffect::new(move |_| {
        let places = query.with(|query| find_all(&searched, query));
        current.set((!places.is_empty()).then_some(FoundIndex(0)));
        found.set(Rc::from(places));
    });
    on_cleanup_local(move || drop(searching));

    let step = move |direction: Step| {
        let count = found.with_untracked(|found| found.len());
        current.set(stepped(current.get_untracked(), count, direction));
    };
    let on_key = move |ev: &mut EventCx<'_, events::KeyDown>| match ev.key {
        Key::Named(NamedKey::Enter) => {
            let direction = if ev.modifiers.shift() { Step::Previous } else { Step::Next };
            step(direction);
            ev.prevent_default();
        }
        Key::Named(NamedKey::Escape) if query.with_untracked(|query| !query.is_empty()) => {
            query.set(String::new());
            ev.prevent_default();
            ev.stop_propagation();
        }
        _ => {}
    };
    let count = move || {
        let total = found.with(|found| found.len());
        let shown = current.get().map_or(0, |index| index.0 + 1);
        query.with(|query| !query.is_empty()).then(|| format!("{shown}/{total}"))
    };
    let nothing_found = Signal::derive_local(move || found.with(|found| found.is_empty()));
    let search_label = t!("ui.replay.debug.search").into_owned();
    let previous_label = t!("ui.replay.debug.previous_match").into_owned();
    let previous_tip = previous_label.clone();
    let next_label = t!("ui.replay.debug.next_match").into_owned();
    let next_tip = next_label.clone();

    view! {
        column(class = "wt-rp-json", attr:data-testid = "replay-raw-json") {
            row(class = "wt-rp-json__toolbar") {
                row(class = "wt-rp-json__search") {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::Search)
                    Input(
                        value = query,
                        placeholder = search_label.clone(),
                        label = search_label,
                        class = "wt-rp-json__field",
                        attr:data-testid = "raw-json-search",
                        on:key_down = on_key
                    )
                    text(class = "wt-rp-json__count", attr:data-testid = "raw-json-count") {{count}}
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::IconSm,
                                disabled = nothing_found,
                                a11y:label = previous_label.clone(),
                                attr:data-testid = "raw-json-previous",
                                on:click = move |_| step(Step::Previous)
                            ) {
                                Icon(icon = CHEVRON_UP)
                            }
                        }
                        TooltipContent {{previous_tip.clone()}}
                    }
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::IconSm,
                                disabled = nothing_found,
                                a11y:label = next_label.clone(),
                                attr:data-testid = "raw-json-next",
                                on:click = move |_| step(Step::Next)
                            ) {
                                Icon(icon = CHEVRON_DOWN)
                            }
                        }
                        TooltipContent {{next_tip.clone()}}
                    }
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "raw-json-copy",
                    on:click = move |_| shell.copy_text(content.to_string())
                ) {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::Copy)
                    text {{t!("ui.buttons.copy").into_owned()}}
                }
            }
            ScrollArea(class = "wt-rp-json__body", orientation = Orientation::Both) {
                JsonLines(payload = payload, found = found, current = current)
            }
        }
    }
}

/// The lines in view, inside the scroll area, which also scroll to the current found place.
#[component]
fn JsonLines(
    payload: Rc<Payload>,
    found: RwSignal<Rc<[Found]>, LocalStorage>,
    current: RwSignal<Option<FoundIndex>, LocalStorage>,
) -> impl IntoView {
    let port = ScrollAreaContext::current().expect("the lines are inside their scroll area").viewport();
    let line_count = payload.lines.len();
    let seen = Virtualize::new(port, Signal::stored_local(line_count), LINE_HEIGHT, 8);
    let current_piece = NodeRef::new();
    // Set by a step through the found places and taken once the current piece is built, which
    // brings it into view across too. A piece built again by a scroll is left where it is.
    let revealing = StoredValue::new_local(false);

    let centring = RenderEffect::new(move |_| {
        let Some(index) = current.get() else { return };
        let Some(line) = found.with(|found| found.get(index.0).map(|found| found.line)) else { return };
        centre_line(port, line_count, line);
        revealing.set_value(true);
    });
    let revealing_effect = RenderEffect::new(move |_| {
        if current_piece.get().is_some() && revealing.get_value() {
            revealing.set_value(false);
            current_piece.scroll_to(ScrollTarget::IntoView, ScrollBehavior::Instant);
        }
    });
    on_cleanup_local(move || {
        drop(centring);
        drop(revealing_effect);
    });

    let line = move |index: usize| {
        let payload = Rc::clone(&payload);
        move || line_view(&payload, LineIndex(index), found, current.get(), current_piece)
    };

    view! {
        column(
            class = "wt-rp-json__lines",
            style:padding-top = move || Some(format!("{}px", seen.window().lead)),
            style:padding-bottom = move || Some(format!("{}px", seen.window().trail))
        ) {
            for index in move || seen.indices(), key = |index: &usize| *index {
                {line(index)}
            }
        }
    }
}

/// One line, highlighted, with its found places marked.
fn line_view(
    payload: &Payload,
    line: LineIndex,
    found: RwSignal<Rc<[Found]>, LocalStorage>,
    current: Option<FoundIndex>,
    current_piece: NodeRef,
) -> AnyView {
    let text = payload.line(line);
    let (marks, current_range) = found.with(|found| {
        let first = found.partition_point(|place| place.line < line);
        let last = found.partition_point(|place| place.line <= line);
        let marks: Vec<Range<usize>> = found[first..last].iter().map(|place| place.range.clone()).collect();
        let current_range =
            current.filter(|index| (first..last).contains(&index.0)).map(|index| found[index.0].range.clone());
        (marks, current_range)
    });
    let pieces: Vec<AnyView> = pieces(text, &tokens(text), &marks, current_range.as_ref())
        .into_iter()
        .map(|piece| {
            let said = text[piece.range.clone()].to_owned();
            let mark = match piece.mark {
                Mark::None => "",
                Mark::Found => " wt-json-found",
                Mark::Current => " wt-json-current",
            };
            let class = format!("{}{mark}", piece.kind.class().unwrap_or(""));
            if piece.mark == Mark::Current {
                AnyView::new(view! { text(class = class, node_ref = current_piece, attr:data-testid = "raw-json-current") {{said}} })
            } else {
                AnyView::new(view! { text(class = class) {{said}} })
            }
        })
        .collect();

    AnyView::new(view! { text(class = "wt-rp-json__line") {{pieces}} })
}

/// Scrolls `port`, holding `count` lines, so `line` is in the middle of it.
fn centre_line(port: NodeRef, count: usize, line: LineIndex) {
    let layout = zgui_ui::virtualize::window(count, LINE_HEIGHT, 0.0, 0.0, 0);
    let scale = port.scale();
    let position = port.scroll_position();
    let top = layout.offset_of(line.0) * scale;
    let target = (top - (position.scrollport.height.0 - LINE_HEIGHT * scale) / 2.0).max(0.0);

    port.scroll_to(ScrollTarget::Offset(Point::new(position.offset.x, DevicePx(target))), ScrollBehavior::Instant);
}

/// Every place `query` occurs in the payload, ignoring ASCII case, in reading order.
fn find_all(payload: &Payload, query: &str) -> Vec<Found> {
    if query.is_empty() {
        return Vec::new();
    }

    // ASCII case folding keeps every byte where it was, so a range in the folded line is the same
    // range in the line itself.
    let query = query.to_ascii_lowercase();
    let mut found = Vec::new();
    for (index, range) in payload.lines.iter().enumerate() {
        let folded = payload.content[range.clone()].to_ascii_lowercase();
        found.extend(
            folded
                .match_indices(&query)
                .map(|(start, _)| Found { line: LineIndex(index), range: start..start + query.len() }),
        );
    }

    found
}

/// The place after or before `current` among `count`, wrapping round.
fn stepped(current: Option<FoundIndex>, count: usize, direction: Step) -> Option<FoundIndex> {
    if count == 0 {
        return None;
    }

    let next = match (current, direction) {
        (None, Step::Next) => 0,
        (None, Step::Previous) => count - 1,
        (Some(at), Step::Next) => (at.0 + 1) % count,
        (Some(at), Step::Previous) => (at.0 + count - 1) % count,
    };

    Some(FoundIndex(next))
}

/// Cuts one line of pretty-printed JSON into runs. A string never spans lines there, since its
/// line breaks are escaped.
fn tokens(line: &str) -> Vec<(Range<usize>, TokenKind)> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let kind = match bytes[at] {
            byte if byte.is_ascii_whitespace() => {
                at += bytes[at..].iter().take_while(|byte| byte.is_ascii_whitespace()).count();
                TokenKind::Space
            }
            b'"' => {
                at = string_end(bytes, at);
                let rest = &bytes[at..];
                let colon = rest.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b':');
                if colon { TokenKind::Key } else { TokenKind::String }
            }
            b'-' | b'0'..=b'9' => {
                at += bytes[at..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E'))
                    .count();
                TokenKind::Number
            }
            byte if byte.is_ascii_alphabetic() => {
                at += bytes[at..].iter().take_while(|byte| byte.is_ascii_alphabetic()).count();
                TokenKind::Literal
            }
            _ => {
                // Outside a string only ASCII is valid JSON, but a stray character is stepped
                // over whole so every range stays on a character boundary.
                at += line[at..].chars().next().map_or(1, char::len_utf8);
                TokenKind::Punctuation
            }
        };
        tokens.push((start..at, kind));
    }

    tokens
}

/// The byte after the string opening at `open`, or the line's end when it is not closed.
fn string_end(bytes: &[u8], open: usize) -> usize {
    let mut at = open + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }

    bytes.len()
}

/// `line` cut at the edges of its tokens and of the found places on it, `current` among them.
fn pieces(
    line: &str,
    tokens: &[(Range<usize>, TokenKind)],
    marks: &[Range<usize>],
    current: Option<&Range<usize>>,
) -> Vec<Piece> {
    let mut edges: Vec<usize> = tokens
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .chain(marks.iter().flat_map(|range| [range.start, range.end]))
        .filter(|&edge| edge <= line.len())
        .collect();
    edges.sort_unstable();
    edges.dedup();

    edges
        .windows(2)
        .map(|pair| {
            let range = pair[0]..pair[1];
            let kind = tokens
                .iter()
                .find(|(token, _)| token.contains(&range.start))
                .map_or(TokenKind::Space, |(_, kind)| *kind);
            let mark = if current.is_some_and(|current| current.contains(&range.start)) {
                Mark::Current
            } else if marks.iter().any(|mark| mark.contains(&range.start)) {
                Mark::Found
            } else {
                Mark::None
            };
            Piece { range, kind, mark }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use landaire_ui::test_support::HeadlessOptions;
    use landaire_ui::test_support::HeadlessWindow;
    use landaire_ui::test_support::WindowSize;
    use wows_toolkit_viewmodel::settings::ThemeChoice;
    use zgui::vocab::Modifiers;

    use super::*;
    use crate::shell::ShellHarnessProps;

    fn kinds(line: &str) -> Vec<(&str, TokenKind)> {
        tokens(line).into_iter().map(|(range, kind)| (&line[range], kind)).collect()
    }

    /// A payload of `count` numbered keys, one per line.
    fn open(count: usize) -> HeadlessWindow {
        let lines: Vec<String> = (0..count).map(|line| format!("  \"key{line}\": {line},")).collect();
        let content: Rc<str> = Rc::from(format!("{{\n{}\n}}", lines.join("\n")));
        let mut window =
            HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 500.0, height: 400.0 }), move || {
                let content = Rc::clone(&content);
                view! {
                    ShellHarness(theme = ThemeChoice::Dark) {
                        RawJsonPanel(content = content)
                    }
                }
            })
            .expect("the panel opens");
        window.advance(Duration::from_millis(100));

        window
    }

    fn count(window: &HeadlessWindow) -> String {
        window.texts_by_class("wt-rp-json__count").concat()
    }

    /// Whether the current found place is drawn inside the scrolled body.
    fn current_in_view(window: &HeadlessWindow) -> bool {
        let body = window.by_class("wt-rp-json__body").and_then(|body| window.bounds(body));
        let current = window.by_test_id("raw-json-current").and_then(|current| window.bounds(current));
        let (Some(body), Some(current)) = (body, current) else { return false };

        body.top().0 <= current.top().0 && current.bottom().0 <= body.bottom().0
    }

    /// Typing finds the text and scrolls to it, Enter and Shift+Enter step through the places
    /// wrapping round, and Escape clears the search.
    #[test]
    fn the_search_scrolls_to_each_place_in_turn() {
        let mut window = open(2000);
        assert!(window.by_test_id("raw-json-current").is_none());
        assert!(window.texts_by_class("wt-rp-json__line").len() < 200, "only the lines in view are built");

        let field = window.by_test_id("raw-json-search").expect("the search field is drawn");
        window.click(field).expect("the field takes focus");
        window.type_text("KEY1500\"");
        window.advance(Duration::from_millis(100));
        assert_eq!(count(&window), "1/1");
        assert!(current_in_view(&window), "the body scrolled to line 1500");

        window.press_key(NamedKey::Backspace, Modifiers::NONE);
        window.press_key(NamedKey::Backspace, Modifiers::NONE);
        window.advance(Duration::from_millis(100));
        assert_eq!(count(&window), "1/11", "key150 and key1500 to key1509");

        window.press_key(NamedKey::Enter, Modifiers::NONE);
        window.advance(Duration::from_millis(100));
        assert_eq!(count(&window), "2/11");
        window.press_key(NamedKey::Enter, Modifiers::SHIFT);
        window.press_key(NamedKey::Enter, Modifiers::SHIFT);
        window.advance(Duration::from_millis(100));
        assert_eq!(count(&window), "11/11");
        assert!(current_in_view(&window));
        assert_eq!(window.texts_by_class("wt-json-current"), ["key150"], "one place is current");

        window.press_key(NamedKey::Escape, Modifiers::NONE);
        window.advance(Duration::from_millis(100));
        assert_eq!(count(&window), "");
        assert_eq!(window.texts_by_class("wt-json-found").len(), 0, "the marks are cleared");
        assert_eq!(window.texts_by_class("wt-json-current").len(), 0, "the marks are cleared");
    }

    #[test]
    fn a_line_is_cut_into_keys_values_and_punctuation() {
        assert_eq!(
            kinds(r#"  "name": "Ocean \"A\"","#),
            [
                ("  ", TokenKind::Space),
                (r#""name""#, TokenKind::Key),
                (":", TokenKind::Punctuation),
                (" ", TokenKind::Space),
                (r#""Ocean \"A\"""#, TokenKind::String),
                (",", TokenKind::Punctuation),
            ]
        );
        assert_eq!(
            kinds(r#""a": [-1.5e3, true, null]"#),
            [
                (r#""a""#, TokenKind::Key),
                (":", TokenKind::Punctuation),
                (" ", TokenKind::Space),
                ("[", TokenKind::Punctuation),
                ("-1.5e3", TokenKind::Number),
                (",", TokenKind::Punctuation),
                (" ", TokenKind::Space),
                ("true", TokenKind::Literal),
                (",", TokenKind::Punctuation),
                (" ", TokenKind::Space),
                ("null", TokenKind::Literal),
                ("]", TokenKind::Punctuation),
            ]
        );
    }

    #[test]
    fn an_unclosed_string_runs_to_the_line_end() {
        assert_eq!(kinds(r#""open \"#), [(r#""open \"#, TokenKind::String)]);
    }

    #[test]
    fn the_search_ignores_case_and_finds_every_place() {
        let payload = Payload::new(Rc::from("{\n  \"Map\": \"map_ocean\"\r\n}"));
        assert_eq!(
            find_all(&payload, "MAP"),
            [Found { line: LineIndex(1), range: 3..6 }, Found { line: LineIndex(1), range: 10..13 },]
        );
        assert_eq!(payload.line(LineIndex(1)), "  \"Map\": \"map_ocean\"");
        assert!(find_all(&payload, "").is_empty());
    }

    #[test]
    fn stepping_wraps_round_both_ways() {
        assert_eq!(stepped(None, 3, Step::Next), Some(FoundIndex(0)));
        assert_eq!(stepped(Some(FoundIndex(2)), 3, Step::Next), Some(FoundIndex(0)));
        assert_eq!(stepped(Some(FoundIndex(0)), 3, Step::Previous), Some(FoundIndex(2)));
        assert_eq!(stepped(Some(FoundIndex(1)), 0, Step::Next), None);
    }

    #[test]
    fn a_found_place_splits_the_tokens_it_crosses() {
        let line = r#""map": 12"#;
        let mark = 3..9;
        let cut: Vec<(&str, TokenKind, Mark)> = pieces(line, &tokens(line), std::slice::from_ref(&mark), Some(&mark))
            .into_iter()
            .map(|piece| (&line[piece.range], piece.kind, piece.mark))
            .collect();
        assert_eq!(
            cut,
            [
                (r#""ma"#, TokenKind::Key, Mark::None),
                (r#"p""#, TokenKind::Key, Mark::Current),
                (":", TokenKind::Punctuation, Mark::Current),
                (" ", TokenKind::Space, Mark::Current),
                ("12", TokenKind::Number, Mark::Current),
            ]
        );
    }
}

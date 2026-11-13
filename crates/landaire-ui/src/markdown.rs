//! A view of the common parts of a Markdown document: headings, paragraphs, lists, quotes, code
//! blocks and rules, with strong, emphasised, code and link runs inside them.
//!
//! Not a full CommonMark reader. Nested blocks inside list items, tables, images and reference
//! links are shown as the text they are written as.

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::reactive::Callable;
use zgui::reactive::UnsyncCallback;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

const SHEET: &str = "landaire-ui-markdown";

const CSS: &str = css!(
    ".lui-markdown {
        display: flex;
        flex-direction: column;
        gap: 6px;
        min-width: 0;
        user-select: text;
    }
    .lui-markdown__block,
    .lui-markdown__item-text,
    .lui-markdown__quote,
    .lui-markdown__code-block { user-select: text; }
    .lui-markdown__block { white-space: pre-wrap; }
    .lui-markdown__heading { font-weight: 700; color: var(--lui-color-text-bright); }
    .lui-markdown__heading[data-level=\"1\"] { font-size: 1.4em; margin-top: 4px; }
    .lui-markdown__heading[data-level=\"2\"] { font-size: 1.2em; margin-top: 4px; }
    .lui-markdown__heading[data-level=\"3\"] { font-size: 1.05em; }
    .lui-markdown__item {
        display: flex;
        flex-direction: row;
        gap: 6px;
        min-width: 0;
    }
    .lui-markdown__marker {
        flex: none;
        min-width: 12px;
        color: var(--lui-color-text-dim);
    }
    .lui-markdown__bullet {
        flex: none;
        width: 5px;
        height: 5px;
        margin: 0.55em 4px 0 3px;
        border-radius: 3px;
        background-color: var(--lui-color-text-dim);
    }
    .lui-markdown__bullet[data-hollow] {
        background-color: transparent;
        border: 1px solid var(--lui-color-text-dim);
    }
    .lui-markdown__item-text { flex: 1 1 auto; min-width: 0; white-space: pre-wrap; }
    .lui-markdown__quote {
        padding-left: 8px;
        border-left: 2px solid var(--lui-color-border-bright);
        color: var(--lui-color-text-dim);
        white-space: pre-wrap;
    }
    .lui-markdown__code-block {
        padding: 6px 8px;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-widget);
        font-family: var(--lui-font-family-mono);
        white-space: pre;
        overflow-x: auto;
    }
    .lui-markdown__rule {
        flex: none;
        height: 1px;
        background-color: var(--lui-color-border);
    }
    .lui-markdown__strong { font-weight: 700; }
    .lui-markdown__emphasis { font-style: italic; }
    .lui-markdown__code {
        font-family: var(--lui-font-family-mono);
        background-color: var(--lui-color-widget);
        border-radius: 3px;
    }
    .lui-markdown__link {
        color: var(--lui-color-accent);
        text-decoration: underline;
        cursor: pointer;
    }
    .lui-markdown__link:focus-visible {
        outline: 1px solid var(--lui-color-accent);
        outline-offset: 1px;
        border-radius: 2px;
    }"
);

/// How deep a heading is, 1 to 6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadingLevel(u8);

impl HeadingLevel {
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// How far a list item is nested, 0 for the outermost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListDepth(u8);

impl ListDepth {
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// What a list item starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMarker {
    Bullet,
    Number(u64),
}

/// How a run of text is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunStyle {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
}

/// Text set one way, and where it links to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<String>,
}

/// One block of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading { level: HeadingLevel, runs: Vec<Run> },
    Paragraph(Vec<Run>),
    ListItem { depth: ListDepth, marker: ListMarker, runs: Vec<Run> },
    Quote(Vec<Run>),
    Code(String),
    Rule,
}

/// The blocks of `source`, in order.
pub fn parse(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut open = Open::None;
    let mut code: Option<String> = None;

    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(text) = code.as_mut() {
            if trimmed.starts_with("```") {
                let mut text = std::mem::take(text);
                text.truncate(text.trim_end_matches('\n').len());
                blocks.push(Block::Code(text));
                code = None;
            } else {
                text.push_str(line);
                text.push('\n');
            }
            continue;
        }

        if trimmed.starts_with("```") {
            open.close_into(&mut blocks);
            code = Some(String::new());
        } else if trimmed.is_empty() {
            open.close_into(&mut blocks);
        } else if is_rule(trimmed) {
            open.close_into(&mut blocks);
            blocks.push(Block::Rule);
        } else if let Some((level, text)) = heading(trimmed) {
            open.close_into(&mut blocks);
            blocks.push(Block::Heading { level, runs: runs(text) });
        } else if let Some(text) = trimmed.strip_prefix('>') {
            let text = text.trim_start();
            match &mut open {
                Open::Quote(held) => held.join(text, line),
                _ => {
                    open.close_into(&mut blocks);
                    open = Open::Quote(Held::new(text, line));
                }
            }
        } else if let Some((marker, text)) = list_item(trimmed) {
            open.close_into(&mut blocks);
            open = Open::Item { depth: depth_of(line), marker, held: Held::new(text, line) };
        } else {
            match &mut open {
                Open::Paragraph(held) | Open::Quote(held) | Open::Item { held, .. } => held.join(trimmed, line),
                Open::None => open = Open::Paragraph(Held::new(trimmed, line)),
            }
        }
    }

    open.close_into(&mut blocks);
    // An unclosed fence runs to the end of the document.
    if let Some(mut text) = code {
        text.truncate(text.trim_end_matches('\n').len());
        blocks.push(Block::Code(text));
    }

    blocks
}

/// The block whose lines are still being read.
enum Open {
    None,
    Paragraph(Held),
    Quote(Held),
    Item { depth: ListDepth, marker: ListMarker, held: Held },
}

impl Open {
    fn close_into(&mut self, blocks: &mut Vec<Block>) {
        match std::mem::replace(self, Self::None) {
            Self::None => {}
            Self::Paragraph(held) => blocks.push(Block::Paragraph(runs(&held.text))),
            Self::Quote(held) => blocks.push(Block::Quote(runs(&held.text))),
            Self::Item { depth, marker, held } => {
                blocks.push(Block::ListItem { depth, marker, runs: runs(&held.text) })
            }
        }
    }
}

/// The text of an open block, trimmed, and whether its last line asked for a hard break.
struct Held {
    text: String,
    /// The last line ended in two or more spaces, which trimming took off.
    spaced_break: bool,
}

impl Held {
    /// `text` is the trimmed content of `line`.
    fn new(text: &str, line: &str) -> Self {
        Self { text: text.to_owned(), spaced_break: line.ends_with("  ") }
    }

    /// Joins a wrapped line onto the block it continues. A line ending in two spaces or a
    /// backslash is a hard break; at the end of a block neither is, so the backslash stays.
    fn join(&mut self, text: &str, line: &str) {
        if let Some(kept) = self.text.strip_suffix('\\') {
            self.text.truncate(kept.len());
            self.text.push('\n');
        } else if self.spaced_break {
            self.text.push('\n');
        } else {
            self.text.push(' ');
        }
        self.text.push_str(text);
        self.spaced_break = line.ends_with("  ");
    }
}

fn is_rule(line: &str) -> bool {
    let mut marks = line.chars().filter(|c| !c.is_whitespace());
    let Some(first) = marks.next() else { return false };
    matches!(first, '-' | '*' | '_') && marks.clone().count() >= 2 && marks.all(|c| c == first)
}

fn heading(line: &str) -> Option<(HeadingLevel, &str)> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();

    Some((HeadingLevel(hashes as u8), text))
}

fn list_item(line: &str) -> Option<(ListMarker, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(bullet) {
            return Some((ListMarker::Bullet, text.trim_start()));
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &line[digits..];
    let text = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))?;
    let number = line[..digits].parse().ok()?;

    Some((ListMarker::Number(number), text.trim_start()))
}

/// Two spaces of indent, or one tab, per level.
fn depth_of(line: &str) -> ListDepth {
    let mut columns = 0usize;
    for c in line.chars() {
        match c {
            ' ' => columns += 1,
            '\t' => columns += 4,
            _ => break,
        }
    }

    ListDepth(u8::try_from(columns / 2).unwrap_or(u8::MAX).min(6))
}

/// The runs of one block's text.
pub fn runs(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut reader = RunReader { out: Vec::new(), held: String::new(), style: RunStyle::default(), link: None };
    reader.read(&chars);
    reader.flush();

    reader.out
}

struct RunReader {
    out: Vec<Run>,
    held: String,
    style: RunStyle,
    link: Option<String>,
}

impl RunReader {
    fn flush(&mut self) {
        if !self.held.is_empty() {
            self.out.push(Run { text: std::mem::take(&mut self.held), style: self.style, link: self.link.clone() });
        }
    }

    /// Flips `flag` at a delimiter, when the delimiter can open or close there: an opening one is
    /// followed by text and a closing one follows text, so `2 * 3` stays as written.
    fn toggle(&mut self, chars: &[char], at: usize, width: usize, flag: fn(&mut RunStyle) -> &mut bool) -> bool {
        let before = at.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let after = chars.get(at + width).copied();
        let on = *flag(&mut self.style);
        let fits =
            if on { before.is_some_and(|c| !c.is_whitespace()) } else { after.is_some_and(|c| !c.is_whitespace()) };
        if !fits {
            return false;
        }
        self.flush();
        let flag = flag(&mut self.style);
        *flag = !*flag;

        true
    }

    fn read(&mut self, chars: &[char]) {
        let mut at = 0;
        while at < chars.len() {
            let c = chars[at];
            let next = chars.get(at + 1).copied();
            let before = at.checked_sub(1).map(|i| chars[i]);

            if c == '\\' && next.is_some_and(|n| n.is_ascii_punctuation()) {
                self.held.extend(next);
                at += 2;
                continue;
            }
            if c == '`'
                && let Some(close) = find(chars, at + 1, |c| c == '`')
            {
                self.flush();
                let code: String = chars[at + 1..close].iter().collect();
                let style = RunStyle { code: true, ..self.style };
                self.out.push(Run { text: code, style, link: self.link.clone() });
                at = close + 1;
                continue;
            }
            if (c == '*' || c == '_') && next == Some(c) && self.toggle(chars, at, 2, |style| &mut style.strong) {
                at += 2;
                continue;
            }
            // An underscore inside a word, as in a file name, is not emphasis.
            let inside_word =
                c == '_' && before.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
            if (c == '*' || c == '_') && !inside_word && self.toggle(chars, at, 1, |style| &mut style.emphasis) {
                at += 1;
                continue;
            }
            if c == '['
                && self.link.is_none()
                && let Some(end) = self.link_at(chars, at)
            {
                at = end;
                continue;
            }
            if c == '<'
                && let Some(close) = find(chars, at + 1, |c| c == '>')
            {
                let target: String = chars[at + 1..close].iter().collect();
                if is_url(&target) && self.link.is_none() {
                    self.push_link(target.clone(), target);
                    at = close + 1;
                    continue;
                }
            }
            if self.link.is_none() && before.is_none_or(|b| !b.is_alphanumeric()) && starts_url(&chars[at..]) {
                let mut end = find(chars, at, char::is_whitespace).unwrap_or(chars.len());
                while end > at && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | '\'' | '"') {
                    end -= 1;
                }
                let target: String = chars[at..end].iter().collect();
                self.push_link(target.clone(), target);
                at = end;
                continue;
            }

            self.held.push(c);
            at += 1;
        }
    }

    /// Reads `[text](target)` starting at `at`, and answers where it ends.
    fn link_at(&mut self, chars: &[char], at: usize) -> Option<usize> {
        let close = find(chars, at + 1, |c| c == ']')?;
        if chars.get(close + 1) != Some(&'(') {
            return None;
        }
        let end = find(chars, close + 2, |c| c == ')')?;
        let target: String = chars[close + 2..end].iter().collect();
        let target = target.split_whitespace().next().unwrap_or_default().to_owned();

        self.flush();
        self.link = Some(target);
        self.read(&chars[at + 1..close]);
        self.flush();
        self.link = None;

        Some(end + 1)
    }

    fn push_link(&mut self, text: String, target: String) {
        self.flush();
        self.out.push(Run { text, style: self.style, link: Some(target) });
    }
}

fn find(chars: &[char], from: usize, matches: impl Fn(char) -> bool) -> Option<usize> {
    chars.get(from..)?.iter().position(|c| matches(*c)).map(|i| from + i)
}

fn is_url(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://")
}

fn starts_url(chars: &[char]) -> bool {
    let head: String = chars.iter().take(8).collect();
    is_url(&head)
}

/// A Markdown document, laid out as blocks. Its text can be selected.
#[component]
pub fn Markdown(
    #[prop(into)] source: String,
    /// Called with a link's target when the link is clicked. Left out, links are inert.
    #[prop(optional)]
    on_link: Option<UnsyncCallback<String>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let blocks: Vec<AnyView> = parse(&source).into_iter().map(|block| block_view(block, on_link)).collect();
    let class = Classes::from("lui-markdown").merged(class);

    view! {
        column({..attrs}, class = class) {{blocks}}
    }
}

fn block_view(block: Block, on_link: Option<UnsyncCallback<String>>) -> AnyView {
    match block {
        Block::Heading { level, runs } => AnyView::new(view! {
            text(class = "lui-markdown__block lui-markdown__heading", attr:data-level = level.get().to_string()) {
                {runs_view(runs, on_link)}
            }
        }),
        Block::Paragraph(runs) => AnyView::new(view! {
            text(class = "lui-markdown__block") {{runs_view(runs, on_link)}}
        }),
        Block::ListItem { depth, marker, runs } => {
            let marker = match marker {
                ListMarker::Bullet => {
                    let hollow = (depth.get() % 2 == 1).then(String::new);
                    AnyView::new(view! { box(class = "lui-markdown__bullet", attr:data-hollow = hollow) })
                }
                ListMarker::Number(number) => {
                    AnyView::new(view! { text(class = "lui-markdown__marker") {{format!("{number}.")}} })
                }
            };
            let indent = format!("{}px", u32::from(depth.get()) * 16);
            AnyView::new(view! {
                row(class = "lui-markdown__item", style:padding-left = indent) {
                    {marker}
                    text(class = "lui-markdown__item-text") {{runs_view(runs, on_link)}}
                }
            })
        }
        Block::Quote(runs) => AnyView::new(view! {
            text(class = "lui-markdown__quote") {{runs_view(runs, on_link)}}
        }),
        Block::Code(code) => AnyView::new(view! {
            text(class = "lui-markdown__code-block") {{code}}
        }),
        Block::Rule => AnyView::new(view! { box(class = "lui-markdown__rule") }),
    }
}

fn runs_view(runs: Vec<Run>, on_link: Option<UnsyncCallback<String>>) -> Vec<AnyView> {
    runs.into_iter()
        .map(|run| {
            let mut class = String::from("lui-markdown__run");
            for (on, name) in [
                (run.style.strong, " lui-markdown__strong"),
                (run.style.emphasis, " lui-markdown__emphasis"),
                (run.style.code, " lui-markdown__code"),
                (run.link.is_some(), " lui-markdown__link"),
            ] {
                if on {
                    class.push_str(name);
                }
            }
            match (run.link, on_link) {
                (Some(target), Some(on_link)) => {
                    let pressed = target.clone();
                    AnyView::new(view! {
                        text(
                            class = class,
                            attr:data-href = target.clone(),
                            tabindex = {Focus::Sequential},
                            a11y:role = {Role::Link},
                            on:click = move |_| on_link.run(pressed.clone()),
                            on:key_down = move |ev| {
                                if ev.key != Key::Named(NamedKey::Enter) || ev.modifiers != Modifiers::NONE {
                                    return;
                                }

                                on_link.run(target.clone());
                                ev.prevent_default();
                                ev.stop_propagation();
                            }
                        ) {{run.text}}
                    })
                }
                (Some(target), None) => {
                    AnyView::new(view! { text(class = class, attr:data-href = target) {{run.text}} })
                }
                (None, _) => AnyView::new(view! { text(class = class) {{run.text}} }),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_read_as_blocks() {
        let notes = "## What's new\n\
            \n\
            - **Replays**: the table keeps its *column order*\n\
            - Fixed `wows_toolkit.exe` crashing, see [#123](https://github.com/x/y/issues/123)\n\
            \x20 which also\n\
            \x20 - nested point\n\
            1. first\n\
            \n\
            > a quote\n\
            ---\n\
            ```\n\
            let a = 1;\n\
            ```\n\
            Full changelog: https://github.com/x/y/compare/a...b.\n\
            2 * 3 and snake_case_name stay as written.";
        insta::assert_debug_snapshot!(parse(notes));
    }

    #[test]
    fn a_heading_needs_a_space_after_its_hashes() {
        assert_eq!(parse("#hashtag"), [Block::Paragraph(runs("#hashtag"))]);
        assert_eq!(parse("### Fixes ###"), [Block::Heading { level: HeadingLevel(3), runs: runs("Fixes") }]);
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end() {
        assert_eq!(parse("```\ncode\n\nmore"), [Block::Code("code\n\nmore".to_owned())]);
    }

    #[test]
    fn two_spaces_or_a_backslash_end_a_line_with_a_hard_break() {
        assert_eq!(parse("one  \ntwo"), [Block::Paragraph(runs("one\ntwo"))]);
        assert_eq!(parse("one\\\ntwo"), [Block::Paragraph(runs("one\ntwo"))]);
        assert_eq!(parse("one \ntwo"), [Block::Paragraph(runs("one two"))], "one space is a soft break");
        assert_eq!(parse("> one  \n> two"), [Block::Quote(runs("one\ntwo"))]);
        assert_eq!(
            parse("- one\\\n  two"),
            [Block::ListItem { depth: ListDepth(0), marker: ListMarker::Bullet, runs: runs("one\ntwo") }]
        );
        assert_eq!(parse("last\\"), [Block::Paragraph(runs("last\\"))], "a backslash ending the block stays");
    }
}

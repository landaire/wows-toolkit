//! A file viewer panel: the contents of one VFS file, as read-only text in a monospace face or as
//! an image.
//!
//! Text is highlighted off the UI thread where a grammar covers the format, and shown plain until
//! the highlighting lands.

use std::rc::Rc;
use std::sync::LazyLock;

use rust_i18n::t;
use syntect::parsing::ParseState;
use syntect::parsing::ScopeStack;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use wows_toolkit_viewmodel::unpacker::viewer::ViewerContent;
use wows_toolkit_viewmodel::unpacker::viewer::highlight_language;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::PropValue;
use zgui_image::ImageBytes;

use crate::tabs::unpacker::text_view::TextViewProps;

const SHEET: &str = "wt-unpacker-viewer";

// The token colours borrow the semantic tones, which already differ per appearance.
const CSS: &str = css!(
    ".wt-viewer { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-viewer__header {
        flex: none;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
        font-size: 12px;
        color: var(--wt-color-text-dim);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .wt-syn-comment { color: var(--wt-color-text-faint); }
    .wt-syn-string { color: var(--wt-color-chat-team); }
    .wt-syn-number { color: var(--wt-color-armor-ricochet); }
    .wt-syn-constant { color: var(--wt-color-division); }
    .wt-syn-keyword { color: var(--wt-color-abuser); }
    .wt-syn-tag { color: var(--wt-color-abuser); }
    .wt-syn-attribute { color: var(--wt-color-chat-other); }
    .wt-syn-punctuation { color: var(--wt-color-text-dim); }
    .wt-viewer__image { flex: 1 1 auto; min-height: 0; overflow: auto; padding: 8px; }
    .wt-viewer__svg { width: 512px; height: 512px; }
    .wt-viewer__failed {
        display: flex;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        font-size: 13px;
        color: var(--wt-color-text-dim);
    }"
);

/// Text larger than this is shown plain: highlighting holds a span per token in memory.
const HIGHLIGHT_LIMIT: usize = 4 * 1024 * 1024;

/// What a highlighted run of text is, as far as its colour goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenClass {
    Comment,
    String,
    Number,
    Constant,
    Keyword,
    Tag,
    Attribute,
    Punctuation,
}

impl TokenClass {
    pub(super) const fn class(self) -> &'static str {
        match self {
            Self::Comment => "wt-syn-comment",
            Self::String => "wt-syn-string",
            Self::Number => "wt-syn-number",
            Self::Constant => "wt-syn-constant",
            Self::Keyword => "wt-syn-keyword",
            Self::Tag => "wt-syn-tag",
            Self::Attribute => "wt-syn-attribute",
            Self::Punctuation => "wt-syn-punctuation",
        }
    }

    /// The class of the innermost scope that has one.
    fn of(stack: &ScopeStack) -> Option<Self> {
        stack.as_slice().iter().rev().find_map(|scope| {
            let name = scope.build_string();
            const RULES: [(&str, TokenClass); 9] = [
                ("comment", TokenClass::Comment),
                ("string", TokenClass::String),
                ("constant.numeric", TokenClass::Number),
                ("constant", TokenClass::Constant),
                ("keyword", TokenClass::Keyword),
                ("storage", TokenClass::Keyword),
                ("entity.name.tag", TokenClass::Tag),
                ("entity.other.attribute-name", TokenClass::Attribute),
                ("punctuation", TokenClass::Punctuation),
            ];
            RULES.iter().find(|(prefix, _)| name.starts_with(prefix)).map(|(_, class)| *class)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub class: Option<TokenClass>,
    pub text: String,
}

pub type CodeLine = Vec<Token>;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// `text` split into lines of one plain token each.
pub fn plain_lines(text: &str) -> Vec<CodeLine> {
    text.lines().map(|line| vec![Token { class: None, text: line.to_string() }]).collect()
}

/// `text` highlighted by the grammar its extension names, or `None` when no grammar covers the
/// format or the text is too large.
pub fn highlight(extension: &str, text: &str) -> Option<Vec<CodeLine>> {
    highlight_language(extension)?;
    if text.len() > HIGHLIGHT_LIMIT {
        return None;
    }
    let set = &*SYNTAXES;
    let syntax = set.find_syntax_by_extension(extension.trim_start_matches('.').to_ascii_lowercase().as_str())?;
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(text) {
        let ops = state.parse_line(line, set).ok()?;
        let content = line.trim_end_matches(['\n', '\r']);
        let mut tokens: CodeLine = Vec::new();
        let mut at = 0;
        for (index, op) in ops {
            let end = index.min(content.len());
            if end > at {
                tokens.push(Token { class: TokenClass::of(&stack), text: content[at..end].to_string() });
                at = end;
            }
            stack.apply(&op).ok()?;
        }
        if content.len() > at {
            tokens.push(Token { class: TokenClass::of(&stack), text: content[at..].to_string() });
        }
        lines.push(tokens);
    }
    Some(lines)
}

/// What a viewer panel shows.
#[derive(Clone)]
pub enum Shown {
    Text(RwSignal<Rc<Vec<CodeLine>>, LocalStorage>),
    Image(Rc<ImageBytes>),
    Svg(String),
    /// The bytes loaded but could not be decoded as an image.
    Undecodable(String),
}

/// One open file. `Copy`: every field is a handle on state owned by the tab.
#[derive(Clone, Copy)]
pub struct ViewerDoc {
    pub title: StoredValue<String, LocalStorage>,
    pub shown: StoredValue<Shown, LocalStorage>,
}

impl ViewerDoc {
    /// Shows `content` at once, plain, and starts highlighting text in the background.
    pub fn open(title: String, content: ViewerContent) -> Self {
        let shown = match content {
            ViewerContent::Plaintext { extension, text } => {
                let lines = RwSignal::new_local(Rc::new(plain_lines(&text)));
                spawn_local(async move {
                    if let Some(highlighted) = blocking(move || highlight(&extension, &text)).await {
                        lines.set(Rc::new(highlighted));
                    }
                });
                Shown::Text(lines)
            }
            ViewerContent::Image { bytes } if title.to_ascii_lowercase().ends_with(".svg") => {
                match String::from_utf8(bytes) {
                    Ok(svg) => Shown::Svg(svg),
                    Err(err) => Shown::Undecodable(err.to_string()),
                }
            }
            ViewerContent::Image { bytes } => match zgui_image::probe(&bytes) {
                Ok(_) => Shown::Image(Rc::new(ImageBytes::new(bytes))),
                Err(err) => Shown::Undecodable(err.to_string()),
            },
        };
        Self { title: StoredValue::new_local(title), shown: StoredValue::new_local(shown) }
    }
}

#[component]
pub fn ViewerPanel(doc: ViewerDoc) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let body = match doc.shown.get_value() {
        Shown::Text(lines) => AnyView::new(view! { TextView(lines = lines, title = doc.title.get_value()) }),
        Shown::Image(bytes) => AnyView::new(view! {
            box(class = "wt-viewer__image", attr:data-testid = "unpacker-viewer-image") {
                image(src = bytes.url())
            }
        }),
        Shown::Svg(svg) => AnyView::new(view! {
            box(class = "wt-viewer__image", attr:data-testid = "unpacker-viewer-image") {
                vector(class = "wt-viewer__svg", prop:svg = PropValue::from(svg))
            }
        }),
        Shown::Undecodable(reason) => AnyView::new(view! {
            box(class = "wt-viewer__failed", attr:data-testid = "unpacker-viewer-failed") {
                text {{t!("ui.unpacker.image_decode_failed", reason = reason).into_owned()}}
            }
        }),
    };

    view! {
        column(class = "wt-viewer", attr:data-testid = "unpacker-viewer") {
            text(class = "wt-viewer__header") {{doc.title.get_value()}}
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_split_into_classed_tokens() {
        let lines = highlight(".json", "{\"a\": 1}\n").expect("JSON has a grammar");
        assert_eq!(lines.len(), 1);
        let classes: Vec<(Option<TokenClass>, &str)> =
            lines[0].iter().map(|token| (token.class, token.text.as_str())).collect();
        assert!(classes.contains(&(Some(TokenClass::Number), "1")), "the number is a number: {classes:?}");
        assert!(
            classes.iter().any(|(class, text)| *class == Some(TokenClass::String) && text.contains('a')),
            "the key is a string: {classes:?}"
        );
        let text: String = lines[0].iter().map(|token| token.text.as_str()).collect();
        assert_eq!(text, "{\"a\": 1}", "the tokens are the line, with no newline");
    }

    #[test]
    fn a_format_without_a_grammar_is_left_plain() {
        assert_eq!(highlight(".log", "a log line"), None);
        assert_eq!(
            plain_lines("one\ntwo"),
            vec![
                vec![Token { class: None, text: "one".to_string() }],
                vec![Token { class: None, text: "two".to_string() }],
            ]
        );
    }
}

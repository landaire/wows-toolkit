use std::borrow::Cow;
use std::fmt;

use crate::error::PrefsEditError;
use crate::error::PrefsParseError;

/// A BigWorld-style XML file kept byte for byte.
///
/// Tag names may start with digits or contain `@`, values carry padding, and a
/// BOM may lead. Edits replace only the text of the value they change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefsDocument {
    source: String,
    elements: Vec<Element>,

    /// Whitespace the file places around element values, reused when a value
    /// is written into an element that had none.
    value_padding: Span,
}

/// An element path relative to the root element, with `/` between names.
/// Each name selects the first child with that name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PrefsPath(Cow<'static, str>);

/// An attribute name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AttrName(Cow<'static, str>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefsEdit {
    /// Replaces the text of an element that has no child elements.
    SetValue { path: PrefsPath, value: String },
    /// Replaces the value of an existing attribute.
    SetAttribute { path: PrefsPath, name: AttrName, value: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteOffset(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ElementId(usize);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
    name: Span,
    attributes: Vec<Attribute>,
    body: Body,
    children: Vec<ElementId>,

    /// Comments, CDATA or processing instructions in the content, which a
    /// value edit would remove.
    has_markup: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Attribute {
    name: Span,
    /// The text between the quotes.
    value: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Body {
    /// `<name ... />`. The span covers the closing `/>`.
    SelfClosing { close: Span },
    /// `<name ...>content</name>`.
    Content { content: Span },
}

enum Markup {
    Skip { end: usize },
    Open { element: Element, end: usize },
    Close { name: Span, end: usize },
}

impl PrefsDocument {
    pub fn parse(source: String) -> Result<PrefsDocument, PrefsParseError> {
        let mut elements: Vec<Element> = Vec::new();
        let mut open: Vec<ElementId> = Vec::new();
        let mut root: Option<ElementId> = None;
        let bytes = source.as_bytes();

        let mut position = source.strip_prefix('\u{FEFF}').map_or(0, |_| '\u{FEFF}'.len_utf8());
        while position < bytes.len() {
            let Some(found) = bytes[position..].iter().position(|&byte| byte == b'<') else {
                check_outside_text(&source, &open, position, bytes.len())?;
                break;
            };

            let tag_start = position + found;
            check_outside_text(&source, &open, position, tag_start)?;

            match read_markup(&source, tag_start)? {
                Markup::Skip { end } => {
                    if let Some(parent) = open.last() {
                        elements[parent.0].has_markup = true;
                    }

                    position = end;
                }
                Markup::Open { element, end } => {
                    if open.is_empty() && root.is_some() {
                        return Err(PrefsParseError::MultipleRoots { offset: ByteOffset(tag_start) });
                    }

                    let id = ElementId(elements.len());
                    let is_open = matches!(element.body, Body::Content { .. });
                    elements.push(element);
                    match open.last() {
                        Some(parent) => elements[parent.0].children.push(id),
                        None => root = Some(id),
                    }
                    if is_open {
                        open.push(id);
                    }

                    position = end;
                }
                Markup::Close { name, end } => {
                    let Some(id) = open.pop() else {
                        return Err(PrefsParseError::UnexpectedCloseTag { offset: ByteOffset(tag_start) });
                    };

                    let element = &mut elements[id.0];
                    if source[element.name.range()] != source[name.range()] {
                        return Err(PrefsParseError::MismatchedCloseTag {
                            offset: ByteOffset(tag_start),
                            expected: source[element.name.range()].to_owned(),
                            found: source[name.range()].to_owned(),
                        });
                    }

                    // The content span was opened with its start; the close tag fixes its end.
                    if let Body::Content { content } = &mut element.body {
                        content.end = tag_start;
                    }

                    position = end;
                }
            }
        }

        if let Some(unclosed) = open.last() {
            return Err(PrefsParseError::UnclosedElement {
                name: source[elements[unclosed.0].name.range()].to_owned(),
            });
        }

        if root != Some(ElementId(0)) {
            return Err(PrefsParseError::NoRootElement);
        }

        let value_padding = detect_value_padding(&source, &elements);

        Ok(PrefsDocument { source, elements, value_padding })
    }

    /// The document text, identical to the parsed input until edited.
    pub fn text(&self) -> &str {
        &self.source
    }

    pub fn into_text(self) -> String {
        self.source
    }

    /// The name of the root element, for example `preferences.xml`.
    pub fn root_name(&self) -> &str {
        &self.source[self.elements[0].name.range()]
    }

    pub fn contains(&self, path: &PrefsPath) -> bool {
        self.find(path).is_some()
    }

    /// The text of an element without child elements, without its padding.
    pub fn value(&self, path: &PrefsPath) -> Option<&str> {
        let element = &self.elements[self.find(path)?.0];
        if !element.children.is_empty() {
            return None;
        }

        match element.body {
            Body::SelfClosing { .. } => Some(""),
            Body::Content { content } => Some(self.source[content.range()].trim_matches(is_xml_space)),
        }
    }

    pub fn attribute(&self, path: &PrefsPath, name: &AttrName) -> Option<&str> {
        let element = &self.elements[self.find(path)?.0];
        let attribute = self.find_attribute(element, name)?;

        Some(&self.source[attribute.value.range()])
    }

    /// The names of an element's children, in document order.
    pub fn child_names(&self, path: &PrefsPath) -> Option<impl Iterator<Item = &str>> {
        let element = &self.elements[self.find(path)?.0];

        Some(element.children.iter().map(|child| &self.source[self.elements[child.0].name.range()]))
    }

    /// Rewrites only the edited span; keeps padding and everything else. An
    /// edit to the current value leaves the text untouched.
    pub fn set(&mut self, edit: &PrefsEdit) -> Result<(), PrefsEditError> {
        let replacement = match edit {
            PrefsEdit::SetValue { path, value } => self.value_replacement(path, value)?,
            PrefsEdit::SetAttribute { path, name, value } => self.attribute_replacement(path, name, value)?,
        };
        let Some((replaced, replacement)) = replacement else {
            return Ok(());
        };

        let mut source = String::with_capacity(self.source.len() + replacement.len());
        source.push_str(&self.source[..replaced.start]);
        source.push_str(&replacement);
        source.push_str(&self.source[replaced.end..]);

        // Spans after the edit all move; parsing again is simpler than
        // shifting them and the files are small.
        *self = PrefsDocument::parse(source)?;

        Ok(())
    }

    fn value_replacement(&self, path: &PrefsPath, value: &str) -> Result<Option<(Span, String)>, PrefsEditError> {
        check_text(value, None)?;

        let Some(id) = self.find(path) else {
            return Err(PrefsEditError::ElementNotFound { path: path.clone() });
        };

        let element = &self.elements[id.0];
        if !element.children.is_empty() {
            return Err(PrefsEditError::HasChildren { path: path.clone() });
        }
        if element.has_markup {
            return Err(PrefsEditError::HasMarkup { path: path.clone() });
        }

        if self.value(path) == Some(value) {
            return Ok(None);
        }

        let padding = &self.source[self.value_padding.range()];
        let edit = match element.body {
            Body::SelfClosing { close } => {
                let name = &self.source[element.name.range()];
                (close, format!(">{padding}{value}{padding}</{name}>"))
            }
            Body::Content { content } => {
                let text = &self.source[content.range()];
                let trimmed = text.trim_matches(is_xml_space);
                if trimmed.is_empty() {
                    (content, format!("{padding}{value}{padding}"))
                } else {
                    let start = content.start + (text.len() - text.trim_start_matches(is_xml_space).len());
                    (Span { start, end: start + trimmed.len() }, value.to_owned())
                }
            }
        };

        Ok(Some(edit))
    }

    fn attribute_replacement(
        &self,
        path: &PrefsPath,
        name: &AttrName,
        value: &str,
    ) -> Result<Option<(Span, String)>, PrefsEditError> {
        let Some(id) = self.find(path) else {
            return Err(PrefsEditError::ElementNotFound { path: path.clone() });
        };

        let element = &self.elements[id.0];
        let Some(attribute) = self.find_attribute(element, name) else {
            return Err(PrefsEditError::AttributeNotFound { path: path.clone(), name: name.clone() });
        };

        let quote = self.source.as_bytes()[attribute.value.start - 1];
        check_text(value, Some(char::from(quote)))?;

        if self.source[attribute.value.range()] == *value {
            return Ok(None);
        }

        Ok(Some((attribute.value, value.to_owned())))
    }

    fn find(&self, path: &PrefsPath) -> Option<ElementId> {
        let mut current = ElementId(0);
        for segment in path.segments() {
            let element = &self.elements[current.0];
            current =
                *element.children.iter().find(|child| &self.source[self.elements[child.0].name.range()] == segment)?;
        }

        Some(current)
    }

    fn find_attribute(&self, element: &Element, name: &AttrName) -> Option<Attribute> {
        element.attributes.iter().copied().find(|attribute| self.source[attribute.name.range()] == *name.as_str())
    }
}

impl PrefsPath {
    /// Panics when `path` is empty or has an empty segment or a character a
    /// tag name cannot hold; in a `const` this fails the build.
    pub const fn new(path: &'static str) -> PrefsPath {
        assert!(is_valid_path(path.as_bytes()), "invalid preferences path");

        PrefsPath(Cow::Borrowed(path))
    }

    pub fn parse(path: &str) -> Option<PrefsPath> {
        is_valid_path(path.as_bytes()).then(|| PrefsPath(Cow::Owned(path.to_owned())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for PrefsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AttrName {
    /// Panics when `name` is empty or holds a character an attribute name
    /// cannot; in a `const` this fails the build.
    pub const fn new(name: &'static str) -> AttrName {
        assert!(is_valid_name(name.as_bytes(), 0, name.len()), "invalid attribute name");

        AttrName(Cow::Borrowed(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AttrName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for ByteOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Span {
    fn range(self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

fn is_xml_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n')
}

const fn is_name_byte(byte: u8) -> bool {
    !matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'<' | b'>' | b'/' | b'=' | b'"' | b'\'' | b'&')
}

const fn is_valid_name(bytes: &[u8], start: usize, end: usize) -> bool {
    if start == end {
        return false;
    }

    let mut idx = start;
    while idx < end {
        if !is_name_byte(bytes[idx]) {
            return false;
        }
        idx += 1;
    }

    true
}

const fn is_valid_path(bytes: &[u8]) -> bool {
    let mut start = 0;
    let mut idx = 0;
    while idx <= bytes.len() {
        if idx == bytes.len() || bytes[idx] == b'/' {
            if !is_valid_name(bytes, start, idx) {
                return false;
            }
            start = idx + 1;
        }
        idx += 1;
    }

    true
}

/// Rejects text the game's reader would not return unchanged. Attribute
/// values are delimited by `quote`; element text by tags.
fn check_text(value: &str, quote: Option<char>) -> Result<(), PrefsEditError> {
    let forbidden = |character: char| match quote {
        Some(quote) => matches!(character, '<' | '&') || character == quote,
        None => matches!(character, '<' | '>' | '&'),
    };
    if let Some(character) = value.chars().find(|&character| forbidden(character)) {
        return Err(PrefsEditError::ForbiddenCharacter { character });
    }

    // The reader trims values, so padding inside one would not survive a read.
    if quote.is_none() && value.trim_matches(is_xml_space).len() != value.len() {
        return Err(PrefsEditError::SurroundingWhitespace);
    }

    Ok(())
}

fn check_outside_text(source: &str, open: &[ElementId], start: usize, end: usize) -> Result<(), PrefsParseError> {
    if !open.is_empty() || source[start..end].trim_matches(is_xml_space).is_empty() {
        return Ok(());
    }

    Err(PrefsParseError::TextOutsideRoot { offset: ByteOffset(start) })
}

fn find_from(source: &str, from: usize, needle: &str) -> Option<usize> {
    source[from..].find(needle).map(|found| from + found)
}

fn read_markup(source: &str, tag_start: usize) -> Result<Markup, PrefsParseError> {
    const SKIPPED: [(&str, &str); 4] = [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>"), ("<!", ">")];

    let rest = &source[tag_start..];
    for (opener, closer) in SKIPPED {
        if rest.starts_with(opener) {
            let Some(found) = find_from(source, tag_start + opener.len(), closer) else {
                return Err(PrefsParseError::UnexpectedEnd { offset: ByteOffset(tag_start) });
            };

            return Ok(Markup::Skip { end: found + closer.len() });
        }
    }

    if rest.starts_with("</") {
        let Some(close) = find_from(source, tag_start, ">") else {
            return Err(PrefsParseError::UnexpectedEnd { offset: ByteOffset(tag_start) });
        };

        let name_start = tag_start + 2;
        let name_end = name_start + source[name_start..close].trim_end_matches(is_xml_space).len();
        if !is_valid_name(source.as_bytes(), name_start, name_end) {
            return Err(PrefsParseError::MalformedTag { offset: ByteOffset(tag_start) });
        }

        return Ok(Markup::Close { name: Span { start: name_start, end: name_end }, end: close + 1 });
    }

    read_open_tag(source, tag_start)
}

fn read_open_tag(source: &str, tag_start: usize) -> Result<Markup, PrefsParseError> {
    let bytes = source.as_bytes();
    let malformed = PrefsParseError::MalformedTag { offset: ByteOffset(tag_start) };
    let unexpected_end = PrefsParseError::UnexpectedEnd { offset: ByteOffset(tag_start) };

    let name_start = tag_start + 1;
    let mut position = name_start;
    while position < bytes.len() && is_name_byte(bytes[position]) {
        position += 1;
    }
    if position == name_start {
        return Err(malformed);
    }

    let name = Span { start: name_start, end: position };
    let mut attributes = Vec::new();
    loop {
        let tail_start = position;
        while position < bytes.len() && is_xml_space(char::from(bytes[position])) {
            position += 1;
        }

        match bytes.get(position) {
            None => return Err(unexpected_end),
            Some(b'>') => {
                let content = Span { start: position + 1, end: position + 1 };
                let element = Element {
                    name,
                    attributes,
                    body: Body::Content { content },
                    children: Vec::new(),
                    has_markup: false,
                };

                return Ok(Markup::Open { element, end: position + 1 });
            }
            Some(b'/') => {
                if bytes.get(position + 1) != Some(&b'>') {
                    return Err(malformed);
                }

                let close = Span { start: position, end: position + 2 };
                let element = Element {
                    name,
                    attributes,
                    body: Body::SelfClosing { close },
                    children: Vec::new(),
                    has_markup: false,
                };

                return Ok(Markup::Open { element, end: position + 2 });
            }
            Some(_) if position == tail_start => return Err(malformed),
            Some(_) => {}
        }

        let attribute_start = position;
        while position < bytes.len() && is_name_byte(bytes[position]) {
            position += 1;
        }
        if position == attribute_start {
            return Err(malformed);
        }
        let attribute_name = Span { start: attribute_start, end: position };

        while position < bytes.len() && is_xml_space(char::from(bytes[position])) {
            position += 1;
        }
        if bytes.get(position) != Some(&b'=') {
            return Err(malformed);
        }
        position += 1;
        while position < bytes.len() && is_xml_space(char::from(bytes[position])) {
            position += 1;
        }

        let quote = match bytes.get(position) {
            Some(&quote @ (b'"' | b'\'')) => quote,
            Some(_) => return Err(malformed),
            None => return Err(unexpected_end),
        };
        let value_start = position + 1;
        let Some(value_end) =
            bytes[value_start..].iter().position(|&byte| byte == quote).map(|found| value_start + found)
        else {
            return Err(unexpected_end);
        };

        attributes.push(Attribute { name: attribute_name, value: Span { start: value_start, end: value_end } });
        position = value_end + 1;
    }
}

fn detect_value_padding(source: &str, elements: &[Element]) -> Span {
    for element in elements {
        let Body::Content { content } = element.body else {
            continue;
        };
        if !element.children.is_empty() {
            continue;
        }

        let text = &source[content.range()];
        let leading = text.len() - text.trim_start_matches(is_xml_space).len();
        if leading == text.len() {
            continue;
        }

        let padding = &text[..leading];
        if padding.contains(['\r', '\n']) {
            continue;
        }

        return Span { start: content.start, end: content.start + leading };
    }

    Span { start: 0, end: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One line per element: depth, name, attributes and value.
    fn outline(document: &PrefsDocument) -> String {
        let mut lines = String::new();
        let mut pending = vec![(ElementId(0), 0usize)];
        while let Some((id, depth)) = pending.pop() {
            let element = &document.elements[id.0];
            let source = &document.source;
            lines.push_str(&"  ".repeat(depth));
            lines.push_str(&source[element.name.range()]);
            for attribute in &element.attributes {
                lines.push_str(&format!(" {}={:?}", &source[attribute.name.range()], &source[attribute.value.range()]));
            }
            if element.children.is_empty() {
                let value = match element.body {
                    Body::SelfClosing { .. } => "(self-closing)".to_owned(),
                    Body::Content { content } => format!("{:?}", source[content.range()].trim_matches(is_xml_space)),
                };
                lines.push_str(&format!(" = {value}"));
            }
            lines.push('\n');

            pending.extend(element.children.iter().rev().map(|child| (*child, depth + 1)));
        }

        lines
    }

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    #[test]
    fn user_preferences_outline() {
        let document = PrefsDocument::parse(fixture("user_preferences.xml")).unwrap();

        insta::assert_snapshot!(outline(&document));
    }

    #[test]
    fn engine_config_outline() {
        let document = PrefsDocument::parse(fixture("engine_config.xml")).unwrap();

        insta::assert_snapshot!(outline(&document));
    }

    #[test]
    fn rejects_malformed_input() {
        let cases = [
            "",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "</a>",
            "text<a/>",
            "<a x></a>",
            "<a x=1></a>",
            "<a x=\"1></a>",
            "<a/x></a>",
            "<a><!-- open</a>",
        ];
        let results: Vec<String> = cases
            .iter()
            .map(|case| format!("{case:?}: {:?}", PrefsDocument::parse((*case).to_owned()).unwrap_err()))
            .collect();

        insta::assert_snapshot!(results.join("\n"));
    }
}

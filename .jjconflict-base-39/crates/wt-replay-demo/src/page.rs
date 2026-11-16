//! What the embedding page asks for through the query string.

/// Where the clip is fetched from, relative to the demo's own page.
pub const TRACK_URL: &str = "../assets/replay-demo.bin";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageOptions {
    /// `None` follows the browser's preference.
    pub theme: Option<Theme>,
}

impl PageOptions {
    /// Reads `?theme=light|dark` from a `location.search` string.
    ///
    /// A theme other than the two is read as no theme at all, which follows
    /// the browser: the page asking for something this build does not know is
    /// no reason to refuse to play.
    pub fn parse(search: &str) -> Self {
        let theme = match query_value(search, "theme") {
            Some("light") => Some(Theme::Light),
            Some("dark") => Some(Theme::Dark),
            _ => None,
        };

        Self { theme }
    }
}

/// The value of the first `key` parameter in `search`, as written.
fn query_value<'a>(search: &'a str, key: &str) -> Option<&'a str> {
    search
        .strip_prefix('?')
        .unwrap_or(search)
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_a_theme() {
        assert_eq!(PageOptions::parse("?theme=light").theme, Some(Theme::Light));
        assert_eq!(PageOptions::parse("?other=1&theme=dark").theme, Some(Theme::Dark));
    }

    #[test]
    fn an_empty_query_follows_the_browser() {
        assert_eq!(PageOptions::parse("").theme, None);
    }

    #[test]
    fn an_unknown_theme_follows_the_browser() {
        assert_eq!(PageOptions::parse("?theme=sepia").theme, None);
    }
}

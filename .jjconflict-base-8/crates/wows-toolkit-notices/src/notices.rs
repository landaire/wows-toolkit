use std::sync::LazyLock;

/// The repository's LICENSE file.
pub const PROJECT_LICENSE: &str = include_str!(concat!(env!("OUT_DIR"), "/LICENSE"));

/// The SPDX identifier of [`PROJECT_LICENSE`], as `[workspace.package] license` in Cargo.toml
/// declares it.
pub const PROJECT_LICENSE_ID: &str = "MIT";

/// THIRD_PARTY_NOTICES.txt, which scripts/update_third_party_notices.nu generates.
pub const THIRD_PARTY_NOTICES: &str = include_str!(concat!(env!("OUT_DIR"), "/THIRD_PARTY_NOTICES.txt"));

/// The line that opens and closes the title of each entry in THIRD_PARTY_NOTICES.txt. The
/// generator rejects any license text that contains it.
const SEPARATOR: &str = "================================================================================";

/// THIRD_PARTY_NOTICES.txt divided into its entries.
pub struct Notices {
    preamble: &'static str,
    entries: Vec<Notice>,
}

impl Notices {
    /// The notices compiled into this binary, divided once per process.
    pub fn embedded() -> &'static Self {
        static EMBEDDED: LazyLock<Notices> = LazyLock::new(|| Notices::parse(THIRD_PARTY_NOTICES));

        &EMBEDDED
    }

    /// Divides `text` at each title, which sits between two [`SEPARATOR`] lines.
    pub fn parse(text: &'static str) -> Self {
        let mut sections = Vec::new();
        let mut section_start = 0;
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if line.trim_end_matches(['\r', '\n']) == SEPARATOR {
                sections.push(&text[section_start..offset]);
                section_start = offset + line.len();
            }
            offset += line.len();
        }
        sections.push(&text[section_start..]);

        let mut sections = sections.into_iter();
        let preamble = sections.next().unwrap_or_default().trim();

        // The remainder alternates title, body. A title with no body after it ends the file.
        let mut entries = Vec::new();
        while let Some(title) = sections.next() {
            let body = sections.next().unwrap_or_default();
            entries.push(Notice::new(title.trim(), body.trim_matches(['\r', '\n'])));
        }

        Self { preamble, entries }
    }

    /// The text before the first entry.
    pub fn preamble(&self) -> &'static str {
        self.preamble
    }

    pub fn entries(&self) -> &[Notice] {
        &self.entries
    }

    /// The entries whose title or body contains `query`, ignoring case. An empty query matches
    /// every entry.
    pub fn matching<'a>(&'a self, query: &str) -> impl Iterator<Item = &'a Notice> + 'a {
        let query = query.trim().to_lowercase();

        self.entries.iter().filter(move |entry| entry.folded.contains(&query))
    }
}

/// One titled entry of THIRD_PARTY_NOTICES.txt.
pub struct Notice {
    title: &'static str,
    body: &'static str,

    /// Title and body in lower case, which [`Notices::matching`] searches.
    folded: String,
}

impl Notice {
    fn new(title: &'static str, body: &'static str) -> Self {
        let folded = format!("{title}\n{body}").to_lowercase();

        Self { title, body, folded }
    }

    pub fn title(&self) -> &'static str {
        self.title
    }

    pub fn body(&self) -> &'static str {
        self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_notices_name_known_crates() {
        let notices = Notices::embedded();

        assert!(notices.preamble().contains("WoWs Toolkit"));
        assert!(notices.entries().len() > 10);
        for name in ["serde ", "tokio ", "zgui ", "egui ", "image "] {
            assert!(
                notices.entries().iter().any(|entry| entry.body().contains(name)),
                "no entry names the crate {name:?}"
            );
        }
        assert!(notices.matching("Apache License").next().is_some());
    }

    #[test]
    fn project_license_is_the_mit_text() {
        assert!(PROJECT_LICENSE.contains("Permission is hereby granted, free of charge"));
    }

    #[test]
    fn parse_divides_at_titles() {
        let text = concat!(
            "Preamble\n",
            "\n",
            "================================================================================\n",
            "First\n",
            "================================================================================\n",
            "\n",
            "Body one\n",
            "\n",
            "================================================================================\r\n",
            "Second\r\n",
            "================================================================================\r\n",
            "Body two\r\n",
        );

        let notices = Notices::parse(text);

        assert_eq!(notices.preamble(), "Preamble");
        let entries: Vec<(&str, &str)> = notices.entries().iter().map(|entry| (entry.title(), entry.body())).collect();
        assert_eq!(entries, [("First", "Body one"), ("Second", "Body two")]);
        let matched: Vec<&str> = notices.matching("BODY TWO").map(Notice::title).collect();
        assert_eq!(matched, ["Second"]);
        assert_eq!(notices.matching("").count(), 2);
    }
}

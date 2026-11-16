use std::collections::BTreeMap;
use std::path::Path;

use rootcause::prelude::*;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AppId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DepotId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestId(pub String);

/// The Steam app every depot here belongs to.
pub const WOWS_APP_ID: AppId = AppId(552990);

/// Holds `bin/<build>/`, so it is the depot a build number can be read out of.
pub const CLIENT_DEPOT_ID: DepotId = DepotId(552993);

pub const CONTENT_DEPOT_ID: DepotId = DepotId(552991);

pub const LOCALIZATION_DEPOT_ID: DepotId = DepotId(552994);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotManifest {
    pub depot_id: DepotId,
    pub manifest_id: ManifestId,
}

impl DepotManifest {
    pub fn new(depot_id: DepotId, manifest_id: ManifestId) -> Self {
        Self { depot_id, manifest_id }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameVersionManifest {
    #[serde(default)]
    pub versions: BTreeMap<String, GameVersionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameVersionEntry {
    pub version: String,
    pub client: DepotManifest,
    pub content: Option<DepotManifest>,
    pub localization: Option<DepotManifest>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlatGameVersionEntry {
    version: String,
    client_depot_id: DepotId,
    client_manifest_id: ManifestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_depot_id: Option<DepotId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_manifest_id: Option<ManifestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    localization_depot_id: Option<DepotId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    localization_manifest_id: Option<ManifestId>,
}

impl<'de> Deserialize<'de> for GameVersionEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let flat = FlatGameVersionEntry::deserialize(deserializer)?;
        let content = depot_manifest_from_pair(flat.content_depot_id, flat.content_manifest_id, "content")?;
        let localization =
            depot_manifest_from_pair(flat.localization_depot_id, flat.localization_manifest_id, "localization")?;

        Ok(Self {
            version: flat.version,
            client: DepotManifest::new(flat.client_depot_id, flat.client_manifest_id),
            content,
            localization,
        })
    }
}

impl Serialize for GameVersionEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (content_depot_id, content_manifest_id) = match &self.content {
            Some(manifest) => (Some(manifest.depot_id), Some(manifest.manifest_id.clone())),
            None => (None, None),
        };
        let (localization_depot_id, localization_manifest_id) = match &self.localization {
            Some(manifest) => (Some(manifest.depot_id), Some(manifest.manifest_id.clone())),
            None => (None, None),
        };

        FlatGameVersionEntry {
            version: self.version.clone(),
            client_depot_id: self.client.depot_id,
            client_manifest_id: self.client.manifest_id.clone(),
            content_depot_id,
            content_manifest_id,
            localization_depot_id,
            localization_manifest_id,
        }
        .serialize(serializer)
    }
}

fn depot_manifest_from_pair<E>(
    depot_id: Option<DepotId>,
    manifest_id: Option<ManifestId>,
    depot_name: &str,
) -> Result<Option<DepotManifest>, E>
where
    E: serde::de::Error,
{
    match (depot_id, manifest_id) {
        (Some(depot_id), Some(manifest_id)) => Ok(Some(DepotManifest::new(depot_id, manifest_id))),
        (None, None) => Ok(None),
        (Some(_), None) => Err(E::custom(format!("{depot_name}_manifest_id is required"))),
        (None, Some(_)) => Err(E::custom(format!("{depot_name}_depot_id is required"))),
    }
}

impl GameVersionManifest {
    /// Returns the highest build number in the manifest.
    pub fn latest_build(&self) -> Option<u32> {
        self.versions.keys().filter_map(|k| k.parse::<u32>().ok()).max()
    }

    /// Look up a build number by version string (supports shorthand like "15.1").
    /// When multiple builds match, returns the highest.
    pub fn find_by_version(&self, query: &str) -> Option<u32> {
        let mut matched: Vec<u32> = self
            .versions
            .iter()
            .filter(|(_, entry)| version_matches(&entry.version, query))
            .filter_map(|(k, _)| k.parse::<u32>().ok())
            .collect();
        matched.sort();
        matched.last().copied()
    }

    /// Get a manifest entry by build number.
    pub fn get(&self, build: u32) -> Option<&GameVersionEntry> {
        self.versions.get(&build.to_string())
    }
}

/// Check if a full version string matches a possibly-shorthand query.
/// "15.1.0" matches "15.1", "15.1.0", and "15".
pub fn version_matches(full: &str, query: &str) -> bool {
    let full_parts: Vec<&str> = full.split('.').collect();
    let query_parts: Vec<&str> = query.split('.').collect();

    if query_parts.len() > full_parts.len() {
        return false;
    }

    full_parts.iter().zip(query_parts.iter()).all(|(f, q)| f == q)
}

/// Render one build's table exactly as the file spells it, by routing through
/// the same flat serializer the file is parsed with.
fn render_entry(build: u32, entry: &GameVersionEntry) -> Result<String, Report> {
    let versions = BTreeMap::from([(build.to_string(), entry.clone())]);
    let rendered = toml::to_string(&GameVersionManifest { versions })
        .attach_with(|| format!("Failed to serialize the manifest entry for build {build}"))?;
    Ok(rendered.trim_end().to_string())
}

/// The build a table header names, for any spelling TOML accepts: a quoted
/// key, padding inside the brackets, or a trailing comment. Matching only the
/// one spelling this crate emits would read a hand-written
/// `[versions.13015811] # from SteamDB` as a different table and duplicate it.
fn versions_table_build(line: &str) -> Option<u32> {
    let trimmed = line.trim_start();
    let inner = trimmed.strip_prefix('[')?.split(']').next()?;
    let (table, key) = inner.split_once('.')?;
    if table.trim() != "versions" {
        return None;
    }
    let key = key.trim();
    key.strip_prefix('"').and_then(|k| k.strip_suffix('"')).unwrap_or(key).parse().ok()
}

/// Whether a line opens any table, which is where a build's block ends.
fn is_table_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Walk `end` back over the comments introducing the table at `end`, without
/// crossing into `floor`.
///
/// Only directly adjacent comments belong to the table. A blank line ends the
/// run, which is what separates the file's own header block from the first
/// build rather than attaching it to one.
fn start_of_attached_comments(lines: &[&str], end: usize, floor: usize) -> usize {
    let mut start = end;
    while start > floor && is_comment(lines[start - 1]) {
        start -= 1;
    }
    start
}

/// Splice `entry` into the manifest text, replacing the build's existing table
/// or inserting it in build-descending order.
///
/// Editing text rather than re-serializing the map is what preserves the header
/// documenting each depot and the hand-curated newest-first order; a
/// `BTreeMap<String, _>` round trip would drop the former and sort the latter
/// lexicographically. Output is LF regardless of what came in.
pub fn upsert_entry(source: &str, build: u32, entry: &GameVersionEntry) -> Result<String, Report> {
    let block: Vec<String> = render_entry(build, entry)?.lines().map(str::to_string).collect();
    let lines: Vec<&str> = source.lines().collect();

    let existing = lines.iter().position(|line| versions_table_build(line) == Some(build));
    let (replace_from, resume_at) = match existing {
        Some(header) => {
            let next = (header + 1..lines.len()).find(|&i| is_table_header(lines[i])).unwrap_or(lines.len());
            (header, start_of_attached_comments(&lines, next, header + 1))
        }
        // Newest first, so the entry goes above the first build lower than it,
        // and after any comment that introduces that one.
        None => {
            let successor = lines
                .iter()
                .position(|line| versions_table_build(line).is_some_and(|other| other < build))
                .map(|header| start_of_attached_comments(&lines, header, 0))
                .unwrap_or(lines.len());
            (successor, successor)
        }
    };

    let mut out: Vec<String> = lines[..replace_from].iter().map(|line| line.to_string()).collect();
    // An appended entry needs separating from whatever it follows; a spliced
    // one keeps the blank line already sitting after it.
    if existing.is_none() && out.last().is_some_and(|line| !line.trim().is_empty()) {
        out.push(String::new());
    }
    out.extend(block);
    if resume_at < lines.len() {
        out.push(String::new());
        out.extend(lines[resume_at..].iter().map(|line| line.to_string()));
    }

    while out.last().is_some_and(|line| line.trim().is_empty()) {
        out.pop();
    }
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// Confirm the spliced text still parses and says what it was meant to say.
///
/// The splice is textual, so a source shaped in a way it does not anticipate
/// must fail loudly here. A manifest that no longer parses blocks every other
/// subcommand, since each loads it before doing anything.
fn validate_upsert(source: &str, updated: &str, build: u32, entry: &GameVersionEntry) -> Result<(), Report> {
    let reparsed: GameVersionManifest = toml::from_str(updated)
        .map_err(|e| rootcause::report!("Refusing to write a manifest that no longer parses: {e}"))?;

    match reparsed.get(build) {
        Some(written) if written == entry => {}
        Some(_) => bail!("Refusing to write: the entry for build {build} did not survive the edit intact"),
        None => bail!("Refusing to write: build {build} is missing from the result"),
    }

    // A source that never parsed is being repaired, not preserved.
    let Ok(before) = toml::from_str::<GameVersionManifest>(source) else {
        return Ok(());
    };
    for (key, was) in &before.versions {
        if key == &build.to_string() {
            continue;
        }
        match reparsed.versions.get(key) {
            Some(now) if now == was => {}
            _ => bail!("Refusing to write: editing build {build} would have altered build {key}"),
        }
    }
    Ok(())
}

/// Write one build's pin into the manifest file, creating it if absent.
pub fn save_entry(path: &Path, build: u32, entry: &GameVersionEntry) -> Result<(), Report> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(rootcause::report!("Failed to read {}: {e}", path.display())),
    };
    let updated = upsert_entry(&source, build, entry)?;
    validate_upsert(&source, &updated, build, entry).attach_with(|| format!("Failed to update {}", path.display()))?;
    if updated == source {
        return Ok(());
    }

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, &updated).attach_with(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).attach_with(|| format!("Failed to rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn load_manifest(path: &Path) -> Result<GameVersionManifest, Report> {
    if !path.exists() {
        return Ok(GameVersionManifest { versions: BTreeMap::new() });
    }
    let content = std::fs::read_to_string(path).attach_with(|| format!("Failed to read {}", path.display()))?;
    let manifest: GameVersionManifest =
        toml::from_str(&content).map_err(|e| rootcause::report!("Failed to parse {}: {e}", path.display()))?;
    Ok(manifest)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn version_matches_exact() {
        assert!(version_matches("15.1.0", "15.1.0"));
    }

    #[test]
    fn version_matches_shorthand_two() {
        assert!(version_matches("15.1.0", "15.1"));
    }

    #[test]
    fn version_matches_shorthand_one() {
        assert!(version_matches("15.1.0", "15"));
    }

    #[test]
    fn version_no_match() {
        assert!(!version_matches("15.1.0", "14.1"));
    }

    #[test]
    fn version_query_longer() {
        assert!(!version_matches("15.1", "15.1.0"));
    }

    #[test]
    fn parses_split_depot_manifests() {
        let manifest: GameVersionManifest = toml::from_str(
            r#"
            [versions.13015711]
            version = "15.7.0"
            client_depot_id = 552993
            client_manifest_id = "client"
            content_depot_id = 552991
            content_manifest_id = "content"
            localization_depot_id = 552994
            localization_manifest_id = "localization"
            "#,
        )
        .unwrap();

        let entry = manifest.get(13015711).unwrap();
        assert_eq!(entry.client, DepotManifest::new(DepotId(552993), ManifestId("client".into())));
        assert_eq!(entry.content, Some(DepotManifest::new(DepotId(552991), ManifestId("content".into()))));
        assert_eq!(entry.localization, Some(DepotManifest::new(DepotId(552994), ManifestId("localization".into()))));
    }

    #[test]
    fn parses_client_only_depot_manifest() {
        let manifest: GameVersionManifest = toml::from_str(
            r#"
            [versions.13015711]
            version = "15.7.0"
            client_depot_id = 552993
            client_manifest_id = "client"
            "#,
        )
        .unwrap();

        let entry = manifest.get(13015711).unwrap();
        assert_eq!(entry.client, DepotManifest::new(DepotId(552993), ManifestId("client".into())));
        assert_eq!(entry.content, None);
        assert_eq!(entry.localization, None);
    }

    #[test]
    fn rejects_each_incomplete_optional_depot_manifest() {
        for (optional_field, missing_field) in [
            ("content_depot_id = 552991", "content_manifest_id"),
            ("content_manifest_id = \"content\"", "content_depot_id"),
            ("localization_depot_id = 552994", "localization_manifest_id"),
            ("localization_manifest_id = \"localization\"", "localization_depot_id"),
        ] {
            let source = format!(
                r#"
                [versions.13015711]
                version = "15.7.0"
                client_depot_id = 552993
                client_manifest_id = "client"
                {optional_field}
                "#
            );

            let error = toml::from_str::<GameVersionManifest>(&source).unwrap_err();

            assert!(error.to_string().contains(missing_field));
        }
    }

    #[test]
    fn rejects_misspelled_optional_depot_fields() {
        let error = toml::from_str::<GameVersionManifest>(
            r#"
            [versions.13015711]
            version = "15.7.0"
            client_depot_id = 552993
            client_manifest_id = "client"
            localisation_depot_id = 552994
            localisation_manifest_id = "localization"
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("localisation_depot_id"));
    }

    #[test]
    fn serializes_split_depot_manifests_to_flat_schema() {
        let mut versions = BTreeMap::new();
        versions.insert(
            "13015711".to_string(),
            GameVersionEntry {
                version: "15.7.0".to_string(),
                client: DepotManifest::new(DepotId(552993), ManifestId("client".to_string())),
                content: Some(DepotManifest::new(DepotId(552991), ManifestId("content".to_string()))),
                localization: Some(DepotManifest::new(DepotId(552994), ManifestId("localization".to_string()))),
            },
        );

        let serialized = toml::to_string(&GameVersionManifest { versions }).unwrap();

        assert!(serialized.contains("client_depot_id = 552993"));
        assert!(serialized.contains("client_manifest_id = \"client\""));
        assert!(serialized.contains("content_depot_id = 552991"));
        assert!(serialized.contains("content_manifest_id = \"content\""));
        assert!(serialized.contains("localization_depot_id = 552994"));
        assert!(serialized.contains("localization_manifest_id = \"localization\""));
        assert!(!serialized.contains("[versions.13015711.client]"));
    }

    #[test]
    fn round_trips_all_optional_depot_shapes() {
        let original: GameVersionManifest = toml::from_str(
            r#"
            [versions.13015711]
            version = "15.7.0"
            client_depot_id = 552993
            client_manifest_id = "client"
            content_depot_id = 552991
            content_manifest_id = "content"
            localization_depot_id = 552994
            localization_manifest_id = "localization"

            [versions.2466969]
            version = "9.3.1"
            client_depot_id = 552993
            client_manifest_id = "728775844461842480"
            content_depot_id = 552991
            content_manifest_id = "1770346061135014060"

            [versions.1000000]
            version = "0.1.0"
            client_depot_id = 552993
            client_manifest_id = "client-only"
            "#,
        )
        .unwrap();

        let serialized = toml::to_string(&original).unwrap();
        let reparsed: GameVersionManifest = toml::from_str(&serialized).unwrap();

        assert_eq!(reparsed.versions, original.versions);
        let historical = reparsed.get(2466969).unwrap();
        assert_eq!(
            historical.content,
            Some(DepotManifest::new(DepotId(552991), ManifestId("1770346061135014060".to_string())))
        );
        assert_eq!(historical.localization, None);
    }

    #[test]
    fn find_by_version_picks_highest() {
        let mut versions = BTreeMap::new();
        versions.insert(
            "11791718".to_string(),
            GameVersionEntry {
                version: "15.0.0".to_string(),
                client: DepotManifest::new(DepotId(552991), ManifestId("aaa".to_string())),
                content: None,
                localization: None,
            },
        );
        versions.insert(
            "11965230".to_string(),
            GameVersionEntry {
                version: "15.1.0".to_string(),
                client: DepotManifest::new(DepotId(552991), ManifestId("bbb".to_string())),
                content: None,
                localization: None,
            },
        );
        let manifest = GameVersionManifest { versions };

        assert_eq!(manifest.find_by_version("15"), Some(11965230));
        assert_eq!(manifest.find_by_version("15.1"), Some(11965230));
        assert_eq!(manifest.find_by_version("15.0"), Some(11791718));
        assert_eq!(manifest.find_by_version("14"), None);
    }
}

#[cfg(test)]
mod upsert_tests {
    use super::*;

    /// A trimmed copy of the real file: a header comment block, then entries
    /// newest-first.
    const EXISTING: &str = r#"# Known World of Warships game versions.
#
# Steam App ID: 552990

[versions.13015811]
version = "15.7.0"
client_depot_id = 552993
client_manifest_id = "old-client"
content_depot_id = 552991
content_manifest_id = "old-content"

[versions.12830008]
version = "15.6.0"
client_depot_id = 552993
client_manifest_id = "client-15.6.0"
"#;

    fn entry(version: &str, client: &str) -> GameVersionEntry {
        GameVersionEntry {
            version: version.to_string(),
            client: DepotManifest::new(CLIENT_DEPOT_ID, ManifestId(client.to_string())),
            content: Some(DepotManifest::new(CONTENT_DEPOT_ID, ManifestId("new-content".to_string()))),
            localization: Some(DepotManifest::new(LOCALIZATION_DEPOT_ID, ManifestId("new-loc".to_string()))),
        }
    }

    fn parse(source: &str) -> GameVersionManifest {
        toml::from_str(source).unwrap()
    }

    /// The file is hand-curated newest-first and its header documents the
    /// depots. Re-serializing the whole map would reorder it lexicographically
    /// and drop every comment, so a new build is spliced in as text.
    #[test]
    fn a_new_build_lands_above_the_existing_entries_and_keeps_the_header() {
        let updated = upsert_entry(EXISTING, 13_100_000, &entry("15.8.0", "new-client")).unwrap();

        assert!(updated.starts_with("# Known World of Warships game versions.\n"), "{updated}");
        let order: Vec<&str> = updated.lines().filter(|l| l.starts_with("[versions.")).collect();
        assert_eq!(order, ["[versions.13100000]", "[versions.13015811]", "[versions.12830008]"]);

        let parsed = parse(&updated);
        assert_eq!(parsed.get(13_100_000).unwrap(), &entry("15.8.0", "new-client"));
        assert_eq!(parsed.get(12_830_008).unwrap().client.manifest_id, ManifestId("client-15.6.0".to_string()));
    }

    /// Refreshing a build that is already pinned must not move it or duplicate
    /// its table.
    #[test]
    fn an_existing_build_is_replaced_where_it_already_sits() {
        let updated = upsert_entry(EXISTING, 13_015_811, &entry("15.7.0", "refreshed-client")).unwrap();

        let order: Vec<&str> = updated.lines().filter(|l| l.starts_with("[versions.")).collect();
        assert_eq!(order, ["[versions.13015811]", "[versions.12830008]"]);
        assert!(!updated.contains("old-client"), "{updated}");

        let parsed = parse(&updated);
        assert_eq!(parsed.versions.len(), 2);
        assert_eq!(parsed.get(13_015_811).unwrap(), &entry("15.7.0", "refreshed-client"));
    }

    /// The last table has no following one to separate it from, which is the
    /// case a blank-line-per-block implementation gets wrong.
    #[test]
    fn replacing_the_last_entry_leaves_a_single_trailing_newline() {
        let updated = upsert_entry(EXISTING, 12_830_008, &entry("15.6.0", "refreshed-oldest")).unwrap();

        assert!(updated.ends_with("localization_manifest_id = \"new-loc\"\n"), "{updated:?}");
        assert_eq!(parse(&updated).versions.len(), 2);
    }

    /// Re-running an update that changes nothing must not churn the file, or
    /// every dump would show up as a diff.
    #[test]
    fn rewriting_an_unchanged_entry_reproduces_the_file_byte_for_byte() {
        let once = upsert_entry(EXISTING, 13_100_000, &entry("15.8.0", "new-client")).unwrap();
        let twice = upsert_entry(&once, 13_100_000, &entry("15.8.0", "new-client")).unwrap();

        assert_eq!(once, twice);
    }

    /// Mixed endings turn a rebase into a whole-file conflict, so the writer
    /// emits LF whatever it was handed.
    #[test]
    fn crlf_input_is_written_back_as_lf() {
        let crlf = EXISTING.replace('\n', "\r\n");

        let updated = upsert_entry(&crlf, 13_100_000, &entry("15.8.0", "new-client")).unwrap();

        assert!(!updated.contains('\r'), "{updated:?}");
        assert_eq!(parse(&updated).versions.len(), 3);
    }

    #[test]
    fn a_file_with_only_a_header_gets_the_entry_appended() {
        let updated = upsert_entry("# Known versions.\n", 13_100_000, &entry("15.8.0", "new-client")).unwrap();

        assert!(updated.starts_with("# Known versions.\n\n[versions.13100000]\n"), "{updated:?}");
        assert_eq!(parse(&updated).versions.len(), 1);
    }

    #[test]
    fn an_empty_file_gets_the_entry_alone() {
        let updated = upsert_entry("", 13_100_000, &entry("15.8.0", "new-client")).unwrap();

        assert!(updated.starts_with("[versions.13100000]\n"), "{updated:?}");
        assert_eq!(parse(&updated).versions.len(), 1);
    }

    /// A client-only build is the 0.6.x-era shape; its optional depots must not
    /// render as empty keys.
    #[test]
    fn a_client_only_entry_renders_without_the_optional_depots() {
        let client_only = GameVersionEntry {
            version: "0.6.13".to_string(),
            client: DepotManifest::new(CLIENT_DEPOT_ID, ManifestId("client".to_string())),
            content: None,
            localization: None,
        };

        let updated = upsert_entry(EXISTING, 296_659, &client_only).unwrap();

        assert!(!updated.contains("content_depot_id = 552991\ncontent_manifest_id = \"\""), "{updated}");
        assert_eq!(parse(&updated).get(296_659).unwrap(), &client_only);
    }
}

/// Regressions for defects an adversarial review reproduced against the first
/// text-splicing implementation.
#[cfg(test)]
mod upsert_robustness_tests {
    use super::*;

    fn entry(version: &str, client: &str) -> GameVersionEntry {
        GameVersionEntry {
            version: version.to_string(),
            client: DepotManifest::new(CLIENT_DEPOT_ID, ManifestId(client.to_string())),
            content: None,
            localization: None,
        }
    }

    fn save(dir: &Path, source: &str, build: u32, entry: &GameVersionEntry) -> Result<String, Report> {
        let path = dir.join("game_versions.toml");
        std::fs::write(&path, source).unwrap();
        save_entry(&path, build, entry)?;
        Ok(std::fs::read_to_string(&path).unwrap())
    }

    /// A hand-written header carrying a trailing comment, a quoted key, or
    /// padding is the same table. Matching one exact spelling wrote a second
    /// table for the same build, and the duplicate key then failed to parse,
    /// which every subcommand loads the manifest before doing.
    #[test]
    fn every_toml_spelling_of_a_header_is_the_same_build() {
        for header in [
            "[versions.13015811] # from SteamDB 2026-09-01",
            "[versions.\"13015811\"]",
            "[ versions.13015811 ]",
            "[versions.13015811]",
        ] {
            assert_eq!(versions_table_build(header), Some(13_015_811), "{header}");

            let source =
                format!("{header}\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"old\"\n");
            let updated = upsert_entry(&source, 13_015_811, &entry("15.7.0", "new")).unwrap();

            let parsed: GameVersionManifest = toml::from_str(&updated)
                .unwrap_or_else(|e| panic!("{header} produced unparseable output: {e}\n{updated}"));
            assert_eq!(parsed.versions.len(), 1, "{header} duplicated the table:\n{updated}");
            assert_eq!(parsed.get(13_015_811).unwrap().client.manifest_id, ManifestId("new".to_string()));
        }
    }

    #[test]
    fn lines_that_are_not_versions_tables_name_no_build() {
        for line in
            ["[other.13015811]", "[versions]", "version = \"15.7.0\"", "# [versions.13015811]", "[versions.abc]"]
        {
            assert_eq!(versions_table_build(line), None, "{line}");
        }
    }

    /// A comment sitting above the next table documents that table. Ending the
    /// replaced block at the next header swallowed it, which is exactly what
    /// text splicing exists to avoid.
    #[test]
    fn replacing_an_entry_keeps_the_comment_introducing_the_next_one() {
        let source = "[versions.13015811]\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"old\"\n\n# this build shipped the CV rework, do not re-dump\n[versions.12830008]\nversion = \"15.6.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"older\"\n";

        let updated = upsert_entry(source, 13_015_811, &entry("15.7.0", "new")).unwrap();

        assert!(updated.contains("# this build shipped the CV rework, do not re-dump"), "{updated}");
        let parsed: GameVersionManifest = toml::from_str(&updated).unwrap();
        assert_eq!(parsed.versions.len(), 2);
    }

    /// A new entry belongs above the comment introducing the entry it precedes,
    /// or that comment ends up documenting the wrong build.
    #[test]
    fn a_new_entry_lands_above_the_comment_that_introduces_its_successor() {
        let source = "# newest release\n[versions.13015811]\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"old\"\n";

        let updated = upsert_entry(source, 13_187_581, &entry("15.8.0", "new")).unwrap();

        let newest = updated.find("[versions.13187581]").unwrap();
        let comment = updated.find("# newest release").unwrap();
        assert!(newest < comment, "the comment must stay with the build it introduces:\n{updated}");
        assert!(comment < updated.find("[versions.13015811]").unwrap());
    }

    /// Ordering is by build, not by arrival, so back-filling an old build does
    /// not put 0.6.13 above 15.7.0.
    #[test]
    fn an_older_build_is_spliced_in_descending_order() {
        let source = "[versions.13015811]\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"a\"\n\n[versions.11965230]\nversion = \"15.1.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"b\"\n";

        let updated = upsert_entry(source, 12_830_008, &entry("15.6.0", "c")).unwrap();

        let order: Vec<&str> = updated.lines().filter(|l| l.starts_with("[versions.")).collect();
        assert_eq!(order, ["[versions.13015811]", "[versions.12830008]", "[versions.11965230]"]);

        let oldest = upsert_entry(&updated, 296_659, &entry("0.6.13", "d")).unwrap();
        let order: Vec<&str> = oldest.lines().filter(|l| l.starts_with("[versions.")).collect();
        assert_eq!(order.last(), Some(&"[versions.296659]"), "{oldest}");
        assert_eq!(toml::from_str::<GameVersionManifest>(&oldest).unwrap().versions.len(), 4);
    }

    /// A shape the splice cannot handle must fail loudly. Landing it would take
    /// out every other subcommand, since each parses this file first.
    #[test]
    fn a_write_that_would_break_the_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        // A multi-line string can hold a line the splice reads as a table.
        let source = "note = \"\"\"\n[versions.13015811]\n\"\"\"\n\n[versions.13015811]\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"old\"\n";

        let err = save(dir.path(), source, 13_015_811, &entry("15.7.0", "new")).unwrap_err();

        assert!(err.to_string().contains("Refusing to write"), "{err}");
        // The file on disk is untouched.
        assert_eq!(std::fs::read_to_string(dir.path().join("game_versions.toml")).unwrap(), source);
    }

    /// Editing one build must never disturb another.
    #[test]
    fn a_write_that_would_alter_a_different_build_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source =
            "[versions.13015811]\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"keep\"\n";

        let updated = save(dir.path(), source, 13_187_581, &entry("15.8.0", "new")).unwrap();

        let parsed: GameVersionManifest = toml::from_str(&updated).unwrap();
        assert_eq!(parsed.get(13_015_811).unwrap().client.manifest_id, ManifestId("keep".to_string()));
        assert_eq!(parsed.versions.len(), 2);
    }

    /// Tables that are not builds must survive an edit.
    #[test]
    fn an_unrelated_table_after_the_versions_survives() {
        let source = "[versions.13015811]\nversion = \"15.7.0\"\nclient_depot_id = 552993\nclient_manifest_id = \"old\"\n\n[other]\nkeep = true\n";

        let updated = upsert_entry(source, 13_015_811, &entry("15.7.0", "new")).unwrap();

        assert!(updated.contains("[other]") && updated.contains("keep = true"), "{updated}");
        let value: toml::Value = toml::from_str(&updated).unwrap();
        assert_eq!(value.get("other").and_then(|o| o.get("keep")), Some(&toml::Value::Boolean(true)));
    }
}

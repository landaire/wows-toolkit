//! Support information copied from either desktop front end.

use std::path::Path;
use std::sync::Mutex;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ConstantsFetchStatus {
    #[default]
    NotChecked,
    Succeeded,
    Failed(String),
}

static CONSTANTS_FETCH_STATUS: Mutex<ConstantsFetchStatus> = Mutex::new(ConstantsFetchStatus::NotChecked);

pub fn record_constants_fetch(status: ConstantsFetchStatus) {
    *CONSTANTS_FETCH_STATUS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = status;
}

pub fn constants_fetch_status() -> ConstantsFetchStatus {
    CONSTANTS_FETCH_STATUS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BugReportInfo {
    pub constants_fetch: ConstantsFetchStatus,
    pub application_version: String,
    pub loaded_game_version: Option<String>,
    pub downloaded_constants: Option<String>,
    pub constants_commit: Option<String>,
    pub recent_crash: Option<String>,
}

impl BugReportInfo {
    pub fn discord_code_block(&self) -> String {
        let constants_fetch = match &self.constants_fetch {
            ConstantsFetchStatus::NotChecked => "not checked".to_owned(),
            ConstantsFetchStatus::Succeeded => "succeeded".to_owned(),
            ConstantsFetchStatus::Failed(error) => format!("failed: {}", safe(error)),
        };
        let loaded_game_version = self.loaded_game_version.as_deref().map(safe).unwrap_or_else(|| "none".to_owned());
        let downloaded_constants = self.downloaded_constants.as_deref().map(safe).unwrap_or_else(|| "none".to_owned());
        let constants_commit = self.constants_commit.as_deref().map(safe).unwrap_or_else(|| "none".to_owned());
        let recent_crash = self.recent_crash.as_deref().map(safe).unwrap_or_else(|| "none".to_owned());

        format!(
            "```\nConstants fetch: {constants_fetch}\nApplication version: {}\nLatest loaded game version: {loaded_game_version}\nLatest downloaded constants.json: {downloaded_constants}\nLatest constants check commit: {constants_commit}\nRecent crash:\n{recent_crash}\n```",
            safe(&self.application_version),
        )
    }
}

pub fn full_game_version(version: wowsunpack::data::Version) -> String {
    match version.build_number() {
        Some(build) => format!("{}.{}.{}.{}", version.major, version.minor, version.patch, build),
        None => format!("{}.{}.{}", version.major, version.minor, version.patch),
    }
}

#[derive(serde::Deserialize)]
struct ConstantsMetadata {
    #[serde(rename = "VERSION")]
    version: ConstantsVersionMetadata,
}

#[derive(serde::Deserialize)]
struct ConstantsVersionMetadata {
    #[serde(rename = "VERSION")]
    name: String,
    #[serde(rename = "PATCH")]
    patch: f64,
    #[serde(rename = "BUILD")]
    build: Option<u32>,
}

pub fn latest_downloaded_constants(storage_dir: Option<&Path>) -> Option<String> {
    let directory = storage_dir?;
    let mut latest: Option<(wowsunpack::data::Version, String)> = None;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        if !name.starts_with("constants_") || !name.ends_with(".json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(metadata) = serde_json::from_slice::<ConstantsMetadata>(&bytes) else { continue };
        let declared = metadata.version;
        if declared.patch.fract() != 0.0 || declared.patch < 0.0 || declared.patch > u32::MAX as f64 {
            continue;
        }
        let patch = declared.patch as u32;
        let Some(version) = wowsunpack::data::Version::from_constants_block(&declared.name, patch, declared.build)
        else {
            continue;
        };
        let summary = full_game_version(version);
        if latest.as_ref().is_none_or(|(held, _)| version > *held) {
            latest = Some((version, summary));
        }
    }

    latest.map(|(_, summary)| summary)
}

fn safe(value: &str) -> String {
    value.trim().replace("```", "'''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_report_contains_every_field_in_one_code_block() {
        let report = BugReportInfo {
            constants_fetch: ConstantsFetchStatus::Failed("network unavailable".to_owned()),
            application_version: "1.0.3".to_owned(),
            loaded_game_version: Some("15.10.0.13533378".to_owned()),
            downloaded_constants: Some("15.10.0.13533378".to_owned()),
            constants_commit: Some("abc".to_owned()),
            recent_crash: Some("panic at example.rs:1".to_owned()),
        }
        .discord_code_block();

        assert_eq!(report.matches("```").count(), 2);
        assert!(report.contains("Constants fetch: failed: network unavailable"));
        assert!(report.contains("Application version: 1.0.3"));
        assert!(report.contains("Latest loaded game version: 15.10.0.13533378"));
        assert!(report.contains("Latest downloaded constants.json: 15.10.0.13533378"));
        assert!(report.contains("Latest constants check commit: abc"));
        assert!(report.contains("Recent crash:\npanic at example.rs:1"));
    }

    #[test]
    fn nested_fences_cannot_end_the_discord_block() {
        let report = BugReportInfo {
            constants_fetch: ConstantsFetchStatus::Succeeded,
            application_version: "1.0.3".to_owned(),
            loaded_game_version: None,
            downloaded_constants: None,
            constants_commit: None,
            recent_crash: Some("```panic```".to_owned()),
        }
        .discord_code_block();

        assert_eq!(report.matches("```").count(), 2);
        assert!(report.contains("'''panic'''"));
    }
}

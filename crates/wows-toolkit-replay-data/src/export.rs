//! Writing a match out, as the Export menu and the auto-export setting do.
//!
//! The document is `wows_toolkit_viewmodel::replay_export`, the one the egui
//! app writes, in the same three formats.

use std::path::Path;
use std::path::PathBuf;

use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::replay_export::FlattenedVehicle;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;

/// The formats the Export menu offers, in its order.
pub const MENU_FORMATS: [ReplayExportFormat; 3] =
    [ReplayExportFormat::Json, ReplayExportFormat::Cbor, ReplayExportFormat::Csv];

/// Whether a finished parse writes itself out, and where.
///
/// A directory that is not there is not one: the setting is read when a
/// battle is written, so a directory removed since then turns the writing off
/// rather than failing once per battle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AutoExport {
    #[default]
    Off,
    To {
        directory: PathBuf,
        format: ReplayExportFormat,
    },
}

impl AutoExport {
    /// Reads the setting, refusing a directory that is not one.
    pub fn from_settings(settings: &ReplaySettings) -> Self {
        if !settings.auto_export_data {
            return Self::Off;
        }

        let directory = PathBuf::from(&settings.auto_export_path);
        if !directory.is_dir() {
            return Self::Off;
        }

        Self::To { directory, format: settings.auto_export_format }
    }
}

/// Why a match was not written.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("could not create {path}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not write {path}: {source}")]
    Cbor {
        path: PathBuf,
        #[source]
        source: ciborium::ser::Error<std::io::Error>,
    },
    #[error("could not write {path}: {source}")]
    Csv {
        path: PathBuf,
        #[source]
        source: csv::Error,
    },
}

/// The Export menu's label for `format`.
pub fn menu_label(format: ReplayExportFormat) -> String {
    match format {
        ReplayExportFormat::Json => rust_i18n::t!("ui.replay.export_results_json").into_owned(),
        ReplayExportFormat::Cbor => rust_i18n::t!("ui.replay.export_results_cbor").into_owned(),
        ReplayExportFormat::Csv => rust_i18n::t!("ui.replay.export_results_csv").into_owned(),
    }
}

/// The file a battle whose export is named `stem` is written as.
///
/// The stem is `exported_file_stem`'s, the name the egui app writes, so a
/// directory both apps write into reads as one set.
pub fn file_name(stem: &str, format: ReplayExportFormat) -> String {
    format!("{stem}.{}", format.extension())
}

/// Writes `match_data` to `path`. JSON and CBOR carry the whole match; CSV
/// carries one flattened row per vehicle, which is what a spreadsheet reads.
pub fn write_export(match_data: &ExportedMatch, path: &Path, format: ReplayExportFormat) -> Result<(), ExportError> {
    let file =
        std::fs::File::create(path).map_err(|source| ExportError::Create { path: path.to_path_buf(), source })?;
    let file = std::io::BufWriter::new(file);

    match format {
        ReplayExportFormat::Json => serde_json::to_writer(file, match_data)
            .map_err(|source| ExportError::Json { path: path.to_path_buf(), source }),
        ReplayExportFormat::Cbor => ciborium::into_writer(match_data, file)
            .map_err(|source| ExportError::Cbor { path: path.to_path_buf(), source }),
        ReplayExportFormat::Csv => {
            let failed = |source: csv::Error| ExportError::Csv { path: path.to_path_buf(), source };
            let mut writer = csv::WriterBuilder::new().has_headers(true).from_writer(file);
            for vehicle in match_data.vehicles.iter().cloned() {
                writer.serialize(FlattenedVehicle::from(vehicle)).map_err(failed)?;
            }

            writer.flush().map_err(|source| failed(csv::Error::from(source)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_auto_export_setting_refuses_a_directory_that_is_not_one() {
        let off = ReplaySettings { auto_export_data: false, ..ReplaySettings::default() };
        assert_eq!(AutoExport::from_settings(&off), AutoExport::Off, "the setting is what turns it on");

        let nowhere = ReplaySettings {
            auto_export_data: true,
            auto_export_path: "G:/does-not-exist".to_owned(),
            ..ReplaySettings::default()
        };
        assert_eq!(AutoExport::from_settings(&nowhere), AutoExport::Off, "a missing directory is not one to write to");

        let directory = tempfile::tempdir().expect("a temporary directory");
        let file = directory.path().join("not-a-directory");
        std::fs::write(&file, b"").expect("the file is writable");
        let a_file = ReplaySettings {
            auto_export_data: true,
            auto_export_path: file.to_string_lossy().into_owned(),
            ..ReplaySettings::default()
        };
        assert_eq!(AutoExport::from_settings(&a_file), AutoExport::Off, "a file is not a directory");

        let here = ReplaySettings {
            auto_export_data: true,
            auto_export_path: directory.path().to_string_lossy().into_owned(),
            auto_export_format: ReplayExportFormat::Csv,
            ..ReplaySettings::default()
        };
        assert_eq!(
            AutoExport::from_settings(&here),
            AutoExport::To { directory: directory.path().to_path_buf(), format: ReplayExportFormat::Csv },
            "the chosen format carries through"
        );
    }

    #[test]
    fn each_export_format_writes_its_own_shape() {
        let normalized = crate::test_support::fixture_empty_battle_report();
        let export = ExportedMatch::new(&normalized, &[], &[], true);
        let directory = tempfile::tempdir().expect("a temporary directory");

        for format in MENU_FORMATS {
            let path = directory.path().join(file_name("match", format));
            write_export(&export, &path, format).expect("the export writes");

            assert!(path.is_file(), "{} wrote no file", menu_label(format));
        }

        // CSV writes one row per vehicle and the fixture has none, so an empty
        // file is the honest result there; the whole-document formats always
        // carry the metadata.
        let cbor = std::fs::read(directory.path().join("match.cbor")).expect("the CBOR exists");
        assert!(!cbor.is_empty());

        let json = std::fs::read(directory.path().join("match.json")).expect("the JSON exists");
        let json: serde_json::Value = serde_json::from_slice(&json).expect("the JSON parses back");
        assert!(json.get("metadata").is_some(), "the document keeps its metadata");
    }

    #[test]
    fn a_directory_that_cannot_be_written_is_reported_by_path() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("missing").join("match.json");
        let export = ExportedMatch::new(&crate::test_support::fixture_empty_battle_report(), &[], &[], true);

        let refused = write_export(&export, &path, ReplayExportFormat::Json).expect_err("the parent is missing");

        assert!(matches!(&refused, ExportError::Create { path: named, .. } if *named == path));
    }
}

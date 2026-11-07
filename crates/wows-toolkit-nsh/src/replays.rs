use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use wows_replays::ReplayFile;
use wows_replays::ReplayMeta;

use crate::error::LoadError;

/// Rows shown in the table; fixture metadata is repeated to reach this count.
pub const TABLE_ROWS: usize = 5000;

/// A row index into [`ReplayTable::rows`], independent of sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplayColumn {
    Date,
    Ship,
    Map,
    GameMode,
    Player,
    Version,
}

impl ReplayColumn {
    pub const ALL: [ReplayColumn; 6] = [
        ReplayColumn::Date,
        ReplayColumn::Ship,
        ReplayColumn::Map,
        ReplayColumn::GameMode,
        ReplayColumn::Player,
        ReplayColumn::Version,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ReplayColumn::Date => "date",
            ReplayColumn::Ship => "ship",
            ReplayColumn::Map => "map",
            ReplayColumn::GameMode => "mode",
            ReplayColumn::Player => "player",
            ReplayColumn::Version => "version",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            ReplayColumn::Date => "Date",
            ReplayColumn::Ship => "Ship",
            ReplayColumn::Map => "Map",
            ReplayColumn::GameMode => "Game mode",
            ReplayColumn::Player => "Player",
            ReplayColumn::Version => "Version",
        }
    }

    pub fn from_key(key: &str) -> Option<ReplayColumn> {
        ReplayColumn::ALL.into_iter().find(|column| column.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub column: ReplayColumn,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayRow {
    /// `dateTime` as the replay wrote it.
    pub date_raw: String,
    /// `YYYY-MM-DD HH:MM:SS`, so text order is time order; `None` when the
    /// raw value is not `DD.MM.YYYY HH:MM:SS`.
    pub date_iso: Option<String>,
    pub ship: String,
    pub map: String,
    pub game_mode: String,
    pub player: String,
    pub version: String,
    pub file_name: String,
    pub duration: Duration,
    pub vehicle_count: usize,
}

impl ReplayRow {
    pub fn from_meta(meta: &ReplayMeta, file_name: String) -> Self {
        // matchGroup is absent in old clients; gameType is the next best label
        // without game data, and the numeric mode id is the last resort.
        let game_mode = match (&meta.matchGroup, &meta.gameType) {
            (Some(group), _) => group.clone(),
            (None, Some(game_type)) => game_type.clone(),
            (None, None) => format!("mode {}", meta.gameMode),
        };

        Self {
            date_raw: meta.dateTime.clone(),
            date_iso: iso_date(&meta.dateTime),
            ship: meta.playerVehicle.clone(),
            map: meta.mapDisplayName.clone(),
            game_mode,
            player: meta.playerName.clone(),
            version: meta.clientVersionFromXml.clone(),
            file_name,
            duration: Duration::from_secs(u64::from(meta.duration)),
            vehicle_count: meta.vehicles.len(),
        }
    }

    /// The text a cell shows.
    pub fn field(&self, column: ReplayColumn) -> &str {
        match column {
            ReplayColumn::Date => self.date_iso.as_deref().unwrap_or(&self.date_raw),
            ReplayColumn::Ship => &self.ship,
            ReplayColumn::Map => &self.map,
            ReplayColumn::GameMode => &self.game_mode,
            ReplayColumn::Player => &self.player,
            ReplayColumn::Version => &self.version,
        }
    }

    /// Sort key: dates that did not parse order after those that did.
    fn sort_key(&self, column: ReplayColumn) -> (bool, &str) {
        let unparsed = column == ReplayColumn::Date && self.date_iso.is_none();
        (unparsed, self.field(column))
    }
}

/// Parsed fixture replays repeated to [`TABLE_ROWS`].
#[derive(Debug)]
pub struct ReplayTable {
    pub rows: Vec<ReplayRow>,
    pub source_files: usize,
    pub failed_files: Vec<PathBuf>,
}

impl ReplayTable {
    pub fn load_fixtures() -> Result<Arc<ReplayTable>, LoadError> {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let directories = [
            manifest.join("../../tests/fixtures/replays"),
            manifest.join("../wows-replay-insights/tests/fixtures/replays"),
        ];

        let mut parsed = Vec::new();
        let mut failed_files = Vec::new();
        for directory in &directories {
            let entries = std::fs::read_dir(directory)
                .map_err(|source| LoadError::ReadDir { path: directory.clone(), source })?;

            let mut paths: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "wowsreplay"))
                .collect();
            paths.sort();

            for path in paths {
                match ReplayFile::meta_only_from_file(&path) {
                    Ok(replay) => {
                        // read_dir entries always have a final component.
                        let Some(name) = path.file_name() else {
                            continue;
                        };
                        parsed.push(ReplayRow::from_meta(&replay.meta, name.to_string_lossy().into_owned()));
                    }
                    Err(error) => {
                        log::warn!("replay {} did not parse: {error}", path.display());
                        failed_files.push(path);
                    }
                }
            }
        }

        if parsed.is_empty() {
            return Err(LoadError::NoReplays);
        }

        let source_files = parsed.len();
        let rows = parsed.iter().cycle().take(TABLE_ROWS).cloned().collect();

        Ok(Arc::new(ReplayTable { rows, source_files, failed_files }))
    }

    /// Row ids in display order; `None` keeps load order.
    pub fn order(&self, sort: Option<SortSpec>) -> Vec<RowId> {
        let mut order: Vec<RowId> = (0..self.rows.len() as u32).map(RowId).collect();
        let Some(sort) = sort else {
            return order;
        };

        // Stable sort so equal keys keep load order in both directions.
        order.sort_by(|a, b| {
            let left = self.rows[a.0 as usize].sort_key(sort.column);
            let right = self.rows[b.0 as usize].sort_key(sort.column);
            match sort.direction {
                SortDirection::Ascending => left.cmp(&right),
                SortDirection::Descending => right.cmp(&left),
            }
        });

        order
    }

    pub fn row(&self, id: RowId) -> &ReplayRow {
        &self.rows[id.0 as usize]
    }
}

/// `DD.MM.YYYY HH:MM:SS` to `YYYY-MM-DD HH:MM:SS`; `None` for other shapes.
fn iso_date(raw: &str) -> Option<String> {
    let (date, time) = raw.split_once(' ')?;
    let mut parts = date.split('.');
    let (Some(day), Some(month), Some(year), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return None;
    };

    Some(format!("{year}-{month}-{day} {time}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_reorders_day_first_dates() {
        assert_eq!(iso_date("20.04.2019 12:50:57").as_deref(), Some("2019-04-20 12:50:57"));
        assert_eq!(iso_date("garbage"), None);
    }
}

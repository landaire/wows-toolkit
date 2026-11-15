//! Replays found under directories, newest last.

use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;
use wows_replays::ReplayFile;

/// A replay and the facts its header gives.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    pub path: PathBuf,
    pub player_name: String,

    /// The header's local recording time, `dd.mm.yyyy hh:mm:ss`.
    pub date_time: String,
    pub map: String,

    /// The recording player's ship, such as `PASB110-Vermont`.
    pub vehicle: String,
}

impl SearchHit {
    /// When the replay was recorded. `None` when the header's time does not
    /// parse.
    pub fn recorded_at(&self) -> Option<jiff::civil::DateTime> {
        parse_date_time(&self.date_time).ok()
    }
}

/// Every readable replay under `roots`, ordered by recording time with
/// unparsable times first. `progress` is called with the count read so far
/// every 100 replays.
pub fn search(roots: &[PathBuf], mut progress: impl FnMut(usize)) -> Result<Vec<SearchHit>, walkdir::Error> {
    let mut hits = Vec::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry?;
            if !entry.path().is_file() {
                continue;
            }

            // A file that does not read as a replay is not one.
            let Ok(replay) = ReplayFile::from_file(entry.path()) else { continue };
            hits.push(hit(entry.path(), replay));

            if hits.len() % 100 == 0 {
                progress(hits.len());
            }
        }
    }

    hits.sort_by_cached_key(SearchHit::recorded_at);

    Ok(hits)
}

/// Parses a header's `dd.mm.yyyy hh:mm:ss` recording time.
pub fn parse_date_time(text: &str) -> Result<jiff::civil::DateTime, jiff::Error> {
    jiff::civil::DateTime::strptime("%d.%m.%Y %H:%M:%S", text)
}

fn hit(path: &Path, replay: ReplayFile) -> SearchHit {
    let meta = replay.meta;

    SearchHit {
        path: path.to_path_buf(),
        player_name: meta.playerName,
        date_time: meta.dateTime,
        map: meta.mapDisplayName,
        vehicle: meta.playerVehicle,
    }
}

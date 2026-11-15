//! Reading the game's own logs while it writes them.

use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;

/// The last `limit` bytes of the file at `path`, from the first line that starts inside them.
pub(super) fn tail(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(limit);
    file.seek(SeekFrom::Start(start))?;

    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;

    if start > 0
        && let Some(newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=newline);
    }

    Ok(bytes)
}

/// Reads the complete lines appended to a log since the last read.
#[derive(Debug)]
pub(super) struct LogFollower {
    path: PathBuf,
    offset: u64,
}

impl LogFollower {
    /// Starts within the last `backlog` bytes, so lines written before the capture count too.
    pub(super) fn new(path: PathBuf, backlog: u64) -> io::Result<Self> {
        let len = std::fs::metadata(&path)?.len();

        Ok(Self { path, offset: len.saturating_sub(backlog) })
    }

    /// Up to `limit` bytes of new complete lines.
    pub(super) fn read_new(&mut self, limit: u64) -> io::Result<String> {
        let mut file = File::open(&self.path)?;
        let len = file.metadata()?.len();
        // The game started a new log.
        if len < self.offset {
            self.offset = 0;
        }
        file.seek(SeekFrom::Start(self.offset))?;

        let mut bytes = Vec::new();
        file.take(limit).read_to_end(&mut bytes)?;
        let complete = match bytes.iter().rposition(|byte| *byte == b'\n') {
            Some(newline) => newline + 1,
            // A line longer than `limit` would stop the follower; take it in pieces.
            None if bytes.len() as u64 == limit => bytes.len(),
            None => 0,
        };
        bytes.truncate(complete);
        self.offset += complete as u64;

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

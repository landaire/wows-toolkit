use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::changes::SettingKey;
use crate::document::AttrName;
use crate::document::ByteOffset;
use crate::document::PrefsPath;
use crate::settings::Unavailable;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PrefsParseError {
    #[error("file ends inside the tag at byte {offset}")]
    UnexpectedEnd { offset: ByteOffset },
    #[error("malformed tag at byte {offset}")]
    MalformedTag { offset: ByteOffset },
    #[error("closing tag without an open element at byte {offset}")]
    UnexpectedCloseTag { offset: ByteOffset },
    #[error("closing tag `{found}` at byte {offset} does not match `{expected}`")]
    MismatchedCloseTag { offset: ByteOffset, expected: String, found: String },
    #[error("element `{name}` is not closed")]
    UnclosedElement { name: String },
    #[error("second root element at byte {offset}")]
    MultipleRoots { offset: ByteOffset },
    #[error("text outside the root element at byte {offset}")]
    TextOutsideRoot { offset: ByteOffset },
    #[error("no root element")]
    NoRootElement,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PrefsEditError {
    #[error("`{path}` does not exist")]
    ElementNotFound { path: PrefsPath },
    #[error("`{path}` has no attribute `{name}`")]
    AttributeNotFound { path: PrefsPath, name: AttrName },
    #[error("`{path}` holds elements, not a value")]
    HasChildren { path: PrefsPath },
    #[error("`{path}` holds comments or other markup beside its value")]
    HasMarkup { path: PrefsPath },
    #[error("values cannot contain `{character}`")]
    ForbiddenCharacter { character: char },
    #[error("values cannot start or end with whitespace")]
    SurroundingWhitespace,
    #[error("the edited file does not parse")]
    Parse(#[from] PrefsParseError),
}

#[derive(Debug, Error, Clone, Copy, PartialEq)]
pub enum InvalidValue {
    #[error("{width} x {height} is not a supported resolution")]
    Resolution { width: u16, height: u16 },
    #[error("{hz} Hz is not a supported refresh rate")]
    RefreshRate { hz: u16 },
    #[error("{fps} is not a supported frame rate limit")]
    FrameRate { fps: u16 },
    #[error("volume {volume} is outside 0 to 1")]
    Volume { volume: f32 },
    #[error("interface scale {scale} is not supported")]
    UiScale { scale: f32 },
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("Close the game first. It rewrites its settings when it exits.")]
pub struct GameRunning;

#[derive(Debug, Error)]
pub enum ReadError {
    #[error("Could not read {}.", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{} is not UTF-8 text.", path.display())]
    NotUtf8 { path: PathBuf },
    #[error("Could not parse {}.", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: PrefsParseError,
    },
}

#[derive(Debug, Error)]
pub enum SaveError {
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error("{reason}")]
    SectionUnavailable { reason: Unavailable },
    #[error("{key} is not in the game's settings yet. Start the game once.")]
    FieldAbsent { key: SettingKey },
    #[error("{key} has a value this version does not recognize: {found}")]
    UnrecognizedValue { key: SettingKey, found: String },
    #[error("Channel volumes can only be changed with the custom sound mix.")]
    MixingPresetNotCustom,
    #[error("The game changed {} since it was read. Reopen game settings.", path.display())]
    ChangedOnDisk { path: PathBuf },
    #[error("Could not edit {}.", path.display())]
    Edit {
        path: PathBuf,
        #[source]
        source: PrefsEditError,
    },
    #[error("Could not back up {}.", path.display())]
    Backup {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Could not write {}.", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Earlier files of the save were replaced and could not be put back.
    #[error("Could not write {}. Some files were changed and not restored; backups are beside them.", path.display())]
    PartiallyWritten {
        path: PathBuf,
        #[source]
        source: io::Error,
        unrestored: Vec<PathBuf>,
    },
}

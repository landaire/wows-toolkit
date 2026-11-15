//! Reads and patches World of Warships preference files.
//!
//! The files are BigWorld-style XML that generic XML tools reject or rewrite.
//! [`PrefsDocument`] keeps them byte for byte and replaces only edited values.
//! [`GameSettingsStore`] exposes the settings worth changing outside the game
//! and saves them with a backup while the game is not running.

mod changes;
mod document;
mod error;
mod files;
mod guard;
mod settings;
mod store;
mod values;

pub use changes::SettingKey;
pub use changes::SettingsChanges;
pub use document::AttrName;
pub use document::ByteOffset;
pub use document::PrefsDocument;
pub use document::PrefsEdit;
pub use document::PrefsPath;
pub use error::GameRunning;
pub use error::InvalidValue;
pub use error::PrefsEditError;
pub use error::PrefsParseError;
pub use error::ReadError;
pub use error::SaveError;
pub use files::BuildNumber;
pub use files::DirectFiles;
pub use files::SettingsFile;
pub use files::SettingsFiles;
pub use guard::GameActivity;
pub use guard::GameStopped;
pub use settings::DisplaySettings;
pub use settings::FrameRateLimits;
pub use settings::GameSettings;
pub use settings::GraphicsSettings;
pub use settings::InterfaceSettings;
pub use settings::LanguageSettings;
pub use settings::ReplayConfigSource;
pub use settings::ReplaySettings;
pub use settings::Section;
pub use settings::SectionId;
pub use settings::SoundSettings;
pub use settings::Unavailable;
pub use store::GameSettingsStore;
pub use store::Snapshot;
pub use values::Field;
pub use values::FrameRateLimit;
pub use values::GraphicsPreset;
pub use values::LanguageCode;
pub use values::MixingPreset;
pub use values::MonitorIndex;
pub use values::MsaaMode;
pub use values::PixelCount;
pub use values::Quality;
pub use values::RefreshRate;
pub use values::ReplayCount;
pub use values::Resolution;
pub use values::TextureFiltering;
pub use values::UiScale;
pub use values::Volume;
pub use values::VolumeChannel;
pub use values::WindowMode;

//! The application shell's native side: settings, language, theme, updates,
//! diagnostics and the background work the shell owns.

mod error;
pub mod logging;
mod services;
pub(crate) mod settings;
mod theme;
mod twitch;
pub mod upload;
mod update;
pub mod window;

pub use error::PasteError;
pub use error::SettingsError;
pub use error::ShellError;
pub use error::UpdateError;
pub use error::UploadError;
pub use error::WindowError;
pub use error::chain;
pub use services::Language;
pub use services::LogCopy;
pub use services::Shell;
pub use services::ThemeOption;
pub use settings::AppSettings;
pub use settings::ArmorDefaults;
pub use settings::DEFAULT_ZOOM;
pub use settings::ReplayPreferences;
pub use settings::SettingKey;
pub use settings::Settings;
pub(crate) use settings::update_replay_settings;
pub use twitch::Twitch;
pub use twitch::TwitchState;
pub use twitch::TwitchStatus;
pub use update::ReleaseOffer;

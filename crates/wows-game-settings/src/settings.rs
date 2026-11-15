use std::collections::BTreeMap;
use std::fmt;

use crate::document::AttrName;
use crate::document::PrefsDocument;
use crate::document::PrefsPath;
use crate::files::BuildNumber;
use crate::files::SettingsFile;
use crate::values::Field;
use crate::values::FrameRateLimit;
use crate::values::GraphicsPreset;
use crate::values::LanguageCode;
use crate::values::MixingPreset;
use crate::values::MonitorIndex;
use crate::values::MsaaMode;
use crate::values::PrefsValue;
use crate::values::Quality;
use crate::values::RefreshRate;
use crate::values::ReplayCount;
use crate::values::Resolution;
use crate::values::TextureFiltering;
use crate::values::UiScale;
use crate::values::Volume;
use crate::values::VolumeChannel;
use crate::values::WindowMode;

/// The settings worth exposing, read from the game's files.
#[derive(Clone, Debug, PartialEq)]
pub struct GameSettings {
    pub display: Section<DisplaySettings>,
    pub frame_rate: Section<FrameRateLimits>,
    pub interface: Section<InterfaceSettings>,
    pub sound: Section<SoundSettings>,
    /// Read-only: the game computes and validates these against the hardware.
    pub graphics: Section<GraphicsSettings>,
    pub replays: Section<ReplaySettings>,
    /// Read-only: changing the language needs a different `locale` download.
    pub language: Section<LanguageSettings>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Section<T> {
    Available(T),
    Unavailable(Unavailable),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// The game has not written the file yet, or the install lacks it.
    FileMissing { file: SettingsFile },
    /// The section's version is one this crate does not know, so its keys may
    /// mean something else.
    UnsupportedVersion { file: SettingsFile, section: SectionId, found: Option<String> },
    /// Replay settings are per build, and no build was given.
    BuildUnknown,
}

/// A versioned section of a settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionId {
    /// `user_preferences.xml` `local/device`.
    Device,
    /// `user_preferences.xml` `local/graphics`.
    Graphics,
    /// `user_preferences.xml` `local/sound`.
    Sound,
    /// `user_preferences.xml` `local/ui`.
    Interface,
    /// `preferences.xml` `scriptsPreferences`.
    ScriptsPreferences,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySettings {
    pub window_mode: Field<WindowMode>,
    pub monitor_index: Field<MonitorIndex>,
    /// The game's label for its monitor, such as `2 - Name (primary)`.
    /// Read-only.
    pub monitor_name: Field<String>,
    pub fullscreen_resolution: Field<Resolution>,
    pub refresh_rate: Field<RefreshRate>,
    pub vsync: Field<bool>,
    pub ui_scale: Field<UiScale>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameRateLimits {
    pub battle: Field<FrameRateLimit>,
    pub port: Field<FrameRateLimit>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceSettings {
    pub skip_intro_video: Field<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SoundSettings {
    pub master: Field<Volume>,
    pub music_port: Field<Volume>,
    pub music_battle: Field<Volume>,

    /// Channel volumes are editable only under the custom mix.
    pub mixing_preset: Field<MixingPreset>,
    pub channels: BTreeMap<VolumeChannel, Field<Volume>>,

    /// Read-only: a voice package is a separate download.
    pub voice_language: Field<LanguageCode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsSettings {
    pub preset: Field<GraphicsPreset>,
    pub antialiasing: Field<MsaaMode>,
    pub texture_quality: Field<Quality>,
    pub texture_filtering: Field<TextureFiltering>,
    pub shadow_quality: Field<Quality>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplaySettings {
    /// Replay settings live in game files, which are per build.
    pub build: BuildNumber,
    pub source: ReplayConfigSource,
    pub kept: Field<ReplayCount>,
    /// `true` puts each game version's replays in its own folder.
    pub versioned: Field<bool>,
    /// Read-only: the folder the game writes replays to, relative to the
    /// install unless absolute.
    pub directory: Field<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayConfigSource {
    /// `res_mods/engine_config.xml` exists, from a mod or an earlier save.
    Override,
    /// Only the build's own `res/engine_config.xml` exists. A save creates
    /// the override from it.
    GameDefault,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageSettings {
    /// `game_info.xml` `localization`.
    pub client: Field<LanguageCode>,
}

/// The parsed files settings are read from.
pub(crate) struct SourceDocuments<'a> {
    pub(crate) build: Option<BuildNumber>,
    pub(crate) preferences: Option<&'a PrefsDocument>,
    pub(crate) user_preferences: Option<&'a PrefsDocument>,
    pub(crate) engine_config: Option<(&'a PrefsDocument, ReplayConfigSource)>,
    pub(crate) game_info: Option<&'a PrefsDocument>,
}

/// Paths of every key read or written.
pub(crate) mod keys {
    use crate::document::AttrName;
    use crate::document::PrefsPath;
    use crate::values::VolumeChannel;

    pub(crate) const VERSION: AttrName = AttrName::new("version");

    pub(crate) const DEVICE: PrefsPath = PrefsPath::new("local/device");
    pub(crate) const WINDOW_MODE: AttrName = AttrName::new("windowMode");
    pub(crate) const MONITOR_INDEX: AttrName = AttrName::new("monitorIndex");
    pub(crate) const FULLSCREEN: PrefsPath = PrefsPath::new("local/device/fullscreen");
    pub(crate) const WIDTH: AttrName = AttrName::new("width");
    pub(crate) const HEIGHT: AttrName = AttrName::new("height");
    pub(crate) const ASPECT_RATIO: AttrName = AttrName::new("aspectRatio");
    pub(crate) const REFRESH: AttrName = AttrName::new("refresh");
    pub(crate) const WAIT_VSYNC: AttrName = AttrName::new("waitVSync");
    pub(crate) const MONITOR: PrefsPath = PrefsPath::new("local/monitor");
    pub(crate) const MONITOR_NAME: PrefsPath = PrefsPath::new("local/monitor/index");
    pub(crate) const UI_SCALE: PrefsPath = PrefsPath::new("local/monitor/uiScale");

    pub(crate) const GRAPHICS: PrefsPath = PrefsPath::new("local/graphics");
    pub(crate) const PRESET: AttrName = AttrName::new("preset");
    pub(crate) const MSAA: PrefsPath = PrefsPath::new("local/graphics/msaa/quality");
    pub(crate) const TEXTURE: PrefsPath = PrefsPath::new("local/graphics/texture");
    pub(crate) const SHADOW: PrefsPath = PrefsPath::new("local/graphics/shadow");
    pub(crate) const QUALITY: AttrName = AttrName::new("quality");
    pub(crate) const FILTERING: AttrName = AttrName::new("filtering");

    pub(crate) const UI: PrefsPath = PrefsPath::new("local/ui");
    pub(crate) const SKIP_VIDEO: PrefsPath = PrefsPath::new("local/ui/skipVideo");

    pub(crate) const SOUND: PrefsPath = PrefsPath::new("local/sound");
    pub(crate) const MIXING_PRESET: AttrName = AttrName::new("mixingPreset");
    pub(crate) const MASTER_VOLUME: PrefsPath = PrefsPath::new("local/sound/volume/master");
    pub(crate) const MUSIC_PORT: PrefsPath = PrefsPath::new("local/sound/music/dockVolume");
    pub(crate) const MUSIC_BATTLE: PrefsPath = PrefsPath::new("local/sound/music/battleVolume");
    pub(crate) const VOICE_PACKAGE: PrefsPath = PrefsPath::new("local/sound/voice/package");

    pub(crate) const SCRIPTS_VERSION: PrefsPath = PrefsPath::new("scriptsPreferences/version");
    pub(crate) const MAX_FRAME_RATE: PrefsPath = PrefsPath::new("scriptsPreferences/renderer/maxFrameRate");
    pub(crate) const MAX_DOCK_FRAME_RATE: PrefsPath = PrefsPath::new("scriptsPreferences/renderer/maxDockFrameRate");

    pub(crate) const REPLAYS_KEPT: PrefsPath = PrefsPath::new("replays/maxReplaysToSave");
    pub(crate) const REPLAYS_VERSIONED: PrefsPath = PrefsPath::new("replays/versioned");
    pub(crate) const REPLAYS_DIRECTORY: PrefsPath = PrefsPath::new("replays/dirPath");

    pub(crate) const LOCALIZATION: PrefsPath = PrefsPath::new("game/localization");

    pub(crate) fn channel_volume(channel: VolumeChannel) -> PrefsPath {
        match channel {
            VolumeChannel::Interface => PrefsPath::new("local/sound/volume/ui"),
            VolumeChannel::Engines => PrefsPath::new("local/sound/volume/engines"),
            VolumeChannel::Voice => PrefsPath::new("local/sound/volume/voice"),
            VolumeChannel::World => PrefsPath::new("local/sound/volume/world"),
            VolumeChannel::Gunfire => PrefsPath::new("local/sound/volume/shoot"),
            VolumeChannel::Damage => PrefsPath::new("local/sound/volume/damage"),
        }
    }
}

impl<T> Section<T> {
    pub fn available(&self) -> Option<&T> {
        match self {
            Section::Available(section) => Some(section),
            Section::Unavailable(_) => None,
        }
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unavailable::FileMissing { file } => write!(f, "{file} does not exist yet. Start the game once."),
            Unavailable::UnsupportedVersion { .. } => f.write_str("Changed in this game version."),
            Unavailable::BuildUnknown => f.write_str("No game build found."),
        }
    }
}

impl SectionId {
    /// The version this crate reads.
    pub fn supported_version(self) -> &'static str {
        match self {
            SectionId::Device => "2",
            SectionId::Graphics => "5",
            SectionId::Sound => "1",
            SectionId::Interface => "1",
            SectionId::ScriptsPreferences => "28",
        }
    }

    fn file(self) -> SettingsFile {
        match self {
            SectionId::Device | SectionId::Graphics | SectionId::Sound | SectionId::Interface => {
                SettingsFile::UserPreferences
            }
            SectionId::ScriptsPreferences => SettingsFile::Preferences,
        }
    }

    fn version(self, document: &PrefsDocument) -> Option<&str> {
        match self {
            SectionId::Device => document.attribute(&keys::DEVICE, &keys::VERSION),
            SectionId::Graphics => document.attribute(&keys::GRAPHICS, &keys::VERSION),
            SectionId::Sound => document.attribute(&keys::SOUND, &keys::VERSION),
            SectionId::Interface => document.attribute(&keys::UI, &keys::VERSION),
            SectionId::ScriptsPreferences => document.value(&keys::SCRIPTS_VERSION),
        }
    }
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            SectionId::Device => "device",
            SectionId::Graphics => "graphics",
            SectionId::Sound => "sound",
            SectionId::Interface => "ui",
            SectionId::ScriptsPreferences => "scriptsPreferences",
        };

        f.write_str(name)
    }
}

impl GameSettings {
    pub(crate) fn from_documents(documents: &SourceDocuments<'_>) -> GameSettings {
        let user_preferences = documents.user_preferences;

        GameSettings {
            display: versioned(user_preferences, SectionId::Device, display),
            frame_rate: versioned(documents.preferences, SectionId::ScriptsPreferences, frame_rate),
            interface: versioned(user_preferences, SectionId::Interface, |document| InterfaceSettings {
                skip_intro_video: Field::from_text(document.value(&keys::SKIP_VIDEO)),
            }),
            sound: versioned(user_preferences, SectionId::Sound, sound),
            graphics: versioned(user_preferences, SectionId::Graphics, graphics),
            replays: replays(documents),
            language: match documents.game_info {
                Some(document) => Section::Available(LanguageSettings {
                    client: Field::from_text(document.value(&keys::LOCALIZATION)),
                }),
                None => Section::Unavailable(Unavailable::FileMissing { file: SettingsFile::GameInfo }),
            },
        }
    }
}

/// Reads a section only when its version is one whose keys are known.
pub(crate) fn section_document(
    document: Option<&PrefsDocument>,
    section: SectionId,
) -> Result<&PrefsDocument, Unavailable> {
    let file = section.file();
    let Some(document) = document else {
        return Err(Unavailable::FileMissing { file });
    };

    let found = section.version(document);
    if found != Some(section.supported_version()) {
        return Err(Unavailable::UnsupportedVersion { file, section, found: found.map(str::to_owned) });
    }

    Ok(document)
}

fn versioned<T>(
    document: Option<&PrefsDocument>,
    section: SectionId,
    read: impl FnOnce(&PrefsDocument) -> T,
) -> Section<T> {
    match section_document(document, section) {
        Ok(document) => Section::Available(read(document)),
        Err(reason) => Section::Unavailable(reason),
    }
}

fn attribute_field<T>(document: &PrefsDocument, path: &PrefsPath, name: &AttrName) -> Field<T>
where
    T: PrefsValue,
{
    Field::from_text(document.attribute(path, name))
}

fn display(document: &PrefsDocument) -> DisplaySettings {
    let width = document.attribute(&keys::FULLSCREEN, &keys::WIDTH);
    let height = document.attribute(&keys::FULLSCREEN, &keys::HEIGHT);
    let fullscreen_resolution = match (width, height) {
        (Some(width), Some(height)) => {
            let parsed = width
                .parse()
                .ok()
                .zip(height.parse().ok())
                .and_then(|(width, height)| Resolution::new(width, height).ok());
            match parsed {
                Some(resolution) => Field::Known(resolution),
                None => Field::Unrecognized(format!("{width} x {height}")),
            }
        }
        _ => Field::Absent,
    };

    DisplaySettings {
        window_mode: attribute_field(document, &keys::DEVICE, &keys::WINDOW_MODE),
        monitor_index: attribute_field(document, &keys::DEVICE, &keys::MONITOR_INDEX),
        monitor_name: Field::from_text(document.value(&keys::MONITOR_NAME)),
        fullscreen_resolution,
        refresh_rate: attribute_field(document, &keys::FULLSCREEN, &keys::REFRESH),
        vsync: attribute_field(document, &keys::FULLSCREEN, &keys::WAIT_VSYNC),
        ui_scale: Field::from_text(document.value(&keys::UI_SCALE)),
    }
}

fn frame_rate(document: &PrefsDocument) -> FrameRateLimits {
    FrameRateLimits {
        battle: Field::from_text(document.value(&keys::MAX_FRAME_RATE)),
        port: Field::from_text(document.value(&keys::MAX_DOCK_FRAME_RATE)),
    }
}

fn sound(document: &PrefsDocument) -> SoundSettings {
    let channels = VolumeChannel::ALL
        .into_iter()
        .map(|channel| (channel, Field::from_text(document.value(&keys::channel_volume(channel)))))
        .collect();

    SoundSettings {
        master: Field::from_text(document.value(&keys::MASTER_VOLUME)),
        music_port: Field::from_text(document.value(&keys::MUSIC_PORT)),
        music_battle: Field::from_text(document.value(&keys::MUSIC_BATTLE)),
        mixing_preset: attribute_field(document, &keys::SOUND, &keys::MIXING_PRESET),
        channels,
        voice_language: Field::from_text(document.value(&keys::VOICE_PACKAGE)),
    }
}

fn graphics(document: &PrefsDocument) -> GraphicsSettings {
    GraphicsSettings {
        preset: attribute_field(document, &keys::GRAPHICS, &keys::PRESET),
        antialiasing: Field::from_text(document.value(&keys::MSAA)),
        texture_quality: attribute_field(document, &keys::TEXTURE, &keys::QUALITY),
        texture_filtering: attribute_field(document, &keys::TEXTURE, &keys::FILTERING),
        shadow_quality: attribute_field(document, &keys::SHADOW, &keys::QUALITY),
    }
}

fn replays(documents: &SourceDocuments<'_>) -> Section<ReplaySettings> {
    let Some(build) = documents.build else {
        return Section::Unavailable(Unavailable::BuildUnknown);
    };
    let Some((document, source)) = documents.engine_config else {
        return Section::Unavailable(Unavailable::FileMissing { file: SettingsFile::EngineConfig { build } });
    };

    Section::Available(ReplaySettings {
        build,
        source,
        kept: Field::from_text(document.value(&keys::REPLAYS_KEPT)),
        versioned: Field::from_text(document.value(&keys::REPLAYS_VERSIONED)),
        directory: Field::from_text(document.value(&keys::REPLAYS_DIRECTORY)),
    })
}

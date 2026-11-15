use std::collections::BTreeMap;
use std::fmt;

use crate::document::AttrName;
use crate::document::PrefsDocument;
use crate::document::PrefsEdit;
use crate::document::PrefsPath;
use crate::error::SaveError;
use crate::files::SettingsFile;
use crate::settings::GameSettings;
use crate::settings::Section;
use crate::settings::keys;
use crate::values::Field;
use crate::values::FrameRateLimit;
use crate::values::MixingPreset;
use crate::values::MonitorIndex;
use crate::values::PrefsValue;
use crate::values::RefreshRate;
use crate::values::ReplayCount;
use crate::values::Resolution;
use crate::values::UiScale;
use crate::values::Volume;
use crate::values::VolumeChannel;
use crate::values::WindowMode;

/// Values to write. `None` leaves a setting as the game wrote it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsChanges {
    pub window_mode: Option<WindowMode>,
    pub monitor_index: Option<MonitorIndex>,
    pub fullscreen_resolution: Option<Resolution>,
    pub refresh_rate: Option<RefreshRate>,
    pub vsync: Option<bool>,
    pub ui_scale: Option<UiScale>,

    pub battle_frame_rate: Option<FrameRateLimit>,
    pub port_frame_rate: Option<FrameRateLimit>,

    pub skip_intro_video: Option<bool>,

    pub master_volume: Option<Volume>,
    pub music_port_volume: Option<Volume>,
    pub music_battle_volume: Option<Volume>,
    /// Needs the custom sound mix.
    pub channel_volumes: BTreeMap<VolumeChannel, Volume>,

    pub replays_kept: Option<ReplayCount>,
    pub versioned_replays: Option<bool>,
}

/// An editable setting, for errors that name one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingKey {
    WindowMode,
    MonitorIndex,
    FullscreenResolution,
    RefreshRate,
    Vsync,
    UiScale,
    BattleFrameRate,
    PortFrameRate,
    SkipIntroVideo,
    MasterVolume,
    MusicPortVolume,
    MusicBattleVolume,
    ChannelVolume(VolumeChannel),
    ReplaysKept,
    VersionedReplays,
}

/// One text edit to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedEdit {
    pub(crate) file: SettingsFile,
    pub(crate) edit: PrefsEdit,
}

impl SettingsChanges {
    pub fn is_empty(&self) -> bool {
        self == &SettingsChanges::default()
    }

    fn has_display_change(&self) -> bool {
        self.window_mode.is_some()
            || self.monitor_index.is_some()
            || self.fullscreen_resolution.is_some()
            || self.refresh_rate.is_some()
            || self.vsync.is_some()
            || self.ui_scale.is_some()
    }

    fn has_sound_change(&self) -> bool {
        self.master_volume.is_some()
            || self.music_port_volume.is_some()
            || self.music_battle_volume.is_some()
            || !self.channel_volumes.is_empty()
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            SettingKey::WindowMode => "Window mode",
            SettingKey::MonitorIndex => "Monitor",
            SettingKey::FullscreenResolution => "Fullscreen resolution",
            SettingKey::RefreshRate => "Refresh rate",
            SettingKey::Vsync => "Vertical sync",
            SettingKey::UiScale => "Interface scale",
            SettingKey::BattleFrameRate => "Frame rate limit in battle",
            SettingKey::PortFrameRate => "Frame rate limit in port",
            SettingKey::SkipIntroVideo => "Skip intro video",
            SettingKey::MasterVolume => "Master volume",
            SettingKey::MusicPortVolume => "Music volume in port",
            SettingKey::MusicBattleVolume => "Music volume in battle",
            SettingKey::ChannelVolume(VolumeChannel::Interface) => "Interface volume",
            SettingKey::ChannelVolume(VolumeChannel::Engines) => "Engine volume",
            SettingKey::ChannelVolume(VolumeChannel::Voice) => "Voice volume",
            SettingKey::ChannelVolume(VolumeChannel::World) => "Environment volume",
            SettingKey::ChannelVolume(VolumeChannel::Gunfire) => "Gunfire volume",
            SettingKey::ChannelVolume(VolumeChannel::Damage) => "Damage volume",
            SettingKey::ReplaysKept => "Replays kept",
            SettingKey::VersionedReplays => "Replay folder per version",
        };

        f.write_str(label)
    }
}

struct Planner {
    edits: Vec<PlannedEdit>,
}

impl Planner {
    fn value<T>(&mut self, file: SettingsFile, path: &PrefsPath, value: &T)
    where
        T: PrefsValue,
    {
        self.edits.push(PlannedEdit { file, edit: PrefsEdit::SetValue { path: path.clone(), value: value.to_text() } });
    }

    fn attribute<T>(&mut self, file: SettingsFile, path: &PrefsPath, name: &AttrName, value: &T)
    where
        T: PrefsValue,
    {
        self.attribute_text(file, path, name, value.to_text());
    }

    fn attribute_text(&mut self, file: SettingsFile, path: &PrefsPath, name: &AttrName, value: String) {
        self.edits.push(PlannedEdit {
            file,
            edit: PrefsEdit::SetAttribute { path: path.clone(), name: name.clone(), value },
        });
    }
}

/// Turns changes into edits against the current files. Refuses a change to
/// a section of unknown version, a key the file lacks, or a value the game
/// wrote that this crate does not recognize.
pub(crate) fn plan(
    changes: &SettingsChanges,
    settings: &GameSettings,
    user_preferences: Option<&PrefsDocument>,
) -> Result<Vec<PlannedEdit>, SaveError> {
    let mut planner = Planner { edits: Vec::new() };

    if let Some(display) = section(&settings.display, changes.has_display_change())? {
        let user = SettingsFile::UserPreferences;

        if let Some(mode) = changes.window_mode {
            require(&display.window_mode, SettingKey::WindowMode)?;
            planner.attribute(user, &keys::DEVICE, &keys::WINDOW_MODE, &mode);
            // The `monitor` section repeats the mode; keep the copies equal.
            if user_preferences.and_then(|document| document.attribute(&keys::MONITOR, &keys::WINDOW_MODE)).is_some() {
                planner.attribute(user, &keys::MONITOR, &keys::WINDOW_MODE, &mode);
            }
        }

        if let Some(index) = changes.monitor_index {
            require(&display.monitor_index, SettingKey::MonitorIndex)?;
            planner.attribute(user, &keys::DEVICE, &keys::MONITOR_INDEX, &index);
        }

        if let Some(resolution) = changes.fullscreen_resolution {
            require(&display.fullscreen_resolution, SettingKey::FullscreenResolution)?;
            planner.attribute_text(user, &keys::FULLSCREEN, &keys::WIDTH, resolution.width.get().to_string());
            planner.attribute_text(user, &keys::FULLSCREEN, &keys::HEIGHT, resolution.height.get().to_string());
            if user_preferences
                .and_then(|document| document.attribute(&keys::FULLSCREEN, &keys::ASPECT_RATIO))
                .is_some()
            {
                planner.attribute_text(user, &keys::FULLSCREEN, &keys::ASPECT_RATIO, resolution.aspect_ratio_text());
            }
        }

        if let Some(rate) = changes.refresh_rate {
            require(&display.refresh_rate, SettingKey::RefreshRate)?;
            planner.attribute(user, &keys::FULLSCREEN, &keys::REFRESH, &rate);
        }

        if let Some(vsync) = changes.vsync {
            require(&display.vsync, SettingKey::Vsync)?;
            planner.attribute(user, &keys::FULLSCREEN, &keys::WAIT_VSYNC, &vsync);
        }

        if let Some(scale) = changes.ui_scale {
            require(&display.ui_scale, SettingKey::UiScale)?;
            planner.value(user, &keys::UI_SCALE, &scale);
        }
    }

    let frame_rate_changed = changes.battle_frame_rate.is_some() || changes.port_frame_rate.is_some();
    if let Some(frame_rate) = section(&settings.frame_rate, frame_rate_changed)? {
        let preferences = SettingsFile::Preferences;

        if let Some(limit) = changes.battle_frame_rate {
            require(&frame_rate.battle, SettingKey::BattleFrameRate)?;
            planner.value(preferences, &keys::MAX_FRAME_RATE, &limit);
        }

        if let Some(limit) = changes.port_frame_rate {
            require(&frame_rate.port, SettingKey::PortFrameRate)?;
            planner.value(preferences, &keys::MAX_DOCK_FRAME_RATE, &limit);
        }
    }

    if let Some(interface) = section(&settings.interface, changes.skip_intro_video.is_some())?
        && let Some(skip) = changes.skip_intro_video
    {
        require(&interface.skip_intro_video, SettingKey::SkipIntroVideo)?;
        planner.value(SettingsFile::UserPreferences, &keys::SKIP_VIDEO, &skip);
    }

    if let Some(sound) = section(&settings.sound, changes.has_sound_change())? {
        let user = SettingsFile::UserPreferences;
        let volumes = [
            (changes.master_volume, &sound.master, SettingKey::MasterVolume, keys::MASTER_VOLUME),
            (changes.music_port_volume, &sound.music_port, SettingKey::MusicPortVolume, keys::MUSIC_PORT),
            (changes.music_battle_volume, &sound.music_battle, SettingKey::MusicBattleVolume, keys::MUSIC_BATTLE),
        ];
        for (change, field, key, path) in volumes {
            let Some(volume) = change else {
                continue;
            };

            require(field, key)?;
            planner.value(user, &path, &volume);
        }

        if !changes.channel_volumes.is_empty() && !matches!(sound.mixing_preset, Field::Known(MixingPreset::Custom)) {
            return Err(SaveError::MixingPresetNotCustom);
        }

        for (&channel, volume) in &changes.channel_volumes {
            let key = SettingKey::ChannelVolume(channel);
            let Some(field) = sound.channels.get(&channel) else {
                return Err(SaveError::FieldAbsent { key });
            };

            require(field, key)?;
            planner.value(user, &keys::channel_volume(channel), volume);
        }
    }

    let replays_changed = changes.replays_kept.is_some() || changes.versioned_replays.is_some();
    if let Some(replays) = section(&settings.replays, replays_changed)? {
        let file = SettingsFile::EngineConfig { build: replays.build };

        if let Some(kept) = changes.replays_kept {
            require(&replays.kept, SettingKey::ReplaysKept)?;
            planner.value(file, &keys::REPLAYS_KEPT, &kept);
        }

        if let Some(versioned) = changes.versioned_replays {
            require(&replays.versioned, SettingKey::VersionedReplays)?;
            planner.value(file, &keys::REPLAYS_VERSIONED, &versioned);
        }
    }

    Ok(planner.edits)
}

/// The section when it is readable. A change to an unreadable section is an
/// error; an untouched one is skipped.
fn section<T>(section: &Section<T>, changed: bool) -> Result<Option<&T>, SaveError> {
    match section {
        Section::Available(section) => Ok(Some(section)),
        Section::Unavailable(_) if !changed => Ok(None),
        Section::Unavailable(reason) => Err(SaveError::SectionUnavailable { reason: reason.clone() }),
    }
}

fn require<T>(field: &Field<T>, key: SettingKey) -> Result<(), SaveError> {
    match field {
        Field::Known(_) => Ok(()),
        Field::Absent => Err(SaveError::FieldAbsent { key }),
        Field::Unrecognized(found) => Err(SaveError::UnrecognizedValue { key, found: found.clone() }),
    }
}

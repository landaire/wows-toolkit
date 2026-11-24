//! A baked clip of a battle as one file: its frames, the map art and the icons
//! they draw with, for a viewer that has no game install.
//!
//! Layout: the magic bytes, the format version as a little-endian `u32`, then
//! the zlib-compressed rkyv archive of a [`TrackFile`].

use std::io::Read;
use std::io::Write;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rkyv::util::AlignedVec;
use wows_replays::types::GameClock;

use crate::draw_command::DrawCommand;
use crate::error::TrackFileError;

const MAGIC: [u8; 4] = *b"WTRK";

/// The most a payload may decompress to. A three-and-a-half minute clip at a
/// quarter-second interval is about 27 MiB.
const MAX_PAYLOAD_BYTES: usize = 64 << 20;

/// Which layout of [`TrackFile`] a file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackFileVersion(u32);

impl TrackFileVersion {
    /// The layout this build reads and writes.
    pub const CURRENT: Self = Self(2);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An icon ready to upload: unpremultiplied RGBA rows.
#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RgbaIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The icon sets a track draws with, each keyed as the drawing code looks the
/// icon up.
#[derive(Clone, Debug, Default, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TrackIcons {
    pub ships: Vec<(String, RgbaIcon)>,
    pub planes: Vec<(String, RgbaIcon)>,
    pub buildings: Vec<(String, RgbaIcon)>,
    pub consumables: Vec<(String, RgbaIcon)>,
    pub death_causes: Vec<(String, RgbaIcon)>,
    pub powerups: Vec<(String, RgbaIcon)>,
}

/// A layer switch the viewer offers, by its [`crate::toggles::Toggle::id`],
/// and the label it shows. The label travels with the file so the viewer
/// carries no translation catalog.
#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct OfferedToggle {
    pub id: String,
    pub label: String,
}

/// A clip of a battle and everything needed to draw it.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TrackFile {
    pub frames: Vec<Vec<DrawCommand>>,
    /// The game clock each frame was drawn at, ascending.
    pub clocks: Vec<GameClock>,
    /// When the battle proper began, which the elapsed time is measured from.
    pub battle_start: GameClock,
    pub battle_end: Option<GameClock>,

    /// The map art as a PNG.
    pub map_png: Vec<u8>,
    pub icons: TrackIcons,

    pub toggles: Vec<OfferedToggle>,
}

impl TrackFile {
    /// The file's bytes.
    pub fn encode(&self) -> Result<Vec<u8>, TrackFileError> {
        let archive = rkyv::to_bytes::<rkyv::rancor::Error>(self).map_err(TrackFileError::Archive)?;

        let mut out = Vec::with_capacity(archive.len() / 4);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&TrackFileVersion::CURRENT.get().to_le_bytes());

        let mut encoder = ZlibEncoder::new(out, Compression::best());
        encoder.write_all(&archive).map_err(TrackFileError::Compress)?;

        encoder.finish().map_err(TrackFileError::Compress)
    }

    /// Reads a file written by [`Self::encode`] of the same version.
    pub fn decode(bytes: &[u8]) -> Result<Self, TrackFileError> {
        let Some(payload) = bytes.strip_prefix(&MAGIC) else {
            return Err(TrackFileError::NotATrackFile);
        };

        let Some((version, payload)) = payload.split_first_chunk::<4>() else {
            return Err(TrackFileError::NotATrackFile);
        };
        let version = TrackFileVersion::new(u32::from_le_bytes(*version));
        if version != TrackFileVersion::CURRENT {
            return Err(TrackFileError::UnsupportedVersion { found: version, supported: TrackFileVersion::CURRENT });
        }

        // rkyv reads the archive in place, which needs it aligned. One byte
        // past the limit is read so an oversized payload is told apart from
        // one exactly at it.
        let mut archive = AlignedVec::<16>::new();
        let mut decoder = ZlibDecoder::new(payload).take(MAX_PAYLOAD_BYTES as u64 + 1);
        std::io::copy(&mut decoder, &mut archive).map_err(TrackFileError::Decompress)?;
        if archive.len() > MAX_PAYLOAD_BYTES {
            return Err(TrackFileError::PayloadTooLarge { limit: MAX_PAYLOAD_BYTES });
        }

        let track = rkyv::from_bytes::<Self, rkyv::rancor::Error>(&archive).map_err(TrackFileError::Archive)?;
        track.validate()?;

        Ok(track)
    }

    /// What a viewer relies on without checking again: a frame per clock,
    /// clocks in order, and icons whose pixels match their size.
    fn validate(&self) -> Result<(), TrackFileError> {
        if self.frames.is_empty() {
            return Err(TrackFileError::NoFrames);
        }
        if self.frames.len() != self.clocks.len() {
            return Err(TrackFileError::ClockCount { frames: self.frames.len(), clocks: self.clocks.len() });
        }

        let mut previous = f32::NEG_INFINITY;
        for (index, clock) in self.clocks.iter().enumerate() {
            let seconds = clock.seconds();
            if !seconds.is_finite() || seconds < previous {
                return Err(TrackFileError::BadClock { index });
            }
            previous = seconds;
        }

        let icons = &self.icons;
        let sets =
            [&icons.ships, &icons.planes, &icons.buildings, &icons.consumables, &icons.death_causes, &icons.powerups];
        for (key, icon) in sets.into_iter().flatten() {
            let expected =
                (icon.width as usize).checked_mul(icon.height as usize).and_then(|pixels| pixels.checked_mul(4));
            if icon.width == 0 || icon.height == 0 || expected != Some(icon.rgba.len()) {
                return Err(TrackFileError::IconSize {
                    key: key.clone(),
                    width: icon.width,
                    height: icon.height,
                    bytes: icon.rgba.len(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_command::ShipVisibility;
    use crate::map_data::MinimapPos;

    fn sample() -> TrackFile {
        let ship = DrawCommand::Ship {
            entity_id: wows_replays::types::EntityId::from(7u32),
            pos: MinimapPos { x: 12.0, y: 34.0 },
            yaw: 1.5,
            species: Some("Cruiser".to_owned()),
            color: Some([10, 20, 30]),
            visibility: ShipVisibility::Visible,
            opacity: 1.0,
            is_self: true,
            player_name: Some("Player One".to_owned()),
            ship_name: Some("Des Moines".to_owned()),
            is_detected_teammate: false,
            is_disconnected: false,
            name_color: None,
        };

        TrackFile {
            frames: vec![vec![ship.clone()], vec![ship]],
            clocks: vec![GameClock(30.0), GameClock(30.5)],
            battle_start: GameClock(28.0),
            battle_end: None,
            map_png: vec![0x89, b'P', b'N', b'G'],
            icons: TrackIcons {
                ships: vec![("Cruiser".to_owned(), RgbaIcon { width: 1, height: 1, rgba: vec![1, 2, 3, 4] })],
                ..TrackIcons::default()
            },
            toggles: vec![OfferedToggle { id: "renderer-opt-trails".to_owned(), label: "Heat Trail".to_owned() }],
        }
    }

    /// What is written reads back unchanged.
    #[test]
    fn a_track_reads_back_as_it_was_written() {
        let track = sample();

        let decoded = TrackFile::decode(&track.encode().expect("encodes")).expect("decodes");

        assert_eq!(format!("{:?}", decoded.frames), format!("{:?}", track.frames));
        assert_eq!(decoded.clocks, track.clocks);
        assert_eq!(decoded.battle_start, track.battle_start);
        assert_eq!(decoded.battle_end, track.battle_end);
        assert_eq!(decoded.map_png, track.map_png);
        assert_eq!(decoded.icons, track.icons);
        assert_eq!(decoded.toggles, track.toggles);
    }

    /// A file of another layout is refused by its version rather than misread.
    #[test]
    fn another_version_is_refused() {
        let mut bytes = sample().encode().expect("encodes");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());

        let Err(TrackFileError::UnsupportedVersion { found, supported }) = TrackFile::decode(&bytes) else {
            panic!("a version 3 file is refused");
        };
        assert_eq!(found, TrackFileVersion::new(3));
        assert_eq!(supported, TrackFileVersion::CURRENT);
    }

    #[test]
    fn other_bytes_are_not_a_track() {
        assert!(matches!(TrackFile::decode(b"\x89PNG\r\n\x1a\n"), Err(TrackFileError::NotATrackFile)));
        assert!(matches!(TrackFile::decode(b"WTRK\x01"), Err(TrackFileError::NotATrackFile)));
    }

    /// A payload that inflates past the limit is refused before it is parsed.
    #[test]
    fn an_oversized_payload_is_refused() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&vec![0u8; MAX_PAYLOAD_BYTES + 1]).expect("compresses");
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&TrackFileVersion::CURRENT.get().to_le_bytes());
        bytes.extend_from_slice(&encoder.finish().expect("compresses"));

        let Err(TrackFileError::PayloadTooLarge { limit }) = TrackFile::decode(&bytes) else {
            panic!("an oversized payload is refused");
        };
        assert_eq!(limit, MAX_PAYLOAD_BYTES);
    }

    #[test]
    fn frames_without_a_clock_each_are_refused() {
        let mut track = sample();
        track.clocks.pop();

        let Err(TrackFileError::ClockCount { frames, clocks }) = TrackFile::decode(&track.encode().expect("encodes"))
        else {
            panic!("a frame without a clock is refused");
        };
        assert_eq!((frames, clocks), (2, 1));
    }

    #[test]
    fn clocks_that_run_backwards_are_refused() {
        let mut track = sample();
        track.clocks = vec![GameClock(31.0), GameClock(30.0)];

        let result = TrackFile::decode(&track.encode().expect("encodes"));

        assert!(matches!(result, Err(TrackFileError::BadClock { index: 1 })));
    }

    #[test]
    fn an_icon_whose_pixels_do_not_match_its_size_is_refused() {
        let mut track = sample();
        track.icons.planes = vec![("fighter_ally".to_owned(), RgbaIcon { width: 2, height: 2, rgba: vec![0; 15] })];

        let Err(TrackFileError::IconSize { key, bytes, .. }) = TrackFile::decode(&track.encode().expect("encodes"))
        else {
            panic!("a short icon is refused");
        };
        assert_eq!((key.as_str(), bytes), ("fighter_ally", 15));
    }
}

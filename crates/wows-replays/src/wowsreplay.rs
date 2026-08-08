use crate::error::*;
use crate::types::AccountId;
use crate::types::GameParamId;
use crate::types::PlayerId;
use rootcause::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;
use winnow::Parser;
use winnow::binary::le_u32;
use winnow::combinator::repeat;
use winnow::token::take;

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VehicleInfoMeta {
    pub shipId: GameParamId,
    pub relation: u32,
    pub id: PlayerId,
    pub name: String,
}

// Replay metadata. Fields that some game versions omit (older clients did not
// emit them, or they were added later) are typed as `Option` with
// `#[serde(default)]` so a missing key deserializes to `None` rather than
// failing the whole parse. Fields without `Option` are present in every replay
// format observed across the corpus.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplayMeta {
    /// Absent in replays from older clients (~0.5.x and early 0.6.x).
    #[serde(default)]
    pub matchGroup: Option<String>,
    pub gameMode: u32,
    #[serde(default)]
    pub gameType: Option<String>,
    pub clientVersionFromExe: String,
    /// Absent in replays from older clients (~0.6.x and earlier).
    #[serde(default)]
    pub scenarioUiCategoryId: Option<u32>,
    pub mapDisplayName: String,
    pub mapId: u32,
    pub clientVersionFromXml: String,
    /// Absent in replays from older clients (~0.6.x and earlier).
    #[serde(default)]
    pub weatherParams: Option<HashMap<String, Vec<String>>>,
    //mapBorder: Option<...>,
    pub duration: u32,
    pub gameLogic: Option<String>,
    pub name: String,
    pub scenario: String,
    pub playerID: AccountId,
    pub vehicles: Vec<VehicleInfoMeta>,
    pub playersPerTeam: u32,
    pub dateTime: String,
    pub mapName: String,
    pub playerName: String,
    pub scenarioConfigId: u32,
    pub teamsCount: u32,
    pub logic: Option<String>,
    pub playerVehicle: String,
    #[serde(default)]
    pub battleDuration: Option<u32>,
}

/// Borrowed mirror of [`VehicleInfoMeta`] for zero-copy scanning.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VehicleInfoMetaRef<'a> {
    pub shipId: GameParamId,
    pub relation: u32,
    pub id: PlayerId,
    #[serde(borrow)]
    pub name: Cow<'a, str>,
}

/// Borrowed mirror of [`ReplayMeta`]. `Cow<str>` fields borrow from the raw
/// JSON buffer (owned only when the JSON contains escapes), which makes it
/// the cheap choice for bulk directory scans. Strings nested in collections
/// or `Option` (`weatherParams`, `matchGroup`, `gameType`, `gameLogic`,
/// `logic`) are always owned; serde's zero-copy support only reaches bare
/// `Cow` fields.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplayMetaRef<'a> {
    #[serde(default)]
    pub matchGroup: Option<Cow<'a, str>>,
    pub gameMode: u32,
    #[serde(default)]
    pub gameType: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub clientVersionFromExe: Cow<'a, str>,
    #[serde(default)]
    pub scenarioUiCategoryId: Option<u32>,
    #[serde(borrow)]
    pub mapDisplayName: Cow<'a, str>,
    pub mapId: u32,
    #[serde(borrow)]
    pub clientVersionFromXml: Cow<'a, str>,
    #[serde(default)]
    pub weatherParams: Option<HashMap<String, Vec<String>>>,
    pub duration: u32,
    pub gameLogic: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(borrow)]
    pub scenario: Cow<'a, str>,
    pub playerID: AccountId,
    #[serde(borrow)]
    pub vehicles: Vec<VehicleInfoMetaRef<'a>>,
    pub playersPerTeam: u32,
    #[serde(borrow)]
    pub dateTime: Cow<'a, str>,
    #[serde(borrow)]
    pub mapName: Cow<'a, str>,
    #[serde(borrow)]
    pub playerName: Cow<'a, str>,
    pub scenarioConfigId: u32,
    pub teamsCount: u32,
    pub logic: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub playerVehicle: Cow<'a, str>,
    #[serde(default)]
    pub battleDuration: Option<u32>,
}

impl<'a> ReplayMetaRef<'a> {
    /// Parses metadata from the raw JSON blob returned by
    /// [`ReplayFile::read_meta_blob`], borrowing string fields from it.
    pub fn from_slice(blob: &'a [u8]) -> Result<Self, ParseError> {
        let raw = std::str::from_utf8(blob)?;
        Ok(serde_json::from_str(raw)?)
    }

    pub fn into_owned(self) -> ReplayMeta {
        ReplayMeta {
            matchGroup: self.matchGroup.map(Cow::into_owned),
            gameMode: self.gameMode,
            gameType: self.gameType.map(Cow::into_owned),
            clientVersionFromExe: self.clientVersionFromExe.into_owned(),
            scenarioUiCategoryId: self.scenarioUiCategoryId,
            mapDisplayName: self.mapDisplayName.into_owned(),
            mapId: self.mapId,
            clientVersionFromXml: self.clientVersionFromXml.into_owned(),
            weatherParams: self.weatherParams,
            duration: self.duration,
            gameLogic: self.gameLogic.map(Cow::into_owned),
            name: self.name.into_owned(),
            scenario: self.scenario.into_owned(),
            playerID: self.playerID,
            vehicles: self
                .vehicles
                .into_iter()
                .map(|v| VehicleInfoMeta {
                    shipId: v.shipId,
                    relation: v.relation,
                    id: v.id,
                    name: v.name.into_owned(),
                })
                .collect(),
            playersPerTeam: self.playersPerTeam,
            dateTime: self.dateTime.into_owned(),
            mapName: self.mapName.into_owned(),
            playerName: self.playerName.into_owned(),
            scenarioConfigId: self.scenarioConfigId,
            teamsCount: self.teamsCount,
            logic: self.logic.map(Cow::into_owned),
            playerVehicle: self.playerVehicle.into_owned(),
            battleDuration: self.battleDuration,
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct Replay<'a> {
    meta: ReplayMeta,
    raw_meta: &'a str,
    extra_data: Vec<&'a [u8]>,
    decompressed_size: u32,
    compressed_size: u32,
}

fn decode_meta(meta: &[u8]) -> Result<(&str, ReplayMeta), ParseError> {
    let raw_meta = std::str::from_utf8(meta)?;
    let meta: ReplayMeta = serde_json::from_str(raw_meta)?;
    Ok((raw_meta, meta))
}

fn parse_meta<'a>(i: &mut &'a [u8]) -> PResult<(&'a str, ReplayMeta)> {
    let meta_len = le_u32.parse_next(i)?;
    let raw_meta: &[u8] = take(meta_len as usize).parse_next(i)?;
    let meta = match decode_meta(raw_meta) {
        Ok(x) => x,
        Err(e) => {
            return Err(winnow::error::ErrMode::Cut(e));
        }
    };
    Ok(meta)
}

/// Parse just the file magic, block count, and metadata block, stopping before
/// the (encrypted, compressed) packet stream. Used to read replay metadata
/// without decrypting packets, and tolerant of replays whose trailing data is
/// missing or corrupt.
fn meta_only(i: &mut &[u8]) -> PResult<ReplayMeta> {
    let _magic = le_u32.parse_next(i)?;
    let _block_count = le_u32.parse_next(i)?;
    let (_raw_meta, meta) = parse_meta(i)?;
    Ok(meta)
}

fn block<'a>(i: &mut &'a [u8]) -> PResult<&'a [u8]> {
    let block_size = le_u32.parse_next(i)?;
    take(block_size as usize).parse_next(i)
}

fn replay_format<'a>(i: &mut &'a [u8]) -> PResult<Replay<'a>> {
    let _magic = le_u32.parse_next(i)?;
    let block_count = le_u32.parse_next(i)?;
    let (raw_meta, meta) = parse_meta(i)?;

    let blocks: Vec<&'a [u8]> = repeat((block_count as usize) - 1, block).parse_next(i)?;
    let decompressed_size = le_u32.parse_next(i)?;
    let compressed_size = le_u32.parse_next(i)?;
    Ok(Replay { meta, raw_meta, extra_data: blocks, decompressed_size, compressed_size })
}

const BLOWFISH_BLOCK: usize = 8;
const DECRYPT_LANES: usize = 8;

pub(crate) fn replay_blowfish() -> &'static crate::blowfish::Blowfish {
    static BLOWFISH: std::sync::OnceLock<crate::blowfish::Blowfish> = std::sync::OnceLock::new();
    BLOWFISH.get_or_init(|| {
        let key = [0x29, 0xB7, 0xC9, 0x09, 0x38, 0x3F, 0x84, 0x88, 0xFA, 0x98, 0xEC, 0x4E, 0x13, 0x19, 0x79, 0xFB];
        crate::blowfish::Blowfish::new(&key)
    })
}

/// Streaming Blowfish decryptor for the replay packet stream (ECB plus a
/// previous-plaintext XOR chain, an all-zero IV). Blocks are decrypted
/// straight into the caller's buffer, so no full-size plaintext buffer ever
/// exists. A ragged trailing sub-block is dropped: the loop walks whole
/// 8-byte blocks only, which is what makes a file the game is still writing
/// readable.
struct BlowfishXorReader<'a> {
    encrypted: &'a [u8],
    previous: u64,
    blowfish: &'static crate::blowfish::Blowfish,
}

impl<'a> BlowfishXorReader<'a> {
    fn new(encrypted: &'a [u8]) -> Self {
        Self { encrypted, previous: 0, blowfish: replay_blowfish() }
    }
}

impl Read for BlowfishXorReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // A sub-block destination buffer would falsely signal EOF below;
        // flate2's internal buffer is always much larger than one block.
        debug_assert!(buf.len() >= BLOWFISH_BLOCK || self.encrypted.len() < BLOWFISH_BLOCK);
        let block_count = (self.encrypted.len() / BLOWFISH_BLOCK).min(buf.len() / BLOWFISH_BLOCK);
        if block_count == 0 {
            return Ok(0);
        }

        let n = block_count * BLOWFISH_BLOCK;
        let lane_bytes = DECRYPT_LANES * BLOWFISH_BLOCK;
        let full = n - n % lane_bytes;
        for (src, dst) in self.encrypted[..full].chunks_exact(lane_bytes).zip(buf[..full].chunks_exact_mut(lane_bytes))
        {
            self.blowfish.decrypt_lanes::<DECRYPT_LANES>(src, dst);
        }
        for (src, dst) in
            self.encrypted[full..n].chunks_exact(BLOWFISH_BLOCK).zip(buf[full..n].chunks_exact_mut(BLOWFISH_BLOCK))
        {
            self.blowfish.decrypt_block(src.try_into().unwrap(), dst.try_into().unwrap());
        }

        // XOR chain over the decrypted blocks; cheap relative to the cipher.
        let mut previous = self.previous;
        for dst in buf[..n].chunks_exact_mut(BLOWFISH_BLOCK) {
            let plain = u64::from_ne_bytes(dst.try_into().unwrap()) ^ previous;
            dst.copy_from_slice(&plain.to_ne_bytes());
            previous = plain;
        }
        self.previous = previous;

        self.encrypted = &self.encrypted[n..];
        Ok(n)
    }
}

/// Inflate as much of the compressed stream as is intact. Any inflate error
/// ends the prefix: a stream cut mid-symbol is reported as corrupt rather
/// than as a clean EOF, so a partial write cannot be told apart from real
/// damage. The caller retries either way.
fn inflate_prefix(deflated: impl Read) -> Vec<u8> {
    let mut decoder = flate2::read::ZlibDecoder::new(deflated);
    let mut out = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    out
}

/// Packet-stream state of a [`ReplayFile`]: the stream was decrypted and
/// inflated. Only this state can hand out packet data, so an API that needs
/// packets takes `&ReplayFile` (which defaults to `ReplayFile<Full>`) and the
/// requirement is checked at compile time.
#[derive(Debug, Clone)]
pub struct Full {
    packet_data: Vec<u8>,
}

/// Packet-stream state of a [`ReplayFile`]: only the plaintext metadata
/// header was read. Produced by [`ReplayFile::meta_only_from_file`];
/// upgrade with [`ReplayFile::load_packets`].
#[derive(Debug, Clone)]
pub struct MetaOnly;

#[derive(Debug, Clone)]
pub struct ReplayFile<S = Full> {
    pub meta: ReplayMeta,
    pub raw_meta: String,
    state: S,
}

impl ReplayFile<MetaOnly> {
    /// Reads only the metadata header off disk; see
    /// [`ReplayFile::meta_from_file`] for the bare-metadata variant.
    pub fn meta_only_from_file(replay: &std::path::Path) -> rootcause::Result<ReplayFile<MetaOnly>, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let blob = ReplayFile::read_meta_blob(replay)?;
        let raw_meta = String::from_utf8(blob).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        let meta: ReplayMeta =
            serde_json::from_str(&raw_meta).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        Ok(ReplayFile { meta, raw_meta, state: MetaOnly })
    }

    /// Loads the packet stream from `replay`, upgrading to the full state.
    /// The file is re-read in whole; the metadata is re-parsed from it so the
    /// result is consistent even if the file changed since the scan.
    pub fn load_packets(self, replay: &std::path::Path) -> rootcause::Result<ReplayFile, ParseError> {
        ReplayFile::from_file(replay)
    }
}

impl ReplayFile {
    /// Assembles a replay from already-parsed metadata and packet data.
    pub fn from_parts(meta: ReplayMeta, raw_meta: String, packet_data: Vec<u8>) -> ReplayFile {
        ReplayFile { meta, raw_meta, state: Full { packet_data } }
    }

    pub fn packet_data(&self) -> &[u8] {
        &self.state.packet_data
    }

    pub fn into_parts(self) -> (ReplayMeta, String, Vec<u8>) {
        (self.meta, self.raw_meta, self.state.packet_data)
    }

    /// Assemble a replay from a metadata blob and a packet stream that were
    /// read separately.
    ///
    /// This is what a battle in progress needs. The game writes
    /// `temp.wowsreplay` as the bare packet stream, the same bytes a finished
    /// replay carries once decrypted and inflated, with no container around
    /// them and no metadata block; the metadata lives in the sibling
    /// `tempArenaInfo.json` until the battle ends and the two are wrapped into
    /// a replay file.
    pub fn from_decrypted_parts(meta: Vec<u8>, packet_data: Vec<u8>) -> Result<ReplayFile, ParseError> {
        let (_raw_meta, parsed_meta) = decode_meta(meta.as_slice())?;

        let raw_meta = String::from_utf8(meta)?;

        Ok(ReplayFile::from_parts(parsed_meta, raw_meta, packet_data))
    }

    /// Parse a replay entirely from an in-memory byte slice (sans-io).
    ///
    /// Parses the file header, then Blowfish-CBC decrypts and zlib-decompresses
    /// the trailing packet stream. Use this in environments without filesystem
    /// access (wasm, embedded); [`ReplayFile::from_file`] is a thin wrapper.
    pub fn from_bytes(bytes: &[u8]) -> rootcause::Result<ReplayFile, ParseError> {
        let mut input = bytes;
        let result = replay_format(&mut input).map_err(|e| report!(ParseError::from(e)))?;

        // Decrypt lazily inside the inflate loop; see BlowfishXorReader.
        let mut deflater = flate2::read::ZlibDecoder::new(BlowfishXorReader::new(input));
        // decompressed_size comes from the file; clamp the reservation so a
        // corrupt header cannot force a huge allocation up front.
        const MAX_PREALLOC: usize = 512 * 1024 * 1024;
        let mut packet_data = Vec::with_capacity((result.decompressed_size as usize).min(MAX_PREALLOC));
        deflater.read_to_end(&mut packet_data).map_err(|e| report!(ParseError::from(e)))?;

        Ok(ReplayFile::from_parts(result.meta, result.raw_meta.to_string(), packet_data))
    }

    pub fn from_file(replay: &std::path::Path) -> rootcause::Result<ReplayFile, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let mut f = std::fs::File::options()
            .read(true)
            .open(replay)
            .map_err(|e| report!(ParseError::from(e)))
            .attach_with(path_context)?;
        let mut contents = vec![];
        f.read_to_end(&mut contents).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;

        Self::from_bytes(&contents).attach_with(path_context)
    }

    /// Parse a replay whose packet stream may still be being written.
    ///
    /// Identical to [`ReplayFile::from_bytes`] except that an unfinished zlib
    /// stream yields the packets that did inflate instead of an error. Use this
    /// for `temp.wowsreplay` during a battle; use `from_bytes` everywhere else,
    /// where a truncated stream means a corrupt file and should be reported.
    pub fn from_partial_bytes(bytes: &[u8]) -> rootcause::Result<ReplayFile, ParseError> {
        let mut input = bytes;
        let result = replay_format(&mut input).map_err(|e| report!(ParseError::from(e)))?;
        let packet_data = inflate_prefix(BlowfishXorReader::new(input));

        Ok(ReplayFile::from_parts(result.meta, result.raw_meta.to_string(), packet_data))
    }

    /// Read [`ReplayFile::from_partial_bytes`] from a file on disk.
    pub fn from_partial_file(replay: &std::path::Path) -> rootcause::Result<ReplayFile, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let mut f = std::fs::File::options()
            .read(true)
            .open(replay)
            .map_err(|e| report!(ParseError::from(e)))
            .attach_with(path_context)?;
        let mut contents = vec![];
        f.read_to_end(&mut contents).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;

        Self::from_partial_bytes(&contents).attach_with(path_context)
    }

    /// Parse only the replay metadata header, skipping decryption and
    /// decompression of the packet stream.
    ///
    /// The metadata block (player, map, game version, etc.) is stored in
    /// plaintext at the start of the file, so this is much cheaper than
    /// [`ReplayFile::from_bytes`] and still succeeds when the encrypted packet
    /// stream is truncated or corrupt: only the leading magic, block count, and
    /// metadata block are parsed.
    pub fn meta_from_bytes(bytes: &[u8]) -> rootcause::Result<ReplayMeta, ParseError> {
        let mut input = bytes;
        let meta = meta_only(&mut input).map_err(|e| report!(ParseError::from(e)))?;
        Ok(meta)
    }

    /// Reads only the wrapper header and raw JSON metadata blob off disk, not
    /// the (potentially many megabytes of) packet stream that follows. Parse
    /// the result with [`ReplayMetaRef::from_slice`] (zero-copy) or
    /// [`ReplayFile::meta_from_file`] (owned).
    pub fn read_meta_blob(replay: &std::path::Path) -> rootcause::Result<Vec<u8>, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let mut f = std::fs::File::open(replay).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        let file_len = f.metadata().map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?.len();

        // Layout: magic (u32) | block_count (u32) | meta_len (u32) | meta bytes.
        let mut header = [0u8; 12];
        f.read_exact(&mut header).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        let meta_len = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as u64;
        if meta_len > file_len.saturating_sub(12) {
            return Err(report!(ParseError::InvalidMetaLength { meta_len, file_len })).attach_with(path_context);
        }

        let mut meta = vec![0u8; meta_len as usize];
        f.read_exact(&mut meta).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        Ok(meta)
    }

    /// Read owned metadata from a file on disk via [`ReplayFile::read_meta_blob`].
    ///
    /// Only the file header and metadata block are read off disk, not the
    /// (potentially many megabytes of) packet stream that follows.
    pub fn meta_from_file(replay: &std::path::Path) -> rootcause::Result<ReplayMeta, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let blob = Self::read_meta_blob(replay)?;
        let raw = std::str::from_utf8(&blob).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        let meta: ReplayMeta =
            serde_json::from_str(raw).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        Ok(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zlib stream cut off mid-way, which is what a battle in progress
    /// leaves on disk. `from_bytes` must reject it and `from_partial_bytes`
    /// must keep the prefix that did inflate.
    #[test]
    fn a_truncated_stream_inflates_up_to_the_cut() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&payload).expect("write payload");
        encoder.flush().expect("sync flush emits everything written so far");
        let full = encoder.finish().expect("finish stream");
        let truncated = &full[..full.len() - 8];

        let complete = inflate_prefix(full.as_slice());
        let partial = inflate_prefix(truncated);

        assert_eq!(complete, payload);
        assert!(!partial.is_empty(), "a flushed prefix must inflate to something");
        assert!(payload.starts_with(&partial), "the prefix must be a prefix of the payload");
    }
}

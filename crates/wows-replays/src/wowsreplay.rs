use crate::error::*;
use crate::types::AccountId;
use crate::types::GameParamId;
use blowfish::Blowfish;
use byteorder::BE;
use cipher::BlockDecrypt;
use cipher::KeyInit;
use cipher::generic_array::GenericArray;
use rootcause::prelude::*;
use serde::Deserialize;
use serde::Serialize;
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
    pub id: AccountId,
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

#[derive(Debug, Clone)]
pub struct ReplayFile {
    pub meta: ReplayMeta,
    pub raw_meta: String,
    pub packet_data: Vec<u8>,
}

impl ReplayFile {
    pub fn from_decrypted_parts(meta: Vec<u8>, packet_data: Vec<u8>) -> Result<ReplayFile, ParseError> {
        let (_raw_meta, parsed_meta) = decode_meta(meta.as_slice())?;

        let raw_meta = String::from_utf8(meta)?;

        Ok(ReplayFile { meta: parsed_meta, raw_meta, packet_data })
    }

    /// Parse a replay entirely from an in-memory byte slice (sans-io).
    ///
    /// Parses the file header, then Blowfish-CBC decrypts and zlib-decompresses
    /// the trailing packet stream. Use this in environments without filesystem
    /// access (wasm, embedded); [`ReplayFile::from_file`] is a thin wrapper.
    pub fn from_bytes(bytes: &[u8]) -> rootcause::Result<ReplayFile, ParseError> {
        let mut input = bytes;
        let result = replay_format(&mut input).map_err(|e| report!(ParseError::from(e)))?;
        let encrypted = input;

        let key = [0x29, 0xB7, 0xC9, 0x09, 0x38, 0x3F, 0x84, 0x88, 0xFA, 0x98, 0xEC, 0x4E, 0x13, 0x19, 0x79, 0xFB];
        let cipher = <Blowfish<BE>>::new_from_slice(&key).expect("16-byte key is valid for Blowfish");

        // CBC decrypt: each plaintext block is xored with the previous ciphertext
        // block (the WoWs replay format uses an all-zero IV).
        let mut decrypted = vec![0u8; encrypted.len()];
        let mut previous = [0u8; 8];
        for chunk_idx in 0..(encrypted.len() / 8) {
            let off = chunk_idx * 8;
            let mut block = GenericArray::clone_from_slice(&encrypted[off..off + 8]);
            cipher.decrypt_block(&mut block);
            for j in 0..8 {
                decrypted[off + j] = block[j] ^ previous[j];
            }
            previous.copy_from_slice(&decrypted[off..off + 8]);
        }

        let mut deflater = flate2::read::ZlibDecoder::new(decrypted.as_slice());
        let mut packet_data = vec![];
        deflater.read_to_end(&mut packet_data).map_err(|e| report!(ParseError::from(e)))?;

        Ok(ReplayFile { meta: result.meta, raw_meta: result.raw_meta.to_string(), packet_data })
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

    /// Read [`ReplayFile::meta_from_bytes`] from a file on disk.
    ///
    /// Only the file header and metadata block are read off disk, not the
    /// (potentially many megabytes of) packet stream that follows.
    pub fn meta_from_file(replay: &std::path::Path) -> rootcause::Result<ReplayMeta, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let mut f = std::fs::File::options()
            .read(true)
            .open(replay)
            .map_err(|e| report!(ParseError::from(e)))
            .attach_with(path_context)?;

        // Layout: magic (u32) | block_count (u32) | meta_len (u32) | meta bytes.
        let mut header = [0u8; 12];
        f.read_exact(&mut header).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        let meta_len = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;

        let mut buffer = Vec::with_capacity(12 + meta_len);
        buffer.extend_from_slice(&header);
        let mut meta_bytes = vec![0u8; meta_len];
        f.read_exact(&mut meta_bytes).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        buffer.extend_from_slice(&meta_bytes);

        Self::meta_from_bytes(&buffer).attach_with(path_context)
    }
}

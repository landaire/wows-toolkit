use crate::error::*;
use crate::types::AccountId;
use crate::types::GameParamId;
use crypto::symmetriccipher::BlockDecryptor;
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

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplayMeta {
    pub matchGroup: String,
    pub gameMode: u32,
    pub gameType: String,
    pub clientVersionFromExe: String,
    pub scenarioUiCategoryId: u32,
    pub mapDisplayName: String,
    pub mapId: u32,
    pub clientVersionFromXml: String,
    pub weatherParams: HashMap<String, Vec<String>>,
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
    pub battleDuration: u32,
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

#[derive(Debug)]
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

    pub fn from_file(replay: &std::path::Path) -> rootcause::Result<ReplayFile, ParseError> {
        let path_context = || format!("path: {}", replay.display());

        let mut f = std::fs::File::options()
            .read(true)
            .open(replay)
            .map_err(|e| report!(ParseError::from(e)))
            .attach_with(path_context)?;
        let mut contents = vec![];
        f.read_to_end(&mut contents).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;

        let mut input = &contents[..];
        let result = replay_format(&mut input).map_err(|e| report!(ParseError::from(e))).attach_with(path_context)?;
        let remaining = input;

        // Decrypt
        let key = [0x29, 0xB7, 0xC9, 0x09, 0x38, 0x3F, 0x84, 0x88, 0xFA, 0x98, 0xEC, 0x4E, 0x13, 0x19, 0x79, 0xFB];
        let blowfish = crypto::blowfish::Blowfish::new(&key);
        assert!(blowfish.block_size() == 8);
        let encrypted = remaining;
        let mut decrypted = vec![0; encrypted.len()];
        let num_blocks = encrypted.len() / blowfish.block_size();
        let mut previous = [0; 8]; // 8 == block size
        for i in 0..num_blocks {
            let offset = i * blowfish.block_size();
            blowfish.decrypt_block(
                &encrypted[offset..offset + blowfish.block_size()],
                &mut decrypted[offset..offset + blowfish.block_size()],
            );
            for j in 0..8 {
                decrypted[offset + j] ^= previous[j];
            }
            previous.copy_from_slice(&decrypted[offset..offset + 8]);
        }

        let mut deflater = flate2::read::ZlibDecoder::new(decrypted.as_slice());
        let mut contents = vec![];
        deflater.read_to_end(&mut contents).unwrap();

        Ok(ReplayFile { meta: result.meta, raw_meta: result.raw_meta.to_string(), packet_data: contents })
    }
}

//! Versioned on-disk cache for parsed GameParams.
//!
//! Wraps an rkyv-serialized `Vec<Param>` payload with a small header so the
//! reader can detect and reject caches written by an older parser. Bump
//! [`FORMAT_VERSION`] whenever the parsing logic in this crate changes in a
//! way that affects the cached payload but doesn't change the rkyv schema
//! (e.g. fixing a parser bug that was silently dropping fields).

use std::fs;
use std::io;
use std::path::Path;

use crate::game_params::types::Param;

const MAGIC: [u8; 4] = *b"WUGP";

/// Bump on parser-logic changes that would invalidate previously-written
/// caches. New writes always carry the latest version; reads that see an
/// older or unknown version return `None`, prompting the caller to
/// re-parse from the source VFS.
pub const FORMAT_VERSION: u32 = 7;

const HEADER_LEN: usize = MAGIC.len() + std::mem::size_of::<u32>();

/// Encode `params` as a versioned cache byte sequence. Use when the caller
/// stores the bytes somewhere other than a plain file (e.g. content-addressed
/// storage). For direct file writes, prefer [`save`].
pub fn encode(params: &[Param]) -> io::Result<Vec<u8>> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&params.to_vec())
        .map_err(|e| io::Error::other(format!("rkyv serialize: {e}")))?;
    let mut buf = Vec::with_capacity(HEADER_LEN + payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Decode a versioned cache byte sequence into `Vec<Param>`.
///
/// Returns `None` if the magic doesn't match, the version isn't
/// [`FORMAT_VERSION`], or rkyv deserialization fails. The caller should fall
/// back to re-parsing from the source VFS.
pub fn decode(bytes: &[u8]) -> Option<Vec<Param>> {
    if bytes.len() < HEADER_LEN {
        tracing::debug!("game-params cache rejected: byte sequence shorter than header");
        return None;
    }
    if bytes[..MAGIC.len()] != MAGIC {
        tracing::debug!("game-params cache rejected: missing WUGP magic (pre-versioned format)");
        return None;
    }
    let version_bytes: [u8; 4] = bytes[MAGIC.len()..HEADER_LEN].try_into().ok()?;
    let version = u32::from_le_bytes(version_bytes);
    if version != FORMAT_VERSION {
        tracing::debug!(
            file_version = version,
            current = FORMAT_VERSION,
            "game-params cache rejected: format version mismatch"
        );
        return None;
    }
    let payload = &bytes[HEADER_LEN..];
    rkyv::from_bytes::<Vec<Param>, rkyv::rancor::Error>(payload).ok()
}

/// Read a cached `Vec<Param>` from `path`. Thin wrapper over [`decode`].
pub fn load(path: &Path) -> Option<Vec<Param>> {
    let bytes = fs::read(path).ok()?;
    decode(&bytes)
}

/// Write `params` to `path` with the current header.
pub fn save(path: &Path, params: &[Param]) -> io::Result<()> {
    let buf = encode(params)?;
    fs::write(path, &buf)
}

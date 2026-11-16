use crate::data::idx::VfsEntry;
use std::collections::HashMap;

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SerializedFile {
    pub path: String,
    is_directory: bool,
    compressed_size: usize,
    compression_info: u64,
    unpacked_size: usize,
    crc32: u32,
}

impl SerializedFile {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    pub fn compressed_size(&self) -> usize {
        self.compressed_size
    }

    pub fn compression_info(&self) -> u64 {
        self.compression_info
    }

    pub fn unpacked_size(&self) -> usize {
        self.unpacked_size
    }

    pub fn crc32(&self) -> u32 {
        self.crc32
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SerializedFileInfo {}

pub fn tree_to_serialized_files(entries: &HashMap<String, VfsEntry>) -> Vec<SerializedFile> {
    let mut out = Vec::with_capacity(entries.len());

    for (path, entry) in entries {
        let (is_directory, compressed_size, compression_info, unpacked_size, crc32) = match entry {
            VfsEntry::File { file_info, .. } => (
                false,
                file_info.size as usize,
                file_info.compression_info,
                file_info.unpacked_size as usize,
                file_info.crc32,
            ),
            VfsEntry::Directory => (true, 0, 0, 0, 0),
        };

        out.push(SerializedFile {
            path: path.clone(),
            is_directory,
            compressed_size,
            compression_info,
            unpacked_size,
            crc32,
        });
    }

    // Already sorted since BTreeMap iterates in order
    out
}

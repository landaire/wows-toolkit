//! Reading a VFS file for the in-app viewers.
//!
//! Which files can be viewed, and as what, is the same decision in both front
//! ends; how the result is drawn is not.

use std::io::Read as _;
use wowsunpack::data::assets_bin_vfs::PrototypeType;
use wowsunpack::vfs::VfsPath;

use super::listing::FileKind;

/// A file's contents, in the shape its viewer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerContent {
    Plaintext {
        /// Extension including the dot, which is what a highlighter keys on.
        extension: String,
        text: String,
    },
    Image {
        /// Undecoded bytes: each front end decodes into its own image type.
        bytes: Vec<u8>,
    },
}

/// The prototype an assets.bin entry decodes to, when its extension names one
/// the decoder understands.
///
/// `None` for every other file, including assets.bin entries whose type is
/// recognised but not yet decodable.
pub fn decodable_prototype(file_name: &str) -> Option<PrototypeType> {
    let dot = file_name.rfind('.')?;
    let prototype = PrototypeType::from_extension(&file_name[dot..])?;
    wowsunpack::models::can_decode_prototype(prototype).then_some(prototype)
}

/// Decodes one assets.bin entry to JSON.
pub fn decode_to_json(path: &VfsPath) -> Result<String, ViewerError> {
    let name = path.filename();
    let Some(prototype) = decodable_prototype(&name) else {
        return Err(ViewerError::NotDecodable { name });
    };

    let mut bytes = Vec::new();
    path.open_file()
        .and_then(|mut file| {
            file.read_to_end(&mut bytes)?;
            Ok(())
        })
        .map_err(|source| ViewerError::Read { name: name.clone(), source })?;

    wowsunpack::models::decode_prototype_to_json(&bytes, prototype)
        .map_err(|err| ViewerError::Decode { name, reason: err.to_string() })
}

#[derive(Debug, thiserror::Error)]
pub enum ViewerError {
    #[error("{name} has no in-app viewer")]
    Unviewable { name: String },
    #[error("{name} could not be read")]
    Read {
        name: String,
        #[source]
        source: wowsunpack::vfs::VfsError,
    },
    #[error("{name} is not valid UTF-8, so it cannot be shown as text")]
    NotText { name: String },
    #[error("{name} is not a prototype this build can decode")]
    NotDecodable { name: String },
    #[error("{name} could not be decoded: {reason}")]
    Decode { name: String, reason: String },
}

/// Reads `path` for viewing.
///
/// A file with no viewer is refused rather than opened empty, and a text file
/// that is not UTF-8 says so rather than rendering replacement characters.
pub fn load(path: &VfsPath) -> Result<ViewerContent, ViewerError> {
    let name = path.filename();
    let kind = FileKind::of(&name);
    if kind == FileKind::Opaque {
        return Err(ViewerError::Unviewable { name });
    }

    let mut bytes = Vec::new();
    path.open_file()
        .and_then(|mut file| {
            file.read_to_end(&mut bytes)?;
            Ok(())
        })
        .map_err(|source| ViewerError::Read { name: name.clone(), source })?;

    match kind {
        FileKind::Image => Ok(ViewerContent::Image { bytes }),
        FileKind::Plaintext => {
            let text = String::from_utf8(bytes).map_err(|_| ViewerError::NotText { name: name.clone() })?;
            let extension = name.rfind('.').map(|dot| name[dot..].to_string()).unwrap_or_default();
            Ok(ViewerContent::Plaintext { extension, text })
        }
        FileKind::Opaque => unreachable!("refused above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use wowsunpack::vfs::MemoryFS;

    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("res").unwrap().create_dir_all().unwrap();
        for (path, body) in [("res/a.xml", b"<a/>".to_vec()), ("res/b.png", vec![0x89, b'P', b'N', b'G'])] {
            root.join(path).unwrap().create_file().unwrap().write_all(&body).unwrap();
        }
        root.join("res/bad.txt").unwrap().create_file().unwrap().write_all(&[0xff, 0xfe]).unwrap();
        root.join("res/model.geometry").unwrap().create_file().unwrap().write_all(b"binary").unwrap();
        root
    }

    #[test]
    fn a_text_file_loads_as_text_with_its_extension() {
        let loaded = load(&fixture().join("res/a.xml").unwrap()).unwrap();
        assert_eq!(loaded, ViewerContent::Plaintext { extension: ".xml".into(), text: "<a/>".into() });
    }

    #[test]
    fn an_image_loads_as_undecoded_bytes() {
        let loaded = load(&fixture().join("res/b.png").unwrap()).unwrap();
        assert!(matches!(loaded, ViewerContent::Image { bytes } if bytes.starts_with(&[0x89, b'P'])));
    }

    #[test]
    fn a_file_with_no_viewer_is_refused_by_name_rather_than_read() {
        let error = load(&fixture().join("res/model.geometry").unwrap()).expect_err("no viewer for this");
        assert!(matches!(error, ViewerError::Unviewable { .. }));
    }

    #[test]
    fn a_text_file_that_is_not_utf8_says_so_rather_than_showing_replacements() {
        let error = load(&fixture().join("res/bad.txt").unwrap()).expect_err("the bytes are not text");
        assert!(matches!(error, ViewerError::NotText { .. }));
    }

    #[test]
    fn a_missing_file_reports_the_read_failure() {
        let error = load(&fixture().join("res/nope.xml").unwrap()).expect_err("there is no such file");
        assert!(matches!(error, ViewerError::Read { .. }));
    }
}

#[cfg(test)]
mod prototype_tests {
    use super::ViewerError;
    use super::decodable_prototype;
    use super::decode_to_json;
    use std::io::Write as _;
    use wowsunpack::vfs::MemoryFS;
    use wowsunpack::vfs::VfsPath;

    #[test]
    fn a_file_whose_extension_names_no_prototype_is_not_decodable() {
        assert!(decodable_prototype("notes.txt").is_none());
        assert!(decodable_prototype("noextension").is_none());
    }

    #[test]
    fn asking_to_decode_an_ordinary_file_says_so_rather_than_reading_it() {
        let root: VfsPath = MemoryFS::new().into();
        root.join("a.txt").unwrap().create_file().unwrap().write_all(b"hello").unwrap();

        let error = decode_to_json(&root.join("a.txt").unwrap()).expect_err("txt is not a prototype");
        assert!(matches!(error, ViewerError::NotDecodable { .. }));
    }
}

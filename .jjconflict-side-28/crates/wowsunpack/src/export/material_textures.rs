//! The textures a material references, and the files in the packages that hold each one.
//!
//! The game looks a texture up by its VFS path, so a file placed at the same path under the
//! mod folder replaces it. A texture is stored as a `.dds` holding the smaller mips, with up to
//! three larger single-mip files (`.dd0`, `.dd1`, `.dd2`) beside it; see [`TextureLod`].
//!
//! [`TextureLod`]: super::texture::TextureLod

use std::fmt;
use std::io::Cursor;

use thiserror::Error;
use vfs::VfsPath;

use crate::models::material::PropertyValue;

use super::model_library::ModelLibrary;
use super::texture;

/// A texture-valued property of a material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialTextureRef {
    pub property: PropertyName,
    pub texture: TextureReference,
}

/// A material property's name, when the built-in name table knows its hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyName {
    Known(&'static str),
    /// MurmurHash3 of a name the table does not hold.
    Unknown(u32),
}

impl fmt::Display for PropertyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(name) => f.write_str(name),
            Self::Unknown(hash) => write!(f, "0x{hash:08X}"),
        }
    }
}

/// What a texture property points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureReference {
    /// A VFS path, without a leading slash.
    Path(String),
    /// A path id the `assets.bin` path store does not hold.
    Unresolved(u64),
}

/// The pixel format of a DDS file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureFormat(image_dds::ImageFormat);

impl TextureFormat {
    pub fn image_format(self) -> image_dds::ImageFormat {
        self.0
    }
}

impl fmt::Display for TextureFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The variant names are the format names used by DirectX tools, such as `BC7RgbaUnormSrgb`.
        write!(f, "{:?}", self.0)
    }
}

/// Width and height of a texture's top mip, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSize {
    pub width: u32,
    pub height: u32,
}

impl fmt::Display for TextureSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// One file of a texture in the packages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureFile {
    /// VFS path, without a leading slash.
    pub path: String,
    pub size: TextureSize,
    /// Unpacked length in bytes.
    pub bytes: u64,
}

/// The files that hold one texture, and what the `.dds` header says of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureFiles {
    /// `None` for a format the DDS reader does not recognize.
    pub format: Option<TextureFormat>,
    /// Mip levels stored in the `.dds`.
    pub mip_count: u32,
    /// Largest first, the referenced file last.
    pub files: Vec<TextureFile>,
}

impl TextureFiles {
    /// The size the game shows at its highest texture quality.
    pub fn largest(&self) -> Option<TextureSize> {
        self.files.first().map(|file| file.size)
    }
}

/// Why a texture's files could not be described.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TextureFilesError {
    #[error("{path} is not in the installed packages")]
    Absent { path: String },
    #[error("{path} has no readable DDS header")]
    Header { path: String },
}

impl ModelLibrary {
    /// Every texture the material at `mfm_path_id` references, in record order. `None` when the
    /// material record does not read.
    pub fn material_textures(&self, mfm_path_id: u64) -> Option<Vec<MaterialTextureRef>> {
        let db = self.db();
        let material = texture::parse_mfm_from_db(&db, mfm_path_id)?;
        let index = self.self_id_index();

        let textures = material
            .properties
            .iter()
            .filter_map(|property| {
                let Some(PropertyValue::Texture(path_id)) = property.value else {
                    return None;
                };
                let name = match property.name {
                    Some(name) => PropertyName::Known(name),
                    None => PropertyName::Unknown(property.name_hash),
                };
                let texture = match index.get(&path_id) {
                    Some(&position) => TextureReference::Path(db.reconstruct_path(position, index)),
                    None => TextureReference::Unresolved(path_id),
                };

                Some(MaterialTextureRef { property: name, texture })
            })
            .collect();

        Some(textures)
    }

    /// The albedo the viewer finds by the material's file name, for a material whose
    /// `diffuseMap` is absent or names a file the packages lack.
    pub fn name_matched_albedo(&self, mfm_path: &str) -> Option<String> {
        texture::base_albedo_path(self.vfs(), mfm_path)
    }
}

/// The files holding the texture at `path`: the larger single-mip files present beside a `.dds`,
/// then the file itself. Reads only `path`; the larger files are sized from its header, since
/// each one doubles the one below it.
pub fn texture_files(vfs: &VfsPath, path: &str) -> Result<TextureFiles, TextureFilesError> {
    let absent = || TextureFilesError::Absent { path: path.to_string() };
    let bytes = texture::read_vfs_file(vfs, path).ok_or_else(absent)?;
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(&bytes))
        .map_err(|_| TextureFilesError::Header { path: path.to_string() })?;
    let size = TextureSize { width: dds.get_width(), height: dds.get_height() };
    let format = image_dds::dds_image_format(&dds).ok().map(TextureFormat);

    let mut files = Vec::with_capacity(texture::DDS_TIER_SUFFIXES.len() + 1);
    if let Some(stem) = path.strip_suffix(".dds") {
        let tiers: Vec<(String, u64)> = texture::DDS_TIER_SUFFIXES
            .into_iter()
            .filter_map(|suffix| {
                let tier = format!("{stem}.{suffix}");
                let bytes = file_len(vfs, &tier)?;
                Some((tier, bytes))
            })
            .collect();
        let count = tiers.len() as u32;
        for (rank, (tier, bytes)) in tiers.into_iter().enumerate() {
            let doublings = count - rank as u32;
            let size = TextureSize {
                width: size.width.checked_shl(doublings).unwrap_or(u32::MAX),
                height: size.height.checked_shl(doublings).unwrap_or(u32::MAX),
            };
            files.push(TextureFile { path: tier, size, bytes });
        }
    }
    files.push(TextureFile { path: path.to_string(), size, bytes: bytes.len() as u64 });

    Ok(TextureFiles { format, mip_count: dds.get_num_mipmap_levels().max(1), files })
}

/// The unpacked length of the file at `path`, `None` when it is absent or empty.
fn file_len(vfs: &VfsPath, path: &str) -> Option<u64> {
    let file = vfs.join(path).ok()?;
    if !file.is_file().ok()? {
        return None;
    }
    let len = file.metadata().ok()?.len;

    (len > 0).then_some(len)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image_dds::ddsfile::AlphaMode;
    use image_dds::ddsfile::D3D10ResourceDimension;
    use image_dds::ddsfile::Dds;
    use image_dds::ddsfile::DxgiFormat;
    use image_dds::ddsfile::NewDxgiParams;

    use super::*;

    fn dds_bytes(width: u32, height: u32, mips: u32) -> Vec<u8> {
        let dds = Dds::new_dxgi(NewDxgiParams {
            height,
            width,
            depth: None,
            format: DxgiFormat::BC1_UNorm_sRGB,
            mipmap_levels: Some(mips),
            array_layers: None,
            caps2: None,
            is_cubemap: false,
            resource_dimension: D3D10ResourceDimension::Texture2D,
            alpha_mode: AlphaMode::Unknown,
        })
        .expect("a BC1 header");
        let mut bytes = Vec::new();
        dds.write(&mut bytes).expect("the header writes");

        bytes
    }

    fn write(root: &VfsPath, path: &str, bytes: &[u8]) {
        let file = root.join(path).unwrap();
        file.parent().create_dir_all().unwrap();
        file.create_file().unwrap().write_all(bytes).unwrap();
    }

    #[test]
    fn a_texture_lists_its_larger_files_first() {
        let root = VfsPath::new(vfs::MemoryFS::new());
        write(&root, "content/ship/hull_a.dds", &dds_bytes(512, 256, 10));
        write(&root, "content/ship/hull_a.dd0", &[0; 40]);
        write(&root, "content/ship/hull_a.dd1", &[0; 20]);
        // An empty tier is as absent as an unlisted one.
        write(&root, "content/ship/hull_a.dd2", &[]);

        insta::assert_debug_snapshot!(texture_files(&root, "content/ship/hull_a.dds"));
    }

    #[test]
    fn a_missing_or_unreadable_texture_says_which() {
        let root = VfsPath::new(vfs::MemoryFS::new());
        write(&root, "content/broken_a.dds", b"not a dds");

        assert_eq!(
            texture_files(&root, "content/absent_a.dds"),
            Err(TextureFilesError::Absent { path: "content/absent_a.dds".to_string() })
        );
        assert_eq!(
            texture_files(&root, "content/broken_a.dds"),
            Err(TextureFilesError::Header { path: "content/broken_a.dds".to_string() })
        );
    }
}

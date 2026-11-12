//! The normal and metal-gloss maps a material names, decoded as stored, and
//! the encodings glTF reads them in.

use std::collections::HashMap;

use image_dds::image::Rgba;
use image_dds::image::RgbaImage;
use rootcause::Report;

use crate::models::assets_bin::PrototypeDatabase;
use crate::models::material::MaterialPrototype;

use super::texture;
use super::texture::TextureError;
use super::texture::TextureLod;

/// A tangent-space normal map as stored: R and G hold X and Y, with +Y up
/// the image as glTF has it. B holds an unrelated mask, so Z is rebuilt from
/// X and Y wherever a normal is needed.
pub struct NormalMap(RgbaImage);

impl NormalMap {
    pub fn new(image: RgbaImage) -> Self {
        Self(image)
    }

    pub fn image(&self) -> &RgbaImage {
        &self.0
    }

    pub fn into_image(self) -> RgbaImage {
        self.0
    }

    /// The map as glTF `normalTexture` reads it: X and Y as stored, Z rebuilt
    /// into B, alpha opaque.
    pub fn to_gltf(&self) -> RgbaImage {
        let mut out = self.0.clone();
        for pixel in out.pixels_mut() {
            let x = unorm_to_snorm(pixel[0]);
            let y = unorm_to_snorm(pixel[1]);
            let z = (1.0 - x * x - y * y).max(0.0).sqrt();
            *pixel = Rgba([pixel[0], pixel[1], snorm_to_unorm(z), u8::MAX]);
        }

        out
    }
}

/// A metal-gloss map as stored: R is gloss, G is metalness. B is an on/off
/// mask of unknown purpose. The files carry no color space flag (legacy DXT1
/// headers), so the values are read as stored.
pub struct MetalGlossMap(RgbaImage);

impl MetalGlossMap {
    pub fn new(image: RgbaImage) -> Self {
        Self(image)
    }

    pub fn image(&self) -> &RgbaImage {
        &self.0
    }

    pub fn into_image(self) -> RgbaImage {
        self.0
    }

    /// The map as glTF `metallicRoughnessTexture` reads it: G is roughness
    /// (one minus gloss), B is metalness. R (occlusion in a packed map) and
    /// alpha are left at one.
    pub fn to_gltf_metallic_roughness(&self) -> RgbaImage {
        let mut out = self.0.clone();
        for pixel in out.pixels_mut() {
            *pixel = Rgba([u8::MAX, u8::MAX - pixel[0], pixel[1], u8::MAX]);
        }

        out
    }
}

/// The surface detail maps one material names, each absent when the
/// material names none or the VFS lacks the file.
pub struct SurfaceMaps {
    pub normal: Option<NormalMap>,
    pub metal_gloss: Option<MetalGlossMap>,
}

impl SurfaceMaps {
    pub fn is_empty(&self) -> bool {
        self.normal.is_none() && self.metal_gloss.is_none()
    }

    /// Both maps as glTF reads them, PNG encoded.
    pub fn to_gltf_pngs(&self) -> Result<GltfSurfacePngs, Report<TextureError>> {
        let normal = self.normal.as_ref().map(|map| texture::encode_png(&map.to_gltf())).transpose()?;
        let metal_roughness =
            self.metal_gloss.as_ref().map(|map| texture::encode_png(&map.to_gltf_metallic_roughness())).transpose()?;

        Ok(GltfSurfacePngs { normal, metal_roughness })
    }
}

/// A material's surface maps PNG encoded in glTF's channel layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GltfSurfacePngs {
    /// `normalTexture`: X, Y, Z in R, G, B.
    pub normal: Option<Vec<u8>>,
    /// `metallicRoughnessTexture`: roughness in G, metalness in B.
    pub metal_roughness: Option<Vec<u8>>,
}

impl GltfSurfacePngs {
    pub fn is_empty(&self) -> bool {
        self.normal.is_none() && self.metal_roughness.is_none()
    }
}

/// The VFS paths of the surface maps a material names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceMapPaths {
    pub normal: Option<String>,
    pub metal_gloss: Option<String>,
}

impl SurfaceMapPaths {
    /// A material that names no map.
    pub const NONE: Self = Self { normal: None, metal_gloss: None };

    /// The paths `material`'s `normalMap` and `metallicGlossMap` name.
    pub fn of_material(
        material: &MaterialPrototype,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
    ) -> Self {
        let path_of = |property: &str| {
            let hash = material.get_texture_hash(property)?;
            let &position = self_id_index.get(&hash)?;

            Some(db.reconstruct_path(position, self_id_index))
        };

        Self { normal: path_of("normalMap"), metal_gloss: path_of("metallicGlossMap") }
    }

    /// Reads and decodes each named map at `lod`. A map the VFS lacks or
    /// that does not decode is absent.
    pub fn load(&self, vfs: &vfs::VfsPath, lod: TextureLod) -> SurfaceMaps {
        let decode = |path: &Option<String>| {
            let bytes = texture::load_dds_from_vfs(vfs, path.as_deref()?, lod)?;
            texture::decode_dds(&bytes, lod).ok()
        };

        SurfaceMaps {
            normal: decode(&self.normal).map(NormalMap),
            metal_gloss: decode(&self.metal_gloss).map(MetalGlossMap),
        }
    }
}

/// A stored unsigned byte as a component in [-1, 1].
fn unorm_to_snorm(value: u8) -> f32 {
    f32::from(value) / 255.0 * 2.0 - 1.0
}

/// A component in [-1, 1] as an unsigned byte, rounded to nearest.
fn snorm_to_unorm(value: f32) -> u8 {
    ((value * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_pixel(pixel: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(1, 1, Rgba(pixel))
    }

    #[test]
    fn a_flat_normal_rebuilds_z_as_one() {
        let map = NormalMap::new(one_pixel([128, 128, 0, 17]));

        assert_eq!(map.to_gltf().get_pixel(0, 0).0, [128, 128, 255, 255]);
    }

    #[test]
    fn a_tilted_normal_rebuilds_a_unit_length_z_and_drops_the_mask() {
        // X = 0.6, Y = 0, so Z = 0.8.
        let x = snorm_to_unorm(0.6);
        let map = NormalMap::new(one_pixel([x, 128, 200, 0]));

        let [r, g, b, a] = map.to_gltf().get_pixel(0, 0).0;
        assert_eq!([r, g, a], [x, 128, 255]);
        assert!((unorm_to_snorm(b) - 0.8).abs() < 0.01, "z {}", unorm_to_snorm(b));
    }

    #[test]
    fn a_normal_past_the_unit_circle_rebuilds_z_as_zero() {
        let map = NormalMap::new(one_pixel([255, 255, 255, 255]));

        assert_eq!(map.to_gltf().get_pixel(0, 0).0, [255, 255, 128, 255]);
    }

    #[test]
    fn metal_gloss_moves_to_roughness_in_green_and_metalness_in_blue() {
        let map = MetalGlossMap::new(one_pixel([200, 30, 255, 7]));

        assert_eq!(map.to_gltf_metallic_roughness().get_pixel(0, 0).0, [255, 55, 30, 255]);
    }

    #[test]
    fn full_gloss_is_zero_roughness_and_zero_gloss_is_full_roughness() {
        let map = MetalGlossMap::new(RgbaImage::from_fn(2, 1, |x, _| Rgba([if x == 0 { 255 } else { 0 }, 0, 0, 255])));

        let converted = map.to_gltf_metallic_roughness();
        assert_eq!(converted.get_pixel(0, 0)[1], 0);
        assert_eq!(converted.get_pixel(1, 0)[1], 255);
    }
}

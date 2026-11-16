//! How a material's albedo alpha is meant to be read. Most albedo alpha
//! channels carry other data (height, roughness) and the material is drawn
//! opaque; alpha-tested and alpha-blended materials (propeller discs, glass,
//! flags, nets) need their alpha kept.

use crate::models::material::MaterialPrototype;
use crate::models::material::PropertyValue;

/// How a material draws its albedo alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlphaMode {
    /// Alpha is not opacity.
    Opaque,
    /// Texels with alpha below `cutoff` (0 to 1) are not drawn; the rest are opaque.
    Mask { cutoff: f32 },
    /// Alpha is opacity.
    Blend,
}

impl AlphaMode {
    /// The mode `material`, stored at `mfm_path`, draws with.
    ///
    /// An `alphaTestEnable` material tests against its `alphaReference`
    /// (0 to 255). A material with glass properties, or named `_alpha` as
    /// the game names its blended materials (`Japan_Disk_2Blade_alpha.mfm`,
    /// `transparent_glass_alpha.mfm`), blends. Every other material is opaque.
    pub fn of_material(material: &MaterialPrototype, mfm_path: &str) -> Self {
        // The cutoff of an alpha-tested material that names no reference.
        const DEFAULT_CUTOFF: f32 = 0.5;

        if material.get_bool("alphaTestEnable") == Some(true) {
            let reference = material.get_property("alphaReference").and_then(|property| match property.value {
                Some(PropertyValue::Int32(value)) => Some(value as f32),
                Some(PropertyValue::Float(value)) => Some(value),
                _ => None,
            });
            let cutoff = reference.map_or(DEFAULT_CUTOFF, |reference| (reference / 255.0).clamp(0.0, 1.0));

            return Self::Mask { cutoff };
        }

        let has_glass =
            material.properties.iter().any(|property| property.name.is_some_and(|name| name.starts_with("glass")));
        if has_glass || is_named_alpha(mfm_path) {
            return Self::Blend;
        }

        Self::Opaque
    }

    /// Rewrites the alpha of RGBA8 `pixels` to what the mode draws: 255 for
    /// an opaque material, and as stored otherwise. A masked texture keeps
    /// its alpha so the cutoff applies to filtered samples, as glTF and the
    /// game test it.
    pub fn apply(self, pixels: &mut [u8]) {
        match self {
            Self::Opaque => {
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    pixel[3] = u8::MAX;
                }
            }
            Self::Mask { .. } | Self::Blend => {
                // Alpha is coverage or opacity already.
            }
        }
    }

    /// The texture alpha below which a sample is not drawn. An opaque
    /// texture has none below 1; a blended one draws every sample with any
    /// coverage.
    pub fn cutoff(self) -> f32 {
        // The least alpha an 8-bit texel can carry above zero, halved.
        const ANY_COVERAGE: f32 = 0.5 / 255.0;

        match self {
            Self::Mask { cutoff } => cutoff,
            Self::Opaque | Self::Blend => ANY_COVERAGE,
        }
    }
}

/// Whether the `.mfm` file stem carries the `_alpha` suffix or infix
/// (`LC332_glass_alpha_skinned.mfm`).
fn is_named_alpha(mfm_path: &str) -> bool {
    let file = mfm_path.rsplit('/').next().unwrap_or(mfm_path);
    let stem = file.strip_suffix(".mfm").unwrap_or(file).to_ascii_lowercase();

    stem.ends_with("_alpha") || stem.contains("_alpha_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alpha_suffix_marks_a_blended_material() {
        assert!(is_named_alpha("content/gameplay/common/textures/Japan_Disk_2Blade_alpha.mfm"));
        assert!(is_named_alpha("content/x/LC332_glass_alpha_skinned.mfm"));
        assert!(!is_named_alpha("content/x/AAF014_Curtiss_TS1.mfm"));
        assert!(!is_named_alpha("content/x/alphabet.mfm"));
    }

    #[test]
    fn only_an_opaque_material_loses_its_alpha() {
        let mut opaque = [1, 2, 3, 40];
        let mut mask = [1, 2, 3, 40];
        let mut blend = [1, 2, 3, 40];
        AlphaMode::Opaque.apply(&mut opaque);
        AlphaMode::Mask { cutoff: 0.5 }.apply(&mut mask);
        AlphaMode::Blend.apply(&mut blend);

        assert_eq!((opaque[3], mask[3], blend[3]), (255, 40, 40));
    }
}

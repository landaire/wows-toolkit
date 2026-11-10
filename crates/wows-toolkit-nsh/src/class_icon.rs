//! A ship class icon as the game draws it: a dark outline around a white
//! fill. The other front ends multiply the icon by the row's color, which
//! colors the fill and keeps the outline dark. nsh's tint colors every
//! opaque pixel of an image, which would draw the icon as a solid shape, so
//! the fill is split into a mask of its own and drawn tinted over the icon.

use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use nsh_core::prelude::*;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

/// The two layers of a class icon, as `data:` URLs.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ClassIcon {
    /// The icon as the game ships it, drawn untinted.
    pub outline: String,
    /// The icon's light parts as a white mask, drawn tinted over `outline`.
    pub fill: String,
}

impl ClassIcon {
    /// Reads `species`' icon from `vfs`. `None` when the build lacks it or
    /// its SVG cannot be drawn.
    pub fn read(species: Species, vfs: &VfsPath) -> Option<Self> {
        let svg = wows_toolkit_replay_data::row_icons::ship_class_svg(species, vfs)?;

        Self::from_svg(&svg)
    }

    /// Splits `svg` into its layers.
    pub fn from_svg(svg: &[u8]) -> Option<Self> {
        // Rows show the icon at 16 points; this covers a 3x display.
        const RASTER_PX: u32 = 48;

        let raster = match nsh_core::svg::rasterize(svg, RASTER_PX) {
            Ok(raster) => raster,
            Err(error) => {
                tracing::debug!(?error, "class icon could not be drawn");
                return None;
            }
        };
        let fill = fill_mask(&raster.pixels);
        let png = encode_png(raster.width, raster.height, &fill)?;

        Some(Self { outline: data_uri("image/svg+xml", svg), fill: data_uri("image/png", &png) })
    }

    /// The icon tinted with `tint`, `size` points square and turned a quarter
    /// to lie as the tables show it, under the screen reader label `label`.
    pub fn element(&self, tint: AdaptiveColor, size: f32, label: String) -> View {
        let layer = |src: &str| {
            image(SharedString::from(src.to_owned()))
                .app_class(Some(SharedString::from("absolute inset-0")))
                .w(px(size))
                .h(px(size))
        };

        view()
            .app_class(Some(SharedString::from("flex-none rotate-90")))
            .w(px(size))
            .h(px(size))
            .child(layer(&self.outline).label(label))
            .child(layer(&self.fill).tint_color(tint))
    }
}

/// White at each pixel's lightness, so a tint shows where the icon is light
/// and leaves the dark outline beneath it alone. `premultiplied` is RGBA.
fn fill_mask(premultiplied: &[u8]) -> Vec<u8> {
    let mut mask = Vec::with_capacity(premultiplied.len());
    for pixel in premultiplied.as_chunks::<4>().0 {
        // Premultiplied, so the lightest channel is already the lightness
        // times the coverage: the mask's alpha.
        let alpha = pixel[0].max(pixel[1]).max(pixel[2]);
        mask.extend_from_slice(&[alpha, alpha, alpha, alpha]);
    }

    mask
}

/// `premultiplied` RGBA as a PNG, whose pixels are straight. The mask is
/// white wherever it is not transparent, so its straight form is white at
/// the same alpha.
fn encode_png(width: u32, height: u32, premultiplied: &[u8]) -> Option<Vec<u8>> {
    let straight: Vec<u8> = premultiplied
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| if pixel[3] == 0 { [0, 0, 0, 0] } else { [0xff, 0xff, 0xff, pixel[3]] })
        .collect();

    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(Cursor::new(&mut png), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let written = encoder.write_header().and_then(|mut writer| writer.write_image_data(&straight));
    if let Err(error) = written {
        tracing::debug!(%error, "class icon mask could not be encoded");
        return None;
    }

    Some(png)
}

fn data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &str = r##"<svg width="2" height="1" viewBox="0 0 2 1" xmlns="http://www.w3.org/2000/svg">
<rect x="0" y="0" width="1" height="1" fill="#333333"/>
<rect x="1" y="0" width="1" height="1" fill="white"/>
</svg>"##;

    #[test]
    fn the_mask_covers_the_light_fill_and_not_the_dark_outline() {
        let raster = nsh_core::svg::rasterize(ICON.as_bytes(), 2).expect("the icon draws");
        let mask = fill_mask(&raster.pixels);

        let alpha = |x: usize| mask[x * 4 + 3];
        assert_eq!(alpha(1), 0xff, "white fill is fully tinted");
        assert_eq!(alpha(0), 0x33, "the dark outline takes only its own lightness of the tint");
    }

    #[test]
    fn an_svg_that_does_not_parse_has_no_icon() {
        assert_eq!(ClassIcon::from_svg(b"not an svg"), None);
    }
}

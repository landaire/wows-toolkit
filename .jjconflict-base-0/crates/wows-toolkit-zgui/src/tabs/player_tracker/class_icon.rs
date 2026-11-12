//! The live roster's ship-class icons, drawn from the battle's build.
//!
//! The game's icons are white and point up. The roster turns them to point right and multiplies
//! them by the row's tone, as the egui and GPUI rosters do, so the result is a picture rather than
//! a vector the text colour could tint.

use std::io::Cursor;

use image::ImageFormat;
use image::RgbaImage;
use landaire_ui::Rgb;
use resvg::tiny_skia::Pixmap;
use resvg::tiny_skia::Transform;
use resvg::usvg::Options;
use resvg::usvg::Tree;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::theme::Semantic;

/// One icon the roster draws: a class in a row's tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassIconKey {
    pub species: Species,
    pub tint: Rgb,
}

impl ClassIconKey {
    pub fn new(species: Species, tint: PlayerTint) -> Self {
        Self { species, tint: tint_rgb(tint) }
    }
}

/// The tone an icon is multiplied by. The dark set in either appearance, as the GPUI roster's
/// `class_icon_tint` does: the icons sit on the row's own band tint, not on the panel.
fn tint_rgb(tint: PlayerTint) -> Rgb {
    let tones = &Semantic::DARK;
    match tint {
        PlayerTint::SelfPlayer => tones.text_strong,
        PlayerTint::Ally => tones.win,
        PlayerTint::Enemy => tones.loss,
        PlayerTint::DivisionMate => tones.division,
        PlayerTint::Abuser => tones.abuser,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClassIconError {
    #[error("the build has no icon for {0:?}")]
    Missing(Species),
    #[error("the icon for {species:?} is not a readable SVG")]
    Parse {
        species: Species,
        #[source]
        source: resvg::usvg::Error,
    },
    #[error("the icon for {species:?} has no area to draw")]
    Empty { species: Species },
    #[error("the icon for {species:?} could not be encoded")]
    Encode {
        species: Species,
        #[source]
        source: image::ImageError,
    },
}

/// The PNG the roster shows for `key`, read from `vfs`.
pub fn render(vfs: &VfsPath, key: ClassIconKey) -> Result<Vec<u8>, ClassIconError> {
    let species = key.species;
    let svg = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }
        .read(vfs, None)
        .ok_or(ClassIconError::Missing(species))?;
    render_svg(&svg, key)
}

fn render_svg(svg: &[u8], key: ClassIconKey) -> Result<Vec<u8>, ClassIconError> {
    let species = key.species;
    let tree = Tree::from_data(svg, &Options::default()).map_err(|source| ClassIconError::Parse { species, source })?;
    let size = tree.size().to_int_size();
    let mut pixmap = Pixmap::new(size.width(), size.height()).ok_or(ClassIconError::Empty { species })?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());

    let (red, green, blue) =
        (key.tint.red() as f32 / 255.0, key.tint.green() as f32 / 255.0, key.tint.blue() as f32 / 255.0);
    let mut drawn = RgbaImage::new(size.width(), size.height());
    for (out, pixel) in drawn.pixels_mut().zip(pixmap.pixels()) {
        let color = pixel.demultiply();
        out.0 = [
            (color.red() as f32 * red).round() as u8,
            (color.green() as f32 * green).round() as u8,
            (color.blue() as f32 * blue).round() as u8,
            color.alpha(),
        ];
    }
    let turned = image::imageops::rotate90(&drawn);

    let mut png = Vec::new();
    turned
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|source| ClassIconError::Encode { species, source })?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use image::ImageFormat;
    use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
    use wowsunpack::game_params::types::Species;

    use super::ClassIconKey;
    use super::render_svg;
    use crate::theme::Semantic;

    #[test]
    fn an_icon_turns_to_point_right_and_takes_the_rows_tone() {
        // A white bar along the top of a 4x2 icon: turned clockwise it runs down the right edge.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="1" fill="#ffffff"/></svg>"##;
        let key = ClassIconKey::new(Species::Destroyer, PlayerTint::Ally);
        let png = render_svg(svg, key).expect("the icon draws");
        let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png).expect("the PNG decodes").to_rgba8();

        assert_eq!(decoded.dimensions(), (2, 4));
        let tone = Semantic::DARK.win;
        assert_eq!(decoded.get_pixel(1, 0).0, [tone.red(), tone.green(), tone.blue(), 255]);
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
    }
}

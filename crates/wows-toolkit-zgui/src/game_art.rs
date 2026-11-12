//! Ship-class icons and nation flags read from a build's game files, and the tone a class icon is
//! drawn in.
//!
//! The game's class icons are white. A row's icon is its SVG with every paint multiplied by the
//! row's tone, as the game tints them, either as markup or drawn to a PNG turned to point right.

use std::io::Cursor;

use image::ImageFormat;
use image::RgbaImage;
use landaire_ui::Rgb;
use resvg::tiny_skia::Pixmap;
use resvg::tiny_skia::Transform;
use resvg::usvg::Options;
use resvg::usvg::Tree;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::theme::Semantic;

/// The relation a class icon's tone shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassTone {
    SelfPlayer,
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

impl ClassTone {
    /// The tone the icon is multiplied by: the dark theme's in either appearance. The tone
    /// multiplies the icon's white body, so a dark tone would leave the body the shade of its
    /// outline and the icon would read as a solid shape.
    pub fn rgb(self) -> Rgb {
        let tones = &Semantic::DARK;
        match self {
            Self::SelfPlayer => tones.text_strong,
            Self::Ally => tones.win,
            Self::Enemy => tones.loss,
            Self::DivisionMate => tones.division,
            Self::Abuser => tones.abuser,
        }
    }
}

impl From<PlayerColorKind> for ClassTone {
    fn from(kind: PlayerColorKind) -> Self {
        match kind {
            PlayerColorKind::SelfPlayer => Self::SelfPlayer,
            PlayerColorKind::Ally => Self::Ally,
            PlayerColorKind::Enemy => Self::Enemy,
            PlayerColorKind::DivisionMate => Self::DivisionMate,
            PlayerColorKind::Abuser => Self::Abuser,
        }
    }
}

impl From<PlayerTint> for ClassTone {
    fn from(tint: PlayerTint) -> Self {
        match tint {
            PlayerTint::SelfPlayer => Self::SelfPlayer,
            PlayerTint::Ally => Self::Ally,
            PlayerTint::Enemy => Self::Enemy,
            PlayerTint::DivisionMate => Self::DivisionMate,
            PlayerTint::Abuser => Self::Abuser,
        }
    }
}

/// `species`' white class icon in `vfs`, as SVG bytes.
pub fn class_icon_svg(vfs: &VfsPath, species: Species) -> Option<Vec<u8>> {
    GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.read(vfs, None)
}

/// `nation`'s flag in `vfs`, as PNG bytes. `nation` is the nation's name in the game params.
pub fn nation_flag_png(vfs: &VfsPath, nation: &str) -> Option<Vec<u8>> {
    GuiAsset::NationFlag(nation).read(vfs, None)
}

/// `svg` with each `fill` and `stroke` colour multiplied by `tone`, channel by channel. Paints other
/// than a hex colour, white or black are left as they are.
pub fn tinted_svg(svg: &str, tone: Rgb) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find("=\"") {
        let (head, tail) = rest.split_at(at + 2);
        out.push_str(head);
        let end = tail.find('"').unwrap_or(tail.len());
        let (value, after) = tail.split_at(end);
        let name = &head[..head.len() - 2];
        let paint = (name.ends_with(" fill") || name.ends_with(" stroke")).then(|| paint(value)).flatten();
        match paint {
            Some(color) => out.push_str(&multiply(color, tone).to_string()),
            None => out.push_str(value),
        }
        rest = after;
    }
    out.push_str(rest);

    out
}

fn paint(value: &str) -> Option<Rgb> {
    match value.trim() {
        "white" => Some(Rgb::new(0xffffff)),
        "black" => Some(Rgb::new(0x000000)),
        hex if hex.starts_with('#') => Rgb::parse(hex).ok(),
        _ => None,
    }
}

fn multiply(color: Rgb, tone: Rgb) -> Rgb {
    let channel = |a: u8, b: u8| ((u16::from(a) * u16::from(b) + 127) / 255) as u8;
    Rgb::from_channels(
        channel(color.red(), tone.red()),
        channel(color.green(), tone.green()),
        channel(color.blue(), tone.blue()),
    )
}

/// One class icon drawn in one tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassIconKey {
    pub species: Species,
    pub tone: Rgb,
}

impl ClassIconKey {
    pub fn new(species: Species, tone: impl Into<ClassTone>) -> Self {
        Self { species, tone: tone.into().rgb() }
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

/// `key`'s icon from `vfs`, drawn in its tone and turned to point right, as PNG bytes.
pub fn turned_class_icon_png(vfs: &VfsPath, key: ClassIconKey) -> Result<Vec<u8>, ClassIconError> {
    let svg = class_icon_svg(vfs, key.species).ok_or(ClassIconError::Missing(key.species))?;
    turned_png(&svg, key)
}

fn turned_png(svg: &[u8], key: ClassIconKey) -> Result<Vec<u8>, ClassIconError> {
    let species = key.species;
    let tree = Tree::from_data(svg, &Options::default()).map_err(|source| ClassIconError::Parse { species, source })?;
    let size = tree.size().to_int_size();
    let mut pixmap = Pixmap::new(size.width(), size.height()).ok_or(ClassIconError::Empty { species })?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());

    let mut drawn = RgbaImage::new(size.width(), size.height());
    for (out, pixel) in drawn.pixels_mut().zip(pixmap.pixels()) {
        let color = pixel.demultiply();
        let tinted = multiply(Rgb::from_channels(color.red(), color.green(), color.blue()), key.tone);
        out.0 = [tinted.red(), tinted.green(), tinted.blue(), color.alpha()];
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
    use landaire_ui::Rgb;
    use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
    use wowsunpack::game_params::types::Species;

    use super::ClassIconKey;
    use super::tinted_svg;
    use super::turned_png;
    use crate::theme::Semantic;

    #[test]
    fn a_class_icon_is_tinted_as_the_game_tints_it() {
        let svg = r##"<svg viewBox="0 0 9 16" fill="none"><path fill-rule="evenodd" d="M9 16Z" fill="#333333"/><path d="M1 15Z" fill="white"/></svg>"##;

        assert_eq!(
            tinted_svg(svg, Rgb::new(0x80ff00)),
            r##"<svg viewBox="0 0 9 16" fill="none"><path fill-rule="evenodd" d="M9 16Z" fill="#1a3300"/><path d="M1 15Z" fill="#80ff00"/></svg>"##
        );
    }

    #[test]
    fn a_turned_icon_points_right_and_takes_the_rows_tone() {
        // A white bar along the top of a 4x2 icon: turned clockwise it runs down the right edge.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="1" fill="#ffffff"/></svg>"##;
        let key = ClassIconKey::new(Species::Destroyer, PlayerTint::Ally);
        let png = turned_png(svg, key).expect("the icon draws");
        let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png).expect("the PNG decodes").to_rgba8();

        assert_eq!(decoded.dimensions(), (2, 4));
        let tone = Semantic::DARK.win;
        assert_eq!(decoded.get_pixel(1, 0).0, [tone.red(), tone.green(), tone.blue(), 255]);
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
    }
}

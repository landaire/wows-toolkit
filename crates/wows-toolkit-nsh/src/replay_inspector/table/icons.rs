//! The game icons the table shows, read from the replay's build files off
//! the engine thread and handed to the image elements as data URIs. An icon
//! the build lacks is absent, and the table shows text in its place.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Cursor;
use std::rc::Rc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nsh_core::prelude::*;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::row_detail::IconSize;
use wows_toolkit_replay_data::row_icons::RowIcon;
use wows_toolkit_replay_data::row_icons::row_icons;
use wows_toolkit_replay_data::row_icons::ship_class_svg;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

/// One decoded-on-demand icon: its source and its natural size in pixels,
/// which wide ribbons are fitted by.
#[derive(Debug, Clone, PartialEq)]
pub struct IconImage {
    pub uri: SharedString,
    pub size: IconSize,
}

/// What the background read found.
#[derive(Debug, Default)]
struct LoadedIcons {
    ship_classes: HashMap<Species, SharedString>,
    keyed: HashMap<RowIcon, IconImage>,
}

/// The icons of one document. Compared by identity: a new read is a new set.
#[derive(Debug, Clone, Default)]
pub struct TableIcons(Rc<LoadedIcons>);

impl PartialEq for TableIcons {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl TableIcons {
    /// The class icon for `species`, as SVG; the table tints and turns it.
    pub fn ship_class(&self, species: Species) -> Option<SharedString> {
        self.0.ship_classes.get(&species).cloned()
    }

    pub fn icon(&self, icon: &RowIcon) -> Option<IconImage> {
        self.0.keyed.get(icon).cloned()
    }
}

/// Reads every icon `rows` show from `vfs` in a blocking task.
pub async fn load(rows: &[PlayerRow], vfs: VfsPath) -> TableIcons {
    let wanted = row_icons(rows);
    let mut species = HashSet::new();
    let classes: Vec<Species> = rows.iter().map(|row| row.ship_class).filter(|class| species.insert(*class)).collect();

    let loaded = blocking(move || read(&classes, &wanted, &vfs)).await;
    match loaded {
        Ok(loaded) => TableIcons(Rc::new(loaded)),
        Err(source) => {
            // A panicked read leaves the table on its text labels, which is
            // what a build without the icons shows too.
            tracing::error!("player table icons could not be read: {source}");
            TableIcons::default()
        }
    }
}

fn read(classes: &[Species], wanted: &[RowIcon], vfs: &VfsPath) -> LoadedIcons {
    let mut loaded = LoadedIcons::default();

    for species in classes {
        if let Some(bytes) = ship_class_svg(*species, vfs) {
            loaded.ship_classes.insert(*species, data_uri("image/svg+xml", &bytes));
        }
    }

    for icon in wanted {
        let Some(bytes) = icon.read(vfs) else {
            continue;
        };
        // Bytes that are not a PNG would not draw either.
        let Some(size) = png_size(&bytes) else {
            tracing::debug!(icon = icon.cache_key(), "player table icon is not a readable PNG");
            continue;
        };
        loaded.keyed.insert(icon.clone(), IconImage { uri: data_uri("image/png", &bytes), size });
    }

    loaded
}

fn data_uri(mime: &str, bytes: &[u8]) -> SharedString {
    SharedString::from(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

/// A PNG's size from its header.
fn png_size(bytes: &[u8]) -> Option<IconSize> {
    let reader = png::Decoder::new(Cursor::new(bytes)).read_info().ok()?;
    let info = reader.info();

    Some(IconSize { width: info.width as f32, height: info.height as f32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("a PNG header");
            writer.write_image_data(&vec![0; (width * height * 4) as usize]).expect("the pixels");
        }
        bytes
    }

    #[test]
    fn a_png_reads_its_size_and_anything_else_reads_none() {
        assert_eq!(png_size(&png_bytes(220, 60)), Some(IconSize { width: 220.0, height: 60.0 }));
        assert_eq!(png_size(b"not an image"), None);
    }

    #[test]
    fn an_icon_becomes_a_base64_data_uri() {
        assert_eq!(data_uri("image/png", b"abc").as_ref(), "data:image/png;base64,YWJj");
    }
}

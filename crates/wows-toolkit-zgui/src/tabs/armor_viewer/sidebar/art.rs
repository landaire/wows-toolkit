//! The nation flags and class icons the sidebar's folder rows show.

use std::collections::HashMap;
use std::sync::Arc;

use wows_toolkit_armor::catalog::ShipCatalog;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;
use zgui_image::ImageBytes;

use crate::game_art;

/// Flags by GameParams nation key and white class icons by species. What the build lacks is
/// left out, and its row shows no icon.
#[derive(Default)]
pub struct SidebarArt {
    flags: HashMap<String, ImageBytes>,
    classes: HashMap<Species, Arc<str>>,
}

impl SidebarArt {
    /// The art in `vfs` for every nation and class `catalog` lists, hidden ships included.
    pub fn read(vfs: &VfsPath, catalog: &ShipCatalog) -> Self {
        let every = catalog.with_hidden();
        let mut flags = HashMap::new();
        let mut classes = HashMap::new();
        for nation in &every.nations {
            if let Some(bytes) = game_art::nation_flag_png(vfs, &nation.nation) {
                flags.insert(nation.nation.clone(), ImageBytes::new(bytes));
            }
            for class in &nation.classes {
                if classes.contains_key(&class.species) {
                    continue;
                }
                let svg = game_art::class_icon_svg(vfs, class.species).and_then(|bytes| String::from_utf8(bytes).ok());
                if let Some(svg) = svg {
                    classes.insert(class.species, Arc::from(svg));
                }
            }
        }

        Self { flags, classes }
    }

    pub fn flag(&self, nation: &str) -> Option<&ImageBytes> {
        self.flags.get(nation)
    }

    pub fn class_icon(&self, species: Species) -> Option<Arc<str>> {
        self.classes.get(&species).cloned()
    }
}

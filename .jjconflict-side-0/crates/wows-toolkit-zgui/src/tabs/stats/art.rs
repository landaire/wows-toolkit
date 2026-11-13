//! Achievement, nation flag and ship class art, read from the newest loaded build.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use wows_toolkit_replay_data::load::LoadedGameData;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui_image::ImageBytes;

/// One picture the build holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum ArtKey {
    /// By the achievement's icon key.
    Achievement(String),
    /// By the nation's name in the game params.
    NationFlag(String),
    ShipClass(Species),
}

/// A picture as the window draws it.
#[derive(Clone, Debug)]
pub(super) enum Art {
    /// PNG bytes, for an image element.
    Raster(ImageBytes),
    /// SVG markup, for a vector element.
    Vector(Arc<str>),
}

/// The build the art was read from. A newer build replaces it all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceBuild(u32);

/// The art a panel has asked for, as a `Copy` handle.
#[derive(Clone, Copy)]
pub(super) struct GameArt {
    loaded: RwSignal<HashMap<ArtKey, Art>, LocalStorage>,
    work: StoredValue<ArtWork, LocalStorage>,
}

struct ArtWork {
    /// The panel's owner: a read outlives the effect that asked for it, and ends with the panel.
    owner: Owner,
    source: Option<SourceBuild>,
    asked: HashSet<ArtKey>,
}

impl GameArt {
    /// Called from a component's body.
    pub(super) fn new() -> Self {
        Self {
            loaded: RwSignal::new_local(HashMap::new()),
            work: StoredValue::new_local(ArtWork {
                owner: Owner::current().expect("game art is made inside a component"),
                source: None,
                asked: HashSet::new(),
            }),
        }
    }

    /// The picture under `key`, `None` until it is read or when the build has none. Reactive.
    pub(super) fn get(self, key: &ArtKey) -> Option<Art> {
        self.loaded.with(|loaded| loaded.get(key).cloned())
    }

    /// Reads the pictures in `keys` not asked for yet from `data`. Without a build, drops them all.
    pub(super) fn want(self, data: Option<Arc<LoadedGameData>>, keys: impl IntoIterator<Item = ArtKey>) {
        let Some(data) = data else {
            self.release();
            return;
        };

        let source = SourceBuild(data.build());
        if self.work.with_value(|work| work.source) != Some(source) {
            self.release();
            self.work.update_value(|work| work.source = Some(source));
        }

        let fresh: Vec<ArtKey> =
            self.work.with_value(|work| keys.into_iter().filter(|key| !work.asked.contains(key)).collect());
        if fresh.is_empty() {
            return;
        }
        self.work.update_value(|work| work.asked.extend(fresh.iter().cloned()));

        let owner = self.work.with_value(|work| work.owner.clone());
        owner.with(|| {
            spawn_local(async move {
                let read = blocking(move || read_art(&data, fresh)).await;
                if self.work.try_with_value(|work| work.source) != Some(Some(source)) {
                    return;
                }
                self.loaded.update(|loaded| loaded.extend(read));
            })
        });
    }

    fn release(self) {
        self.work.update_value(|work| {
            work.source = None;
            work.asked.clear();
        });
        if self.loaded.with_untracked(|loaded| !loaded.is_empty()) {
            self.loaded.update(HashMap::clear);
        }
    }
}

fn read_art(data: &LoadedGameData, keys: Vec<ArtKey>) -> Vec<(ArtKey, Art)> {
    keys.into_iter()
        .filter_map(|key| {
            let art = match &key {
                ArtKey::Achievement(icon) => {
                    Art::Raster(ImageBytes::new(GuiAsset::Achievement(icon).read(data.vfs(), None)?))
                }
                ArtKey::NationFlag(nation) => {
                    Art::Raster(ImageBytes::new(GuiAsset::NationFlag(nation).read(data.vfs(), None)?))
                }
                ArtKey::ShipClass(species) => {
                    let bytes = GuiAsset::ShipClassIcon { species: *species, state: ShipIconState::Alive }
                        .read(data.vfs(), None)?;
                    Art::Vector(Arc::from(String::from_utf8(bytes).ok()?))
                }
            };

            Some((key, art))
        })
        .collect()
}

//! The Armor Viewer's game data: the ship-export assets with the ship catalogue, then the model
//! library built over them. Held by the shell, so the palette lists the ships from any tab, and
//! loaded once something asks for it: the tab being shown, the ship list entered, or a viewport
//! asking to show a ship.
//!
//! Also carries what a replay viewport asks of the viewer, since the tab may not be built yet.

use std::sync::Arc;

use rust_i18n::t;
use wows_replays::types::GameClock;
use wows_toolkit_armor::assets::ModelAssets;
use wows_toolkit_armor::assets::load_ship_assets;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::catalog::species_name;
use wows_toolkit_armor::catalog::tier_roman;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollowed;
use wows_toolkit_replay_data::replay_render::armor::ShowArmor;
use wowsunpack::export::ship::ShipAssets;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::services::GameDataState;
use crate::services::Job;
use crate::services::Services;
use crate::shell::palette::ShipParamIndex;
use crate::shell::palette::modes::ArmorShipRow;
use crate::shell::palette::modes::ShipList;

/// One build's ship data, as every pane reads it.
pub struct ArmorBundle {
    pub assets: Arc<ShipAssets>,
    pub catalog: Arc<ShipCatalog>,
}

/// Where the ship data load stands.
#[derive(Clone)]
pub enum BundleState {
    NotStarted,
    Loading,
    Ready(Arc<ArmorBundle>),
    Failed(String),
}

/// Where the model library stands. It is built once the ship data has loaded.
#[derive(Clone)]
pub enum ModelsState {
    NotStarted,
    Loading,
    Ready(Arc<ModelAssets>),
    Failed(String),
}

/// Names the replay viewport a feed came from, so the viewer follows only the playback it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedSource(u64);

/// What a replay viewport asked of the Armor Viewer, waiting for the tab to take it.
pub enum ViewerFeed {
    /// Opens the ship with what it had taken. `seek` moves the playback behind it.
    Show { source: FeedSource, shown: Box<ShowArmor>, seek: Option<UnsyncCallback<GameClock>> },
    /// Playback moved and the ship took different hits.
    Followed { source: FeedSource, followed: ArmorFollowed },
}

/// The Armor Viewer's data, as one `Copy` handle.
#[derive(Clone, Copy)]
pub struct ArmorLibrary {
    bundle: RwSignal<BundleState, LocalStorage>,
    models: RwSignal<ModelsState, LocalStorage>,
    /// Set once anything has asked for the data.
    wanted: RwSignal<bool, LocalStorage>,
    /// The game data the bundle was loaded from, so a new build loads again.
    source: StoredValue<Option<Arc<LoadedGameData>>, LocalStorage>,
    /// Counts loads, so one overtaken by a newer build is dropped.
    generation: StoredValue<u64>,
    feed: RwSignal<Vec<ViewerFeed>, LocalStorage>,
    next_source: StoredValue<u64>,
}

impl ArmorLibrary {
    pub fn new() -> Self {
        Self {
            bundle: RwSignal::new_local(BundleState::NotStarted),
            models: RwSignal::new_local(ModelsState::NotStarted),
            wanted: RwSignal::new_local(false),
            source: StoredValue::new_local(None),
            generation: StoredValue::new(0),
            feed: RwSignal::new_local(Vec::new()),
            next_source: StoredValue::new(0),
        }
    }

    /// Loads the data once it is wanted and the game data is ready, and again for each newer
    /// build. Called once, from the shell's body.
    pub fn begin(self, services: Services) {
        let loading = RenderEffect::new(move |_| {
            if !self.wanted.get() {
                return;
            }
            let GameDataState::Ready(loaded) = services.game_data.state() else { return };
            let same = self.source.with_value(|source| source.as_ref().is_some_and(|held| Arc::ptr_eq(held, &loaded)));
            if same {
                return;
            }
            self.source.set_value(Some(Arc::clone(&loaded)));
            self.load(services, loaded);
        });
        on_cleanup_local(move || drop(loading));
    }

    /// Asks for the data to be loaded.
    pub fn want(self) {
        if !self.wanted.get_untracked() {
            self.wanted.set(true);
        }
    }

    pub fn bundle(self) -> BundleState {
        self.bundle.get()
    }

    pub fn bundle_untracked(self) -> BundleState {
        self.bundle.get_untracked()
    }

    /// The loaded ship data, tracked.
    pub fn ready(self) -> Option<Arc<ArmorBundle>> {
        self.bundle.with(|bundle| match bundle {
            BundleState::Ready(bundle) => Some(Arc::clone(bundle)),
            BundleState::NotStarted | BundleState::Loading | BundleState::Failed(_) => None,
        })
    }

    pub fn ready_untracked(self) -> Option<Arc<ArmorBundle>> {
        match self.bundle.get_untracked() {
            BundleState::Ready(bundle) => Some(bundle),
            BundleState::NotStarted | BundleState::Loading | BundleState::Failed(_) => None,
        }
    }

    pub fn models(self) -> ModelsState {
        self.models.get()
    }

    pub fn models_ready_untracked(self) -> Option<Arc<ModelAssets>> {
        match self.models.get_untracked() {
            ModelsState::Ready(models) => Some(models),
            ModelsState::NotStarted | ModelsState::Loading | ModelsState::Failed(_) => None,
        }
    }

    /// Every ship, as the palette's ship list reads it.
    pub fn ship_list(self, services: Services) -> Signal<ShipList, LocalStorage> {
        Signal::derive_local(move || match services.game_data.state() {
            GameDataState::NoInstall => ShipList::Unavailable(t!("ui.armor.load_settings").into_owned()),
            GameDataState::Failed(reason) => ShipList::Unavailable(reason),
            GameDataState::Loading { .. } => ShipList::Pending,
            GameDataState::Ready(_) => match self.bundle.get() {
                BundleState::NotStarted | BundleState::Loading => ShipList::Pending,
                BundleState::Ready(bundle) => ShipList::Ready(ship_rows(&bundle.catalog).into()),
                BundleState::Failed(reason) => ShipList::Unavailable(reason),
            },
        })
    }

    /// A name for one replay viewport's feed, distinct from every other.
    pub fn new_source(self) -> FeedSource {
        let source = self.next_source.get_value();
        self.next_source.set_value(source + 1);

        FeedSource(source)
    }

    /// Hands the viewer something a replay viewport asked for.
    pub fn send(self, feed: ViewerFeed) {
        self.want();
        self.feed.update(|held| {
            // Only a viewport's latest position matters to a viewer that has not taken the last.
            if let ViewerFeed::Followed { source, .. } = &feed {
                held.retain(|held| !matches!(held, ViewerFeed::Followed { source: from, .. } if from == source));
            }
            held.push(feed);
        });
    }

    /// Takes what replay viewports asked for, oldest first, subscribing to the next.
    pub fn take_feed(self) -> Vec<ViewerFeed> {
        if self.feed.with(Vec::is_empty) {
            return Vec::new();
        }
        let mut taken = Vec::new();
        self.feed.update(|held| taken = std::mem::take(held));
        taken
    }

    fn load(self, services: Services, loaded: Arc<LoadedGameData>) {
        self.generation.set_value(self.generation.get_value() + 1);
        let generation = self.generation.get_value();
        self.bundle.set(BundleState::Loading);
        self.models.set(ModelsState::NotStarted);

        spawn_local(async move {
            let job = services.jobs.start(Job { name: t!("ui.armor.loading_catalog").into_owned(), progress: None });
            let read = blocking(move || {
                let assets = load_ship_assets(loaded.vfs_with_assets(), loaded.assets_bin_bytes(), loaded.provider())?;
                let catalog = ShipCatalog::build(loaded.provider());
                Ok::<_, wows_toolkit_armor::assets::ArmorAssetsError>(ArmorBundle {
                    assets: Arc::new(assets),
                    catalog: Arc::new(catalog),
                })
            })
            .await;
            drop(job);
            if self.generation.try_get_value() != Some(generation) {
                return;
            }
            let bundle = match read {
                Ok(bundle) => Arc::new(bundle),
                Err(err) => {
                    tracing::error!("armor viewer: failed to load ship assets: {err}");
                    self.bundle.set(BundleState::Failed(err.to_string()));
                    return;
                }
            };
            self.bundle.set(BundleState::Ready(Arc::clone(&bundle)));

            // Separate, so the ship list is usable while `assets.bin` is indexed.
            self.models.set(ModelsState::Loading);
            let job = services.jobs.start(Job { name: t!("ui.models.loading").into_owned(), progress: None });
            let built = blocking(move || ModelAssets::build(&bundle.assets)).await;
            drop(job);
            if self.generation.try_get_value() != Some(generation) {
                return;
            }
            match built {
                Ok(models) => self.models.set(ModelsState::Ready(Arc::new(models))),
                Err(err) => {
                    tracing::error!("model viewer: failed to build the model library: {err}");
                    self.models.set(ModelsState::Failed(err.to_string()));
                }
            }
        });
    }

    /// Puts ready data in place without loading it, for tests.
    #[cfg(test)]
    pub fn adopt_for_test(self, bundle: Arc<ArmorBundle>, models: Option<Arc<ModelAssets>>) {
        self.bundle.set(BundleState::Ready(bundle));
        self.models.set(models.map_or(ModelsState::NotStarted, ModelsState::Ready));
    }
}

impl Default for ArmorLibrary {
    fn default() -> Self {
        Self::new()
    }
}

/// Every ship in `catalog`, in its nation, class and tier order.
pub fn ship_rows(catalog: &ShipCatalog) -> Vec<ArmorShipRow> {
    let mut rows = Vec::new();
    for nation in &catalog.nations {
        for class in &nation.classes {
            for ship in &class.ships {
                rows.push(ArmorShipRow::new(
                    ShipParamIndex(ship.param_index.clone()),
                    ship.display_name.clone(),
                    tier_roman(ship.tier),
                    species_name(&class.species),
                    &nation.display_name,
                ));
            }
        }
    }
    rows
}

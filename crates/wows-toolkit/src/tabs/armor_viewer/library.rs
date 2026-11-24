//! The Armor Viewer's game data: the ship-export assets with the ship catalogue, then the model
//! library built over them. Held by the shell, so the palette lists the ships from any tab, and
//! loaded once something asks for it: the tab being shown, the ship list entered, or a viewport
//! asking to show a ship.
//!
//! Also carries what a replay viewport asks of the viewer, since the tab may not be built yet.

use std::rc::Rc;
use std::sync::Arc;

use wows_replays::types::GameClock;
use wows_toolkit_armor::assets::ModelAssets;
use wows_toolkit_armor::assets::load_ship_assets;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::catalog::species_name;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollowed;
use wows_toolkit_replay_data::replay_render::armor::ShowArmor;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::tier_roman;
use wt_translations::t;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;

use crate::long_job::long_job;
use crate::services::GameDataState;
use crate::services::Job;
use crate::services::Services;
use crate::shell::palette::ShipParamIndex;
use crate::shell::palette::modes::ArmorShipRow;
use crate::shell::palette::modes::ShipList;
use crate::tabs::armor_viewer::sidebar::art::SidebarArt;

/// One build's ship data, as every pane reads it.
pub struct ArmorBundle {
    pub assets: Arc<ShipAssets>,
    pub catalog: Arc<ShipCatalog>,
    pub sidebar_art: Arc<SidebarArt>,
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
    /// The reason is logged where the build fails.
    Failed,
}

/// Names the replay viewport a feed came from, so the viewer follows only the playback it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeedSource(u64);

/// Seeks replay playback independently of the reactive scope that requested the viewer.
pub type PlaybackSeek = Rc<dyn Fn(GameClock)>;

/// What a replay viewport asked of the Armor Viewer, waiting for the tab to take it.
pub enum ViewerFeed {
    /// Opens the ship with what it had taken. The owned `seek` outlives the caller's reactive owner.
    Show { source: FeedSource, shown: Box<ShowArmor>, seek: Option<PlaybackSeek> },
    /// Playback moved, and with it the clock and health, and perhaps the hits.
    Followed { source: FeedSource, followed: ArmorFollowed },
    /// The battle was baked again with its players named differently.
    Renamed { source: FeedSource, shown: Box<ShowArmor> },
}

impl ViewerFeed {
    /// Whether this is `source` showing the viewer or moving its hits.
    pub(super) fn moves_hits_of(&self, source: FeedSource) -> bool {
        match self {
            Self::Show { source: from, .. } | Self::Followed { source: from, .. } => *from == source,
            Self::Renamed { .. } => false,
        }
    }
}

/// Held while an export runs; dropping it lets the next one start.
#[must_use]
pub struct ExportRunning(ArmorLibrary);

impl Drop for ExportRunning {
    fn drop(&mut self) {
        self.0.exporting.try_set_value(false);
    }
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
    /// Set while a model export is being written. One runs at a time.
    exporting: StoredValue<bool>,
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
            exporting: StoredValue::new(false),
        }
    }

    pub fn is_exporting(self) -> bool {
        self.exporting.try_get_value().unwrap_or(false)
    }

    /// Marks an export as running until the returned guard drops, or `None` while one already is.
    pub fn begin_export(self) -> Option<ExportRunning> {
        if self.is_exporting() {
            return None;
        }
        self.exporting.set_value(true);

        Some(ExportRunning(self))
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
        // `try_`, as a popped-out replay viewport sends to the viewer from a window that can
        // outlive the main window owning the library.
        if self.wanted.try_get_untracked() == Some(false) {
            self.wanted.set(true);
        }
    }

    /// The loaded ship data, tracked.
    pub fn ready(self) -> Option<Arc<ArmorBundle>> {
        self.bundle.with(|bundle| match bundle {
            BundleState::Ready(bundle) => Some(Arc::clone(bundle)),
            BundleState::NotStarted | BundleState::Loading | BundleState::Failed(_) => None,
        })
    }

    /// Whether the ship data failed to load, so nothing asked of the viewer can be opened.
    /// Tracked.
    pub fn failed(self) -> bool {
        self.bundle.with(|bundle| matches!(bundle, BundleState::Failed(_)))
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
            ModelsState::NotStarted | ModelsState::Loading | ModelsState::Failed => None,
        }
    }

    /// Every ship, as the palette's ship list reads it.
    pub fn ship_list(self, services: Services) -> Signal<ShipList, LocalStorage> {
        Signal::derive_local(move || match services.game_data.state() {
            GameDataState::NoInstall => ShipList::Unavailable(t!("ui-armor-load_settings").into_owned()),
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
        self.feed.update(|held| push_feed(held, feed));
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
            let job = services.jobs.start(Job { name: t!("ui-armor-loading_catalog").into_owned(), progress: None });
            let read = long_job(move || {
                let assets = load_ship_assets(loaded.vfs_with_assets(), loaded.assets_bin_bytes(), loaded.provider())?;
                let catalog = ShipCatalog::build(loaded.provider());
                let sidebar_art = SidebarArt::read(assets.vfs(), &catalog);
                Ok::<_, wows_toolkit_armor::assets::ArmorAssetsError>(ArmorBundle {
                    assets: Arc::new(assets),
                    catalog: Arc::new(catalog),
                    sidebar_art: Arc::new(sidebar_art),
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
            let job = services.jobs.start(Job { name: t!("ui-models-loading").into_owned(), progress: None });
            let built = long_job(move || ModelAssets::build(&bundle.assets)).await;
            drop(job);
            if self.generation.try_get_value() != Some(generation) {
                return;
            }
            match built {
                Ok(models) => self.models.set(ModelsState::Ready(Arc::new(models))),
                Err(err) => {
                    tracing::error!("model viewer: failed to build the model library: {err}");
                    self.models.set(ModelsState::Failed);
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

/// Adds `feed` to what is waiting for the viewer. Only a viewport's latest position matters to a
/// viewer that has not taken the last, but never one from before that viewport showed the viewer
/// again: the show starts its hits afresh.
fn push_feed(held: &mut Vec<ViewerFeed>, mut feed: ViewerFeed) {
    if let ViewerFeed::Followed { source, followed } = &mut feed
        && let Some(at) = held.iter().rposition(|held| held.moves_hits_of(*source))
        && matches!(held[at], ViewerFeed::Followed { .. })
        && let ViewerFeed::Followed { followed: earlier, .. } = held.remove(at)
    {
        followed.supersede(earlier);
    }
    held.push(feed);
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

#[cfg(test)]
mod tests {
    use zgui::reactive::Mounted;

    use super::*;

    /// A popped-out replay viewport asks for the viewer from a window that can outlive the main
    /// window owning the library.
    #[test]
    fn a_request_after_the_library_is_gone_does_nothing() {
        zgui::reactive::install().expect("the reactive runtime installs");
        let main = Mounted::new();
        let library = main.with(ArmorLibrary::new);
        library.want();
        assert!(library.wanted.get_untracked(), "a request is held while the main window is");

        main.unmount();
        library.want();
    }
}

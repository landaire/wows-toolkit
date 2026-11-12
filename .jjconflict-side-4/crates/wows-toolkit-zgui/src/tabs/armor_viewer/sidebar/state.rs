//! What the sidebar holds between frames: the search, the filters, the folders open, the model
//! catalogue listing once it is built, and the listing built from all of them.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::TreeHandle;
use wows_toolkit_armor::catalog::HiddenReason;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::catalog::TierRange;
use wows_toolkit_armor::model_refs::OpenTarget;
use wows_toolkit_armor::model_refs::VehicleParamIndex;
use wows_toolkit_armor::model_tree::FortificationRow;
use wows_toolkit_armor::model_tree::ModelReferences;
use wows_toolkit_armor::model_tree::ModelTreeIndex;
use wows_toolkit_armor::model_tree::ShipPartRows;
use wows_toolkit_armor::model_tree::list_all_ship_parts;
use wows_toolkit_armor::model_tree::list_fortifications;
use wows_toolkit_armor::model_tree::list_model_buildings;
use wows_toolkit_armor::sidebar_tree::Expansion;
use wows_toolkit_armor::sidebar_tree::ModelSection;
use wows_toolkit_armor::sidebar_tree::RowId;
use wows_toolkit_armor::sidebar_tree::RowKind;
use wows_toolkit_armor::sidebar_tree::SearchMatches;
use wows_toolkit_armor::sidebar_tree::TreeSources;
use wows_toolkit_armor::sidebar_tree::build_tree;
use wows_toolkit_armor::sidebar_tree::open_row_ids;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::export::building_armor::BuildingArmorIndex;
use wowsunpack::export::model_library::ModelLibrary;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;

use crate::services::Services;
use crate::tabs::armor_viewer::sidebar::ModelsSource;
use crate::tabs::armor_viewer::sidebar::ShipChoice;
use crate::tabs::armor_viewer::sidebar::SidebarCatalog;
use crate::tabs::armor_viewer::sidebar::SidebarEvent;
use crate::tabs::armor_viewer::sidebar::tree_model::SidebarTree;

/// Where the model catalogue listing stands.
pub enum ModelsState {
    Loading,
    Unavailable,
    Ready(Arc<ModelTreeIndex>),
}

/// What a listing rebuild keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rebuild {
    /// The search or the listed ships or models changed: match again and open the folders
    /// holding matches.
    Rematch,
    /// Keep the matches and the folders the user opened or closed.
    Keep,
}

/// The sidebar's state, as a handle over signals.
#[derive(Clone, Copy)]
pub struct SidebarState {
    services: Services,
    on_event: StoredValue<UnsyncCallback<SidebarEvent>, LocalStorage>,

    catalog: RwSignal<Option<Arc<SidebarCatalog>>, LocalStorage>,
    library: StoredValue<Option<Arc<ModelLibrary>>, LocalStorage>,
    pub models: RwSignal<ModelsState, LocalStorage>,
    ship_parts: StoredValue<Arc<HashMap<VehicleParamIndex, ShipPartRows>>, LocalStorage>,
    fortifications: StoredValue<Arc<Vec<FortificationRow>>, LocalStorage>,

    /// The search field's text, and the search applied to the listing.
    pub search_text: RwSignal<String, LocalStorage>,
    pub query: RwSignal<FilterQuery, LocalStorage>,
    /// `query` matched against the listing; `None` when listing everything.
    search: StoredValue<Option<SearchMatches>, LocalStorage>,
    debounce: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    timers: StoredValue<Option<Timers>, LocalStorage>,

    /// The tiers the Ships section lists.
    pub tiers: RwSignal<TierRange, LocalStorage>,
    /// Whether the ships the catalog leaves out are listed too, the catalog listing them while
    /// they are, and why each is left out.
    pub show_hidden: RwSignal<bool, LocalStorage>,
    with_hidden: StoredValue<Option<Arc<ShipCatalog>>, LocalStorage>,
    hidden_reasons: StoredValue<Arc<HashMap<String, HiddenReason>>, LocalStorage>,
    /// Whether the reader set the tiers or the hidden-ship choice this session, which a saved
    /// value read later does not undo.
    tiers_chosen: StoredValue<bool, LocalStorage>,
    hidden_chosen: StoredValue<bool, LocalStorage>,
    settings_restored: StoredValue<bool, LocalStorage>,

    /// Folders open while not searching, kept across searches; folders open during the current
    /// search; and folders the reader closed during it, which a later match leaves closed.
    browse_expanded: StoredValue<HashSet<RowId>, LocalStorage>,
    search_expanded: StoredValue<HashSet<RowId>, LocalStorage>,
    search_collapsed: StoredValue<HashSet<RowId>, LocalStorage>,

    /// The listing, the folders the tree draws open, and the highlighted row.
    pub tree: RwSignal<Rc<SidebarTree>, LocalStorage>,
    pub expanded: RwSignal<HashSet<RowId>, LocalStorage>,
    pub selection: RwSignal<HashSet<RowId>, LocalStorage>,
    pub handle: TreeHandle<RowId>,

    /// What the active pane shows, highlighted in the listing.
    open: RwSignal<Option<OpenTarget>, LocalStorage>,

    index_task: StoredValue<Option<Task>, LocalStorage>,
    restore_tasks: StoredValue<Vec<Task>, LocalStorage>,
}

impl SidebarState {
    pub fn new(services: Services, on_event: UnsyncCallback<SidebarEvent>) -> Self {
        Self {
            services,
            on_event: StoredValue::new_local(on_event),
            catalog: RwSignal::new_local(None),
            library: StoredValue::new_local(None),
            models: RwSignal::new_local(ModelsState::Loading),
            ship_parts: StoredValue::new_local(Arc::new(HashMap::new())),
            fortifications: StoredValue::new_local(Arc::new(Vec::new())),
            search_text: RwSignal::new_local(String::new()),
            query: RwSignal::new_local(FilterQuery::default()),
            search: StoredValue::new_local(None),
            debounce: StoredValue::new_local(None),
            timers: StoredValue::new_local(Timers::current()),
            tiers: RwSignal::new_local(TierRange::ALL),
            show_hidden: RwSignal::new_local(false),
            with_hidden: StoredValue::new_local(None),
            hidden_reasons: StoredValue::new_local(Arc::new(HashMap::new())),
            tiers_chosen: StoredValue::new_local(false),
            hidden_chosen: StoredValue::new_local(false),
            settings_restored: StoredValue::new_local(false),
            browse_expanded: StoredValue::new_local(HashSet::new()),
            search_expanded: StoredValue::new_local(HashSet::new()),
            search_collapsed: StoredValue::new_local(HashSet::new()),
            tree: RwSignal::new_local(Rc::new(SidebarTree::default())),
            expanded: RwSignal::new_local(HashSet::new()),
            selection: RwSignal::new_local(HashSet::new()),
            handle: TreeHandle::new(),
            open: RwSignal::new_local(None),
            index_task: StoredValue::new_local(None),
            restore_tasks: StoredValue::new_local(Vec::new()),
        }
    }

    pub fn catalog(self) -> Option<Arc<SidebarCatalog>> {
        self.catalog.get()
    }

    pub fn catalog_untracked(self) -> Option<Arc<SidebarCatalog>> {
        self.catalog.get_untracked()
    }

    fn emit(self, event: SidebarEvent) {
        self.on_event.get_value().run(event);
    }

    /// Adopts the loaded ship catalog. Any model library belongs to the previous catalog, so the
    /// Models section waits for the one built over this one.
    pub fn set_catalog(self, catalog: Option<Arc<SidebarCatalog>>) {
        if self.catalog.with_untracked(|current| same(current.as_ref(), catalog.as_ref())) {
            return;
        }

        let reasons = catalog.as_ref().map_or_else(HashMap::new, |catalog| {
            catalog.catalog.hidden.iter().map(|hidden| (hidden.ship.param_index.clone(), hidden.reason)).collect()
        });
        self.hidden_reasons.set_value(Arc::new(reasons));
        self.catalog.set(catalog);
        self.refresh_with_hidden();
        self.cancel_index();
        self.models.set(ModelsState::Loading);
        self.ship_parts.set_value(Arc::new(HashMap::new()));
        self.fortifications.set_value(Arc::new(Vec::new()));
        self.start_model_index();
        self.rebuild(Rebuild::Rematch);
    }

    /// Adopts where the model library stands.
    pub fn set_models_source(self, source: ModelsSource) {
        match source {
            ModelsSource::Loading => {
                self.library.set_value(None);
                self.cancel_index();
                self.models.set(ModelsState::Loading);
                self.rebuild(Rebuild::Rematch);
            }
            ModelsSource::Unavailable => {
                self.library.set_value(None);
                self.cancel_index();
                self.models.set(ModelsState::Unavailable);
                self.rebuild(Rebuild::Rematch);
            }
            ModelsSource::Ready(library) => {
                if self.library.with_value(|current| same(current.as_ref(), Some(&library))) {
                    return;
                }
                self.library.set_value(Some(library));
                self.start_model_index();
            }
        }
    }

    /// Adopts what the active pane shows, so its row is highlighted.
    pub fn set_open(self, open: Option<OpenTarget>) {
        if self.open.with_untracked(|current| *current == open) {
            return;
        }
        self.open.set(open);
        self.highlight_open_row();
    }

    fn cancel_index(self) {
        if let Some(task) = self.index_task.try_update_value(Option::take).flatten() {
            task.cancel();
        }
    }

    /// Lists the model catalogue and every ship's parts off the UI thread. The index is listed as
    /// soon as it is ready; the parts take longer.
    fn start_model_index(self) {
        let (Some(catalog), Some(library)) = (self.catalog.get_untracked(), self.library.get_value()) else {
            return;
        };
        self.cancel_index();

        let task = spawn_local(async move {
            let references_catalog = Arc::clone(&catalog);
            // Hidden ships list their parts and name their models too, for when the reader shows
            // them.
            let (all_ships, references) = blocking(move || {
                let all_ships = Arc::new(references_catalog.catalog.with_hidden());
                let references = ModelReferences::build(&all_ships, references_catalog.assets.metadata());
                (all_ships, Arc::new(references))
            })
            .await;

            let (index_catalog, index_library, index_references) =
                (Arc::clone(&catalog), Arc::clone(&library), Arc::clone(&references));
            let index = blocking(move || {
                let metadata = index_catalog.assets.metadata();
                let buildings = BuildingArmorIndex::build(metadata, &index_library);
                let models = index_library.catalog();
                let fortifications = list_fortifications(&buildings, models, metadata);
                let model_buildings = list_model_buildings(&buildings, metadata);
                let index =
                    ModelTreeIndex::build(models, &index_catalog.catalog, &index_references, model_buildings, metadata);
                (index, fortifications)
            });
            let (parts_catalog, parts_library) = (Arc::clone(&catalog), Arc::clone(&library));
            let parts = blocking(move || {
                list_all_ship_parts(&all_ships, parts_catalog.assets.metadata(), &parts_library, &references)
            });

            let (index, fortifications) = index.await;
            if !self.is_current(&catalog, &library) {
                return;
            }
            self.fortifications.set_value(Arc::new(fortifications));
            self.adopt_model_index(index);

            let parts = parts.await;
            if self.is_current(&catalog, &library) {
                self.adopt_ship_parts(parts);
            }
        });
        self.index_task.set_value(Some(task));
    }

    /// Whether `catalog` and `library` are still the ones the sidebar lists.
    fn is_current(self, catalog: &Arc<SidebarCatalog>, library: &Arc<ModelLibrary>) -> bool {
        let catalog_current = self.catalog.with_untracked(|current| same(current.as_ref(), Some(catalog)));
        let library_current = self.library.with_value(|current| same(current.as_ref(), Some(library)));

        catalog_current && library_current
    }

    pub fn adopt_model_index(self, index: ModelTreeIndex) {
        self.models.set(ModelsState::Ready(Arc::new(index)));
        self.rebuild(Rebuild::Rematch);
    }

    /// A search names parts as well as ships. Folders the reader closed meanwhile stay closed.
    pub fn adopt_ship_parts(self, parts: HashMap<VehicleParamIndex, ShipPartRows>) {
        self.ship_parts.set_value(Arc::new(parts));
        self.rebuild(Rebuild::Rematch);
    }

    /// Whether every ship's parts are listed.
    #[cfg(test)]
    pub fn parts_listed(self) -> bool {
        self.ship_parts.with_value(|parts| !parts.is_empty())
    }

    #[cfg(test)]
    pub fn adopt_fortifications(self, fortifications: Vec<FortificationRow>) {
        self.fortifications.set_value(Arc::new(fortifications));
        self.rebuild(Rebuild::Rematch);
    }

    /// Reads back the tiers and hidden-ship choice saved by an earlier session, once the
    /// settings database is open.
    pub fn restore_settings(self) {
        if self.settings_restored.get_value() {
            return;
        }
        let Some(pool) = self.services.settings.pool() else {
            return;
        };
        self.settings_restored.set_value(true);

        let services = self.services;
        let tiers_pool = pool.clone();
        let tiers = spawn_local(async move {
            let stored = services
                .on_tokio(async move {
                    wows_toolkit_config::queries::get_setting::<TierRange>(&tiers_pool, keys::MODEL_VIEWER_SHIP_TIERS)
                        .await
                })
                .await;
            // A choice the reader made before the read finished stands.
            if let Ok(Some(tiers)) = stored
                && !self.tiers_chosen.get_value()
            {
                self.set_tiers(tiers);
            }
        });
        let hidden = spawn_local(async move {
            let stored = services
                .on_tokio(async move {
                    wows_toolkit_config::queries::get_setting::<bool>(&pool, keys::MODEL_VIEWER_SHOW_HIDDEN_SHIPS).await
                })
                .await;
            if let Ok(Some(show)) = stored
                && !self.hidden_chosen.get_value()
            {
                self.set_show_hidden(show);
            }
        });
        self.restore_tasks.set_value(vec![tiers, hidden]);
    }

    /// The reader moved a tier thumb.
    pub fn choose_tiers(self, tiers: TierRange) {
        self.tiers_chosen.set_value(true);
        self.set_tiers(tiers);
    }

    /// The reader let go of a tier thumb: the tiers are saved once, not on every step.
    pub fn commit_tiers(self, tiers: TierRange) {
        self.choose_tiers(tiers);
        self.services.settings.save(keys::MODEL_VIEWER_SHIP_TIERS, &tiers);
    }

    pub fn set_tiers(self, tiers: TierRange) {
        if self.tiers.get_untracked() == tiers {
            return;
        }
        self.tiers.set(tiers);
        self.rebuild(Rebuild::Rematch);
    }

    /// The reader ticked or cleared "Show hidden ships".
    pub fn choose_show_hidden(self, show: bool) {
        self.hidden_chosen.set_value(true);
        self.set_show_hidden(show);
        self.services.settings.save(keys::MODEL_VIEWER_SHOW_HIDDEN_SHIPS, &show);
    }

    fn set_show_hidden(self, show: bool) {
        if self.show_hidden.get_untracked() == show {
            return;
        }
        self.show_hidden.set(show);
        self.refresh_with_hidden();
        self.rebuild(Rebuild::Rematch);
    }

    fn refresh_with_hidden(self) {
        let catalog = self.catalog.get_untracked();
        let with_hidden = match (catalog, self.show_hidden.get_untracked()) {
            (Some(catalog), true) => Some(Arc::new(catalog.catalog.with_hidden())),
            (Some(_), false) | (None, _) => None,
        };
        self.with_hidden.set_value(with_hidden);
    }

    /// The search field changed: applies the search after a pause in typing, or at once when
    /// it was cleared.
    pub fn search_changed(self, text: &str) {
        // Coalesces typing so a 15000-model search runs once per pause.
        const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

        let query = FilterQuery::new(text);
        if self.query.with_untracked(|current| *current == query) {
            self.debounce.set_value(None);
            return;
        }
        if !query.is_active() {
            self.debounce.set_value(None);
            self.apply_query(query);
            return;
        }

        let apply = move || self.apply_query(query);
        let timer =
            self.timers.with_value(|timers| timers.as_ref().map(|timers| timers.set_timeout(SEARCH_DEBOUNCE, apply)));
        match timer {
            Some(timer) => self.debounce.set_value(Some(timer)),
            None => self.apply_query(FilterQuery::new(text)),
        }
    }

    /// Applies the search field's text now if it is still waiting out its pause.
    pub fn apply_pending_query(self) {
        let query = self.search_text.with_untracked(|text| FilterQuery::new(text));
        if self.query.with_untracked(|current| *current != query) {
            self.debounce.set_value(None);
            self.apply_query(query);
        }
    }

    pub fn apply_query(self, query: FilterQuery) {
        self.query.set(query);
        self.search_expanded.set_value(HashSet::new());
        self.search_collapsed.set_value(HashSet::new());
        self.rebuild(Rebuild::Rematch);
    }

    /// The tree opened or closed folders: `next` is the set it wants open.
    pub fn set_expanded(self, next: HashSet<RowId>) {
        let searching = self.query.with_untracked(FilterQuery::is_active);
        let current = self.expanded.get_untracked();
        let opened: Vec<RowId> = next.difference(&current).cloned().collect();
        let closed: Vec<RowId> = current.difference(&next).cloned().collect();
        if opened.is_empty() && closed.is_empty() {
            return;
        }

        let set = if searching { self.search_expanded } else { self.browse_expanded };
        set.update_value(|set| {
            set.extend(opened.iter().cloned());
            for id in &closed {
                set.remove(id);
            }
        });
        self.search_collapsed.update_value(|collapsed| {
            for id in &opened {
                collapsed.remove(id);
            }
            if searching {
                collapsed.extend(closed);
            }
        });

        self.rebuild(Rebuild::Keep);
    }

    /// Opens or closes folder `id`, as the tree's chevron does.
    fn toggle_folder(self, id: &RowId) {
        let mut next = self.expanded.get_untracked();
        if !next.remove(id) {
            next.insert(id.clone());
        }
        self.set_expanded(next);
    }

    /// Opens the folder `ids` while browsing, as the reader opening each would.
    #[cfg(test)]
    pub fn open_folders(self, ids: &[&str]) {
        let mut next = self.expanded.get_untracked();
        next.extend(ids.iter().map(|id| RowId::from(*id)));
        self.set_expanded(next);
    }

    pub fn rebuild(self, rebuild: Rebuild) {
        let Some(catalog) = self.catalog.get_untracked() else {
            self.tree.set(Rc::new(SidebarTree::default()));
            return;
        };

        let with_hidden = self.with_hidden.get_value();
        let ship_parts = self.ship_parts.get_value();
        let fortifications = self.fortifications.get_value();
        let hidden = self.hidden_reasons.get_value();
        let query = self.query.get_untracked();
        let searching = query.is_active();
        let tiers = self.tiers.get_untracked();
        let expanded_store = if searching { self.search_expanded } else { self.browse_expanded };
        let mut set = expanded_store.get_value();
        let closed = self.search_collapsed.get_value();

        let built = self.models.with_untracked(|models| {
            let models = match models {
                ModelsState::Loading => ModelSection::Loading,
                ModelsState::Unavailable => ModelSection::Unavailable,
                ModelsState::Ready(index) => ModelSection::Ready(index),
            };
            let sources = TreeSources {
                ships: with_hidden.as_deref().unwrap_or(&catalog.catalog),
                ship_parts: &ship_parts,
                models,
                fortifications: &fortifications,
                hidden: &hidden,
            };
            if rebuild == Rebuild::Rematch {
                self.search.set_value(SearchMatches::from_query(&sources, &query, tiers));
            }

            let auto = rebuild == Rebuild::Rematch && searching;
            let mut expansion = Expansion { set: &mut set, closed: &closed, auto };
            self.search.with_value(|search| build_tree(&sources, search.as_ref(), &mut expansion))
        });

        let tree = Rc::new(SidebarTree::from_built(built));
        expanded_store.set_value(set.clone());
        self.expanded.set(set);
        self.tree.set(Rc::clone(&tree));

        // A folder opened or closed keeps the row the reader is on.
        let kept = rebuild == Rebuild::Keep
            && self.selection.with_untracked(|selected| selected.iter().all(|id| tree.contains(id)));
        if !kept {
            self.highlight_open_row();
        }
    }

    /// Selects the open target's row, or nothing when it is not listed.
    fn highlight_open_row(self) {
        let ids = self.open_row_ids();
        let listed = self.tree.with_untracked(|tree| ids.into_iter().find(|id| tree.contains(id)));
        match listed {
            Some(id) => {
                self.selection.set(HashSet::from([id.clone()]));
                self.handle.select_key(id);
            }
            None => {
                if self.selection.with_untracked(|selected| !selected.is_empty()) {
                    self.selection.set(HashSet::new());
                }
            }
        }
    }

    /// The rows that stand for the open target.
    fn open_row_ids(self) -> Vec<RowId> {
        let Some(open) = self.open.get_untracked() else {
            return Vec::new();
        };

        self.models.with_untracked(|models| {
            let index = match models {
                ModelsState::Ready(index) => Some(index.as_ref()),
                ModelsState::Loading | ModelsState::Unavailable => None,
            };
            open_row_ids(&open, index)
        })
    }

    /// Enter in the search field: applies a search still waiting out its pause, then opens the
    /// row the reader highlighted, or the first one that opens.
    pub fn open_from_search(self) {
        self.apply_pending_query();

        // The open target's row is highlighted for the pane, not chosen.
        let open = self.open_row_ids();
        let highlighted = self.selection.with_untracked(|selected| selected.iter().next().cloned()).filter(|id| {
            !open.contains(id) && self.tree.with_untracked(|tree| tree.kind(id).is_some_and(RowKind::opens_on_its_own))
        });
        let target = highlighted.or_else(|| self.tree.with_untracked(|tree| tree.first_openable()));
        if let Some(id) = target {
            self.open_row(&id);
        }
    }

    /// Down in the search field moves into the listing, onto the first row that opens.
    pub fn enter_listing(self) -> bool {
        self.apply_pending_query();
        let Some(id) = self.tree.with_untracked(|tree| tree.first_openable()) else {
            return false;
        };
        self.handle.focus_key(id);

        true
    }

    /// Does what a click on row `id` does: a ship, building or model row opens, a folder opens
    /// or closes.
    pub fn open_row(self, id: &RowId) {
        let Some(kind) = self.tree.with_untracked(|tree| tree.kind(id).cloned()) else {
            return;
        };

        if kind.is_folder() {
            self.toggle_folder(id);
            return;
        }
        if let Some(event) = open_event(&kind) {
            self.emit(event);
        }
    }

    /// Opens what row `id` stands for, without opening or closing it as a folder.
    pub fn open_target(self, id: &RowId) {
        let event = self.tree.with_untracked(|tree| tree.kind(id).and_then(open_event));
        if let Some(event) = event {
            self.emit(event);
        }
    }

    pub fn send(self, event: SidebarEvent) {
        self.emit(event);
    }
}

/// What opening a row of `kind` asks for, for a row that opens on its own.
pub fn open_event(kind: &RowKind) -> Option<SidebarEvent> {
    match kind {
        RowKind::Ship { param_index, display_name } => Some(SidebarEvent::OpenShip(ShipChoice {
            param_index: VehicleParamIndex::new(param_index.clone()),
            display_name: display_name.clone(),
        })),
        RowKind::Building { id, .. } => Some(SidebarEvent::OpenBuilding(*id)),
        RowKind::Model(_) | RowKind::ShipPart { .. } => {
            kind.model_selection().map(|(model, from_ship)| SidebarEvent::OpenModel { model, from_ship })
        }
        _ => None,
    }
}

/// Whether `a` and `b` are the same shared value, or both absent.
fn same<T>(a: Option<&Arc<T>>, b: Option<&Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

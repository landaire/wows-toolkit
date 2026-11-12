//! The Unpacker tab's state and what the reader can ask of it: the builds and the one loaded,
//! the two browser panes, the extraction queue and its run, and the search and viewer panels.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use futures::StreamExt;
use futures::channel::mpsc;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::PanelEntry;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use rust_i18n::t;
use wows_toolkit_services::dialog::Answer;
use wows_toolkit_viewmodel::settings::keys as setting_keys;
use wows_toolkit_viewmodel::unpacker::build::BuildNumber;
use wows_toolkit_viewmodel::unpacker::build::BuildSource;
use wows_toolkit_viewmodel::unpacker::build::list_builds;
use wows_toolkit_viewmodel::unpacker::build::load_assets_bin;
use wows_toolkit_viewmodel::unpacker::build::load_package_vfs;
use wows_toolkit_viewmodel::unpacker::extract::ExtractProgress;
use wows_toolkit_viewmodel::unpacker::extract::ExtractState;
use wows_toolkit_viewmodel::unpacker::extract::PrototypeOutput;
use wows_toolkit_viewmodel::unpacker::extract::expand_to_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_root;
use wows_toolkit_viewmodel::unpacker::game_params::DumpRequest;
use wows_toolkit_viewmodel::unpacker::game_params::GameParamsDumpError;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::queue::ExtractQueue;
use wows_toolkit_viewmodel::unpacker::search::ContentSearchHit;
use wows_toolkit_viewmodel::unpacker::search::SearchProgress;
use wows_toolkit_viewmodel::unpacker::search::compile_query;
use wows_toolkit_viewmodel::unpacker::search::files_to_scan;
use wows_toolkit_viewmodel::unpacker::search::scan;
use wows_toolkit_viewmodel::unpacker::viewer;
use wows_toolkit_viewmodel::unpacker::viewer::ViewerContent;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::services::Job;
use crate::services::Progress;
use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::unpacker::pane::BrowserSource;
use crate::tabs::unpacker::pane::Pane;
use crate::tabs::unpacker::search::SearchResults;
use crate::tabs::unpacker::viewer::ViewerDoc;

/// The kinds of panel the tab's dock holds.
pub const BROWSER_KIND: &str = "unpacker-browser";
pub const QUEUE_KIND: &str = "unpacker-queue";
pub const SEARCH_KIND: &str = "unpacker-search";
pub const VIEWER_KIND: &str = "unpacker-viewer";

/// The queue panel's id; it is the only one.
pub const QUEUE_PANEL: &str = "queue";

/// One message from a running scan.
enum SearchUpdate {
    Hit(ContentSearchHit),
    Progress(SearchProgress),
}

/// The tab's state. `Copy`: every field is a handle on reactive state owned by the tab.
#[derive(Clone, Copy)]
pub struct UnpackerModel {
    pub services: Services,
    pub shell: Shell,
    pub dock: DockState,
    /// Builds found in the install, newest first.
    pub builds: RwSignal<Vec<BuildNumber>, LocalStorage>,
    pub selected_build: RwSignal<Option<BuildNumber>, LocalStorage>,
    wows_dir: StoredValue<Option<PathBuf>, LocalStorage>,
    /// The build whose VFS the panes hold or are loading.
    loaded: StoredValue<Option<BuildSource>, LocalStorage>,
    /// Bumped per build load, so a slower earlier load cannot overwrite a later one.
    generation: StoredValue<u64, LocalStorage>,
    /// The package VFS of the loaded build, for the parameters dump.
    pub package_vfs: RwSignal<Option<VfsPath>, LocalStorage>,
    /// What the last parameters dump did, shown beside its button.
    pub dump_status: RwSignal<Option<String>, LocalStorage>,
    pub pkg: Pane,
    pub assets: Pane,
    pub queue: RwSignal<ExtractQueue, LocalStorage>,
    pub extract: RwSignal<ExtractState, LocalStorage>,
    /// Set to stop the extraction in flight; replaced per run.
    stop: StoredValue<Arc<AtomicBool>, LocalStorage>,
    /// Session state: the setting is not saved.
    pub prototypes: RwSignal<PrototypeOutput, LocalStorage>,
    /// Where an extraction writes. Empty until one is chosen, which keeps Extract disabled.
    pub output_dir: RwSignal<String, LocalStorage>,
    /// The output directory last saved, so an unchanged field is not written again.
    saved_output_dir: StoredValue<String, LocalStorage>,
    pub searches: StoredValue<HashMap<PanelId, SearchResults>, LocalStorage>,
    pub viewers: StoredValue<HashMap<PanelId, ViewerDoc>, LocalStorage>,
    next_panel: StoredValue<u64, LocalStorage>,
    /// The tab's scope, for work asked for from a menu or panel that can close before it ends.
    owner: StoredValue<Option<Owner>, LocalStorage>,
}

impl UnpackerModel {
    pub fn new(services: Services, shell: Shell, dock: DockState) -> Self {
        Self {
            services,
            shell,
            dock,
            builds: RwSignal::new_local(Vec::new()),
            selected_build: RwSignal::new_local(None),
            wows_dir: StoredValue::new_local(None),
            loaded: StoredValue::new_local(None),
            generation: StoredValue::new_local(0),
            package_vfs: RwSignal::new_local(None),
            dump_status: RwSignal::new_local(None),
            pkg: Pane::new(BrowserSource::Pkg),
            assets: Pane::new(BrowserSource::AssetsBin),
            queue: RwSignal::new_local(ExtractQueue::new()),
            extract: RwSignal::new_local(ExtractState::Idle),
            stop: StoredValue::new_local(Arc::new(AtomicBool::new(false))),
            prototypes: RwSignal::new_local(PrototypeOutput::Raw),
            output_dir: RwSignal::new_local(String::new()),
            saved_output_dir: StoredValue::new_local(String::new()),
            searches: StoredValue::new_local(HashMap::new()),
            viewers: StoredValue::new_local(HashMap::new()),
            next_panel: StoredValue::new_local(0),
            owner: StoredValue::new_local(Owner::current()),
        }
    }

    pub fn pane(self, source: BrowserSource) -> Pane {
        match source {
            BrowserSource::Pkg => self.pkg,
            BrowserSource::AssetsBin => self.assets,
        }
    }

    /// Runs `build` in the tab's scope: what it makes lives, and what it spawns runs, as long as
    /// the tab rather than the control that asked.
    fn in_tab<R>(self, build: impl FnOnce() -> R) -> R {
        match self.owner.with_value(Clone::clone) {
            Some(owner) => owner.with(build),
            None => build(),
        }
    }

    fn spawn(self, work: impl Future<Output = ()> + 'static) {
        self.in_tab(|| spawn_local(work));
    }

    /// Adopts the saved output directory. A save of the value already held changes nothing, so an
    /// unrelated settings change keeps what the reader is typing.
    pub fn adopt_output_dir(self, dir: String) {
        if self.saved_output_dir.get_value() == dir {
            return;
        }
        self.saved_output_dir.set_value(dir.clone());
        self.output_dir.set(dir);
    }

    /// Saved on focus out or Enter rather than per keystroke, so a half-typed path never reaches
    /// the database.
    pub fn store_output_dir(self, dir: String) {
        if self.saved_output_dir.get_value() == dir {
            return;
        }
        self.saved_output_dir.set_value(dir.clone());
        if self.output_dir.get_untracked() != dir {
            self.output_dir.set(dir.clone());
        }
        self.services.settings.edit(setting_keys::OUTPUT_DIR, move |settings| {
            settings.output_dir = dir.clone();
            dir
        });
    }

    pub fn browse_for_output_dir(self) {
        let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.unpacker.extraction_output"));
        self.spawn(async move {
            let Some(picked) = chosen(self.shell, asked).await else { return };
            self.store_output_dir(picked.to_string_lossy().into_owned());
        });
    }

    /// Adopts the WoWs directory, lists its builds and loads the newest. An empty directory
    /// clears the panes rather than leaving stale data.
    pub fn apply_wows_dir(self, wows_dir: String) {
        let dir = (!wows_dir.is_empty()).then(|| PathBuf::from(wows_dir));
        if self.wows_dir.get_value() == dir && dir.is_some() {
            return;
        }
        self.wows_dir.set_value(dir.clone());
        let builds = dir
            .as_deref()
            .map(|dir| {
                list_builds(dir).unwrap_or_else(|err| {
                    tracing::warn!("unpacker: {err}");
                    Vec::new()
                })
            })
            .unwrap_or_default();
        self.builds.set(builds.clone());
        match builds.first().copied() {
            Some(newest) => self.select_build(newest),
            None => {
                self.selected_build.set(None);
                self.loaded.set_value(None);
                self.package_vfs.set(None);
                self.generation.update_value(|generation| *generation += 1);
                self.pkg.clear();
                self.assets.clear();
            }
        }
    }

    /// Loads `build`'s package VFS in the background, then its assets.bin, so the package pane is
    /// usable first.
    pub fn select_build(self, build: BuildNumber) {
        let Some(dir) = self.wows_dir.get_value() else { return };
        let source = BuildSource { dir, build };
        if self.loaded.with_value(|loaded| loaded.as_ref() == Some(&source)) {
            return;
        }
        self.selected_build.set(Some(build));
        self.loaded.set_value(Some(source.clone()));
        self.package_vfs.set(None);
        self.generation.update_value(|generation| *generation += 1);
        let generation = self.generation.get_value();
        let current = move || self.generation.try_get_value() == Some(generation);

        self.pkg.set_loading();
        self.assets.set_loading();
        let services = self.services;
        self.spawn(async move {
            let job = services.jobs.start(Job { name: t!("ui.replay.loading").into_owned(), progress: None });
            let loaded = blocking(move || load_package_vfs(&source).map_err(|err| err.to_string())).await;
            if !current() {
                return;
            }
            let package_vfs = match loaded {
                Ok(vfs) => vfs,
                Err(reason) => {
                    self.pkg.set_failed(reason.clone());
                    self.assets.set_failed(reason);
                    return;
                }
            };
            self.package_vfs.set(Some(package_vfs.clone()));
            self.pkg.set_vfs(package_vfs.clone());

            job.report(Job { name: t!("ui.unpacker.loading_assets").into_owned(), progress: None });
            let opened = blocking(move || load_assets_bin(&package_vfs).map_err(|err| err.to_string())).await;
            if !current() {
                return;
            }
            match opened {
                Ok(vfs) => self.assets.set_vfs(vfs),
                Err(reason) => self.assets.set_failed(reason),
            }
        });
    }

    pub fn queue_path(self, path: VfsPath) {
        self.queue.update(|queue| {
            queue.push(path);
        });
    }

    pub fn unqueue(self, path: &VfsPath) {
        self.queue.update(|queue| queue.remove(path));
    }

    /// Queues what a listing shows. The rows travel rather than bare paths so the queue applies
    /// its own files-only rule.
    pub fn queue_listed(self, entries: &[ListingEntry]) {
        self.queue.update(|queue| {
            queue.push_listed_files(entries);
        });
    }

    /// Refused while a run is going: the run holds what it was given, and its progress would
    /// overwrite the state of a run started after the clear.
    pub fn clear_queue(self) {
        if self.extract.with_untracked(ExtractState::is_busy) {
            return;
        }
        self.queue.update(ExtractQueue::clear);
        self.extract.set(ExtractState::Idle);
    }

    /// Writes the queue under the output directory off the UI thread.
    pub fn start_extraction(self) {
        let output_dir = self.output_dir.get_untracked().trim().to_string();
        if self.queue.with_untracked(ExtractQueue::is_empty)
            || self.extract.with_untracked(ExtractState::is_busy)
            || output_dir.is_empty()
        {
            return;
        }
        self.store_output_dir(output_dir.clone());
        let root = extract_root(Path::new(&output_dir));
        let queued = self.queue.with_untracked(|queue| queue.entries().to_vec());
        let prototypes = self.prototypes.get_untracked();
        let stop = Arc::new(AtomicBool::new(false));
        self.stop.set_value(stop.clone());
        self.extract.set(ExtractState::Counting);

        let (progress_tx, mut progress_rx) = mpsc::unbounded::<ExtractProgress>();
        let worker = blocking(move || {
            let files = expand_to_files(&queued);
            extract_files(
                &files,
                &root,
                prototypes,
                |progress| {
                    let _ = progress_tx.unbounded_send(progress);
                },
                || stop.load(Ordering::Relaxed),
            )
        });
        let services = self.services;
        self.spawn(async move {
            let job = services.jobs.start(Job { name: t!("ui.unpacker.counting_files").into_owned(), progress: None });
            while let Some(mut progress) = progress_rx.next().await {
                // A fast run reports faster than a frame; only the newest report is drawn.
                while let Ok(newer) = progress_rx.try_recv() {
                    progress = newer;
                }
                job.report(Job {
                    name: t!("ui.unpacker.extracting", done = progress.written, total = progress.total).into_owned(),
                    progress: Progress::new(progress.written as u64, progress.total as u64),
                });
                self.extract.set(ExtractState::Running(progress));
            }
            self.extract.set(ExtractState::finished(worker.await));
        });
    }

    pub fn cancel_extraction(self) {
        self.stop.get_value().store(true, Ordering::Relaxed);
    }

    /// Opens a results panel and starts the scan behind it. Each run gets its own panel, so an
    /// earlier result set survives.
    pub fn open_search(self, source: BrowserSource, query: String, path_filter: String, files: Arc<FileList>) {
        let Some(pattern) = compile_query(&query) else {
            tracing::warn!("unpacker: the search query {query:?} could not be compiled");
            return;
        };
        let results = self.in_tab(|| SearchResults::new(query, source));
        let id = self.next_panel_id("search");
        self.searches.update_value(|searches| {
            searches.insert(id.clone(), results);
        });
        self.add_panel(DockTarget::Region(DockRegion::Bottom), PanelEntry::new(id, PanelKind::new(SEARCH_KIND)));

        let stop = results.stop_flag.get_value();
        let (tx, mut rx) = mpsc::unbounded::<SearchUpdate>();
        let worker = blocking(move || {
            let targets = files_to_scan(&files, &path_filter);
            let hits = tx.clone();
            scan(
                &files,
                &targets,
                &pattern,
                |hit| {
                    let _ = hits.unbounded_send(SearchUpdate::Hit(hit));
                },
                |progress| {
                    let _ = tx.unbounded_send(SearchUpdate::Progress(progress));
                },
                || stop.load(Ordering::Relaxed),
            );
        });
        let services = self.services;
        self.spawn(async move {
            let job = services.jobs.start(Job { name: results.title(), progress: None });
            // Drained in batches: a broad query matches faster than the screen can be drawn.
            while let Some(first) = rx.next().await {
                // A closed panel stops its scan; what is still arriving is for nobody.
                if !results.running.get_untracked() {
                    break;
                }
                let mut hits = Vec::new();
                let mut latest = None;
                let mut pending = Some(first);
                while let Some(update) = pending.take().or_else(|| rx.try_recv().ok()) {
                    match update {
                        SearchUpdate::Hit(hit) => hits.push(hit),
                        SearchUpdate::Progress(progress) => latest = Some(progress),
                    }
                }
                if let Some(progress) = latest {
                    job.report(Job {
                        name: results.title(),
                        progress: Progress::new(progress.scanned as u64, progress.total as u64),
                    });
                    results.progress.set(progress);
                }
                results.extend(hits);
            }
            worker.await;
            results.running.set(false);
        });
    }

    /// Stops the scans of search panels that have been closed and forgets their results.
    pub fn forget_closed_panels(self) {
        let open: Vec<PanelId> = self.dock.panels_untracked().into_iter().map(|entry| entry.id).collect();
        self.searches.update_value(|searches| {
            searches.retain(|id, results| {
                let keep = open.contains(id);
                if !keep {
                    results.stop();
                }
                keep
            });
        });
        self.viewers.update_value(|viewers| viewers.retain(|id, _| open.contains(id)));
    }

    /// Shows `path` in the tree it came from, and brings that pane forward.
    pub fn reveal(self, source: BrowserSource, path: VfsPath) {
        self.pane(source).reveal(&path);
        if let Err(err) = self.dock.activate(&PanelId::new(source.id_fragment())) {
            tracing::warn!("unpacker: the {source:?} browser was not shown: {err}");
        }
    }

    /// Opens a file in its viewer, or logs why it has none. The viewable files are the small text
    /// and image ones, so they are read inline.
    pub fn open_viewer(self, path: VfsPath) {
        match viewer::load(&path) {
            Ok(content) => self.show_viewer(path.as_str().trim_start_matches('/').to_string(), content),
            Err(err) => tracing::warn!("unpacker: {err}"),
        }
    }

    /// Decodes an assets.bin prototype and shows the JSON in a viewer.
    pub fn open_json_viewer(self, path: VfsPath) {
        match viewer::decode_to_json(&path) {
            Ok(json) => self.show_viewer(
                format!("{} (JSON)", path.as_str().trim_start_matches('/')),
                ViewerContent::Plaintext { extension: ".json".to_string(), text: json },
            ),
            Err(err) => tracing::warn!("unpacker: {err}"),
        }
    }

    fn show_viewer(self, title: String, content: ViewerContent) {
        let doc = self.in_tab(|| ViewerDoc::open(title, content));
        let id = self.next_panel_id("viewer");
        self.viewers.update_value(|viewers| {
            viewers.insert(id.clone(), doc);
        });
        let target = self
            .dock
            .group_of(&PanelId::new(BrowserSource::Pkg.id_fragment()))
            .map_or(DockTarget::Region(DockRegion::Center), DockTarget::Group);
        self.add_panel(target, PanelEntry::new(id, PanelKind::new(VIEWER_KIND)));
    }

    /// Decodes an assets.bin prototype and writes the JSON where the reader asks.
    pub fn extract_as_json(self, path: VfsPath) {
        let name = path.filename();
        let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).to_string();
        let asked = wows_toolkit_services::dialog::save_file(
            Some(&t!("ui.unpacker.extract_as_json")),
            &format!("{stem}.json"),
            None,
        );
        self.spawn(async move {
            let Some(target) = chosen(self.shell, asked).await else { return };
            let written = blocking(move || {
                let json = viewer::decode_to_json(&path).map_err(|err| err.to_string())?;
                std::fs::write(&target, json).map_err(|err| err.to_string())
            })
            .await;
            self.dump_status.set(Some(match written {
                Ok(()) => t!("ui.unpacker.prototype_written").into_owned(),
                Err(err) => err,
            }));
        });
    }

    /// Writes the loaded build's parameters out as `request` asks.
    pub fn dump_game_params(self, request: DumpRequest) {
        let (Some(vfs), Some(build)) = (self.package_vfs.get_untracked(), self.selected_build.get_untracked()) else {
            self.dump_status.set(Some(t!("ui.unpacker.load_a_build").into_owned()));
            return;
        };
        let asked = wows_toolkit_services::dialog::save_file(Some("Save game parameters"), &request.file_name(), None);
        let services = self.services;
        self.spawn(async move {
            let Some(path) = chosen(self.shell, asked).await else { return };
            self.dump_status.set(Some("Writing...".to_string()));
            let _job =
                services.jobs.start(Job { name: t!("ui.unpacker.dump_parameters").into_owned(), progress: None });
            let written = blocking(move || {
                request.write(&vfs, &path, |vfs| {
                    wows_toolkit_replay_data::load::load_game_params(vfs, build.0)
                        .map_err(|err| GameParamsDumpError::Decode(err.to_string()))
                })
            })
            .await;
            self.dump_status.set(Some(match written {
                Ok(()) => t!("ui.unpacker.parameters_written").into_owned(),
                Err(err) => err.to_string(),
            }));
        });
    }

    fn next_panel_id(self, kind: &str) -> PanelId {
        let next = self.next_panel.get_value();
        self.next_panel.set_value(next + 1);
        PanelId::new(format!("{kind}-{next}"))
    }

    fn add_panel(self, target: DockTarget, entry: PanelEntry) {
        let id = entry.id.clone();
        if let Err(err) = self.dock.add_panel(target, entry) {
            tracing::error!("unpacker: the panel {} was not docked: {err}", id.as_str());
        }
    }
}

/// Awaits a file dialog's answer. A dialog that could not be opened is logged and reported, and
/// answers `None` like a dismissed one.
async fn chosen(shell: Shell, asked: impl Future<Output = Answer>) -> Option<PathBuf> {
    // A closure rather than a match: off Linux the error type is empty, and an arm for it would be
    // an unreachable pattern.
    asked.await.unwrap_or_else(|err| {
        tracing::error!("file dialog could not be opened: {err}");
        shell.toasts().failed(t!("ui.dialogs.file_dialog_failed", reason = err).into_owned());
        None
    })
}

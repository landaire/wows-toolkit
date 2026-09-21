//! The Unpacker tab: a build selector over a dock holding one browser pane
//! per VFS source, plus the extraction queue those panes feed.
//!
//! Mirrors the egui app's `build_unpacker_tab`: the version bar appears only
//! when the install carries more than one build, and the package and
//! assets.bin browsers are separate tabs of an inner dock.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::dock::DockSkin;
use gpui_kit::component::dock::PanelId;
use gpui_kit::component::dock::panel_handle;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wowsunpack::vfs::VfsPath;

use super::browser::BrowserEvent;
use super::browser::BrowserPanel;
use super::browser::BrowserSource;
use super::search_panel::SearchPanel;
use super::search_panel::SearchPanelEvent;
use super::viewer_panel::FileViewerPanel;
use wows_toolkit_viewmodel::settings::keys as setting_keys;
use wows_toolkit_viewmodel::unpacker::assets_bin;
use wows_toolkit_viewmodel::unpacker::extract::ExtractOutcome;
use wows_toolkit_viewmodel::unpacker::extract::ExtractProgress;
use wows_toolkit_viewmodel::unpacker::extract::PrototypeOutput;
use wows_toolkit_viewmodel::unpacker::extract::expand_to_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_files;
use wows_toolkit_viewmodel::unpacker::extract::extract_root;
use wows_toolkit_viewmodel::unpacker::game_params;
use wows_toolkit_viewmodel::unpacker::game_params::GameParamsDumpError;
use wows_toolkit_viewmodel::unpacker::game_params::GameParamsFormat;
use wows_toolkit_viewmodel::unpacker::listing::FileList;
use wows_toolkit_viewmodel::unpacker::queue::ExtractQueue;
use wows_toolkit_viewmodel::unpacker::search::ContentSearchHit;
use wows_toolkit_viewmodel::unpacker::search::SearchProgress;
use wows_toolkit_viewmodel::unpacker::search::compile_query;
use wows_toolkit_viewmodel::unpacker::search::files_to_scan;
use wows_toolkit_viewmodel::unpacker::search::scan;
use wows_toolkit_viewmodel::unpacker::viewer;

/// The queue dropdown's box: it takes the width its longest path wants, up to
/// a cap that keeps it off the browser, and scrolls past the height cap.
const QUEUE_POPOVER_MAX_WIDTH: Pixels = px(420.);
const QUEUE_POPOVER_MAX_HEIGHT: Pixels = px(300.);
const DUMP_POPOVER_MAX_WIDTH: Pixels = px(220.);

/// One message from a running scan.
enum SearchUpdate {
    Hit(ContentSearchHit),
    Progress(SearchProgress),
}

/// A game build number. A newtype so a build cannot be mixed up with the
/// other bare integers on this tab (queue lengths, progress counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BuildNumber(pub u32);

impl std::fmt::Display for BuildNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// One entry in the build combo. Local newtype: `BuildNumber` is ours but
/// `SearchableListItem` is not, and carrying the build keeps the confirm
/// handler off a string round trip.
#[derive(Clone)]
struct BuildItem(BuildNumber);

impl SearchableListItem for BuildItem {
    type Value = BuildNumber;

    fn title(&self) -> SharedString {
        SharedString::from(self.0.to_string())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

/// What the extraction queue is doing.
enum ExtractState {
    Idle,
    /// Expanding the queue's directories. The file count is not known yet, so
    /// there is no fraction to show.
    Counting,
    Running(ExtractProgress),
    Failed(String),
    /// Finished or cancelled, with what actually reached disk.
    Done(ExtractOutcome),
}

pub struct UnpackerView {
    /// Builds found in the install, newest first -- the order the egui combo
    /// lists them.
    builds: Vec<BuildNumber>,
    selected_build: Option<BuildNumber>,
    /// The directory and build whose VFS the package pane currently holds.
    loaded_source: Option<(PathBuf, BuildNumber)>,
    /// Bumped per build-load request so a stale result can be dropped.
    load_generation: u64,
    /// The package VFS of the loaded build, for the parameters dump. Held here
    /// rather than reached out of the pane, which owns its own copy for
    /// browsing.
    package_vfs: Option<wowsunpack::vfs::VfsPath>,
    /// What the last parameters dump did, shown beside the button.
    dump_status: Option<String>,
    build_select: Entity<SelectState<SearchableVec<BuildItem>>>,
    wows_dir: Option<PathBuf>,
    dock_area: Entity<DockArea>,
    pkg_browser: Entity<BrowserPanel>,
    assets_browser: Entity<BrowserPanel>,
    /// Entries queued for extraction. The shared queue owns the rules about
    /// what queueing a listing means and about queueing the same entry twice.
    queue: ExtractQueue,
    extract_state: ExtractState,
    /// Set to stop an in-flight extraction; replaced per run.
    stop_flag: Arc<AtomicBool>,
    /// Where an extraction writes. Empty until the user picks one, which is
    /// what keeps Extract disabled; shared with the egui app, so a directory
    /// chosen in either is the one both use.
    output_dir: String,
    output_dir_input: Entity<InputState>,
    /// Whether an extraction rewrites decodable assets.bin prototypes as
    /// JSON. Session state, as in the egui app, which does not persist it.
    prototypes: PrototypeOutput,
    /// One per open search panel, kept so its events keep reaching this tab.
    search_subscriptions: Vec<Subscription>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl UnpackerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pkg_browser = cx.new(|cx| BrowserPanel::new(BrowserSource::Pkg, window, cx));
        let assets_browser = cx.new(|cx| BrowserPanel::new(BrowserSource::AssetsBin, window, cx));
        // The skinned area is what draws a tab bar over a group holding more
        // than one panel; a bare one renders only the displayed panel.
        let (dock_area, _) = DockSkin::dock_area("unpacker-dock", None, window, cx);

        dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(pkg_browser.clone()), DockPlacement::Center, None, window, cx);
            dock.add_panel_view(panel_handle(assets_browser.clone()), DockPlacement::Center, None, window, cx);
            // Each add activates what it added, so the last one would be
            // showing; the packages listing is the one the tab opens on.
            dock.select_panel(PanelId::from(pkg_browser.entity_id()), window, cx);
        });

        let build_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(false));

        let output_dir_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.unpacker.output_dir_hint").to_string()));

        let subscriptions = vec![
            cx.subscribe(&output_dir_input, Self::on_output_dir_edited),
            cx.subscribe_in(&pkg_browser, window, Self::on_browser_event),
            cx.subscribe_in(&assets_browser, window, Self::on_browser_event),
            cx.subscribe_in(&build_select, window, |this, _state, event, window, cx| {
                let SelectEvent::Confirm(Some(build)) = event else {
                    return;
                };
                this.select_build(*build, window, cx);
            }),
        ];

        Self {
            builds: Vec::new(),
            selected_build: None,
            loaded_source: None,
            load_generation: 0,
            package_vfs: None,
            dump_status: None,
            build_select,
            wows_dir: None,
            dock_area,
            pkg_browser,
            assets_browser,
            queue: ExtractQueue::new(),
            output_dir: String::new(),
            output_dir_input,
            prototypes: PrototypeOutput::Raw,
            extract_state: ExtractState::Idle,
            stop_flag: Arc::new(AtomicBool::new(false)),
            search_subscriptions: Vec::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Adopts the saved extraction directory and seeds the path field with it.
    pub fn set_output_dir(&mut self, output_dir: String, window: &mut Window, cx: &mut Context<Self>) {
        self.output_dir_input.update(cx, |state, cx| state.set_value(output_dir.clone(), window, cx));
        self.output_dir = output_dir;
        cx.notify();
    }

    /// Saved on blur or Enter rather than per keystroke, so a half-typed path
    /// never reaches the database.
    fn on_output_dir_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let path = state.read(cx).value().trim().to_string();
        self.store_output_dir(path, cx);
    }

    fn store_output_dir(&mut self, output_dir: String, cx: &mut Context<Self>) {
        if self.output_dir == output_dir {
            return;
        }
        self.output_dir = output_dir.clone();
        crate::settings_store::save(setting_keys::OUTPUT_DIR, &output_dir, cx);
        cx.notify();
    }

    fn browse_for_output_dir(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder("Extract to");
        cx.spawn(async move |this, cx| {
            let Some(picked) = asked.await else { return };
            let picked = picked.to_string_lossy().into_owned();
            let _ = this.update_in(cx, |this, window, cx| {
                this.output_dir_input.update(cx, |state, cx| state.set_value(picked.clone(), window, cx));
                this.store_output_dir(picked, cx);
            });
        })
        .detach();
    }

    /// Adopts the WoWs directory, enumerates its builds and loads the newest.
    /// An empty directory clears the panes rather than leaving stale data.
    pub fn apply_settings(&mut self, wows_dir: String, window: &mut Window, cx: &mut Context<Self>) {
        if wows_dir.is_empty() {
            self.wows_dir = None;
            self.builds.clear();
            self.selected_build = None;
            self.loaded_source = None;
            self.pkg_browser.update(cx, |pane, cx| pane.clear(window, cx));
            self.assets_browser.update(cx, |pane, cx| pane.clear(window, cx));
            cx.notify();
            return;
        }

        let dir = PathBuf::from(&wows_dir);
        let mut builds: Vec<BuildNumber> = match wowsunpack::game_data::list_available_builds(&dir) {
            Ok(builds) => builds.into_iter().map(BuildNumber).collect(),
            Err(err) => {
                tracing::warn!("unpacker: could not list builds in {}: {err}", dir.display());
                Vec::new()
            }
        };
        builds.sort_unstable();
        builds.reverse();

        self.wows_dir = Some(dir);
        self.build_select.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(builds.iter().copied().map(BuildItem).collect::<Vec<_>>()), window, cx);
        });
        self.builds = builds;

        match self.builds.first().copied() {
            Some(newest) => self.select_build(newest, window, cx),
            None => {
                self.selected_build = None;
                self.pkg_browser.update(cx, |pane, cx| pane.clear(window, cx));
                self.assets_browser.update(cx, |pane, cx| pane.clear(window, cx));
                cx.notify();
            }
        }
    }

    /// Loads `build`'s package VFS in the background and hands it to the
    /// package pane. The assets.bin pane stays on its own loading path.
    ///
    /// The guard keys on the directory as well as the build: two installs can
    /// carry the same build number, and comparing the build alone would leave
    /// the previous install's VFS on screen under the new directory.
    fn select_build(&mut self, build: BuildNumber, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.wows_dir.clone() else { return };
        if self.loaded_source.as_ref() == Some(&(dir.clone(), build)) {
            return;
        }
        self.selected_build = Some(build);
        self.loaded_source = Some((dir.clone(), build));

        if let Some(index) = self.builds.iter().position(|b| *b == build) {
            self.build_select.update(cx, |state, cx| {
                state.set_selected_index(Some(IndexPath::new(index)), window, cx);
            });
        }

        // Stamps this request so a slower earlier load cannot overwrite a
        // later one that already finished.
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;

        self.pkg_browser.update(cx, |pane, cx| pane.set_loading(cx));
        self.assets_browser.update(cx, |pane, cx| pane.set_loading(cx));
        let pkg_browser = self.pkg_browser.clone();
        let assets_browser = self.assets_browser.clone();
        cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    wowsunpack::game_data::build_game_vfs_for_build(&dir, build.0).map_err(|err| format!("{err}"))
                })
                .await;

            let still_current = this.update(cx, |this, _cx| this.load_generation == generation).unwrap_or(false);
            if !still_current {
                return;
            }

            let package_vfs = match loaded {
                Ok(vfs) => {
                    let handed = vfs.clone();
                    let held = vfs.clone();
                    let _ = this.update(cx, |this, _cx| this.package_vfs = Some(held));
                    let _ = pkg_browser.update_in(cx, |pane, window, cx| pane.set_vfs(handed, window, cx));
                    vfs
                }
                Err(reason) => {
                    pkg_browser.update(cx, |pane, cx| pane.set_failed(reason.clone(), cx));
                    assets_browser.update(cx, |pane, cx| pane.set_failed(reason, cx));
                    return;
                }
            };

            // assets.bin is a separate archive inside that VFS: read and parsed
            // after the package tree, so the pane beside it is usable first.
            let opened =
                cx.background_spawn(async move { assets_bin::open(&package_vfs).map_err(|err| err.to_string()) }).await;

            let still_current = this.update(cx, |this, _cx| this.load_generation == generation).unwrap_or(false);
            if !still_current {
                return;
            }

            let _ = assets_browser.update_in(cx, |pane, window, cx| match opened {
                Ok(vfs) => pane.set_vfs(vfs, window, cx),
                Err(reason) => pane.set_failed(reason, cx),
            });
        })
        .detach();

        cx.notify();
    }

    fn on_browser_event(
        &mut self,
        _browser: &Entity<BrowserPanel>,
        event: &BrowserEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BrowserEvent::Extract(entries) => {
                self.queue.push_listed_files(entries);
                self.publish_queue(cx);
            }
            BrowserEvent::Queue(path) => {
                self.queue.push(path.clone());
                self.publish_queue(cx);
            }
            BrowserEvent::Unqueue(path) => {
                self.queue.remove(path);
                self.publish_queue(cx);
            }
            BrowserEvent::Search { source, query, path_filter, files } => {
                self.open_search(*source, query.clone(), path_filter.clone(), files.clone(), _window, cx);
            }
            BrowserEvent::View { path } => self.open_viewer(path.clone(), _window, cx),
            BrowserEvent::ViewAsJson { path } => self.open_json_viewer(path.clone(), _window, cx),
            BrowserEvent::ExtractAsJson { path } => self.extract_as_json(path.clone(), cx),
        }
    }

    /// Opens a results panel and starts the scan behind it. Each run gets its
    /// own panel, matching the egui app, so an earlier result set survives.
    fn open_search(
        &mut self,
        source: BrowserSource,
        query: String,
        path_filter: String,
        files: Arc<FileList>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pattern) = compile_query(&query) else {
            tracing::warn!("unpacker: the search query {query:?} could not be compiled");
            return;
        };

        let stop_flag = Arc::new(AtomicBool::new(false));
        let panel = cx.new(|cx| SearchPanel::new(query.into(), source, stop_flag.clone(), cx));
        self.dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(panel.clone()), DockPlacement::Bottom, None, window, cx);
        });
        self.search_subscriptions.push(cx.subscribe_in(&panel, window, Self::on_search_panel_event));

        // Weak, so the running scan does not keep a closed panel alive; the
        // panel's own `Drop` then sets the stop flag and the worker winds up.
        let panel = panel.downgrade();
        cx.spawn(async move |_this, cx| {
            let (tx, mut rx) = futures::channel::mpsc::unbounded::<SearchUpdate>();
            let worker = cx.background_spawn(async move {
                let targets = files_to_scan(&files, &path_filter);
                scan(
                    &targets,
                    &pattern,
                    |hit| {
                        let _ = tx.unbounded_send(SearchUpdate::Hit(hit));
                    },
                    |progress| {
                        let _ = tx.unbounded_send(SearchUpdate::Progress(progress));
                    },
                    || stop_flag.load(Ordering::Relaxed),
                );
            });

            // Drained in batches: a broad query matches faster than the
            // foreground can repaint, and applying one hit per wake-up turns
            // the scan into a per-hit relayout.
            while let Some(first) = futures::StreamExt::next(&mut rx).await {
                let mut hits = Vec::new();
                let mut latest_progress = None;
                let mut pending = Some(first);
                while let Some(update) = pending.take().or_else(|| rx.try_recv().ok()) {
                    match update {
                        SearchUpdate::Hit(hit) => hits.push(hit),
                        SearchUpdate::Progress(progress) => latest_progress = Some(progress),
                    }
                }

                let applied = panel.update(cx, |panel, cx| {
                    if let Some(progress) = latest_progress {
                        panel.set_progress(progress, cx);
                    }
                    panel.extend_hits(hits, cx);
                });
                if applied.is_err() {
                    return;
                }
            }

            worker.await;
            let _ = panel.update(cx, |panel, cx| panel.finish(cx));
        })
        .detach();
    }

    fn on_search_panel_event(
        &mut self,
        _panel: &Entity<SearchPanel>,
        event: &SearchPanelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SearchPanelEvent::View { path } = event;
        self.open_viewer(path.clone(), _window, cx);
    }

    /// Opens a file in its viewer, or logs why it has none.
    ///
    /// Reading happens on the UI thread: the viewable files are the small text
    /// and image ones, and the egui viewer reads them inline too.
    fn open_viewer(&mut self, path: VfsPath, window: &mut Window, cx: &mut Context<Self>) {
        let content = match viewer::load(&path) {
            Ok(content) => content,
            Err(err) => {
                tracing::warn!("unpacker: {err}");
                return;
            }
        };

        let title = SharedString::from(path.as_str().trim_start_matches('/').to_string());
        let panel = cx.new(|cx| FileViewerPanel::new(title, content, cx));
        self.dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        });
        cx.notify();
    }

    /// Writes the build's parameters out in `format`.
    ///
    /// The minimal formats write the toolkit's own decoded parameters, which
    /// this tab does not hold, so they are not offered here; the two raw
    /// formats are.
    fn dump_game_params(&mut self, format: GameParamsFormat, base_only: bool, cx: &mut Context<Self>) {
        let Some(vfs) = self.package_vfs.clone() else {
            self.dump_status = Some("No build is loaded".to_string());
            cx.notify();
            return;
        };
        let Some(build) = self.selected_build else {
            self.dump_status = Some("No build is loaded".to_string());
            cx.notify();
            return;
        };
        // The minimal formats write the toolkit's own decoded set, which is
        // the same file for the whole tree; "base parameters only" is a cut of
        // the raw pickled tree and has no meaning for them.
        let base_only = base_only && !format.is_minimal();
        let stem = if format.is_minimal() { "MinGameParams" } else { "GameParams" };
        let asked =
            crate::dialog::save_file(Some("Save game parameters"), &format!("{stem}.{}", format.extension()), None);

        cx.spawn(async move |this, cx| {
            let Some(path) = asked.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.dump_status = Some("Writing...".to_string());
                cx.notify();
            });
            let written = cx
                .background_spawn(async move {
                    if format.is_minimal() {
                        let params = crate::replay_inspector::load_game_params(&vfs, build.0)
                            .map_err(|err| GameParamsDumpError::Decode(err.to_string()))?;
                        game_params::write_value(&params, &path, format)
                    } else {
                        game_params::dump_pickled(&vfs, &path, format, base_only)
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.dump_status = Some(match written {
                    Ok(()) => "Parameters written".to_string(),
                    Err(err) => format!("{err}"),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Decodes an assets.bin prototype and writes the JSON where the user
    /// asks, which is what the egui listing's own item does.
    fn extract_as_json(&mut self, path: VfsPath, cx: &mut Context<Self>) {
        let name = path.filename();
        let stem = name.rsplit_once('.').map(|(stem, _)| stem.to_string()).unwrap_or(name);
        let asked = crate::dialog::save_file(Some("Extract as JSON"), &format!("{stem}.json"), None);
        cx.spawn(async move |this, cx| {
            let Some(target) = asked.await else { return };
            let written = cx
                .background_spawn(async move {
                    let json = viewer::decode_to_json(&path).map_err(|err| err.to_string())?;
                    std::fs::write(&target, json).map_err(|err| err.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.dump_status = Some(match written {
                    Ok(()) => "Prototype written".to_string(),
                    Err(err) => err,
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Decodes an assets.bin prototype and shows the JSON in a viewer tab.
    fn open_json_viewer(&mut self, path: VfsPath, window: &mut Window, cx: &mut Context<Self>) {
        let json = match viewer::decode_to_json(&path) {
            Ok(json) => json,
            Err(err) => {
                tracing::warn!("unpacker: {err}");
                return;
            }
        };

        let title = SharedString::from(format!("{} (JSON)", path.as_str().trim_start_matches('/')));
        let content = viewer::ViewerContent::Plaintext { extension: ".json".to_string(), text: json };
        let panel = cx.new(|cx| FileViewerPanel::new(title, content, cx));
        self.dock_area.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        });
        cx.notify();
    }

    /// Drops one entry from the queue, from the queue popover's per-row
    /// remove button.
    fn remove_from_queue(&mut self, path: VfsPath, cx: &mut Context<Self>) {
        self.queue.remove(&path);
        self.publish_queue(cx);
    }

    fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.queue.clear();
        self.extract_state = ExtractState::Idle;
        self.publish_queue(cx);
    }

    /// Hands both browser panes the queued paths, so their listing rows show
    /// what is queued. One shared set rather than a copy per pane: a path is
    /// queued for the tab, not for the pane it was queued from.
    fn publish_queue(&mut self, cx: &mut Context<Self>) {
        let queued: Rc<HashSet<String>> =
            Rc::new(self.queue.entries().iter().map(|path| path.as_str().to_string()).collect());
        self.pkg_browser.update(cx, |pane, cx| pane.set_queued(queued.clone(), cx));
        self.assets_browser.update(cx, |pane, cx| pane.set_queued(queued, cx));
        cx.notify();
    }

    /// Asks for a destination, then writes the queue to it off the UI thread.
    fn start_extraction(&mut self, cx: &mut Context<Self>) {
        if self.queue.is_empty() || matches!(self.extract_state, ExtractState::Counting | ExtractState::Running(_)) {
            return;
        }
        if self.output_dir.is_empty() {
            return;
        }
        let output_dir = extract_root(Path::new(&self.output_dir));

        let queued = self.queue.take();
        self.publish_queue(cx);
        let prototypes = self.prototypes;
        let stop_flag = Arc::new(AtomicBool::new(false));
        self.stop_flag = stop_flag.clone();
        self.extract_state = ExtractState::Counting;

        cx.spawn(async move |this, cx| {
            let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<ExtractProgress>();
            let worker = cx.background_spawn(async move {
                let files = expand_to_files(&queued);
                extract_files(
                    &files,
                    &output_dir,
                    prototypes,
                    |progress| {
                        let _ = progress_tx.unbounded_send(progress);
                    },
                    || stop_flag.load(Ordering::Relaxed),
                )
            });

            while let Some(progress) = futures::StreamExt::next(&mut progress_rx).await {
                let _ = this.update(cx, |this, cx| {
                    this.extract_state = ExtractState::Running(progress);
                    cx.notify();
                });
            }

            let outcome = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.extract_state = match outcome {
                    Ok(outcome) => ExtractState::Done(outcome),
                    Err(err) => ExtractState::Failed(format!("{err}")),
                };
                cx.notify();
            });
        })
        .detach();

        cx.notify();
    }

    fn cancel_extraction(&mut self, cx: &mut Context<Self>) {
        self.stop_flag.store(true, Ordering::Relaxed);
        cx.notify();
    }
}

/// The parameters-dump dropdown, mirroring the egui app's "Dump Game Params"
/// menu: each of the four formats, and the same four again limited to the
/// base entry. A disabled trigger says the build has no VFS yet rather than
/// opening a menu whose every item would fail.
fn dump_params_popover(view: Entity<UnpackerView>, enabled: bool) -> impl IntoElement {
    let trigger = Button::new("unpacker-dump-params")
        .label(t!("ui.unpacker.dump_parameters").to_string())
        .compact()
        .disabled(!enabled)
        .tooltip(if enabled {
            t!("ui.unpacker.dump_parameters_tooltip").into_owned()
        } else {
            t!("ui.unpacker.load_a_build").into_owned()
        });

    Popover::new("unpacker-dump-params-menu").trigger(trigger).content(move |_state, _window, _cx| {
        let full = GameParamsFormat::ALL
            .into_iter()
            .map(|format| dump_params_item(view.clone(), format, false))
            .collect::<Vec<_>>();
        // The base-entry cut only applies to the raw tree, so the minimal
        // formats are not offered a second time here.
        let base = GameParamsFormat::ALL
            .into_iter()
            .filter(|format| !format.is_minimal())
            .map(|format| dump_params_item(view.clone(), format, true))
            .collect::<Vec<_>>();

        v_flex()
            .max_w(DUMP_POPOVER_MAX_WIDTH)
            .gap_1()
            .p_1()
            .child(div().text_xs().font_weight(FontWeight::BOLD).child(t!("ui.unpacker.whole_tree").to_string()))
            .children(full)
            .child(div().text_xs().font_weight(FontWeight::BOLD).child(t!("ui.unpacker.base_only").to_string()))
            .children(base)
    })
}

/// One dump-menu entry.
fn dump_params_item(view: Entity<UnpackerView>, format: GameParamsFormat, base_only: bool) -> impl IntoElement {
    let id = format!("unpacker-dump-{}{}", if base_only { "base-" } else { "" }, format.label().replace(' ', "-"));

    Button::new(SharedString::from(id.to_lowercase())).label(format.label()).compact().on_click(
        move |_event, _window, cx: &mut App| {
            view.update(cx, |this, cx| this.dump_game_params(format, base_only, cx));
        },
    )
}

/// The extraction queue dropdown, mirroring the egui app's queue button and
/// its popup: the count on the trigger, one removable row per entry, and a
/// "Clear all" that empties it. Paths read as the game's own `res/` layout,
/// which is where they land on disk.
fn queue_popover(view: Entity<UnpackerView>, entries: Vec<VfsPath>) -> impl IntoElement {
    let label = match entries.len() {
        0 => t!("ui.unpacker.queue").into_owned(),
        count => t!("ui.unpacker.queued_count", count = count).into_owned(),
    };
    let trigger = Button::new("unpacker-queue-trigger")
        .child(crate::icons::icon(crate::icons::LIST_CHECKS))
        .label(label)
        .compact();

    Popover::new("unpacker-queue").trigger(trigger).content(move |_state, _window, _cx| {
        let view = view.clone();
        let rows = entries.iter().cloned().map(|entry| queue_row(view.clone(), entry)).collect::<Vec<_>>();
        let clear_view = view.clone();

        v_flex()
            .max_w(QUEUE_POPOVER_MAX_WIDTH)
            .gap_1()
            .p_1()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(t!("ui.unpacker.extraction_queue").to_string()),
                    )
                    .child(
                        Button::new("unpacker-queue-clear-all")
                            .label(t!("ui.unpacker.clear_all").to_string())
                            .compact()
                            .on_click(move |_event, _window, cx: &mut App| {
                                clear_view.update(cx, |this, cx| this.clear_queue(cx));
                            }),
                    ),
            )
            // A plain scroll container, not the kit's `overflow_y_scrollbar`:
            // the overlaid scrollbar keeps requesting frames, which leaves a
            // headless test spinning on a popover that never settles.
            .child(
                v_flex()
                    .id("unpacker-queue-list")
                    .max_h(QUEUE_POPOVER_MAX_HEIGHT)
                    .overflow_y_scroll()
                    .gap_1()
                    .children(rows),
            )
    })
}

/// One queued entry: its path and a button that drops it.
fn queue_row(view: Entity<UnpackerView>, entry: VfsPath) -> impl IntoElement {
    let path = entry.as_str().trim_start_matches('/').to_string();
    let remove = entry.clone();

    h_flex()
        .gap_1()
        .items_center()
        .justify_between()
        .child(div().flex_1().min_w(px(0.)).text_xs().truncate().child(format!("res/{path}")))
        .child(
            Button::new(SharedString::from(format!("unpacker-queue-remove-{path}")))
                .icon(IconName::Close)
                .compact()
                .tooltip(t!("ui.unpacker.remove_from_queue").to_string())
                .on_click(move |_event, _window, cx: &mut App| {
                    let remove = remove.clone();
                    view.update(cx, |this, cx| this.remove_from_queue(remove, cx));
                }),
        )
}

impl Focusable for UnpackerView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for UnpackerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        // The egui tab shows the version bar only when there is a choice to
        // make; a single-build install gets no chrome for it.
        let version_bar = (self.builds.len() > 1).then(|| {
            h_flex()
                .flex_none()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border)
                .child(
                    div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.unpacker.version").to_string()),
                )
                .child(Select::new(&self.build_select).id("unpacker-build").small().w(px(160.)))
        });

        let status: Option<String> = match &self.extract_state {
            ExtractState::Idle => None,
            ExtractState::Counting => Some(t!("ui.unpacker.counting_files").into_owned()),
            ExtractState::Running(progress) => {
                Some(t!("ui.unpacker.extracting", done = progress.written, total = progress.total).into_owned())
            }
            ExtractState::Failed(reason) => Some(t!("ui.unpacker.extract_failed", reason = reason).into_owned()),
            ExtractState::Done(ExtractOutcome::Completed { written }) => {
                Some(t!("ui.unpacker.extracted", count = written).into_owned())
            }
            ExtractState::Done(ExtractOutcome::Stopped { written }) => {
                Some(t!("ui.unpacker.cancelled", count = written).into_owned())
            }
        };
        let running = match &self.extract_state {
            ExtractState::Running(progress) => Some(*progress),
            _ => None,
        };
        let busy = running.is_some() || matches!(self.extract_state, ExtractState::Counting);

        // The egui button carries the queue count in its own label rather
        // than only beside it.
        let queued = self.queue.len();
        let extract_label = match queued {
            0 => t!("ui.unpacker.extract").into_owned(),
            1 => t!("ui.unpacker.extract_one").into_owned(),
            count => t!("ui.unpacker.extract_many", count = count).into_owned(),
        };

        let output_bar = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                Button::new("unpacker-browse-output")
                    .label(t!("ui.unpacker.browse").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event, window, cx| this.browse_for_output_dir(window, cx))),
            )
            .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.output_dir_input).id("unpacker-output-dir")))
            .child(
                Checkbox::new("unpacker-decode-prototypes")
                    .label(t!("ui.unpacker.decode_prototypes").to_string())
                    .checked(self.prototypes == PrototypeOutput::DecodeToJson)
                    .tooltip(t!("ui.unpacker.decode_prototypes_tooltip").into_owned())
                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                        this.prototypes = if *checked { PrototypeOutput::DecodeToJson } else { PrototypeOutput::Raw };
                        cx.notify();
                    })),
            );

        let queue_popover = queue_popover(cx.entity(), self.queue.entries().to_vec());

        let queue_bar = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(queue_popover)
            .when_some(running, |this, progress| {
                this.child(
                    div()
                        .w(px(160.))
                        .child(Progress::new("unpacker-extract-progress").value(progress.fraction() * 100.)),
                )
            })
            .when_some(status, |this, status| {
                this.child(div().flex_1().text_xs().text_color(crate::theme::text_dim()).child(status))
            })
            .child(
                Button::new("unpacker-extract")
                    .label(extract_label)
                    .compact()
                    .disabled(self.queue.is_empty() || busy || self.output_dir.is_empty())
                    .when(self.output_dir.is_empty(), |this| {
                        this.tooltip(t!("ui.unpacker.choose_directory").into_owned())
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| this.start_extraction(cx))),
            )
            .child(
                Button::new("unpacker-cancel")
                    .label(t!("ui.buttons.cancel").to_string())
                    .compact()
                    .disabled(!busy)
                    .on_click(cx.listener(|this, _event, _window, cx| this.cancel_extraction(cx))),
            )
            .child(dump_params_popover(cx.entity(), self.package_vfs.is_some()))
            .when_some(self.dump_status.clone(), |this, status| {
                this.child(div().text_xs().text_color(crate::theme::text_dim()).child(status))
            })
            .child(
                Button::new("unpacker-clear-queue")
                    .label(t!("ui.stats.clear").to_string())
                    .compact()
                    .disabled(self.queue.is_empty())
                    .on_click(cx.listener(|this, _event, _window, cx| this.clear_queue(cx))),
            );

        v_flex()
            .id("unpacker-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .when_some(version_bar, |this, bar| this.child(bar))
            .child(queue_bar)
            .child(output_bar)
            .child(div().flex_1().min_h(px(0.)).child(self.dock_area.clone()))
    }
}

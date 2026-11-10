//! The Resource Unpacker tab: a build selector over a dock holding one
//! browser pane per VFS source, the extraction queue those panes feed, and
//! the viewer and search panels they open.
//!
//! What to list, filter, search and extract is
//! `wows_toolkit_viewmodel::unpacker`, shared with the other front ends;
//! this module is the NSH view of it.

mod browser;
mod dump;
mod error;
mod listing;
mod load;
mod queue;
mod search;
mod text;
mod ui;
mod viewer;

use std::path::PathBuf;
use std::rc::Rc;

use nsh_base::SelectItem;
use nsh_base::SelectValue;
use nsh_base::dock::DockArea;
use nsh_base::dock::DockAreaApi;
use nsh_base::dock::DockAreaLayout;
use nsh_base::dock::DockInfo;
use nsh_base::dock::DockLayout;
use nsh_base::dock::DockPanel;
use nsh_base::dock::DockPanelOptions;
use nsh_base::dock::DockPlacement;
use nsh_base::dock::DockRegion;
use nsh_base::dock::DockTab;
use nsh_base::dock::PanelId;
use nsh_base::dock::dock_area;
use nsh_base::select;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::unpacker::assets_bin;
use wows_toolkit_viewmodel::unpacker::listing::ListingEntry;
use wows_toolkit_viewmodel::unpacker::viewer::ViewerContent;
use wowsunpack::vfs::VfsPath;

use crate::icons::Icon;

use self::browser::BrowserPane;
use self::browser::browser_view;
use self::dump::DumpSource;
use self::error::UnpackerError;
use self::listing::BrowserSource;
use self::listing::Shared;
use self::listing::Walked;
use self::load::BuildNumber;
use self::load::UnpackerSettings;
use self::queue::QueueState;
use self::queue::queue_view;
use self::search::SearchRun;
use self::search::search_view;
use self::text::Text;
use self::ui::caption;
use self::ui::class;
use self::ui::icon_button;
use self::viewer::ViewerDocument;
use self::viewer::viewer_view;

/// The queue panel's key in the dock.
const QUEUE_PANEL: &str = "queue";

/// How wide the queue dock opens beside the listing.
const QUEUE_DOCK_WIDTH: Points = px(320.0);

/// How tall the search dock opens below the listing.
const SEARCH_DOCK_HEIGHT: Points = px(260.0);

#[derive(Debug, Clone, PartialEq, Props)]
pub struct UnpackerProps {
    /// The locale the tab's text is shown in.
    pub locale: Prop<Option<SharedString>>,
    /// Whether the tab is showing. The shared settings are read again each
    /// time it is shown, so a WoWs directory set in Settings takes effect.
    #[prop(default)]
    pub active: Prop<bool>,
}

/// A panel opened from a listing or a search.
#[derive(Clone)]
struct OpenedPanel {
    id: PanelId,
    title: String,
    content: OpenedContent,
}

#[derive(Clone)]
enum OpenedContent {
    Viewer(Rc<ViewerDocument>),
    Search(SearchRun),
}

/// Which build-load request a result answers, so a stale one is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoadGeneration(u64);

impl LoadGeneration {
    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// The tab's state and what acts on it. `Copy`: handles only, so every
/// closure in the view can hold it.
#[derive(Clone, Copy)]
pub struct Tab {
    /// The catalog in the shown locale; a change remakes the tab's view.
    text: Signal<Text>,
    /// The directory and build whose VFS the panes hold, so applying the
    /// same settings twice does not reload it.
    wows_dir: Stored<Option<PathBuf>>,
    builds: Signal<Vec<BuildNumber>>,
    selected_build: Signal<Option<BuildNumber>>,
    loaded_source: Stored<Option<(PathBuf, BuildNumber)>>,
    load_generation: Stored<LoadGeneration>,
    /// The loaded build's package VFS, for the GameParams export.
    dump_source: Signal<Option<DumpSource>>,
    /// What the last export or failed action did, beside the export menu.
    status: Signal<Option<String>>,
    pkg: BrowserPane,
    assets: BrowserPane,
    queue: QueueState,
    opened: Stored<Vec<OpenedPanel>>,
    next_panel: Stored<u32>,
    dock: ComponentRef<DockArea>,
    layout: Signal<DockAreaLayout>,
}

impl Tab {
    fn new(locale: Option<&str>) -> Self {
        Self {
            text: signal_with(Text::new(locale), SignalEquals::Never),
            wows_dir: stored(None),
            builds: signal(Vec::new()),
            selected_build: signal(None),
            loaded_source: stored(None),
            load_generation: stored(LoadGeneration(0)),
            dump_source: signal_with(None, SignalEquals::Never),
            status: signal(None),
            pkg: BrowserPane::new(BrowserSource::Pkg),
            assets: BrowserPane::new(BrowserSource::AssetsBin),
            queue: QueueState::new(),
            opened: stored(Vec::new()),
            next_panel: stored(0),
            dock: component_ref::<DockArea>(),
            layout: signal(initial_layout()),
        }
    }

    pub fn text(&self, key: &str) -> String {
        self.text.with(|text| text.get(key))
    }

    pub fn text_with(&self, key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
        self.text.with(|text| text.with(key, args))
    }

    pub fn set_status(&self, status: String) {
        self.status.set(Some(status));
    }

    /// Empty until an action reports: the caption beside the export menu
    /// shows nothing.
    pub fn status_text(&self) -> String {
        self.status.get().unwrap_or_default()
    }

    pub fn dump_source(&self) -> Option<DumpSource> {
        self.dump_source.get_untracked()
    }

    pub fn dump_source_loaded(&self) -> bool {
        self.dump_source.with(Option::is_some)
    }

    pub fn is_queued(&self, path: &VfsPath) -> bool {
        self.queue.is_queued(path)
    }

    pub fn toggle_queued(&self, path: VfsPath) {
        self.queue.toggle(path);
    }

    pub fn queue_path(&self, path: VfsPath) {
        self.queue.push(path);
    }

    pub fn queue_listed(&self, entries: Vec<ListingEntry>) {
        self.queue.push_listed(&entries);
    }

    fn pane(&self, source: BrowserSource) -> BrowserPane {
        match source {
            BrowserSource::Pkg => self.pkg,
            BrowserSource::AssetsBin => self.assets,
        }
    }

    fn with_dock(&self, f: impl FnOnce(&DockAreaApi)) {
        if let Some(api) = self.dock.api::<DockAreaApi>() {
            f(&api);
        }
    }

    /// Reads the shared settings off the engine thread and applies them.
    fn refresh_settings(&self) {
        let tab = *self;
        spawn(async move {
            let settings = blocking(load::read_settings)
                .await
                .map_err(|source| UnpackerError::Task { source })
                .and_then(|settings| settings);
            match settings {
                Ok(settings) => tab.apply_settings(settings),
                Err(error) => tab.set_status(error.to_string()),
            }
        });
    }

    /// Adopts the WoWs directory, enumerates its builds and loads the
    /// newest. An unset directory clears the panes rather than leaving
    /// stale data.
    fn apply_settings(&self, settings: UnpackerSettings) {
        if self.queue.output_dir.get_untracked() != settings.output_dir {
            self.queue.set_output_dir(settings.output_dir);
        }

        if self.wows_dir.get() == settings.wows_dir && self.loaded_source.get().is_some() {
            return;
        }
        self.wows_dir.replace(settings.wows_dir.clone());

        let Some(dir) = settings.wows_dir else {
            self.builds.set(Vec::new());
            self.selected_build.set(None);
            self.loaded_source.replace(None);
            self.dump_source.set(None);
            self.pkg.clear();
            self.assets.clear();
            return;
        };

        let tab = *self;
        spawn(async move {
            let listed = {
                let dir = dir.clone();
                blocking(move || load::list_builds(&dir))
                    .await
                    .map_err(|source| UnpackerError::Task { source })
                    .and_then(|builds| builds)
            };
            let builds = match listed {
                Ok(builds) => builds,
                Err(error) => {
                    let error = Shared::new(error);
                    tab.pkg.set_failed(Shared::clone(&error));
                    tab.assets.set_failed(error);
                    Vec::new()
                }
            };
            tab.builds.set(builds.clone());

            match builds.first() {
                Some(newest) => tab.select_build(*newest),
                None => {
                    tab.selected_build.set(None);
                    tab.dump_source.set(None);
                }
            }
        });
    }

    /// Loads `build`'s package VFS in the background and hands it to the
    /// package pane, then parses assets.bin out of it for the pane beside.
    ///
    /// The guard keys on the directory as well as the build: two installs
    /// can carry the same build number.
    fn select_build(&self, build: BuildNumber) {
        let Some(dir) = self.wows_dir.get() else {
            return;
        };
        if self.loaded_source.get().as_ref() == Some(&(dir.clone(), build)) {
            return;
        }
        self.selected_build.set(Some(build));
        self.loaded_source.replace(Some((dir.clone(), build)));

        let generation = self.load_generation.with(|generation| {
            *generation = generation.next();
            *generation
        });
        self.dump_source.set(None);
        self.pkg.set_loading();
        self.assets.set_loading();

        let tab = *self;
        spawn(async move {
            let loaded = blocking(move || {
                let vfs = load::load_package_vfs(&dir, build)?;
                Ok::<_, UnpackerError>(Walked::walk(vfs))
            })
            .await
            .map_err(|source| UnpackerError::Task { source })
            .and_then(|loaded| loaded);
            if tab.load_generation.get() != generation {
                return;
            }

            let package_vfs = match loaded {
                Ok(walked) => {
                    let vfs = walked.vfs.clone();
                    tab.dump_source.set(Some(DumpSource { vfs: vfs.clone(), build }));
                    tab.pkg.set_ready(walked);
                    vfs
                }
                Err(error) => {
                    let error = Shared::new(error);
                    tab.pkg.set_failed(Shared::clone(&error));
                    tab.assets.set_failed(error);
                    return;
                }
            };

            let opened = blocking(move || assets_bin::open(&package_vfs).map(Walked::walk))
                .await
                .map_err(|source| UnpackerError::Task { source })
                .and_then(|opened| opened.map_err(UnpackerError::from));
            if tab.load_generation.get() != generation {
                return;
            }

            match opened {
                Ok(walked) => tab.assets.set_ready(walked),
                Err(error) => tab.assets.set_failed(Shared::new(error)),
            }
        });
    }

    fn add_panel(&self, title: String, content: OpenedContent, placement: DockPlacement, size: Option<Points>) {
        let id = self.next_panel.with(|next| {
            *next += 1;
            PanelId::from(format!("opened-{next}"))
        });
        self.opened.with(|opened| opened.push(OpenedPanel { id: id.clone(), title, content }));
        self.with_dock(|dock| {
            dock.add_panel(id, Some(placement), size);
            if placement != DockPlacement::Center {
                dock.set_dock_open(placement, true);
            }
        });
    }

    /// Opens a file in its viewer, or says why it has none. Reading happens
    /// here: the viewable files are the small text and image ones.
    pub fn open_viewer(&self, path: VfsPath) {
        match wows_toolkit_viewmodel::unpacker::viewer::load(&path) {
            Ok(content) => {
                let title = path.as_str().trim_start_matches('/').to_owned();
                let document = ViewerDocument { title: title.clone(), content };
                self.add_panel(title, OpenedContent::Viewer(Rc::new(document)), DockPlacement::Center, None);
            }
            Err(error) => self.set_status(error.to_string()),
        }
    }

    /// Decodes an assets.bin prototype and shows the JSON in a viewer.
    pub fn open_json_viewer(&self, path: VfsPath) {
        match wows_toolkit_viewmodel::unpacker::viewer::decode_to_json(&path) {
            Ok(json) => {
                let title = format!("{} (JSON)", path.as_str().trim_start_matches('/'));
                let content = ViewerContent::Plaintext { extension: ".json".to_owned(), text: json };
                let document = ViewerDocument { title: title.clone(), content };
                self.add_panel(title, OpenedContent::Viewer(Rc::new(document)), DockPlacement::Center, None);
            }
            Err(error) => self.set_status(error.to_string()),
        }
    }

    pub fn extract_as_json(&self, path: VfsPath) {
        dump::extract_as_json(*self, path);
    }

    /// Opens a results panel and starts the scan behind it.
    pub fn start_search(&self, pane: BrowserPane) {
        let query = pane.search_query.get_untracked().trim().to_owned();
        if query.is_empty() {
            return;
        }
        let Some(files) = pane.files() else {
            return;
        };
        let path_filter = pane.path_filter.get_untracked().trim().to_owned();

        match SearchRun::start(pane.source, query, path_filter, files) {
            Ok(run) => {
                let title = run.title(*self);
                self.add_panel(title, OpenedContent::Search(run), DockPlacement::Bottom, Some(SEARCH_DOCK_HEIGHT));
            }
            Err(error) => self.set_status(error.to_string()),
        }
    }

    /// Shows `path` in the tree it came from and brings that pane forward.
    pub fn reveal(&self, source: BrowserSource, path: VfsPath) {
        self.pane(source).reveal(&path);
        self.with_dock(|dock| dock.select_panel(PanelId::from(source.panel_id())));
    }

    fn panel_removed(&self, id: &PanelId) {
        self.opened.with(|opened| {
            let Some(at) = opened.iter().position(|panel| panel.id == *id) else {
                return;
            };
            let panel = opened.remove(at);
            // Closing the results stops the scan feeding them and frees
            // what it found.
            if let OpenedContent::Search(run) = panel.content {
                run.stop();
                run.hits.set(Vec::new());
            }
        });
    }

    fn opened_panel(&self, id: &PanelId) -> Option<OpenedPanel> {
        self.opened.with(|opened| opened.iter().find(|panel| panel.id == *id).cloned())
    }

    fn panel_title(&self, id: &PanelId) -> String {
        match id.as_str() {
            QUEUE_PANEL => self.text("ui.unpacker.extraction_queue"),
            key if key == BrowserSource::Pkg.panel_id() => self.text(BrowserSource::Pkg.title_key()),
            key if key == BrowserSource::AssetsBin.panel_id() => self.text(BrowserSource::AssetsBin.title_key()),
            // A layout naming a panel this session never opened: its tab
            // has no title to show.
            _ => self.opened_panel(id).map(|panel| panel.title).unwrap_or_default(),
        }
    }

    fn render_panel(&self, id: &PanelId) -> AnyElement {
        match id.as_str() {
            QUEUE_PANEL => queue_view(self.queue, *self),
            key if key == BrowserSource::Pkg.panel_id() => browser_view(self.pkg, *self),
            key if key == BrowserSource::AssetsBin.panel_id() => browser_view(self.assets, *self),
            _ => match self.opened_panel(id).map(|panel| panel.content) {
                Some(OpenedContent::Viewer(document)) => viewer_view(&document, *self),
                Some(OpenedContent::Search(run)) => search_view(run, *self),
                // A layout naming a panel this session never opened: it has
                // nothing to show.
                None => view().into_element(),
            },
        }
    }

    fn render_tab(&self, dock_tab: &DockTab) -> AnyElement {
        let tab = *self;
        let title = self.panel_title(&dock_tab.panel);
        let panel = dock_tab.panel.clone();
        let close = dock_tab.closable.then(|| {
            icon_button(Icon::X, format!("Close {title}"))
                .on_press(move |_: &PressEvent| tab.with_dock(|dock| dock.close_panel(panel.clone())))
        });

        view()
            .app_class(class("flex-row items-center gap-1 px-2"))
            .child(text(title).app_class(class("text-xs")))
            .children(close)
            .into_element()
    }

    /// A side or bottom dock's header control that puts it away.
    fn render_dock(&self, info: &DockInfo) -> AnyElement {
        let tab = *self;
        let placement = info.placement;
        let glyph = match placement {
            DockPlacement::Left => Icon::PanelLeftClose,
            DockPlacement::Bottom => Icon::PanelBottom,
            DockPlacement::Right | DockPlacement::Center => Icon::PanelRightClose,
        };
        view()
            .app_class(class("flex-row justify-end px-1"))
            .child(
                icon_button(glyph, self.text("ui.buttons.collapse_panel"))
                    .on_press(move |_: &PressEvent| tab.with_dock(|dock| dock.set_dock_open(placement, false))),
            )
            .into_element()
    }

    /// Whether the dock at `placement` holds panels but is put away, so a
    /// rail has to offer it back.
    fn dock_closed(&self, placement: DockPlacement) -> bool {
        self.layout.with(|layout| {
            let region = match placement {
                DockPlacement::Right => layout.right.as_ref(),
                DockPlacement::Bottom => layout.bottom.as_ref(),
                DockPlacement::Left => layout.left.as_ref(),
                DockPlacement::Center => None,
            };
            region.is_some_and(|region| region.open == Some(false))
        })
    }

    /// Selects the build a selector item names. Every item is a build this
    /// tab listed, so only a cleared selection names none.
    fn select_build_value(&self, value: &Option<SharedString>) {
        let Some(build) = value.as_deref().and_then(|value| BuildNumber::parse(value).ok()) else {
            return;
        };
        self.select_build(build);
    }
}

/// The packages listing and assets.bin as tabs of the center, packages
/// showing, and the queue in the right dock.
fn initial_layout() -> DockAreaLayout {
    let mut queue = DockRegion::new(DockLayout::tabs().panel(QUEUE_PANEL));
    queue.size = Some(QUEUE_DOCK_WIDTH);

    DockAreaLayout::new(
        DockLayout::tabs()
            .panel(BrowserSource::Pkg.panel_id())
            .panel(BrowserSource::AssetsBin.panel_id())
            .active_index(0),
    )
    .with_dock(DockPlacement::Right, queue)
}

/// The panels nothing reopens, so none of them may be closed.
fn fixed_panels() -> Vec<DockPanelOptions> {
    [BrowserSource::Pkg.panel_id(), BrowserSource::AssetsBin.panel_id(), QUEUE_PANEL]
        .into_iter()
        .map(|id| {
            let mut options = DockPanelOptions::new(id);
            options.closable = Some(false);
            options
        })
        .collect()
}

#[component]
pub fn Unpacker(props: UnpackerProps) -> impl IntoElement {
    let initial = props.locale.get_untracked();
    let tab = Tab::new(initial.as_deref());
    let shown_locale = stored(initial);

    effect(move || {
        if props.active.get() {
            tab.refresh_settings();
        }
    });

    effect(move || {
        let locale = props.locale.get();
        if shown_locale.with(|shown| *shown != locale) {
            tab.text.set(Text::new(locale.as_deref()));
            shown_locale.replace(locale);
        }
    });

    view().app_class(class("flex-1 min-h-0")).child(dynamic(move || {
        tab.text.with(|_| ());
        untracked(|| tab_view(tab))
    }))
}

/// The tab's view; made again when the locale changes, as GPUI words every
/// frame in the current one. Its state lives in `tab`, so nothing is lost.
fn tab_view(tab: Tab) -> AnyElement {
    // The version bar shows only when there is a choice to make.
    let version_bar = when(
        move || tab.builds.with(|builds| builds.len() > 1),
        move || {
            view()
                .app_class(class("flex-row items-center gap-2"))
                .child(caption(tab.text("ui.unpacker.version")))
                .child(
                    select()
                        .accessibility_label(tab.text("ui.unpacker.version"))
                        .items(move || {
                            tab.builds.with(|builds| {
                                builds
                                    .iter()
                                    .map(|build| SelectItem::new(build.to_string(), build.to_string()))
                                    .collect::<Vec<_>>()
                            })
                        })
                        .value(move || {
                            SelectValue::Controlled(
                                tab.selected_build.get().map(|build| SharedString::from(build.to_string())),
                            )
                        })
                        .app_class(class(
                            "w-40 min-h-7 px-2 rounded-(--radius) border border-input bg-background text-xs",
                        ))
                        .on_change(move |value: &Option<SharedString>| tab.select_build_value(value)),
                )
        },
    );

    let toolbar = view()
        .app_class(class("flex-row flex-wrap items-center gap-2 px-3 py-2 border-b border-border"))
        .child(version_bar)
        .child(dump::dump_menu(tab))
        .child(dump::dump_status(tab));

    let area = dock_area()
        .component_ref(&tab.dock)
        .app_class(class("flex-1 min-w-0 min-h-0"))
        .layout(move || tab.layout.get())
        .panels(fixed_panels())
        .tab_bar_class("flex-row items-center gap-1 h-8 px-1 border-b border-border bg-card")
        .tab_class("h-7 rounded-(--radius) text-muted-foreground selected:bg-accent selected:text-foreground")
        .content_class("flex-1 min-h-0")
        .group_class("flex-1 min-h-0 bg-background")
        .dock_class("bg-background border-border")
        .handle_class("bg-border")
        .render_panel(move |panel: &DockPanel| tab.render_panel(&panel.id))
        .render_tab(move |dock_tab: &DockTab| tab.render_tab(dock_tab))
        .render_dock(move |info: &DockInfo| tab.render_dock(info))
        .on_layout_change(move |next: &DockAreaLayout| tab.layout.set(next.clone()))
        .on_panel_remove(move |panel: &PanelId| tab.panel_removed(panel));

    let rail = |placement: DockPlacement, glyph: Icon| {
        when(
            move || tab.dock_closed(placement),
            move || {
                icon_button(glyph, tab.text("ui.buttons.expand_panel"))
                    .on_press(move |_: &PressEvent| tab.with_dock(|dock| dock.set_dock_open(placement, true)))
            },
        )
    };

    view()
        .app_class(class("flex-1 min-h-0 bg-background"))
        .child(toolbar)
        .child(
            view()
                .app_class(class("flex-1 min-h-0 flex-row"))
                .child(
                    view().app_class(class("items-center pt-1")).child(rail(DockPlacement::Left, Icon::PanelLeftOpen)),
                )
                .child(
                    view().app_class(class("flex-1 min-w-0 min-h-0")).child(area).child(
                        view()
                            .app_class(class("flex-row justify-center"))
                            .child(rail(DockPlacement::Bottom, Icon::PanelBottomOpen)),
                    ),
                )
                .child(
                    view()
                        .app_class(class("items-center pt-1"))
                        .child(rail(DockPlacement::Right, Icon::PanelRightOpen)),
                ),
        )
        .into_element()
}

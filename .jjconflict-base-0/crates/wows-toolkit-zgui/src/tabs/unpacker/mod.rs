//! The Unpacker tab: a build selector over a dock holding one browser pane per VFS source, the
//! extraction queue those panes feed, and a panel per content search and per opened file.
//!
//! The version selector appears only when the install carries more than one build.

mod browser;
mod model;
mod pane;
mod queue;
mod search;
#[cfg(test)]
mod tests;
mod viewer;

use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::PanelEntry;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::PanelSide;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::unpacker::build::BuildNumber;
use wows_toolkit_viewmodel::unpacker::game_params::DumpRequest;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SelectContentProps;
use zgui_ui::SelectItemProps;
use zgui_ui::SelectProps;
use zgui_ui::SelectTriggerProps;
use zgui_ui::SelectTriggerSize;
use zgui_ui::SelectValueProps;
use zgui_ui::SeparatorOrientation;
use zgui_ui::SeparatorProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui::prelude::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::PanelToggle;
use crate::shell::tabs::TabChrome;
use crate::shell::tabs::panel_side_name;
use crate::tabs::unpacker::browser::BrowserPaneProps;
use crate::tabs::unpacker::model::BROWSER_KIND;
use crate::tabs::unpacker::model::QUEUE_KIND;
use crate::tabs::unpacker::model::QUEUE_PANEL;
use crate::tabs::unpacker::model::SEARCH_KIND;
use crate::tabs::unpacker::model::UnpackerModel;
use crate::tabs::unpacker::model::VIEWER_KIND;
use crate::tabs::unpacker::pane::BrowserSource;
use crate::tabs::unpacker::queue::QueuePanelProps;
use crate::tabs::unpacker::search::SearchPanelProps;
use crate::tabs::unpacker::viewer::ViewerPanelProps;

const SHEET: &str = "wt-unpacker";

const CSS: &str = css!(
    ".wt-unpacker { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-unpacker-toolbar {
        display: flex;
        flex: none;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-unpacker-toolbar__label { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-unpacker-toolbar__build { width: 160px; }
    .wt-unpacker-toolbar__status { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-unpacker-dump { display: flex; flex-direction: column; gap: 4px; padding: 4px; max-width: 220px; }
    .wt-unpacker-dump__heading { font-size: 12px; font-weight: 700; }
    .wt-unpacker-dock { display: flex; flex: 1 1 auto; min-height: 0; }"
);

/// The dock's side regions as status-bar toggles, each test-findable by side.
const DOCK_SIDES: [(DockRegion, PanelSide, &str); 3] = [
    (DockRegion::Left, PanelSide::Left, "unpacker-dock-toggle-left"),
    (DockRegion::Right, PanelSide::Right, "unpacker-dock-toggle-right"),
    (DockRegion::Bottom, PanelSide::Bottom, "unpacker-dock-toggle-bottom"),
];

#[cfg(test)]
thread_local! {
    /// The model of the tab built last on this thread, for tests to drive.
    static BUILT: std::cell::Cell<Option<UnpackerModel>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub fn built_model() -> Option<UnpackerModel> {
    BUILT.with(std::cell::Cell::get)
}

#[component]
pub fn UnpackerTab() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let shell = Shell::current();
    let dock = DockState::new(PanelRegistry::new());
    let model = UnpackerModel::new(services, shell, dock);
    #[cfg(test)]
    BUILT.with(|built| built.set(Some(model)));
    register_panels(model);
    lay_out(model);
    dock_toggles(dock, TabChrome::current());

    // The install and the output directory follow the settings.
    keep(RenderEffect::new(move |_| {
        if let Some(dir) = services.settings.read(|settings| settings.wows_dir.clone()) {
            model.apply_wows_dir(dir);
        }
    }));
    keep(RenderEffect::new(move |_| {
        if let Some(dir) = services.settings.read(|settings| settings.output_dir.clone()) {
            model.adopt_output_dir(dir);
        }
    }));
    // A closed search panel stops its scan.
    keep(RenderEffect::new(move |_| {
        let _ = dock.panels();
        model.forget_closed_panels();
    }));

    let locale = services.locale;
    view! {
        column(class = "wt-unpacker", attr:data-testid = "unpacker-tab") {
            // Rebuilt with the language, so every label it holds follows it.
            {move || {
                locale.track();
                view! { Toolbar(model = model) }
            }}
            Dock(
                state = dock,
                class = "wt-unpacker-dock",
                attr:data-testid = "unpacker-dock"
            )
        }
    }
}

fn register_panels(model: UnpackerModel) {
    let locale = model.services.locale;
    let dock = model.dock;
    dock.register(
        PanelKind::new(BROWSER_KIND),
        PanelFactory::new(
            |id: &PanelId| browser_source(id).map(BrowserSource::title).unwrap_or_default(),
            move |id: &PanelId| match browser_source(id) {
                Some(source) => {
                    let pane = model.pane(source);
                    AnyView::new(move || {
                        locale.track();
                        view! { BrowserPane(model = model, pane = pane) }
                    })
                }
                None => AnyView::new(()),
            },
        ),
    );
    dock.register(
        PanelKind::new(QUEUE_KIND),
        PanelFactory::new(
            |_: &PanelId| t!("ui.unpacker.extraction_queue").into_owned(),
            move |_: &PanelId| {
                AnyView::new(move || {
                    locale.track();
                    view! { QueuePanel(model = model) }
                })
            },
        ),
    );
    dock.register(
        PanelKind::new(SEARCH_KIND),
        PanelFactory::new(
            move |id: &PanelId| {
                model
                    .searches
                    .with_value(|searches| searches.get(id).map(|results| results.title()))
                    .unwrap_or_default()
            },
            move |id: &PanelId| match model.searches.with_value(|searches| searches.get(id).copied()) {
                Some(results) => AnyView::new(view! { SearchPanel(model = model, results = results) }),
                None => AnyView::new(()),
            },
        ),
    );
    dock.register(
        PanelKind::new(VIEWER_KIND),
        PanelFactory::new(
            move |id: &PanelId| {
                model.viewers.with_value(|viewers| viewers.get(id).map(|doc| doc.title.get_value())).unwrap_or_default()
            },
            move |id: &PanelId| match model.viewers.with_value(|viewers| viewers.get(id).copied()) {
                Some(doc) => AnyView::new(view! { ViewerPanel(doc = doc) }),
                None => AnyView::new(()),
            },
        ),
    );
}

fn browser_source(id: &PanelId) -> Option<BrowserSource> {
    BrowserSource::ALL.into_iter().find(|source| source.id_fragment() == id.as_str())
}

/// The two browsers in one center group, the package listing in front, and the queue on the
/// right.
fn lay_out(model: UnpackerModel) {
    let dock = model.dock;
    let mut group = None;
    for source in BrowserSource::ALL {
        // Not closable: nothing reopens a browser.
        let entry = PanelEntry {
            closable: false,
            ..PanelEntry::new(PanelId::new(source.id_fragment()), PanelKind::new(BROWSER_KIND))
        };
        let target = group.map_or(DockTarget::Region(DockRegion::Center), DockTarget::Group);
        match dock.add_panel(target, entry) {
            Ok(added) => group = Some(added),
            Err(err) => tracing::error!("unpacker: the {source:?} browser was not docked: {err}"),
        }
    }
    // Not closable: it is the only place the extraction controls live.
    let queue =
        PanelEntry { closable: false, ..PanelEntry::new(PanelId::new(QUEUE_PANEL), PanelKind::new(QUEUE_KIND)) };
    if let Err(err) = dock.add_panel(DockTarget::Region(DockRegion::Right), queue) {
        tracing::error!("unpacker: the queue was not docked: {err}");
    }
    if let Err(err) = dock.activate(&PanelId::new(BrowserSource::Pkg.id_fragment())) {
        tracing::error!("unpacker: the package browser was not shown: {err}");
    }
}

/// Puts a status-bar toggle for each side region holding a panel, and keeps the toggle and the
/// region's open state in step.
fn dock_toggles(dock: DockState, chrome: Option<TabChrome>) {
    let opens = DOCK_SIDES.map(|(region, _, _)| RwSignal::new_local(dock.is_region_open(region)));
    for ((region, _, _), open) in DOCK_SIDES.into_iter().zip(opens) {
        keep(RenderEffect::new(move |_| {
            let shown = dock.is_region_open(region);
            if open.get_untracked() != shown {
                open.set(shown);
            }
        }));
        keep(RenderEffect::new(move |_| {
            let wanted = open.get();
            if dock.is_region_open(region) != wanted {
                dock.set_region_open(region, wanted);
            }
        }));
    }
    let Some(chrome) = chrome else { return };
    let locale = Services::current().locale;
    keep(RenderEffect::new(move |_| {
        locale.track();
        let toggles = DOCK_SIDES
            .into_iter()
            .zip(opens)
            .filter(|((region, _, _), _)| !dock.is_region_empty(*region))
            .map(|((region, side, test_id), open)| {
                let name = match region {
                    DockRegion::Right => t!("ui.unpacker.show_queue").into_owned(),
                    _ => panel_side_name(side),
                };
                PanelToggle { test_id, name, side, open }
            })
            .collect();
        chrome.set_toggles(toggles);
    }));
}

/// Keeps `effect` running until the calling scope is cleaned up.
fn keep(effect: RenderEffect<()>) {
    on_cleanup_local(move || drop(effect));
}

/// The build the listing is of, and the dump that acts on the build rather than on the queue.
#[component]
fn Toolbar(model: UnpackerModel) -> impl IntoView {
    let chosen =
        Signal::derive_local(move || model.selected_build.get().map(|build| build.to_string()).unwrap_or_default());
    let choose = move |chosen: String| {
        if let Ok(build) = chosen.parse::<u32>() {
            model.select_build(BuildNumber(build));
        }
    };
    let builds = Signal::derive_local(move || model.builds.get());
    let loaded = Signal::derive_local(move || model.package_vfs.with(Option::is_some));

    view! {
        row(class = "wt-unpacker-toolbar", attr:data-testid = "unpacker-toolbar") {
            Show(when = move || model.builds.with(|builds| builds.len() > 1)) {
                text(class = "wt-unpacker-toolbar__label") {{t!("ui.unpacker.version").into_owned()}}
                Select(value = Binding::controlled(chosen, choose)) {
                    SelectTrigger(
                        size = SelectTriggerSize::Sm,
                        label = t!("ui.unpacker.version").into_owned(),
                        class = "wt-unpacker-toolbar__build",
                        attr:data-testid = "unpacker-build"
                    ) {
                        SelectValue()
                    }
                    SelectContent {
                        for build in move || builds.get(), key = |build: &BuildNumber| build.0 {
                            SelectItem(value = build.to_string(), text = build.to_string()) {{build.to_string()}}
                        }
                    }
                }
                Separator(orientation = SeparatorOrientation::Vertical)
            }
            DumpMenu(model = model, enabled = loaded)
            text(class = "wt-unpacker-toolbar__status", attr:data-testid = "unpacker-dump-status") {
                {move || model.dump_status.get().unwrap_or_default()}
            }
        }
    }
}

/// The parameters dump: each format for the whole tree, and the raw formats again limited to the
/// base entry. Refused, and saying why, until a build is loaded.
#[component]
fn DumpMenu(model: UnpackerModel, enabled: Signal<bool, LocalStorage>) -> impl IntoView {
    let open = RwSignal::new_local(false);
    let item = move |request: DumpRequest| {
        AnyView::new(view! {
            Button(
                size = ButtonSize::Sm,
                variant = ButtonVariant::Ghost,
                attr:data-testid = format!("unpacker-dump-{}", request.id()),
                on:click = move |_| {
                    open.set(false);
                    model.dump_game_params(request);
                }
            ) {
                {request.format.label()}
            }
        })
    };
    // The popover trigger has no refused state, so a refused dump is a plain refused button.
    let trigger = move || {
        if !enabled.get() {
            return AnyView::new(view! {
                Button(
                    size = ButtonSize::IconSm,
                    variant = ButtonVariant::Ghost,
                    disabled = true,
                    a11y:label = t!("ui.unpacker.dump_parameters").into_owned(),
                    attr:data-testid = "unpacker-dump-params"
                ) {
                    AppIcon(glyph = Glyph::Database)
                }
            });
        }
        let (whole, base) = DumpRequest::menu();
        let whole = move || whole.iter().copied().map(item).collect::<Vec<AnyView>>();
        let base = move || base.iter().copied().map(item).collect::<Vec<AnyView>>();
        AnyView::new(view! {
            Popover(open = open) {
                PopoverTrigger(
                    size = ButtonSize::IconSm,
                    variant = ButtonVariant::Ghost,
                    a11y:label = t!("ui.unpacker.dump_parameters").into_owned(),
                    attr:data-testid = "unpacker-dump-params"
                ) {
                    AppIcon(glyph = Glyph::Database)
                }
                PopoverContent(class = "wt-unpacker-dump", attr:data-testid = "unpacker-dump-menu") {
                    text(class = "wt-unpacker-dump__heading") {{t!("ui.unpacker.whole_tree").into_owned()}}
                    {whole.clone()}
                    text(class = "wt-unpacker-dump__heading") {{t!("ui.unpacker.base_only").into_owned()}}
                    {base.clone()}
                }
            }
        })
    };
    let hover = move || {
        if enabled.get() { t!("ui.unpacker.dump_parameters_tooltip") } else { t!("ui.unpacker.load_a_build") }
            .into_owned()
    };

    view! {
        Tooltip {
            TooltipTrigger {{trigger}}
            TooltipContent {{hover}}
        }
    }
}

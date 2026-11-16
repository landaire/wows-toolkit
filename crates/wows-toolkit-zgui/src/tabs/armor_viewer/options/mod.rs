//! The options panel: a search over every option, a strip of tabs on the panel's outer edge, and
//! the chosen tab's tools and folding sections.
//!
//! - Ship: the hull configuration, the textures and the export.
//! - Layers: what is drawn, from armor zones and plates to hull parts and splash boxes.
//! - Analysis: casting at the hull, from trajectories to the penetration check and splash.
//! - Display: how it is drawn, from the model settings to the camera rings and the lighting.
//!
//! Everything here acts on the active pane, which the header names. A change emits from that
//! pane, and the workspace widens it to the others when the options are synced.
//!
//! The search reaches every tab: while it is active every section is open whatever the reader
//! left it at, a match on a section's title keeps everything in it, and a section with nothing
//! matched is not drawn. Each tab carries how many tools and sections it kept, one that kept none
//! is dimmed, and the first tab holding a match is shown while the chosen one holds none. A
//! collapsed section's body is never built.

mod display;
pub mod filter;
mod hull;
mod splash_zones;
mod tab_strip;
mod textures;
mod tools;
mod tree_filter;
mod visibility;

use std::sync::Arc;

use landaire_ui::EscapeClears;
use rust_i18n::t;
use wows_toolkit_viewmodel::armor::options_panel::OptionSection;
use wows_toolkit_viewmodel::armor::options_panel::OptionSectionsOpen;
use wows_toolkit_viewmodel::armor::options_panel::OptionsTab;
use wows_toolkit_viewmodel::armor::options_panel::TabMatches;
use zgui::component;
use zgui::css;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::icons::AppIconProps;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::options::tab_strip::OptionTabStripProps;
use crate::tabs::armor_viewer::panels::pen::PenCheckPanelProps;
use crate::tabs::armor_viewer::panels::splash::SplashReadoutPanelProps;
use crate::tabs::armor_viewer::panels::trajectory::TrajectoryPanelProps;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::PaneMenu;
use crate::tabs::armor_viewer::viewport::SceneFeatures;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;

pub use crate::tabs::armor_viewer::options::textures::TextureFocus;
pub use crate::tabs::armor_viewer::options::tools::quick_toggles;

const SHEET: &str = "wt-armor-options";

const CSS: &str = css!(
    ".wt-armor-options {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        background-color: var(--lui-color-surface);
    }
    .wt-armor-options__header {
        flex: none;
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-armor-options__title { font-size: 14px; font-weight: 700; }
    .wt-armor-options__target { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-options__target[data-comparing] { font-weight: 600; color: var(--lui-color-accent); }
    .wt-armor-options__filter { display: flex; align-items: center; gap: 4px; }
    .wt-armor-options__field { flex: 1 1 auto; min-width: 0; }
    .wt-armor-options__main { display: flex; flex-direction: row; flex: 1 1 auto; min-height: 0; min-width: 0; }
    .wt-armor-options__body { flex: 1 1 auto; min-width: 0; min-height: 0; overflow-y: auto; }
    .wt-armor-options__content { display: flex; flex-direction: column; gap: 8px; padding: 8px; }
    .wt-armor-options__tab-title { font-size: 12px; font-weight: 700; color: var(--wt-color-text-dim); }
    .wt-armor-options__none { font-size: 13px; color: var(--wt-color-text-dim); }
    .wt-armor-section { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-section__body { padding-left: 4px; }
    .wt-armor-head {
        display: flex;
        align-items: center;
        gap: 4px;
        width: 100%;
        box-sizing: border-box;
        padding: 4px 8px;
        background-color: var(--lui-color-card);
        border-top: 1px solid var(--lui-color-border-bright);
    }
    .wt-armor-head[data-interactive] { cursor: pointer; }
    .wt-armor-head__chevron { transition: transform var(--lui-motion-turn) var(--lui-motion-ease); }
    .wt-armor-head[data-open] .wt-armor-head__chevron { transform: rotate(90deg); }
    .wt-armor-head__label { font-size: 14px; font-weight: 700; }"
);

/// The controls of the trajectory and pen check panels, which a filter finds by these names and
/// then shows whole.
const TRAJECTORY_LABELS: [&str; 17] = [
    "ui.armor.continue_ricochet",
    "ui.armor.trajectory_next_cast",
    "ui.armor.trajectory_range",
    "ui.armor.trajectory_add_ship",
    "ui.armor.trajectory_add_ship_tooltip",
    "ui.armor.trajectory_ship",
    "ui.armor.trajectory_select_ship",
    "ui.armor.trajectory_shell",
    "ui.armor.trajectory_select_shell",
    "ui.armor.trajectory_ship_tooltip",
    "ui.armor.trajectory_shell_tooltip",
    "ui.armor.trajectory_clear",
    "ui.armor.arc_isolate_plates",
    "ui.armor.arc_isolate_plates_tooltip",
    "ui.armor.arc_isolate_zones",
    "ui.armor.arc_isolate_zones_tooltip",
    "ui.armor.arc_delete",
];
const PEN_CHECK_LABELS: [&str; 12] = [
    "ui.armor.pen.ifhe",
    "ui.armor.pen.ifhe_tooltip",
    "ui.armor.pen.clear_all",
    "ui.armor.pen.remove",
    "ui.armor.pen.search_hint",
    "ui.armor.pen.use_for_trajectory",
    "ui.armor.pen.firing_ship_tooltip",
    "ui.armor.realtime.incoming_fire",
    "ui.armor.realtime.attacker_filter",
    "ui.armor.realtime.all_enemies",
    "ui.armor.realtime.show_secondaries",
    "ui.armor.realtime.seek_to_salvo",
];

/// What a section body reads.
#[derive(Clone)]
pub struct SectionCx {
    pub workspace: ArmorWorkspace,
    pub viewport: ArmorViewportState,
    pub filter: RailFilter,
    /// The open sections, as the rail was built from them.
    pub sections: OptionSectionsOpen,
    /// What the scene handed to the pane has, as the rail was built from it.
    pub features: Option<SceneFeatures>,
    /// What decides which rows the filter can match, while it is active. `None` otherwise: each
    /// row then follows that state by itself.
    pub matching: Option<MatchState>,
}

/// What decides which rows the filter can match: the switches that put rows away while they are
/// off, and the counts in the tool labels. While the filter is active the rail is built again when
/// one of these changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchState {
    /// Hits are drawn, so their markers have an opacity to set.
    pub markers: bool,
    pub zoom_path: bool,
    pub perspective: bool,
    /// The openings counted while gap detection is on.
    pub gaps: Option<usize>,
    /// The zones the last burst reached while splash mode is on.
    pub splash: Option<usize>,
}

/// What the viewport's context menu offers from the tab: the rail and the quick toggles.
pub fn pane_menu(workspace: ArmorWorkspace, viewport: ArmorViewportState) -> PaneMenu {
    PaneMenu {
        toggle_options: UnsyncCallback::new(move |()| workspace.toggle_options()),
        quick_toggles: UnsyncCallback::new(move |()| quick_toggles(workspace, viewport)),
    }
}

/// The rail itself.
#[component]
pub fn OptionsRail(workspace: ArmorWorkspace) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = workspace.services.locale;
    let filter_text = workspace.options_filter;
    let features = Memo::new(move |_| workspace.active().scene_features());
    let scene = Memo::new(move |_| workspace.active().scene_key());
    let match_state = Memo::new(move |_| {
        let viewport = workspace.active();
        let gaps = viewport.gaps();
        let splash = viewport.splash_state();
        MatchState {
            markers: viewport.hits_drawn() > 0,
            zoom_path: viewport.camera_rings().zoom_path,
            perspective: viewport.perspective().enabled,
            gaps: gaps.shown.then_some(gaps.count),
            splash: splash.shown.then_some(splash.zones_reached),
        }
    });
    // Written as the content is built, which is where a search counts what each tab kept.
    let matches = workspace.options_matches;

    // With a comparison split open the pane is named in the accent its border is drawn in.
    let target = move || {
        locale.track();
        let position = workspace.active_position();
        let name = workspace.active().shown_ship_name();
        let comparing = position.count > 1;
        let said = match (comparing, name) {
            (true, name) => {
                let pane = t!("ui.armor.options_pane", number = position.number, count = position.count);
                Some(match name {
                    Some(name) => format!("{pane}: {name}"),
                    None => pane.into_owned(),
                })
            }
            (false, name) => name,
        }?;
        Some(view! {
            text(
                class = "wt-armor-options__target",
                attr:data-testid = "armor-options-target",
                attr:data-comparing = comparing.then(String::new)
            ) {{said}}
        })
    };
    let escape = EscapeClears::new(filter_text);

    view! {
        column(class = "wt-armor-options", attr:data-testid = "armor-options-panel") {
            column(class = "wt-armor-options__header") {
                text(class = "wt-armor-options__title") {{move || { locale.track(); t!("ui.armor.options").into_owned() }}}
                {target}
                row(class = "wt-armor-options__filter") {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::Search)
                    Input(
                        value = filter_text,
                        placeholder = t!("ui.armor.filter_options").into_owned(),
                        label = t!("ui.armor.filter_options").into_owned(),
                        class = "wt-armor-options__field",
                        attr:data-testid = "armor-options-filter",
                        on:key_down:capture = move |ev| {
                            escape.key_down(ev);
                        }
                    )
                    if move || !filter_text.get().is_empty() {
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::IconXs,
                            attr:data-testid = "armor-options-filter-clear",
                            a11y:label = t!("ui.stats.clear").into_owned(),
                            on:click = move |_| filter_text.set(String::new())
                        ) {
                            AppIcon(glyph = wows_toolkit_icons::Glyph::X)
                        }
                    }
                }
            }
            row(class = "wt-armor-options__main") {
                box(class = "wt-armor-options__body", attr:data-testid = "armor-options-body") {
                    {move || {
                        locale.track();
                        // Another pane, or another scene in this one, builds the rail again.
                        let pane = workspace.active_id();
                        let shown = (pane, scene.get());
                        let opened = workspace.options_scene.with_value(|last| *last != Some(shown));
                        workspace.options_scene.set_value(Some(shown));
                        let filter = RailFilter::new(&filter_text.get());
                        let matching = filter.is_active().then(|| match_state.get());
                        let inputs = RailInputs {
                            filter,
                            sections: workspace.sections.get(),
                            features: features.get(),
                            matching,
                            tab: workspace.options_tab.get(),
                            opened,
                        };
                        let built = rail_content(workspace, inputs);
                        if matches.get_untracked() != built.matches {
                            matches.set(built.matches);
                        }
                        built.view
                    }}
                }
                OptionTabStrip(workspace = workspace, matches = Signal::from(matches))
            }
        }
    }
}

/// What the rail's content is built from. It is rebuilt when one of these changes, and binds
/// everything else in closures of its own, so a hover or a slider drag does not rebuild it.
struct RailInputs {
    filter: RailFilter,
    sections: OptionSectionsOpen,
    features: Option<SceneFeatures>,
    matching: Option<MatchState>,
    /// The tab the reader chose.
    tab: OptionsTab,
    /// Whether a pane or scene other than the one last built for is shown.
    opened: bool,
}

/// The shown tab's content, and what a search kept in each tab.
struct RailContent {
    view: AnyView,
    /// `None` while nothing is searched for.
    matches: Option<TabMatches>,
}

/// The chosen tab as the filter leaves it. While a search is typed every tab is built to count
/// what it kept, and the first tab holding a match is shown in place of a chosen one with none.
fn rail_content(workspace: ArmorWorkspace, inputs: RailInputs) -> RailContent {
    let RailInputs { filter, sections, features, matching, tab, opened } = inputs;
    let viewport = workspace.active_untracked();
    let cx = SectionCx { workspace, viewport, filter: filter.clone(), sections, features, matching };

    if !filter.is_active() {
        let parts = tab_parts(&cx, tab);
        // A scene opening on a tab with nothing for it, such as Analysis for a model with no
        // armor, opens on the first tab that has something instead. A tab the reader picks is
        // shown as it is, saying it has nothing.
        if opened
            && parts.is_empty()
            && let Some((shown, parts)) = OptionsTab::ALL
                .into_iter()
                .filter(|other| *other != tab)
                .map(|other| (other, tab_parts(&cx, other)))
                .find(|(_, parts)| !parts.is_empty())
        {
            workspace.options_tab.set(shown);
            return RailContent { view: tab_view(shown, parts, false), matches: None };
        }
        return RailContent { view: tab_view(tab, parts, false), matches: None };
    }

    let mut matches = TabMatches::default();
    let mut built: Vec<(OptionsTab, TabParts)> = OptionsTab::ALL
        .into_iter()
        .map(|tab| {
            let parts = tab_parts(&cx, tab);
            matches.add(tab, parts.kept());
            (tab, parts)
        })
        .collect();
    let shown = tab.shown(Some(&matches));
    let parts = built.swap_remove(shown.index()).1;

    RailContent { view: tab_view(shown, parts, matches.total() == 0), matches: Some(matches) }
}

/// What one tab holds, as the filter leaves it.
struct TabParts {
    tools: Vec<AnyView>,
    /// What the splash tool answers. Not an option, so neither the filter keeps it nor a count
    /// includes it.
    readout: Option<AnyView>,
    sections: Vec<AnyView>,
}

impl TabParts {
    /// How many tools and sections the filter kept.
    fn kept(&self) -> usize {
        self.tools.len() + self.sections.len()
    }

    /// Whether the tab has nothing at all for the scene.
    fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.sections.is_empty() && self.readout.is_none()
    }
}

/// The tools and sections of `tab`. Nothing open yet keeps every section, each saying so; an open
/// model drops what it has nothing for.
fn tab_parts(cx: &SectionCx, tab: OptionsTab) -> TabParts {
    let features = cx.features;
    let has_armor = features.is_none_or(|features| features.armor);
    let ship = features.is_some_and(|features| features.ship);
    let tools = tools::tools(cx, tab);
    let mut readout = None;
    let mut sections: Vec<AnyView> = Vec::new();

    match tab {
        OptionsTab::Ship => {
            sections.extend(section(
                cx,
                OptionSection::Configuration,
                &t!("ui.armor.option_tabs.configuration"),
                hull::config_body,
            ));
            sections.extend(section(cx, OptionSection::Textures, &t!("ui.armor.textures.title"), textures::body));
        }
        OptionsTab::Layers => {
            if has_armor {
                sections.extend(section(
                    cx,
                    OptionSection::Visibility,
                    &t!("ui.armor.armor_options"),
                    visibility::body,
                ));
            }
            // Only a ship whose hull ships a splash file has boxes to list.
            if features.is_some_and(|features| features.splash_boxes) {
                sections.extend(section(
                    cx,
                    OptionSection::SplashZones,
                    &t!("ui.armor.splash.section"),
                    splash_zones::body,
                ));
            }
            sections.extend(section(
                cx,
                OptionSection::HullParts,
                &t!("ui.armor.option_tabs.hull_parts"),
                hull::parts_body,
            ));
        }
        OptionsTab::Analysis => {
            if !cx.filter.is_active() && ship {
                readout = Some(splash_readout(cx));
            }
            if has_armor {
                sections.extend(section(cx, OptionSection::Trajectory, &t!("ui.armor.trajectory"), trajectory_body));
                sections.extend(section(cx, OptionSection::PenCheck, &t!("ui.armor.pen_check"), pen_check_body));
            }
        }
        OptionsTab::Display => {
            sections.extend(section(cx, OptionSection::Display, &t!("ui.armor.display"), display::body));
            if ship {
                sections.extend(section(
                    cx,
                    OptionSection::CameraRings,
                    &t!("ui.armor.camera_rings"),
                    display::camera_body,
                ));
            }
            if features.is_some() {
                sections.extend(section(cx, OptionSection::Lighting, &t!("ui.armor.lighting"), display::lighting_body));
            }
        }
    }

    TabParts { tools, readout, sections }
}

/// One tab's content: its tools, then its sections, or a line saying it has nothing for the scene,
/// or that the search matched nothing in any tab.
fn tab_view(tab: OptionsTab, parts: TabParts, nothing_matched: bool) -> AnyView {
    let TabParts { tools, readout, sections } = parts;
    let none = nothing_matched.then(|| {
        view! {
            text(class = "wt-armor-options__none", attr:data-testid = "armor-options-no-match") {{
                t!("ui.armor.no_matching_options").into_owned()
            }}
        }
    });
    let empty = (!nothing_matched && tools.is_empty() && sections.is_empty()).then(|| {
        view! {
            text(class = "wt-armor-options__none", attr:data-testid = "armor-options-tab-empty") {{
                t!("ui.armor.option_tabs.empty").into_owned()
            }}
        }
    });
    let tools = tools::tool_grid(tools);

    AnyView::new(view! {
        column(class = "wt-armor-options__content", attr:data-testid = format!("armor-options-page-{}", tab.id())) {
            text(class = "wt-armor-options__tab-title") {{tab_strip::tab_name(tab)}}
            {tools}
            {readout}
            {sections}
            {empty}
            {none}
        }
    })
}

/// What the splash tool answers, while splash mode is on.
fn splash_readout(cx: &SectionCx) -> AnyView {
    let pen = cx.workspace.pen;
    let viewport = cx.viewport;

    AnyView::new(view! {
        if move || viewport.splash_state().shown {
            SplashReadoutPanel(pen = pen, viewport = viewport)
        }
    })
}

fn trajectory_body(cx: &SectionCx) -> Option<AnyView> {
    cx.filter.keeps_any_key(&TRAJECTORY_LABELS).then(|| {
        let workspace = cx.workspace;
        let open_pen_check = UnsyncCallback::new(move |()| workspace.open_section(OptionSection::PenCheck));
        AnyView::new(view! {
            TrajectoryPanel(pen = workspace.pen, viewport = cx.viewport, on_open_pen_check = Some(open_pen_check))
        })
    })
}

fn pen_check_body(cx: &SectionCx) -> Option<AnyView> {
    cx.filter.keeps_any_key(&PEN_CHECK_LABELS).then(|| {
        let workspace = cx.workspace;
        let library = workspace.library;
        let catalog = Signal::derive_local(move || library.ready().map(|bundle| Arc::clone(&bundle.catalog)));
        let assets = Signal::derive_local(move || library.ready().map(|bundle| Arc::clone(&bundle.assets)));
        let playback_at = Signal::derive_local(move || workspace.playback_at.get());
        let on_seek = UnsyncCallback::new(move |clock| workspace.seek_to(clock));
        AnyView::new(view! {
            PenCheckPanel(
                pen = workspace.pen,
                catalog = catalog,
                assets = assets,
                incoming = workspace.incoming,
                playback_at = playback_at,
                viewport = cx.viewport,
                on_seek = Some(on_seek)
            )
        })
    })
}

/// One section with its folding head. `body` is called only while the section is expanded.
fn section(
    cx: &SectionCx,
    which: OptionSection,
    title: &str,
    body: impl FnOnce(&SectionCx) -> Option<AnyView>,
) -> Option<AnyView> {
    let filter = &cx.filter;
    let marked = filter.label(title.to_owned());
    let (expanded, content) = if filter.is_active() {
        let inner = SectionCx { filter: filter.within(title), ..cx.clone() };
        let content = body(&inner);
        if content.is_none() && marked.is_none() {
            return None;
        }
        (true, content)
    } else {
        let expanded = cx.sections.is_open(which);
        (expanded, if expanded { body(cx) } else { None })
    };
    let title = marked.unwrap_or_else(|| MarkedLabel::plain(title));
    let workspace = cx.workspace;
    // Inert while filtering, so the open state the reader left is what comes back.
    let interactive = !filter.is_active();
    let id = which.id();
    let head = folding_head(
        format!("armor-options-section-{id}"),
        title,
        expanded,
        interactive.then_some(move || workspace.toggle_section(which)),
    );
    let content = content.map(|content| {
        view! { box(class = "wt-armor-section__body", attr:data-testid = format!("armor-options-body-{id}")) {{content}} }
    });

    Some(AnyView::new(view! {
        column(class = "wt-armor-section", attr:data-testid = format!("armor-options-tab-section-{id}")) {
            {head}
            {content}
        }
    }))
}

/// The head of a section that folds away: a band one surface step above the rail, ruled off at
/// its top, so the reader can see where each section begins. `toggle` is `None` while it cannot be
/// folded.
fn folding_head(test_id: String, title: MarkedLabel, open: bool, toggle: Option<impl Fn() + 'static>) -> AnyView {
    let interactive = toggle.is_some();
    let toggle = std::rc::Rc::new(toggle);

    AnyView::new(view! {
        row(
            class = "wt-armor-head",
            attr:data-testid = test_id,
            attr:data-open = open.then(String::new),
            attr:data-interactive = interactive.then(String::new),
            a11y:expanded = open,
            on:click = move |_| {
                if let Some(toggle) = toggle.as_ref() {
                    toggle();
                }
            }
        ) {
            Icon(icon = CHEVRON_RIGHT, size = IconSize::Md, class = "wt-armor-head__chevron")
            box(class = "wt-armor-head__label") {{title.view()}}
        }
    })
}

//! The options rail: the viewport tools, then the trajectory, penetration, armor, hull, textures
//! and display sections.
//!
//! Everything here acts on the active pane, which the header names. A change emits from that
//! pane, and the workspace widens it to the others when the options are synced.
//!
//! The filter reaches every option: while it is active every section is open whatever the reader
//! left it at, a match on a section's title keeps everything in it, and a section with nothing
//! matched is not drawn. A collapsed section's body is never built.

mod display;
pub mod filter;
mod hull;
mod textures;
mod tools;
mod tree_filter;
mod visibility;

use std::sync::Arc;

use landaire_ui::EscapeClears;
use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
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
use crate::shell::tabs::ModifierKey;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::panels::pen::PenCheckPanelProps;
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
    .wt-armor-options__body { flex: 1 1 auto; min-height: 0; overflow-y: auto; }
    .wt-armor-options__content { display: flex; flex-direction: column; gap: 8px; padding: 8px; }
    .wt-armor-options__hint { display: flex; align-items: flex-start; gap: 4px; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-options__hint-text { flex: 1 1 auto; min-width: 0; white-space: normal; }
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
    .wt-armor-head__label { font-size: 14px; font-weight: 700; }
    .wt-armor-head[data-depth=\"group\"] .wt-armor-head__label { font-size: 13px; font-weight: 600; }"
);

/// A section of the options rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionSection {
    Trajectory,
    PenCheck,
    Visibility,
    Hull,
    Textures,
    Display,
    /// The camera ring and camera lock controls inside Display.
    CameraRings,
    /// The lighting controls inside Display.
    Lighting,
}

/// Which sections of the options rail are expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionSectionsOpen {
    pub trajectory: bool,
    pub pen_check: bool,
    pub visibility: bool,
    pub hull: bool,
    pub textures: bool,
    pub display: bool,
    pub camera_rings: bool,
    pub lighting: bool,
}

impl Default for OptionSectionsOpen {
    /// Trajectory and Display: the arcs cast are read there, and Display is the section read most
    /// often. The camera and lighting groups start closed, since most readers only touch the model
    /// settings above them.
    fn default() -> Self {
        Self {
            trajectory: true,
            pen_check: false,
            visibility: false,
            hull: false,
            textures: false,
            display: true,
            camera_rings: false,
            lighting: false,
        }
    }
}

impl OptionSectionsOpen {
    pub fn is_open(self, section: OptionSection) -> bool {
        match section {
            OptionSection::Trajectory => self.trajectory,
            OptionSection::PenCheck => self.pen_check,
            OptionSection::Visibility => self.visibility,
            OptionSection::Hull => self.hull,
            OptionSection::Textures => self.textures,
            OptionSection::Display => self.display,
            OptionSection::CameraRings => self.camera_rings,
            OptionSection::Lighting => self.lighting,
        }
    }

    fn open_mut(&mut self, section: OptionSection) -> &mut bool {
        match section {
            OptionSection::Trajectory => &mut self.trajectory,
            OptionSection::PenCheck => &mut self.pen_check,
            OptionSection::Visibility => &mut self.visibility,
            OptionSection::Hull => &mut self.hull,
            OptionSection::Textures => &mut self.textures,
            OptionSection::Display => &mut self.display,
            OptionSection::CameraRings => &mut self.camera_rings,
            OptionSection::Lighting => &mut self.lighting,
        }
    }

    pub fn open(&mut self, section: OptionSection) {
        *self.open_mut(section) = true;
    }

    pub fn toggle(&mut self, section: OptionSection) {
        let open = self.open_mut(section);
        *open = !*open;
    }

    /// The test id fragment of `section`.
    pub const fn id(section: OptionSection) -> &'static str {
        match section {
            OptionSection::Trajectory => "Trajectory",
            OptionSection::PenCheck => "PenCheck",
            OptionSection::Visibility => "Visibility",
            OptionSection::Hull => "Hull",
            OptionSection::Textures => "Textures",
            OptionSection::Display => "Display",
            OptionSection::CameraRings => "CameraRings",
            OptionSection::Lighting => "Lighting",
        }
    }
}

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
    /// The open sections and groups, as the rail was built from them.
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
    use zgui::reactive::UnsyncCallback;

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
            box(class = "wt-armor-options__body", attr:data-testid = "armor-options-body") {
                {move || {
                    locale.track();
                    // Another pane, or another scene in this one, builds the rail again.
                    let _ = workspace.active_id();
                    let _ = scene.get();
                    let filter = RailFilter::new(&filter_text.get());
                    let matching = filter.is_active().then(|| match_state.get());
                    let sections = workspace.sections.get();
                    let built = RailInputs { filter, sections, features: features.get(), matching };
                    rail_content(workspace, built)
                }}
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
}

/// The tools, the cast hint and the sections, as the filter leaves them.
fn rail_content(workspace: ArmorWorkspace, inputs: RailInputs) -> AnyView {
    let RailInputs { filter, sections, features, matching } = inputs;
    let viewport = workspace.active_untracked();
    let cx = SectionCx { workspace, viewport, filter: filter.clone(), sections, features, matching };

    let tools = tools::tools(&cx);
    // Nothing open yet keeps every section, each saying so; an open model drops what it has
    // nothing for.
    let has_armor = features.is_none_or(|features| features.armor);
    // Where a reader looks for how to cast: a Shift-click does it, with nothing to switch on.
    let cast_hint = (!filter.is_active() && has_armor).then(|| {
        view! {
            row(class = "wt-armor-options__hint", attr:data-testid = "armor-cast-hint") {
                AppIcon(glyph = wows_toolkit_icons::Glyph::Crosshair)
                text(class = "wt-armor-options__hint-text", attr:data-testid = "armor-cast-hint-text") {{
                    t!("ui.armor.cast_hint", key = ModifierKey::Shift.label()).into_owned()
                }}
            }
        }
    });

    let mut sections: Vec<AnyView> = Vec::new();
    // Not built at all for a model without armor.
    if has_armor {
        sections.extend(section(&cx, OptionSection::Trajectory, &t!("ui.armor.trajectory"), |cx| {
            cx.filter.keeps_any_key(&TRAJECTORY_LABELS).then(|| {
                let workspace = cx.workspace;
                let open_pen_check = UnsyncCallback::new(move |()| workspace.open_section(OptionSection::PenCheck));
                AnyView::new(view! {
                    TrajectoryPanel(pen = workspace.pen, viewport = cx.viewport, on_open_pen_check = Some(open_pen_check))
                })
            })
        }));
        sections.extend(section(&cx, OptionSection::PenCheck, &t!("ui.armor.pen_check"), |cx| {
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
        }));
        sections.extend(section(&cx, OptionSection::Visibility, &t!("ui.armor.armor_options"), visibility::body));
    }
    sections.extend(section(&cx, OptionSection::Hull, &t!("ui.armor.hull_toggle"), hull::body));
    sections.extend(section(&cx, OptionSection::Textures, &t!("ui.armor.textures.title"), textures::body));
    sections.extend(section(&cx, OptionSection::Display, &t!("ui.armor.display"), display::body));

    let nothing_matched = filter.is_active() && tools.is_none() && sections.is_empty();
    let none = nothing_matched.then(|| {
        view! {
            text(class = "wt-armor-options__none", attr:data-testid = "armor-options-no-match") {{
                t!("ui.armor.no_matching_options").into_owned()
            }}
        }
    });

    AnyView::new(view! {
        column(class = "wt-armor-options__content") {
            {tools}
            {cast_hint}
            {sections}
            {none}
        }
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
    let id = OptionSectionsOpen::id(which);
    let head = folding_head(
        format!("armor-options-section-{id}"),
        title,
        expanded,
        HeadDepth::Section,
        interactive.then_some(move || workspace.sections.update(|sections| sections.toggle(which))),
    );
    let content = content.map(|content| {
        view! { box(class = "wt-armor-section__body", attr:data-testid = format!("armor-options-body-{id}")) {{content}} }
    });

    Some(AnyView::new(view! {
        column(class = "wt-armor-section") {
            {head}
            {content}
        }
    }))
}

/// How deep a folding head sits in the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadDepth {
    /// A section of the rail.
    Section,
    /// A group inside a section.
    Group,
}

/// The head of a part of the rail that folds away: a band one surface step above the rail, ruled
/// off at its top, so the reader can see where each part begins. A section's title is set larger
/// than a group's inside it. `toggle` is `None` while it cannot be folded.
pub fn folding_head(
    test_id: String,
    title: MarkedLabel,
    open: bool,
    depth: HeadDepth,
    toggle: Option<impl Fn() + 'static>,
) -> AnyView {
    let depth_name = match depth {
        HeadDepth::Section => "section",
        HeadDepth::Group => "group",
    };
    let interactive = toggle.is_some();
    let toggle = std::rc::Rc::new(toggle);

    AnyView::new(view! {
        row(
            class = "wt-armor-head",
            attr:data-testid = test_id,
            attr:data-depth = depth_name,
            attr:data-open = open.then(String::new),
            attr:data-interactive = interactive.then(String::new),
            a11y:expanded = open,
            on:click = move |_| {
                if let Some(toggle) = toggle.as_ref() {
                    toggle();
                }
            }
        ) {
            Icon(icon = CHEVRON_RIGHT, size = IconSize::Sm, class = "wt-armor-head__chevron")
            box(class = "wt-armor-head__label") {{title.view()}}
        }
    })
}

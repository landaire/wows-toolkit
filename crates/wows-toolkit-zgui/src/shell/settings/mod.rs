//! The settings drawer: every setting in one panel that slides over the right of the page, filtered
//! by a search over each setting's name and description, and grouped into sections the strip under
//! the search jumps to.

mod cache;
mod controls;
#[cfg(test)]
mod tests;
mod zoom_anchor;

use landaire_ui::DrawerProps;
use landaire_ui::LoadingPlacement;
use landaire_ui::LoadingProps;
use landaire_ui::MarkedTextProps;
use landaire_ui::PanelWidth;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Track;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputGroupAddonAlign;
use zgui_ui::InputGroupAddonProps;
use zgui_ui::InputGroupButtonProps;
use zgui_ui::InputGroupButtonSize;
use zgui_ui::InputGroupInputProps;
use zgui_ui::InputGroupProps;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::services::SettingsState;
use crate::shell::Shell;
use crate::shell::settings::controls::SettingControlProps;
use crate::shell::settings::controls::SettingNoteProps;
use crate::shell::settings::zoom_anchor::ZoomAnchor;
use crate::text_marks::marks;

pub use wows_toolkit_viewmodel::settings_drawer::Setting;
pub use wows_toolkit_viewmodel::settings_drawer::SettingsSection;

const SHEET: &str = "wt-settings-drawer";

const CSS: &str = css!(
    ".wt-settings__head {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 16px 16px 12px 16px;
    }
    .wt-settings__head .wt-icon { color: var(--lui-color-accent); }
    .wt-settings__title {
        flex: 1 1 auto;
        color: var(--lui-color-text-bright);
        font-size: 14px;
        font-weight: 600;
    }
    .wt-settings__search {
        display: flex;
        flex-direction: column;
        flex: none;
        padding: 0 16px 8px 16px;
    }
    .wt-settings__search .wt-icon { color: var(--lui-color-text-dim); }
    .wt-settings__status {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 0 16px;
    }
    .wt-settings__failed { font-weight: 700; }
    .wt-settings__reason { color: var(--lui-color-text-dim); }
    .wt-settings__body {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-height: 0;
    }
    .wt-settings__count {
        flex: none;
        padding: 0 16px 8px 16px;
        font-size: 11px;
        color: var(--lui-color-text-dim);
    }
    .wt-settings__strip {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: none;
        gap: 4px;
        padding: 0 16px 12px 16px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-settings__strip .wt-icon { color: var(--lui-color-text-dim); }
    .wt-settings__strip [data-selected] {
        background-color: var(--lui-color-selection);
    }
    .wt-settings__strip [data-selected] .wt-icon { color: var(--lui-color-accent); }
    .wt-settings__scroll {
        flex: 1 1 auto;
        min-height: 0;
        overflow-y: auto;
    }
    .wt-settings__list {
        display: flex;
        flex-direction: column;
        /* The engine reserves about 12px on the right for the scrollbar; with it the cards line
           up with the search above them. */
        padding: 12px 4px 16px 16px;
    }
    .wt-settings__heading {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        padding: 20px 0 8px 0;
        font-size: 11px;
        font-weight: 600;
        color: var(--lui-color-text-dim);
    }
    .wt-settings__heading[data-first] { padding-top: 0; }
    .wt-settings__heading .wt-icon { color: var(--lui-color-accent); }
    .wt-settings__card {
        display: flex;
        flex-direction: column;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
        background-color: var(--lui-color-card);
        overflow: hidden;
    }
    .wt-settings__row {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 10px 12px;
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-settings__row:first-child { border-top-width: 0; }
    .wt-settings__row-body {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        column-gap: 12px;
        row-gap: 8px;
    }
    .wt-settings__row-body[data-stacked] {
        flex-direction: column;
        flex-wrap: nowrap;
        align-items: stretch;
        row-gap: 8px;
    }
    .wt-settings__row-text {
        display: flex;
        flex-direction: column;
        flex: 1 1 0;
        gap: 2px;
        min-width: 112px;
    }
    .wt-settings__row-title {
        font-size: 13px;
        font-weight: 500;
        color: var(--lui-color-text-bright);
    }
    .wt-settings__row-description {
        font-size: 11px;
        color: var(--lui-color-text-dim);
    }
    .wt-settings__row-control { flex: none; }
    .wt-settings__row-body[data-stacked] > .wt-settings__row-control { flex: 1 1 auto; }
    .wt-settings__no-match {
        flex: 1 1 auto;
        padding: 16px;
        font-size: 13px;
        color: var(--lui-color-text-dim);
    }
    .wt-settings-open[data-invalid] .wt-icon { color: var(--zui-color-destructive); }
    .wt-settings-open[data-open] { background-color: var(--lui-color-selection); }"
);

/// How wide the drawer is.
const DRAWER_WIDTH: PanelWidth = PanelWidth(440.0);

/// A place in the drawer to bring into view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTarget {
    Section(SettingsSection),
    Setting(Setting),
}

/// Whether the drawer is open, and where it is to be scrolled to.
#[derive(Clone, Copy)]
pub struct SettingsDrawerControl {
    open: RwSignal<bool, LocalStorage>,
    /// The place to bring to the top of the list once it is drawn, as a message about a setting
    /// or a section button asks. Cleared once it is in place.
    jump: RwSignal<Option<SettingsTarget>, LocalStorage>,
    /// Whether the game directory in the field is one an install could be in. The field says so,
    /// and the gear that opens the drawer carries the mark.
    pub wows_dir_invalid: RwSignal<bool, LocalStorage>,
    /// Advanced by each look at a game directory, so an earlier answer arriving late is dropped.
    wows_dir_check: StoredValue<WowsDirCheck>,
}

impl SettingsDrawerControl {
    pub fn new() -> Self {
        Self {
            open: RwSignal::new_local(false),
            jump: RwSignal::new_local(None),
            wows_dir_invalid: RwSignal::new_local(false),
            wows_dir_check: StoredValue::new(WowsDirCheck::default()),
        }
    }

    /// Starts a look at a game directory, replacing any still out.
    pub fn begin_wows_dir_check(self) -> WowsDirCheck {
        self.wows_dir_check.update_value(|check| check.0 += 1);

        self.wows_dir_check.get_value()
    }

    /// Whether `check` is the latest look, `None` once the shell is gone.
    pub fn is_latest_wows_dir_check(self, check: WowsDirCheck) -> Option<bool> {
        self.wows_dir_check.try_get_value().map(|latest| latest == check)
    }

    pub fn is_open(self) -> bool {
        self.open.get()
    }

    pub fn open(self) {
        self.open.set(true);
    }

    /// Opens the drawer with `setting` at the top of the list.
    pub fn open_at(self, setting: Setting) {
        self.jump.set(Some(SettingsTarget::Setting(setting)));
        self.open.set(true);
    }

    pub fn close(self) {
        if self.open.get_untracked() {
            self.open.set(false);
            self.jump.set(None);
        }
    }

    pub fn toggle(self) {
        if self.open.get_untracked() {
            self.close();
        } else {
            self.open();
        }
    }
}

impl Default for SettingsDrawerControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Names one look at a game directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WowsDirCheck(u64);

/// The glyph a section's heading and its button in the strip carry.
pub const fn section_glyph(section: SettingsSection) -> Glyph {
    match section {
        SettingsSection::General => Glyph::Settings,
        SettingsSection::Appearance => Glyph::Paintbrush,
        SettingsSection::Game => Glyph::FolderOpen,
        SettingsSection::GameDataCache => Glyph::Archive,
        SettingsSection::ReplayIndex => Glyph::Database,
        SettingsSection::Replays => Glyph::Table,
        SettingsSection::Collaboration => Glyph::Users,
        SettingsSection::Twitch => Glyph::Twitch,
        SettingsSection::ArmorViewer => Glyph::Shield,
    }
}

/// What a test finds a setting's row by.
pub fn row_test_id(setting: Setting) -> String {
    format!("settings-row-{setting:?}")
}

/// What a test finds a section's heading by.
pub fn section_test_id(section: SettingsSection) -> String {
    format!("settings-section-{section:?}")
}

/// What a test finds a section's button in the strip by.
pub fn category_test_id(section: SettingsSection) -> String {
    format!("settings-category-{section:?}")
}

/// The gear at the end of the tab strip, which opens and shuts the drawer.
#[component]
pub fn SettingsButton(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let control = shell.settings_drawer;
    let tip = locale.text("ui.tabs.settings");

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    class = "wt-settings-open",
                    attr:data-testid = "settings-open",
                    attr:data-open = move || control.is_open().then(String::new),
                    attr:data-invalid = move || control.wows_dir_invalid.get().then(String::new),
                    a11y:label = tip,
                    on:click = move |_| control.toggle()
                ) {
                    AppIcon(glyph = Glyph::Settings)
                }
            }
            TooltipContent {{tip}}
        }
    }
}

/// The drawer over the page. Its content is built each time it opens, so the search starts empty.
#[component]
pub fn SettingsDrawer(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    #[cfg(test)]
    tests::capture(shell);
    let locale = Services::current().locale;

    // The forwarded label replaces the fixed one, so a change of language reaches the drawer.
    view! {
        Drawer(
            open = shell.settings_drawer.open,
            label = t!("ui.tabs.settings").into_owned(),
            width = DRAWER_WIDTH,
            a11y:label = locale.text("ui.tabs.settings"),
            attr:data-testid = "settings-drawer"
        ) {
            DrawerContent(shell = shell)
        }
    }
}

/// How far the settings read has got, without the settings: an edit changes the settings but not
/// this, so the list is not rebuilt by one.
#[derive(Clone, Debug, PartialEq)]
enum SettingsRead {
    Loading,
    Failed(String),
    Loaded,
}

/// The header, the search, and the list for what is typed in it.
#[component]
fn DrawerContent(shell: Shell) -> impl IntoView {
    let services = Services::current();
    let locale = services.locale;
    let control = shell.settings_drawer;
    let close_label = locale.text("ui.settings.panel.close");
    let clear_label = locale.text("ui.buttons.clear");

    let typed = RwSignal::new_local(String::new());
    let query = Signal::derive_local(move || FilterQuery::new(&typed.get()));
    let search = NodeRef::new();

    // The search is ready for typing as the drawer opens.
    let focusing = RenderEffect::new(move |focused: Option<bool>| {
        if focused == Some(true) || search.get().is_none() {
            return false;
        }
        search.focus();
        true
    });
    on_cleanup_local(move || drop(focusing));

    let loaded = Memo::new(move |_| {
        services.settings.with_state(|state| match state {
            SettingsState::Loading => SettingsRead::Loading,
            SettingsState::Failed(reason) => SettingsRead::Failed(reason.clone()),
            SettingsState::Loaded(_) => SettingsRead::Loaded,
        })
    });

    view! {
        row(class = "wt-settings__head") {
            AppIcon(glyph = Glyph::Settings)
            text(class = "wt-settings__title") {{locale.text("ui.tabs.settings")}}
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "settings-close",
                        a11y:label = close_label,
                        on:click = move |_| control.close()
                    ) {
                        AppIcon(glyph = Glyph::X)
                    }
                }
                TooltipContent {{close_label}}
            }
        }
        box(class = "wt-settings__search") {
            InputGroup {
                InputGroupAddon {
                    AppIcon(glyph = Glyph::Search)
                }
                InputGroupInput(
                    value = typed,
                    placeholder = t!("ui.settings.panel.search_placeholder").into_owned(),
                    node_ref = search,
                    a11y:label = t!("ui.settings.panel.search_placeholder").into_owned(),
                    attr:data-testid = "settings-search"
                )
                if move || !typed.get().is_empty() {
                    InputGroupAddon(align = InputGroupAddonAlign::InlineEnd) {
                        InputGroupButton(
                            size = InputGroupButtonSize::IconXs,
                            attr:data-testid = "settings-search-clear",
                            a11y:label = clear_label,
                            on:click = move |_| {
                                typed.set(String::new());
                                search.focus();
                            }
                        ) {
                            AppIcon(glyph = Glyph::X)
                        }
                    }
                } else {}
            }
        }
        {move || match loaded.get() {
            SettingsRead::Loading => AnyView::new(view! {
                Loading(
                    label = Signal::derive_local(locale.text("ui.settings.loading")),
                    placement = LoadingPlacement::Top,
                    attr:data-testid = "settings-loading"
                )
            }),
            SettingsRead::Failed(reason) => AnyView::new(view! {
                column(class = "wt-settings__status") {
                    text(class = "wt-settings__failed") {{locale.text("ui.settings.load_failed")}}
                    text(class = "wt-settings__reason") {{reason.clone()}}
                }
            }),
            SettingsRead::Loaded => AnyView::new(view! { SettingsBody(shell = shell, query = query) }),
        }}
    }
}

/// What the parts of the list share: the rows the search kept, the headings drawn, and the
/// scrolling element.
#[derive(Clone, Copy)]
struct ListContext {
    rows: Memo<Vec<Setting>>,
    /// Each drawn section's heading, for which section is at the top.
    headings: StoredValue<Vec<(SettingsSection, NodeRef)>, LocalStorage>,
    viewport: NodeRef,
}

/// The count, the section strip and the scrolling rows, for `query`.
#[component]
fn SettingsBody(shell: Shell, query: Signal<FilterQuery, LocalStorage>) -> impl IntoView {
    let services = Services::current();
    let locale = services.locale;

    let rows = Memo::new(move |_| {
        let query = query.get();
        locale.track();
        Setting::ALL
            .into_iter()
            .filter(|setting| present(shell, *setting) && setting.matches(&query))
            .collect::<Vec<_>>()
    });
    let viewport = NodeRef::new();
    let headings = StoredValue::new_local(Vec::new());
    let list = ListContext { rows, headings, viewport };
    provide_local_context(list);
    let anchor = ZoomAnchor::follow(shell, viewport);
    provide_local_context(anchor);

    // The cache is measured once per directory, off the UI thread, for the cached builds row.
    let measuring = RenderEffect::new(move |_| {
        let Some(base) = services
            .settings
            .read(|settings| wows_toolkit_services::game_data_cache::base(&settings.game_data_cache_dir))
        else {
            return;
        };
        let Some(base) = base else { return };
        if shell.cache.with(|cache| cache.stats.is_none()) {
            shell.cache.measure_once(base);
        }
    });
    on_cleanup_local(move || drop(measuring));

    let sections = Memo::new(move |_| {
        let rows = rows.get();
        SettingsSection::ALL
            .into_iter()
            .filter(|section| rows.iter().any(|setting| setting.section() == *section))
            .collect::<Vec<_>>()
    });
    let first_section = move || sections.with(|sections| sections.first().copied());

    let scrolled = viewport.observe_scroll();
    let at_top = Memo::new(move |_| {
        scrolled.track();
        rows.track();
        section_at_top(list)
    });

    let count = move || {
        locale.track();
        match rows.get().len() {
            1 => t!("ui.settings.panel.count_one").into_owned(),
            count => t!("ui.settings.panel.count", count = count).into_owned(),
        }
    };

    view! {
        column(class = "wt-settings__body") {
            if move || query.get().is_active() {
                text(class = "wt-settings__count", attr:data-testid = "settings-count", a11y:label = count) {{count}}
            } else {}
            row(class = "wt-settings__strip") {
                for section in move || SettingsSection::ALL.to_vec(), key = |section: &SettingsSection| *section {
                    SectionButton(shell = shell, section = section, at_top = at_top)
                }
            }
            if move || rows.get().is_empty() {
                text(class = "wt-settings__no-match", attr:data-testid = "settings-no-match", a11y:label = locale.text("ui.settings.panel.no_matches")) {
                    {locale.text("ui.settings.panel.no_matches")}
                }
            } else {
                scroll(class = "wt-settings__scroll", node_ref = viewport, attr:data-testid = "settings-drawer-scroll") {
                    column(class = "wt-settings__list") {
                        for section in move || sections.get(), key = |section: &SettingsSection| *section {
                            SectionHeading(shell = shell, section = section, first = Signal::derive_local(move || first_section() == Some(section)))
                            column(class = "wt-settings__card") {
                                for setting in move || rows.get().into_iter().filter(|setting| setting.section() == section).collect::<Vec<_>>(), key = |setting: &Setting| *setting {
                                    SettingRow(shell = shell, setting = setting, query = query)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Whether `setting` has a row at all, which for a few depends on what there is to show.
fn present(shell: Shell, setting: Setting) -> bool {
    let settings = shell.services().settings;
    let armor_saved = settings.read(|settings| settings.armor_defaults.is_some()) == Some(true);
    match setting {
        Setting::CodeIntegrity => cfg!(windows),
        Setting::CachedBuilds => {
            let has_base = settings
                .read(|settings| wows_toolkit_services::game_data_cache::base(&settings.game_data_cache_dir).is_some())
                == Some(true);
            has_base && shell.cache.with(|cache| cache.stats.is_some_and(|stats| stats.version_count > 0))
        }
        Setting::ArmorPlateEdges | Setting::ArmorOpaqueHull => armor_saved,
        Setting::ArmorNoDefaults => !armor_saved,
        _ => true,
    }
}

/// The section whose heading was last scrolled past the top of the list, or the first drawn one
/// while none has been.
fn section_at_top(list: ListContext) -> Option<SettingsSection> {
    let port_top = list.viewport.window_bounds()?.origin.y.0;

    list.headings.with_value(|headings| {
        let mut drawn: Vec<(SettingsSection, f32)> =
            headings.iter().filter_map(|(section, node)| Some((*section, node.window_bounds()?.origin.y.0))).collect();
        drawn.sort_by(|left, right| left.1.total_cmp(&right.1));

        // A heading put at the top by a jump sits on the edge, give or take rounding.
        let passed = drawn.iter().rev().find(|(_, top)| *top <= port_top + 1.0);
        passed.or(drawn.first()).map(|(section, _)| *section)
    })
}

/// A section's button in the strip, which brings its heading to the top of the list.
#[component]
fn SectionButton(shell: Shell, section: SettingsSection, at_top: Memo<Option<SettingsSection>>) -> impl IntoView {
    let list = use_local_context::<ListContext>().expect("a section button is inside the settings list");
    let locale = Services::current().locale;
    let name = move || {
        locale.track();
        section.name()
    };
    let listed = move || list.rows.get().iter().any(|setting| setting.section() == section);
    let current = move || at_top.get() == Some(section);

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    disabled = Signal::derive_local(move || !listed()),
                    attr:data-testid = category_test_id(section),
                    attr:data-selected = move || current().then(String::new),
                    a11y:label = name,
                    a11y:selected = current,
                    on:click = move |_| shell.settings_drawer.jump.set(Some(SettingsTarget::Section(section)))
                ) {
                    AppIcon(glyph = section_glyph(section))
                }
            }
            TooltipContent {{name}}
        }
    }
}

/// A section's name over its card, which the strip's button for it jumps to.
#[component]
fn SectionHeading(shell: Shell, section: SettingsSection, first: Signal<bool, LocalStorage>) -> impl IntoView {
    let list = use_local_context::<ListContext>().expect("a heading is inside the settings list");
    let locale = Services::current().locale;
    let node = NodeRef::new();

    list.headings.update_value(|headings| headings.push((section, node)));
    on_cleanup_local(move || {
        list.headings.try_update_value(|headings| headings.retain(|(drawn, _)| *drawn != section));
    });

    // A jump to this section, or to a setting of it that has no row now (the cache contents
    // before the cache is measured), lands on the heading.
    let wanted = move |target: SettingsTarget| match target {
        SettingsTarget::Section(wanted) => wanted == section,
        SettingsTarget::Setting(setting) => {
            setting.section() == section && !list.rows.with_untracked(|rows| rows.contains(&setting))
        }
    };
    follow_jump(shell, node, wanted);

    view! {
        row(
            class = "wt-settings__heading",
            node_ref = node,
            attr:data-testid = section_test_id(section),
            attr:data-first = move || first.get().then(String::new)
        ) {
            AppIcon(glyph = section_glyph(section))
            text {{move || {
                locale.track();
                section.name()
            }}}
        }
    }
}

/// One setting: its name and description, each marked where `query` matched it, the control that
/// changes it, and a line about it.
#[component]
fn SettingRow(shell: Shell, setting: Setting, query: Signal<FilterQuery, LocalStorage>) -> impl IntoView {
    let locale = Services::current().locale;
    let node = NodeRef::new();
    follow_jump(shell, node, move |target| target == SettingsTarget::Setting(setting));

    view! {
        column(class = "wt-settings__row", node_ref = node, attr:data-testid = row_test_id(setting)) {
            row(class = "wt-settings__row-body", attr:data-stacked = setting.stacked().then(String::new)) {
                column(class = "wt-settings__row-text") {
                    {move || {
                        locale.track();
                        let title = setting.title().into_owned();
                        let ranges = query.with(|query| marks(query, &title));
                        view! {
                            MarkedText(
                                class = "wt-settings__row-title",
                                attr:data-testid = "settings-row-title",
                                text = title,
                                ranges = ranges
                            )
                        }
                    }}
                    {move || {
                        locale.track();
                        let description = setting.description().into_owned();
                        let ranges = query.with(|query| marks(query, &description));
                        view! {
                            MarkedText(
                                class = "wt-settings__row-description",
                                attr:data-testid = "settings-row-description",
                                text = description,
                                ranges = ranges
                            )
                        }
                    }}
                }
                SettingControl(shell = shell, setting = setting)
            }
            SettingNote(shell = shell, setting = setting)
        }
    }
}

/// Scrolls `node` to the top of the list when the drawer is asked to jump to a place `wanted`
/// says is this one, once the node is laid out.
fn follow_jump(shell: Shell, node: NodeRef, wanted: impl Fn(SettingsTarget) -> bool + 'static) {
    let jump = shell.settings_drawer.jump;
    let jumping = RenderEffect::new(move |_| {
        let Some(target) = jump.get() else { return };
        if node.get().is_none() || !wanted(target) {
            return;
        }

        node.scroll_to(ScrollTarget::IntoViewStart, ScrollBehavior::Instant);
        jump.set(None);
    });
    on_cleanup_local(move || drop(jumping));
}

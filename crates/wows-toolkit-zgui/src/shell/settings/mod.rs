//! The settings drawer: every setting in one panel that slides over the right of the page, filtered
//! by a search over each setting's name and description, and grouped into sections the strip under
//! the search jumps to.

mod cache;
mod controls;
mod faults;
mod licenses;
#[cfg(test)]
pub(crate) mod tests;
mod zoom_anchor;

use std::time::Duration;

use landaire_ui::DrawerProps;
use landaire_ui::EscapeClears;
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
use zgui::reactive::prelude::SetValue;
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
use zgui::view::TimeoutHandle;
use zgui::view::Timers;
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
use crate::shell::settings::faults::any_faulted;
use crate::shell::settings::faults::section_faulted;
use crate::shell::settings::faults::setting_faults;
use crate::shell::settings::licenses::LicensesPageProps;
use crate::shell::settings::zoom_anchor::ZoomAnchor;
use crate::text_marks::marks;

pub use faults::watch_auto_export_dir;
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
        padding: 12px 16px 8px 16px;
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
    .wt-settings__search .zui-input-group { min-height: 26px; }
    .wt-settings__search .zui-field.zui-input-group__field {
        height: 24px;
        padding: 2px 8px;
    }
    .wt-settings__search .zui-input-group__addon[data-align] { padding-top: 0; padding-bottom: 0; }
    .wt-settings__search .zui-input-group__addon[data-align=\"inline-start\"] { padding-left: 8px; }
    .wt-settings__search .zui-input-group__addon[data-align=\"inline-end\"] { padding-right: 8px; }
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
        padding: 0 16px 8px 16px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-settings__strip .wt-icon { color: var(--lui-color-text-dim); }
    .wt-settings__strip [data-selected] {
        background-color: var(--lui-color-selection);
    }
    .wt-settings__strip [data-selected] .wt-icon { color: var(--lui-color-accent); }
    .wt-settings__strip [data-error] .wt-icon { color: var(--wt-color-error); }
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
        --wt-settings-row-ground: transparent;
        background-color: var(--wt-settings-row-ground);
    }
    .wt-settings__row:first-child { border-top-width: 0; }
    .wt-settings__row[data-error] {
        --wt-settings-row-ground: color-mix(in srgb, var(--wt-color-error) 10%, transparent);
        box-shadow: inset 3px 0 0 var(--wt-color-error);
    }
    /* One animation under two names, as long as ATTENTION: a changed name starts it over, so a
       second landing replays it. */
    .wt-settings__row[data-attention=\"even\"] { animation: wt-settings-attention-even 1200ms ease-out; }
    .wt-settings__row[data-attention=\"odd\"] { animation: wt-settings-attention-odd 1200ms ease-out; }
    [reduced-motion] .wt-settings__row[data-attention] {
        animation: none;
        background-color: color-mix(in srgb, var(--lui-color-accent) 25%, var(--wt-settings-row-ground));
    }
    @keyframes wt-settings-attention-even {
        from { background-color: color-mix(in srgb, var(--lui-color-accent) 30%, var(--wt-settings-row-ground)); }
        35% { background-color: color-mix(in srgb, var(--lui-color-accent) 30%, var(--wt-settings-row-ground)); }
        to { background-color: var(--wt-settings-row-ground); }
    }
    @keyframes wt-settings-attention-odd {
        from { background-color: color-mix(in srgb, var(--lui-color-accent) 30%, var(--wt-settings-row-ground)); }
        35% { background-color: color-mix(in srgb, var(--lui-color-accent) 30%, var(--wt-settings-row-ground)); }
        to { background-color: var(--wt-settings-row-ground); }
    }
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
    .wt-settings__row-heading {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
    }
    .wt-settings__row-heading > .wt-settings__row-title {
        flex: 1 1 auto;
        min-width: 0;
    }
    .wt-settings__row-fault { color: var(--wt-color-error); }
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
    .wt-settings__foot {
        display: flex;
        flex-direction: row;
        flex: none;
        padding: 8px 16px;
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-settings__foot .wt-icon { color: var(--lui-color-text-dim); }
    .wt-settings-open[data-error] .wt-icon { color: var(--wt-color-error); }
    .wt-settings-open[data-open] { background-color: var(--lui-color-selection); }"
);

/// How wide the drawer is.
const DRAWER_WIDTH: PanelWidth = PanelWidth(440.0);

/// How long a row the drawer was opened at stays highlighted. The highlight's animation in `CSS`
/// runs as long.
const ATTENTION: Duration = Duration::from_millis(1200);

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
    /// Whether the game directory holds no install. Read through `faults`.
    pub wows_dir_invalid: RwSignal<bool, LocalStorage>,
    /// Advanced by each look at a game directory, so an earlier answer arriving late is dropped.
    wows_dir_check: StoredValue<WowsDirCheck>,
    /// Whether the auto-export directory was found missing. Read through `faults`.
    auto_export_missing: RwSignal<bool, LocalStorage>,
}

impl SettingsDrawerControl {
    pub fn new() -> Self {
        Self {
            open: RwSignal::new_local(false),
            jump: RwSignal::new_local(None),
            wows_dir_invalid: RwSignal::new_local(false),
            auto_export_missing: RwSignal::new_local(false),
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

/// The highlight on a row the drawer was opened at, taken off again after `ATTENTION`.
#[derive(Clone, Copy)]
struct Attention {
    pulse: RwSignal<Option<Pulse>, LocalStorage>,
    /// Takes the highlight off. Replaced, and so cancelled, by a later landing.
    fading: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    timers: StoredValue<Option<Timers>, LocalStorage>,
}

impl Attention {
    /// Called in a component's body, where the window's clock is found.
    fn new() -> Self {
        Self {
            pulse: RwSignal::new_local(None),
            fading: StoredValue::new_local(None),
            timers: StoredValue::new_local(Timers::current()),
        }
    }

    /// Highlights the row afresh, for `ATTENTION` from now.
    fn light(self) {
        let Some(timers) = self.timers.get_value() else { return };

        self.pulse.set(Some(self.pulse.get_untracked().map_or(Pulse::Even, Pulse::next)));
        let pulse = self.pulse;
        self.fading.set_value(Some(timers.set_timeout(ATTENTION, move || {
            pulse.try_set(None);
        })));
    }

    /// The highlight's mark on the row, while it has one.
    fn mark(self) -> Option<String> {
        self.pulse.get().map(|pulse| pulse.mark().to_owned())
    }
}

/// Which of the two names of the highlight's animation a row runs. Flipped by each landing: a
/// changed name starts the animation over, where the same name carries on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pulse {
    Even,
    Odd,
}

impl Pulse {
    fn next(self) -> Self {
        match self {
            Self::Even => Self::Odd,
            Self::Odd => Self::Even,
        }
    }

    fn mark(self) -> &'static str {
        match self {
            Self::Even => "even",
            Self::Odd => "odd",
        }
    }
}

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

/// `name`, saying a setting under it needs attention when one is `faulted`.
fn fault_label(name: String, faulted: bool) -> String {
    if faulted { t!("ui.settings.panel.in_error", name = name).into_owned() } else { name }
}

/// The gear at the end of the tab strip, which opens and shuts the drawer.
#[component]
pub fn SettingsButton(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let control = shell.settings_drawer;
    let faulted = Memo::new(move |_| any_faulted(shell));
    let tip = move || {
        locale.track();
        fault_label(t!("ui.tabs.settings").into_owned(), faulted.get())
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    class = "wt-settings-open",
                    attr:data-testid = "settings-open",
                    attr:data-open = move || control.is_open().then(String::new),
                    attr:data-error = move || faulted.get().then(String::new),
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

/// Which page of the drawer is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawerPage {
    Settings,
    Licenses,
}

/// The page the drawer shows. A new drawer opens on the settings, and a jump to a place in the
/// settings brings them back.
#[component]
fn DrawerContent(shell: Shell) -> impl IntoView {
    let page = RwSignal::new_local(DrawerPage::Settings);
    let jump = shell.settings_drawer.jump;
    let returning = RenderEffect::new(move |_| {
        if jump.with(Option::is_some) && page.get_untracked() != DrawerPage::Settings {
            page.set(DrawerPage::Settings);
        }
    });
    on_cleanup_local(move || drop(returning));

    view! {
        {move || match page.get() {
            DrawerPage::Settings => AnyView::new(view! { SettingsPage(shell = shell, page = page) }),
            DrawerPage::Licenses => AnyView::new(view! { LicensesPage(shell = shell, page = page) }),
        }}
    }
}

/// The header, the search, the list for what is typed in it, and the way to the licenses page.
#[component]
fn SettingsPage(shell: Shell, page: RwSignal<DrawerPage, LocalStorage>) -> impl IntoView {
    let services = Services::current();
    let locale = services.locale;
    let control = shell.settings_drawer;
    let close_label = locale.text("ui.settings.panel.close");
    let clear_label = locale.text("ui.buttons.clear");

    let typed = RwSignal::new_local(String::new());
    let query = Signal::derive_local(move || FilterQuery::new(&typed.get()));
    let escape = EscapeClears::new(typed);
    let search = escape.node_ref();

    // The search is ready for typing as the drawer opens.
    let focusing = RenderEffect::new(move |focused: Option<bool>| {
        if focused == Some(true) || search.get().is_none() {
            return false;
        }
        search.focus();
        true
    });
    on_cleanup_local(move || drop(focusing));

    // A setting the drawer is asked to open at is brought out from under a search that hides it,
    // and its row then takes the jump.
    let revealing = RenderEffect::new(move |_| {
        let Some(SettingsTarget::Setting(setting)) = control.jump.get() else { return };
        if !query.with_untracked(|query| setting.matches(query)) {
            typed.set(String::new());
        }
    });
    on_cleanup_local(move || drop(revealing));

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
                        size = ButtonSize::IconXs,
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
                // Made again in the new language: a placeholder is read as the input is made.
                {move || {
                    locale.track();
                    view! {
                        InputGroupInput(
                            value = typed,
                            placeholder = t!("ui.settings.panel.search_placeholder").into_owned(),
                            node_ref = search,
                            a11y:label = t!("ui.settings.panel.search_placeholder").into_owned(),
                            attr:data-testid = "settings-search",
                            on:key_down = move |ev| {
                                escape.key_down(ev);
                            }
                        )
                    }
                }}
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
        row(class = "wt-settings__foot") {
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::Sm,
                attr:data-testid = "settings-licenses-open",
                on:click = move |_| page.set(DrawerPage::Licenses)
            ) {
                AppIcon(glyph = Glyph::FileText)
                text {{locale.text("ui.settings.licenses.title")}}
            }
        }
    }
}

/// What the parts of the list share: the search, the rows it kept, the headings drawn, and the
/// scrolling element.
#[derive(Clone, Copy)]
struct ListContext {
    query: Signal<FilterQuery, LocalStorage>,
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
    let list = ListContext { query, rows, headings, viewport };
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
    let faulted = Memo::new(move |_| section_faulted(shell, section));
    let name = move || {
        locale.track();
        fault_label(section.name(), faulted.get())
    };
    let listed = move || list.rows.get().iter().any(|setting| setting.section() == section);
    let current = move || at_top.get() == Some(section);

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconXs,
                    disabled = Signal::derive_local(move || !listed()),
                    attr:data-testid = category_test_id(section),
                    attr:data-selected = move || current().then(String::new),
                    attr:data-error = move || faulted.get().then(String::new),
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
    // before the cache is measured), lands on the heading. One the search hides is not taken:
    // the search is cleared, and the setting's row takes it.
    let wanted = move |target: SettingsTarget| match target {
        SettingsTarget::Section(wanted) => wanted == section,
        SettingsTarget::Setting(setting) => {
            setting.section() == section
                && !list.rows.with_untracked(|rows| rows.contains(&setting))
                && list.query.with_untracked(|query| setting.matches(query))
        }
    };
    follow_jump(shell, node, wanted, || {});

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
/// changes it, and the lines about it. A setting with a fault is marked as a whole, and one the
/// drawer was opened at is highlighted for a moment.
#[component]
fn SettingRow(shell: Shell, setting: Setting, query: Signal<FilterQuery, LocalStorage>) -> impl IntoView {
    let locale = Services::current().locale;
    let node = NodeRef::new();
    let attention = Attention::new();
    follow_jump(shell, node, move |target| target == SettingsTarget::Setting(setting), move || attention.light());
    let faults = Memo::new(move |_| setting_faults(shell, setting));
    let faulted = Memo::new(move |_| faults.with(|faults| !faults.is_empty()));

    view! {
        column(
            class = "wt-settings__row",
            node_ref = node,
            attr:data-testid = row_test_id(setting),
            attr:data-error = move || faulted.get().then(String::new),
            attr:data-attention = move || attention.mark()
        ) {
            row(class = "wt-settings__row-body", attr:data-stacked = setting.stacked().then(String::new)) {
                column(class = "wt-settings__row-text") {
                    row(class = "wt-settings__row-heading") {
                        if move || faulted.get() {
                            AppIcon(glyph = Glyph::CircleX, class = "wt-settings__row-fault", attr:data-testid = "settings-row-fault")
                        } else {}
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
                    }
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
                // Made again in the new language: the controls' words are read as they are made.
                // The language list names each language in its own words, and is the control the
                // change comes from, so it is kept.
                {move || {
                    if setting != Setting::Language {
                        locale.track();
                    }
                    view! { SettingControl(shell = shell, setting = setting) }
                }}
            }
            SettingNote(faults = faults, shell = shell, setting = setting)
        }
    }
}

/// Scrolls `node` to the top of the list when the drawer is asked to jump to a place `wanted`
/// says is this one, once the node is laid out, and then runs `landed`.
fn follow_jump(
    shell: Shell,
    node: NodeRef,
    wanted: impl Fn(SettingsTarget) -> bool + 'static,
    landed: impl Fn() + 'static,
) {
    let jump = shell.settings_drawer.jump;
    let jumping = RenderEffect::new(move |_| {
        let Some(target) = jump.get() else { return };
        if node.get().is_none() || !wanted(target) {
            return;
        }

        node.scroll_to(ScrollTarget::IntoViewStart, ScrollBehavior::Instant);
        jump.set(None);
        landed();
    });
    on_cleanup_local(move || drop(jumping));
}

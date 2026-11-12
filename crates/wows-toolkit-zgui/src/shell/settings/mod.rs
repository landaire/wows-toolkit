//! The settings drawer: every setting in one panel that slides over the right of the page, filtered
//! by a search over each setting's name and description, and grouped into sections the strip under
//! the search jumps to.

mod setting;

use landaire_ui::DrawerProps;
use landaire_ui::PanelWidth;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::services::SettingsState;
use crate::shell::Shell;

pub use crate::shell::settings::setting::Setting;

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
    .wt-settings__status {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 0 16px;
    }
    .wt-settings__failed { font-weight: 700; }
    .wt-settings__reason { color: var(--lui-color-text-dim); }
    .wt-settings-open[data-invalid] .wt-icon { color: var(--zui-color-destructive); }
    .wt-settings-open[data-open] { background-color: var(--lui-color-selection); }"
);

/// How wide the drawer is.
const DRAWER_WIDTH: PanelWidth = PanelWidth(440.0);

/// Whether the drawer is open, and the setting it was opened at.
#[derive(Clone, Copy)]
pub struct SettingsDrawerControl {
    open: RwSignal<bool, LocalStorage>,
    /// The setting to bring into view and mark as the drawer opens, as a message about it asks.
    focus: RwSignal<Option<Setting>, LocalStorage>,
    /// Whether the game directory in the field is one an install could be in. The field says so,
    /// and the gear that opens the drawer carries the mark.
    pub wows_dir_invalid: RwSignal<bool, LocalStorage>,
}

impl SettingsDrawerControl {
    pub fn new() -> Self {
        Self {
            open: RwSignal::new_local(false),
            focus: RwSignal::new_local(None),
            wows_dir_invalid: RwSignal::new_local(false),
        }
    }

    pub fn is_open(self) -> bool {
        self.open.get()
    }

    pub fn open(self) {
        self.open.set(true);
    }

    /// Opens the drawer with `setting` in view.
    pub fn open_at(self, setting: Setting) {
        self.focus.set(Some(setting));
        self.open.set(true);
    }

    pub fn close(self) {
        if self.open.get_untracked() {
            self.open.set(false);
        }
    }

    pub fn toggle(self) {
        self.open.update(|open| *open = !*open);
    }

    /// Takes the setting the drawer was opened at, if a message asked for one.
    pub fn take_focus(self) -> Option<Setting> {
        let focus = self.focus.get_untracked();
        if focus.is_some() {
            self.focus.set(None);
        }

        focus
    }

    /// The signal the drawer's own open state is bound to.
    pub fn open_signal(self) -> RwSignal<bool, LocalStorage> {
        self.open
    }
}

impl Default for SettingsDrawerControl {
    fn default() -> Self {
        Self::new()
    }
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
                    AppIcon(glyph = wows_toolkit_icons::Glyph::Settings)
                }
            }
            TooltipContent {{tip}}
        }
    }
}

/// The drawer over the page.
#[component]
pub fn SettingsDrawer(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let locale = services.locale;
    let control = shell.settings_drawer;
    let close_label = locale.text("ui.settings.panel.close");

    view! {
        Drawer(
            open = control.open,
            label = rust_i18n::t!("ui.tabs.settings").into_owned(),
            width = DRAWER_WIDTH,
            attr:data-testid = "settings-drawer"
        ) {
            row(class = "wt-settings__head") {
                AppIcon(glyph = wows_toolkit_icons::Glyph::Settings)
                text(class = "wt-settings__title") {{locale.text("ui.tabs.settings")}}
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    attr:data-testid = "settings-close",
                    a11y:label = close_label,
                    on:click = move |_| control.close()
                ) {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::X)
                }
            }
            {move || services.settings.with_state(|state| match state {
                SettingsState::Loading => AnyView::new(view! {
                    column(class = "wt-settings__status") {
                        text {{locale.text("ui.settings.loading")}}
                    }
                }),
                SettingsState::Failed(reason) => {
                    let reason = reason.clone();
                    AnyView::new(view! {
                        column(class = "wt-settings__status") {
                            text(class = "wt-settings__failed") {{locale.text("ui.settings.load_failed")}}
                            text(class = "wt-settings__reason") {{reason}}
                        }
                    })
                }
                SettingsState::Loaded(_) => AnyView::new(view! { column() }),
            })}
        }
    }
}

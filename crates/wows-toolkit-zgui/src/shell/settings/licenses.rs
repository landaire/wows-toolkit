//! The drawer's page that names this build, shows its license, and lists the notices of the
//! third-party components it contains, filtered by a search over each notice's title and text.

use landaire_ui::EscapeClears;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use wows_toolkit_icons::Glyph;
use wows_toolkit_notices::Notice;
use wows_toolkit_notices::Notices;
use wows_toolkit_notices::PROJECT_LICENSE;
use wows_toolkit_notices::PROJECT_LICENSE_ID;
use wt_translations::t;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CollapsibleContentProps;
use zgui_ui::CollapsibleProps;
use zgui_ui::CollapsibleTriggerProps;
use zgui_ui::InputGroupAddonAlign;
use zgui_ui::InputGroupAddonProps;
use zgui_ui::InputGroupButtonProps;
use zgui_ui::InputGroupButtonSize;
use zgui_ui::InputGroupInputProps;
use zgui_ui::InputGroupProps;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::settings::DrawerPage;

const SHEET: &str = "wt-settings-licenses";

const CSS: &str = css!(
    ".wt-licenses__scroll {
        flex: 1 1 auto;
        min-height: 0;
        overflow-y: auto;
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-licenses__list {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 12px 4px 16px 16px;
    }
    .wt-licenses__about {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .wt-licenses__app {
        font-size: 13px;
        font-weight: 600;
        color: var(--lui-color-text-bright);
    }
    .wt-licenses__line {
        font-size: 12px;
        color: var(--lui-color-text-dim);
    }
    .wt-licenses__heading {
        padding-top: 12px;
        font-size: 11px;
        font-weight: 600;
        color: var(--lui-color-text-dim);
    }
    .wt-licenses__text {
        padding: 8px;
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-widget);
        font-family: var(--lui-font-family-mono);
        font-size: 11px;
        white-space: pre-wrap;
        user-select: text;
    }
    .wt-licenses__card {
        display: flex;
        flex-direction: column;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
        background-color: var(--lui-color-card);
        overflow: hidden;
    }
    .wt-licenses__entry {
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-licenses__entry:first-child { border-top-width: 0; }
    /* Compound, so it outranks the collapsible's own trigger padding whichever sheet loads last. */
    .zui-collapsible__trigger.wt-licenses__entry-trigger { padding: 8px 12px; }
    .wt-licenses__entry-title {
        flex: 1 1 auto;
        font-size: 12px;
        color: var(--lui-color-text-bright);
    }
    .wt-licenses__entry-body { margin: 0 12px 12px 12px; }
    .wt-licenses__no-match {
        padding: 8px 0;
        font-size: 13px;
        color: var(--lui-color-text-dim);
    }"
);

/// One entry of the embedded notices, compared by identity.
#[derive(Clone, Copy)]
struct Entry(&'static Notice);

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}

impl Eq for Entry {}

/// The header with the way back to the settings, the search, and the page for what is typed in it.
#[component]
pub(super) fn LicensesPage(shell: Shell, page: RwSignal<DrawerPage, LocalStorage>) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let control = shell.settings_drawer;
    let back_label = locale.text("ui-settings-licenses-back");
    let close_label = locale.text("ui-settings-panel-close");
    let clear_label = locale.text("ui-buttons-clear");

    let typed = RwSignal::new_local(String::new());
    let escape = EscapeClears::new(typed);
    let search = escape.node_ref();

    // The search is ready for typing as the page opens, as the settings search is.
    let focusing = RenderEffect::new(move |focused: Option<bool>| {
        if focused == Some(true) || search.get().is_none() {
            return false;
        }
        search.focus();
        true
    });
    on_cleanup_local(move || drop(focusing));

    let shown =
        Memo::new(move |_| typed.with(|typed| Notices::embedded().matching(typed).map(Entry).collect::<Vec<_>>()));

    let version = move || {
        locale.track();
        t!("ui-labels-app_version", version = env!("CARGO_PKG_VERSION")).into_owned()
    };
    let project_license = move || {
        locale.track();
        t!("ui-labels-project_license", license = PROJECT_LICENSE_ID).into_owned()
    };

    view! {
        row(class = "wt-settings__head") {
            AppIcon(glyph = Glyph::FileText)
            text(class = "wt-settings__title") {{locale.text("ui-settings-licenses-title")}}
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconXs,
                        attr:data-testid = "settings-licenses-back",
                        a11y:label = back_label,
                        on:click = move |_| page.set(DrawerPage::Settings)
                    ) {
                        AppIcon(glyph = Glyph::Settings)
                    }
                }
                TooltipContent {{back_label}}
            }
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
                            placeholder = t!("ui-labels-notices_search_hint").into_owned(),
                            node_ref = search,
                            a11y:label = t!("ui-labels-notices_search_hint").into_owned(),
                            attr:data-testid = "licenses-search",
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
                            attr:data-testid = "licenses-search-clear",
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
        scroll(class = "wt-licenses__scroll", attr:data-testid = "licenses-scroll") {
            column(class = "wt-licenses__list") {
                // A search shows its matches first, rather than below the project's license.
                if move || typed.with(String::is_empty) {
                    column(class = "wt-licenses__about") {
                        text(class = "wt-licenses__app") {{wows_toolkit_config::APP_NAME}}
                        text(class = "wt-licenses__line wt-licenses__version", attr:data-testid = "licenses-version") {{version}}
                        text(class = "wt-licenses__line") {{project_license}}
                        text(class = "wt-licenses__text wt-licenses__project", attr:data-testid = "licenses-project") {{PROJECT_LICENSE}}
                    }
                } else {}
                text(class = "wt-licenses__heading") {{locale.text("ui-labels-third_party_notices")}}
                text(class = "wt-licenses__line") {{locale.text("ui-settings-licenses-third_party_intro")}}
                if move || shown.with(Vec::is_empty) {
                    text(class = "wt-licenses__no-match", attr:data-testid = "licenses-no-match") {
                        {locale.text("ui-labels-no_matching_notices")}
                    }
                } else {
                    column(class = "wt-licenses__card") {
                        for entry in move || shown.get(), key = |entry: &Entry| entry.0.title() {
                            NoticeEntry(entry = entry)
                        }
                    }
                }
            }
        }
    }
}

/// One notice: its title, which opens and closes the components that use it and its full text.
#[component]
fn NoticeEntry(entry: Entry) -> impl IntoView {
    let open = RwSignal::new_local(false);

    // Only an opened body is laid out: all the bodies together are about a megabyte of text.
    view! {
        Collapsible(open = open, class = "wt-licenses__entry") {
            CollapsibleTrigger(class = "wt-licenses__entry-trigger", attr:data-testid = "licenses-entry") {
                text(class = "wt-licenses__entry-title") {{entry.0.title()}}
            }
            CollapsibleContent {
                if move || open.get() {
                    text(class = "wt-licenses__text wt-licenses__entry-body", attr:data-testid = "licenses-entry-body") {{entry.0.body()}}
                } else {}
            }
        }
    }
}

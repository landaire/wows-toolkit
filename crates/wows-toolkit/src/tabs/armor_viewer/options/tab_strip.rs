//! The strip of tabs on the options panel's outer edge, each named in text set on its side.
//!
//! While a search is typed each tab carries how many tools and sections it kept, and a tab that
//! kept none is dimmed and cannot be chosen.

use wows_toolkit_viewmodel::armor::options_panel::OptionsTab;
use wows_toolkit_viewmodel::armor::options_panel::TabMatches;
use wt_translations::t;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui_ui::TabsListProps;
use zgui_ui::TabsListVariant;
use zgui_ui::TabsProps;
use zgui_ui::TabsTriggerProps;
use zgui_ui::prelude::Binding;
use zgui_ui_primitives::Orientation;

use crate::i18n::Locale;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;

const SHEET: &str = "wt-armor-tabs";

// The tab rules repeat their classes to outrank the tab set's own vertical rules.
const CSS: &str = css!(
    ".wt-armor-tabs {
        flex: none;
        align-self: stretch;
        background-color: var(--lui-color-panel);
        border-left: 1px solid var(--lui-color-border);
    }
    .wt-armor-tabs .wt-armor-tabs__list.wt-armor-tabs__list {
        width: 28px;
        padding: 6px 0;
        gap: 4px;
        align-items: stretch;
        justify-content: flex-start;
    }
    .wt-armor-tabs .wt-armor-tab.wt-armor-tab.wt-armor-tab {
        flex: none;
        flex-direction: column;
        align-items: center;
        justify-content: flex-start;
        gap: 4px;
        width: 28px;
        height: auto;
        padding: 8px 0;
        margin-left: -1px;
        border: 1px solid transparent;
        border-left: none;
        border-radius: 0 4px 4px 0;
        color: var(--lui-color-text-dim);
    }
    .wt-armor-tabs .wt-armor-tab.wt-armor-tab.wt-armor-tab:hover { color: var(--lui-color-text); }
    .wt-armor-tabs .wt-armor-tab.wt-armor-tab.wt-armor-tab[data-state=\"active\"] {
        background-color: var(--lui-color-surface);
        border-color: var(--lui-color-border);
        color: var(--lui-color-text-bright);
    }
    .wt-armor-tabs .wt-armor-tab.wt-armor-tab.wt-armor-tab[data-matched] { color: var(--lui-color-accent); }
    .wt-armor-tabs .wt-armor-tab.wt-armor-tab.wt-armor-tab::after { display: none; }
    .wt-armor-tab__label { position: relative; flex: none; width: 16px; height: 40px; }
    .wt-armor-tab__text {
        position: absolute;
        top: 0;
        left: 16px;
        width: max-content;
        white-space: nowrap;
        font-size: 12px;
        line-height: 16px;
        transform-origin: 0 0;
        transform: rotate(90deg);
    }
    .wt-armor-tab__count {
        min-width: 16px;
        padding: 0 3px;
        box-sizing: border-box;
        border-radius: 8px;
        font-size: 10px;
        font-weight: 700;
        line-height: 14px;
        text-align: center;
        background-color: var(--lui-color-accent);
        color: var(--lui-color-accent-foreground);
    }"
);

/// The tab strip. `matches` is `None` while nothing is searched for.
#[component]
pub fn OptionTabStrip(workspace: ArmorWorkspace, matches: Signal<Option<TabMatches>, LocalStorage>) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = workspace.services.locale;
    let shown = Signal::derive_local(move || workspace.options_tab.get().shown(matches.get().as_ref()).id().to_owned());
    let value = Binding::controlled(shown, move |id: String| {
        if let Some(tab) = OptionsTab::from_id(&id) {
            workspace.select_options_tab(tab);
        }
    });
    let triggers: Vec<AnyView> = OptionsTab::ALL.into_iter().map(|tab| tab_trigger(locale, tab, matches)).collect();

    view! {
        Tabs(
            value = value,
            orientation = Orientation::Vertical,
            label = t!("ui-armor-option_tabs-label").into_owned(),
            class = "wt-armor-tabs",
            attr:data-testid = "armor-options-tabs"
        ) {
            TabsList(variant = TabsListVariant::Line, class = "wt-armor-tabs__list") {{triggers}}
        }
    }
}

/// The name a tab is shown under.
pub fn tab_name(tab: OptionsTab) -> String {
    match tab {
        OptionsTab::Ship => t!("ui-armor-option_tabs-ship"),
        OptionsTab::Layers => t!("ui-armor-option_tabs-layers"),
        OptionsTab::Analysis => t!("ui-armor-option_tabs-analysis"),
        OptionsTab::Display => t!("ui-armor-display"),
    }
    .into_owned()
}

/// One tab: the count a search kept in it over its name.
fn tab_trigger(locale: Locale, tab: OptionsTab, matches: Signal<Option<TabMatches>, LocalStorage>) -> AnyView {
    let id = tab.id();
    let count = Signal::derive_local(move || matches.get().map(|matches| matches.count(tab)));
    let kept = move || count.get().filter(|count| *count > 0);
    let name = move || {
        locale.track();
        tab_name(tab)
    };
    let label = move || {
        locale.track();
        match kept() {
            Some(count) => t!("ui-armor-option_tabs-tab_matches", tab = tab_name(tab), count = count).into_owned(),
            None => tab_name(tab),
        }
    };
    let badge = move || {
        kept().map(|count| {
            view! {
                text(class = "wt-armor-tab__count", attr:data-testid = format!("armor-options-tab-count-{id}")) {{
                    count.to_string()
                }}
            }
        })
    };

    AnyView::new(view! {
        TabsTrigger(
            value = id,
            disabled = Signal::derive_local(move || count.get() == Some(0)),
            class = "wt-armor-tab",
            attr:data-testid = format!("armor-options-tab-{id}"),
            attr:data-matched = move || kept().map(|_| String::new()),
            a11y:label = label
        ) {
            {badge}
            {vertical_label(name)}
        }
    })
}

/// `text` turned to read downward. A turned element keeps the room it took before it was turned,
/// so the box it sits in is given the turned text's measured height, which is its length.
fn vertical_label(text: impl Fn() -> String + 'static) -> AnyView {
    let node = NodeRef::new();
    let size = node.observe_border_size();
    let length = move || {
        let scale = node.scale();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        size.get().map(|size| format!("{}px", (size.height.0 / scale).ceil()))
    };

    AnyView::new(view! {
        box(class = "wt-armor-tab__label", style:height = length) {
            box(class = "wt-armor-tab__text", node_ref = node) {{text}}
        }
    })
}

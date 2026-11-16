//! The control that chooses which battles the session holds, shown in the replay listing's header
//! and the Stats tab's filter bar.

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::stats::session::AppOpened;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use zgui::component;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::ToggleGroupItemProps;
use zgui_ui::ToggleGroupProps;
use zgui_ui::ToggleSize;
use zgui_ui::ToggleVariant;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::services::Session;

const SHEET: &str = "wt-session-scope";

const CSS: &str = css!(
    ".wt-session-scope {
        display: flex;
        flex-direction: column;
        gap: 6px;
        width: 280px;
        padding: 4px;
    }
    .wt-session-scope__title { font-size: 14px; font-weight: 700; }
    .wt-session-scope__hint { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-session-scope__chip { flex: 0 1 auto; min-width: 0; max-width: 100%; }
    .wt-session-scope__chip-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }"
);

/// The local time the app opened, as a session since then names it.
pub fn opened_label(opened: AppOpened) -> String {
    opened.at().strftime("%H:%M").to_string()
}

/// "5 battles".
pub fn battles_label(count: usize) -> String {
    if count == 1 {
        t!("ui.stats.session_battles_one").into_owned()
    } else {
        t!("ui.stats.session_battles_many", count = count).into_owned()
    }
}

/// The session's state in one line: its scope and how many battles it holds.
pub fn session_state(session: Session) -> String {
    let battles = battles_label(session.games().with(|games| games.len()));
    match session.scope() {
        SessionScope::Manual => t!("ui.stats.session_chip_manual", battles = battles).into_owned(),
        SessionScope::SinceAppOpened => {
            t!("ui.stats.session_chip_since", time = opened_label(session.opened()), battles = battles).into_owned()
        }
    }
}

/// What the chosen scope holds.
pub fn scope_hint(session: Session) -> String {
    match session.scope() {
        SessionScope::Manual => t!("ui.stats.session_manual_tooltip").into_owned(),
        SessionScope::SinceAppOpened => {
            t!("ui.stats.session_since_opened_tooltip", time = opened_label(session.opened())).into_owned()
        }
    }
}

/// The value a scope's toggle carries.
const fn scope_value(scope: SessionScope) -> &'static str {
    match scope {
        SessionScope::Manual => "manual",
        SessionScope::SinceAppOpened => "since-opened",
    }
}

/// Manual or since the app opened, as two exclusive toggles. `test_id` names the group; each
/// toggle is `{test_id}-manual` and `{test_id}-since-opened`.
#[component]
pub fn SessionScopeChoice(test_id: &'static str) -> impl IntoView {
    let services = Services::current();
    let session = services.session;
    let locale = services.locale;
    let chosen = Signal::derive_local(move || vec![scope_value(session.scope()).to_owned()]);
    let choose = move |values: Vec<String>| {
        // A press on the scope already chosen would leave none; the choice stays.
        let Some(scope) =
            values.first().and_then(|value| SessionScope::ALL.into_iter().find(|scope| scope_value(*scope) == value))
        else {
            return;
        };
        if scope != session.scope_untracked() {
            session.choose_scope(scope);
        }
    };

    view! {
        ToggleGroup(
            value = Binding::controlled(chosen, choose),
            variant = ToggleVariant::Outline,
            size = ToggleSize::Sm,
            label = t!("ui.stats.session_title").into_owned(),
            attr:data-testid = test_id
        ) {
            ToggleGroupItem(value = scope_value(SessionScope::Manual), attr:data-testid = format!("{test_id}-manual")) {
                {locale.text(SessionScope::Manual.translation_key())}
            }
            ToggleGroupItem(value = scope_value(SessionScope::SinceAppOpened), attr:data-testid = format!("{test_id}-since-opened")) {
                {locale.text(SessionScope::SinceAppOpened.translation_key())}
            }
        }
    }
}

/// The session's state as a button in the listing's header, opening the scope choice.
#[component]
pub fn SessionChip() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let session = services.session;
    let locale = services.locale;
    let open = RwSignal::new_local(false);
    let state = Signal::derive_local(move || {
        locale.track();
        session_state(session)
    });
    let hint = Signal::derive_local(move || {
        locale.track();
        scope_hint(session)
    });

    view! {
        Tooltip {
            TooltipTrigger {
                Popover(open = open) {
                    PopoverTrigger(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        class = "wt-session-scope__chip",
                        attr:data-testid = "replay-listing-session",
                        a11y:label = move || state.get()
                    ) {
                        AppIcon(glyph = Glyph::ChartColumn)
                        text(class = "wt-session-scope__chip-text") {{move || state.get()}}
                    }
                    PopoverContent(
                        placement = Placement::new(Side::Bottom, Align::Start),
                        class = "wt-session-scope",
                        attr:data-testid = "replay-listing-session-menu"
                    ) {
                        text(class = "wt-session-scope__title") {{locale.text("ui.stats.session_title")}}
                        SessionScopeChoice(test_id = "replay-listing-session-scope")
                        text(class = "wt-session-scope__hint") {{move || hint.get()}}
                    }
                }
            }
            TooltipContent {{locale.text("ui.stats.session_chip_tooltip")}}
        }
    }
}

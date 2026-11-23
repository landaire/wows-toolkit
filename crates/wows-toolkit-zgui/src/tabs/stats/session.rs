//! The control that chooses which battles the session holds, shown in the replay listing's options
//! and the Stats tab's filter bar.

use wows_toolkit_viewmodel::stats::session::AppOpened;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use wt_translations::t;
use zgui::component;
use zgui::reactive::Signal;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::IntoView;
use zgui_ui::ToggleGroupItemProps;
use zgui_ui::ToggleGroupProps;
use zgui_ui::ToggleSize;
use zgui_ui::ToggleVariant;
use zgui_ui_primitives::Binding;

use crate::services::Services;
use crate::services::Session;

/// The local time the app opened, as a session since then names it.
pub fn opened_label(opened: AppOpened) -> String {
    opened.at().strftime("%H:%M").to_string()
}

/// "5 battles".
pub fn battles_label(count: usize) -> String {
    if count == 1 {
        t!("ui-stats-session_battles_one").into_owned()
    } else {
        t!("ui-stats-session_battles_many", count = count).into_owned()
    }
}

/// The session's state in one line: its scope and how many battles it holds.
pub fn session_state(session: Session) -> String {
    let battles = battles_label(session.games().with(|games| games.len()));
    match session.scope() {
        SessionScope::Manual => t!("ui-stats-session_chip_manual", battles = battles).into_owned(),
        SessionScope::SinceAppOpened => {
            t!("ui-stats-session_chip_since", time = opened_label(session.opened()), battles = battles).into_owned()
        }
    }
}

/// What the chosen scope holds.
pub fn scope_hint(session: Session) -> String {
    match session.scope() {
        SessionScope::Manual => t!("ui-stats-session_manual_tooltip").into_owned(),
        SessionScope::SinceAppOpened => {
            t!("ui-stats-session_since_opened_tooltip", time = opened_label(session.opened())).into_owned()
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
            label = t!("ui-stats-session_title").into_owned(),
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

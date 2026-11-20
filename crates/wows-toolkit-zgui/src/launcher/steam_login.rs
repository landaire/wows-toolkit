//! The per-install Wargaming login switch of a Steam install: off, Play hands the install to
//! Steam; on, Play signs in through the toolkit with WG's Game Center files laid over it.

use std::path::PathBuf;

use rust_i18n::t;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::LabelProps;
use zgui_ui::SpinnerProps;
use zgui_ui::SwitchProps;
use zgui_ui::SwitchSize;
use zgui_ui::prelude::Binding;

use crate::desktop;
use crate::launcher::display::WgcLoginStatus;
use crate::launcher::display::WgcLoginView;
use crate::launcher::model::LauncherModel;
use crate::services::Services;

/// WG's article on signing in without a bound email, which also covers adding one.
const STEAM_ACCOUNT_HELP: &str = "https://wargaming.net/support/en/products/wows/article/36251/";

/// The switch for the install at `path`. Nothing shows for an install that does not offer it.
/// `brief` leaves out the explanation, for the popover.
#[component]
pub fn WgcLoginControl(model: LauncherModel, path: PathBuf, test_id: String, brief: bool) -> impl IntoView {
    let locale = Services::current().locale;
    let shown_path = path.clone();
    let login = Memo::new(move |_| {
        model.with(|state| {
            let settings = state.settings.as_ref()?;
            WgcLoginView::of(&state.launcher, settings, &shown_path)
        })
    });
    let path = StoredValue::new(path);
    let on = Signal::derive_local(move || login.get().is_some_and(|login| login.on));
    // An install that has the login on can always turn it off.
    let locked = Signal::derive_local(move || login.get().is_none_or(|login| !login.on && !login.status.allows_on()));
    let switch_id = format!("{test_id}-switch");
    let status_id = format!("{test_id}-status");
    let help_id = StoredValue::new(format!("{test_id}-help"));
    let status_line = move || login.get().map(|login| status(login.status, status_id.clone()));
    let control = NodeRef::new();
    let name = NodeRef::new();

    view! {
        Show(when = move || login.get().is_some()) {
            column(class = "wt-launcher__column", attr:data-testid = test_id.clone()) {
                row(class = "wt-launcher__row wt-launcher__row--tight") {
                    Switch(
                        checked = Binding::controlled(on, move |wanted: bool| {
                            let path = path.get_value();
                            model.op(move |launcher, settings, env| launcher.set_wgc_login(&path, wanted, settings, env));
                        }),
                        disabled = locked,
                        size = SwitchSize::Sm,
                        node_ref = control,
                        labelled_by = name,
                        attr:data-testid = switch_id.clone()
                    )
                    Label(node_ref = name, control = control, class = "wt-launcher__text") {
                        {locale.text("ui.launcher.wgc_login.label")}
                    }
                }
                {status_line.clone()}
                if move || !brief {
                    column(class = "wt-launcher__column") {
                        text(class = "wt-launcher__dim") {{locale.text("ui.launcher.wgc_login.about")}}
                        text(class = "wt-launcher__dim") {{locale.text("ui.launcher.wgc_login.steam_accounts")}}
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::Xs,
                            attr:data-testid = help_id.get_value(),
                            on:click = move |_| desktop::open_url(STEAM_ACCOUNT_HELP)
                        ) {
                            {locale.text("ui.launcher.wgc_login.steam_accounts_link")}
                        }
                    }
                } else {}
            }
        }
    }
}

/// The line under the switch saying where the Game Center files stand.
fn status(status: WgcLoginStatus, test_id: String) -> AnyView {
    let words = match &status {
        WgcLoginStatus::Ready => t!("ui.launcher.wgc_login.ready"),
        WgcLoginStatus::Fetchable { size } => {
            t!("ui.launcher.wgc_login.fetchable", size = humansize::format_size(*size, humansize::DECIMAL))
        }
        WgcLoginStatus::Downloading { percent: Some(percent) } => {
            t!("ui.launcher.wgc_login.downloading", percent = percent)
        }
        WgcLoginStatus::Downloading { percent: None } => t!("ui.launcher.wgc_login.downloading_unknown"),
        WgcLoginStatus::Checking => t!("ui.launcher.wgc_login.checking"),
        WgcLoginStatus::NeedsSignIn => t!("ui.launcher.wgc_login.needs_sign_in"),
        WgcLoginStatus::Unavailable(error) => t!("ui.launcher.wgc_login.unavailable", error = error.as_str()),
    }
    .into_owned();
    let busy = matches!(status, WgcLoginStatus::Checking | WgcLoginStatus::Downloading { .. });
    // Tests read the line by `wt-launcher-wgc-status`.
    let class = match status {
        WgcLoginStatus::NeedsSignIn | WgcLoginStatus::Unavailable(_) => "wt-launcher__warn wt-launcher-wgc-status",
        _ => "wt-launcher__dim wt-launcher-wgc-status",
    };

    AnyView::new(view! {
        row(class = "wt-launcher__row wt-launcher__row--tight") {
            if move || busy {
                Spinner()
            } else {}
            text(class = class, attr:data-testid = test_id) {{words}}
        }
    })
}

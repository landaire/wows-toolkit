//! The account section: who is signed in, or the sign-in form and its two-factor and CAPTCHA
//! steps.

use std::time::SystemTime;

use rust_i18n::t;
use wgc_auth::AccountRealm;
use wows_toolkit_launcher::REALMS;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::LabelProps;
use zgui_ui::SpinnerProps;
use zgui_ui::ToggleGroupItemProps;
use zgui_ui::ToggleGroupProps;
use zgui_ui::ToggleSize;
use zgui_ui::ToggleVariant;
use zgui_ui::prelude::Binding;

use crate::services::Services;
use crate::tabs::launcher::SectionProps;
use crate::tabs::launcher::display::AccountMode;
use crate::tabs::launcher::display::Expiry;
use crate::tabs::launcher::display::SessionView;
use crate::tabs::launcher::model::LauncherModel;

/// What the sign-in and two-factor forms hold between presses.
#[derive(Clone, Copy)]
struct SignInForm {
    /// Whether the signed-out section shows the form rather than the offer to sign in.
    shown: RwSignal<bool, LocalStorage>,
    realm: RwSignal<AccountRealm, LocalStorage>,
    username: RwSignal<String, LocalStorage>,
    password: RwSignal<String, LocalStorage>,
    code: RwSignal<Checked, LocalStorage>,
    two_factor_code: RwSignal<String, LocalStorage>,
}

impl SignInForm {
    fn new() -> Self {
        Self {
            shown: RwSignal::new_local(false),
            realm: RwSignal::new_local(AccountRealm::Eu),
            username: RwSignal::new_local(String::new()),
            password: RwSignal::new_local(String::new()),
            code: RwSignal::new_local(Checked::No),
            two_factor_code: RwSignal::new_local(String::new()),
        }
    }

    fn sign_in(self, model: LauncherModel) {
        let username = self.username.get_untracked().trim().to_owned();
        // Cleared from the field the moment it is used.
        let password = wgc_auth::SecretPassword::new(self.password.get_untracked());
        self.password.set(String::new());
        let realm = self.realm.get_untracked();

        model.op(move |launcher, settings, env| launcher.start_login(realm, username, password, settings, env));
    }

    fn submit_code(self, model: LauncherModel) {
        let code = self.two_factor_code.get_untracked().trim().to_owned();
        self.two_factor_code.set(String::new());
        let code = match self.code.get_untracked() {
            Checked::Yes => wgc_auth::TwoFactorCode::Backup(code),
            Checked::No | Checked::Mixed => wgc_auth::TwoFactorCode::Otp(code),
        };

        model.op(move |launcher, settings, env| launcher.submit_two_factor(code, settings, env));
    }
}

#[component]
pub fn AccountSection(model: LauncherModel) -> impl IntoView {
    let form = SignInForm::new();
    let mode = Memo::new(move |_| model.with(|state| AccountMode::of(&state.launcher, SystemTime::now())));

    view! {
        Section(heading = "ui.launcher.account.heading") {
            {move || match mode.get() {
                AccountMode::SignedIn(session) => AnyView::new(view! { SignedIn(model = model, session = session) }),
                AccountMode::TwoFactor => AnyView::new(view! { TwoFactor(model = model, form = form) }),
                AccountMode::Captcha => AnyView::new(view! { CaptchaNotice(model = model, form = form) }),
                AccountMode::SignedOut => AnyView::new(view! { SignedOut(model = model, form = form) }),
            }}
        }
    }
}

#[component]
fn SignedIn(model: LauncherModel, session: SessionView) -> impl IntoView {
    let locale = Services::current().locale;
    let SessionView { nickname, region, expiry, persisted } = session;
    let region = move || {
        locale.track();
        t!("ui.launcher.account.region", region = region).into_owned()
    };
    let expiry = match expiry {
        Expiry::In(duration) => AnyView::new(view! {
            text(class = "wt-launcher__dim") {{move || {
                locale.track();
                t!("ui.launcher.account.expires_in", duration = duration.as_str()).into_owned()
            }}}
        }),
        Expiry::Passed => AnyView::new(view! {
            text(class = "wt-launcher__error") {{locale.text("ui.launcher.account.expired")}}
        }),
        Expiry::Unknown => AnyView::new(view! {
            text(class = "wt-launcher__dim") {{locale.text("ui.launcher.account.expiry_unknown")}}
        }),
    };

    view! {
        column(class = "wt-launcher__column", attr:data-testid = "launcher-session") {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__strong") {{nickname}}
                text(class = "wt-launcher__dim") {{region}}
            }
            {expiry}
            if move || !persisted {
                text(class = "wt-launcher__dim") {{locale.text("ui.launcher.account.not_persisted")}}
            }
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-sign-out",
                    on:click = move |_| model.op(|launcher, settings, env| launcher.sign_out(settings, env))
                ) {
                    {locale.text("ui.launcher.account.sign_out")}
                }
            }
        }
    }
}

#[component]
fn TwoFactor(model: LauncherModel, form: SignInForm) -> impl IntoView {
    let locale = Services::current().locale;
    let busy = login_busy(model);
    let backup_box = NodeRef::new();

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide") {
            text {{locale.text("ui.launcher.account.code_required")}}
            row(class = "wt-launcher__row") {
                Input(
                    value = form.two_factor_code,
                    placeholder = locale.text("ui.launcher.account.code")(),
                    label = locale.text("ui.launcher.account.code")(),
                    class = "wt-launcher__input",
                    attr:data-testid = "launcher-code"
                )
                row(class = "wt-launcher__row wt-launcher__row--tight") {
                    Checkbox(checked = form.code, node_ref = backup_box, attr:data-testid = "launcher-backup-code")
                    Label(control = backup_box) {{locale.text("ui.launcher.account.use_backup_code")}}
                }
            }
            LoginError(model = model)
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = busy,
                    attr:data-testid = "launcher-submit-code",
                    on:click = move |_| form.submit_code(model)
                ) {
                    {locale.text("ui.launcher.account.submit_code")}
                }
                if move || busy.get() {
                    Spinner()
                }
            }
        }
    }
}

#[component]
fn CaptchaNotice(model: LauncherModel, form: SignInForm) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide") {
            text(class = "wt-launcher__warn", attr:data-testid = "launcher-captcha") {{locale.text("ui.launcher.account.captcha")}}
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-captcha-retry",
                    on:click = move |_| {
                        model.update_launcher(|launcher| launcher.dismiss_captcha());
                        form.shown.set(true);
                    }
                ) {
                    {locale.text("ui.launcher.retry")}
                }
            }
        }
    }
}

#[component]
fn SignedOut(model: LauncherModel, form: SignInForm) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        if move || form.shown.get() {
            SignInFields(model = model, form = form)
        } else {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__dim") {{locale.text("ui.launcher.account.signed_out")}}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-show-sign-in",
                    on:click = move |_| form.shown.set(true)
                ) {
                    {locale.text("ui.launcher.account.sign_in")}
                }
            }
        }
    }
}

#[component]
fn SignInFields(model: LauncherModel, form: SignInForm) -> impl IntoView {
    let locale = Services::current().locale;
    let busy = login_busy(model);
    let chosen = Signal::derive_local(move || vec![realm_value(form.realm.get())]);
    // A press on the chosen realm would leave none chosen, so only a press on another is taken.
    let choose = move |values: Vec<String>| {
        let picked = values.iter().find_map(|value| REALMS.into_iter().find(|realm| realm_value(*realm) == *value));
        if let Some(realm) = picked {
            form.realm.set(realm);
        }
    };
    let realms: Vec<AnyView> = REALMS
        .into_iter()
        .enumerate()
        .map(|(index, realm)| {
            AnyView::new(view! {
                ToggleGroupItem(value = realm_value(realm), attr:data-testid = format!("launcher-realm-button-{index}")) {
                    {wows_toolkit_launcher::realm_label(realm)}
                }
            })
        })
        .collect();

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide") {
            ToggleGroup(
                value = Binding::controlled(chosen, choose),
                variant = ToggleVariant::Outline,
                size = ToggleSize::Sm,
                spacing = 4.0
            ) {
                {realms}
            }
            row(class = "wt-launcher__row") {
                Input(
                    value = form.username,
                    placeholder = locale.text("ui.launcher.account.username")(),
                    label = locale.text("ui.launcher.account.username")(),
                    class = "wt-launcher__input",
                    attr:data-testid = "launcher-username"
                )
                Input(
                    value = form.password,
                    secure = true,
                    placeholder = locale.text("ui.launcher.account.password")(),
                    label = locale.text("ui.launcher.account.password")(),
                    class = "wt-launcher__input",
                    attr:data-testid = "launcher-password"
                )
            }
            LoginError(model = model)
            row(class = "wt-launcher__row") {
                Button(
                    size = ButtonSize::Sm,
                    disabled = busy,
                    attr:data-testid = "launcher-sign-in",
                    on:click = move |_| form.sign_in(model)
                ) {
                    {locale.text("ui.launcher.account.sign_in")}
                }
                if move || busy.get() {
                    Spinner()
                }
            }
        }
    }
}

/// Why the last sign-in or code was refused.
#[component]
fn LoginError(model: LauncherModel) -> impl IntoView {
    let error = Memo::new(move |_| model.with(|state| state.launcher.login_error().map(str::to_owned)));

    view! {
        {move || error.get().map(|error| view! {
            text(class = "wt-launcher__error", attr:data-testid = "launcher-login-error") {{error}}
        })}
    }
}

/// Whether a sign-in or a code is being checked.
fn login_busy(model: LauncherModel) -> Signal<bool, LocalStorage> {
    let busy = Memo::new(move |_| model.with(|state| state.launcher.login_in_progress()));
    Signal::derive_local(move || busy.get())
}

/// The toggle group's value for `realm`.
fn realm_value(realm: AccountRealm) -> String {
    wows_toolkit_launcher::realm_label(realm).to_owned()
}

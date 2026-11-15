//! Signing an account in: the realm, email and password, then the two-factor code or the CAPTCHA
//! notice. Hosted as the Add account dialog and as a step of the setup dialog.

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wgc_auth::AccountRealm;
use wows_toolkit_launcher::KeyringState;
use wows_toolkit_launcher::LoginTarget;
use wows_toolkit_launcher::REALMS;
use wows_toolkit_launcher::SessionStorage;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::InputProps;
use zgui_ui::LabelProps;
use zgui_ui::SpinnerProps;
use zgui_ui::ToggleGroupItemProps;
use zgui_ui::ToggleGroupProps;
use zgui_ui::ToggleSize;
use zgui_ui::ToggleVariant;
use zgui_ui::prelude::Binding;

use crate::launcher::CSS;
use crate::launcher::SHEET;
use crate::launcher::display::SignInStep;
use crate::launcher::display::realm_badge;
use crate::launcher::model::LauncherModel;
use crate::launcher::realm::RealmGlyphProps;
use crate::services::Services;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;

/// What the sign-in and two-factor forms hold between presses.
#[derive(Clone, Copy)]
struct SignInForm {
    realm: RwSignal<AccountRealm, LocalStorage>,
    email: RwSignal<String, LocalStorage>,
    password: RwSignal<String, LocalStorage>,
    backup_code: RwSignal<Checked, LocalStorage>,
    code: RwSignal<String, LocalStorage>,
}

impl SignInForm {
    fn new(realm: AccountRealm) -> Self {
        Self {
            realm: RwSignal::new_local(realm),
            email: RwSignal::new_local(String::new()),
            password: RwSignal::new_local(String::new()),
            backup_code: RwSignal::new_local(Checked::No),
            code: RwSignal::new_local(String::new()),
        }
    }

    /// Whether the credentials are worth sending.
    fn credentials_ready(self) -> bool {
        wows_toolkit_launcher::is_plausible_email(&self.email.get()) && !self.password.with(String::is_empty)
    }

    fn code_ready(self) -> bool {
        !self.code.with(|code| code.trim().is_empty())
    }

    fn sign_in(self, model: LauncherModel, target: LoginTarget) {
        if !self.credentials_ready() || model.with_untracked(|state| state.launcher.login_in_progress()) {
            return;
        }
        let email = self.email.get_untracked().trim().to_owned();
        // Cleared from the field the moment it is used.
        let password = wgc_auth::SecretPassword::new(self.password.get_untracked());
        self.password.set(String::new());
        let realm = self.realm.get_untracked();

        model.op(move |launcher, settings, env| launcher.start_login(target, realm, email, password, settings, env));
    }

    fn submit_code(self, model: LauncherModel) {
        if !self.code_ready() || model.with_untracked(|state| state.launcher.login_in_progress()) {
            return;
        }
        let code = self.code.get_untracked().trim().to_owned();
        self.code.set(String::new());
        let code = match self.backup_code.get_untracked() {
            Checked::Yes => wgc_auth::TwoFactorCode::Backup(code),
            Checked::No | Checked::Mixed => wgc_auth::TwoFactorCode::Otp(code),
        };

        model.op(move |launcher, settings, env| launcher.submit_two_factor(code, settings, env));
    }
}

/// Opens the sign-in dialog for `target`. It closes itself once the account is signed in.
pub fn open_sign_in(shell: Shell, model: LauncherModel, target: LoginTarget) {
    model.update_launcher(|launcher| launcher.reset_login(target));
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let title = match target {
            LoginTarget::NewAccount => t!("ui.launcher.account.add_title").into_owned(),
            LoginTarget::Reauthenticate(_) => t!("ui.launcher.account.again_title").into_owned(),
        };
        let title = zgui::reactive::StoredValue::new(title);
        let finished = Memo::new(move |_| model.with(|state| state.launcher.login_finished()));
        let closing = RenderEffect::new(move |_| {
            let Some((_, storage)) = finished.get() else { return };
            if storage == SessionStorage::MemoryOnly {
                shell.toasts().warn(t!("ui.launcher.account.memory_only").into_owned());
            }
            handle.close();
        });
        on_cleanup_local(move || drop(closing));

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "launcher-sign-in-dialog") {
                    DialogHeader {
                        DialogTitle {{title.get_value()}}
                    }
                    KeyringHint(model = model)
                    SignInPanel(model = model, target = target)
                }
            }
        })
    });
    // After the dialog, so its keyring hint is up before the store can prompt.
    model.ensure_sessions();
    model.op(|launcher, settings, env| {
        // A restore reading the store answers this too, and a second read would prompt again.
        if !launcher.keyring_prompt_ahead(settings) {
            launcher.start_keyring_probe(env);
        }
        false
    });
}

/// Says, until the store first answers, that the system keyring may ask to unlock for the saved
/// sign-ins. The prompt names no application, so this says who asks.
#[component]
pub fn KeyringHint(model: LauncherModel) -> impl IntoView {
    let locale = Services::current().locale;
    let ahead = Memo::new(move |_| {
        model
            .with(|state| state.settings.as_ref().is_some_and(|settings| state.launcher.keyring_prompt_ahead(settings)))
    });

    view! {
        Show(when = move || ahead.get()) {
            text(class = "wt-launcher__dim", attr:data-testid = "launcher-keyring-hint") {{locale.text("ui.launcher.keyring.unlock_hint")}}
        }
    }
}

/// The sign-in form for `target`, at whichever step the attempt has reached.
#[component]
pub fn SignInPanel(model: LauncherModel, target: LoginTarget) -> impl IntoView {
    let realm = match target {
        LoginTarget::Reauthenticate(key) => key.realm,
        LoginTarget::NewAccount => model
            .with_untracked(|state| state.settings.as_ref().and_then(|settings| settings.sign_in_realm))
            .unwrap_or(AccountRealm::Eu),
    };
    let form = SignInForm::new(realm);
    let step = Memo::new(move |_| model.with(|state| SignInStep::of(&state.launcher)));

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide", attr:data-testid = "launcher-sign-in-panel") {
            {move || match step.get() {
                SignInStep::Credentials => AnyView::new(view! { Credentials(model = model, form = form, target = target) }),
                SignInStep::TwoFactor => AnyView::new(view! { TwoFactor(model = model, form = form) }),
                SignInStep::Captcha => AnyView::new(view! { CaptchaNotice(model = model) }),
            }}
        }
    }
}

#[component]
fn Credentials(model: LauncherModel, form: SignInForm, target: LoginTarget) -> impl IntoView {
    let locale = Services::current().locale;
    let busy = login_busy(model);
    let blocked = Signal::derive_local(move || busy.get() || !form.credentials_ready());
    let email_invalid = Signal::derive_local(move || {
        form.email.with(|email| !email.trim().is_empty() && !wows_toolkit_launcher::is_plausible_email(email))
    });
    let chosen = Signal::derive_local(move || vec![realm_value(form.realm.get())]);
    // A press on the chosen realm would leave none chosen, so only a press on another is taken.
    let choose = move |values: Vec<String>| {
        let current = form.realm.get_untracked();
        let picked = values
            .iter()
            .filter_map(|value| REALMS.into_iter().find(|realm| realm_value(*realm) == *value))
            .find(|realm| *realm != current);
        if let Some(realm) = picked {
            form.realm.set(realm);
        }
    };
    let realms: Vec<AnyView> = REALMS
        .into_iter()
        .map(|realm| {
            AnyView::new(view! {
                ToggleGroupItem(value = realm_value(realm), attr:data-testid = format!("launcher-realm-{}", realm_value(realm))) {
                    RealmGlyph(realm = realm, attr:data-testid = format!("launcher-realm-{}-glyph", realm_value(realm)))
                    text {{wows_toolkit_launcher::realm_label(realm)}}
                }
            })
        })
        .collect();
    let submit_on_enter = move |ev: &zgui::vocab::KeyEvent| {
        if ev.key == Key::Named(NamedKey::Enter) {
            form.sign_in(model, target);
        }
    };
    // A re-sign-in keeps the account's realm: a WG account exists on one realm only.
    let realm_fixed = matches!(target, LoginTarget::Reauthenticate(_));

    let realm_picker = if realm_fixed {
        AnyView::new(view! {
            text(class = "wt-launcher__dim") {{move || {
                locale.track();
                t!("ui.launcher.account.realm_fixed", realm = realm_badge(form.realm.get())).into_owned()
            }}}
        })
    } else {
        AnyView::new(view! {
            ToggleGroup(
                value = Binding::controlled(chosen, choose),
                variant = ToggleVariant::Outline,
                size = ToggleSize::Sm,
                spacing = 4.0,
                attr:data-testid = "launcher-realms"
            ) {
                {realms}
            }
        })
    };

    view! {
        {realm_picker}
        Input(
            value = form.email,
            placeholder = locale.text("ui.launcher.account.email")(),
            label = locale.text("ui.launcher.account.email")(),
            required = true,
            invalid = email_invalid,
            class = "wt-launcher__input",
            attr:data-testid = "launcher-email",
            on:key_down = move |ev| submit_on_enter(ev)
        )
        Input(
            value = form.password,
            secure = true,
            placeholder = locale.text("ui.launcher.account.password")(),
            label = locale.text("ui.launcher.account.password")(),
            required = true,
            class = "wt-launcher__input",
            attr:data-testid = "launcher-password",
            on:key_down = move |ev| submit_on_enter(ev)
        )
        KeyringNotice(model = model)
        LoginError(model = model)
        row(class = "wt-launcher__row") {
            Button(
                size = ButtonSize::Sm,
                disabled = blocked,
                attr:data-testid = "launcher-sign-in",
                on:click = move |_| form.sign_in(model, target)
            ) {
                {locale.text("ui.launcher.account.sign_in")}
            }
            if move || busy.get() {
                Spinner(attr:data-testid = "launcher-signing-in")
            }
        }
        text(class = "wt-launcher__dim") {{locale.text("ui.launcher.account.password_note")}}
    }
}

#[component]
fn TwoFactor(model: LauncherModel, form: SignInForm) -> impl IntoView {
    let locale = Services::current().locale;
    let busy = login_busy(model);
    let blocked = Signal::derive_local(move || busy.get() || !form.code_ready());
    let backup_box = NodeRef::new();
    let backup_name = NodeRef::new();
    let backup = form.backup_code;

    view! {
        text {{locale.text("ui.launcher.account.code_required")}}
        Input(
            value = form.code,
            placeholder = locale.text("ui.launcher.account.code")(),
            label = locale.text("ui.launcher.account.code")(),
            class = "wt-launcher__input",
            attr:data-testid = "launcher-code",
            on:key_down = move |ev| {
                if ev.key == Key::Named(NamedKey::Enter) {
                    form.submit_code(model);
                }
            }
        )
        row(class = "wt-launcher__row wt-launcher__row--tight") {
            Checkbox(checked = backup, node_ref = backup_box, labelled_by = backup_name, attr:data-testid = "launcher-backup-code")
            Label(
                node_ref = backup_name,
                control = backup_box,
                attr:data-testid = "launcher-backup-code-label",
                on:click = move |_| backup.set(Checked::from(!backup.get_untracked().is_checked()))
            ) {
                {locale.text("ui.launcher.account.use_backup_code")}
            }
        }
        LoginError(model = model)
        row(class = "wt-launcher__row") {
            Button(
                size = ButtonSize::Sm,
                disabled = blocked,
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

#[component]
fn CaptchaNotice(model: LauncherModel) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        text(class = "wt-launcher__warn", attr:data-testid = "launcher-captcha") {{locale.text("ui.launcher.account.captcha")}}
        row(class = "wt-launcher__row") {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "launcher-captcha-retry",
                on:click = move |_| model.update_launcher(|launcher| launcher.dismiss_captcha())
            ) {
                {locale.text("ui.launcher.retry")}
            }
        }
    }
}

/// Says, while the OS has no secret store, that a session signed in now is saved unencrypted,
/// and how to get a store.
#[component]
pub fn KeyringNotice(model: LauncherModel) -> impl IntoView {
    let locale = Services::current().locale;
    let missing = Memo::new(move |_| model.with(|state| state.launcher.keyring() == KeyringState::Unavailable));

    view! {
        Show(when = move || missing.get()) {
            column(class = "wt-launcher__column wt-launcher__keyring", attr:data-testid = "launcher-keyring-missing") {
                text(class = "wt-launcher__warn") {{locale.text("ui.launcher.keyring.unencrypted")}}
                Tooltip {
                    TooltipTrigger {
                        text(class = "wt-launcher__link", attr:data-testid = "launcher-keyring-help") {{locale.text("ui.launcher.keyring.how")}}
                    }
                    TooltipContent {{locale.text("ui.launcher.keyring.how_detail")}}
                }
            }
        }
    }
}

/// Why the last sign-in or code was refused.
#[component]
fn LoginError(model: LauncherModel) -> impl IntoView {
    let locale = Services::current().locale;
    let error = Memo::new(move |_| model.with(|state| state.launcher.login_error()));

    view! {
        {move || error.get().map(|failure| view! {
            text(class = "wt-launcher__error", attr:data-testid = "launcher-login-error") {{locale.text(wows_toolkit_launcher::sign_in_failure_key(failure))}}
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
    realm_badge(realm).to_lowercase()
}

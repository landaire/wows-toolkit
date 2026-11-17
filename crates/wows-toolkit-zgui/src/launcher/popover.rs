//! The account popover under the Play chevron: the install line, the saved accounts and their
//! sessions, and the ways to the rarer launcher tasks.

use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wgc_auth::TokenExpiry;
use wows_toolkit_launcher::AccountKey;
use wows_toolkit_launcher::AccountStatus;
use wows_toolkit_launcher::LoginTarget;
use wows_toolkit_launcher::SessionStorage;
use wows_toolkit_viewmodel::settings_drawer::Setting;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::set_interval;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::SeparatorProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::ELLIPSIS;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::launcher::account::KeyringHintProps;
use crate::launcher::account::open_sign_in;
use crate::launcher::display::AccountRow;
use crate::launcher::display::CatalogView;
use crate::launcher::display::DownloadTarget;
use crate::launcher::display::PlayState;
use crate::launcher::download::DownloadStatusProps;
use crate::launcher::installs::add_install;
use crate::launcher::model::LauncherModel;
use crate::launcher::model::PlayOrigin;
use crate::launcher::play::SafeModeCheckProps;
use crate::launcher::play::play;
use crate::launcher::realm::RealmGlyphProps;
use crate::launcher::replay;
use crate::launcher::setup;
use crate::launcher::steam_login::WgcLoginControlProps;
use crate::shell::Shell;

/// How often an account row reads the time again for its session expiry.
const EXPIRY_TICK: Duration = Duration::from_secs(60);

const POPOVER_CSS: &str = zgui::css!(
    ".wt-account-popover__body {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .wt-account-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        padding: 4px 6px;
        border-radius: 4px;
        min-width: 0;
    }
    .wt-account-row[data-default] {
        background-color: var(--lui-color-selection);
    }
    .wt-account-row__name {
        flex: 1 1 0;
        min-width: 48px;
        overflow: hidden;
    }
    .wt-account-row__choose {
        min-width: 0;
        max-width: 100%;
    }
    .wt-account-row__status {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 0 1 auto;
        min-width: 0;
        overflow: hidden;
    }
    .wt-account-row__status .wt-launcher__dim,
    .wt-account-row__status .wt-launcher__warn {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-account-row__actions {
        flex: none;
    }
    .wt-account-popover__link {
        justify-content: flex-start;
    }"
);

/// The popover's content. `open` is the popover's own, so an action that leaves it can close it.
#[component]
pub fn AccountPopover(shell: Shell, model: LauncherModel, open: RwSignal<bool, LocalStorage>) -> impl IntoView {
    zgui::view::install_stylesheet("wt-account-popover", POPOVER_CSS);
    let locale = shell.services().locale;
    let rows = Memo::new(move |_| model.with(AccountRow::all));
    let running = Memo::new(move |_| model.with(|state| !state.launcher.game_state().is_idle()));
    let state = Memo::new(move |_| {
        let wows_dir = model.wows_dir();
        model.with(|state| PlayState::of(state, wows_dir.as_deref(), false))
    });
    let close_then = move |action: Box<dyn Fn()>| {
        open.set(false);
        action();
    };
    // The rows show each account's session, so opening is when the store is first read.
    let reading = RenderEffect::new(move |_| {
        if open.get() {
            model.ensure_sessions();
        }
    });
    on_cleanup_local(move || drop(reading));

    let play_anyway = move || {
        match state.get() {
        PlayState::Update { account, .. } => Some(
            view! {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Sm,
                    class = "wt-account-popover__link",
                    attr:data-testid = "launcher-play-anyway",
                    on:click = move |_| close_then(Box::new(move || play(shell, model, account.key, PlayOrigin::Default)))
                ) {
                    {t!("ui.launcher.popover.play_anyway").into_owned()}
                }
            }
            .into_view(),
        ),
        _ => None,
    }
    };

    view! {
        column(class = "wt-account-popover__body") {
            InstallLine(shell = shell, model = model, open = open)
            Separator()
            text(class = "wt-launcher__dim") {{locale.text("ui.launcher.popover.accounts")}}
            KeyringHint(model = model)
            if move || rows.with(Vec::is_empty) {
                text(class = "wt-launcher__dim", attr:data-testid = "launcher-no-accounts") {{locale.text("ui.launcher.popover.no_accounts")}}
            } else {}
            if move || running.get() {
                text(class = "wt-launcher__dim", attr:data-testid = "launcher-close-to-switch") {{locale.text("ui.launcher.popover.close_to_switch")}}
            } else {}
            for row in move || rows.get(), key = |row: &AccountRow| row.clone() {
                AccountRowView(shell = shell, model = model, row = row, running = running, open = open)
            }
            Separator()
            column(class = "wt-launcher__column") {{play_anyway}}
            SafeModeCheck(model = model, test_id = "launcher-play-safe-mode")
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::Sm,
                class = "wt-account-popover__link",
                attr:data-testid = "launcher-add-account",
                on:click = move |_| close_then(Box::new(move || open_sign_in(shell, model, LoginTarget::NewAccount)))
            ) {
                {locale.text("ui.launcher.popover.add_account")}
            }
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::Sm,
                class = "wt-account-popover__link",
                attr:data-testid = "launcher-open-game-settings",
                on:click = move |_| close_then(Box::new(move || crate::launcher::game_settings::open(shell, model)))
            ) {
                {locale.text("ui.launcher.popover.game_settings")}
            }
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::Sm,
                class = "wt-account-popover__link",
                attr:data-testid = "launcher-open-setup",
                on:click = move |_| close_then(Box::new(move || shell.settings_drawer.open_at(Setting::GameInstalls)))
            ) {
                {locale.text("ui.launcher.popover.installs")}
            }
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::Sm,
                class = "wt-account-popover__link",
                disabled = Signal::derive_local(move || running.get()),
                attr:data-testid = "launcher-open-replay",
                on:click = move |_| close_then(Box::new(move || replay::pick_and_open(shell, model)))
            ) {
                {locale.text("ui.launcher.popover.open_replay")}
            }
        }
    }
}

/// The default account's install: its version, its update check and its download. Without one,
/// the ways to set one.
#[component]
fn InstallLine(shell: Shell, model: LauncherModel, open: RwSignal<bool, LocalStorage>) -> impl IntoView {
    let locale = shell.services().locale;
    let install: Memo<Option<(usize, PathBuf)>> = Memo::new(move |_| {
        let wows_dir = model.wows_dir();
        model.with(|state| {
            let settings = state.settings.as_ref()?;
            let account = settings.default_account().or_else(|| settings.accounts.first());
            let path = match account {
                Some(account) => account.install.resolve(wows_dir.as_deref()),
                None => wows_dir,
            }?;
            let slot = state.launcher.installs().iter().position(|install| install.path == path)?;
            Some((slot, path))
        })
    });
    let version = Memo::new(move |_| {
        let (slot, _) = install.get()?;
        model.with(|state| {
            let version = state.launcher.installs().get(slot)?.version?;
            Some(format!("{}.{}.{}", version.major, version.minor, version.patch))
        })
    });
    let catalog = Memo::new(move |_| {
        let Some((_, path)) = install.get() else { return CatalogView::Nothing };
        model.with(|state| CatalogView::of(state.launcher.catalog_status(&path)))
    });
    let status = move || {
        locale.track();
        let version = version.get().unwrap_or_default();
        let check = match catalog.get() {
            CatalogView::Checking => t!("ui.launcher.checking_updates").into_owned(),
            CatalogView::Found { updates, .. } if updates.is_empty() => t!("ui.launcher.up_to_date").into_owned(),
            CatalogView::Found { .. } => t!("ui.launcher.popover.update_available").into_owned(),
            CatalogView::Failed(_) => t!("ui.launcher.popover.check_failed").into_owned(),
            CatalogView::Nothing => String::new(),
        };
        format!("{version}   {check}").trim().to_owned()
    };
    let failed = Memo::new(move |_| matches!(catalog.get(), CatalogView::Failed(_)));

    view! {
        column(class = "wt-launcher__column", attr:data-testid = "launcher-install-line") {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__strong") {{locale.text("ui.launcher.popover.game")}}
                box(class = "wt-launcher__spacer")
            }
            {move || match install.get() {
                None => AnyView::new(view! {
                    column(class = "wt-launcher__column") {
                        text(class = "wt-launcher__dim", attr:data-testid = "launcher-install-none") {{locale.text("ui.launcher.popover.no_install")}}
                        row(class = "wt-launcher__row wt-launcher__row--tight") {
                            Button(
                                variant = ButtonVariant::Outline,
                                size = ButtonSize::Xs,
                                attr:data-testid = "launcher-install-set-up",
                                on:click = move |_| {
                                    open.set(false);
                                    setup::open(shell, model);
                                }
                            ) {
                                {locale.text("ui.launcher.play.set_up_game")}
                            }
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::Xs,
                                attr:data-testid = "launcher-install-add",
                                on:click = move |_| add_install(model, shell)
                            ) {
                                {locale.text("ui.launcher.add_install")}
                            }
                        }
                    }
                }),
                Some((slot, path)) => {
                    let login_path = path.clone();
                    AnyView::new(view! {
                    column(class = "wt-launcher__column") {
                        text(class = "wt-launcher__dim wt-launcher__truncate") {{path.display().to_string()}}
                        row(class = "wt-launcher__row") {
                            text(class = "wt-launcher__text wt-launcher-install-status", attr:data-testid = "launcher-install-status") {{status}}
                            if move || failed.get() {
                                Button(
                                    variant = ButtonVariant::Ghost,
                                    size = ButtonSize::Xs,
                                    attr:data-testid = "launcher-recheck",
                                    on:click = {
                                        let path = path.clone();
                                        move |_| {
                                        let path = path.clone();
                                        model.op(move |launcher, _, env| {
                                            launcher.recheck_catalog(&path, env);
                                            false
                                        });
                                        }
                                    }
                                ) {
                                    {t!("ui.launcher.retry").into_owned()}
                                }
                            } else {}
                        }
                        DownloadStatus(model = model, shell = shell, target = DownloadTarget::Install(slot))
                        WgcLoginControl(model = model, path = login_path, test_id = "launcher-install-wgc-login".to_owned(), brief = true)
                    }
                })
                }
            }}
        }
    }
}

/// One saved account: its realm, name and session, a press making it the default, and a menu of
/// the rest.
#[component]
fn AccountRowView(
    shell: Shell,
    model: LauncherModel,
    row: AccountRow,
    running: Memo<bool>,
    open: RwSignal<bool, LocalStorage>,
) -> impl IntoView {
    let locale = shell.services().locale;
    let AccountRow { label, is_default, status, expiry, other_install } = row;
    let key = label.key;
    let id = format!("launcher-account-{}-{}", label.realm.to_lowercase(), key.account_id.get());
    let not_saved_id = format!("{id}-not-saved");
    let sign_in_id = format!("{id}-sign-in");
    let choose_id = format!("{id}-choose");
    let status_id = format!("{id}-status");
    let actions_id = format!("{id}-actions");
    let make_default_id = format!("{id}-make-default");
    let sign_out_id = format!("{id}-sign-out");
    let forget_id = format!("{id}-forget");
    let signed_in = matches!(status, AccountStatus::Ready(_));
    let status_view = match status {
        AccountStatus::Ready(SessionStorage::Keyring) if is_default => {
            AnyView::new(view! { text(class = "wt-launcher__dim") {{locale.text("ui.launcher.popover.default")}} })
        }
        AccountStatus::Ready(SessionStorage::Keyring) => {
            AnyView::new(view! { text(class = "wt-launcher__dim") {{locale.text("ui.launcher.popover.ready")}} })
        }
        AccountStatus::Ready(SessionStorage::MemoryOnly) => AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    text(class = "wt-launcher__warn", attr:data-testid = not_saved_id) {{locale.text("ui.launcher.popover.not_saved")}}
                }
                TooltipContent {{locale.text("ui.launcher.account.memory_only")}}
            }
        }),
        AccountStatus::Ready(SessionStorage::PlaintextFile) => AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    text(class = "wt-launcher__warn", attr:data-testid = not_saved_id) {{locale.text("ui.launcher.popover.unencrypted")}}
                }
                TooltipContent {
                    column(class = "wt-launcher__column") {
                        text {{locale.text("ui.launcher.keyring.unencrypted")}}
                        text {{locale.text("ui.launcher.keyring.how_detail")}}
                    }
                }
            }
        }),
        AccountStatus::NotLoaded | AccountStatus::Restoring => AnyView::new(view! {
            text(class = "wt-launcher__dim") {{locale.text("ui.launcher.popover.restoring")}}
        }),
        AccountStatus::KeyringLocked => AnyView::new(view! {
            text(class = "wt-launcher__warn", attr:data-testid = "launcher-keyring-locked") {{locale.text("ui.launcher.popover.keyring_locked")}}
        }),
        AccountStatus::Expired | AccountStatus::SignedOut => AnyView::new(view! {
            Button(
                variant = ButtonVariant::Link,
                size = ButtonSize::Xs,
                attr:data-testid = sign_in_id,
                on:click = move |_| {
                    open.set(false);
                    open_sign_in(shell, model, LoginTarget::Reauthenticate(key));
                }
            ) {
                {locale.text("ui.launcher.play.sign_in_again")}
            }
        }),
    };
    let install_note = other_install.map(|path| {
        view! { text(class = "wt-launcher__dim wt-launcher__truncate") {{path.display().to_string()}} }
    });
    let expiry_note = expiry.map(|expiry| {
        // The duration counts down in minutes, and a deadline that passes while the row is up
        // turns it into the expired warning.
        let now = RwSignal::new_local(SystemTime::now());
        let ticking = matches!(expiry, TokenExpiry::KnownAt(_))
            .then(|| set_interval(EXPIRY_TICK, move || now.set(SystemTime::now())));
        on_cleanup_local(move || drop(ticking));
        let lapsed = move || matches!(expiry, TokenExpiry::KnownAt(at) if at <= now.get());
        view! {
            text(
                class = "wt-launcher__truncate wt-account-row__expiry",
                class:wt-launcher__dim = move || !lapsed(),
                class:wt-launcher__warn = lapsed
            ) {{move || {
                locale.track();
                expiry_text(expiry, now.get())
            }}}
        }
    });

    view! {
        row(
            class = "wt-account-row",
            attr:data-testid = id.clone(),
            attr:data-default = is_default.then(String::new)
        ) {
            RealmGlyph(realm = key.realm, attr:data-testid = format!("{id}-realm"))
            text(class = "wt-launcher__badge") {{label.realm}}
            column(class = "wt-account-row__name") {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Xs,
                    class = "wt-account-popover__link wt-account-row__choose",
                    disabled = Signal::derive_local(move || running.get()),
                    attr:data-testid = choose_id,
                    a11y:label = label.nickname.clone(),
                    on:click = move |_| make_default(model, key)
                ) {
                    text(class = "wt-launcher__truncate") {{label.nickname.clone()}}
                }
                {install_note}
                {expiry_note}
            }
            box(class = "wt-account-row__status", attr:data-testid = status_id) {{status_view}}
            DropdownMenu {
                DropdownMenuTrigger(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconXs,
                    class = "wt-account-row__actions",
                    attr:data-testid = actions_id,
                    a11y:label = locale.text("ui.launcher.popover.account_actions")
                ) {
                    Icon(icon = ELLIPSIS)
                }
                DropdownMenuContent(placement = Placement::new(Side::Bottom, Align::End)) {
                    MenuItem(
                        disabled = Signal::derive_local(move || running.get() || is_default),
                        attr:data-testid = make_default_id.clone(),
                        on_select = UnsyncCallback::new(move |()| make_default(model, key))
                    ) {
                        {locale.text("ui.launcher.popover.make_default")}
                    }
                    MenuItem(
                        disabled = Signal::derive_local(move || running.get() || !signed_in),
                        attr:data-testid = sign_out_id.clone(),
                        on_select = UnsyncCallback::new(move |()| model.op(move |launcher, settings, env| {
                            launcher.sign_out(key, settings, env);
                            false
                        }))
                    ) {
                        {locale.text("ui.launcher.popover.sign_out")}
                    }
                    MenuItem(
                        disabled = Signal::derive_local(move || running.get()),
                        attr:data-testid = forget_id.clone(),
                        on_select = UnsyncCallback::new(move |()| model.op(move |launcher, settings, env| launcher.forget(key, settings, env)))
                    ) {
                        {locale.text("ui.launcher.popover.forget")}
                    }
                }
            }
        }
    }
}

fn make_default(model: LauncherModel, key: AccountKey) {
    model.edit_settings(|settings| {
        settings.set_default_account(key);
    });
}

/// How long the session `expiry` has left at `now`.
fn expiry_text(expiry: TokenExpiry, now: SystemTime) -> String {
    match expiry {
        TokenExpiry::KnownAt(at) => match at.duration_since(now) {
            Ok(remaining) => {
                t!("ui.launcher.popover.expires_in", duration = wows_toolkit_launcher::format_duration(remaining))
            }
            Err(_) => t!("ui.launcher.popover.expired"),
        },
        TokenExpiry::Unknown => t!("ui.launcher.popover.expiry_unknown"),
    }
    .into_owned()
}

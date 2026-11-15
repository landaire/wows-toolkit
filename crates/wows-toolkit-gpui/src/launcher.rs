//! The Launcher tab: discover, launch and manage WoWs installs, sign in, and
//! install a game fresh. The state and background work are shared with the
//! egui app through `wows_toolkit_launcher`; this is the view over them.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::input::TextareaState;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wgc_catalog::known_games::KnownGame;
use wgc_install::GameInstall;
use wows_toolkit_launcher::AccountStatus;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::CatalogSummary;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherEnv;
use wows_toolkit_launcher::LauncherError;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::LoginTarget;
use wows_toolkit_launcher::SessionStorage;
use wows_toolkit_launcher::sign_in_failure_key;

use crate::runtime;

const SECTION_RADIUS: Pixels = px(6.);
const INPUT_WIDTH: Pixels = px(240.);
const PROGRESS_WIDTH: Pixels = px(220.);
/// Where the known games' download controls start numbering, past any
/// install row's.
const GAME_SLOTS: usize = 1 << 16;

/// The reader picked an install to drive the rest of the app from.
pub struct UseForToolkit(pub PathBuf);

pub struct LauncherView {
    launcher: Launcher,
    /// `None` until the settings row has been read, which needs the database.
    settings: Option<LauncherSettings>,
    settings_requested: bool,
    /// The resolved proxy the app's own requests use.
    proxy_url: Option<String>,
    /// Applies to the next launch only, as in the egui tab.
    safe_mode: bool,
    realm: wgc_auth::AccountRealm,
    show_sign_in: bool,
    use_backup_code: bool,
    username: Entity<InputState>,
    password: Entity<InputState>,
    two_factor_code: Entity<InputState>,
    extra_args: Entity<TextareaState>,
    env_vars: Entity<TextareaState>,
    scroll: ScrollHandle,
    _events: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<UseForToolkit> for LauncherView {}

impl LauncherView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (launcher, mut events) = Launcher::new();
        // Background work reports through the channel; each event is applied
        // on the app's thread and redraws the tab.
        let events_task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.apply(event, cx)).is_err() {
                    break;
                }
            }
        });
        let username =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.launcher.account.email").to_string()));
        let password = cx.new(|cx| {
            InputState::new(window, cx).masked(true).placeholder(t!("ui.launcher.account.password").to_string())
        });
        let two_factor_code =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.launcher.account.code").to_string()));
        let extra_args = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let env_vars = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let subscriptions = vec![
            cx.subscribe(&extra_args, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    this.edit_settings(cx, |settings| settings.set_extra_args_text(&text));
                }
            }),
            cx.subscribe(&env_vars, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    this.edit_settings(cx, |settings| settings.set_env_vars_text(&text));
                }
            }),
        ];
        LauncherView {
            launcher,
            settings: None,
            settings_requested: false,
            proxy_url: None,
            safe_mode: false,
            realm: wgc_auth::AccountRealm::Eu,
            show_sign_in: false,
            use_backup_code: false,
            username,
            password,
            two_factor_code,
            extra_args,
            env_vars,
            scroll: ScrollHandle::new(),
            _events: events_task,
            _subscriptions: subscriptions,
        }
    }

    /// The proxy the launcher's own requests go through, from the app's
    /// setting. Empty is no manual proxy.
    pub fn set_proxy(&mut self, proxy_url: &str) {
        self.proxy_url = wows_toolkit_viewmodel::proxy::resolve_proxy(Some(proxy_url)).map(|proxy| proxy.url);
    }

    /// Adopts the app's proxy setting, and on the first call reads the
    /// launcher's own settings row and starts discovery. Called once the
    /// config database is open.
    pub fn apply_settings(&mut self, proxy_url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_proxy(proxy_url);
        if std::mem::replace(&mut self.settings_requested, true) {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let stored = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<LauncherSettings>(
                    &pool,
                    wows_toolkit_launcher::SETTINGS_KEY,
                )
                .await
            })
            .await;
            // No row is a launcher never configured, for which the defaults are
            // right; an unreadable row is logged by `get_setting` and starts
            // from the defaults too, as the egui app does.
            let settings = stored.ok().flatten().unwrap_or_default();
            let _ = this.update_in(cx, |this, window, cx| this.adopt_settings(settings, window, cx));
        })
        .detach();
    }

    fn adopt_settings(&mut self, settings: LauncherSettings, window: &mut Window, cx: &mut Context<Self>) {
        let args = settings.extra_args_text();
        let env = settings.env_vars_text();
        self.extra_args.update(cx, |state, cx| state.set_value(args, window, cx));
        self.env_vars.update(cx, |state, cx| state.set_value(env, window, cx));
        let enabled = settings.is_enabled();
        self.settings = Some(settings);
        // Nothing starts before the opt-in. Sessions wait for Play or a sign-in, since reading
        // them may prompt to unlock the system keyring.
        if enabled {
            self.op(cx, |launcher, settings, env| {
                launcher.start_install_discovery(settings, env);
                false
            });
        }
    }

    /// Reads the saved sessions not read yet, or asks the store whether it is there when there
    /// are none, so the account section can say where a sign-in will be kept.
    fn ensure_sessions(&mut self, cx: &mut Context<Self>) {
        self.op(cx, |launcher, settings, env| {
            let changed = launcher.ensure_sessions(settings, env);
            // A restore reading the store answers this too, and a second read would prompt again.
            if !launcher.keyring_prompt_ahead(settings) {
                launcher.start_keyring_probe(env);
            }
            changed
        });
    }

    fn env(&self, cx: &App) -> Option<LauncherEnv> {
        let Some(runtime) = runtime::runtime(cx) else {
            tracing::warn!("launcher: tokio runtime is not available");
            return None;
        };
        let storage_dir = wows_toolkit_config::storage_dir();
        Some(LauncherEnv {
            runtime: runtime.handle().clone(),
            proxy_url: self.proxy_url.clone(),
            // Events are awaited by `_events`, which needs no other wake-up.
            wake: Arc::new(|| {}),
            runtime_dirs: storage_dir.as_deref().map(wgc_launch::RuntimeDirs::new),
            credentials: wgc_auth::CredentialStore::new(storage_dir.as_deref()),
        })
    }

    /// Runs a launcher operation against the settings, saving them when it
    /// reports a change. A no-op before the settings are read.
    fn op(
        &mut self,
        cx: &mut Context<Self>,
        op: impl FnOnce(&mut Launcher, &mut LauncherSettings, &LauncherEnv) -> bool,
    ) {
        let Some(env) = self.env(cx) else { return };
        let Some(settings) = self.settings.as_mut() else { return };
        if op(&mut self.launcher, settings, &env) {
            self.save(cx);
        }
        cx.notify();
    }

    fn apply(&mut self, event: wows_toolkit_launcher::LauncherEvent, cx: &mut Context<Self>) {
        self.op(cx, |launcher, settings, env| launcher.apply(event, settings, env));
    }

    fn edit_settings(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut LauncherSettings)) {
        let Some(settings) = self.settings.as_mut() else { return };
        edit(settings);
        self.save(cx);
        cx.notify();
    }

    fn save(&self, cx: &mut Context<Self>) {
        if let Some(settings) = &self.settings {
            crate::settings_store::save(wows_toolkit_launcher::SETTINGS_KEY, settings, cx);
        }
    }

    fn report(result: Result<(), LauncherError>, window: &mut Window, cx: &mut App) {
        if let Err(err) = result {
            crate::toast::failed(err.to_string(), window, cx);
        }
    }

    fn add_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.launcher.add_install_title"));
        cx.spawn_in(window, async move |this, cx| {
            let Some(dir) = crate::dialog::chosen(asked, &this, cx).await else { return };
            let _ = this.update(cx, |this, cx| {
                this.op(cx, |launcher, settings, env| {
                    settings.manual_installs.push(dir);
                    launcher.start_install_discovery(settings, env);
                    true
                })
            });
        })
        .detach();
    }

    fn download(&mut self, install: PathBuf, start: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let asked = wows_toolkit_services::dialog::pick_folder_in(&t!("ui.launcher.download_title"), &start);
        cx.spawn_in(window, async move |this, cx| {
            let Some(dest) = crate::dialog::chosen(asked, &this, cx).await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                let mut result = Ok(());
                this.op(cx, |launcher, _, env| {
                    result = launcher.start_download(install, dest, env);
                    false
                });
                Self::report(result, window, cx);
            });
        })
        .detach();
    }

    fn install_game(&mut self, game: KnownGame, window: &mut Window, cx: &mut Context<Self>) {
        let asked = wows_toolkit_services::dialog::pick_folder_in(
            &t!("ui.launcher.install_title"),
            &wgc_download::default_install_dir(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let Some(dest) = crate::dialog::chosen(asked, &this, cx).await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                let mut result = Ok(());
                this.op(cx, |launcher, _, env| {
                    result = launcher.install_known_game(&game, dest, env);
                    false
                });
                Self::report(result, window, cx);
            });
        })
        .detach();
    }

    fn resume(&mut self, key: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let mut result = Ok(());
        self.op(cx, |launcher, _, env| {
            result = launcher.resume_download(&key, env);
            false
        });
        Self::report(result, window, cx);
    }

    /// Plays the default account. An account set to the toolkit's install plays `install`.
    fn launch(&mut self, install: GameInstall, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.settings.as_ref().and_then(|settings| settings.default_account) else { return };
        // An unread session is read first; the account section says the keyring may ask, and a
        // second press plays.
        if self.launcher.account_status(key) == AccountStatus::NotLoaded {
            self.ensure_sessions(cx);
            return;
        }
        let safe_mode = self.safe_mode;
        let mut launched = Ok(());
        self.op(cx, |launcher, settings, env| {
            launched = launcher.play(key, Some(&install.path), safe_mode, settings, env, |command| {
                wows_toolkit_services::child_process::prepare(command);
            });
            false
        });
        if let Err(err) = launched {
            crate::toast::failed(t!("ui.launcher.launch_failed", error = err.to_string()).into_owned(), window, cx);
        }
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username = self.username.read(cx).value().trim().to_string();
        // Cleared from the field the moment it is used.
        let password = wgc_auth::SecretPassword::new(self.password.read(cx).value().to_string());
        self.password.update(cx, |state, cx| state.set_value("", window, cx));
        let realm = self.realm;
        self.op(cx, |launcher, settings, env| {
            launcher.start_login(LoginTarget::NewAccount, realm, username, password, settings, env)
        });
    }

    fn submit_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.two_factor_code.read(cx).value().trim().to_string();
        self.two_factor_code.update(cx, |state, cx| state.set_value("", window, cx));
        let code = if self.use_backup_code {
            wgc_auth::TwoFactorCode::Backup(code)
        } else {
            wgc_auth::TwoFactorCode::Otp(code)
        };
        self.op(cx, |launcher, settings, env| launcher.submit_two_factor(code, settings, env));
    }

    fn render_account(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let launcher = &self.launcher;
        let default_account = self.settings.as_ref().and_then(|settings| settings.default_account);
        let signed_in = default_account.and_then(|key| launcher.session(key).map(|session| (key, session)));
        let body = if let Some((key, session)) = signed_in {
            let expiry = match session.expiry {
                wgc_auth::TokenExpiry::KnownAt(at) => match at.duration_since(std::time::SystemTime::now()) {
                    Ok(remaining) => dim(t!(
                        "ui.launcher.account.expires_in",
                        duration = wows_toolkit_launcher::format_duration(remaining)
                    )),
                    Err(_) => colored(t!("ui.launcher.account.expired"), cx.theme().danger),
                },
                wgc_auth::TokenExpiry::Unknown => dim(t!("ui.launcher.account.expiry_unknown")),
            };
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().font_weight(FontWeight::BOLD).child(session.nickname.as_str().to_string()))
                        .child(dim(t!(
                            "ui.launcher.account.region",
                            region = wows_toolkit_launcher::realm_label(session.realm)
                        ))),
                )
                .child(expiry)
                .when(launcher.account_status(key) != AccountStatus::Ready(SessionStorage::Keyring), |this| {
                    this.child(dim(t!("ui.launcher.account.not_persisted")))
                })
                .child(
                    h_flex().child(
                        Button::new("launcher-sign-out")
                            .label(t!("ui.launcher.account.sign_out").to_string())
                            .compact()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.op(cx, |launcher, settings, env| {
                                    launcher.sign_out(key, settings, env);
                                    false
                                })
                            })),
                    ),
                )
                .into_any_element()
        } else if launcher.two_factor_pending() {
            v_flex()
                .gap_2()
                .child(t!("ui.launcher.account.code_required").to_string())
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().w(INPUT_WIDTH).child(Input::new(&self.two_factor_code).id("launcher-code").small()),
                        )
                        .child(
                            Checkbox::new("launcher-backup-code")
                                .label(t!("ui.launcher.account.use_backup_code").to_string())
                                .checked(self.use_backup_code)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.use_backup_code = *checked;
                                    cx.notify();
                                })),
                        ),
                )
                .children(
                    launcher
                        .login_error()
                        .map(|failure| colored(t!(sign_in_failure_key(failure)).to_string(), cx.theme().danger)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("launcher-submit-code")
                                .label(t!("ui.launcher.account.submit_code").to_string())
                                .compact()
                                .disabled(launcher.login_in_progress())
                                .on_click(cx.listener(|this, _, window, cx| this.submit_code(window, cx))),
                        )
                        .when(launcher.login_in_progress(), |this| this.child(Spinner::new().xsmall())),
                )
                .into_any_element()
        } else if launcher.captcha_blocked() {
            v_flex()
                .gap_2()
                .child(colored(t!("ui.launcher.account.captcha"), cx.theme().warning))
                .child(
                    h_flex().child(
                        Button::new("launcher-captcha-retry")
                            .label(t!("ui.launcher.retry").to_string())
                            .compact()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.launcher.dismiss_captcha();
                                this.show_sign_in = true;
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element()
        } else if default_account.is_some_and(|key| launcher.account_status(key) == AccountStatus::Restoring) {
            h_flex()
                .gap_2()
                .items_center()
                .child(dim(t!("ui.launcher.popover.restoring")))
                .child(Spinner::new().xsmall())
                .into_any_element()
        } else if !self.show_sign_in {
            h_flex()
                .gap_2()
                .items_center()
                .child(dim(t!("ui.launcher.account.signed_out")))
                .child(
                    Button::new("launcher-show-sign-in")
                        .label(t!("ui.launcher.account.sign_in").to_string())
                        .compact()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_sign_in = true;
                            this.ensure_sessions(cx);
                        })),
                )
                .into_any_element()
        } else {
            let realms =
                h_flex().gap_1().children(wows_toolkit_launcher::REALMS.iter().enumerate().map(|(i, realm)| {
                    let realm = *realm;
                    crate::ui::selectable(
                        ("launcher-realm", i),
                        self.realm == realm,
                        Button::new(("launcher-realm-button", i))
                            .label(wows_toolkit_launcher::realm_label(realm))
                            .compact()
                            .selected(self.realm == realm)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.realm = realm;
                                cx.notify();
                            })),
                    )
                }));
            v_flex()
                .gap_2()
                .child(realms)
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().w(INPUT_WIDTH).child(Input::new(&self.username).id("launcher-username").small()))
                        .child(div().w(INPUT_WIDTH).child(Input::new(&self.password).id("launcher-password").small())),
                )
                .children(
                    launcher
                        .login_error()
                        .map(|failure| colored(t!(sign_in_failure_key(failure)).to_string(), cx.theme().danger)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("launcher-sign-in")
                                .label(t!("ui.launcher.account.sign_in").to_string())
                                .primary()
                                .compact()
                                .disabled(launcher.login_in_progress())
                                .on_click(cx.listener(|this, _, window, cx| this.sign_in(window, cx))),
                        )
                        .when(launcher.login_in_progress(), |this| this.child(Spinner::new().xsmall())),
                )
                .into_any_element()
        };
        let prompt_ahead = self.settings.as_ref().is_some_and(|settings| self.launcher.keyring_prompt_ahead(settings));
        let body = v_flex()
            .gap_2()
            .when(prompt_ahead, |this| this.child(dim(t!("ui.launcher.keyring.unlock_hint"))))
            .child(body);
        section(t!("ui.launcher.account.heading"), body, cx)
    }

    fn render_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = v_flex()
            .gap_2()
            .child(t!("ui.launcher.options.extra_args").to_string())
            .child(Textarea::new(&self.extra_args).w_full())
            .child(t!("ui.launcher.options.env_vars").to_string())
            .child(Textarea::new(&self.env_vars).w_full());
        section(t!("ui.launcher.options.heading"), body, cx)
    }

    fn render_install(&self, index: usize, install: &GameInstall, cx: &mut Context<Self>) -> AnyElement {
        let launcher = &self.launcher;
        let version = match &install.version {
            Some(v) => Some(format!("{}.{}.{}", v.major, v.minor, v.patch)),
            None if install.source.is_read_only_content() => Some(t!("ui.launcher.managed_by_steam").into_owned()),
            None => None,
        };
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(div().font_weight(FontWeight::BOLD).child(install.source.label()))
            .child(div().font_family(cx.theme().mono_font_family.clone()).child(install.path.display().to_string()))
            .children(version);

        let mut body = v_flex().gap_2().child(header);
        if !install.source.is_read_only_content() {
            let path = install.path.clone();
            body = body.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(catalog_status(launcher.catalog_status(&install.path), cx))
                    .child(
                        Button::new(("launcher-recheck", index))
                            .label(t!("ui.launcher.recheck").to_string())
                            .compact()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let path = path.clone();
                                this.op(cx, |launcher, _, env| {
                                    launcher.recheck_catalog(&path, env);
                                    false
                                })
                            })),
                    ),
            );
            let state = launcher.download_state(&install.path);
            body = body.children(download_status(index, &install.path, state, cx));
            if launcher.has_downloads(&install.path) && !state.is_some_and(DownloadState::is_active) {
                // An install with no detected version has no content on disk
                // yet, so the picker starts at the platform default.
                let start =
                    if install.version.is_some() { install.path.clone() } else { wgc_download::default_install_dir() };
                let path = install.path.clone();
                body = body.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new(("launcher-download", index))
                                .label(t!("ui.launcher.download").to_string())
                                .compact()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.download(path.clone(), start.clone(), window, cx)
                                })),
                        )
                        .child(dim(t!("ui.launcher.download_hint"))),
                );
            }
        }
        let for_launch = install.clone();
        let for_toolkit = install.path.clone();
        body = body.child(
            h_flex()
                .gap_2()
                .child(
                    Button::new(("launcher-launch", index))
                        .label(t!("ui.launcher.launch").to_string())
                        .primary()
                        .compact()
                        .on_click(cx.listener(move |this, _, window, cx| this.launch(for_launch.clone(), window, cx))),
                )
                .child(
                    Button::new(("launcher-use", index))
                        .label(t!("ui.launcher.use_for_toolkit").to_string())
                        .compact()
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(UseForToolkit(for_toolkit.clone())))),
                ),
        );
        card(body, cx)
    }

    fn render_install_game(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // Rendered only while the tab is shown; once every game has a result or
        // a check in flight, this never fires again.
        if !self.launcher.available_games_checked() {
            self.op(cx, |launcher, _, env| {
                launcher.start_available_games_check(env);
                false
            });
        }
        let launcher = &self.launcher;
        let games = wgc_catalog::known_games::known_games().iter().enumerate().map(|(index, known)| {
            let game = *known;
            let version = match launcher.available_game(known) {
                Some(CatalogCheckState::Checking) => h_flex()
                    .gap_1()
                    .child(Spinner::new().xsmall())
                    .child(dim(t!("ui.launcher.checking_version")))
                    .into_any_element(),
                Some(CatalogCheckState::Ready(build)) => match wows_toolkit_launcher::fresh_install_version(build) {
                    Some(version) => {
                        div().child(t!("ui.launcher.version", version = version).to_string()).into_any_element()
                    }
                    None => dim(t!("ui.launcher.version_unreported")).into_any_element(),
                },
                Some(CatalogCheckState::Error(detail)) => h_flex()
                    .gap_2()
                    .items_center()
                    .child(dim(t!("ui.launcher.unavailable", error = detail)))
                    .child(
                        Button::new(("launcher-retry-game", index))
                            .label(t!("ui.launcher.retry").to_string())
                            .compact()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.op(cx, |launcher, _, env| {
                                    launcher.start_available_games_check(env);
                                    false
                                })
                            })),
                    )
                    .into_any_element(),
                Some(CatalogCheckState::NoUpdateHost) | None => div().into_any_element(),
            };
            let download = launcher
                .installing_download(known)
                .and_then(|(key, state)| download_status(GAME_SLOTS + index, key, Some(state), cx));
            card(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().font_weight(FontWeight::BOLD).child(known.display_name))
                            .child(version),
                    )
                    .children(download)
                    .child(
                        h_flex().child(
                            Button::new(("launcher-install-game", index))
                                .label(t!("ui.launcher.install").to_string())
                                .compact()
                                .disabled(!launcher.can_install(known))
                                .on_click(cx.listener(move |this, _, window, cx| this.install_game(game, window, cx))),
                        ),
                    ),
                cx,
            )
        });
        let body = v_flex().gap_2().child(dim(t!("ui.launcher.install_hint"))).children(games.collect::<Vec<_>>());
        section(t!("ui.launcher.install_heading"), body, cx)
    }
}

impl Render for LauncherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.settings.is_none() {
            return crate::loading::Loading::new("launcher-loading", t!("ui.launcher.loading").into_owned())
                .top()
                .into_any_element();
        }
        let account = self.render_account(cx);
        let toolbar = h_flex()
            .gap_2()
            .items_center()
            .child(Button::new("launcher-rescan").label(t!("ui.launcher.rescan").to_string()).compact().on_click(
                cx.listener(|this, _, _, cx| {
                    this.op(cx, |launcher, settings, env| {
                        launcher.start_install_discovery(settings, env);
                        false
                    })
                }),
            ))
            .child(
                Button::new("launcher-add-install")
                    .label(t!("ui.launcher.add_install").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _, window, cx| this.add_install(window, cx))),
            )
            .when(self.launcher.is_discovering(), |this| this.child(Spinner::new().xsmall()))
            .child(div().flex_1())
            .child(
                Checkbox::new("launcher-safe-mode")
                    .label(t!("ui.launcher.safe_mode").to_string())
                    .checked(self.safe_mode)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.safe_mode = *checked;
                        cx.notify();
                    })),
            );
        let options = self.render_options(cx);
        let installs = self.launcher.installs().to_vec();
        let rows: Vec<AnyElement> = if installs.is_empty() && !self.launcher.is_discovering() {
            vec![dim(t!("ui.launcher.no_installs")).into_any_element()]
        } else {
            installs.iter().enumerate().map(|(i, install)| self.render_install(i, install, cx)).collect()
        };
        let installs = section(t!("ui.launcher.installs_heading"), v_flex().gap_2().child(toolbar).children(rows), cx);
        let install_game = self.render_install_game(cx);

        div()
            .id("launcher-tab")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(v_flex().gap_6().p_4().child(account).child(installs).child(options).child(install_game))
            .into_any_element()
    }
}

fn dim(text: impl Into<SharedString>) -> Div {
    div().text_sm().text_color(crate::theme::text_dim()).child(text.into())
}

fn colored(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_sm().text_color(color).child(text.into())
}

fn card(body: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .w_full()
        .rounded(SECTION_RADIUS)
        .border_1()
        .border_color(cx.theme().border)
        .bg(crate::theme::surface())
        .p_3()
        .child(body)
        .into_any_element()
}

fn section(title: impl Into<SharedString>, body: impl IntoElement, cx: &App) -> AnyElement {
    v_flex()
        .gap_2()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(title.into()))
        .child(card(body, cx))
        .into_any_element()
}

fn catalog_status(state: Option<&CatalogCheckState>, cx: &App) -> AnyElement {
    match state {
        Some(CatalogCheckState::Checking) => h_flex()
            .gap_1()
            .child(Spinner::new().xsmall())
            .child(dim(t!("ui.launcher.checking_updates")))
            .into_any_element(),
        Some(CatalogCheckState::Ready(build)) => {
            let summary = CatalogSummary::of(build);
            let headline = if summary.updates.is_empty() {
                div().text_sm().child(t!("ui.launcher.up_to_date").to_string())
            } else {
                colored(t!("ui.launcher.update_available", parts = summary.updates.join(", ")), cx.theme().warning)
            };
            v_flex()
                .child(headline)
                .when(!summary.not_installed.is_empty(), |this| {
                    this.child(dim(t!("ui.launcher.not_installed", parts = summary.not_installed.join(", "))))
                })
                .into_any_element()
        }
        Some(CatalogCheckState::Error(detail)) => {
            dim(t!("ui.launcher.check_failed", error = detail)).into_any_element()
        }
        Some(CatalogCheckState::NoUpdateHost) | None => div().into_any_element(),
    }
}

/// A download's progress and its controls, or how it ended. `slot` tells the
/// row's controls apart from every other row's.
fn download_status(
    slot: usize,
    key: &Path,
    state: Option<&DownloadState>,
    cx: &mut Context<LauncherView>,
) -> Option<AnyElement> {
    let key = key.to_path_buf();
    let element = match state? {
        DownloadState::Running { phase, done_bytes, total_bytes, current } => {
            let (pause_key, cancel_key) = (key.clone(), key);
            let meter = if *total_bytes > 0 {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().w(PROGRESS_WIDTH).child(
                            Progress::new(("launcher-progress", slot))
                                .value((*done_bytes as f32 / *total_bytes as f32 * 100.).clamp(0., 100.)),
                        ),
                    )
                    .child(div().text_sm().child(format!(
                        "{} / {}",
                        humansize::format_size(*done_bytes, humansize::BINARY),
                        humansize::format_size(*total_bytes, humansize::BINARY)
                    )))
                    .into_any_element()
            } else {
                Spinner::new().xsmall().into_any_element()
            };
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(meter)
                        .child(
                            Button::new(("launcher-pause", slot))
                                .label(t!("ui.launcher.pause").to_string())
                                .compact()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, _| this.launcher.pause_download(&pause_key))),
                        )
                        .child(
                            Button::new(("launcher-cancel", slot))
                                .label(t!("ui.launcher.cancel").to_string())
                                .compact()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, _| this.launcher.cancel_download(&cancel_key))),
                        ),
                )
                .child(div().text_sm().truncate().child(format!("{phase}: {current}")))
                .into_any_element()
        }
        DownloadState::Paused => h_flex()
            .gap_2()
            .items_center()
            .child(dim(t!("ui.launcher.paused")))
            .child(
                Button::new(("launcher-resume", slot))
                    .label(t!("ui.launcher.resume").to_string())
                    .compact()
                    .on_click(cx.listener(move |this, _, window, cx| this.resume(key.clone(), window, cx))),
            )
            .into_any_element(),
        DownloadState::Done { skipped_deltas: 0 } => {
            colored(t!("ui.launcher.installed"), cx.theme().success).into_any_element()
        }
        DownloadState::Done { skipped_deltas } => {
            colored(t!("ui.launcher.installed_skipped", count = skipped_deltas), cx.theme().warning).into_any_element()
        }
        DownloadState::Failed(detail) => {
            colored(t!("ui.launcher.download_failed", error = detail), cx.theme().danger).into_any_element()
        }
        DownloadState::Cancelled => dim(t!("ui.launcher.cancelled")).into_any_element(),
    };
    Some(element)
}

//! The control each setting is changed with, and the line under it that reports on it.
//!
//! A control writes through `Services::settings`, which changes the snapshot every view reads and
//! saves the row the egui and GPUI apps read, so a change takes effect at once and holds in all
//! three. There is no apply step to forget.

use landaire_ui::TABULAR_CLASS;
use rust_i18n::t;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::settings_drawer::EXPORT_FORMATS;
use zgui::component;
use zgui::css;
use zgui::platform::ColorScheme;
use zgui::prelude::try_use_window;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::use_local_context;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_detached;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::focused_node;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::SelectContentProps;
use zgui_ui::SelectItemProps;
use zgui_ui::SelectProps;
use zgui_ui::SelectTriggerProps;
use zgui_ui::SelectTriggerSize;
use zgui_ui::SelectValueProps;
use zgui_ui::SliderProps;
use zgui_ui::SpinnerProps;
use zgui_ui::SwitchProps;
use zgui_ui::SwitchSize;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui::prelude::Binding;

use crate::desktop;
use crate::i18n::LanguageCode;
use crate::icons::AppIconProps;
use crate::shell::Shell;
use crate::shell::settings::Setting;
use crate::shell::settings::cache::CachedBuildsProps;
use crate::shell::settings::zoom_anchor::ZoomAnchor;
use crate::shell::twitch::TwitchPaste;
use crate::shell::twitch::TwitchStatus;
use crate::theme::palette_choice;

const SHEET: &str = "wt-settings-controls";

const CSS: &str = css!(
    ".wt-settings__control-row {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
    }
    .wt-settings__control-stack {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .wt-settings__choices {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        gap: 4px;
    }
    .wt-settings__choice[data-selected] {
        background-color: var(--lui-color-selection);
        border-color: var(--lui-color-accent);
        color: var(--lui-color-text-bright);
    }
    .wt-settings__language { width: 170px; }
    .wt-settings__field { width: 100%; }
    .wt-settings__path-tip {
        display: flex;
        flex: 1 1 160px;
        min-width: 160px;
    }
    .wt-settings__path {
        position: relative;
        flex: 1 1 auto;
        min-width: 0;
    }
    .wt-settings__path[data-shortened] .zui-input { color: transparent; }
    .wt-settings__path-shown {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        display: flex;
        flex-direction: row;
        align-items: center;
        padding: 0 9px;
        pointer-events: none;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-size: 13px;
    }
    .wt-settings__path[data-invalid] .wt-settings__path-shown,
    .wt-settings__path[data-invalid] .zui-input { color: var(--wt-color-error); }
    .wt-settings__themes {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        gap: 8px;
    }
    .wt-settings__theme {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 4px;
        width: 116px;
        height: 40px;
        padding: 4px 8px;
    }
    .wt-settings__theme-name {
        font-size: 11px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 100%;
    }
    .wt-settings__swatches {
        display: flex;
        flex-direction: row;
        gap: 4px;
    }
    .wt-settings__swatch {
        width: 20px;
        height: 9px;
        border-radius: 1px;
    }
    .wt-settings__zoom-slider {
        width: 160px;
        padding: 0 8px;
    }
    .wt-settings__zoom-value {
        flex: none;
        min-width: 2.5em;
        font-size: 13px;
    }
    .wt-settings__line {
        min-width: 0;
        font-size: 13px;
    }
    .wt-settings__line--dim {
        font-size: 11px;
        color: var(--lui-color-text-dim);
    }
    .wt-settings__clip[data-error] { color: var(--wt-color-error); }
    .wt-settings__selectable { user-select: text; }
    .wt-settings__clip {
        flex: 1 1 0;
        min-width: 0;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-size: 13px;
    }
    .wt-settings__twitch-status[data-tone=\"ok\"] { color: var(--wt-color-ok); }
    .wt-settings__twitch-status[data-tone=\"warn\"] { color: var(--wt-color-warn); }
    .wt-settings__twitch-status[data-tone=\"dim\"] { color: var(--lui-color-text-dim); }
    .wt-settings__twitch-status[data-tone=\"error\"] { color: var(--wt-color-error); }
    .wt-settings__note {
        font-size: 11px;
        color: var(--lui-color-text-dim);
    }
    .wt-settings__note[data-error] { color: var(--wt-color-error); }
    .wt-settings__row-control .zui-spinner {
        width: 14px;
        height: 14px;
    }"
);

/// Chatterino's login page, which is what the egui app opens: it mints a chat-scoped token, so the
/// toolkit does not host a login of its own.
const TWITCH_TOKEN_URL: &str = "https://chatterino.com/client_login";

/// The control a setting is changed with, beside or under its name. Nothing for a setting that is
/// only reported on.
#[component]
pub fn SettingControl(shell: Shell, setting: Setting) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let control = match setting {
        Setting::Language => AnyView::new(view! { LanguageSelect(shell = shell) }),
        Setting::DataSharing => AnyView::new(view! { DataSharingChoices(shell = shell) }),
        Setting::CodeIntegrity => code_integrity_choices(shell),
        Setting::Proxy => AnyView::new(view! {
            CommittedField(
                shell = shell,
                setting = setting,
                test_id = "proxy-url",
                placeholder = "http://host:port".to_owned(),
                read = |settings: &StartupSettings| settings.proxy_url.clone(),
                commit = UnsyncCallback::new(move |url: String| {
                    let unchanged = shell.services().settings.read_untracked(|settings| settings.proxy_url == url);
                    if unchanged == Some(false) {
                        shell.services().settings.edit(keys::PROXY_URL, |settings| {
                            settings.proxy_url = url.clone();
                            url
                        });
                    }
                })
            )
        }),
        Setting::CheckForUpdates => setting_switch(
            shell,
            setting,
            "check-for-updates",
            |settings| settings.check_for_updates,
            |shell, on| {
                shell.services().settings.edit(keys::CHECK_FOR_UPDATES, |settings| {
                    settings.check_for_updates = on;
                    on
                });
            },
        ),
        Setting::EnableLogging => setting_switch(
            shell,
            setting,
            "enable-logging",
            |settings| settings.enable_logging,
            |shell, on| {
                shell.services().settings.edit(keys::ENABLE_LOGGING, |settings| {
                    settings.enable_logging = on;
                    on
                });
            },
        ),
        Setting::DataFolder => AnyView::new(view! {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "open-data-dir",
                on:click = move |_| desktop::open_data_directory()
            ) {
                AppIcon(glyph = Glyph::FolderOpen)
                text {{t!("ui.settings.wows.cache.open_folder").into_owned()}}
            }
        }),
        Setting::Theme => AnyView::new(view! { ThemeTiles(shell = shell) }),
        Setting::Zoom => AnyView::new(view! { ZoomRow(shell = shell) }),
        Setting::WowsDirectory => AnyView::new(view! { WowsDirectory(shell = shell) }),
        Setting::AutoDump => setting_switch(
            shell,
            setting,
            "cache-auto-dump",
            |settings| settings.auto_dump_game_data,
            |shell, on| {
                shell.services().settings.edit(keys::AUTO_DUMP_GAME_DATA, |settings| {
                    settings.auto_dump_game_data = on;
                    on
                });
            },
        ),
        Setting::CacheDirectory => AnyView::new(view! { CacheDirectory(shell = shell) }),
        Setting::CachedBuilds => AnyView::new(view! { CachedBuilds(shell = shell) }),
        Setting::ReplayIndex => AnyView::new(view! { ReplayIndexControl(shell = shell) }),
        Setting::CurrentReplayPath => {
            let settings = shell.services().settings;
            let shown = move || {
                let path = settings.read(|settings| settings.current_replay_path.display().to_string());
                match path {
                    Some(path) if !path.is_empty() => path,
                    _ => t!("ui.settings.not_set").into_owned(),
                }
            };
            AnyView::new(view! {
                text(class = "wt-settings__line wt-settings__selectable", attr:data-testid = "settings-current-replay-path") {{shown}}
            })
        }
        Setting::ShowRawXp => replay_switch(
            shell,
            setting,
            "show-raw-xp",
            |replay| replay.show_raw_xp,
            |replay, on| {
                replay.show_raw_xp = on;
            },
        ),
        Setting::ShowObservedDamage => replay_switch(
            shell,
            setting,
            "show-observed-damage",
            |replay| replay.show_observed_damage,
            |replay, on| replay.show_observed_damage = on,
        ),
        Setting::ShowEntityId => replay_switch(
            shell,
            setting,
            "show-entity-id",
            |replay| replay.show_entity_id,
            |replay, on| replay.show_entity_id = on,
        ),
        Setting::ShowHeals => replay_switch(
            shell,
            setting,
            "show-heals",
            |replay| replay.show_heals,
            |replay, on| {
                replay.show_heals = on;
            },
        ),
        Setting::ReplayPreviews => replay_switch(
            shell,
            setting,
            "enable-replay-previews",
            |replay| replay.enable_replay_previews,
            |replay, on| replay.enable_replay_previews = on,
        ),
        Setting::AutoExport => AnyView::new(view! { AutoExport(shell = shell) }),
        Setting::DisplayName => AnyView::new(view! {
            CommittedField(
                shell = shell,
                setting = setting,
                test_id = "session-display-name",
                placeholder = t!("ui.collab.display_name_hint").into_owned(),
                read = |settings: &StartupSettings| settings.collab_display_name.clone(),
                commit = UnsyncCallback::new(move |name: String| {
                    let unchanged =
                        shell.services().settings.read_untracked(|settings| settings.collab_display_name == name);
                    if unchanged == Some(false) {
                        shell.services().settings.edit(keys::COLLAB_DISPLAY_NAME, |settings| {
                            settings.collab_display_name = name.clone();
                            name
                        });
                    }
                })
            )
        }),
        Setting::SuppressIpWarning => setting_switch(
            shell,
            setting,
            "session-suppress-ip-warning",
            |settings| settings.suppress_p2p_ip_warning,
            |shell, on| {
                shell.services().settings.edit(keys::SUPPRESS_P2P_IP_WARNING, |settings| {
                    settings.suppress_p2p_ip_warning = on;
                    on
                });
            },
        ),
        Setting::DisableAutoOpen => setting_switch(
            shell,
            setting,
            "session-disable-auto-open",
            |settings| settings.disable_auto_open_session_windows,
            |shell, on| {
                shell.services().settings.edit(keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS, |settings| {
                    settings.disable_auto_open_session_windows = on;
                    on
                });
            },
        ),
        Setting::TwitchGetToken => AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "twitch-get-token",
                        a11y:label = setting.title().into_owned(),
                        on:click = move |_| desktop::open_url(TWITCH_TOKEN_URL)
                    ) {
                        AppIcon(glyph = Glyph::AppWindow)
                    }
                }
                TooltipContent {{t!("ui.settings.twitch.get_token_tooltip").into_owned()}}
            }
        }),
        Setting::TwitchToken => AnyView::new(view! { TwitchTokenButton(shell = shell) }),
        Setting::TwitchChannel => AnyView::new(view! {
            CommittedField(
                shell = shell,
                setting = setting,
                test_id = "twitch-channel",
                placeholder = t!("ui.settings.twitch.monitored_channel").into_owned(),
                read = |settings: &StartupSettings| settings.twitch_channel.clone(),
                commit = UnsyncCallback::new(move |channel: String| shell.set_twitch_channel(channel))
            )
        }),
        // The viewport writes these itself when a pane changes, so the drawer reports them rather
        // than offering a second way to set them.
        Setting::ArmorPlateEdges => {
            armor_switch(shell, setting, "armor-show-plate-edges", |defaults| defaults.show_plate_edges)
        }
        Setting::ArmorOpaqueHull => armor_switch(shell, setting, "armor-hull-opaque", |defaults| defaults.hull_opaque),
        Setting::ArmorNoDefaults => return AnyView::new(()),
    };

    AnyView::new(view! {
        box(class = "wt-settings__row-control") {
            {control}
        }
    })
}

/// What is reported under a setting.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    text: String,
    error: bool,
}

impl Note {
    fn dim(text: String) -> Self {
        Self { text, error: false }
    }

    fn error(text: String) -> Self {
        Self { text, error: true }
    }
}

/// The line under a setting that reports on it, when there is something to report.
#[component]
pub fn SettingNote(shell: Shell, setting: Setting) -> impl IntoView {
    let services = shell.services();
    let locale = services.locale;
    let note = move || -> Option<Note> {
        locale.track();
        match setting {
            Setting::DataSharing => {
                services.settings.read(|settings| Note::dim(settings.data_sharing.description().to_owned()))
            }
            Setting::CodeIntegrity => Some(Note::dim(t!("ui.settings.app.code_integrity_tooltip").into_owned())),
            Setting::Zoom => Some(Note::dim(crate::zoom::shortcuts_hint())),
            Setting::WowsDirectory => shell
                .settings_drawer
                .wows_dir_invalid
                .get()
                .then(|| Note::error(t!("ui.messages.wows_dir_invalid").into_owned())),
            Setting::TwitchToken => match shell.twitch.paste()? {
                TwitchPaste::Stored { who } => {
                    Some(Note::dim(t!("ui.settings.twitch.signed_in_as", who = who).into_owned()))
                }
                TwitchPaste::Refused { reason } => Some(Note::error(reason)),
            },
            _ => None,
        }
    };

    view! {
        {move || note().map(|note| view! {
            text(
                class = "wt-settings__note",
                attr:data-testid = "settings-row-note",
                attr:data-error = note.error.then(String::new)
            ) {{note.text.clone()}}
        }.into_view())}
    }
}

/// A switch for an on/off setting, named for a reader by the setting's title, which is the label
/// beside it.
fn setting_switch(
    shell: Shell,
    setting: Setting,
    test_id: &'static str,
    read: fn(&StartupSettings) -> bool,
    write: fn(Shell, bool),
) -> AnyView {
    let settings = shell.services().settings;
    // The drawer lists the settings only once they are loaded, so `None` is never read here.
    let on = Signal::derive_local(move || settings.read(read) == Some(true));

    AnyView::new(view! {
        Switch(
            checked = Binding::controlled(on, move |on: bool| write(shell, on)),
            size = SwitchSize::Sm,
            attr:data-testid = test_id,
            a11y:label = setting.title().into_owned()
        )
    })
}

/// A switch over one field of the replay settings, which are stored as one row.
fn replay_switch(
    shell: Shell,
    setting: Setting,
    test_id: &'static str,
    read: fn(&ReplaySettings) -> bool,
    write: fn(&mut ReplaySettings, bool),
) -> AnyView {
    let settings = shell.services().settings;
    let on = Signal::derive_local(move || settings.read(|settings| read(&settings.replay)) == Some(true));

    AnyView::new(view! {
        Switch(
            checked = Binding::controlled(on, move |on: bool| edit_replay(shell, |replay| write(replay, on))),
            size = SwitchSize::Sm,
            attr:data-testid = test_id,
            a11y:label = setting.title().into_owned()
        )
    })
}

/// A switch that reports a saved armor viewer default and cannot be moved here.
fn armor_switch(
    shell: Shell,
    setting: Setting,
    test_id: &'static str,
    read: fn(&wows_toolkit_config::queries::ArmorViewerDefaultsRow) -> bool,
) -> AnyView {
    let settings = shell.services().settings;
    let on = Signal::derive_local(move || {
        settings.read(|settings| settings.armor_defaults.as_ref().map(read)).flatten() == Some(true)
    });

    AnyView::new(view! {
        Switch(
            checked = Binding::controlled(on, |_: bool| {}),
            size = SwitchSize::Sm,
            disabled = true,
            attr:data-testid = test_id,
            a11y:label = setting.title().into_owned()
        )
    })
}

/// Rewrites the replay settings, which are stored as one row.
fn edit_replay(shell: Shell, apply: impl FnOnce(&mut ReplaySettings)) {
    shell.services().settings.edit(keys::REPLAY_SETTINGS, |settings| {
        apply(&mut settings.replay);
        settings.replay.clone()
    });
}

/// The language the interface reads in.
#[component]
fn LanguageSelect(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    // No language chosen reads as English.
    let chosen = Signal::derive_local(move || {
        settings.read(|settings| settings.locale.clone()).flatten().unwrap_or_else(|| "en".to_owned())
    });

    view! {
        Select(value = Binding::controlled(chosen, move |code: String| shell.set_locale(LanguageCode::new(code)))) {
            SelectTrigger(
                size = SelectTriggerSize::Sm,
                label = Setting::Language.title().into_owned(),
                class = "wt-settings__language",
                attr:data-testid = "settings-language"
            ) {
                SelectValue()
            }
            SelectContent {
                for language in move || wt_translations::SUPPORTED_LANGUAGES.iter().collect::<Vec<_>>(), key = |language: &&wt_translations::LanguageInfo| language.code {
                    SelectItem(value = language.code, text = language.native_name) {
                        {language.native_name}
                    }
                }
            }
        }
    }
}

/// What may be shared, as three choices.
#[component]
fn DataSharingChoices(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    let chosen = move || settings.read(|settings| settings.data_sharing);

    view! {
        row(class = "wt-settings__choices") {
            for mode in move || DataSharingMode::ALL.to_vec(), key = |mode: &DataSharingMode| *mode as usize {
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            class = "wt-settings__choice",
                            attr:data-testid = format!("data-sharing-{mode:?}"),
                            attr:data-selected = move || (chosen() == Some(mode)).then(String::new),
                            a11y:selected = move || chosen() == Some(mode),
                            on:click = move |_| shell.adopt_data_sharing(mode)
                        ) {
                            {mode.label()}
                        }
                    }
                    TooltipContent {{mode.description()}}
                }
            }
        }
    }
}

/// Whether unsigned code may load into this process, which is the one mitigation with a
/// compatibility cost worth a choice.
///
/// Windows only: the policy does not exist elsewhere, and a control for one that cannot be applied
/// would be a control that does nothing. The change lands at the next launch, because the policy
/// cannot be lifted once a process has started, which is what the note says.
#[cfg(windows)]
fn code_integrity_choices(shell: Shell) -> AnyView {
    use wows_toolkit_hardening::CodeIntegrityPreference;

    let settings = shell.services().settings;
    let chosen = move || settings.read(|settings| settings.code_integrity);

    AnyView::new(view! {
        row(class = "wt-settings__choices") {
            for choice in move || CodeIntegrityPreference::ALL.to_vec(), key = |choice: &CodeIntegrityPreference| *choice as usize {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    class = "wt-settings__choice",
                    attr:data-testid = format!("code-integrity-{choice:?}"),
                    attr:data-selected = move || (chosen() == Some(choice)).then(String::new),
                    a11y:selected = move || chosen() == Some(choice),
                    on:click = move |_| {
                        shell.services().settings.edit(keys::CODE_INTEGRITY, |settings| {
                            settings.code_integrity = choice;
                            choice
                        });
                    }
                ) {
                    {t!(choice.key()).into_owned()}
                }
            }
        }
    })
}

/// Nothing, where the policy does not exist; the row is not listed there either.
#[cfg(not(windows))]
fn code_integrity_choices(_shell: Shell) -> AnyView {
    AnyView::new(())
}

/// A text field whose value is adopted when it is left or Enter is pressed, so a half-typed value
/// never reaches the database.
#[component]
fn CommittedField(
    shell: Shell,
    setting: Setting,
    test_id: &'static str,
    #[prop(into)] placeholder: String,
    read: fn(&StartupSettings) -> String,
    commit: UnsyncCallback<String>,
) -> impl IntoView {
    let value = RwSignal::new_local(shell.services().settings.read_untracked(read).unwrap_or_default());
    let adopt = move || commit.run(value.get_untracked().trim().to_owned());

    view! {
        Input(
            value = value,
            placeholder = placeholder,
            label = setting.title().into_owned(),
            class = "wt-settings__field",
            attr:data-testid = test_id,
            on:key_down = move |ev| {
                if ev.key == Key::Named(NamedKey::Enter) {
                    adopt();
                }
            },
            on:focus_out = move |_| adopt()
        )
    }
}

/// A field for a directory. While it is not focused the path is drawn over it cut short, with the
/// whole path in a tooltip; the field stays under the drawing, its own text hidden rather than the
/// field, so a click still places the caret and Tab still reaches it.
#[component]
fn PathField(
    setting: Setting,
    test_id: &'static str,
    value: RwSignal<String, LocalStorage>,
    #[prop(into)] placeholder: String,
    #[prop(into)] invalid: Signal<bool, LocalStorage>,
    commit: UnsyncCallback<String>,
) -> impl IntoView {
    let field = NodeRef::new();
    let focused = focused_node();
    let editing = move || focused.get().is_some_and(|node| field.get() == Some(node));
    let adopt = move || commit.run(value.get_untracked().trim().to_owned());
    let placeholder_tip = placeholder.clone();
    let shortened = move || !editing() && !value.with(String::is_empty);
    let whole = move || {
        let path = value.get();
        if path.is_empty() { placeholder_tip.clone() } else { path }
    };

    view! {
        Tooltip {
            TooltipTrigger(class = "wt-settings__path-tip") {
                box(
                    class = "wt-settings__path",
                    attr:data-shortened = move || shortened().then(String::new),
                    attr:data-invalid = move || invalid.get().then(String::new)
                ) {
                    Input(
                        value = value,
                        placeholder = placeholder,
                        label = setting.title().into_owned(),
                        node_ref = field,
                        class = "wt-settings__field",
                        attr:data-testid = test_id,
                        on:key_down = move |ev| {
                            if ev.key == Key::Named(NamedKey::Enter) {
                                adopt();
                            }
                        },
                        on:focus_out = move |_| adopt()
                    )
                    // An empty field shows its own placeholder, which the overlay would draw a
                    // second time.
                    if move || !editing() && !value.with(String::is_empty) {
                        text(
                            class = "wt-settings__path-shown",
                            attr:data-testid = format!("settings-path-{setting:?}"),
                            a11y:hidden = true
                        ) {{move || value.get()}}
                    } else {}
                }
            }
            TooltipContent {{whole.clone()}}
        }
    }
}

/// The game directory, typed or browsed for.
#[component]
fn WowsDirectory(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    let value = RwSignal::new_local(settings.read_untracked(|settings| settings.wows_dir.clone()).unwrap_or_default());
    let commit = UnsyncCallback::new(move |path: String| shell.apply_wows_dir(path));
    let browse = move |_: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
        let asked = wows_toolkit_services::dialog::pick_folder("World of Warships directory");
        spawn_detached(async move {
            let Some(picked) = shell.chosen_path(asked).await else { return };
            if shell.is_unmounted() {
                return;
            }
            let path = picked.to_string_lossy().into_owned();
            value.try_set(path.clone());
            shell.apply_wows_dir(path);
        });
    };

    view! {
        row(class = "wt-settings__control-row") {
            PathField(
                setting = Setting::WowsDirectory,
                test_id = "wows-dir",
                value = value,
                placeholder = t!("ui.settings.wows.directory_hint").into_owned(),
                invalid = Signal::derive_local(move || shell.settings_drawer.wows_dir_invalid.get()),
                commit = commit
            )
            Button(variant = ButtonVariant::Outline, size = ButtonSize::Sm, attr:data-testid = "wows-dir-browse", on:click = browse) {
                AppIcon(glyph = Glyph::FolderOpen)
                text {{t!("ui.settings.wows.browse").into_owned()}}
            }
        }
    }
}

/// Where the game-data cache is kept. Empty shows the default location as its placeholder, which
/// is what an empty setting means.
#[component]
fn CacheDirectory(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    let value = RwSignal::new_local(
        settings.read_untracked(|settings| settings.game_data_cache_dir.clone()).unwrap_or_default(),
    );
    let placeholder =
        wows_toolkit_config::game_data_dump_base().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
    // The measurement and the findings belong to the old directory, so they are dropped rather
    // than shown against the new one.
    let adopt = move |dir: String| {
        let unchanged = settings.read_untracked(|settings| settings.game_data_cache_dir == dir);
        if unchanged != Some(false) {
            return;
        }
        settings.edit(keys::GAME_DATA_CACHE_DIR, |settings| {
            settings.game_data_cache_dir = dir.clone();
            dir
        });
        shell.cache.forget_findings();
    };
    let commit = UnsyncCallback::new(adopt);
    let browse = move |_: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
        let asked = wows_toolkit_services::dialog::pick_folder("Game data cache directory");
        spawn_detached(async move {
            let Some(picked) = shell.chosen_path(asked).await else { return };
            if shell.is_unmounted() {
                return;
            }
            let path = picked.to_string_lossy().into_owned();
            value.try_set(path.clone());
            adopt(path);
        });
    };

    view! {
        row(class = "wt-settings__control-row") {
            PathField(
                setting = Setting::CacheDirectory,
                test_id = "cache-dir",
                value = value,
                placeholder = placeholder,
                invalid = false,
                commit = commit
            )
            Button(variant = ButtonVariant::Outline, size = ButtonSize::Sm, attr:data-testid = "cache-dir-browse", on:click = browse) {
                AppIcon(glyph = Glyph::FolderOpen)
                text {{t!("ui.settings.wows.cache.browse").into_owned()}}
            }
        }
    }
}

/// The palettes, three tiles to a row. A tile under the pointer previews its palette on the whole
/// window until the pointer leaves it.
#[component]
fn ThemeTiles(shell: Shell) -> impl IntoView {
    let appearance = shell.appearance;
    let settings = shell.services().settings;
    let chosen = move || settings.read(|settings| settings.theme);
    let desktop = try_use_window().map(|window| window.color_scheme());
    let desktop_appearance = move || match desktop.as_ref().map(|scheme| scheme.get()) {
        Some(ColorScheme::Dark) => landaire_ui::Appearance::Dark,
        _ => landaire_ui::Appearance::Light,
    };

    // A tile the search takes away, or the drawer closing over, never reports the pointer leaving.
    on_cleanup_local(move || {
        if appearance.theme_preview.try_get_untracked().flatten().is_some() {
            appearance.theme_preview.set(None);
        }
    });

    view! {
        row(class = "wt-settings__themes") {
            for choice in move || ThemeChoice::ALL.to_vec(), key = |choice: &ThemeChoice| *choice as usize {
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            class = "wt-settings__choice wt-settings__theme",
                            attr:data-testid = format!("theme-choice-{choice:?}"),
                            attr:data-selected = move || (chosen() == Some(choice)).then(String::new),
                            a11y:selected = move || chosen() == Some(choice),
                            a11y:label = choice.label(),
                            // On a pointer move rather than on entry: a zoom change rescales the
                            // page under a still pointer, and a tile that lands under it was not
                            // pointed at.
                            on:pointer_move = move |_| {
                                if appearance.theme_preview.get_untracked() != Some(choice) {
                                    appearance.theme_preview.set(Some(choice));
                                }
                            },
                            on:pointer_leave = move |_| {
                                if appearance.theme_preview.get_untracked() == Some(choice) {
                                    appearance.theme_preview.set(None);
                                }
                            },
                            on:click = move |_| shell.set_theme(choice)
                        ) {
                            text(class = "wt-settings__theme-name") {{choice.label()}}
                            row(class = "wt-settings__swatches") {
                                for swatch in move || swatches(choice, desktop_appearance()).into_iter().enumerate().collect::<Vec<_>>(), key = |swatch: &(usize, landaire_ui::Rgb)| swatch.0 {
                                    box(class = "wt-settings__swatch", style:background-color = swatch.1.to_string())
                                }
                            }
                        }
                    }
                    TooltipContent {{move || theme_tip(choice, desktop_appearance())}}
                }
            }
        }
    }
}

/// The surface, panel, text and accent of the palette `choice` draws with on `desktop`.
fn swatches(choice: ThemeChoice, desktop: landaire_ui::Appearance) -> [landaire_ui::Rgb; 4] {
    let palette = palette_choice(choice).palette(desktop);
    [palette.surface, palette.panel, palette.text, palette.accent]
}

/// The palette's name and its four colours, for the tile's tooltip.
fn theme_tip(choice: ThemeChoice, desktop: landaire_ui::Appearance) -> String {
    let colors = swatches(choice, desktop).iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");

    format!("{}  {colors}", choice.label())
}

/// The zoom slider, the value it shows and its reset.
///
/// A drag applies when it ends rather than as it moves. Applying rescales the whole window, this
/// slider included, so the track would move under a pointer that has not, and every step would
/// reflow the page under it. A key applies at once: there is no drag to wait out.
#[component]
fn ZoomRow(shell: Shell) -> impl IntoView {
    let anchor = use_local_context::<ZoomAnchor>().expect("the zoom row is inside the settings list");
    let zoom = shell.appearance.zoom;
    let shown = RwSignal::new_local(f64::from(zoom.get_untracked().get()));
    let dragging = RwSignal::new_local(false);

    // A zoom set elsewhere, by a shortcut or the reset, moves the slider.
    let following = RenderEffect::new(move |_| {
        let now = f64::from(zoom.get().get());
        if !dragging.get_untracked() {
            shown.set(now);
        }
    });
    on_cleanup_local(move || drop(following));

    let apply = move || {
        let wanted = ZoomFactor::new(shown.get_untracked() as f32);
        if wanted != zoom.get_untracked() {
            shell.set_zoom(wanted);
        }
    };

    view! {
        row(class = "wt-settings__control-row") {
            box(class = "wt-settings__zoom-slider", node_ref = anchor.slider, attr:data-testid = "zoom-slider") {
                Slider(
                    value = shown,
                    min = f64::from(ZoomFactor::MIN.get()),
                    max = f64::from(ZoomFactor::MAX.get()),
                    step = 0.05,
                    label = Setting::Zoom.title().into_owned(),
                    attr:data-testid = "zoom-slider-control",
                    on:pointer_down = move |_| dragging.set(true),
                    on:pointer_up = move |_| {
                        dragging.set(false);
                        apply();
                    },
                    on:pointer_cancel = move |_| {
                        dragging.set(false);
                        apply();
                    },
                    // After the slider's own handler has moved the value.
                    on:key_down = move |_| apply()
                )
            }
            // Wide enough for "2.00" at any zoom, so the reset button holds still.
            text(class = "wt-settings__zoom-value", class = TABULAR_CLASS, attr:data-testid = "zoom-value") {{
                move || format!("{:.2}", shown.get())
            }}
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "reset-zoom",
                on:click = move |_| shell.set_zoom(ZoomFactor::DEFAULT)
            ) {
                {t!("ui.buttons.reset").into_owned()}
            }
        }
    }
}

/// Building the replay index, and how far a build has got. Refused without a game directory, since
/// the replays are under it.
#[component]
fn ReplayIndexControl(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    let index = shell.replay_index;
    let has_dir = move || settings.read(|settings| !settings.wows_dir.is_empty()) == Some(true);
    let refused = Signal::derive_local(move || !has_dir() || index.is_running());
    let progress = move || {
        index.progress().map(|step| {
            if step.skipped > 0 {
                t!(
                    "ui.settings.index.progress_skipped",
                    done = step.done,
                    total = step.total,
                    skipped = step.skipped,
                    failed = step.failed
                )
                .into_owned()
            } else {
                t!("ui.settings.index.progress", done = step.done, total = step.total, failed = step.failed)
                    .into_owned()
            }
        })
    };

    view! {
        column(class = "wt-settings__control-stack") {
            row(class = "wt-settings__control-row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = refused,
                    attr:data-testid = "index-build",
                    on:click = move |_| shell.build_replay_index(IndexMode::FillGaps)
                ) {
                    {t!("ui.settings.index.build").into_owned()}
                }
                // For rows an older parse decoded through constants that did not fit: nothing else
                // can correct them (the egui app's Re-index All).
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            disabled = refused,
                            attr:data-testid = "index-rebuild",
                            on:click = move |_| shell.build_replay_index(IndexMode::RefreshAll)
                        ) {
                            {t!("ui.settings.index.rebuild").into_owned()}
                        }
                    }
                    TooltipContent {{t!("ui.settings.index.rebuild_hint").into_owned()}}
                }
                if move || index.is_running() {
                    Spinner(label = t!("ui.app.status_indexing").into_owned())
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        attr:data-testid = "index-stop",
                        on:click = move |_| index.stop()
                    ) {
                        {t!("ui.buttons.stop").into_owned()}
                    }
                } else {}
            }
            {move || progress().map(|said| view! {
                text(class = "wt-settings__line wt-settings__line--dim", class = TABULAR_CLASS) {{said}}
            }.into_view())}
            {move || index.outcome().map(|said| view! {
                text(class = "wt-settings__line wt-settings__line--dim") {{said}}
            }.into_view())}
        }
    }
}

/// Whether to write a file per battle, in which format, and where.
///
/// The directory is shown rather than typed: a path typed into a field is only checked when it is
/// left, and one that does not exist reads as working until the first battle fails to write.
#[component]
fn AutoExport(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    let enabled =
        Signal::derive_local(move || settings.read(|settings| settings.replay.auto_export_data) == Some(true));
    let chosen = move || settings.read(|settings| settings.replay.auto_export_format);
    let directory = move || settings.read(|settings| settings.replay.auto_export_path.clone()).unwrap_or_default();
    // A directory that is not there cannot be written to, and the reader has no other way to find
    // that out until a battle is lost. Looked for off the UI thread, since a directory on an
    // offline share can take many seconds to answer.
    let missing = RwSignal::new_local(false);
    let looking = StoredValue::new_local(None::<Task>);
    let checking = RenderEffect::new(move |_| {
        let directory = directory();
        if let Some(Some(earlier)) = looking.try_update_value(Option::take) {
            earlier.cancel();
        }
        // Unmarked until the new directory has been looked for.
        missing.set(false);
        if directory.is_empty() {
            return;
        }

        let task = spawn_local(async move {
            let present = blocking(move || std::path::Path::new(&directory).is_dir()).await;
            missing.try_set(!present);
        });
        looking.set_value(Some(task));
    });
    on_cleanup_local(move || drop(checking));

    let shown = move || {
        let directory = directory();
        if directory.is_empty() { t!("ui.settings.not_set").into_owned() } else { directory }
    };
    let choose = move |_: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
        let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.settings.replay.export_path_hint"));
        spawn_detached(async move {
            let Some(directory) = shell.chosen_path(asked).await else { return };
            let directory = directory.to_string_lossy().into_owned();
            edit_replay(shell, |replay| replay.auto_export_path = directory);
        });
    };

    view! {
        column(class = "wt-settings__control-stack") {
            row(class = "wt-settings__control-row") {
                Switch(
                    checked = Binding::controlled(enabled, move |on: bool| edit_replay(shell, |replay| replay.auto_export_data = on)),
                    size = SwitchSize::Sm,
                    attr:data-testid = "auto-export-data",
                    a11y:label = Setting::AutoExport.title().into_owned()
                )
                row(class = "wt-settings__choices") {
                    for format in move || EXPORT_FORMATS.to_vec(), key = |format: &wows_toolkit_config::ReplayExportFormat| *format as usize {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            class = "wt-settings__choice",
                            disabled = Signal::derive_local(move || !enabled.get()),
                            attr:data-testid = format!("auto-export-format-{}", format.as_str()),
                            attr:data-selected = move || (chosen() == Some(format)).then(String::new),
                            a11y:selected = move || chosen() == Some(format),
                            on:click = move |_| edit_replay(shell, |replay| replay.auto_export_format = format)
                        ) {
                            {format.as_str().to_owned()}
                        }
                    }
                }
            }
            row(class = "wt-settings__control-row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || !enabled.get()),
                    attr:data-testid = "auto-export-choose",
                    on:click = choose
                ) {
                    {t!("ui.buttons.choose").into_owned()}
                }
                Tooltip {
                    TooltipTrigger(class = "wt-settings__clip") {
                        text(
                            class = "wt-settings__clip",
                            attr:data-testid = "settings-auto-export-path",
                            attr:data-error = move || missing.get().then(String::new)
                        ) {{shown}}
                    }
                    TooltipContent {{shown}}
                }
            }
        }
    }
}

/// The paste button for a Twitch credential: what Twitch made of the stored one, and what the last
/// paste made of the clipboard. The egui settings tab names the same four states.
#[component]
fn TwitchTokenButton(shell: Shell) -> impl IntoView {
    let twitch = shell.twitch;
    let pasted_wrong = move || matches!(twitch.paste(), Some(TwitchPaste::Refused { .. }));
    let label = move || {
        let key = if pasted_wrong() {
            "ui.settings.twitch.paste_token_invalid"
        } else {
            match twitch.status() {
                TwitchStatus::Unset => "ui.settings.twitch.paste_token_no_token",
                TwitchStatus::Checking => "ui.settings.twitch.paste_token_unvalidated",
                TwitchStatus::Accepted { .. } => "ui.settings.twitch.paste_token_valid",
                TwitchStatus::Refused => "ui.settings.twitch.paste_token_invalid",
            }
        };
        t!(key).into_owned()
    };
    // The glyph the egui tab draws beside the same label, and its tone.
    let status = move || -> (Glyph, &'static str) {
        if pasted_wrong() {
            return (Glyph::CircleX, "error");
        }
        match twitch.status() {
            TwitchStatus::Unset => (Glyph::TriangleAlert, "warn"),
            // Nothing has been proven either way yet, so nothing is claimed.
            TwitchStatus::Checking => (Glyph::Clock, "dim"),
            TwitchStatus::Accepted { .. } => (Glyph::CircleCheck, "ok"),
            TwitchStatus::Refused => (Glyph::CircleX, "error"),
        }
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "twitch-paste-token",
                    on:click = move |_| shell.paste_twitch_token()
                ) {
                    AppIcon(glyph = Glyph::ClipboardList)
                    text {{label}}
                    box(class = "wt-settings__twitch-status", attr:data-tone = move || Some(status().1.to_owned())) {
                        AppIcon(glyph = Signal::derive_local(move || status().0))
                    }
                }
            }
            TooltipContent {{t!("ui.settings.twitch.paste_token_tooltip").into_owned()}}
        }
    }
}

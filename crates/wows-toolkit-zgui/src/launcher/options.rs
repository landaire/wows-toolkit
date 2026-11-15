//! The launch option fields and the Proton setup, as Settings drawer controls.

use std::path::PathBuf;

use rust_i18n::t;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::proton::ProtonSetup;
use wows_toolkit_launcher::proton::Severity;
use wows_toolkit_services::dialog::Answer;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::IntoView;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::TextareaProps;

use crate::launcher::model::LauncherModel;
use crate::long_job::long_job;
use crate::services::Services;
use crate::shell::Shell;

/// Which Proton path a row shows and sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProtonPath {
    Binary,
    Prefix,
}

impl ProtonPath {
    fn label_key(self) -> &'static str {
        match self {
            Self::Binary => "ui.launcher.proton.binary",
            Self::Prefix => "ui.launcher.proton.prefix",
        }
    }

    fn test_id(self) -> &'static str {
        match self {
            Self::Binary => "launcher-proton-binary",
            Self::Prefix => "launcher-proton-prefix",
        }
    }

    fn of(self, settings: &LauncherSettings) -> Option<PathBuf> {
        match self {
            Self::Binary => settings.proton_binary.clone(),
            Self::Prefix => settings.proton_prefix.clone(),
        }
    }

    fn set(self, settings: &mut LauncherSettings, path: PathBuf) {
        match self {
            Self::Binary => settings.proton_binary = Some(path),
            Self::Prefix => settings.proton_prefix = Some(path),
        }
    }

    /// Asks for the path, and saves the answer.
    fn browse(self, model: LauncherModel, shell: Shell) {
        match self {
            Self::Binary => self.save_chosen(
                model,
                shell,
                wows_toolkit_services::dialog::pick_file(Some(&t!("ui.launcher.proton.binary")), None),
            ),
            Self::Prefix => self.save_chosen(
                model,
                shell,
                wows_toolkit_services::dialog::pick_folder(&t!("ui.launcher.proton.prefix")),
            ),
        }
    }

    fn save_chosen(self, model: LauncherModel, shell: Shell, asked: impl Future<Output = Answer> + 'static) {
        spawn_local(async move {
            let Some(path) = shell.chosen_path(asked).await else { return };
            model.edit_settings(|settings| self.set(settings, path));
        });
    }
}

/// Which one-per-line launch option a field edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchText {
    Args,
    Env,
    SafeModeArgs,
}

impl LaunchText {
    fn read(self, settings: &LauncherSettings) -> String {
        match self {
            Self::Args => settings.extra_args_text(),
            Self::Env => settings.env_vars_text(),
            Self::SafeModeArgs => settings.safe_mode_args_text(),
        }
    }

    fn write(self, settings: &mut LauncherSettings, text: &str) {
        match self {
            Self::Args => settings.set_extra_args_text(text),
            Self::Env => settings.set_env_vars_text(text),
            Self::SafeModeArgs => settings.set_safe_mode_args_text(text),
        }
    }

    fn test_id(self) -> &'static str {
        match self {
            Self::Args => "launcher-extra-args",
            Self::Env => "launcher-env-vars",
            Self::SafeModeArgs => "launcher-safe-mode-args",
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Args => "ui.settings.launcher.launch_args",
            Self::Env => "ui.settings.launcher.environment",
            Self::SafeModeArgs => "ui.settings.launcher.safe_mode_args",
        }
    }
}

/// A one-per-line launch option, saved when the field is left.
#[component]
pub fn LaunchTextField(model: LauncherModel, which: LaunchText) -> impl IntoView {
    let locale = Services::current().locale;
    // The drawer lists launcher rows only once the settings are read, so they are there to start
    // the field from.
    let start = model.with_untracked(|state| state.settings.as_ref().map(|settings| which.read(settings)));
    let text = RwSignal::new_local(start.unwrap_or_default());

    view! {
        Textarea(
            value = text,
            label = locale.text(which.label_key())(),
            class = "wt-launcher__textarea",
            attr:data-testid = which.test_id(),
            on:focus_out = move |_| {
                let text = text.get_untracked();
                model.edit_settings(|settings| which.write(settings, &text));
            }
        )
    }
}

/// The Proton binary and prefix, each with Browse, and Auto-detect.
#[component]
pub fn RunnerControl(model: LauncherModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide") {
            ProtonPathRow(model = model, shell = shell, which = ProtonPath::Binary)
            ProtonPathRow(model = model, shell = shell, which = ProtonPath::Prefix)
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-proton-detect",
                    on:click = move |_| autodetect_proton(model, shell)
                ) {
                    {locale.text("ui.launcher.proton.detect")}
                }
            }
        }
    }
}

#[component]
fn ProtonPathRow(model: LauncherModel, shell: Shell, which: ProtonPath) -> impl IntoView {
    let locale = Services::current().locale;
    let path = Memo::new(move |_| model.with(|state| state.settings.as_ref().and_then(|settings| which.of(settings))));

    view! {
        row(class = "wt-launcher__row") {
            text(class = "wt-launcher__text wt-launcher__path-label") {{locale.text(which.label_key())}}
            {move || match path.get() {
                Some(path) => view! {
                    text(class = "wt-launcher__text wt-launcher__truncate") {{path.display().to_string()}}
                }
                .into_view(),
                None => view! {
                    text(class = "wt-launcher__dim") {{locale.text("ui.launcher.not_set")}}
                }
                .into_view(),
            }}
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = which.test_id(),
                on:click = move |_| which.browse(model, shell)
            ) {
                {locale.text("ui.launcher.browse")}
            }
        }
    }
}

/// Fills in the Proton binary and prefix from this machine's Steam install, off the UI thread,
/// and says how it went.
pub fn autodetect_proton(model: LauncherModel, shell: Shell) {
    let Some(mut settings) = model.with_untracked(|state| state.settings.clone()) else { return };
    let asked_with = (settings.proton_binary.clone(), settings.proton_prefix.clone());

    spawn_local(async move {
        let (detected, result) = long_job(move || {
            let result =
                wows_toolkit_launcher::proton::autodetect(&mut settings, wows_toolkit_config::storage_dir().as_deref());
            (settings, result)
        })
        .await;

        // A path the reader chose while the detection ran is newer than what it found.
        let unchanged = model.with_untracked(|state| {
            state.settings.as_ref().is_some_and(|settings| {
                (settings.proton_binary.as_ref(), settings.proton_prefix.as_ref())
                    == (asked_with.0.as_ref(), asked_with.1.as_ref())
            })
        });
        if !unchanged {
            return;
        }

        if result.changed() {
            model.edit_settings(|settings| {
                settings.proton_binary = detected.proton_binary;
                settings.proton_prefix = detected.proton_prefix;
            });
        }
        report_proton(shell, result);
    });
}

fn report_proton(shell: Shell, result: ProtonSetup) {
    let toasts = shell.toasts();
    match result.severity() {
        Severity::Success => toasts.ok(result.to_string()),
        Severity::Warning => toasts.warn(result.to_string()),
        Severity::Error => toasts.failed(result.to_string()),
    };
}

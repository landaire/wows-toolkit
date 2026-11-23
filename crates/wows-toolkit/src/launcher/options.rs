//! The launch option fields and the runner choice, as Settings drawer controls.

use wgc_launch::Runner;
use wgc_launch::RunnerChoice;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::RunnerState;
use wt_translations::t;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::task::spawn_detached;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::TextareaProps;

use crate::launcher::model::LauncherModel;
use crate::services::Services;
use crate::shell::Shell;

/// What the runner row says of the runner search.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RunnerLine {
    Looking,
    Found {
        runner: String,
        /// One-time work the next launch does first.
        first_run: Vec<String>,
    },
    Missing(String),
}

impl RunnerLine {
    fn of(state: &RunnerState) -> Self {
        let runner = match state {
            RunnerState::Unknown => return Self::Looking,
            RunnerState::Missing(err) => return Self::Missing(err.to_string()),
            RunnerState::Found(runner) => runner,
        };

        let name = match runner.runner() {
            Runner::Native => t!("ui-launcher-runner-native"),
            Runner::SteamProton { proton, .. } => t!("ui-launcher-runner-steam_proton", name = proton.name()),
            Runner::Umu { proton: Some(proton), .. } => t!("ui-launcher-runner-umu", name = proton.name()),
            Runner::Umu { proton: None, .. } => t!("ui-launcher-runner-umu_default"),
        };
        let work = runner.first_run_work();
        let mut first_run = Vec::new();
        if work.prefix_creation {
            first_run.push(t!("ui-launcher-runner-first_run_prefix").into_owned());
        }
        if !work.umu_downloads.is_empty() {
            first_run.push(t!("ui-launcher-runner-first_run_download").into_owned());
        }

        Self::Found { runner: name.into_owned(), first_run }
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
            Self::Args => "ui-settings-launcher-launch_args",
            Self::Env => "ui-settings-launcher-environment",
            Self::SafeModeArgs => "ui-settings-launcher-safe_mode_args",
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

/// The runner the launcher found, with controls to choose a Proton build or go back to the
/// automatic choice.
#[component]
pub fn RunnerControl(model: LauncherModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;
    let line = Memo::new(move |_| model.with(|state| RunnerLine::of(state.launcher.runner())));
    let automatic = Memo::new(move |_| {
        model.with(|state| state.settings.as_ref().is_none_or(|settings| settings.runner == RunnerChoice::Auto))
    });

    view! {
        column(class = "wt-launcher__column wt-launcher__column--wide") {
            {move || match line.get() {
                RunnerLine::Looking => AnyView::new(view! {
                    text(class = "wt-launcher__dim", attr:data-testid = "launcher-runner-looking") {{t!("ui-launcher-runner-looking").into_owned()}}
                }),
                RunnerLine::Found { runner, first_run } => AnyView::new(view! {
                    text(class = "wt-launcher__text", attr:data-testid = "launcher-runner-found") {{runner}}
                    text(class = "wt-launcher__dim") {{first_run.join(" ")}}
                }),
                RunnerLine::Missing(error) => AnyView::new(view! {
                    text(class = "wt-launcher__warn", attr:data-testid = "launcher-runner-missing") {{t!("ui-launcher-runner-needed").into_owned()}}
                    text(class = "wt-launcher__dim") {{error}}
                }),
            }}
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-runner-choose",
                    on:click = move |_| choose_proton(model, shell)
                ) {
                    {locale.text("ui-launcher-runner-choose")}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || automatic.get()),
                    attr:data-testid = "launcher-runner-automatic",
                    on:click = move |_| model.set_runner(RunnerChoice::Auto)
                ) {
                    {locale.text("ui-launcher-runner-automatic")}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "launcher-runner-detect",
                    on:click = move |_| model.probe_runner()
                ) {
                    {locale.text("ui-launcher-runner-detect")}
                }
            }
        }
    }
}

/// Asks for a Proton build's folder and plays through it with Steam.
fn choose_proton(model: LauncherModel, shell: Shell) {
    let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui-launcher-runner-choose"));
    // Detached: the drawer or setup step that asked can close before the reader answers.
    spawn_detached(async move {
        let Some(proton_dir) = shell.chosen_path(asked).await else { return };
        if shell.is_unmounted() {
            return;
        }
        model.set_runner(RunnerChoice::SteamProton { proton_dir });
    });
}

//! The launcher's rows in the Settings drawer's Game section.

use wows_toolkit_viewmodel::settings_drawer::Setting;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::SwitchProps;
use zgui_ui::SwitchSize;
use zgui_ui::prelude::Binding;

use crate::launcher::CSS;
use crate::launcher::SHEET;
use crate::launcher::installs::InstallsControlProps;
use crate::launcher::opt_in::ask_to_enable;
use crate::launcher::options::LaunchText;
use crate::launcher::options::LaunchTextFieldProps;
use crate::launcher::options::RunnerControlProps;
use crate::shell::Shell;

/// Whether `setting`, a launcher row, is listed: the opt-in always, the rest once the launcher's
/// settings are read and the user opted in.
pub fn present(shell: Shell, setting: Setting) -> bool {
    match setting {
        Setting::GameLauncher => shell.launcher.with(|state| state.settings.is_some()),
        _ => shell.launcher.is_enabled(),
    }
}

/// The control of the launcher row `setting`.
pub fn control(shell: Shell, setting: Setting) -> AnyView {
    install_stylesheet(SHEET, CSS);
    let model = shell.launcher;

    match setting {
        Setting::GameLauncher => {
            let on = Memo::new(move |_| model.is_enabled());
            let on = Signal::derive_local(move || on.get());
            AnyView::new(view! {
                Switch(
                    checked = Binding::controlled(on, move |wanted: bool| {
                        if wanted {
                            ask_to_enable(shell, model);
                        } else {
                            model.disable();
                        }
                    }),
                    size = SwitchSize::Sm,
                    attr:data-testid = "launcher-enabled",
                    a11y:label = setting.title().into_owned()
                )
            })
        }
        Setting::GameInstalls => AnyView::new(view! { InstallsControl(model = model, shell = shell) }),
        Setting::GameRunner => AnyView::new(view! { RunnerControl(model = model, shell = shell) }),
        Setting::GameLaunchArgs => AnyView::new(view! { LaunchTextField(model = model, which = LaunchText::Args) }),
        Setting::GameEnvironment => AnyView::new(view! { LaunchTextField(model = model, which = LaunchText::Env) }),
        Setting::GameSafeModeArgs => {
            AnyView::new(view! { LaunchTextField(model = model, which = LaunchText::SafeModeArgs) })
        }
        // Not a launcher row; the drawer draws it itself.
        _ => AnyView::new(()),
    }
}

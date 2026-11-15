//! The first-run setup dialog: the game install, Proton, accounts and replays, one step each.
//! Every step can be skipped.

use rust_i18n::t;
use wows_toolkit_launcher::LoginTarget;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::SeparatorProps;

use crate::launcher::CSS;
use crate::launcher::SHEET;
use crate::launcher::account::SignInPanelProps;
use crate::launcher::display::AccountRow;
use crate::launcher::display::InstallView;
use crate::launcher::game_settings::ReplayRetentionProps;
use crate::launcher::games::GamesListProps;
use crate::launcher::installs::add_install;
use crate::launcher::model::LauncherModel;
use crate::launcher::options::RunnerControlProps;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;

const SETUP_CSS: &str = zgui::css!(
    ".wt-setup {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 620px;
        max-width: 100%;
    }
    .wt-setup__steps {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    .wt-setup__step {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        color: var(--wt-color-text-dim);
    }
    .wt-setup__step[data-current] {
        color: var(--lui-color-text-bright);
        font-weight: var(--zui-type-weight-bold);
    }
    .wt-setup__number {
        width: 20px;
        height: 20px;
        border-radius: 10px;
        border: 1px solid var(--lui-color-border);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 11px;
    }
    .wt-setup__step[data-current] .wt-setup__number {
        background-color: var(--lui-color-accent);
        border-color: var(--lui-color-accent);
        color: var(--lui-color-surface);
    }
    .wt-setup__rule {
        width: 24px;
        height: 1px;
        background-color: var(--lui-color-border);
    }
    .wt-setup__body {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-height: 220px;
        max-height: 60vh;
        overflow-y: auto;
    }
    .wt-setup__choice {
        justify-content: flex-start;
    }
    .wt-setup__choice[data-selected] {
        border-color: var(--lui-color-accent);
        background-color: var(--lui-color-selection);
    }"
);

/// One step of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupStep {
    Game,
    Proton,
    Accounts,
    Replays,
}

impl SetupStep {
    const ALL: [Self; 4] = [Self::Game, Self::Proton, Self::Accounts, Self::Replays];

    fn label(self) -> String {
        match self {
            Self::Game => t!("ui.launcher.setup.game"),
            Self::Proton => t!("ui.launcher.setup.proton"),
            Self::Accounts => t!("ui.launcher.setup.accounts"),
            Self::Replays => t!("ui.launcher.setup.replays"),
        }
        .into_owned()
    }

    fn name(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Proton => "proton",
            Self::Accounts => "accounts",
            Self::Replays => "replays",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap_or_default()
    }

    fn next(self) -> Option<Self> {
        Self::ALL.get(self.index() + 1).copied()
    }

    fn previous(self) -> Option<Self> {
        self.index().checked_sub(1).map(|at| Self::ALL[at])
    }
}

/// Opens the setup dialog on its first step.
pub fn open(shell: Shell, model: LauncherModel) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        install_stylesheet("wt-setup", SETUP_CSS);
        let step = RwSignal::new_local(SetupStep::Game);
        model.update_launcher(|launcher| launcher.reset_login(LoginTarget::NewAccount));
        let is_last = Signal::derive_local(move || step.get().next().is_none());

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "launcher-setup") {
                    DialogHeader {
                        DialogTitle {{t!("ui.launcher.setup.title").into_owned()}}
                    }
                    column(class = "wt-setup") {
                        Steps(step = step)
                        Separator()
                        column(class = "wt-setup__body", attr:data-testid = move || Some(format!("launcher-setup-{}", step.get().name()))) {
                            {move || match step.get() {
                                SetupStep::Game => AnyView::new(view! { GameStep(shell = shell, model = model) }),
                                SetupStep::Proton => AnyView::new(view! { ProtonStep(shell = shell, model = model) }),
                                SetupStep::Accounts => AnyView::new(view! { AccountsStep(model = model) }),
                                SetupStep::Replays => AnyView::new(view! { ReplaysStep(shell = shell, model = model) }),
                            }}
                        }
                    }
                    DialogFooter {
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::Sm,
                            attr:data-testid = "launcher-setup-skip",
                            on:click = move |_| handle.close()
                        ) {
                            {t!("ui.launcher.setup.skip").into_owned()}
                        }
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            disabled = Signal::derive_local(move || step.get().previous().is_none()),
                            attr:data-testid = "launcher-setup-back",
                            on:click = move |_| {
                                if let Some(previous) = step.get_untracked().previous() {
                                    step.set(previous);
                                }
                            }
                        ) {
                            {t!("ui.launcher.setup.back").into_owned()}
                        }
                        Button(
                            size = ButtonSize::Sm,
                            attr:data-testid = "launcher-setup-next",
                            on:click = move |_| match step.get_untracked().next() {
                                Some(next) => step.set(next),
                                None => handle.close(),
                            }
                        ) {
                            {move || if is_last.get() { t!("ui.launcher.setup.done").into_owned() } else { t!("ui.launcher.setup.next").into_owned() }}
                        }
                    }
                }
            }
        })
    });
}

/// The numbered steps, the current one marked.
#[component]
fn Steps(step: RwSignal<SetupStep, LocalStorage>) -> impl IntoView {
    let parts: Vec<AnyView> = SetupStep::ALL
        .into_iter()
        .enumerate()
        .map(|(index, each)| {
            let rule = (index > 0).then(|| view! { box(class = "wt-setup__rule") });
            AnyView::new(view! {
                {rule}
                row(
                    class = "wt-setup__step",
                    attr:data-testid = format!("launcher-setup-step-{}", each.name()),
                    attr:data-current = move || (step.get() == each).then(String::new)
                ) {
                    text(class = "wt-setup__number") {{(index + 1).to_string()}}
                    text {{each.label()}}
                }
            })
        })
        .collect();

    view! {
        row(class = "wt-setup__steps") {
            {parts}
        }
    }
}

/// Which copy of the game the toolkit uses: one found, a folder, or a fresh install.
#[component]
fn GameStep(shell: Shell, model: LauncherModel) -> impl IntoView {
    let installs = Memo::new(move |_| model.with(|state| InstallView::all(&state.launcher)));
    let discovering = Memo::new(move |_| model.with(|state| state.launcher.is_discovering()));
    let installing = RwSignal::new_local(false);

    view! {
        text {{t!("ui.launcher.setup.game_question").into_owned()}}
        for install in move || installs.get(), key = |install: &InstallView| install.clone() {
            InstallChoiceButton(shell = shell, model = model, install = install)
        }
        if move || installs.with(Vec::is_empty) && !discovering.get() {
            text(class = "wt-launcher__dim", attr:data-testid = "launcher-setup-none-found") {{t!("ui.launcher.setup.none_found").into_owned()}}
        } else {}
        row(class = "wt-launcher__row") {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "launcher-setup-choose-folder",
                on:click = move |_| add_install(model, shell)
            ) {
                {t!("ui.launcher.setup.choose_folder").into_owned()}
            }
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "launcher-setup-install-new",
                on:click = move |_| installing.set(true)
            ) {
                {t!("ui.launcher.setup.install_new").into_owned()}
            }
        }
        Show(when = move || installing.get()) {
            GamesList(model = model, shell = shell)
        }
    }
}

/// One install found, which the toolkit uses once pressed.
#[component]
fn InstallChoiceButton(shell: Shell, model: LauncherModel, install: InstallView) -> impl IntoView {
    let path = install.path.clone();
    let chosen_path = path.clone();
    let chosen = Memo::new(move |_| model.wows_dir().is_some_and(|dir| dir == chosen_path));
    let version = match &install.version {
        Some(crate::launcher::display::InstallVersion::Known(version)) => version.clone(),
        Some(crate::launcher::display::InstallVersion::ManagedBySteam) => {
            t!("ui.launcher.managed_by_steam").into_owned()
        }
        None => String::new(),
    };
    let shown = path.display().to_string();

    view! {
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            class = "wt-setup__choice",
            attr:data-testid = format!("launcher-setup-install-{}", install.slot),
            attr:data-selected = move || chosen.get().then(String::new),
            on:click = move |_| shell.apply_wows_dir(path.to_string_lossy().into_owned())
        ) {
            text(class = "wt-launcher__mono wt-launcher__truncate") {{shown}}
            text(class = "wt-launcher__dim") {{format!("{version}   {}", install.source)}}
        }
    }
}

/// Proton: what was found, and the controls to change it.
#[component]
fn ProtonStep(shell: Shell, model: LauncherModel) -> impl IntoView {
    let needed = Memo::new(move |_| {
        model.with(|state| state.settings.as_ref().is_some_and(|settings| settings.proton_config().is_none()))
    });

    view! {
        text {{t!("ui.launcher.setup.proton_explain").into_owned()}}
        if move || needed.get() {
            text(class = "wt-launcher__warn", attr:data-testid = "launcher-setup-proton-needed") {{t!("ui.launcher.proton.needed").into_owned()}}
        } else {
            text(class = "wt-launcher__ok", attr:data-testid = "launcher-setup-proton-ready") {{t!("ui.launcher.setup.proton_ready").into_owned()}}
        }
        RunnerControl(model = model, shell = shell)
    }
}

/// The accounts saved so far, and the sign-in form for another.
#[component]
fn AccountsStep(model: LauncherModel) -> impl IntoView {
    let rows = Memo::new(move |_| model.with(AccountRow::all));
    let finished = Memo::new(move |_| model.with(|state| state.launcher.login_finished()));
    // A finished sign-in leaves the form ready for the next account.
    let generation = Memo::new(move |_| finished.get().map(|(key, _)| key));

    view! {
        text {{t!("ui.launcher.setup.accounts_explain").into_owned()}}
        for row in move || rows.get(), key = |row: &AccountRow| row.label.clone() {
            row(class = "wt-launcher__row", attr:data-testid = format!("launcher-setup-account-{}", row.label.key.account_id.get())) {
                text(class = "wt-launcher__badge") {{row.label.realm}}
                text {{row.label.nickname.clone()}}
            }
        }
        {move || {
            let _ = generation.get();
            view! { SignInPanel(model = model, target = LoginTarget::NewAccount) }
        }}
    }
}

/// How many replays the game keeps.
#[component]
fn ReplaysStep(shell: Shell, model: LauncherModel) -> impl IntoView {
    view! {
        text {{t!("ui.launcher.setup.replays_explain").into_owned()}}
        ReplayRetention(shell = shell, model = model)
    }
}

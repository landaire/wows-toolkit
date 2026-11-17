//! The Play split button at the end of the tab strip: the main part does what the launcher's
//! state calls for, and the chevron opens the account popover.

use std::path::Path;
use std::path::PathBuf;

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_launcher::AccountKey;
use wows_toolkit_launcher::AccountStatus;
use wows_toolkit_launcher::LaunchFailure;
use wows_toolkit_launcher::LoginTarget;
use wows_toolkit_launcher::PlayError;
use wows_toolkit_viewmodel::settings_drawer::Setting;
use zgui::component;
use zgui::prelude::try_use_window;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::task::spawn_detached;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::LabelProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SpinnerProps;
use zgui_ui::prelude::Binding;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::launcher::CSS;
use crate::launcher::SHEET;
use crate::launcher::account::KeyringHintProps;
use crate::launcher::account::open_sign_in;
use crate::launcher::display::PlayState;
use crate::launcher::display::realm_badge;
use crate::launcher::model::LauncherModel;
use crate::launcher::model::PendingPlay;
use crate::launcher::model::PlayOrigin;
use crate::launcher::palette::LauncherOffersProps;
use crate::launcher::popover::AccountPopoverProps;
use crate::launcher::realm::RealmGlyphProps;
use crate::launcher::setup;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;

const PLAY_CSS: &str = zgui::css!(
    ".wt-play {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: none;
        gap: 1px;
        margin-right: 4px;
    }
    .wt-play__main {
        border-top-right-radius: 0;
        border-bottom-right-radius: 0;
        gap: 6px;
    }
    .wt-play__chevron {
        border-top-left-radius: 0;
        border-bottom-left-radius: 0;
        padding-left: 4px;
        padding-right: 4px;
    }
    .wt-play-primary {
        background-color: var(--lui-color-accent);
        border-color: var(--lui-color-accent);
        color: var(--lui-color-surface);
    }
    .wt-play__realm {
        font-size: 11px;
        opacity: 0.85;
    }
    .wt-play__nickname {
        max-width: 160px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-play__live {
        width: 8px;
        height: 8px;
        border-radius: 4px;
        background-color: var(--lui-color-accent);
        flex: none;
    }
    .wt-play .zui-spinner {
        width: 12px;
        height: 12px;
        flex: none;
    }
    .zui-popover.wt-account-popover {
        width: 400px;
    }
    .wt-play__hint {
        max-width: 320px;
        font-size: 11px;
    }"
);

/// Below this window width, in CSS pixels, the button keeps only its glyph and the realm badge.
const COLLAPSE_BELOW: f32 = 1000.0;

/// The split button, shown while the launcher is enabled.
#[component]
pub fn PlayControl(shell: Shell, model: LauncherModel) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    install_stylesheet("wt-play", PLAY_CSS);
    let enabled = Memo::new(move |_| model.is_enabled());

    view! {
        Show(when = move || enabled.get()) {
            SplitButton(shell = shell, model = model)
            LauncherOffers(shell = shell, model = model)
        }
    }
}

#[component]
fn SplitButton(shell: Shell, model: LauncherModel) -> impl IntoView {
    let state = Memo::new(move |_| {
        let wows_dir = model.wows_dir();
        let in_battle = shell.live.live_match.with(Option::is_some);
        model.with(|state| PlayState::of(state, wows_dir.as_deref(), in_battle))
    });
    let window = try_use_window().map(|window| window.size());
    let collapsed = Memo::new(move |_| window.is_some_and(|size| size.get().width.0 < COLLAPSE_BELOW));
    let resuming = RenderEffect::new(move |_| {
        let Some(pending) = model.pending_play() else { return };
        let (holds, reading) = model.with(|state| {
            let reading = matches!(
                state.launcher.account_status(pending.account),
                AccountStatus::NotLoaded | AccountStatus::Restoring
            );
            (pending.holds(state), reading)
        });
        if !holds {
            model.clear_pending_play();
        } else if !reading {
            model.clear_pending_play();
            launch(shell, model, pending.account, pending.safe_mode, pending.origin);
        }
    });
    on_cleanup_local(move || drop(resuming));
    let waiting = Memo::new(move |_| model.pending_play().is_some());
    let open = RwSignal::new_local(false);
    let primary = move || state.with(PlayState::is_primary);
    let tip = move || {
        let state = state.get();
        match state.nickname() {
            Some(nickname) => format!("{} {nickname}", label(&state)),
            None => label(&state),
        }
    };

    view! {
        row(class = "wt-play", attr:data-testid = "launcher-play-control") {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        class = "wt-play__main",
                        class:wt-play-primary = primary,
                        disabled = Signal::derive_local(move || state.with(|state| matches!(state, PlayState::Loading | PlayState::Restoring(_)))),
                        attr:data-testid = "launcher-play",
                        attr:data-state = move || Some(state.with(state_name).to_owned()),
                        a11y:label = tip,
                        on:click = move |_| press(shell, model, state.get_untracked(), open)
                    ) {
                        {move || content(state.get(), collapsed.get())}
                    }
                }
                TooltipContent {{tip}}
            }
            Popover(open = open) {
                PopoverTrigger(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    class = "wt-play__chevron",
                    class:wt-play-primary = primary,
                    attr:data-testid = "launcher-play-menu",
                    a11y:label = t!("ui.launcher.play.menu").into_owned()
                ) {
                    AppIcon(glyph = Glyph::ChevronDown)
                }
                PopoverContent(
                    placement = Placement::new(Side::Bottom, Align::End),
                    class = "wt-account-popover",
                    attr:data-testid = "launcher-popover"
                ) {
                    AccountPopover(shell = shell, model = model, open = open)
                }
            }
            Show(when = move || waiting.get()) {
                box(class = "wt-play__hint") {
                    KeyringHint(model = model)
                }
            }
        }
    }
}

/// What the main part shows for `state`; collapsed keeps the glyph and the realm badge.
fn content(state: PlayState, collapsed: bool) -> AnyView {
    let realm = state.realm();
    let glyph = match &state {
        PlayState::Play(_) | PlayState::AddAccount => Some(Glyph::Play),
        PlayState::Update { .. } => Some(Glyph::Download),
        PlayState::SetUpGame | PlayState::SetUpRunner => Some(Glyph::Settings),
        PlayState::Paused { .. } => Some(Glyph::Pause),
        _ => None,
    };
    let busy = matches!(state, PlayState::Starting { .. } | PlayState::Updating { .. } | PlayState::Restoring(_));
    let live = matches!(state, PlayState::Running { .. } | PlayState::InBattle { .. });
    let words = (!collapsed).then(|| label(&state));
    let nickname = (!collapsed).then(|| state.nickname().map(str::to_owned)).flatten();

    AnyView::new(view! {
        if move || busy {
            Spinner()
        } else {}
        if move || live {
            box(class = "wt-play__live")
        } else {}
        {optional(glyph.map(|glyph| AnyView::new(view! { AppIcon(glyph = glyph) })))}
        {optional(words.map(|words| AnyView::new(view! { text(class = "wt-play__label", attr:data-testid = "launcher-play-label") {{words}} })))}
        {optional(realm.map(|realm| AnyView::new(view! { RealmGlyph(realm = realm, attr:data-testid = "launcher-play-realm-glyph") })))}
        {optional(realm.map(|realm| AnyView::new(view! { text(class = "wt-play__realm", attr:data-testid = "launcher-play-realm") {{realm_badge(realm)}} })))}
        {optional(nickname.map(|nickname| AnyView::new(view! { text(class = "wt-play__nickname") {{nickname}} })))}
    })
}

/// `view`, or nothing.
fn optional(view: Option<AnyView>) -> AnyView {
    view.unwrap_or_else(|| AnyView::new(()))
}

/// The words the main part says for `state`, without the account.
fn label(state: &PlayState) -> String {
    match state {
        PlayState::Loading | PlayState::AddAccount | PlayState::Restoring(_) | PlayState::Play(_) => {
            t!("ui.launcher.play.play")
        }
        PlayState::SetUpGame => t!("ui.launcher.play.set_up_game"),
        PlayState::SetUpRunner => t!("ui.launcher.play.set_up_runner"),
        PlayState::SignInAgain(_) => t!("ui.launcher.play.sign_in_again"),
        PlayState::Update { size, .. } => {
            t!("ui.launcher.play.update", size = humansize::format_size(*size, humansize::DECIMAL))
        }
        PlayState::Updating { percent: Some(percent), .. } => t!("ui.launcher.play.updating", percent = percent),
        PlayState::Updating { percent: None, .. } => t!("ui.launcher.play.updating_unknown"),
        PlayState::Paused { .. } => t!("ui.launcher.play.paused"),
        PlayState::PlayThroughSteam { .. } => t!("ui.launcher.play.play_through_steam"),
        PlayState::Starting { .. } => t!("ui.launcher.play.starting"),
        PlayState::Running { .. } => t!("ui.launcher.play.running"),
        PlayState::InBattle { .. } => t!("ui.launcher.play.in_battle"),
    }
    .into_owned()
}

/// What a test reads the state off the button by.
fn state_name(state: &PlayState) -> &'static str {
    match state {
        PlayState::Loading => "loading",
        PlayState::SetUpGame => "set-up-game",
        PlayState::SetUpRunner => "set-up-runner",
        PlayState::AddAccount => "add-account",
        PlayState::Restoring(_) => "restoring",
        PlayState::SignInAgain(_) => "sign-in-again",
        PlayState::Update { .. } => "update",
        PlayState::Updating { .. } => "updating",
        PlayState::Paused { .. } => "paused",
        PlayState::PlayThroughSteam { .. } => "play-through-steam",
        PlayState::Play(_) => "play",
        PlayState::Starting { .. } => "starting",
        PlayState::Running { .. } => "running",
        PlayState::InBattle { .. } => "in-battle",
    }
}

/// Does what the main part offers in `state`.
fn press(shell: Shell, model: LauncherModel, state: PlayState, popover: RwSignal<bool, LocalStorage>) {
    match state {
        PlayState::SetUpGame => setup::open(shell, model),
        PlayState::SetUpRunner => shell.settings_drawer.open_at(Setting::GameRunner),
        PlayState::AddAccount => open_sign_in(shell, model, LoginTarget::NewAccount),
        PlayState::SignInAgain(account) => open_sign_in(shell, model, LoginTarget::Reauthenticate(account.key)),
        PlayState::Update { install, .. } => start_update(shell, model, install),
        PlayState::Updating { .. } | PlayState::Paused { .. } => popover.set(true),
        PlayState::Play(account) => play(shell, model, account.key, PlayOrigin::Default),
        PlayState::PlayThroughSteam { install } => play_through_steam(shell, model, install),
        PlayState::InBattle { .. } => shell.select(TabKey::App(AppTab::PlayerTracker)),
        // Nothing to do until the state moves on.
        PlayState::Loading | PlayState::Restoring(_) | PlayState::Starting { .. } | PlayState::Running { .. } => {}
    }
}

/// Starts the game for `account`, in safe mode when the safe mode box is ticked, saying in a
/// message why it could not.
pub fn play(shell: Shell, model: LauncherModel, account: AccountKey, origin: PlayOrigin) {
    launch(shell, model, account, model.safe_mode_untracked(), origin);
}

/// [`play`] with safe mode as it was when the press was made.
fn launch(shell: Shell, model: LauncherModel, account: AccountKey, safe_mode: bool, origin: PlayOrigin) {
    // An unread session is read first. The press waits on it beside the keyring hint, and the
    // Play control plays once the read answers.
    let unread = model.with_untracked(|state| {
        matches!(state.launcher.account_status(account), AccountStatus::NotLoaded | AccountStatus::Restoring)
    });
    if unread {
        model.defer_play(PendingPlay { account, safe_mode, origin });
        model.ensure_sessions();
        return;
    }
    let wows_dir = model.wows_dir_untracked();
    let played = model.run(move |launcher, settings, env| {
        let played = launcher.play(account, wows_dir.as_deref(), safe_mode, settings, env, |command| {
            wows_toolkit_services::child_process::prepare(command);
        });
        (played, false)
    });
    let Some(Err(err)) = played else { return };

    match &err {
        PlayError::NotSignedIn { key, .. } => open_sign_in(shell, model, LoginTarget::Reauthenticate(*key)),
        PlayError::NoInstall { .. } => {
            shell.toasts().failed(t!("ui.launcher.launch_failed", error = err.to_string()).into_owned());
            shell.settings_drawer.open_at(Setting::GameInstalls);
        }
        _ => {
            shell.toasts().failed(t!("ui.launcher.launch_failed", error = err.to_string()).into_owned());
        }
    }
}

/// Hands the Steam install at `install` to Steam.
fn play_through_steam(shell: Shell, model: LauncherModel, install: PathBuf) {
    let safe_mode = model.safe_mode_untracked();
    let played = model.run(move |launcher, settings, env| {
        let played = launcher.play_through_steam(&install, safe_mode, settings, env, |command| {
            wows_toolkit_services::child_process::prepare(command);
        });
        (played, false)
    });
    if let Some(Err(err)) = played {
        shell.toasts().failed(t!("ui.launcher.launch_failed", error = err.to_string()).into_owned());
    }
}

/// The safe mode box. Every way to Play reads it, so the popover and the drawer share it.
#[component]
pub fn SafeModeCheck(model: LauncherModel, test_id: &'static str) -> impl IntoView {
    let locale = Services::current().locale;
    let name = NodeRef::new();
    let control = NodeRef::new();
    let checked = Signal::derive_local(move || Checked::from(model.safe_mode()));

    view! {
        row(class = "wt-launcher__row wt-launcher__row--tight") {
            Checkbox(
                checked = Binding::controlled(checked, move |on: Checked| model.set_safe_mode(on.is_checked())),
                node_ref = control,
                labelled_by = name,
                attr:data-testid = test_id
            )
            Label(node_ref = name, control = control, on:click = move |_| model.set_safe_mode(!model.safe_mode_untracked())) {
                {locale.text("ui.launcher.popover.play_safe_mode")}
            }
        }
    }
}

/// Says why a launch failed after Play returned, and opens the setting that can fix it.
pub fn report_launch_failure(shell: Shell, failure: &LaunchFailure) {
    shell.toasts().failed(t!("ui.launcher.launch_failed", error = failure.to_string()).into_owned());
    match failure {
        LaunchFailure::Runner(_) | LaunchFailure::Prefix(_) | LaunchFailure::NoWinePrefix => {
            shell.settings_drawer.open_at(Setting::GameRunner);
        }
        LaunchFailure::View(_) | LaunchFailure::WgcLayer(_) => shell.settings_drawer.open_at(Setting::GameInstalls),
        // Nothing in the settings changes these.
        LaunchFailure::Lock(_)
        | LaunchFailure::RelayPrefix(_)
        | LaunchFailure::Relay(_)
        | LaunchFailure::Spawn(_)
        | LaunchFailure::SteamLaunch(_)
        | LaunchFailure::LaunchDelta(_)
        | LaunchFailure::Task(_) => {}
    }
}

/// Asks to confirm an update of the install at `install` with a folder picker opened at the
/// install, and updates it in place once the install's own folder is chosen. The update is a
/// delta against that install's files, so no other folder can take it.
pub fn start_update(shell: Shell, model: LauncherModel, install: PathBuf) {
    let asked = wows_toolkit_services::dialog::pick_folder_in(&t!("ui.launcher.download_title"), &install);
    // Detached: the popover or palette that asked has closed before the picker answers.
    spawn_detached(async move {
        let Some(dest) = shell.chosen_path(asked).await else { return };
        if shell.is_unmounted() {
            return;
        }
        if !same_folder(&dest, &install) {
            let path = install.display().to_string();
            shell.toasts().failed(t!("ui.launcher.update_in_place", path = path).into_owned());
            return;
        }
        let started = model.run(move |launcher, _, env| (launcher.start_download(install, dest, env), false));
        if let Some(Err(err)) = started {
            shell.toasts().failed(t!("ui.launcher.download_failed", error = err.to_string()).into_owned());
        }
    });
}

/// Whether `a` and `b` name the same folder. A folder that cannot be resolved is no match.
fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

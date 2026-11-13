//! The collaborative session: hosting one, joining one, and steering it from the session control
//! on the status bar.
//!
//! What it drives is `wows_toolkit_services::collab::CollabState`, shared with the GPUI front end
//! and speaking `wt-collab-client`, so a session hosted from any front end looks the same to a
//! peer. One session per window: it belongs to the replays tab, so its control is on the bar only
//! while that tab is selected.

use std::time::Duration;

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::CollabState;
use wows_toolkit_services::collab::ConnectedPeer;
use wows_toolkit_services::collab::SharedWindow;
use wows_toolkit_viewmodel::collab::NoticeLevel;
use wows_toolkit_viewmodel::collab::SessionFailure;
use wows_toolkit_viewmodel::collab::SessionNotice;
use wows_toolkit_viewmodel::notices::Notice;
use wows_toolkit_viewmodel::settings::keys;
use wt_collab_client::PeerRole;
use wt_collab_client::Permissions;
use wt_collab_client::SessionStatus;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Track;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::Timers;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SeparatorProps;
use zgui_ui::SpinnerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::shell::Shell;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;

const SHEET: &str = "wt-collab";

// 300px matches the egui popover's own minimum.
const CSS: &str = css!(
    ".wt-collab-popover { width: 300px; }
    .wt-collab {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-width: 0;
    }
    .wt-collab__title { font-size: 14px; font-weight: 700; }
    .wt-collab__heading { flex: 1; min-width: 0; font-size: 14px; font-weight: 700; color: var(--lui-color-danger); }
    .wt-collab__hint { font-size: 12px; color: var(--lui-color-text-muted); }
    .wt-collab__error { font-size: 12px; color: var(--lui-color-danger); }
    .wt-collab__row { display: flex; align-items: center; gap: 4px; font-size: 12px; }
    .wt-collab__wrap { flex-wrap: wrap; }
    .wt-collab__battle { display: flex; flex-direction: column; flex: 1; min-width: 0; }
    .wt-collab__head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
    .wt-collab__grow { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .wt-collab__token { flex: 1; min-width: 0; font-family: monospace; font-size: 12px; overflow: hidden; }
    .wt-collab__check { display: flex; align-items: center; gap: 8px; font-size: 14px; }
    .wt-collab__wide { width: 100%; }
    .wt-session-toggle-active { color: var(--lui-color-danger); }"
);

/// How often a running session is looked at between wakes.
const SESSION_TICK: Duration = Duration::from_millis(250);

/// The window's session. A `Copy` handle.
#[derive(Clone, Copy)]
pub struct CollabHost {
    state: StoredValue<CollabState, LocalStorage>,
    /// Bumped whenever the session may have changed, which is what every reader of it follows.
    revision: RwSignal<u64, LocalStorage>,
    /// Whether a session is running, as last seen. A change hands every viewport a new link.
    active: RwSignal<bool, LocalStorage>,
    /// Whether this end may steer, as last seen. A link made before a promotion has no mesh to
    /// send frames into, so a change hands every viewport a new link too.
    steer: RwSignal<bool, LocalStorage>,
    /// The name being typed into the popover, seeded from the settings.
    name: RwSignal<String, LocalStorage>,
    token: RwSignal<String, LocalStorage>,
    /// Shared windows asked to be watched, for the replays tab to open.
    watch_requests: RwSignal<Vec<SharedWindow>, LocalStorage>,
}

impl CollabHost {
    pub fn new() -> Self {
        Self {
            state: StoredValue::new_local(CollabState::default()),
            revision: RwSignal::new_local(0),
            active: RwSignal::new_local(false),
            steer: RwSignal::new_local(false),
            name: RwSignal::new_local(String::new()),
            token: RwSignal::new_local(String::new()),
            watch_requests: RwSignal::new_local(Vec::new()),
        }
    }

    /// This window's end of the session, for a viewport. Changes when a session starts or ends,
    /// and when this end is raised to co-host.
    ///
    /// Each read makes a new link: a link holds what its own viewport drew while there is no
    /// session, and one shared between viewports would put every viewport's drawings on every
    /// other.
    pub fn link(self) -> Signal<CollabLink, LocalStorage> {
        Signal::derive_local(move || {
            self.active.track();
            self.steer.track();
            self.state.with_value(CollabState::link)
        })
    }

    /// Whether a session is running.
    pub fn is_active(self) -> bool {
        self.active.get()
    }

    /// Takes every shared window asked to be watched, oldest first, subscribing to the next one.
    pub fn take_watch_requests(self) -> Vec<SharedWindow> {
        if self.watch_requests.with(Vec::is_empty) {
            return Vec::new();
        }
        let mut taken = Vec::new();
        self.watch_requests.update(|requests| taken = std::mem::take(requests));

        taken
    }

    /// Reads the session, following its changes.
    fn read<R>(self, read: impl FnOnce(&CollabState) -> R) -> R {
        self.revision.track();
        self.state.with_value(read)
    }

    /// Reads the session once, for a listener.
    fn peek<R>(self, read: impl FnOnce(&CollabState) -> R) -> R {
        self.state.with_value(read)
    }

    /// Changes the session and says so to every reader.
    fn change(self, shell: Shell, change: impl FnOnce(&mut CollabState)) {
        self.state.update_value(change);
        self.refresh(shell);
    }

    /// Notes what the session is now, handing the tactics board its end of it when a session has
    /// started or ended or this end was raised.
    fn refresh(self, shell: Shell) {
        self.revision.update(|revision| *revision += 1);
        let active = self.state.with_value(CollabState::is_active);
        let steer = active && self.state.with_value(CollabState::may_steer);
        if active == self.active.get_untracked() && steer == self.steer.get_untracked() {
            return;
        }
        self.active.set(active);
        self.steer.set(steer);
        shell.tactics.share_session(active.then(|| self.state.with_value(CollabState::link)));
    }

    /// Takes what the peer task raised: says each notice at its own level, and asks for whatever
    /// the host opened for everyone.
    fn poll(self, shell: Shell) {
        let notices = self.state.update_value_returning(CollabState::poll);
        for notice in &notices {
            say(shell, notice);
        }

        let forced = self.state.with_value(|state| state.link().take_forced_windows());
        if !forced.is_empty() {
            let shared = self.state.with_value(CollabState::shared_windows);
            let opened: Vec<SharedWindow> =
                forced.into_iter().filter_map(|id| shared.iter().find(|one| one.replay_id == id).cloned()).collect();
            self.watch(opened);
        }
        self.refresh(shell);
    }

    fn watch(self, windows: Vec<SharedWindow>) {
        if windows.is_empty() {
            return;
        }
        self.watch_requests.update(|requests| requests.extend(windows));
    }

    /// Follows the peer task and the saved display name. Called once, from the main window's
    /// shell.
    pub fn begin(self, shell: Shell) {
        let services = shell.services();
        // Seeded once, as the settings land; a name typed before then is saved instead.
        let seeded = StoredValue::new(false);
        let seeding = RenderEffect::new(move |_| {
            if seeded.get_value() {
                return;
            }
            let Some(saved) = services.settings.read(|settings| settings.collab_display_name.clone()) else { return };
            seeded.set_value(true);
            if self.name.with_untracked(String::is_empty) {
                self.name.set(saved);
            } else {
                self.adopt_name(shell);
            }
        });
        // What this end asks of the peer task (locks, a replay opened, a window opened for
        // everyone) changes the session without a wake, so a running session is also looked at
        // on a tick, as the GPUI view does on each draw.
        let ticking = Timers::current().map(|timers| {
            timers.set_interval(SESSION_TICK, move || {
                if self.active.get_untracked() {
                    self.poll(shell);
                }
            })
        });
        on_cleanup_local(move || {
            drop(seeding);
            drop(ticking);
        });

        let Some(mut wakes) = self.state.with_value(CollabState::wakes) else { return };
        spawn_local(async move {
            use futures::StreamExt as _;
            // Each wake is drained here: the session's inbox is unbounded, so events left in it
            // would accumulate for as long as the session runs.
            while wakes.next().await.is_some() {
                if self.state.try_with_value(|_| ()).is_none() {
                    break;
                }
                self.poll(shell);
            }
        });
    }

    /// Saves the typed name, when it differs from the saved one.
    fn adopt_name(self, shell: Shell) {
        let name = self.name.get_untracked().trim().to_owned();
        let settings = shell.services().settings;
        if settings.read_untracked(|settings| settings.collab_display_name == name) != Some(false) {
            return;
        }
        settings.edit(keys::COLLAB_DISPLAY_NAME, |settings| {
            settings.collab_display_name = name.clone();
            name
        });
    }

    fn named(self) -> bool {
        !self.name.with(|name| name.trim().is_empty())
    }

    /// Starts hosting, after the address notice.
    fn host(self, shell: Shell) {
        before_revealing_address(shell, move || {
            self.adopt_name(shell);
            let name = self.name.get_untracked();
            let runtime = shell.services().runtime();
            self.change(shell, |state| {
                state.display_name = name.trim().to_owned();
                match runtime {
                    Some(runtime) => state.host(toolkit_version(), runtime),
                    None => state.failure = Some(SessionFailure::NoRuntime),
                }
            });
        });
    }

    /// Joins the session the typed token names, after the address notice.
    fn join(self, shell: Shell) {
        before_revealing_address(shell, move || {
            self.adopt_name(shell);
            let name = self.name.get_untracked();
            let token = self.token.get_untracked();
            let runtime = shell.services().runtime();
            self.change(shell, |state| {
                state.display_name = name.trim().to_owned();
                match runtime {
                    Some(runtime) => state.join(token, toolkit_version(), runtime),
                    None => state.failure = Some(SessionFailure::NoRuntime),
                }
            });
        });
    }

    fn leave(self, shell: Shell) {
        self.change(shell, CollabState::leave);
    }

    /// Puts the session's state in `edit`'s hands, for a test that has no peer task to drive it.
    #[cfg(test)]
    pub(crate) fn edit_for_test(self, shell: Shell, edit: impl FnOnce(&mut CollabState)) {
        self.change(shell, edit);
    }
}

impl Default for CollabHost {
    fn default() -> Self {
        Self::new()
    }
}

/// `StoredValue::update_value` with the closure's result handed back.
trait UpdateValueReturning<T> {
    fn update_value_returning<R>(&self, update: impl FnOnce(&mut T) -> R) -> R;
}

impl<T: 'static> UpdateValueReturning<T> for StoredValue<T, LocalStorage> {
    fn update_value_returning<R>(&self, update: impl FnOnce(&mut T) -> R) -> R {
        let mut out = None;
        self.update_value(|value| out = Some(update(value)));
        out.expect("a stored value outlives the shell that reads it")
    }
}

/// Puts a session notice on screen at its own level.
fn say(shell: Shell, notice: &SessionNotice) {
    let said = notice.text();
    let toasts = shell.toasts();
    match notice.level() {
        NoticeLevel::Info => toasts.info(said),
        NoticeLevel::Warn => toasts.warn(said),
        NoticeLevel::Failed => toasts.failed(said),
    };
}

/// What this build calls itself to its peers.
fn toolkit_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Runs `go`, asking first: hosting or joining tells every peer where this machine is. A reader
/// who switched the notice off is not asked.
fn before_revealing_address(shell: Shell, go: impl Fn() + Copy + 'static) {
    let settings = shell.services().settings;
    // Settings not read yet count as not suppressed: asking is the side that loses nothing.
    if settings.read_untracked(|settings| settings.suppress_p2p_ip_warning) == Some(true) {
        go();
        return;
    }

    let notice = Notice::Address;
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        let ticked = Signal::derive_local(move || {
            let suppressed = settings.read(|settings| settings.suppress_p2p_ip_warning) == Some(true);
            if suppressed { Checked::Yes } else { Checked::No }
        });
        // Written as it is ticked, as the egui dialog's own checkbox is: ticking and then
        // cancelling suppresses the notice in every front end.
        let suppress = move |checked: Checked| {
            let on = checked == Checked::Yes;
            settings.edit(notice.setting_key(), |settings| {
                settings.suppress_p2p_ip_warning = on;
                on
            });
        };
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "notice-address") {
                    AlertDialogHeader {
                        AlertDialogTitle {{notice.title()}}
                        AlertDialogDescription {{notice.message()}}
                    }
                    row(class = "wt-collab__check") {
                        Checkbox(
                            checked = Binding::controlled(ticked, suppress),
                            attr:data-testid = "notice-suppress",
                            a11y:label = t!("ui.labels.suppress_warning").into_owned()
                        )
                        text {{t!("ui.labels.suppress_warning").into_owned()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel(size = ButtonSize::Sm, attr:data-testid = "notice-cancel") {
                            {t!("ui.buttons.cancel").into_owned()}
                        }
                        AlertDialogAction(size = ButtonSize::Sm, attr:data-testid = "notice-continue", on:click = move |_| go()) {
                            {t!("ui.buttons.continue_").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// What the popover shows, which only changes when the session moves between them. Each is built
/// once per entry, so a field being typed into keeps its focus while the session changes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Waiting,
    Active,
    Failed(String),
}

impl Phase {
    fn of(status: SessionStatus) -> Self {
        match status {
            SessionStatus::Idle => Self::Idle,
            SessionStatus::Starting | SessionStatus::Connecting => Self::Waiting,
            SessionStatus::Active => Self::Active,
            SessionStatus::Error(reason) => Self::Failed(reason),
        }
    }
}

/// The session control on the status bar, while the replays tab is selected: a session belongs to
/// the replays workspace it was started from.
#[component]
pub fn SessionControl(shell: Shell) -> impl IntoView {
    view! {
        if move || shell.selected() == Some(TabKey::App(AppTab::ReplayInspector)) {
            SessionPopover(shell = shell)
        } else {}
    }
}

/// The session button and the popover above it. The button carries the session's state: a running
/// session is what the reader most needs to know about, so it is on the control rather than behind
/// a hover.
#[component]
fn SessionPopover(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let collab = shell.collab;
    let locale = shell.services().locale;
    let active = move || collab.is_active();
    let tip = move || {
        locale.track();
        if active() { t!("ui.collab.session_active").into_owned() } else { t!("ui.collab.session").into_owned() }
    };
    let phase = Memo::new(move |_| Phase::of(collab.read(CollabState::status)));
    let open = RwSignal::new_local(false);
    // The tip opens above the button, where the popover is: it would cover the popover and take
    // the presses meant for it.
    let tip_wanted = RwSignal::new_local(false);
    let tip_open = Signal::derive_local(move || tip_wanted.get() && !open.get());

    view! {
        // The popover inside the tooltip's trigger: the trigger reads the innermost overlay, so
        // the other way round a press would toggle the tooltip.
        Tooltip(open = Binding::controlled(tip_open, move |on: bool| tip_wanted.set(on))) {
            TooltipTrigger {
                Popover(open = open) {
                    PopoverTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        class = "wt-session-toggle",
                        class:wt-session-toggle-active = active,
                        attr:data-testid = "collab-session-toggle",
                        a11y:label = tip
                    ) {
                        AppIcon(glyph = Glyph::Radio)
                    }
                    // The control sits on the bar at the bottom of the window, so the popover
                    // opens above.
                    PopoverContent(
                        placement = Placement::new(Side::Top, Align::Start),
                        class = "wt-collab-popover",
                        attr:data-testid = "collab-session-body"
                    ) {
                        column(class = "wt-collab") {
                            {move || match phase.get() {
                                Phase::Idle => AnyView::new(view! { SessionStart(shell = shell) }),
                                Phase::Waiting => AnyView::new(view! { SessionWaiting(shell = shell) }),
                                Phase::Active => AnyView::new(view! { SessionActive(shell = shell) }),
                                Phase::Failed(reason) => AnyView::new(view! { SessionFailed(shell = shell, reason = reason) }),
                            }}
                        }
                    }
                }
            }
            TooltipContent {{tip}}
        }
    }
}

/// With no session running: a name, then host or join. Nothing starts without a name, which every
/// other peer reads.
#[component]
fn SessionStart(shell: Shell) -> impl IntoView {
    let collab = shell.collab;
    let locale = shell.services().locale;
    let unnamed = Signal::derive_local(move || !collab.named());
    let failure = move || collab.read(|state| state.failure.as_ref().map(SessionFailure::text));
    let adopt_on_enter = move |ev: &zgui::vocab::KeyEvent| {
        if ev.key == Key::Named(NamedKey::Enter) {
            collab.adopt_name(shell);
        }
    };

    view! {
        text(class = "wt-collab__title") {{locale.text("ui.collab.display_name")}}
        Input(
            value = collab.name,
            placeholder = t!("ui.collab.display_name_hint").into_owned(),
            label = t!("ui.collab.display_name").into_owned(),
            attr:data-testid = "collab-display-name",
            on:key_down = move |ev| adopt_on_enter(ev),
            on:focus_out = move |_| collab.adopt_name(shell)
        )
        if move || unnamed.get() {
            text(class = "wt-collab__hint") {{locale.text("ui.collab.enter_display_name")}}
        } else {}
        Separator()
        text(class = "wt-collab__title") {{locale.text("ui.collab.host_session")}}
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            disabled = unnamed,
            attr:data-testid = "collab-host",
            on:click = move |_| collab.host(shell)
        ) {
            {locale.text("ui.collab.start_session")}
        }
        Separator()
        text(class = "wt-collab__title") {{locale.text("ui.collab.join_session")}}
        Input(
            value = collab.token,
            placeholder = "toolkit-...".to_owned(),
            label = t!("ui.collab.join_session").into_owned(),
            attr:data-testid = "collab-token"
        )
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            disabled = unnamed,
            attr:data-testid = "collab-join",
            on:click = move |_| collab.join(shell)
        ) {
            {locale.text("ui.collab.paste_and_join")}
        }
        {move || failure().map(|reason| view! {
            text(class = "wt-collab__error", attr:data-testid = "collab-failure") {{reason}}
        })}
    }
}

/// The session is coming up. Nothing can be done with it yet but abandon it.
#[component]
fn SessionWaiting(shell: Shell) -> impl IntoView {
    let locale = shell.services().locale;

    view! {
        row(class = "wt-collab__row") {
            Spinner(label = t!("ui.collab.starting").into_owned())
            text(class = "wt-collab__grow") {{locale.text("ui.collab.starting")}}
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "collab-cancel",
                on:click = move |_| shell.collab.leave(shell)
            ) {
                {locale.text("ui.buttons.cancel")}
            }
        }
    }
}

#[component]
fn SessionFailed(shell: Shell, reason: String) -> impl IntoView {
    let locale = shell.services().locale;

    view! {
        text(class = "wt-collab__error", attr:data-testid = "collab-failure") {{reason}}
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            attr:data-testid = "collab-dismiss",
            on:click = move |_| shell.collab.leave(shell)
        ) {
            {locale.text("ui.collab.leave")}
        }
    }
}

/// A running session: who is in it, what they may do, and how to leave.
#[component]
fn SessionActive(shell: Shell) -> impl IntoView {
    let collab = shell.collab;
    let locale = shell.services().locale;
    let heading = move || {
        locale.track();
        if collab.read(CollabState::is_hosting) {
            t!("ui.collab.session_active").into_owned()
        } else {
            t!("ui.collab.connected_to_session").into_owned()
        }
    };
    let count = move || {
        locale.track();
        t!("ui.collab.connected_count", count = collab.read(CollabState::connected).len()).into_owned()
    };
    let may_steer = Memo::new(move |_| collab.read(CollabState::may_steer));
    // Only the host raises peers: the session ignores a co-host asking to.
    let hosting = Memo::new(move |_| collab.read(|state| state.state.lock().role == PeerRole::Host));

    view! {
        row(class = "wt-collab__head") {
            text(class = "wt-collab__heading") {{heading}}
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "collab-leave",
                on:click = move |_| collab.leave(shell)
            ) {
                {locale.text("ui.collab.leave")}
            }
        }
        text(class = "wt-collab__hint") {{count}}
        // Only a host has a token to hand out; a joiner already used one.
        if move || collab.read(|state| state.token().is_some()) {
            SessionToken(shell = shell)
        } else {}
        Separator()
        // The board is on its own button rather than among the shared windows: it is drawn from
        // what the session says about it rather than from any replay.
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            class = "wt-collab__wide",
            attr:data-testid = "collab-tactics-board",
            on:click = move |_| shell.tactics.open()
        ) {
            {locale.text("ui.collab.tactics_board")}
        }
        if move || collab.read(|state| !state.state.lock().open_replays.is_empty()) {
            Separator()
            text(class = "wt-collab__title") {{locale.text("ui.collab.shared_windows")}}
            // Ids only: a shared window carries its map art, which is not copied on every wake.
            for replay_id in move || collab.read(|state| state.state.lock().open_replays.iter().map(|replay| replay.replay_id).collect::<Vec<u64>>()), key = |replay_id: &u64| *replay_id {
                SharedWindowRow(shell = shell, replay_id = replay_id, may_steer = may_steer)
            }
        } else {}
        Separator()
        for entry in move || collab.read(CollabState::connected).into_iter().enumerate().collect::<Vec<_>>(), key = |entry: &(usize, ConnectedPeer)| (entry.1.user_id, format!("{:?}", entry.1.role), entry.1.is_me) {
            PeerRow(shell = shell, index = entry.0, peer = entry.1, hosting = hosting)
        }
        // Only whoever may steer sees the locks; for everyone else they are someone else's
        // decision, not a control.
        if move || may_steer.get() {
            SessionLocks(shell = shell)
        } else {}
    }
}

/// The host's token, masked unless revealed, and the ways to hand it out.
#[component]
fn SessionToken(shell: Shell) -> impl IntoView {
    let collab = shell.collab;
    let locale = shell.services().locale;
    let shown = move || collab.read(CollabState::token_display).unwrap_or_default();
    let revealed = move || collab.read(|state| state.token_revealed);
    let reveal_tip = t!("ui.collab.toggle_visibility").into_owned();
    let copy = move |value: Option<String>, said: &'static str| {
        let Some(value) = value else { return };
        shell.copy_text(value);
        shell.toasts().info(t!(said).into_owned());
    };

    view! {
        Separator()
        text(class = "wt-collab__hint") {{locale.text("ui.collab.session_token")}}
        row(class = "wt-collab__row") {
            text(class = "wt-collab__token", attr:data-testid = "collab-token-shown") {{shown}}
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconSm,
                attr:data-testid = "collab-reveal",
                a11y:label = reveal_tip,
                on:click = move |_| collab.change(shell, |state| state.token_revealed = !state.token_revealed)
            ) {
                AppIcon(glyph = Signal::derive_local(move || if revealed() { Glyph::EyeOff } else { Glyph::Eye }))
            }
        }
        row(class = "wt-collab__row wt-collab__wrap") {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Xs,
                attr:data-testid = "collab-copy-token",
                on:click = move |_| copy(collab.peek(CollabState::token), "ui.collab.token_copied")
            ) {
                {locale.text("ui.collab.copy_token")}
            }
            if move || collab.read(CollabState::web_link).is_some() {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "collab-copy-web-link",
                    on:click = move |_| copy(collab.peek(CollabState::web_link), "ui.collab.web_link_copied")
                ) {
                    {locale.text("ui.collab.copy_web_link")}
                }
            } else {}
            {localhost_copy(shell)}
        }
    }
}

/// The link to a locally served web client, in a debug build. A release build has nothing to
/// point at; the egui popover draws the same button under the same gate.
#[cfg(debug_assertions)]
fn localhost_copy(shell: Shell) -> Option<AnyView> {
    let collab = shell.collab;
    let locale = shell.services().locale;
    let copy = move || {
        let Some(link) = collab.peek(CollabState::localhost_link) else { return };
        shell.copy_text(link);
        shell.toasts().info(t!("ui.collab.localhost_copied").into_owned());
    };
    Some(AnyView::new(view! {
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Xs,
            attr:data-testid = "collab-copy-localhost",
            on:click = move |_| copy()
        ) {
            {locale.text("ui.collab.copy_localhost_link")}
        }
    }))
}

#[cfg(not(debug_assertions))]
fn localhost_copy(_shell: Shell) -> Option<AnyView> {
    None
}

/// One battle the session is on, with a way to watch it: drawn from the frames the end that owns
/// the replay sends, so a reader with no copy of it sees the battle anyway.
#[component]
fn SharedWindowRow(shell: Shell, replay_id: u64, may_steer: Memo<bool>) -> impl IntoView {
    let locale = shell.services().locale;
    let battle = shell.collab.peek(|state| state.shared_windows().into_iter().find(|one| one.replay_id == replay_id));
    let name = battle.as_ref().map(|battle| battle.replay_name.clone()).unwrap_or_default();
    let map = battle.as_ref().map(|battle| battle.map.clone()).unwrap_or_default();
    let watched = StoredValue::new(battle);

    view! {
        row(class = "wt-collab__row", attr:data-testid = format!("collab-shared-{replay_id}"), a11y:label = name.clone()) {
            AppIcon(glyph = Glyph::Monitor)
            column(class = "wt-collab__battle") {
                text(class = "wt-collab__grow") {{name}}
                text(class = "wt-collab__hint wt-collab__grow") {{map}}
            }
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Xs,
                attr:data-testid = format!("collab-open-shared-{replay_id}"),
                on:click = move |_| shell.collab.watch(watched.get_value().into_iter().collect())
            ) {
                {locale.text("ui.collab.open")}
            }
            // Only the end the session watches can tell the rest to open one.
            if move || may_steer.get() {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = format!("collab-open-for-everyone-{replay_id}"),
                    on:click = move |_| shell.collab.peek(|state| state.link().open_for_everyone(replay_id))
                ) {
                    {locale.text("ui.collab.open_for_everyone")}
                }
            } else {}
        }
    }
}

/// One person in the session. A host can raise a peer, but not itself and not another host.
#[component]
fn PeerRow(shell: Shell, index: usize, peer: ConnectedPeer, hosting: Memo<bool>) -> impl IntoView {
    let locale = shell.services().locale;
    let role = match peer.role {
        PeerRole::Host => Some("ui.collab.role_host"),
        PeerRole::CoHost => Some("ui.collab.role_cohost"),
        PeerRole::Peer => None,
    };
    let raisable = matches!(peer.role, PeerRole::Peer) && !peer.is_me;
    let user_id = peer.user_id;

    view! {
        row(class = "wt-collab__row", attr:data-testid = format!("collab-peer-{index}")) {
            text(class = "wt-collab__grow") {{peer.name.clone()}}
            if move || peer.is_me {
                text(class = "wt-collab__hint") {{locale.text("ui.collab.you")}}
            } else {}
            {role.map(|role| view! { text(class = "wt-collab__hint") {{locale.text(role)}} })}
            if move || raisable && hosting.get() {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = format!("collab-promote-{index}"),
                    on:click = move |_| shell.collab.peek(|state| state.promote(user_id))
                ) {
                    {locale.text("ui.collab.promote_cohost")}
                }
            } else {}
        }
    }
}

/// What peers may change, and a way to return their display settings to the host's.
#[component]
fn SessionLocks(shell: Shell) -> impl IntoView {
    let collab = shell.collab;
    let locale = shell.services().locale;
    let permissions = move || collab.read(CollabState::permissions);
    let checked = |on: bool| if on { Checked::Yes } else { Checked::No };
    let annotations = Signal::derive_local(move || checked(permissions().annotations_locked));
    let settings = Signal::derive_local(move || checked(permissions().settings_locked));
    let set = move |edit: fn(&mut Permissions, bool), on: Checked| {
        let mut next = collab.state.with_value(CollabState::permissions);
        edit(&mut next, on == Checked::Yes);
        collab.state.with_value(|state| state.set_permissions(next));
    };

    view! {
        Separator()
        text(class = "wt-collab__title") {{locale.text("ui.collab.permissions")}}
        row(class = "wt-collab__check") {
            Checkbox(
                checked = Binding::controlled(annotations, move |on: Checked| set(|p, on| p.annotations_locked = on, on)),
                attr:data-testid = "collab-lock-annotations",
                a11y:label = t!("ui.collab.lock_annotations").into_owned()
            )
            text {{locale.text("ui.collab.lock_annotations")}}
        }
        row(class = "wt-collab__check") {
            Checkbox(
                checked = Binding::controlled(settings, move |on: Checked| set(|p, on| p.settings_locked = on, on)),
                attr:data-testid = "collab-lock-settings",
                a11y:label = t!("ui.collab.lock_settings").into_owned()
            )
            text {{locale.text("ui.collab.lock_settings")}}
        }
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "collab-reset-overrides",
                    on:click = move |_| collab.state.with_value(CollabState::reset_overrides)
                ) {
                    {locale.text("ui.collab.reset_overrides")}
                }
            }
            TooltipContent {{locale.text("ui.collab.reset_overrides_tooltip")}}
        }
    }
}

#[cfg(test)]
mod tests;

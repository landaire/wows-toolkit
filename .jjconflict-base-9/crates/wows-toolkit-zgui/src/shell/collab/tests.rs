use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wt_collab_client::ConnectedUser;
use wt_collab_client::OpenReplay;
use wt_collab_client::PeerRole;
use wt_collab_client::SessionStatus;
use wt_collab_client::protocol::ClientType;
use zgui::component;
use zgui::geom::DevicePx;
use zgui::view;
use zgui::view::IntoView;

use crate::shell::Shell;
use crate::shell::ShellHarnessProps;
use crate::shell::dialogs::DialogHostProps;
use crate::shell::settings::tests::test_settings;
use crate::shell::status_bar::ShellStatusBarProps;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;

/// The shell that `Bar` was built in, for the test to steer.
type Caught = Rc<Cell<Option<Shell>>>;

/// The status bar along the bottom of an otherwise empty page.
#[component]
fn Bar(caught: Caught) -> impl IntoView {
    let shell = Shell::current();
    caught.set(Some(shell));
    view! {
        column(style = "width: 100%; height: 100%") {
            box(style = "flex: 1")
            ShellStatusBar(shell = shell)
            DialogHost(dialogs = shell.dialogs)
        }
    }
}

fn open_with(theme: ThemeChoice, options: HeadlessOptions) -> (HeadlessWindow, Shell) {
    let caught: Caught = Rc::new(Cell::new(None));
    let inner = Rc::clone(&caught);
    let mut window = HeadlessWindow::open(options, move || {
        let caught = Rc::clone(&inner);
        view! { ShellHarness(theme = theme) { Bar(caught = caught) } }
    })
    .expect("the status bar opens headlessly");
    window.advance(Duration::from_millis(100));
    let shell = caught.get().expect("the bar is built");

    (window, shell)
}

fn open() -> (HeadlessWindow, Shell) {
    open_with(ThemeChoice::Dark, HeadlessOptions::new(WindowSize { width: 900.0, height: 700.0 }))
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let found = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is on screen"));
    window.click(found).expect("the element takes a click");
    window.advance(Duration::from_millis(300));
}

fn user(id: u64, name: &str, role: PeerRole) -> ConnectedUser {
    ConnectedUser {
        id,
        name: name.to_owned(),
        color: [200, 80, 80],
        role,
        client_type: ClientType::Desktop { toolkit_version: "0.0.0".to_owned() },
    }
}

/// A hosted session with a guest and one battle on, as the peer task would leave it.
fn host_a_session(shell: Shell) {
    shell.collab.edit_for_test(shell, |collab| {
        let mut state = collab.state.lock();
        state.status = SessionStatus::Active;
        state.role = PeerRole::Host;
        state.my_user_id = 1;
        state.token = Some("toolkit-abcdef".to_owned());
        state.connected_users = vec![user(1, "Host", PeerRole::Host), user(2, "Guest", PeerRole::Peer)];
        state.open_replays = vec![OpenReplay {
            replay_id: 7,
            replay_name: "20260101_000000_PASC020-Des-Moines_16_OC_bees_to_honey".to_owned(),
            map_image_png: Vec::new(),
            game_version: "15.0".to_owned(),
            map_name: "16_OC_bees_to_honey".to_owned(),
            display_name: "Sleeping Giant".to_owned(),
        }];
    });
}

/// The session belongs to the replays workspace it was started from, so its control is on the
/// bar there and nowhere else.
#[test]
fn the_session_control_is_on_the_replay_tab_only() {
    let (mut window, shell) = open();
    assert!(window.by_test_id("collab-session-toggle").is_some(), "the session control is on the bar");

    shell.select(TabKey::App(AppTab::Launcher));
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("collab-session-toggle").is_none(), "no session control away from the replays");

    shell.select(TabKey::App(AppTab::ReplayInspector));
    window.advance(Duration::from_millis(100));
    assert!(window.by_test_id("collab-session-toggle").is_some(), "and back with them");
}

/// With no session the popover offers a name and the two ways to start one, and starts neither
/// until a name is given: every peer reads that name.
#[test]
fn the_session_popover_starts_nothing_without_a_display_name() {
    let (mut window, shell) = open();
    click(&mut window, "collab-session-toggle");

    assert!(window.by_test_id("collab-display-name").is_some(), "a name is asked for first");
    assert!(window.by_test_id("collab-token").is_some());
    click(&mut window, "collab-host");
    click(&mut window, "collab-join");

    assert!(window.by_test_id("notice-address").is_none(), "nothing is asked without a name");
    assert!(!shell.collab.is_active(), "nothing started without a name to start it under");
}

/// Hosting tells every peer where this machine is, which the reader is asked about first. Carrying
/// on with no runtime to run a session on says so under the controls.
#[test]
fn hosting_asks_about_the_address_first() {
    let (mut window, shell) = open();
    click(&mut window, "collab-session-toggle");
    click(&mut window, "collab-display-name");
    window.type_text("Reader");
    window.advance(Duration::from_millis(100));

    click(&mut window, "collab-host");
    assert!(window.by_test_id("notice-address").is_some(), "the address notice comes first");
    assert!(!shell.collab.is_active());

    click(&mut window, "notice-continue");
    assert!(window.by_test_id("notice-address").is_none());
    assert!(!shell.collab.is_active(), "the harness has no runtime to run a session on");
    // A press on the notice is outside the popover, which may have closed under it.
    if window.by_test_id("collab-session-body").is_none() {
        click(&mut window, "collab-session-toggle");
    }
    assert!(window.by_test_id("collab-failure").is_some(), "the failure is said under the controls");
}

/// A host sees the token masked, the board, the battle on, who is in, and the locks; a guest can
/// be raised, the host cannot.
#[test]
fn a_hosted_session_shows_its_token_roster_and_locks() {
    let (mut window, shell) = open();
    host_a_session(shell);
    click(&mut window, "collab-session-toggle");

    assert!(window.by_test_id("collab-leave").is_some());
    let token = window.by_test_id("collab-token-shown").expect("the token is shown");
    let masked = window.a11y(token).and_then(|state| state.label).unwrap_or_default();
    assert!(!masked.contains("toolkit"), "the token starts masked, not {masked:?}");
    assert!(window.by_test_id("collab-copy-token").is_some());
    assert!(window.by_test_id("collab-copy-web-link").is_some());
    assert!(window.by_test_id("collab-tactics-board").is_some());
    assert!(window.by_test_id("collab-shared-7").is_some(), "the battle on is listed");
    assert!(window.by_test_id("collab-open-for-everyone-7").is_some(), "a host may open it for everyone");
    assert!(window.by_test_id("collab-promote-0").is_none(), "the host is not raised");
    assert!(window.by_test_id("collab-promote-1").is_some(), "a guest can be");
    assert!(window.by_test_id("collab-lock-annotations").is_some());
    assert!(window.by_test_id("collab-reset-overrides").is_some());

    click(&mut window, "collab-open-shared-7");
    let asked = shell.collab.take_watch_requests();
    assert_eq!(asked.iter().map(|shared| shared.replay_id).collect::<Vec<_>>(), [7], "open asks the replays tab");

    click(&mut window, "collab-leave");
    assert!(window.by_test_id("collab-session-body").is_some(), "the popover stays open");
    assert!(window.by_test_id("collab-host").is_some(), "leaving returns to the start");
}

/// A guest steers nothing: no locks, no raising, no opening for everyone.
#[test]
fn a_guest_steers_nothing() {
    let (mut window, shell) = open();
    host_a_session(shell);
    shell.collab.edit_for_test(shell, |collab| {
        let mut state = collab.state.lock();
        state.role = PeerRole::Peer;
        state.my_user_id = 2;
        state.token = None;
    });
    click(&mut window, "collab-session-toggle");

    assert!(window.by_test_id("collab-leave").is_some());
    assert!(window.by_test_id("collab-copy-token").is_none(), "a guest has no token to hand out");
    assert!(window.by_test_id("collab-open-shared-7").is_some(), "a guest can watch");
    assert!(window.by_test_id("collab-open-for-everyone-7").is_none());
    assert!(window.by_test_id("collab-promote-0").is_none());
    assert!(window.by_test_id("collab-lock-annotations").is_none());
}

/// A window the host opens for everyone is watched here unless the reader turned that off, and an
/// ask declined is not answered later when the setting changes back.
#[test]
fn a_window_opened_for_everyone_follows_the_auto_open_setting() {
    let (_window, shell) = open();
    host_a_session(shell);
    let ask = |declined: bool| {
        let settings = StartupSettings { disable_auto_open_session_windows: declined, ..test_settings() };
        shell.services().settings.adopt_unsaved(settings);
        shell.collab.edit_for_test(shell, |collab| {
            collab.state.lock().force_open_window_ids.insert(7);
        });
        shell.collab.poll(shell);
        shell.collab.take_watch_requests().into_iter().map(|window| window.replay_id).collect::<Vec<_>>()
    };

    assert_eq!(ask(true), Vec::<u64>::new(), "the reader turned auto-opening off");
    shell.collab.poll(shell);
    assert!(shell.collab.take_watch_requests().is_empty(), "the declined ask is gone");
    assert_eq!(ask(false), vec![7], "auto-opening is on");
}

/// The popover opens above its button: the button is on the bar at the bottom of the window.
#[test]
fn the_session_popover_opens_above_the_status_bar() {
    let (mut window, _shell) = open();
    click(&mut window, "collab-session-toggle");

    let toggle = window.by_test_id("collab-session-toggle").and_then(|found| window.bounds(found)).expect("a toggle");
    let body = window.by_test_id("collab-session-body").and_then(|found| window.bounds(found)).expect("a popover");
    assert!(body.bottom() <= toggle.top(), "the popover {body:?} is above its button {toggle:?}");
    assert!(body.top() >= DevicePx(0.0) && body.left() >= DevicePx(0.0), "the popover {body:?} is inside the window");
}

/// Draws the popover with no session and with a hosted one, in both palettes, for review.
#[test]
#[ignore = "draws on the graphics device; run by hand to review the popover"]
fn the_session_popover_renders_in_dark_and_light() {
    for (theme, appearance, name) in
        [(ThemeChoice::Dark, Appearance::Dark, "dark"), (ThemeChoice::Light, Appearance::Light, "light")]
    {
        let options = HeadlessOptions {
            drawing: Drawing::Device,
            desktop: Some(appearance),
            ..HeadlessOptions::new(WindowSize { width: 900.0, height: 700.0 })
        };
        let (mut window, shell) = open_with(theme, options);
        click(&mut window, "collab-session-toggle");
        let idle = crate::interaction_tests::save_shot(&mut window, &format!("collab_idle_{name}"));

        click(&mut window, "collab-session-toggle");
        host_a_session(shell);
        click(&mut window, "collab-session-toggle");
        let hosted = crate::interaction_tests::save_shot(&mut window, &format!("collab_hosted_{name}"));
        eprintln!("{} {}", idle.display(), hosted.display());
    }
}

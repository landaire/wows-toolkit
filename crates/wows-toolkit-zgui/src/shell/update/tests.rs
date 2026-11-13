//! The update check and install against releases the tests publish themselves.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use futures::future::LocalBoxFuture;
use landaire_ui::test_support::HeadlessWindow;
use rust_i18n::t;
use wows_toolkit_services::update::Downloaded;
use wows_toolkit_services::update::UpdateError;
use wows_toolkit_viewmodel::update::Release;
use zgui::reactive::prelude::GetUntracked;

use super::Asked;
use super::ReleaseSource;
use super::UpdateStatus;
use super::check_for_update;
use super::install_update;
use crate::interaction_tests::open_bare_shell;
use crate::interaction_tests::shell;
use crate::services::Job;

type Answer<T> = oneshot::Sender<Result<T, rootcause::Report>>;

/// An install the test answers.
struct Install {
    asset_url: String,
    progress: UnboundedSender<Downloaded>,
    answer: Answer<()>,
}

/// Releases whose checks and installs wait for the test to answer them.
#[derive(Default)]
struct FakeReleases {
    checks: RefCell<Vec<Answer<Option<Release>>>>,
    installs: RefCell<Vec<Install>>,
}

impl FakeReleases {
    fn answer_check(&self, answer: Result<Option<Release>, rootcause::Report>) {
        let pending = self.checks.borrow_mut().remove(0);
        let _ = pending.send(answer);
    }
}

/// The source a test hands the flow, sharing the test's own record of what was asked.
struct Shared(Rc<FakeReleases>);

impl ReleaseSource for Shared {
    fn check(
        &self,
        _services: crate::services::Services,
    ) -> LocalBoxFuture<'static, Result<Option<Release>, rootcause::Report>> {
        let (answer, answered) = oneshot::channel();
        self.0.checks.borrow_mut().push(answer);
        Box::pin(async move { answered.await.unwrap_or_else(|_| Err(network_failure())) })
    }

    fn install(
        &self,
        _services: crate::services::Services,
        asset_url: String,
        progress: UnboundedSender<Downloaded>,
    ) -> LocalBoxFuture<'static, Result<(), rootcause::Report>> {
        let (answer, answered) = oneshot::channel();
        self.0.installs.borrow_mut().push(Install { asset_url, progress, answer });
        Box::pin(async move { answered.await.unwrap_or_else(|_| Err(network_failure())) })
    }
}

fn network_failure() -> rootcause::Report {
    std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused").into()
}

fn release(body: Option<&str>) -> Release {
    Release {
        tag_name: "v99.0.0".to_owned(),
        html_url: "https://example.invalid/releases/v99.0.0".to_owned(),
        draft: false,
        prerelease: false,
        body: body.map(str::to_owned),
        assets: Vec::new(),
    }
}

/// The shell, with its releases looked up in a fake the test answers.
fn open() -> (HeadlessWindow, Rc<FakeReleases>) {
    let window = open_bare_shell();
    let releases = Rc::new(FakeReleases::default());
    shell().update.use_source(Rc::new(Shared(Rc::clone(&releases))));

    (window, releases)
}

fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(200));
}

fn jobs() -> Vec<Job> {
    shell().services().jobs.running()
}

fn error_text(window: &HeadlessWindow) -> String {
    window.texts_by_class("wt-error__text").concat()
}

#[test]
fn a_check_by_hand_that_cannot_reach_the_server_says_why() {
    let (mut window, releases) = open();
    check_for_update(shell().services(), shell(), Asked::ByHand);
    settle(&mut window);
    assert_eq!(jobs().len(), 1, "the check is on the status bar while it runs");

    releases.answer_check(Err(network_failure()));
    settle(&mut window);
    assert!(jobs().is_empty());
    let said = error_text(&window);
    assert!(said.contains(t!("ui.messages.update_check_failed").as_ref()), "{said}");
    assert!(said.contains("connection refused"), "{said}");
}

#[test]
fn a_startup_check_that_fails_says_nothing() {
    let (mut window, releases) = open();
    check_for_update(shell().services(), shell(), Asked::AtStartup);
    settle(&mut window);
    assert!(jobs().is_empty(), "a check nobody asked for is not shown");

    releases.answer_check(Err(network_failure()));
    settle(&mut window);
    assert!(window.by_test_id("notice-error").is_none());
}

#[test]
fn a_check_by_hand_during_the_startup_check_is_shown_and_answered() {
    let (mut window, releases) = open();
    check_for_update(shell().services(), shell(), Asked::AtStartup);
    check_for_update(shell().services(), shell(), Asked::ByHand);
    settle(&mut window);
    assert_eq!(releases.checks.borrow().len(), 1, "the second ask joins the running check");
    assert_eq!(jobs().len(), 1, "the job is shown for the ask by hand");

    // The startup check failing is now the reader's to hear.
    releases.answer_check(Err(network_failure()));
    settle(&mut window);
    assert!(jobs().is_empty());
    assert!(error_text(&window).contains("connection refused"));
}

#[test]
fn a_check_by_hand_that_finds_nothing_says_the_app_is_up_to_date() {
    let (mut window, releases) = open();
    check_for_update(shell().services(), shell(), Asked::ByHand);
    settle(&mut window);
    releases.answer_check(Ok(None));
    settle(&mut window);

    let toasts = window.texts_by_class("zui-toast").concat();
    assert!(toasts.contains(t!("ui.messages.app_up_to_date").as_ref()), "{toasts}");
}

#[test]
fn a_newer_release_is_offered_with_its_notes_set_out() {
    let (mut window, releases) = open();
    check_for_update(shell().services(), shell(), Asked::AtStartup);
    settle(&mut window);
    releases.answer_check(Ok(Some(release(Some("## Fixes\n\n- **Replays** load faster")))));
    settle(&mut window);

    assert!(window.by_test_id("update-offer").is_some());
    let version = window.texts_by_class("wt-update__version").concat();
    assert!(version.contains("v99.0.0"), "{version}");
    let heading = window.texts_by_class("lui-markdown__heading");
    assert_eq!(heading.first().map(String::as_str), Some("Fixes"), "the notes are read as Markdown, not as written");
    let strong = window.texts_by_class("lui-markdown__strong");
    assert_eq!(strong.first().map(String::as_str), Some("Replays"));
    assert!(window.by_test_id("update-view-release").is_some());

    window.click(window.by_test_id("update-close").expect("the offer can be put off")).expect("the button is shown");
    settle(&mut window);
    assert!(window.by_test_id("update-offer").is_none());
}

#[test]
fn an_install_says_how_far_it_got_and_why_it_failed() {
    let (mut window, releases) = open();
    install_update(shell().services(), shell(), "https://example.invalid/wows_toolkit.zip".to_owned());
    settle(&mut window);
    assert_eq!(shell().update.status.get_untracked(), UpdateStatus::Installing);

    let install = releases.installs.borrow_mut().remove(0);
    assert_eq!(install.asset_url, "https://example.invalid/wows_toolkit.zip");
    install.progress.unbounded_send(Downloaded { read: 1024, total: Some(4096) }).expect("the flow listens");
    settle(&mut window);
    let named: Vec<String> = jobs().into_iter().map(|job| job.name).collect();
    assert_eq!(named, [t!("ui.messages.update_downloading", read = "1 KiB", total = "4 KiB").into_owned()]);

    // An archive without the executable, as a bad release asset would be. The install's end
    // closes its progress channel, as the real one's does.
    drop(install.progress);
    let _ = install.answer.send(Err(UpdateError::MissingExecutable.into()));
    settle(&mut window);
    assert!(jobs().is_empty());
    assert_eq!(shell().update.status.get_untracked(), UpdateStatus::Idle);
    let said = error_text(&window);
    assert!(said.contains(&UpdateError::MissingExecutable.to_string()), "{said}");
}

//! This front end's end of `wows_toolkit_services::collab`: waking a view when
//! the peer task changes something, starting a session on the shared runtime,
//! and saying what the session reports.

use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Entity;
use gpui_kit::Window;
use wows_toolkit_services::collab::CollabState;
use wows_toolkit_viewmodel::collab::NoticeLevel;
use wows_toolkit_viewmodel::collab::SessionFailure;
use wows_toolkit_viewmodel::collab::SessionNotice;

/// Redraws `view` whenever the peer task changes the session. Installed once;
/// later calls are no-ops.
pub fn bind<V: 'static>(collab: &CollabState, view: &Entity<V>, cx: &mut App) {
    let Some(mut wakes) = collab.wakes() else { return };

    let view = view.downgrade();
    cx.spawn(async move |cx: &mut AsyncApp| {
        use futures::StreamExt as _;
        while wakes.next().await.is_some() {
            // A view that has gone ends the loop: nothing is drawing the
            // session, so there is nothing to wake.
            if view.update(cx, |_view, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Starts hosting on the shared runtime.
pub fn host(collab: &mut CollabState, toolkit_version: String, cx: &App) {
    match crate::runtime::runtime(cx) {
        Some(runtime) => collab.host(toolkit_version, runtime),
        None => collab.failure = Some(SessionFailure::NoRuntime),
    }
}

/// Joins the session `token` names, on the shared runtime.
pub fn join(collab: &mut CollabState, token: String, toolkit_version: String, cx: &App) {
    match crate::runtime::runtime(cx) {
        Some(runtime) => collab.join(token, toolkit_version, runtime),
        None => collab.failure = Some(SessionFailure::NoRuntime),
    }
}

/// Puts a session notice on screen at its own level.
pub fn say(notice: &SessionNotice, window: &mut Window, cx: &mut App) {
    let said = notice.text();
    match notice.level() {
        NoticeLevel::Info => crate::toast::info(said, window, cx),
        NoticeLevel::Warn => crate::toast::warn(said, window, cx),
        NoticeLevel::Failed => crate::toast::failed(said, window, cx),
    }
}

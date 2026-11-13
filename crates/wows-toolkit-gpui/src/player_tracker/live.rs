//! Hands the changes `wows_toolkit_replay_data::live_match::LivePoll` sees to the tracker.

use std::path::Path;

use futures::StreamExt;
use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Task;
use wows_toolkit_replay_data::live_match::LivePoll;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use wows_toolkit_services::supervision::watchdog::WatchdogError;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;

/// Polls `replay_dir` under `watchdog`, handing each change to `on_change`; `None` means no
/// battle is under way.
///
/// The poll runs until the returned handle is dropped, which is what ends it when the tab's
/// directory changes.
pub fn watch(
    watchdog: &Watchdog,
    replay_dir: &Path,
    cx: &App,
    mut on_change: impl FnMut(Option<LiveMatch>, &mut AsyncApp) -> bool + 'static,
) -> Result<Task<()>, WatchdogError> {
    let (poll, mut changes) = LivePoll::start(watchdog, replay_dir)?;

    Ok(cx.spawn(async move |cx| {
        // Held by the task, so dropping it ends the poll.
        let _poll = poll;
        while let Some(live) = changes.next().await {
            // The view is gone; so is any reason to keep polling.
            if !on_change(live, cx) {
                return;
            }
        }
    }))
}

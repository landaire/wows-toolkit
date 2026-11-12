//! Drives `wows_toolkit_replay_data::live_match::LiveWatch` on a timer.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::AsyncApp;
use gpui_kit::Task;
use wows_toolkit_replay_data::live_match::LiveWatch;
use wows_toolkit_replay_data::live_match::Observed;
use wows_toolkit_replay_data::live_match::POLL_INTERVAL;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;

/// Polls `replay_dir`, handing each change to `on_change`; `None` means no
/// battle is under way.
///
/// The task runs until the returned handle is dropped, which is what ends it
/// when the tab's directory changes.
pub fn watch(
    replay_dir: PathBuf,
    cx: &App,
    mut on_change: impl FnMut(Option<LiveMatch>, &mut AsyncApp) -> bool + 'static,
) -> Task<()> {
    let mut watch = LiveWatch::new(&replay_dir);

    cx.spawn(async move |cx| {
        loop {
            // A step is a blocking `metadata`, and a read when it changed,
            // which on a network share or a spun-down drive would stall a
            // frame if it ran here.
            let observed;
            (watch, observed) = cx
                .background_spawn(async move {
                    let observed = watch.step();
                    (watch, observed)
                })
                .await;

            // The view is gone; so is any reason to keep polling.
            if let Observed::Changed(live) = observed
                && !on_change(live, cx)
            {
                return;
            }

            cx.background_executor().timer(POLL_INTERVAL).await;
        }
    })
}

//! The battle currently in progress.
//!
//! Where it is read from and when it counts as changed are shared
//! (`wows_toolkit_viewmodel::player_tracker::live_source`), as are the roster
//! scan and the stats request (`wows_toolkit_replay_data::live_match`); this
//! drives the poll on the app's executor.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::AsyncApp;
use gpui_kit::Task;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveSource;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveWatcher;
use wows_toolkit_viewmodel::player_tracker::live_source::POLL_INTERVAL;
use wows_toolkit_viewmodel::player_tracker::live_source::Settled;

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
    let source = LiveSource::in_dir(&replay_dir);

    cx.spawn(async move |cx| {
        let mut watcher = LiveWatcher::new();

        loop {
            // `poll` is a blocking `metadata`, which on a network share or a
            // spun-down drive would stall a frame if it ran here.
            let stamp = {
                let source = source.clone();
                cx.background_spawn(async move { wows_toolkit_viewmodel::player_tracker::live_source::poll(&source) })
                    .await
            };

            if watcher.wants_read(stamp.as_ref()) {
                let read_source = source.clone();
                let read = match stamp {
                    Some(_) => Some(
                        cx.background_spawn(async move {
                            wows_toolkit_viewmodel::player_tracker::live_source::read(&read_source)
                        })
                        .await,
                    ),
                    None => None,
                };

                // The view is gone; so is any reason to keep polling.
                if let Settled::Report(live) = watcher.settle(stamp, read)
                    && !on_change(live, cx)
                {
                    return;
                }
            }

            cx.background_executor().timer(POLL_INTERVAL).await;
        }
    })
}

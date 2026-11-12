//! The Replays tab.

pub mod browser;

use zgui::component;
use zgui::reactive::UnsyncCallback;
use zgui::view;
use zgui::view::IntoView;

use crate::shell::ReplayRequest;
use crate::shell::Shell;
use crate::tabs::replays::browser::BrowserControl;
use crate::tabs::replays::browser::ReplayBrowserEvent;
use crate::tabs::replays::browser::ReplayBrowserProps;

#[component]
pub fn ReplaysTab() -> impl IntoView {
    let shell = Shell::current();
    let control = BrowserControl::new();
    let take = move || {
        for request in shell.take_replay_requests() {
            if let ReplayRequest::OpenFolder(root) = request {
                control.open_folder(root);
            }
        }
    };
    let requests = zgui::reactive::RenderEffect::new(move |_| take());
    zgui::reactive::on_cleanup_local(move || drop(requests));
    let on_event = UnsyncCallback::new(move |event: ReplayBrowserEvent| tracing::debug!(?event, "replay browser"));

    view! {
        box(class = "wt-tab-empty", attr:data-testid = "replays-tab", style:width = "320px", style:height = "100%") {
            ReplayBrowser(control = control, on_event = on_event)
        }
    }
}

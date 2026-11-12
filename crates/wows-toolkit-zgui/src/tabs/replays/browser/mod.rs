//! The replays tab's side panel: the replays of the install's directory and of every directory
//! the reader opened, one tab each, as a tree of two-line rows.
//!
//! The container shows [`ReplayBrowser`] and hears what the reader asks of a replay through its
//! `on_event` callback; it routes `Shell::take_replay_requests`'s `OpenFolder` to
//! [`BrowserControl::open_folder`].

mod actions;
mod controls;
pub mod files;
mod folders;
mod header;
mod listing;
#[cfg(test)]
mod tests;
mod thumbnails;
mod tree;
mod view;

use std::path::PathBuf;
use std::rc::Rc;

use wows_toolkit_replay_data::force_reindex::Confirm;
use wows_toolkit_replay_data::force_reindex::ReindexCandidate;
use wows_toolkit_replay_data::replay_dir::MissingBuild;
use zgui::component;
use zgui::reactive::Callable;
use zgui::reactive::UnsyncCallback;
use zgui::view::IntoView;

use crate::tabs::replays::browser::listing::Listing;

pub use crate::tabs::replays::browser::folders::BrowserControl;

/// What the reader asked of the listed replays, for the replays tab to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayBrowserEvent {
    /// Opens the replay in place of the replay tab showing.
    Open(PathBuf),
    OpenInNewTab(PathBuf),
    /// Plays the battle back on the minimap.
    RenderReplay(PathBuf),
    RenderManyToVideo(Vec<PathBuf>),
    RenderManyToClipboard(Vec<PathBuf>),
    /// Makes the replays the session's stats, or adds them to it.
    SessionStats { paths: Vec<PathBuf>, replace: bool },
    /// A watched directory gained a replay. `battle` is its "Ship - Map"; `install` says whether
    /// it landed in the install's directory, which is what autoloading follows.
    ReplayAppeared { path: PathBuf, battle: String, install: bool },
    /// A listed replay was written again.
    ReplayChanged(PathBuf),
    ReplayRemoved(PathBuf),
    /// The listed replays need builds nothing here can read.
    BuildsMissing(Vec<MissingBuild>),
    /// The reader asked which builds the listed replays need.
    CheckGameData(Vec<MissingBuild>),
}

/// Where a listing sends what the reader asks of it.
#[derive(Clone)]
pub struct BrowserEvents {
    send: Rc<dyn Fn(ReplayBrowserEvent)>,
    control: BrowserControl,
}

impl BrowserEvents {
    pub fn send(&self, event: ReplayBrowserEvent) {
        (self.send)(event);
    }

    /// Reads `paths` of `listing` again and rewrites their index entries, once confirmed as
    /// `confirm` asks.
    pub fn force_reindex(&self, listing: Listing, paths: &[PathBuf], confirm: Confirm) {
        let candidates: Vec<ReindexCandidate> =
            listing.reindex_candidates().into_iter().filter(|candidate| paths.contains(&candidate.path)).collect();
        actions::force_reindex(self.control, candidates, confirm);
    }
}

/// The side panel: the header, a tab per directory, and the selected directory's listing.
#[component]
pub fn ReplayBrowser(control: BrowserControl, on_event: UnsyncCallback<ReplayBrowserEvent>) -> impl IntoView {
    let events = BrowserEvents { send: Rc::new(move |event| on_event.run(event)), control };
    folders::FolderPanels(control, events)
}

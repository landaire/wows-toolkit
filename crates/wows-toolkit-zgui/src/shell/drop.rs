//! Files dropped on the window from outside it: one replay opens, one directory is listed.

use std::path::Path;
use std::path::PathBuf;

use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;

use crate::services::Services;

const SHEET: &str = "wt-drop-scrim";

/// White on the scrim's own dark ground, whatever the palette, as the egui overlay is.
const CSS: &str = css!(
    ".wt-drop-scrim {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        z-index: 30;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: rgba(0, 0, 0, 0.75);
        pointer-events: none;
    }
    .wt-drop-scrim__said {
        color: #ffffff;
        font-size: 18px;
    }"
);

/// What dropping a set of paths on the window would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropIntent {
    OpenReplay(PathBuf),
    /// Lists the directory in a folder tab of the replays tab.
    OpenFolder(PathBuf),
    NotAReplay,
    TooMany,
}

impl DropIntent {
    /// What the scrim and the refusal say about it.
    pub fn said(&self) -> String {
        let named = |path: &Path| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();

        match self {
            Self::NotAReplay => t!("ui.messages.drop_not_a_replay").into_owned(),
            Self::OpenReplay(path) => t!("ui.app.drop_to_load", file = named(path)).into_owned(),
            Self::OpenFolder(path) => t!("ui.app.drop_to_list", folder = named(path)).into_owned(),
            Self::TooMany => t!("ui.messages.drop_one_at_a_time").into_owned(),
        }
    }
}

fn is_replay(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
}

/// What a drop of `paths` would do, read from their names alone. `None` when no path names a
/// replay, and [`folder_drop`] has to look at them.
pub fn replay_drop(paths: &[PathBuf]) -> Option<DropIntent> {
    let replays: Vec<&PathBuf> = paths.iter().filter(|path| is_replay(path)).collect();
    match replays.as_slice() {
        [one] => Some(DropIntent::OpenReplay((*one).clone())),
        [_, _, ..] => Some(DropIntent::TooMany),
        [] => None,
    }
}

/// What a drop of `paths` that names no replay would do. Queries the filesystem per path, which
/// can block on an offline share, so it runs off the UI thread.
pub fn folder_drop(paths: &[PathBuf]) -> DropIntent {
    let folders: Vec<&PathBuf> = paths.iter().filter(|path| path.is_dir()).collect();
    match folders.as_slice() {
        [one] => DropIntent::OpenFolder((*one).clone()),
        [_, _, ..] => DropIntent::TooMany,
        [] => DropIntent::NotAReplay,
    }
}

/// Classifies `paths`, looking at the filesystem off the UI thread only when their names do not
/// settle it.
pub async fn classify(paths: Vec<PathBuf>) -> DropIntent {
    if let Some(intent) = replay_drop(&paths) {
        return intent;
    }

    blocking(move || folder_drop(&paths)).await
}

/// Files being dragged over the window, with what dropping them would do.
#[derive(Clone, Debug, PartialEq)]
struct Hovering {
    paths: Vec<PathBuf>,
    /// `None` while the paths are being looked at.
    intent: Option<DropIntent>,
}

/// What the scrim shows: the drag over the window, while there is one.
#[derive(Clone, Copy)]
pub struct DropScrimState {
    hovering: RwSignal<Option<Hovering>, LocalStorage>,
}

impl DropScrimState {
    pub fn new() -> Self {
        Self { hovering: RwSignal::new_local(None) }
    }

    /// Notes the files now being dragged over the window, and looks at what dropping them would
    /// do. Call from a component's listener: the look is a task of that component.
    pub fn enter(self, paths: Vec<PathBuf>) {
        let same =
            self.hovering.with_untracked(|hovering| hovering.as_ref().is_some_and(|hovering| hovering.paths == paths));
        if same {
            return;
        }
        self.hovering.set(Some(Hovering { paths: paths.clone(), intent: None }));

        spawn_local(async move {
            let intent = classify(paths.clone()).await;
            // The drag has moved on to other files, or left.
            self.hovering.update(|hovering| {
                if let Some(hovering) = hovering.as_mut().filter(|hovering| hovering.paths == paths) {
                    hovering.intent = Some(intent);
                }
            });
        });
    }

    /// Takes the scrim down, for a drag that has left the window or landed.
    pub fn leave(self) {
        if self.hovering.get_untracked().is_some() {
            self.hovering.set(None);
        }
    }

    /// What the scrim says. `None` with no drag over the window, and while a directory is being
    /// looked for, which is brief.
    fn said(self) -> Option<String> {
        self.hovering.get()?.intent.map(|intent| intent.said())
    }

    fn is_shown(self) -> bool {
        self.hovering.get().is_some()
    }
}

impl Default for DropScrimState {
    fn default() -> Self {
        Self::new()
    }
}

/// The scrim over the window while files are dragged onto it, saying what letting go would do:
/// without it a drag has nothing to say it will be taken, and a reader dragging two files learns
/// only after letting go that one was expected.
#[component]
pub fn DropScrim(state: DropScrimState) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;

    view! {
        if move || state.is_shown() {
            box(
                class = "wt-drop-scrim",
                attr:data-testid = "app-drop-scrim",
                a11y:label = move || {
                    locale.track();
                    state.said().unwrap_or_default()
                }
            ) {
                text(class = "wt-drop-scrim__said") {{move || {
                    locale.track();
                    state.said().unwrap_or_default()
                }}}
            }
        } else {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_opens_one_replay_or_one_folder() {
        let folder = tempfile::tempdir().expect("a temporary directory");
        let other = tempfile::tempdir().expect("a second temporary directory");
        let folder_path = folder.path().to_path_buf();
        let replay = PathBuf::from("G:/replays/one.wowsreplay");

        assert_eq!(replay_drop(std::slice::from_ref(&folder_path)), None, "a directory has to be looked at");
        assert_eq!(folder_drop(std::slice::from_ref(&folder_path)), DropIntent::OpenFolder(folder_path.clone()));
        assert_eq!(replay_drop(&[folder_path.clone(), replay.clone()]), Some(DropIntent::OpenReplay(replay.clone())));
        assert_eq!(folder_drop(&[folder_path, other.path().to_path_buf()]), DropIntent::TooMany);
        assert_eq!(replay_drop(&[replay.clone(), replay]), Some(DropIntent::TooMany));
        assert_eq!(folder_drop(&[PathBuf::from("G:/notes.txt")]), DropIntent::NotAReplay);
    }
}

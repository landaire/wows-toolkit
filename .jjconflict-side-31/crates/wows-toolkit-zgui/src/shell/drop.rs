//! Files dropped on the window from outside it: one replay opens, several are listed together in
//! a tab of their own, and each directory is listed in its own tab.

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DropIntent {
    /// One opens; several are listed together in one tab, in the order dropped.
    pub replays: Vec<PathBuf>,
    /// Each is listed in a folder tab of the replays tab.
    pub folders: Vec<PathBuf>,
    /// How many paths are neither a replay nor a directory, which the drop leaves alone.
    pub skipped: usize,
}

impl DropIntent {
    /// Sorts `paths` by their names, once each. Returns the paths that name no replay, which
    /// [`Self::look_at`] sorts.
    fn from_names(paths: Vec<PathBuf>) -> (Self, Vec<PathBuf>) {
        let mut intent = Self::default();
        let mut others = Vec::new();
        for path in paths {
            if intent.replays.contains(&path) || others.contains(&path) {
                continue;
            }
            if is_replay(&path) {
                intent.replays.push(path);
            } else {
                others.push(path);
            }
        }

        (intent, others)
    }

    /// Sorts `others`, paths that name no replay, into directories and the rest. Queries the
    /// filesystem per path, which can block on an offline share, so it runs off the UI thread.
    fn look_at(&mut self, others: Vec<PathBuf>) {
        for path in others {
            if path.is_dir() {
                self.folders.push(path);
            } else {
                self.skipped += 1;
            }
        }
    }

    fn takes_nothing(&self) -> bool {
        self.replays.is_empty() && self.folders.is_empty()
    }

    /// What the scrim says letting go would do.
    pub fn said(&self) -> String {
        let named = |path: &Path| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();

        match (self.replays.as_slice(), self.folders.as_slice()) {
            ([], []) if self.skipped > 1 => t!("ui.messages.drop_not_replays").into_owned(),
            ([], []) => t!("ui.messages.drop_not_a_replay").into_owned(),
            ([replay], []) => t!("ui.app.drop_to_load", file = named(replay)).into_owned(),
            (replays, []) => t!("ui.app.drop_to_list_replays", count = replays.len()).into_owned(),
            ([], [folder]) => t!("ui.app.drop_to_list", folder = named(folder)).into_owned(),
            ([], folders) => t!("ui.app.drop_to_list_folders", count = folders.len()).into_owned(),
            (replays, folders) => {
                let folders = match folders {
                    [folder] => named(folder),
                    folders => t!("ui.app.drop_folder_count", count = folders.len()).into_owned(),
                };
                match replays {
                    [replay] => t!("ui.app.drop_to_open_and_list", file = named(replay), folders = folders),
                    replays => {
                        t!("ui.app.drop_to_list_replays_and_folders", count = replays.len(), folders = folders)
                    }
                }
                .into_owned()
            }
        }
    }

    /// What a landed drop says of the paths it left alone: `None` when it left none.
    pub fn refusal(&self) -> Option<String> {
        if self.takes_nothing() {
            return Some(self.said());
        }

        (self.skipped > 0).then(|| t!("ui.messages.drop_skipped", count = self.skipped).into_owned())
    }
}

fn is_replay(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
}

/// Classifies `paths`, looking at the filesystem off the UI thread only when their names do not
/// settle it.
pub async fn classify(paths: Vec<PathBuf>) -> DropIntent {
    let (mut intent, others) = DropIntent::from_names(paths);
    if others.is_empty() {
        return intent;
    }

    blocking(move || {
        intent.look_at(others);
        intent
    })
    .await
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
/// without it a drag has nothing to say it will be taken, and a reader dragging a mix of files
/// learns only after letting go which of them were left alone.
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

    /// Classifies `paths` on this thread.
    fn classified(paths: &[PathBuf]) -> DropIntent {
        let (mut intent, others) = DropIntent::from_names(paths.to_vec());
        intent.look_at(others);
        intent
    }

    #[test]
    fn a_drop_sorts_paths_into_replays_folders_and_the_rest() {
        let folder = tempfile::tempdir().expect("a temporary directory");
        let other = tempfile::tempdir().expect("a second temporary directory");
        let folder_path = folder.path().to_path_buf();
        let other_path = other.path().to_path_buf();
        let one = PathBuf::from("G:/replays/one.wowsreplay");
        let two = PathBuf::from("G:/replays/two.wowsreplay");
        let notes = PathBuf::from("G:/notes.txt");

        assert_eq!(
            DropIntent::from_names(vec![one.clone(), folder_path.clone(), two.clone(), one.clone()]),
            (
                DropIntent { replays: vec![one.clone(), two.clone()], folders: Vec::new(), skipped: 0 },
                vec![folder_path.clone()]
            ),
            "a replay is known by its name, once, and a directory has to be looked at"
        );
        assert_eq!(
            classified(&[folder_path.clone(), two.clone(), notes.clone(), other_path.clone(), one.clone()]),
            DropIntent { replays: vec![two.clone(), one.clone()], folders: vec![folder_path, other_path], skipped: 1 }
        );
        assert_eq!(
            classified(&[notes.clone(), PathBuf::from("G:/more.txt")]),
            DropIntent { replays: Vec::new(), folders: Vec::new(), skipped: 2 }
        );
    }

    #[test]
    fn the_scrim_and_the_refusal_say_what_the_drop_does() {
        let replay = |name: &str| PathBuf::from(format!("G:/replays/{name}.wowsreplay"));
        let folder = |name: &str| PathBuf::from(format!("G:/{name}"));
        let intent = |replays: Vec<PathBuf>, folders: Vec<PathBuf>, skipped| DropIntent { replays, folders, skipped };

        let one = intent(vec![replay("Yamato")], Vec::new(), 0);
        assert_eq!(one.said(), "Drop to load Yamato.wowsreplay");
        assert_eq!(one.refusal(), None, "a drop that takes every path refuses nothing");

        let three = intent(vec![replay("a"), replay("b"), replay("c")], Vec::new(), 2);
        assert_eq!(three.said(), "Drop to list 3 replays");
        assert_eq!(three.refusal().as_deref(), Some("Skipped 2 dropped file(s) that are not replays or folders"));

        assert_eq!(intent(Vec::new(), vec![folder("battles")], 0).said(), "Drop to list the replays in battles");
        assert_eq!(
            intent(Vec::new(), vec![folder("a"), folder("b")], 0).said(),
            "Drop to list the replays in 2 folders"
        );
        assert_eq!(
            intent(vec![replay("a"), replay("b")], vec![folder("battles")], 0).said(),
            "Drop to list 2 replays and the replays in battles"
        );
        assert_eq!(
            intent(vec![replay("Yamato")], vec![folder("a"), folder("b")], 0).said(),
            "Drop to open Yamato.wowsreplay and list the replays in 2 folders"
        );

        let none = intent(Vec::new(), Vec::new(), 1);
        assert_eq!(none.said(), "That is not a replay file");
        assert_eq!(none.refusal().as_deref(), Some("That is not a replay file"));
        assert_eq!(intent(Vec::new(), Vec::new(), 2).said(), "Those are not replay files");
    }
}

//! The replay keyboard reference: what the game's own controls do while a replay plays, read from
//! the loaded build's `commands.scheme.xml`.

use std::io::Read as _;
use std::rc::Rc;

use rust_i18n::t;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::controls::CommandGroup;
use wows_toolkit_viewmodel::controls::parse_commands_scheme;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::ScrollAreaProps;

use crate::shell::Shell;

/// Where the game keeps the scheme.
const SCHEME_PATH: &str = "system/data/commands.scheme.xml";

/// The scheme in `vfs`. `None` when the build carries none, or one the parse makes nothing of.
pub fn read_scheme(vfs: &VfsPath) -> Option<Vec<CommandGroup>> {
    let mut file = vfs.join(SCHEME_PATH).and_then(|path| path.open_file()).ok()?;
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return None;
    }
    let groups = parse_commands_scheme(&bytes);
    (!groups.is_empty()).then_some(groups)
}

/// Shows the reference for the newest loaded build, or says there is none. What the replay
/// browser's "Show replay controls" item runs.
pub fn show_replay_controls(shell: Shell, cache: &GameDataCache) {
    match cache.newest_loaded().and_then(|build| read_scheme(build.vfs())) {
        Some(groups) => open(shell, groups),
        None => {
            shell.toasts().warn(t!("ui.replay.controls.unavailable").into_owned());
        }
    }
}

/// Shows `groups` in a dialog over the window.
pub fn open(shell: Shell, groups: Vec<CommandGroup>) {
    let groups = Rc::new(groups);
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        let groups = StoredValue::new_local(Rc::clone(&groups));
        AnyView::new(view! {
            Dialog(open = open, on_open_change = handle.on_open_change()) {
                DialogContent(class = "wt-rp-controls", attr:data-testid = "replay-controls") {
                    DialogHeader {
                        DialogTitle {{t!("ui.replay.controls.window_title").into_owned()}}
                    }
                    ScrollArea(class = "wt-rp-controls__body") {
                        column(class = "wt-rp-controls__groups") {{move || groups.with_value(|groups| groups.iter().map(group_view).collect::<Vec<_>>())}}
                    }
                }
            }
        })
    });
}

fn group_view(group: &CommandGroup) -> AnyView {
    let title = group.title.to_string();
    let rows: Vec<AnyView> = group
        .commands
        .iter()
        .map(|command| {
            let keys = match &command.key2 {
                Some(second) => format!("{}  /  {}", command.key1, second),
                None => command.key1.clone(),
            };
            let label = command.label.clone();
            AnyView::new(view! {
                row(class = "wt-rp-controls__row") {
                    text(class = "wt-rp-controls__label") {{label}}
                    text(class = "wt-rp-controls__keys") {{keys}}
                }
            })
        })
        .collect();
    AnyView::new(view! {
        column(class = "wt-rp-controls__group") {
            text(class = "wt-rp-controls__title") {{title}}
            {rows}
        }
    })
}

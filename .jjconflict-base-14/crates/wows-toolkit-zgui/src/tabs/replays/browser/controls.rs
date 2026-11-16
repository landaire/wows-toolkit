//! The replay keyboard reference: what the game's own controls do while a replay is playing.

use std::rc::Rc;

use rust_i18n::t;
use wows_toolkit_viewmodel::controls::CommandGroup;
use wows_toolkit_viewmodel::controls::ReplayCommand;
use zgui::css;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::install_stylesheet;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;

use crate::shell::HostedDialogProps;
use crate::shell::Shell;

const SHEET: &str = "wt-replay-controls";

// A reading list, so a column wide enough for a command name beside its keys.
const CSS: &str = css!(
    ".wt-controls { width: 560px; }
    .wt-controls__body {
        display: flex;
        flex-direction: column;
        gap: 16px;
        padding: 8px;
        max-height: 520px;
        overflow-y: auto;
    }
    .wt-controls__group { display: flex; flex-direction: column; gap: 4px; }
    .wt-controls__title { font-size: 14px; font-weight: 700; }
    .wt-controls__command { display: flex; gap: 8px; align-items: center; font-size: 14px; }
    .wt-controls__label { flex: 1; min-width: 0; }
    .wt-controls__keys {
        flex: none;
        width: 180px;
        color: var(--wt-color-text-dim);
        font-family: monospace;
    }"
);

/// Shows `groups` over the window.
pub fn open(shell: Shell, groups: Vec<CommandGroup>) {
    let groups = Rc::new(groups);
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let groups = StoredValue::new_local(Rc::clone(&groups));
        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(class = "wt-controls", attr:data-testid = "replay-controls") {
                    DialogHeader {
                        DialogTitle {{t!("ui.replay.controls.window_title").into_owned()}}
                    }
                    column(class = "wt-controls__body") {
                        for group in move || groups.with_value(|groups| groups.as_ref().clone()), key = |group: &CommandGroup| group.title {
                            column(class = "wt-controls__group") {
                                text(class = "wt-controls__title") {{group.title.to_owned()}}
                                for command in move || group.commands.clone(), key = |command: &ReplayCommand| command.label.clone() {
                                    row(class = "wt-controls__command") {
                                        text(class = "wt-controls__label") {{command.label.clone()}}
                                        text(class = "wt-controls__keys") {{keys(&command)}}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
    });
}

fn keys(command: &ReplayCommand) -> String {
    match &command.key2 {
        Some(second) => format!("{}  /  {}", command.key1, second),
        None => command.key1.clone(),
    }
}

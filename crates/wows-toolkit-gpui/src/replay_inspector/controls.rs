//! The replay keyboard reference: what the game's own controls do while a
//! replay is playing.
//!
//! The groups come from `system/data/commands.scheme.xml` in whichever build
//! is loaded, parsed by `wows_toolkit_viewmodel::controls`, which is the same
//! parse the egui app's reference window uses.

use std::io::Read as _;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_viewmodel::controls::CommandGroup;
use wows_toolkit_viewmodel::controls::parse_commands_scheme;
use wowsunpack::vfs::VfsPath;

/// Where the game keeps the scheme.
const SCHEME_PATH: &str = "system/data/commands.scheme.xml";

/// The reference is a reading list, so it gets a column wide enough for a
/// command name beside its keys rather than the dialog's default.
const DIALOG_WIDTH: Pixels = px(560.);
const KEY_COLUMN_WIDTH: Pixels = px(180.);

/// Reads and parses the scheme out of `vfs`.
///
/// `None` when the build carries no scheme, or one this parse makes nothing
/// of: there is then no reference to show rather than an empty window.
pub fn read_scheme(vfs: &VfsPath) -> Option<Vec<CommandGroup>> {
    let mut file = vfs.join(SCHEME_PATH).and_then(|path| path.open_file()).ok()?;
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return None;
    }
    let groups = parse_commands_scheme(&bytes);
    (!groups.is_empty()).then_some(groups)
}

/// Shows `groups` over the window.
pub fn open(groups: Vec<CommandGroup>, window: &mut Window, cx: &mut App) {
    let groups = std::rc::Rc::new(groups);
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let groups = std::rc::Rc::clone(&groups);
        dialog.title(t!("ui.replay.controls.window_title").into_owned()).width(DIALOG_WIDTH).content(
            move |content, _window, _cx| {
                content.child(
                    div()
                        .id("replay-controls-body")
                        .max_h(px(520.))
                        .overflow_y_scroll()
                        .child(v_flex().gap_4().p_2().children(groups.iter().map(render_group))),
                )
            },
        )
    });
}

fn render_group(group: &CommandGroup) -> impl IntoElement + use<> {
    let heading = div().text_sm().font_weight(FontWeight::BOLD).child(group.title.to_string());
    let rows = group.commands.iter().map(|command| {
        let keys = match &command.key2 {
            Some(second) => format!("{}  /  {}", command.key1, second),
            None => command.key1.clone(),
        };
        h_flex()
            .gap_2()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).text_sm().child(command.label.clone()))
            .child(
                div()
                    .flex_none()
                    .w(KEY_COLUMN_WIDTH)
                    .text_sm()
                    .text_color(crate::theme::text_dim())
                    .font_family("monospace")
                    .child(keys),
            )
    });
    v_flex().gap_1().child(heading).children(rows)
}

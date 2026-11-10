//! In-game chat view for an opened replay. Renders `ReplayReportModel::chat`
//! (populated by `model::build_chat_messages` during the background parse)
//! with the same coloring the egui app's `build_replay_chat_content` /
//! `show_game_chat_window` use (`ui/replay_parser/mod.rs` ~4363, ~4893-4960):
//! sender name colored by team relation, clan tag colored by the packed
//! clan-league color, message body colored by chat channel, and a
//! hover-revealed per-message copy button. `panel.rs` owns the show/hide
//! toggle and only constructs a `ChatPanel` when the replay's chat log is
//! non-empty, matching the egui app disabling its chat toggle in that case.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_replay_data::chat::channel_color;
use wows_toolkit_replay_data::chat::clan_color;
use wows_toolkit_replay_data::chat::copy_text;
use wows_toolkit_replay_data::chat::sender_color;
use wows_toolkit_replay_data::model::ChatMessage;

/// One chat message: a two-line block (colored sender/clan line, colored
/// message line) reproducing the egui `LayoutJob`'s name-then-newline-then-
/// message layout, plus a copy button revealed on row hover via a gpui_kit
/// element group (egui reveals the same button on `rect_contains_pointer`).
fn render_message(ix: usize, message: &ChatMessage, border: Hsla) -> impl IntoElement {
    let semantic = crate::theme::semantic();
    let name_color: Hsla = rgb(sender_color(message.sender_relation, semantic)).into();
    let body_color: Hsla = rgb(channel_color(&message.channel, semantic)).into();
    let group_name = SharedString::from(format!("chat-row-{ix}"));
    let copy_payload = copy_text(message);

    div()
        .id(("chat-row", ix))
        .group(group_name.clone())
        .relative()
        .w_full()
        .px_2()
        .py_1p5()
        .border_b_1()
        .border_color(border)
        .child(
            v_flex()
                .gap_0p5()
                .pr_6()
                .child(
                    h_flex()
                        .gap_1()
                        .when_some(message.clan_tag.as_ref().zip(message.clan_color), |this, (tag, color)| {
                            let tag_color: Hsla = rgb(clan_color(color, semantic)).into();
                            this.child(
                                div()
                                    .text_color(tag_color)
                                    .child(crate::ui::selectable_text(("chat-clan", ix), format!("[{tag}]"))),
                            )
                        })
                        .child(div().text_color(name_color).child(crate::ui::selectable_text(
                            ("chat-sender", ix),
                            format!("{}:", message.sender_name),
                        ))),
                )
                .child(
                    div()
                        .text_color(body_color)
                        .child(crate::ui::selectable_text(("chat-body", ix), message.message.clone())),
                ),
        )
        .child(
            div().absolute().top_1().right_1().invisible().group_hover(group_name, |this| this.visible()).child(
                Button::new(("chat-copy", ix))
                    .icon(IconName::Copy)
                    .ghost()
                    .xsmall()
                    .tooltip(t!("ui.replay.copy_message").to_string())
                    .on_click(move |_event, window, cx: &mut App| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_payload.clone()));
                        crate::toast::ok(t!("ui.replay.chat_copied").to_string(), window, cx);
                    }),
            ),
        )
}

/// One open replay's chat log view. `panel.rs` constructs this once the
/// messages are known and never mutates it afterward; it does not construct
/// one at all when the replay's chat log is empty (see `panel.rs`'s
/// `LoadState`), matching the egui app disabling its chat toggle in that
/// case rather than showing an empty window.
pub struct ChatPanel {
    messages: Vec<ChatMessage>,
    scroll: ScrollHandle,
    /// The name a saved log is offered under, built from the battle the way
    /// the egui chat window builds it.
    title: String,
}

impl ChatPanel {
    pub fn new(messages: Vec<ChatMessage>, title: String, _cx: &mut Context<Self>) -> Self {
        Self { messages, scroll: ScrollHandle::new(), title }
    }

    /// The whole log as one block of text, in the same per-message format the
    /// copy button beside each line writes.
    fn transcript(&self) -> String {
        wows_toolkit_replay_data::chat::transcript(&self.messages)
    }

    fn copy_all(&self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.transcript()));
    }

    /// Asks where to write the log and writes it there.
    fn save_to_file(&self, window: &mut Window, cx: &mut Context<Self>) {
        let transcript = self.transcript();
        let asked = crate::dialog::save_file(
            Some(t!("ui.replay.chat_save_title").as_ref()),
            &format!("{} - Game Chat.txt", self.title),
            None,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = asked.await else { return };
            let shown = path.display().to_string();
            let written = cx.background_spawn(async move { std::fs::write(&path, transcript) }).await;
            let _ = this.update_in(cx, |_this, window, cx| match written {
                Ok(()) => crate::toast::ok(t!("ui.replay.chat_saved", path = shown).to_string(), window, cx),
                Err(err) => {
                    tracing::warn!("replay chat: the log was not saved: {err}");
                    crate::toast::failed(t!("ui.replay.chat_save_failed", error = err).to_string(), window, cx);
                }
            });
        })
        .detach();
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        let rows = div().id("chat-messages").size_full().overflow_y_scroll().track_scroll(&self.scroll).child(
            v_flex()
                .w_full()
                .children(self.messages.iter().enumerate().map(|(ix, message)| render_message(ix, message, border))),
        );

        // What the egui chat window offers above its log.
        let toolbar = h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                Button::new("chat-copy-all")
                    .icon(IconName::Copy)
                    .label(t!("ui.replay.chat_copy_all").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event, _window, cx| this.copy_all(cx))),
            )
            .child(
                Button::new("chat-save")
                    .label(t!("ui.replay.chat_save_to_file").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event, window, cx| this.save_to_file(window, cx))),
            );

        v_flex()
            .size_full()
            .child(toolbar)
            .child(div().relative().flex_1().min_h(px(0.)).child(rows).child(Scrollbar::vertical(&self.scroll)))
    }
}

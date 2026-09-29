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
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::types::Relation;

use super::columns::relation_color_rgb;
use super::model::ChatMessage;

/// No resolvable team relation: the de-emphasised tone, which is what egui's
/// `Color32::GRAY` fallback amounts to in `build_replay_chat_content`.
fn no_relation_gray() -> u32 {
    crate::theme::semantic().text_dim
}

fn self_color() -> u32 {
    crate::theme::semantic().text_strong
}

fn ally_color() -> u32 {
    crate::theme::semantic().chat_team
}

fn enemy_color() -> u32 {
    crate::theme::semantic().loss
}

fn division_color() -> u32 {
    crate::theme::semantic().chat_division
}

fn system_color() -> u32 {
    crate::theme::semantic().chat_other
}

/// Sender-name color packed as `0xRRGGBB`: the self/ally/enemy triad via
/// `relation_color_rgb` (the single source of truth for those three values),
/// plus the gray fallback for a message with no resolvable `sender_relation`.
/// Split out from `sender_color` (which resolves this to an `Hsla`) so the
/// palette mapping is unit-testable by plain `u32` equality; `Hsla` itself
/// carries no `PartialEq` impl to test against directly.
fn sender_color_rgb(relation: Option<Relation>) -> u32 {
    match relation {
        None => no_relation_gray(),
        Some(r) => relation_color_rgb(r),
    }
}

fn sender_color(relation: Option<Relation>) -> Hsla {
    rgb(sender_color_rgb(relation)).into()
}

/// Message-body color packed as `0xRRGGBB`, by chat channel. Mirrors
/// `build_replay_chat_content`'s `match channel { Division => GOLD, Global =>
/// WHITE, Team => LIGHT_GREEN, _ => ORANGE }`. See `sender_color_rgb` for why
/// this is split from the `Hsla`-returning `channel_color`.
fn channel_color_rgb(channel: &ChatChannel) -> u32 {
    match channel {
        ChatChannel::Division => division_color(),
        ChatChannel::Global => self_color(),
        ChatChannel::Team => ally_color(),
        ChatChannel::System | ChatChannel::Unknown(_) => system_color(),
    }
}

fn channel_color(channel: &ChatChannel) -> Hsla {
    rgb(channel_color_rgb(channel)).into()
}

/// The clipboard text for one message: `"[clan] sender (Channel): message"`,
/// or without the clan segment when the sender is clanless. Mirrors the
/// format string `build_replay_chat_content`'s copy button and
/// `show_game_chat_window`'s copy-all handler both use.
fn copy_text(message: &ChatMessage) -> String {
    match &message.clan_tag {
        Some(clan) => format!("[{clan}] {} ({:?}): {}", message.sender_name, message.channel, message.message),
        None => format!("{} ({:?}): {}", message.sender_name, message.channel, message.message),
    }
}

/// One chat message: a two-line block (colored sender/clan line, colored
/// message line) reproducing the egui `LayoutJob`'s name-then-newline-then-
/// message layout, plus a copy button revealed on row hover via a gpui_kit
/// element group (egui reveals the same button on `rect_contains_pointer`).
fn render_message(ix: usize, message: &ChatMessage, border: Hsla) -> impl IntoElement {
    let name_color = sender_color(message.sender_relation);
    let body_color = channel_color(&message.channel);
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
                        .when_some(message.clan_tag.as_ref().zip(message.clan_color_rgb), |this, (tag, color)| {
                            let clan_color: Hsla = rgb(color).into();
                            this.child(div().text_color(clan_color).child(format!("[{tag}]")))
                        })
                        .child(div().text_color(name_color).child(format!("{}:", message.sender_name))),
                )
                // The message itself is selectable; the clan tag and the
                // sender are not. The pane draws every message it holds on
                // every frame, and a selection run registers itself each
                // time, so a battle log's worth of them is paid for three
                // times over for two words nobody lifts out on their own. The
                // copy button beside the row takes the whole line.
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
        self.messages.iter().map(copy_text).collect::<Vec<_>>().join("\n")
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

// Deliberately not `use super::*;`: that glob also pulls in `render_message`
// and `ChatPanel`, whose `impl IntoElement` return type and gpui_kit builder
// chains blow rustc's macro-expansion recursion tracking sky-high once
// re-monomorphized into a `#[cfg(test)]` module (observed: several thousand
// deep before it just stack-overflows the compiler). Only the plain
// functions/types the tests actually touch are imported.
#[cfg(test)]
mod tests {
    use wows_replays::analyzer::battle_controller::ChatChannel;
    use wows_replays::types::GameClock;
    use wows_replays::types::Relation;

    use super::ChatMessage;
    use super::ally_color;
    use super::channel_color_rgb;
    use super::copy_text;
    use super::division_color;
    use super::no_relation_gray;
    use super::relation_color_rgb;
    use super::self_color;
    use super::sender_color_rgb;
    use super::system_color;

    fn message(sender_relation: Option<Relation>, channel: ChatChannel, clan_tag: Option<&str>) -> ChatMessage {
        ChatMessage {
            clock: GameClock(0.0),
            sender_relation,
            sender_name: "Player".to_string(),
            channel,
            message: "hello".to_string(),
            clan_tag: clan_tag.map(str::to_string),
            clan_color_rgb: clan_tag.map(|_| 0x3399ff),
        }
    }

    #[test]
    fn sender_color_matches_relation_palette() {
        // A sender is coloured by relation, from the table the player list
        // shares; only a message with no relation at all falls back here.
        assert_eq!(sender_color_rgb(None), no_relation_gray());
        for relation in [0, 1, 2] {
            let relation = Relation::new(relation);
            assert_eq!(sender_color_rgb(Some(relation)), relation_color_rgb(relation));
        }
    }

    #[test]
    fn channel_color_matches_palette() {
        assert_eq!(channel_color_rgb(&ChatChannel::Division), division_color());
        assert_eq!(channel_color_rgb(&ChatChannel::Global), self_color());
        assert_eq!(channel_color_rgb(&ChatChannel::Team), ally_color());
        assert_eq!(channel_color_rgb(&ChatChannel::System), system_color());
        assert_eq!(channel_color_rgb(&ChatChannel::Unknown("x".to_string())), system_color());
    }

    #[test]
    fn copy_text_includes_clan_tag_when_present() {
        let msg = message(Some(Relation::new(1)), ChatChannel::Team, Some("WTK"));
        assert_eq!(copy_text(&msg), "[WTK] Player (Team): hello");
    }

    #[test]
    fn copy_text_omits_clan_segment_when_clanless() {
        let msg = message(Some(Relation::new(1)), ChatChannel::Team, None);
        assert_eq!(copy_text(&msg), "Player (Team): hello");
    }

    #[test]
    fn copy_text_formats_unknown_channel_with_its_payload() {
        let msg = message(None, ChatChannel::Unknown("weird".to_string()), None);
        assert_eq!(copy_text(&msg), "Player (Unknown(\"weird\")): hello");
    }
}

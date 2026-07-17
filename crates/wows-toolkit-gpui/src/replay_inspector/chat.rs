//! In-game chat view for an opened replay. Renders `ReplayReportModel::chat`
//! (populated by `model::build_chat_messages` during the background parse)
//! with the same coloring the egui app's `build_replay_chat_content` /
//! `show_game_chat_window` use (`ui/replay_parser/mod.rs` ~4363, ~4893-4960):
//! sender name colored by team relation, clan tag colored by the packed
//! clan-league color, message body colored by chat channel, and a
//! hover-revealed per-message copy button. `panel.rs` owns the show/hide
//! toggle and only constructs a `ChatPanel` when the replay's chat log is
//! non-empty, matching the egui app disabling its chat toggle in that case.

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::IconName;
use gpui_component::Sizable;
use gpui_component::button::Button;
use gpui_component::button::ButtonVariants;
use gpui_component::h_flex;
use gpui_component::scroll::Scrollbar;
use gpui_component::v_flex;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::types::Relation;

use super::model::ChatMessage;

/// No resolvable team relation: rendered gray, matching egui's `Color32::GRAY`
/// fallback in `build_replay_chat_content`.
const NO_RELATION_GRAY: u32 = 0x808080;
const SELF_WHITE: u32 = 0xffffff;
const ALLY_LIGHT_GREEN: u32 = 0x90ee90;
const ENEMY_LIGHT_RED: u32 = 0xff8080;
const DIVISION_GOLD: u32 = 0xffd700;
const CHANNEL_ORANGE: u32 = 0xffa500;

/// Sender-name color packed as `0xRRGGBB`: mirrors
/// `player_color_for_team_relation` (self = white, ally = light green,
/// enemy = light red) plus the gray fallback for a message with no
/// resolvable `sender_relation`. Split out from `sender_color` (which
/// resolves this to an `Hsla`) so the palette mapping is unit-testable by
/// plain `u32` equality; `Hsla` itself carries no `PartialEq` impl to test
/// against directly.
fn sender_color_rgb(relation: Option<Relation>) -> u32 {
    match relation {
        None => NO_RELATION_GRAY,
        Some(r) if r.is_self() => SELF_WHITE,
        Some(r) if r.is_ally() => ALLY_LIGHT_GREEN,
        Some(_) => ENEMY_LIGHT_RED,
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
        ChatChannel::Division => DIVISION_GOLD,
        ChatChannel::Global => SELF_WHITE,
        ChatChannel::Team => ALLY_LIGHT_GREEN,
        ChatChannel::System | ChatChannel::Unknown(_) => CHANNEL_ORANGE,
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
/// message layout, plus a copy button revealed on row hover via a gpui
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
                .child(div().text_color(body_color).child(message.message.clone())),
        )
        .child(div().absolute().top_1().right_1().invisible().group_hover(group_name, |this| this.visible()).child(
            Button::new(("chat-copy", ix)).icon(IconName::Copy).ghost().xsmall().tooltip("Copy message").on_click(
                move |_event, _window, cx: &mut App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_payload.clone()));
                },
            ),
        ))
}

/// One open replay's chat log view. `panel.rs` constructs this once the
/// messages are known and never mutates it afterward; it does not construct
/// one at all when the replay's chat log is empty (see `panel.rs`'s
/// `LoadState`), matching the egui app disabling its chat toggle in that
/// case rather than showing an empty window.
pub struct ChatPanel {
    messages: Vec<ChatMessage>,
    scroll: ScrollHandle,
}

impl ChatPanel {
    pub fn new(messages: Vec<ChatMessage>, _cx: &mut Context<Self>) -> Self {
        Self { messages, scroll: ScrollHandle::new() }
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

        div().relative().size_full().child(rows).child(Scrollbar::vertical(&self.scroll))
    }
}

// Deliberately not `use super::*;`: that glob also pulls in `render_message`
// and `ChatPanel`, whose `impl IntoElement` return type and gpui builder
// chains blow rustc's macro-expansion recursion tracking sky-high once
// re-monomorphized into a `#[cfg(test)]` module (observed: several thousand
// deep before it just stack-overflows the compiler). Only the plain
// functions/types the tests actually touch are imported.
#[cfg(test)]
mod tests {
    use wows_replays::analyzer::battle_controller::ChatChannel;
    use wows_replays::types::GameClock;
    use wows_replays::types::Relation;

    use super::ALLY_LIGHT_GREEN;
    use super::CHANNEL_ORANGE;
    use super::ChatMessage;
    use super::DIVISION_GOLD;
    use super::ENEMY_LIGHT_RED;
    use super::NO_RELATION_GRAY;
    use super::SELF_WHITE;
    use super::channel_color_rgb;
    use super::copy_text;
    use super::sender_color_rgb;

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
        assert_eq!(sender_color_rgb(None), NO_RELATION_GRAY);
        assert_eq!(sender_color_rgb(Some(Relation::new(0))), SELF_WHITE);
        assert_eq!(sender_color_rgb(Some(Relation::new(1))), ALLY_LIGHT_GREEN);
        assert_eq!(sender_color_rgb(Some(Relation::new(2))), ENEMY_LIGHT_RED);
    }

    #[test]
    fn channel_color_matches_palette() {
        assert_eq!(channel_color_rgb(&ChatChannel::Division), DIVISION_GOLD);
        assert_eq!(channel_color_rgb(&ChatChannel::Global), SELF_WHITE);
        assert_eq!(channel_color_rgb(&ChatChannel::Team), ALLY_LIGHT_GREEN);
        assert_eq!(channel_color_rgb(&ChatChannel::System), CHANNEL_ORANGE);
        assert_eq!(channel_color_rgb(&ChatChannel::Unknown("x".to_string())), CHANNEL_ORANGE);
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

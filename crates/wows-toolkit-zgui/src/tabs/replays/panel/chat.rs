//! A replay's in-game chat: sender coloured by team relation, clan tag by its league colour,
//! message by channel, and a copy button revealed on hover.

use std::rc::Rc;

use rust_i18n::t;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::types::Relation;
use wows_toolkit_replay_data::model::ChatMessage;
use wows_toolkit_replay_data::model::ClanColor;
use zgui::component;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ScrollAreaProps;

use crate::icons::AppIconProps;
use crate::shell::Shell;

/// A sender's name colour.
fn relation_color(relation: Relation) -> &'static str {
    if relation.is_self() {
        "var(--wt-color-text-strong)"
    } else if relation.is_ally() {
        "var(--wt-color-win)"
    } else {
        "var(--wt-color-loss)"
    }
}

/// A message's sender colour: by relation, dimmed when the sender has none.
pub fn sender_color(relation: Option<Relation>) -> &'static str {
    relation.map_or("var(--wt-color-text-dim)", relation_color)
}

/// A message body's colour, by channel, as the egui chat colours it.
pub fn channel_color(channel: &ChatChannel) -> &'static str {
    match channel {
        ChatChannel::Division => "var(--wt-color-chat-division)",
        ChatChannel::Global => "var(--wt-color-text-strong)",
        ChatChannel::Team => "var(--wt-color-chat-team)",
        ChatChannel::System | ChatChannel::Unknown(_) => "var(--wt-color-chat-other)",
    }
}

fn clan_color(color: ClanColor) -> String {
    match color {
        ClanColor::League(rgb) => format!("#{rgb:06x}"),
        ClanColor::Relation(relation) => relation_color(relation).to_owned(),
    }
}

/// One message on the clipboard: `"[clan] sender (Channel): message"`, the clan left off for a
/// clanless sender, as the egui copy button writes it.
pub fn copy_text(message: &ChatMessage) -> String {
    match &message.clan_tag {
        Some(clan) => format!("[{clan}] {} ({:?}): {}", message.sender_name, message.channel, message.message),
        None => format!("{} ({:?}): {}", message.sender_name, message.channel, message.message),
    }
}

/// The whole log, one message per line in the copy button's format.
pub fn transcript(messages: &[ChatMessage]) -> String {
    messages.iter().map(copy_text).collect::<Vec<_>>().join("\n")
}

/// One replay's chat log. Only built for a replay with chat; `title` names a saved log.
#[component]
pub fn ChatPanel(messages: Rc<[ChatMessage]>, title: String) -> impl IntoView {
    let shell = Shell::current();
    let rows: Vec<AnyView> = messages.iter().map(|message| message_row(message, shell)).collect();
    let all = Rc::clone(&messages);
    let saved = Rc::clone(&messages);

    let title = Rc::<str>::from(title);

    view! {
        column(class = "wt-rp-chat", attr:data-testid = "replay-chat") {
            row(class = "wt-rp-chat__toolbar") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "chat-copy-all",
                    on:click = move |_| {
                        shell.copy_text(transcript(&all));
                        shell.toasts().ok(t!("ui.replay.chat_copied").into_owned());
                    }
                ) {
                    AppIcon(glyph = wows_toolkit_icons::Glyph::Copy)
                    text {{t!("ui.replay.chat_copy_all").into_owned()}}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "chat-save",
                    on:click = move |_| save_to_file(shell, transcript(&saved), &title)
                ) {
                    {t!("ui.replay.chat_save_to_file").into_owned()}
                }
            }
            ScrollArea(class = "wt-rp-chat__log") {
                column {{rows}}
            }
        }
    }
}

fn message_row(message: &ChatMessage, shell: Shell) -> AnyView {
    let clan =
        message.clan_tag.clone().zip(message.clan_color).map(|(tag, color)| (format!("[{tag}]"), clan_color(color)));
    let sender = format!("{}:", message.sender_name);
    let sender_color = sender_color(message.sender_relation);
    let body = message.message.clone();
    let body_color = channel_color(&message.channel);
    let copied = copy_text(message);
    let copy_label = t!("ui.replay.copy_message").into_owned();

    AnyView::new(view! {
        box(class = "wt-rp-chat__row", attr:data-testid = "chat-row") {
            column(class = "wt-rp-chat__text") {
                row(class = "wt-rp-chat__who") {
                    {clan.map(|(tag, color)| view! { text(style:color = color) {{tag}} })}
                    text(style:color = sender_color) {{sender}}
                }
                text(class = "wt-rp-chat__body", style:color = body_color) {{body}}
            }
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconSm,
                class = "wt-rp-chat__copy",
                attr:data-testid = "chat-copy",
                a11y:label = copy_label,
                on:click = move |_| {
                    shell.copy_text(copied.clone());
                    shell.toasts().ok(t!("ui.replay.chat_copied").into_owned());
                }
            ) {
                AppIcon(glyph = wows_toolkit_icons::Glyph::Copy)
            }
        }
    })
}

/// Asks where to write the log and writes it there.
fn save_to_file(shell: Shell, transcript: String, title: &str) {
    let asked = wows_toolkit_services::dialog::save_file(
        Some(t!("ui.replay.chat_save_title").as_ref()),
        &format!("{title} - Game Chat.txt"),
        None,
    );
    spawn_local(async move {
        let path = match asked.await {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(err) => {
                shell.toasts().failed(t!("ui.dialogs.file_dialog_failed", reason = err).into_owned());
                return;
            }
        };
        let shown = path.display().to_string();
        match blocking(move || std::fs::write(&path, transcript)).await {
            Ok(()) => {
                shell.toasts().ok(t!("ui.replay.chat_saved", path = shown).into_owned());
            }
            Err(err) => {
                tracing::warn!("replay chat: the log was not saved: {err}");
                shell.toasts().failed(t!("ui.replay.chat_save_failed", error = err).into_owned());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use wows_replays::analyzer::battle_controller::ChatChannel;
    use wows_replays::types::Relation;
    use wows_toolkit_replay_data::model::ChatMessage;

    use super::channel_color;
    use super::copy_text;
    use super::sender_color;
    use super::transcript;

    fn message(clan: Option<&str>, channel: ChatChannel) -> ChatMessage {
        ChatMessage {
            clock: Default::default(),
            sender_relation: Some(Relation::new(1)),
            sender_name: "Sender".to_owned(),
            channel,
            message: "gl hf".to_owned(),
            clan_tag: clan.map(str::to_owned),
            clan_color: None,
        }
    }

    /// The clipboard line carries the clan only when the sender has one.
    #[test]
    fn a_copied_message_names_its_clan_only_when_there_is_one() {
        assert_eq!(copy_text(&message(Some("WG"), ChatChannel::Team)), "[WG] Sender (Team): gl hf");
        assert_eq!(copy_text(&message(None, ChatChannel::Global)), "Sender (Global): gl hf");
    }

    /// The transcript is the copied lines, one per message.
    #[test]
    fn the_transcript_is_one_line_per_message() {
        let messages = [message(None, ChatChannel::Team), message(Some("WG"), ChatChannel::Global)];
        assert_eq!(transcript(&messages), "Sender (Team): gl hf\n[WG] Sender (Global): gl hf");
    }

    /// Each relation and channel takes its own colour, and a sender with no relation is dimmed.
    #[test]
    fn senders_and_channels_are_coloured_apart() {
        assert_eq!(sender_color(None), "var(--wt-color-text-dim)");
        assert_ne!(sender_color(Some(Relation::new(0))), sender_color(Some(Relation::new(2))));
        assert_ne!(channel_color(&ChatChannel::Team), channel_color(&ChatChannel::Division));
        assert_eq!(channel_color(&ChatChannel::System), channel_color(&ChatChannel::Unknown("x".into())));
    }
}

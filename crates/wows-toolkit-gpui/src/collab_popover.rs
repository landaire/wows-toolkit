//! The session popover: hosting a session, joining one, and steering it.
//!
//! Mirrors the egui header's own popover (`ui/replay_parser/mod.rs`'s
//! `show_session_popover`). What it drives is `wt-collab-client`, shared with
//! that app, so a session hosted from either looks the same to a peer.

use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wt_collab_client::PeerRole;
use wt_collab_client::Permissions;
use wt_collab_client::SessionStatus;

use crate::collab::CollabState;

/// How wide the popover sits, matching the egui one's own minimum.
const WIDTH: Pixels = px(300.);

/// What the popover needs from the view that owns it.
pub trait SessionHost: 'static {
    fn collab(&self) -> &CollabState;
    fn collab_mut(&mut self) -> &mut CollabState;
    /// The field a display name is typed into.
    fn name_input(&self) -> &Entity<InputState>;
    /// The field a session token is pasted into.
    fn token_input(&self) -> &Entity<InputState>;
}

/// The header button and the popover under it.
///
/// The button carries the session's own state: an active session is what the
/// reader most needs to know is running, so it is on the control rather than
/// behind a hover.
/// `view` is the tab itself rather than its entity: this is called from
/// inside that tab's own render, where the entity is leased and reading it
/// would panic. The entity is only captured for the popover body, which runs
/// later, when the lease has been released.
pub fn render<V: SessionHost + Render>(view: &V, entity: &Entity<V>, cx: &mut Context<V>) -> AnyElement {
    let active = view.collab().is_active();
    let label = t!("ui.collab.session").into_owned();
    let _ = cx;

    let owner = entity.clone();
    Popover::new("collab-session-popover")
        .trigger(
            Button::new("collab-session-toggle")
                .child(crate::icons::icon(crate::icons::BROADCAST))
                .label(label)
                .compact()
                // A running session is the state worth seeing at a glance.
                .when(active, |button| button.danger()),
        )
        .content(move |_state, _window, cx| {
            let body = owner.update(cx, |view, cx| render_body(view, cx));
            div().w(WIDTH).p_2().child(body).into_any_element()
        })
        .into_any_element()
}

fn render_body<V: SessionHost + Render>(view: &mut V, cx: &mut Context<V>) -> AnyElement {
    match view.collab().status() {
        SessionStatus::Idle => render_start(view, cx),
        SessionStatus::Starting | SessionStatus::Connecting => render_waiting(cx),
        SessionStatus::Active => render_active(view, cx),
        SessionStatus::Error(reason) => render_failure(reason, cx),
    }
}

/// What is offered with no session running: a name, then host or join.
fn render_start<V: SessionHost + Render>(view: &mut V, cx: &mut Context<V>) -> AnyElement {
    let named = !view.collab().display_name.trim().is_empty();
    let failure = view.collab().failure.clone();

    v_flex()
        .gap_2()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.collab.display_name").to_string()))
        .child(Input::new(view.name_input()).id("collab-display-name").small().w_full())
        // Every other peer reads this name, so nothing starts without one.
        .when(!named, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.collab.enter_display_name").to_string()),
            )
        })
        .child(crate::ui::rule_h(cx))
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.collab.host_session").to_string()))
        .child(
            Button::new("collab-host")
                .label(t!("ui.collab.start_session").to_string())
                .compact()
                .disabled(!named)
                // Hosting tells every peer where this machine is, which the
                // reader is asked about before it happens.
                .on_click(cx.listener(|_view: &mut V, _event, window, cx| {
                    let owner = cx.entity();
                    crate::notices::before_revealing_address(window, cx, move |_window, cx| {
                        let version = toolkit_version();
                        owner.update(cx, |view: &mut V, cx| {
                            view.collab_mut().host(version, cx);
                            cx.notify();
                        });
                    });
                })),
        )
        .child(crate::ui::rule_h(cx))
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.collab.join_session").to_string()))
        .child(Input::new(view.token_input()).id("collab-token").small().w_full())
        .child(
            Button::new("collab-join")
                .label(t!("ui.collab.paste_and_join").to_string())
                .compact()
                .disabled(!named)
                .on_click(cx.listener(|view: &mut V, _event, window, cx| {
                    let token = view.token_input().read(cx).value().to_string();
                    let owner = cx.entity();
                    crate::notices::before_revealing_address(window, cx, move |_window, cx| {
                        let token = token.clone();
                        let version = toolkit_version();
                        owner.update(cx, |view: &mut V, cx| {
                            view.collab_mut().join(token, version, cx);
                            cx.notify();
                        });
                    });
                })),
        )
        .children(failure.map(|reason| {
            div().text_xs().text_color(rgb(crate::theme::semantic().error)).child(reason).into_any_element()
        }))
        .into_any_element()
}

/// The session is coming up. Nothing can be done with it yet but abandon it.
fn render_waiting<V: SessionHost + Render>(cx: &mut Context<V>) -> AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .child(Spinner::new())
        .child(div().text_sm().child(t!("ui.collab.starting").to_string()))
        .child(Button::new("collab-cancel").label(t!("ui.buttons.cancel").to_string()).compact().on_click(cx.listener(
            |view: &mut V, _event, _window, cx| {
                view.collab_mut().leave();
                cx.notify();
            },
        )))
        .into_any_element()
}

fn render_failure<V: SessionHost + Render>(reason: String, cx: &mut Context<V>) -> AnyElement {
    v_flex()
        .gap_2()
        .child(div().text_sm().text_color(rgb(crate::theme::semantic().error)).child(reason))
        .child(Button::new("collab-dismiss").label(t!("ui.collab.leave").to_string()).compact().on_click(cx.listener(
            |view: &mut V, _event, _window, cx| {
                view.collab_mut().leave();
                cx.notify();
            },
        )))
        .into_any_element()
}

/// A running session: who is in it, what they may do, and how to leave.
fn render_active<V: SessionHost + Render>(view: &mut V, cx: &mut Context<V>) -> AnyElement {
    let collab = view.collab();
    let hosting = collab.is_hosting();
    let may_steer = collab.may_steer();
    let revealed = collab.token_revealed;
    let token = collab.token();
    let shown_token = collab.token_display();
    let web_link = collab.web_link();
    let roster = collab.connected();
    let permissions = collab.permissions();
    let heading = if hosting { t!("ui.collab.session_active") } else { t!("ui.collab.connected_to_session") };

    let mut body = v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(crate::theme::semantic().error))
                        .child(heading.into_owned()),
                )
                .child(Button::new("collab-leave").label(t!("ui.collab.leave").to_string()).compact().on_click(
                    cx.listener(|view: &mut V, _event, _window, cx| {
                        view.collab_mut().leave();
                        cx.notify();
                    }),
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.collab.connected_count", count = roster.len()).to_string()),
        );

    // Only a host has a token to hand out; a joiner already used one.
    if let (Some(token), Some(shown)) = (token, shown_token) {
        body = body.child(crate::ui::rule_h(cx)).child(
            v_flex()
                .gap_1()
                .child(div().text_xs().child(t!("ui.collab.session_token").to_string()))
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(div().flex_1().text_xs().font_family("monospace").child(shown))
                        .child(
                            Button::new("collab-reveal")
                                .child(crate::icons::icon(if revealed {
                                    crate::icons::EYE_SLASH
                                } else {
                                    crate::icons::EYE
                                }))
                                .compact()
                                .tooltip(t!("ui.collab.toggle_visibility").to_string())
                                .on_click(cx.listener(|view: &mut V, _event, _window, cx| {
                                    let collab = view.collab_mut();
                                    collab.token_revealed = !collab.token_revealed;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(copy_button(
                            "collab-copy-token",
                            t!("ui.collab.copy_token").into_owned(),
                            Copies { value: token, said: t!("ui.collab.token_copied").into_owned() },
                        ))
                        .children(web_link.map(|link| {
                            copy_button(
                                "collab-copy-web-link",
                                t!("ui.collab.copy_web_link").into_owned(),
                                Copies { value: link, said: t!("ui.collab.web_link_copied").into_owned() },
                            )
                        })),
                ),
        );
    }

    body = body.child(crate::ui::rule_h(cx)).children(roster.into_iter().enumerate().map(|(index, peer)| {
        let role = match peer.role {
            PeerRole::Host => Some(t!("ui.collab.role_host").into_owned()),
            PeerRole::CoHost => Some(t!("ui.collab.role_cohost").into_owned()),
            PeerRole::Peer => None,
        };
        // A host can raise a peer, but not itself and not another host.
        let promotable = may_steer && matches!(peer.role, PeerRole::Peer) && !peer.is_me;
        let user_id = peer.user_id;
        h_flex()
            .gap_1()
            .items_center()
            .child(div().flex_1().text_xs().child(peer.name))
            .when(peer.is_me, |this| {
                this.child(div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.collab.you").to_string()))
            })
            .children(role.map(|role| div().text_xs().text_color(crate::theme::text_dim()).child(role)))
            .when(promotable, |this| {
                this.child(
                    Button::new(("collab-promote", index))
                        .label(t!("ui.collab.promote_cohost").to_string())
                        .compact()
                        .on_click(cx.listener(move |view: &mut V, _event, _window, cx| {
                            view.collab().promote(user_id);
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }));

    // Only whoever may steer sees the locks; for everyone else they are
    // someone else's decision, not a control.
    if may_steer {
        let annotations_locked = permissions.annotations_locked;
        let settings_locked = permissions.settings_locked;
        body = body
            .child(crate::ui::rule_h(cx))
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.collab.permissions").to_string()))
            .child(
                Checkbox::new("collab-lock-annotations")
                    .label(t!("ui.collab.lock_annotations").to_string())
                    .checked(annotations_locked)
                    .on_click(cx.listener(move |view: &mut V, checked: &bool, _window, cx| {
                        view.collab().set_permissions(Permissions { annotations_locked: *checked, settings_locked });
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("collab-lock-settings")
                    .label(t!("ui.collab.lock_settings").to_string())
                    .checked(settings_locked)
                    .on_click(cx.listener(move |view: &mut V, checked: &bool, _window, cx| {
                        view.collab().set_permissions(Permissions { annotations_locked, settings_locked: *checked });
                        cx.notify();
                    })),
            )
            .child(
                Button::new("collab-reset-overrides")
                    .label(t!("ui.collab.reset_overrides").to_string())
                    .compact()
                    .tooltip(t!("ui.collab.reset_overrides_tooltip").to_string())
                    .on_click(cx.listener(|view: &mut V, _event, _window, cx| {
                        view.collab().reset_overrides();
                        cx.notify();
                    })),
            );
    }

    body.into_any_element()
}

/// What a copy button puts on the clipboard, and what it says once it has.
///
/// One argument rather than two adjacent `String`s: swapping them would compile
/// and put the announcement on the clipboard.
struct Copies {
    value: String,
    said: String,
}

/// A button that copies `copies.value` and reports `copies.said`.
///
/// Reported at `info`, which is the level the egui collab panel uses for the
/// same three copies (`replay_parser/mod.rs:4490`).
fn copy_button(id: &'static str, label: String, copies: Copies) -> AnyElement {
    Button::new(id)
        .label(label)
        .compact()
        .on_click(move |_event, window, cx: &mut App| {
            cx.write_to_clipboard(ClipboardItem::new_string(copies.value.clone()));
            crate::toast::info(copies.said.clone(), window, cx);
        })
        .into_any_element()
}

/// What this build calls itself to its peers.
fn toolkit_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

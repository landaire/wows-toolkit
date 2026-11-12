//! Puts the warnings of `wows_toolkit_viewmodel::notices` in front of the work
//! they guard, and shows errors worth reading in full.

use std::rc::Rc;

use gpui_kit::App;
use gpui_kit::Global;
use gpui_kit::InteractiveElement as _;
use gpui_kit::ParentElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use rust_i18n::t;
use wows_toolkit_viewmodel::notices::Notice;
use wows_toolkit_viewmodel::notices::Suppressed;

/// What to do once the reader has read the warning and carried on.
///
/// Shared rather than owned: the dialog's body is rebuilt on every frame, so the
/// action has to outlive each build of it.
type GoAhead = Rc<dyn Fn(&mut Window, &mut App)>;

/// Which warnings the reader has switched off, for this run.
#[derive(Clone, Copy, Default)]
struct HeldSuppressed(Suppressed);

impl Global for HeldSuppressed {}

/// Adopts what the settings say. Called once, when the startup read lands.
pub fn adopt(address: bool, software_encode: bool, cx: &mut App) {
    cx.set_global(HeldSuppressed(Suppressed { address, software_encode }));
}

fn suppressed(notice: Notice, cx: &App) -> bool {
    cx.try_global::<HeldSuppressed>().is_some_and(|held| held.0.is_suppressed(notice))
}

/// Records the reader's choice, for this session and the next.
fn suppress(notice: Notice, suppressed: bool, cx: &mut App) {
    let mut held = cx.try_global::<HeldSuppressed>().copied().unwrap_or_default();
    held.0.set(notice, suppressed);
    crate::settings_store::save(notice.setting_key(), &suppressed, cx);
    cx.set_global(held);
}

fn element_id(notice: Notice) -> &'static str {
    match notice {
        Notice::Address => "notice-address",
        Notice::SoftwareEncode => "notice-software-encode",
    }
}

/// Asks before hosting or joining a session, which tells the other peers where
/// this machine is.
pub fn before_revealing_address(window: &mut Window, cx: &mut App, go: impl Fn(&mut Window, &mut App) + 'static) {
    ask(Notice::Address, window, cx, go);
}

/// Asks before an export that has no GPU encoder to use, which takes far longer.
pub fn before_software_encode(window: &mut Window, cx: &mut App, go: impl Fn(&mut Window, &mut App) + 'static) {
    ask(Notice::SoftwareEncode, window, cx, go);
}

/// Puts `warning`, and runs `go` if the reader carries on.
///
/// A suppressed warning is not put at all: the reader has already answered it,
/// and the point of switching it off is that the work just happens.
fn ask(notice: Notice, window: &mut Window, cx: &mut App, go: impl Fn(&mut Window, &mut App) + 'static) {
    if suppressed(notice, cx) {
        window.defer(cx, move |window, cx| go(window, cx));
        return;
    }

    let go: GoAhead = Rc::new(go);
    window.open_dialog(cx, move |dialog, _window, cx| {
        let go = Rc::clone(&go);
        let is_suppressed = suppressed(notice, cx);
        dialog
            .title(notice.title())
            .close_button(false)
            .child(
                v_flex()
                    .id(element_id(notice))
                    .gap_2()
                    .max_w(px(480.))
                    .child(div().text_sm().child(notice.message()))
                    .when_some(notice.more_info_url(), |this, url| {
                        this.child(
                            Button::new("notice-more-info")
                                .ghost()
                                .small()
                                .label(t!("ui.labels.more_info").into_owned())
                                .on_click(move |_event, _window, cx: &mut App| cx.open_url(url)),
                        )
                    })
                    .child(
                        // Written as it is ticked, as the egui dialog's own
                        // checkbox is: ticking and then cancelling suppresses the
                        // warning in both apps.
                        Checkbox::new("notice-suppress")
                            .label(t!("ui.labels.suppress_warning").to_string())
                            .checked(is_suppressed)
                            .on_click(move |checked, _window, cx: &mut App| suppress(notice, *checked, cx)),
                    ),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child({
                        let go = Rc::clone(&go);
                        Button::new("notice-continue")
                            .primary()
                            .small()
                            .label(t!("ui.buttons.continue_").into_owned())
                            .on_click(move |_event, window, cx: &mut App| {
                                window.close_dialog(cx);
                                let go = Rc::clone(&go);
                                window.defer(cx, move |window, cx| go(window, cx));
                            })
                    })
                    .child(
                        Button::new("notice-cancel")
                            .small()
                            .label(t!("ui.buttons.cancel").into_owned())
                            .on_click(|_event, window, cx: &mut App| window.close_dialog(cx)),
                    ),
            )
    });
}

/// Shows an error worth reading in full, with a way to take it away.
///
/// A toast is right for a sentence and wrong for a chain of causes: it goes by
/// itself and cannot be copied into a bug report. The egui app has the same
/// window (`app.rs`'s `show_err_window`).
pub fn show_error(text: String, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let text = text.clone();
        let copied = text.clone();
        dialog
            .title(t!("ui.windows.error").into_owned())
            .child(
                v_flex()
                    .id("notice-error")
                    .gap_2()
                    .max_w(px(560.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .child(t!("ui.labels.error_occurred").into_owned()),
                    )
                    .child(
                        div().id("notice-error-text").max_h(px(320.)).overflow_y_scroll().text_xs().child(text.clone()),
                    ),
            )
            .footer(h_flex().gap_2().justify_end().child(
                Button::new("notice-error-copy").small().label(t!("ui.buttons.copy").into_owned()).on_click(
                    move |_event, _window, cx: &mut App| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copied.clone()));
                    },
                ),
            ))
    });
}

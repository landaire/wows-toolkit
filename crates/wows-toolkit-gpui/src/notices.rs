//! Warnings that stand in front of something, and the ones a reader may switch
//! off.
//!
//! The egui app puts the same two questions: hosting or joining a session
//! reveals this machine's address (`app.rs`'s `show_ip_warning_dialog`), and an
//! export with no GPU encoder behind it will encode in software
//! (`replay/renderer/mod.rs`'s `GpuEncoderWarning`). Both are suppressible, in
//! the rows the egui app writes, so a reader who switched one off there does not
//! meet it here.

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
use wows_toolkit_viewmodel::settings::keys;

/// Where the networking note lives, which the warning links as the egui dialog
/// does.
const NETWORKING_URL: &str = "https://landaire.github.io/wows-toolkit/networking";

/// What to do once the reader has read the warning and carried on.
///
/// Shared rather than owned: the dialog's body is rebuilt on every frame, so the
/// action has to outlive each build of it.
type GoAhead = Rc<dyn Fn(&mut Window, &mut App)>;

/// Which warnings the reader has switched off.
#[derive(Clone, Copy, Default)]
struct Suppressed {
    address: bool,
    software_encode: bool,
}

impl Global for Suppressed {}

/// Adopts what the settings say. Called once, when the startup read lands.
pub fn adopt(address: bool, software_encode: bool, cx: &mut App) {
    cx.set_global(Suppressed { address, software_encode });
}

/// One of the two warnings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Warning {
    Address,
    SoftwareEncode,
}

impl Warning {
    fn suppressed(self, cx: &App) -> bool {
        let held = cx.try_global::<Suppressed>().copied().unwrap_or_default();
        match self {
            Self::Address => held.address,
            Self::SoftwareEncode => held.software_encode,
        }
    }

    /// Records the reader's choice, for this session and the next.
    fn suppress(self, suppressed: bool, cx: &mut App) {
        let mut held = cx.try_global::<Suppressed>().copied().unwrap_or_default();
        match self {
            Self::Address => {
                held.address = suppressed;
                crate::settings_store::save(keys::SUPPRESS_P2P_IP_WARNING, &suppressed, cx);
            }
            Self::SoftwareEncode => {
                held.software_encode = suppressed;
                crate::settings_store::save(keys::SUPPRESS_GPU_ENCODER_WARNING, &suppressed, cx);
            }
        }
        cx.set_global(held);
    }

    fn title(self) -> String {
        match self {
            Self::Address => t!("ui.windows.network_warning").into_owned(),
            Self::SoftwareEncode => t!("ui.windows.software_encode").into_owned(),
        }
    }

    fn message(self) -> String {
        match self {
            Self::Address => t!("ui.dialogs.p2p_warning").into_owned(),
            Self::SoftwareEncode => t!("ui.dialogs.software_encode_message").into_owned(),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Address => "notice-address",
            Self::SoftwareEncode => "notice-software-encode",
        }
    }
}

/// Asks before hosting or joining a session, which tells the other peers where
/// this machine is.
pub fn before_revealing_address(window: &mut Window, cx: &mut App, go: impl Fn(&mut Window, &mut App) + 'static) {
    ask(Warning::Address, window, cx, go);
}

/// Asks before an export that has no GPU encoder to use, which takes far longer.
pub fn before_software_encode(window: &mut Window, cx: &mut App, go: impl Fn(&mut Window, &mut App) + 'static) {
    ask(Warning::SoftwareEncode, window, cx, go);
}

/// Puts `warning`, and runs `go` if the reader carries on.
///
/// A suppressed warning is not put at all: the reader has already answered it,
/// and the point of switching it off is that the work just happens.
fn ask(warning: Warning, window: &mut Window, cx: &mut App, go: impl Fn(&mut Window, &mut App) + 'static) {
    if warning.suppressed(cx) {
        window.defer(cx, move |window, cx| go(window, cx));
        return;
    }

    let go: GoAhead = Rc::new(go);
    window.open_dialog(cx, move |dialog, _window, cx| {
        let go = Rc::clone(&go);
        let suppressed = warning.suppressed(cx);
        dialog
            .title(warning.title())
            .close_button(false)
            .child(
                v_flex()
                    .id(warning.id())
                    .gap_2()
                    .max_w(px(480.))
                    .child(div().text_sm().child(warning.message()))
                    .when(warning == Warning::Address, |this| {
                        this.child(
                            Button::new("notice-more-info")
                                .ghost()
                                .small()
                                .label(t!("ui.labels.more_info").into_owned())
                                .on_click(|_event, _window, cx: &mut App| cx.open_url(NETWORKING_URL)),
                        )
                    })
                    .child(
                        // Written as it is ticked, as the egui dialog's own
                        // checkbox is: ticking and then cancelling suppresses the
                        // warning in both apps.
                        Checkbox::new("notice-suppress")
                            .label(t!("ui.labels.suppress_warning").to_string())
                            .checked(suppressed)
                            .on_click(move |checked, _window, cx: &mut App| warning.suppress(*checked, cx)),
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

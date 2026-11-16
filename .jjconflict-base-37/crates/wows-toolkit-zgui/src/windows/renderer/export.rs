//! Starting an export, asking first when it would encode in software.
//!
//! A GPU encode of a long battle takes a while; a software one takes several times that, which is
//! worth knowing before the wait rather than after.

use rust_i18n::t;
use wows_toolkit_replay_data::replay_render::export::ExportTarget;
use wows_toolkit_viewmodel::notices::Notice;
use zgui::component;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::ButtonSize;
use zgui_ui_primitives::Binding;

use crate::windows::renderer::options::CSS;
use crate::windows::renderer::options::CheckRowProps;
use crate::windows::renderer::options::SHEET;
use crate::windows::renderer::session::RendererSession;

/// Starts an export to `target`, or puts the software-encode notice up first when the export
/// would encode in software and the reader has not switched the notice off.
pub(super) fn export_asking_first(session: RendererSession, target: ExportTarget) {
    if !session.export_needs_notice() || notice_suppressed(session) {
        session.start_export(target);
        return;
    }

    session.export_notice.set(Some(target));
}

/// Whether the reader switched the notice off. Settings not read yet count as not: the reader has
/// not said otherwise, and asking is the side that loses nothing.
fn notice_suppressed(session: RendererSession) -> bool {
    let Some(services) = session.services() else { return false };

    services.settings.read_untracked(|settings| settings.suppress_gpu_encoder_warning).unwrap_or(false)
}

/// The software-encode notice, while an export waits on it.
#[component]
pub fn ExportNotice(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let notice = Notice::SoftwareEncode;
    // Unticked while the settings are not read: no choice has been recorded to show.
    let suppressed = Signal::derive_local(move || {
        let services = session.services()?;
        services.settings.read(|settings| settings.suppress_gpu_encoder_warning)
    });
    let ticked = Signal::derive_local(move || suppressed.get() == Some(true));
    let open = Signal::derive_local(move || session.export_notice.with(Option::is_some));
    // Written as it is ticked, as the egui dialog's own checkbox is: ticking and then cancelling
    // suppresses the notice in every front end.
    let suppress = UnsyncCallback::new(move |on: bool| {
        let Some(services) = session.services() else { return };
        services.settings.edit(notice.setting_key(), |settings| {
            settings.suppress_gpu_encoder_warning = on;
            on
        });
    });
    let go_ahead = move || {
        let Some(target) = session.export_notice.get_untracked() else { return };
        session.export_notice.set(None);
        session.start_export(target);
    };

    view! {
        AlertDialog(open = Binding::controlled(open, move |open: bool| {
            if !open {
                session.export_notice.set(None);
            }
        })) {
            AlertDialogContent(attr:data-testid = "notice-software-encode") {
                AlertDialogHeader {
                    AlertDialogTitle {{notice.title()}}
                    AlertDialogDescription {{notice.message()}}
                }
                CheckRow(
                    label = t!("ui.labels.suppress_warning").into_owned(),
                    test_id = "notice-suppress".to_owned(),
                    checked = ticked,
                    on_change = suppress
                )
                AlertDialogFooter {
                    AlertDialogCancel(size = ButtonSize::Sm, attr:data-testid = "notice-cancel") {
                        {t!("ui.buttons.cancel").into_owned()}
                    }
                    AlertDialogAction(size = ButtonSize::Sm, attr:data-testid = "notice-continue", on:click = move |_| go_ahead()) {
                        {t!("ui.buttons.continue_").into_owned()}
                    }
                }
            }
        }
    }
}

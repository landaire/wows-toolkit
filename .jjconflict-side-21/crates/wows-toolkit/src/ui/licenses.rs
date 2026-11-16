//! The window that shows the application version, the project license and the notices of the
//! third-party components the application contains.

use egui::CollapsingHeader;
use egui::Label;
use egui::RichText;
use egui::ScrollArea;
use egui::TextEdit;
use egui::Ui;
use rust_i18n::t;
use wows_toolkit_notices::Notices;
use wows_toolkit_notices::PROJECT_LICENSE;
use wows_toolkit_notices::PROJECT_LICENSE_ID;

#[derive(Default)]
pub(crate) struct LicensesWindow {
    open: bool,
    query: String,
}

impl LicensesWindow {
    pub(crate) fn open(&mut self) {
        self.open = true;
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let query = &mut self.query;
        egui::Window::new(t!("ui.windows.licenses")).open(&mut self.open).default_size([640.0, 560.0]).show(ctx, |ui| {
            build_licenses_window(ui, query);
        });
    }
}

fn build_licenses_window(ui: &mut Ui, query: &mut String) {
    ui.label(t!("ui.labels.app_version", version = env!("CARGO_PKG_VERSION")));
    ui.label(t!("ui.labels.project_license", license = PROJECT_LICENSE_ID));
    CollapsingHeader::new(PROJECT_LICENSE_ID).id_salt("project_license").show(ui, |ui| {
        monospace_text(ui, PROJECT_LICENSE);
    });

    ui.separator();
    ui.heading(t!("ui.labels.third_party_notices"));
    ui.add(TextEdit::singleline(query).hint_text(t!("ui.labels.notices_search_hint")).desired_width(f32::INFINITY));

    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let mut any = false;
        for notice in Notices::embedded().matching(query) {
            any = true;
            // Titles repeat across entries; each body is a distinct slice of the embedded file.
            CollapsingHeader::new(notice.title()).id_salt(notice.body().as_ptr()).show(ui, |ui| {
                monospace_text(ui, notice.body());
            });
        }
        if !any {
            ui.label(t!("ui.labels.no_matching_notices"));
        }
    });
}

fn monospace_text(ui: &mut Ui, text: &str) {
    ui.add(Label::new(RichText::new(text).monospace()).selectable(true));
}

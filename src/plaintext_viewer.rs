use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use egui::{mutex::Mutex, FontFamily, RichText, TextEdit, ViewportBuilder};
use serde::{Deserialize, Serialize};

pub struct PlaintextFileViewer {
    pub title: Arc<String>,
    pub text: Arc<Mutex<String>>,
    pub open: Arc<AtomicBool>,
}

impl PlaintextFileViewer {
    pub fn draw(&mut self, ctx: &egui::Context) {
        let title = self.title.clone();
        let text = self.text.clone();
        let window_open = self.open.clone();
        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(&*self.title),
            ViewportBuilder::default().with_title(&*self.title),
            move |ctx, ui| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
                        let mut theme =
                            egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx());
                        let mut layout_job = egui_extras::syntax_highlighting::highlight(
                            ui.ctx(),
                            &theme,
                            string,
                            "xml",
                        );
                        layout_job.wrap.max_width = wrap_width;
                        ui.fonts(|f| f.layout_job(layout_job))
                    };
                    let mut text = text.lock();
                    let mut text_editor = TextEdit::multiline(&mut *text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add(text_editor);
                    });
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    // Tell parent to close us.
                    window_open.store(false, Ordering::Relaxed);
                }
            },
        );
    }
}

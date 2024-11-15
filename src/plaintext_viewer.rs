use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use egui::{mutex::Mutex, Image, ImageSource, TextEdit, ViewportBuilder};

pub enum FileType {
    PlainTextFile { ext: String, contents: String },
    Image { ext: String, contents: Vec<u8> },
}

pub struct PlaintextFileViewer {
    pub title: Arc<String>,
    pub file_info: Arc<Mutex<FileType>>,
    pub open: Arc<AtomicBool>,
}

impl PlaintextFileViewer {
    pub fn draw(&mut self, ctx: &egui::Context) {
        let title = self.title.clone();
        let info = self.file_info.clone();
        let window_open = self.open.clone();
        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(&*self.title),
            ViewportBuilder::default().with_title(&*self.title),
            move |ctx, _ui| {
                let mut file_info = info.lock();
                egui::CentralPanel::default().show(ctx, |ui| match &mut *file_info {
                    FileType::PlainTextFile { ext, contents } => {
                        let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
                            let style = ui.style().as_ref();
                            let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), style);
                            let mut layout_job = egui_extras::syntax_highlighting::highlight(ui.ctx(), style, &theme, string, &ext[1..]);
                            layout_job.wrap.max_width = wrap_width;
                            ui.fonts(|f| f.layout_job(layout_job))
                        };
                        let text_editor = TextEdit::multiline(contents).code_editor().desired_width(f32::INFINITY).layouter(&mut layouter);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add(text_editor);
                        });
                    }
                    FileType::Image { ext: _, contents } => {
                        let image = Image::new(ImageSource::Bytes {
                            uri: format!("bytes://{}", &*title).into(),
                            // the icon size is <1k, this clone is fairly cheap
                            bytes: contents.clone().into(),
                        });
                        ui.add(image);
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    // Tell parent to close us.
                    window_open.store(false, Ordering::Relaxed);
                }
            },
        );
    }
}

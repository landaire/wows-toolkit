use wowsunpack::export::gltf_export::armor_color_legend;

/// Draw the armor color legend widget.
///
/// Packed tight on the horizontal axis: this floats over the 3D viewport, so
/// every pixel of width hides ship. The window title already names the legend,
/// so there is no heading here.
pub fn show_armor_legend(ui: &mut egui::Ui) {
    let legend = armor_color_legend();

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
        for entry in &legend {
            ui.horizontal(|ui| {
                let color = egui::Color32::from_rgba_unmultiplied(
                    (entry.color[0] * 255.0) as u8,
                    (entry.color[1] * 255.0) as u8,
                    (entry.color[2] * 255.0) as u8,
                    (entry.color[3] * 255.0) as u8,
                );
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
                let range = if entry.max_mm >= 999.0 {
                    format!("{}+ mm", entry.min_mm as u32)
                } else {
                    format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32)
                };
                ui.label(egui::RichText::new(range).size(11.0));
            });
        }
    });
}

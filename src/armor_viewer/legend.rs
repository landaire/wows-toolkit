use wowsunpack::export::gltf_export::armor_color_legend;

/// Draw the armor color legend widget.
pub fn show_armor_legend(ui: &mut egui::Ui) {
    let legend = armor_color_legend();

    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Armor Thickness").strong().size(12.0));
        ui.add_space(2.0);
        for entry in &legend {
            ui.horizontal(|ui| {
                let color = egui::Color32::from_rgba_unmultiplied(
                    (entry.color[0] * 255.0) as u8,
                    (entry.color[1] * 255.0) as u8,
                    (entry.color[2] * 255.0) as u8,
                    (entry.color[3] * 255.0) as u8,
                );
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
                if entry.max_mm >= 999.0 {
                    ui.label(format!("{}+ mm", entry.min_mm as u32));
                } else {
                    ui.label(format!("{}-{} mm", entry.min_mm as u32, entry.max_mm as u32));
                }
            });
        }
    });
}

//! Graphite & Bone theme: palette, semantic colours, and egui style construction.

pub mod contrast;
pub mod palette;
pub mod semantic;
pub mod style;

use crate::data::settings::ThemeChoice;

/// Register both themes with the context. Call once at startup, before the
/// first frame. egui then swaps between them as the preference changes.
pub fn install(ctx: &egui::Context) {
    ctx.set_style_of(egui::Theme::Dark, style::dark_style());
    ctx.set_style_of(egui::Theme::Light, style::light_style());
}

/// Apply a theme choice. `System` follows the desktop preference.
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(egui::ThemePreference::from(choice));
}

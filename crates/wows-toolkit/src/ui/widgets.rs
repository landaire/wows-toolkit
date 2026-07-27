//! Small widgets repeated across tabs, kept in one place so the rendering
//! rule for each lives once instead of drifting per call site.

use egui::Color32;
use egui::Response;
use egui::RichText;
use egui::Ui;
use egui::Visuals;

use crate::util::personal_rating::PersonalRatingCategory;
use crate::util::personal_rating::PersonalRatingCategorySwatch;

/// The personal-rating chip. Single place that knows how a rating is drawn;
/// callers never touch `RatingSwatch` directly. `strong` matches the label's
/// existing `.strong()` sites; the returned `Response` still takes
/// `.on_hover_text(..)` at the call site.
pub fn pr_chip(ui: &mut Ui, category: PersonalRatingCategory, value: &str, strong: bool) -> Response {
    let swatch = category.swatch(ui.visuals());
    let mut text = RichText::new(value).color(swatch.text).background_color(swatch.tint);
    if strong {
        text = text.strong();
    }
    ui.label(text)
}

/// Legible colour for a small per-entity identity marker (collab peer dot,
/// ship/trajectory colour dot) drawn over the current panel background. The
/// repair (`readable_on`) lives here once; callers still choose how to draw
/// the marker (painted circle vs. inline glyph).
pub fn identity_dot_color(visuals: &Visuals, r: u8, g: u8, b: u8) -> Color32 {
    crate::ui::theme::contrast::readable_on(Color32::from_rgb(r, g, b), visuals.panel_fill)
}

/// Paints a small filled circle identity dot at the cursor and advances past
/// it, matching the collab peer-list dot layout.
pub fn identity_dot(ui: &mut Ui, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    response
}

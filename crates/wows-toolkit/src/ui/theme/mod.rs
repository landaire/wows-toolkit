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

/// Pins a pass to the dark theme, restoring the user's preference on drop.
///
/// egui keeps the theme in context-global options, not per viewport, so a
/// window that must stay dark while the rest of the app is light has to bracket
/// its own pass. Restyling the pass's root `Ui` is not enough on its own:
/// `Area`, `Window`, tooltip and menu contents read `Context::global_style`
/// rather than the `Ui` they were spawned from, and would come out light.
/// Conversely the context alone is not enough either, because egui builds the
/// root `Ui` at `begin_pass`, before the viewport callback runs. Hence both.
#[must_use = "the theme reverts when the guard drops"]
pub struct DarkPass {
    ctx: egui::Context,
    previous: egui::ThemePreference,
}

impl DarkPass {
    /// `root` is the `Ui` the pass hands to the viewport callback.
    pub fn force(root: &mut egui::Ui) -> Self {
        let ctx = root.ctx().clone();
        let previous = ctx.options(|opt| opt.theme_preference);
        ctx.set_theme(egui::ThemePreference::Dark);
        root.set_style(ctx.global_style());
        Self { ctx, previous }
    }
}

impl Drop for DarkPass {
    fn drop(&mut self) {
        self.ctx.set_theme(self.previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the deferred-viewport path: egui builds the root `Ui` at
    /// `begin_pass`, then hands it to the callback that installs the guard.
    #[test]
    fn dark_pass_covers_the_root_ui_and_new_areas() {
        let ctx = egui::Context::default();
        install(&ctx);
        apply(&ctx, ThemeChoice::Light);

        let mut observed = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |root| {
            let _guard = DarkPass::force(root);
            let nested = root.new_child(egui::UiBuilder::new());
            observed =
                Some((root.visuals().dark_mode, nested.visuals().dark_mode, ctx.global_style().visuals.dark_mode));
        });

        assert_eq!(observed, Some((true, true, true)));
    }

    #[test]
    fn dark_pass_restores_the_users_preference() {
        for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
            let ctx = egui::Context::default();
            install(&ctx);
            apply(&ctx, choice);

            let _ = ctx.run_ui(egui::RawInput::default(), |root| {
                let _guard = DarkPass::force(root);
            });

            assert_eq!(ctx.options(|opt| opt.theme_preference), egui::ThemePreference::from(choice));
        }
    }
}

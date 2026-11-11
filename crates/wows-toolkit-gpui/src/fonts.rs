//! The UI typeface: Inter (SIL OFL 1.1, `assets/fonts/LICENSE-INTER`), in
//! static weights. GPUI loads a variable face at its default instance only, so
//! a variable system font draws every weight as regular.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::LazyLock;

use gpui_kit::App;
use gpui_kit::FontFeatures;
use gpui_kit::Global;
use gpui_kit::SharedString;

/// The family every bundled face declares.
const FAMILY: &str = "Inter";

/// Regular, medium, semibold and bold: the weights the app draws.
const FACES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
];

/// The UI family, set once the bundled faces are registered.
struct UiFont(SharedString);

impl Global for UiFont {}

/// Registers the bundled faces and names their family as the UI font. On
/// failure the theme keeps the platform's UI font.
pub fn register(cx: &mut App) -> gpui_kit::Result<()> {
    cx.text_system().add_fonts(faces())?;
    cx.set_global(UiFont(FAMILY.into()));

    Ok(())
}

/// The family UI text is drawn in, when the bundled one is registered.
pub fn ui_family(cx: &App) -> Option<SharedString> {
    cx.try_global::<UiFont>().map(|font| font.0.clone())
}

/// Figures of one width, so numbers in a column line up digit for digit.
pub fn tabular_figures() -> FontFeatures {
    static TABULAR: LazyLock<FontFeatures> = LazyLock::new(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)])));

    TABULAR.clone()
}

fn faces() -> Vec<Cow<'static, [u8]>> {
    FACES.iter().map(|face| Cow::Borrowed(*face)).collect()
}

#[cfg(test)]
mod tests {
    use gpui_kit::Font;
    use gpui_kit::FontId;
    use gpui_kit::FontRun;
    use gpui_kit::FontWeight;
    use gpui_kit::PlatformTextSystem;
    use gpui_kit::font;
    use gpui_kit::px;
    use gpui_wgpu::CosmicTextSystem;

    use super::*;

    /// The text system the Linux and Windows builds draw with, holding the
    /// bundled faces and nothing installed on the host.
    fn text_system() -> CosmicTextSystem {
        let text_system = CosmicTextSystem::new_without_system_fonts(FAMILY);
        text_system.add_fonts(faces()).expect("the bundled faces load");

        text_system
    }

    fn weighted(weight: FontWeight) -> Font {
        let mut weighted = font(FAMILY);
        weighted.weight = weight;

        weighted
    }

    fn width(text_system: &CosmicTextSystem, font_id: FontId, text: &str) -> f32 {
        text_system.layout_line(text, px(13.), &[FontRun { len: text.len(), font_id }]).width.as_f32()
    }

    #[test]
    fn each_weight_resolves_to_its_own_face() {
        let text_system = text_system();

        let ids: Vec<FontId> = [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::SEMIBOLD, FontWeight::BOLD]
            .into_iter()
            .map(|weight| text_system.font_id(&weighted(weight)).expect("the family resolves"))
            .collect();

        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[index + 1..].contains(id), "weight {index} shares a face with a heavier one: {ids:?}");
        }
    }

    #[test]
    fn tabular_figures_are_one_width() {
        let text_system = text_system();
        let proportional = text_system.font_id(&font(FAMILY)).expect("the family resolves");
        let mut tabular = font(FAMILY);
        tabular.features = tabular_figures();
        let tabular = text_system.font_id(&tabular).expect("the family resolves");

        assert_ne!(width(&text_system, proportional, "111"), width(&text_system, proportional, "000"));
        assert_eq!(width(&text_system, tabular, "111"), width(&text_system, tabular, "000"));
    }
}

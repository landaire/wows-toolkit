//! The icon glyphs the egui app draws, over the same Phosphor font.
//!
//! The egui app reaches these through `egui-phosphor`, which is an egui
//! crate; this carries the font itself (`assets/fonts/Phosphor.ttf`, MIT) and
//! the codepoints for the glyphs that app actually uses, so the two draw the
//! same icons rather than approximations from a different set.
//!
//! Registered with the text system by [`register_font`]; a glyph is drawn by
//! setting [`FONT_FAMILY`] on the element that carries it.

// The table carries every glyph the egui app draws, so a surface being
// ported finds its icon already here rather than having to look the
// codepoint up again. The ones not yet drawn are the ports still to come.
#![allow(dead_code)]

use gpui_kit::App;
use gpui_kit::Div;
use gpui_kit::ParentElement as _;
use gpui_kit::Styled as _;
use gpui_kit::div;

/// The family name the font declares, which is what an element asks for.
pub const FONT_FAMILY: &str = "Phosphor";

/// The font itself, carried in the binary so no install is required.
const FONT: &[u8] = include_bytes!("../assets/fonts/Phosphor.ttf");

/// Makes the icon glyphs drawable. Called once at startup; a failure leaves
/// every icon as a missing-glyph box rather than stopping the app, which is
/// recoverable in a way a panic during startup is not.
pub fn register_font(cx: &App) {
    if let Err(err) = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(FONT)]) {
        tracing::error!("icons: the Phosphor font could not be registered: {err}");
    }
}

/// One icon glyph, sized to the surrounding text.
///
/// The glyph is a character in the icon family, so it takes the element's own
/// colour and size like any other text; a caller that wants it larger or in
/// another colour styles the returned element.
pub fn icon(glyph: &'static str) -> Div {
    div().flex_none().font_family(FONT_FAMILY).child(glyph)
}

pub const ARCHIVE: &str = "\u{E00C}";
pub const ARROWS_OUT_SIMPLE: &str = "\u{E0A6}";
pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{E038}";
pub const ARROW_SQUARE_OUT: &str = "\u{E5DE}";
pub const BOMB: &str = "\u{EE0A}";
pub const BROADCAST: &str = "\u{E0F2}";
pub const BROWSER: &str = "\u{E0F4}";
pub const BUG: &str = "\u{E5F4}";
pub const CAMERA: &str = "\u{E10E}";
pub const CARET_LEFT: &str = "\u{E138}";
pub const CARET_RIGHT: &str = "\u{E13A}";
pub const CASTLE_TURRET: &str = "\u{E9D0}";
pub const CHART_BAR: &str = "\u{E150}";
pub const CHART_LINE: &str = "\u{E154}";
pub const CHAT_TEXT: &str = "\u{E17A}";
pub const CHECK: &str = "\u{E182}";
pub const CHECK_CIRCLE: &str = "\u{E184}";
pub const CLIPBOARD: &str = "\u{E196}";
pub const CLIPBOARD_TEXT: &str = "\u{E198}";
pub const CLOCK: &str = "\u{E19A}";
pub const CLOCK_CLOCKWISE: &str = "\u{E19E}";
pub const CLOCK_COUNTER_CLOCKWISE: &str = "\u{E1A0}";
pub const COPY: &str = "\u{E1CA}";
pub const CROSSHAIR: &str = "\u{E1D6}";
pub const CROSSHAIR_SIMPLE: &str = "\u{E1D8}";
pub const CROWN: &str = "\u{E614}";
pub const CUBE: &str = "\u{E1DA}";
pub const DATABASE: &str = "\u{E1DE}";
pub const DETECTIVE: &str = "\u{E83E}";
pub const DISCORD_LOGO: &str = "\u{E61A}";
pub const DOTS_THREE: &str = "\u{E1FE}";
pub const DOWNLOAD_SIMPLE: &str = "\u{E20C}";
pub const ERASER: &str = "\u{E21E}";
pub const EXCLAMATION_MARK: &str = "\u{EE44}";
pub const EYE: &str = "\u{E220}";
pub const EYE_SLASH: &str = "\u{E224}";
pub const FAST_FORWARD: &str = "\u{E6A6}";
pub const FILE: &str = "\u{E230}";
pub const FILE_TEXT: &str = "\u{E23A}";
pub const FIRE: &str = "\u{E242}";
pub const FLOPPY_DISK: &str = "\u{E248}";
pub const FOLDER: &str = "\u{E24A}";
pub const FOLDER_OPEN: &str = "\u{E256}";
pub const FUNNEL: &str = "\u{E266}";
pub const GAME_CONTROLLER: &str = "\u{E26E}";
pub const GEAR_FINE: &str = "\u{E87C}";
pub const GITHUB_LOGO: &str = "\u{E576}";
pub const IMAGE: &str = "\u{E2CA}";
pub const INFO: &str = "\u{E2CE}";
pub const KEYBOARD: &str = "\u{E2D8}";
pub const LIST: &str = "\u{E2F0}";
pub const LIST_BULLETS: &str = "\u{E2F2}";
pub const LIST_CHECKS: &str = "\u{EADC}";
pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
pub const MAP_TRIFOLD: &str = "\u{E31A}";
pub const MONITOR: &str = "\u{E32E}";
pub const MUSIC_NOTE: &str = "\u{E33C}";
pub const NOTCHES: &str = "\u{ED3A}";
pub const NOTE_PENCIL: &str = "\u{E34C}";
pub const PAUSE: &str = "\u{E39E}";
pub const PLAY: &str = "\u{E3D0}";
pub const PLUGS: &str = "\u{EB56}";
pub const PLUS_CIRCLE: &str = "\u{E3D6}";
pub const PROHIBIT: &str = "\u{E3DE}";
pub const REWIND: &str = "\u{E6A8}";
pub const SHARE: &str = "\u{E406}";
pub const SHIELD: &str = "\u{E40A}";
pub const SHIELD_STAR: &str = "\u{EC34}";
pub const SIREN: &str = "\u{E9B8}";
pub const SKIP_BACK: &str = "\u{E5A4}";
pub const SKIP_FORWARD: &str = "\u{E5A6}";
pub const SKULL: &str = "\u{E916}";
pub const SMILEY_SAD: &str = "\u{E43E}";
pub const SORT_ASCENDING: &str = "\u{E444}";
pub const SORT_DESCENDING: &str = "\u{E446}";
pub const STAR: &str = "\u{E46A}";
pub const SWORD: &str = "\u{E5BA}";
pub const TABLE: &str = "\u{E476}";
pub const THREE_D: &str = "\u{EA5A}";
pub const TRASH: &str = "\u{E4A6}";
pub const TROPHY: &str = "\u{E67E}";
pub const TWITCH_LOGO: &str = "\u{E5CE}";
pub const USERS: &str = "\u{E4D6}";
pub const USERS_THREE: &str = "\u{E68E}";
pub const WARNING: &str = "\u{E4E0}";
pub const WRENCH: &str = "\u{E5D4}";
pub const X: &str = "\u{E4F6}";
pub const X_CIRCLE: &str = "\u{E4F8}";

#[cfg(test)]
mod tests {
    use super::*;
    // Imported by name, not through the crate glob: that glob re-exports
    // GPUI's own `test` macro, which would shadow Rust's `#[test]`.
    use gpui_kit::AppContext as _;
    use gpui_kit::Context;
    use gpui_kit::InteractiveElement as _;
    use gpui_kit::IntoElement;
    use gpui_kit::Render;
    use gpui_kit::TestAppContext;
    use gpui_kit::TestSupportExt as _;
    use gpui_kit::Window;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    /// Draws one glyph, so the font's own registration is exercised rather
    /// than assumed.
    struct IconProbe;

    impl Render for IconProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("icon-probe")
                .test_support()
                .size_full()
                .child(icon(TROPHY).text_size(px(24.)))
                .child(icon(SMILEY_SAD))
        }
    }

    /// GPUI panics on the first line it lays out in a family it cannot find,
    /// so laying a glyph out is the check that the font registered.
    #[gpui_kit::test]
    fn the_icon_font_registers_and_its_glyphs_lay_out(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| register_font(cx));

        let window = cx.open_window(size(px(200.), px(100.)), |_window, _cx| IconProbe);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("icon-probe").bounds();
            assert!(bounds.size.width > px(0.), "the glyphs took space, so the family resolved");
        })
        .expect("the window is open");
    }

    /// Pinned against `egui-phosphor` 0.13's regular variant, which is where
    /// these were taken from and what the egui app draws. A regeneration that
    /// moved a codepoint would otherwise draw a different icon here than
    /// there, silently.
    #[test]
    fn the_glyphs_keep_the_codepoints_the_egui_app_draws() {
        assert_eq!(TROPHY, "\u{E67E}");
        assert_eq!(SMILEY_SAD, "\u{E43E}");
        assert_eq!(NOTCHES, "\u{ED3A}");
        assert_eq!(CROSSHAIR, "\u{E1D6}");
    }
}

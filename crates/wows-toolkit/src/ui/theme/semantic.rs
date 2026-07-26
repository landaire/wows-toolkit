//! Semantic colours: what a colour *means*, resolved per theme. UI code names
//! roles (`ui.sem().win`) so a palette change moves the whole app and no call
//! site can be tuned for one background.

use egui::Color32;

/// Colours for the in-game chat channels.
pub struct ChatColors {
    pub division: Color32,
    pub global: Color32,
    pub team: Color32,
    pub other: Color32,
}

/// Colours for the armor viewer's angle bands and penetration outcomes.
///
/// `pen` and `angle_bad` currently share a value, as do `overpen` and
/// `angle_mid`. They stay separate fields because they are separate meanings
/// on separate widgets, and either may be retuned without disturbing the
/// other. Do not collapse them.
pub struct ArmorColors {
    pub angle_good: Color32,
    pub angle_mid: Color32,
    pub angle_bad: Color32,
    pub pen: Color32,
    pub overpen: Color32,
    pub ricochet: Color32,
    pub shatter: Color32,
}

/// Every meaning the UI attaches to a colour.
pub struct SemanticColors {
    pub win: Color32,
    pub loss: Color32,
    pub draw: Color32,
    pub warn: Color32,
    pub error: Color32,
    /// Emphasised body text. Replaces bare `Color32::WHITE`.
    pub text_strong: Color32,
    /// De-emphasised detail text. Replaces bare `Color32::GRAY`.
    pub text_dim: Color32,
    /// Division mates.
    pub division: Color32,
    /// Tint for affordance icons that are not status, such as folders.
    pub icon_accent: Color32,
    /// Players flagged by the abuse list.
    pub abuser: Color32,
    pub chat: ChatColors,
    pub armor: ArmorColors,
}

const DARK: SemanticColors = SemanticColors {
    win: Color32::from_rgb(0x6F, 0xD9, 0x8A),
    loss: Color32::from_rgb(0xE8, 0x65, 0x6E),
    draw: Color32::from_rgb(0xCF, 0xC8, 0xB6),
    warn: Color32::from_rgb(0xE8, 0xA5, 0x4A),
    error: Color32::from_rgb(0xF2, 0x72, 0x7C),
    text_strong: Color32::from_rgb(0xFA, 0xF8, 0xF1),
    text_dim: Color32::from_rgb(0x8E, 0x8B, 0x82),
    division: Color32::from_rgb(0xE5, 0xC1, 0x58),
    icon_accent: Color32::from_rgb(0xE5, 0xC1, 0x58),
    abuser: Color32::from_rgb(0xF0, 0x9B, 0xC0),
    chat: ChatColors {
        division: Color32::from_rgb(0xE5, 0xC1, 0x58),
        global: Color32::from_rgb(0xDE, 0xDB, 0xD2),
        team: Color32::from_rgb(0x6F, 0xD9, 0x8A),
        other: Color32::from_rgb(0xE8, 0xA5, 0x4A),
    },
    armor: ArmorColors {
        angle_good: Color32::from_rgb(0x64, 0xD9, 0x8A),
        angle_mid: Color32::from_rgb(0xE0, 0xBE, 0x64),
        angle_bad: Color32::from_rgb(0xE8, 0x73, 0x7B),
        pen: Color32::from_rgb(0xE8, 0x73, 0x7B),
        overpen: Color32::from_rgb(0xE0, 0xBE, 0x64),
        ricochet: Color32::from_rgb(0x7F, 0xB4, 0xE8),
        shatter: Color32::from_rgb(0xA9, 0xA4, 0x9A),
    },
};

const LIGHT: SemanticColors = SemanticColors {
    win: Color32::from_rgb(0x12, 0x79, 0x3A),
    loss: Color32::from_rgb(0xB0, 0x1F, 0x2B),
    draw: Color32::from_rgb(0x5F, 0x5C, 0x52),
    warn: Color32::from_rgb(0x8A, 0x4B, 0x00),
    error: Color32::from_rgb(0xA8, 0x1F, 0x2A),
    text_strong: Color32::from_rgb(0x0A, 0x0A, 0x08),
    text_dim: Color32::from_rgb(0x5C, 0x5A, 0x53),
    division: Color32::from_rgb(0x7A, 0x5A, 0x00),
    icon_accent: Color32::from_rgb(0x7A, 0x5A, 0x00),
    abuser: Color32::from_rgb(0xA3, 0x32, 0x70),
    chat: ChatColors {
        division: Color32::from_rgb(0x7A, 0x5A, 0x00),
        global: Color32::from_rgb(0x1A, 0x1A, 0x17),
        team: Color32::from_rgb(0x12, 0x79, 0x3A),
        other: Color32::from_rgb(0x8A, 0x4B, 0x00),
    },
    armor: ArmorColors {
        angle_good: Color32::from_rgb(0x14, 0x7A, 0x3C),
        angle_mid: Color32::from_rgb(0x7A, 0x5A, 0x08),
        angle_bad: Color32::from_rgb(0xAE, 0x22, 0x30),
        pen: Color32::from_rgb(0xAE, 0x22, 0x30),
        overpen: Color32::from_rgb(0x7A, 0x5A, 0x08),
        ricochet: Color32::from_rgb(0x1B, 0x5F, 0xA8),
        shatter: Color32::from_rgb(0x5F, 0x5C, 0x52),
    },
};

/// Resolve the semantic set for the active theme.
pub fn semantic(visuals: &egui::Visuals) -> &'static SemanticColors {
    if visuals.dark_mode { &DARK } else { &LIGHT }
}

/// Ergonomic access from a `Ui` or `Visuals`.
pub trait SemanticExt {
    fn sem(&self) -> &'static SemanticColors;
}

impl SemanticExt for egui::Ui {
    fn sem(&self) -> &'static SemanticColors {
        semantic(self.visuals())
    }
}

impl SemanticExt for egui::Visuals {
    fn sem(&self) -> &'static SemanticColors {
        semantic(self)
    }
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;
    use crate::ui::theme::contrast::CONTRAST_FLOOR;
    use crate::ui::theme::contrast::contrast_ratio;
    use crate::ui::theme::palette;

    /// Every semantic role, named, so a failure says which one broke.
    fn roles(sem: &SemanticColors) -> Vec<(&'static str, Color32)> {
        vec![
            ("win", sem.win),
            ("loss", sem.loss),
            ("draw", sem.draw),
            ("warn", sem.warn),
            ("error", sem.error),
            ("text_strong", sem.text_strong),
            ("text_dim", sem.text_dim),
            ("division", sem.division),
            ("icon_accent", sem.icon_accent),
            ("abuser", sem.abuser),
            ("chat.division", sem.chat.division),
            ("chat.global", sem.chat.global),
            ("chat.team", sem.chat.team),
            ("chat.other", sem.chat.other),
            ("armor.angle_good", sem.armor.angle_good),
            ("armor.angle_mid", sem.armor.angle_mid),
            ("armor.angle_bad", sem.armor.angle_bad),
            ("armor.pen", sem.armor.pen),
            ("armor.overpen", sem.armor.overpen),
            ("armor.ricochet", sem.armor.ricochet),
            ("armor.shatter", sem.armor.shatter),
        ]
    }

    #[test]
    fn every_dark_role_clears_the_floor_on_panel_and_card() {
        for (name, color) in roles(&DARK) {
            for (surface_name, surface) in [("panel", palette::dark::PANEL), ("card", palette::dark::CARD)] {
                let r = contrast_ratio(color, surface);
                assert!(r >= CONTRAST_FLOOR, "dark {name} on {surface_name} is {r}, needs {CONTRAST_FLOOR}");
            }
        }
    }

    #[test]
    fn every_light_role_clears_the_floor_on_panel_and_card() {
        for (name, color) in roles(&LIGHT) {
            for (surface_name, surface) in [("panel", palette::light::PANEL), ("card", palette::light::CARD)] {
                let r = contrast_ratio(color, surface);
                assert!(r >= CONTRAST_FLOOR, "light {name} on {surface_name} is {r}, needs {CONTRAST_FLOOR}");
            }
        }
    }

    #[test]
    fn chrome_text_clears_the_floor_on_every_surface() {
        for (theme, text, dim, surfaces) in [
            (
                "dark",
                palette::dark::TEXT,
                palette::dark::TEXT_DIM,
                [palette::dark::SURFACE, palette::dark::PANEL, palette::dark::CARD, palette::dark::WIDGET],
            ),
            (
                "light",
                palette::light::TEXT,
                palette::light::TEXT_DIM,
                [palette::light::SURFACE, palette::light::PANEL, palette::light::CARD, palette::light::WIDGET],
            ),
        ] {
            for surface in surfaces {
                for (name, fg) in [("text", text), ("text_dim", dim)] {
                    let r = contrast_ratio(fg, surface);
                    assert!(r >= CONTRAST_FLOOR, "{theme} {name} on {surface:?} is {r}");
                }
            }
        }
    }

    #[test]
    fn inverted_active_state_is_legible() {
        // The signature move: bone fill with the panel colour knocked out of it.
        for (theme, accent, panel) in [
            ("dark", palette::dark::ACCENT, palette::dark::PANEL),
            ("light", palette::light::ACCENT, palette::light::PANEL),
        ] {
            let r = contrast_ratio(panel, accent);
            assert!(r >= CONTRAST_FLOOR, "{theme} inverted active label is {r}");
        }
    }

    #[test]
    fn semantic_selects_by_dark_mode_flag() {
        let mut dark = egui::Visuals::dark();
        dark.dark_mode = true;
        assert_eq!(semantic(&dark).win, DARK.win);

        let mut light = egui::Visuals::light();
        light.dark_mode = false;
        assert_eq!(semantic(&light).win, LIGHT.win);
    }
}

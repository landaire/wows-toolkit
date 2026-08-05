//! Semantic colours: what a colour *means*, resolved per theme. UI code names
//! roles (`ui.sem().win`) so a palette change moves the whole app and no call
//! site can be tuned for one background.

use egui::Color32;

use crate::ui::theme::palette;

/// Colours for the in-game chat channels.
pub struct ChatColors {
    pub division: Color32,
    pub global: Color32,
    pub team: Color32,
    pub other: Color32,
}

/// Colours for the armor viewer's angle bands and penetration outcomes.
///
/// `overpen` and `angle_mid` currently share a value. `pen` and `angle_bad`
/// do not: `pen` reads green (a shell getting through is the answer the
/// viewer exists to give), while `angle_bad` stays red. They stay separate
/// fields because they are separate meanings on separate widgets, and either
/// may be retuned without disturbing the other. Do not collapse them.
pub struct ArmorColors {
    pub angle_good: Color32,
    pub angle_mid: Color32,
    pub angle_bad: Color32,
    pub pen: Color32,
    pub overpen: Color32,
    pub ricochet: Color32,
    pub shatter: Color32,
}

/// Outlines for the query bar's nested bracket groups. Nesting is carried by
/// the bracket's stroke rather than by a background fill: this theme's chrome
/// band spans only about 1.45:1 end to end, so a fill loud enough to be told
/// apart from both the bar under it and the pills over it has to leave the
/// band entirely and reads as a slab. A stroke steps within the band instead,
/// and dropping the fill also gives a pill the most contrast available against
/// what is behind it, which is the bar itself.
///
/// Both are border tones the theme already draws elsewhere, so a bracket reads
/// as chrome rather than as a new kind of object.
///
/// These are outlines, not text, so they are deliberately absent from the
/// `roles()` list the contrast tests walk: that list asserts a colour is
/// readable *as text on* the app's surfaces, which is the wrong question for a
/// line. What they must clear instead is `contrast::CHROME_LINE_FLOOR` against
/// the surface they sit on and `contrast::SURFACE_CONTRAST_FLOOR` against each
/// other, which `query_bar::paint`'s `depth_stroke_clears_its_floors` pins.
pub struct BracketColors {
    /// The outermost bracket: drawn directly on the bar, heaviest and
    /// brightest, so nesting reads as receding from it.
    pub shallow: Color32,
    /// Every level below the first. See `paint::depth_stroke` for why the
    /// ramp stops at two.
    pub deep: Color32,
}

/// Chip fills for a match-outcome badge (the search results table's Result
/// column). Distinct from `win`/`loss`/`draw`: those are flat text colours
/// used elsewhere (the replay listing row's identity tint, chat), and being
/// flat text on the panel binds them to the narrow lightness band that clears
/// `CONTRAST_FLOOR` there -- a band too narrow for three roles to also clear
/// `SURFACE_CONTRAST_FLOOR` against each other, in the light theme most
/// visibly (`win`/`loss`/`draw` there sit at 1.02-1.05:1 apart, all well under
/// the 1.3 floor). A chip fill carries no such binding: it only has to be
/// told apart from its neighbours and from the row it sits on, not double as
/// legible text on its own, so it is free to use the lightness room that
/// requirement was consuming. The label painted on top of the fill is
/// `contrast::label_on(fill)`, legible by construction.
pub struct OutcomeChipColors {
    pub win: Color32,
    pub loss: Color32,
    pub draw: Color32,
}

/// Blends `fg` `num`/`den` of the way over `bg`. Lets a derived chrome tone be
/// written as what it is, a proportion of a role colour over a surface, instead
/// of a hex literal that silently stops matching when either end is retuned.
const fn blend(fg: Color32, bg: Color32, num: u16, den: u16) -> Color32 {
    const fn mix(f: u8, b: u8, num: u16, den: u16) -> u8 {
        ((f as u16 * num + b as u16 * (den - num) + den / 2) / den) as u8
    }
    Color32::from_rgb(mix(fg.r(), bg.r(), num, den), mix(fg.g(), bg.g(), num, den), mix(fg.b(), bg.b(), num, den))
}

/// `error`, named so `blend` can refer to it before the `SemanticColors`
/// literal that also assigns it to the `error` field exists.
const DARK_ERROR: Color32 = Color32::from_rgb(0xF2, 0x72, 0x7C);
const LIGHT_ERROR: Color32 = Color32::from_rgb(0xA8, 0x1F, 0x2A);

/// Every meaning the UI attaches to a colour.
pub struct SemanticColors {
    pub win: Color32,
    pub loss: Color32,
    pub draw: Color32,
    pub warn: Color32,
    pub error: Color32,
    /// A check passed or a scan found no problems. Distinct from `win`, which
    /// is a battle result.
    pub ok: Color32,
    /// Emphasised body text. Replaces bare `Color32::WHITE`.
    pub text_strong: Color32,
    /// De-emphasised detail text. Held to `DIM_CONTRAST_FLOOR`, not
    /// `CONTRAST_FLOOR`, so it reads as secondary to body text rather than
    /// level with it.
    pub text_dim: Color32,
    /// Division mates.
    pub division: Color32,
    /// Tint for affordance icons that are not status, such as folders.
    pub icon_accent: Color32,
    /// Players flagged by the abuse list.
    pub abuser: Color32,
    /// Session host marker. Distinct from `division`, which is a division mate.
    pub crown_host: Color32,
    /// Session co-host marker. Distinct from `abuser`, which flags a reported player.
    pub crown_cohost: Color32,
    /// A value worth noticing that is not a warning.
    pub notice: Color32,
    /// Fill for a control that is engaged, as opposed to merely selected or
    /// hovered. Inverts to bone rather than tinting, so an engaged mode toggle
    /// reads across a busy 3D viewport.
    pub engaged_fill: Color32,
    /// Label on `engaged_fill`, knocked out to the panel tone.
    pub engaged_label: Color32,
    pub chat: ChatColors,
    pub armor: ArmorColors,
    pub bracket: BracketColors,
    pub outcome_chip: OutcomeChipColors,
    /// Divider between two segments of one query-bar pill.
    ///
    /// A pill's segments sit on the pill's own fill (idle, hovered, or
    /// selected), not on the bar, so this cannot borrow `bracket`'s tones: a
    /// `BracketColors` guarantee is contrast against the bar, and neither
    /// bracket tone means "divider inside a filled pill". Kept as its own
    /// field so retuning bracket depth cannot silently starve this of
    /// contrast, and vice versa.
    ///
    /// Like `bracket`, this is chrome rather than text on a surface, so it is
    /// deliberately absent from the `roles()` list the contrast tests walk.
    /// What it must clear instead is `contrast::CHROME_LINE_FLOOR` against
    /// every fill a pill can have, which `query_bar::paint`'s
    /// `pill_separator_clears_every_pill_fill` pins.
    pub pill_separator: Color32,
    /// Fill for a dock tab flagging that it needs attention: `error` blended a
    /// tenth of the way over `SURFACE`. The tab is tinted rather than filled so
    /// the active tab keeps the only full-strength fill in the strip.
    ///
    /// Like `bracket`, this is chrome rather than text on a surface, so it is
    /// deliberately absent from the `roles()` list the contrast tests walk:
    /// that list asserts a colour is readable *as text on* the app's surfaces,
    /// which is the wrong question for a fill.
    pub alert_tab_fill: Color32,
    /// Outline for that tab: the same blend at half strength, which carries the
    /// mark when the fill alone is too dim to catch the eye.
    pub alert_tab_outline: Color32,
}

pub const DARK: SemanticColors = SemanticColors {
    win: Color32::from_rgb(0x6F, 0xD9, 0x8A),
    loss: Color32::from_rgb(0xEA, 0x70, 0x78),
    draw: Color32::from_rgb(0xCF, 0xC8, 0xB6),
    warn: Color32::from_rgb(0xE8, 0xA5, 0x4A),
    error: DARK_ERROR,
    ok: Color32::from_rgb(0x6F, 0xD9, 0x8A),
    text_strong: Color32::from_rgb(0xE8, 0xE5, 0xDC),
    text_dim: Color32::from_rgb(0x7C, 0x7C, 0x6E),
    division: Color32::from_rgb(0xE5, 0xC1, 0x58),
    icon_accent: Color32::from_rgb(0xE5, 0xC1, 0x58),
    abuser: Color32::from_rgb(0xF0, 0x9B, 0xC0),
    crown_host: Color32::from_rgb(0xE5, 0xC1, 0x58),
    crown_cohost: Color32::from_rgb(0xF0, 0x9B, 0xC0),
    notice: Color32::from_rgb(0xE5, 0xC1, 0x58),
    engaged_fill: Color32::from_rgb(0xC7, 0xC3, 0xB8),
    engaged_label: Color32::from_rgb(0x18, 0x18, 0x16),
    chat: ChatColors {
        division: Color32::from_rgb(0xE5, 0xC1, 0x58),
        global: Color32::from_rgb(0xC9, 0xC6, 0xBE),
        team: Color32::from_rgb(0x6F, 0xD9, 0x8A),
        other: Color32::from_rgb(0xE8, 0xA5, 0x4A),
    },
    bracket: BracketColors { shallow: palette::dark::BORDER_BRIGHT, deep: palette::dark::BORDER },
    outcome_chip: OutcomeChipColors {
        win: Color32::from_rgb(0x19, 0x4D, 0x1E),
        loss: Color32::from_rgb(0xF2, 0x8C, 0xA6),
        draw: Color32::from_rgb(0xED, 0xE8, 0xAB),
    },
    pill_separator: palette::dark::BORDER_BRIGHT,
    armor: ArmorColors {
        angle_good: Color32::from_rgb(0x64, 0xD9, 0x8A),
        angle_mid: Color32::from_rgb(0xE0, 0xBE, 0x64),
        angle_bad: Color32::from_rgb(0xE8, 0x73, 0x7B),
        pen: Color32::from_rgb(0x6F, 0xD9, 0x8A),
        overpen: Color32::from_rgb(0xE0, 0xBE, 0x64),
        ricochet: Color32::from_rgb(0x7F, 0xB4, 0xE8),
        shatter: Color32::from_rgb(0xA9, 0xA4, 0x9A),
    },
    alert_tab_fill: blend(DARK_ERROR, palette::dark::SURFACE, 1, 10),
    alert_tab_outline: blend(DARK_ERROR, palette::dark::SURFACE, 1, 2),
};

pub const LIGHT: SemanticColors = SemanticColors {
    win: Color32::from_rgb(0x10, 0x6C, 0x34),
    loss: Color32::from_rgb(0xB0, 0x1F, 0x2B),
    draw: Color32::from_rgb(0x5F, 0x5C, 0x52),
    warn: Color32::from_rgb(0x8A, 0x4B, 0x00),
    error: LIGHT_ERROR,
    ok: Color32::from_rgb(0x10, 0x6C, 0x34),
    text_strong: Color32::from_rgb(0x0A, 0x0A, 0x08),
    text_dim: Color32::from_rgb(0x78, 0x76, 0x6F),
    division: Color32::from_rgb(0x77, 0x58, 0x00),
    icon_accent: Color32::from_rgb(0x77, 0x58, 0x00),
    abuser: Color32::from_rgb(0xA3, 0x32, 0x70),
    crown_host: Color32::from_rgb(0x77, 0x58, 0x00),
    crown_cohost: Color32::from_rgb(0xA3, 0x32, 0x70),
    notice: Color32::from_rgb(0x77, 0x58, 0x00),
    engaged_fill: Color32::from_rgb(0x26, 0x25, 0x1F),
    engaged_label: Color32::from_rgb(0xF4, 0xF3, 0xEF),
    chat: ChatColors {
        division: Color32::from_rgb(0x77, 0x58, 0x00),
        global: Color32::from_rgb(0x1A, 0x1A, 0x17),
        team: Color32::from_rgb(0x10, 0x6C, 0x34),
        other: Color32::from_rgb(0x8A, 0x4B, 0x00),
    },
    bracket: BracketColors { shallow: palette::light::BORDER_BRIGHT, deep: palette::light::BORDER },
    outcome_chip: OutcomeChipColors {
        win: Color32::from_rgb(0x09, 0x53, 0x0F),
        loss: Color32::from_rgb(0x25, 0x04, 0x0C),
        draw: Color32::from_rgb(0xCA, 0xB2, 0x91),
    },
    pill_separator: palette::light::BORDER_BRIGHT,
    armor: ArmorColors {
        angle_good: Color32::from_rgb(0x11, 0x6B, 0x34),
        angle_mid: Color32::from_rgb(0x78, 0x58, 0x08),
        angle_bad: Color32::from_rgb(0xAE, 0x22, 0x30),
        pen: Color32::from_rgb(0x10, 0x6C, 0x34),
        overpen: Color32::from_rgb(0x78, 0x58, 0x08),
        ricochet: Color32::from_rgb(0x1B, 0x5F, 0xA8),
        shatter: Color32::from_rgb(0x5F, 0x5C, 0x52),
    },
    alert_tab_fill: blend(LIGHT_ERROR, palette::light::SURFACE, 1, 10),
    alert_tab_outline: blend(LIGHT_ERROR, palette::light::SURFACE, 1, 2),
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
            ("ok", sem.ok),
            ("text_strong", sem.text_strong),
            ("division", sem.division),
            ("icon_accent", sem.icon_accent),
            ("abuser", sem.abuser),
            ("crown_host", sem.crown_host),
            ("crown_cohost", sem.crown_cohost),
            ("notice", sem.notice),
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
    fn every_dark_role_clears_the_floor_on_every_text_surface() {
        for (name, color) in roles(&DARK) {
            for (surface_name, surface) in [
                ("panel", palette::dark::PANEL),
                ("card", palette::dark::CARD),
                ("faint", palette::dark::FAINT),
                ("selection", palette::dark::SELECTION),
            ] {
                let r = contrast_ratio(color, surface);
                assert!(r >= CONTRAST_FLOOR, "dark {name} on {surface_name} is {r}, needs {CONTRAST_FLOOR}");
            }
        }
    }

    #[test]
    fn every_light_role_clears_the_floor_on_every_text_surface() {
        for (name, color) in roles(&LIGHT) {
            for (surface_name, surface) in [
                ("panel", palette::light::PANEL),
                ("card", palette::light::CARD),
                ("faint", palette::light::FAINT),
                ("selection", palette::light::SELECTION),
            ] {
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
                [
                    palette::dark::SURFACE,
                    palette::dark::PANEL,
                    palette::dark::CARD,
                    palette::dark::WIDGET,
                    palette::dark::WIDGET_HOT,
                    palette::dark::FAINT,
                    palette::dark::SELECTION,
                ],
            ),
            (
                "light",
                palette::light::TEXT,
                palette::light::TEXT_DIM,
                [
                    palette::light::SURFACE,
                    palette::light::PANEL,
                    palette::light::CARD,
                    palette::light::WIDGET,
                    palette::light::WIDGET_HOT,
                    palette::light::FAINT,
                    palette::light::SELECTION,
                ],
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
        for (theme, sem) in [("dark", &DARK), ("light", &LIGHT)] {
            let r = contrast_ratio(sem.engaged_label, sem.engaged_fill);
            assert!(r >= CONTRAST_FLOOR, "{theme} inverted active label is {r}");
        }
    }

    /// Roles that de-emphasise rather than inform. Held to `DIM_CONTRAST_FLOOR`
    /// instead of `CONTRAST_FLOOR`: text pinned at the readable-text floor is
    /// by definition as prominent as body text, which is the opposite of what
    /// this tier is for.
    fn dim_roles(sem: &SemanticColors) -> Vec<(&'static str, Color32)> {
        vec![("text_dim", sem.text_dim)]
    }

    #[test]
    fn every_dim_role_clears_the_dim_floor_on_every_surface() {
        use crate::ui::theme::contrast::DIM_CONTRAST_FLOOR;

        for (theme, sem, surfaces) in [
            (
                "dark",
                &DARK,
                vec![
                    ("surface", palette::dark::SURFACE),
                    ("panel", palette::dark::PANEL),
                    ("card", palette::dark::CARD),
                    ("widget", palette::dark::WIDGET),
                    ("widget_hot", palette::dark::WIDGET_HOT),
                    ("faint", palette::dark::FAINT),
                    ("selection", palette::dark::SELECTION),
                ],
            ),
            (
                "light",
                &LIGHT,
                vec![
                    ("surface", palette::light::SURFACE),
                    ("panel", palette::light::PANEL),
                    ("card", palette::light::CARD),
                    ("widget", palette::light::WIDGET),
                    ("widget_hot", palette::light::WIDGET_HOT),
                    ("faint", palette::light::FAINT),
                    ("selection", palette::light::SELECTION),
                ],
            ),
        ] {
            for (name, color) in dim_roles(sem) {
                for (surface_name, surface) in &surfaces {
                    let r = contrast_ratio(color, *surface);
                    assert!(
                        r >= DIM_CONTRAST_FLOOR,
                        "{theme} {name} on {surface_name} is {r}, needs {DIM_CONTRAST_FLOOR}"
                    );
                }
            }
        }
    }

    /// The regression guard for the bug this tier exists to fix: `text_dim`
    /// once held the same value as body text, so every "de-emphasised" label
    /// in the app rendered at full weight.
    #[test]
    fn text_dim_is_strictly_dimmer_than_body_text() {
        for (theme, sem, body, panel) in [
            ("dark", &DARK, palette::dark::TEXT_DIM, palette::dark::PANEL),
            ("light", &LIGHT, palette::light::TEXT_DIM, palette::light::PANEL),
        ] {
            let dim = contrast_ratio(sem.text_dim, panel);
            let full = contrast_ratio(body, panel);
            assert!(
                dim < full,
                "{theme} text_dim reaches {dim} against the panel and body text reaches {full}; \
                 the de-emphasised tier has collapsed into body text"
            );
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

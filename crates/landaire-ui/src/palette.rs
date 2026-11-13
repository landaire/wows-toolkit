use crate::color::Hsl;
use crate::color::Rgb;
use crate::contrast;
use crate::error::TierOrderError;

/// Whether a palette sets light text on dark surfaces or the other way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Appearance {
    Light,
    Dark,
}

/// One named color of a [`Palette`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Surface,
    Panel,
    Card,
    Widget,
    WidgetHot,
    Faint,
    Border,
    BorderBright,
    Selection,
    Accent,
    Text,
    TextDim,
    TextBright,
}

impl Tier {
    pub const ALL: [Self; 13] = [
        Self::Surface,
        Self::Panel,
        Self::Card,
        Self::Widget,
        Self::WidgetHot,
        Self::Faint,
        Self::Border,
        Self::BorderBright,
        Self::Selection,
        Self::Accent,
        Self::Text,
        Self::TextDim,
        Self::TextBright,
    ];
}

/// A color scheme as distinct surface tiers for window chrome, panels, cards and controls, with
/// one accent for focus and selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    pub appearance: Appearance,

    /// Behind everything: the window itself and its tab strip.
    pub surface: Rgb,
    /// A panel of content.
    pub panel: Rgb,
    /// A card, popover or menu, one step above the panel.
    pub card: Rgb,
    /// A control at rest, and the same control under the pointer.
    pub widget: Rgb,
    pub widget_hot: Rgb,
    /// Every other row of a list or table.
    pub faint: Rgb,

    pub border: Rgb,
    /// A rule that has to be seen, such as one over a tab strip.
    pub border_bright: Rgb,
    /// What a selected row is filled with.
    pub selection: Rgb,
    pub accent: Rgb,

    pub text: Rgb,
    pub text_dim: Rgb,
    pub text_bright: Rgb,
}

impl Palette {
    pub const fn tier(&self, tier: Tier) -> Rgb {
        match tier {
            Tier::Surface => self.surface,
            Tier::Panel => self.panel,
            Tier::Card => self.card,
            Tier::Widget => self.widget,
            Tier::WidgetHot => self.widget_hot,
            Tier::Faint => self.faint,
            Tier::Border => self.border,
            Tier::BorderBright => self.border_bright,
            Tier::Selection => self.selection,
            Tier::Accent => self.accent,
            Tier::Text => self.text,
            Tier::TextDim => self.text_dim,
            Tier::TextBright => self.text_bright,
        }
    }

    /// Checks that the tiers stack as the appearance requires, so a palette whose fields were
    /// filled in from the wrong column is caught.
    ///
    /// Text sits on the far side of the panel. Dark palettes lift each surface step toward the
    /// eye: surface, panel, card, widget. Light palettes keep the card above the panel; their
    /// window surface may sit on either side of the panel. The five surface steps are distinct.
    pub fn check_tier_order(&self) -> Result<(), TierOrderError> {
        let luminance = |tier: Tier| self.tier(tier).relative_luminance();

        let text_against_panel = match self.appearance {
            Appearance::Light => luminance(Tier::Panel) > luminance(Tier::Text),
            Appearance::Dark => luminance(Tier::Panel) < luminance(Tier::Text),
        };
        if !text_against_panel {
            return Err(TierOrderError::TextNotAgainstPanel { appearance: self.appearance });
        }

        let ascending: &[Tier] = match self.appearance {
            Appearance::Light => &[Tier::Panel, Tier::Card],
            Appearance::Dark => &[Tier::Surface, Tier::Panel, Tier::Card, Tier::Widget],
        };
        for pair in ascending.windows(2) {
            if luminance(pair[0]) >= luminance(pair[1]) {
                return Err(TierOrderError::OutOfOrder { appearance: self.appearance, lower: pair[0], upper: pair[1] });
            }
        }

        let surfaces = [Tier::Surface, Tier::Panel, Tier::Card, Tier::Widget, Tier::WidgetHot];
        for (index, first) in surfaces.iter().enumerate() {
            if let Some(second) = surfaces[index + 1..].iter().find(|second| self.tier(**second) == self.tier(*first)) {
                return Err(TierOrderError::Indistinct { first: *first, second: *second });
            }
        }

        Ok(())
    }

    /// `color` lifted to read on both row grounds of this palette, the panel and the stripe. For
    /// colors the application does not choose, such as user-assigned tags.
    pub fn readable_on_rows(&self, color: Rgb) -> Rgb {
        contrast::readable_on(color, &[self.panel, self.faint])
    }

    /// Black or white, for text on a solid accent fill.
    pub fn accent_foreground(&self) -> Rgb {
        contrast::accent_foreground(self.accent)
    }

    /// The fill behind selected text: a blue, as far off the panel as the text on it allows.
    /// Text keeps the readable floor and dim text 3:1, since both are selected. The hue tells a
    /// selection apart from the grey row tones where a palette leaves little room in lightness.
    pub fn text_selection(&self) -> Rgb {
        const HUE: f32 = 210.0 / 360.0;
        const SATURATION: f32 = 0.8;
        const DIM_FLOOR: f32 = 3.0;
        const STEPS: u16 = 200;

        // On the side of the panel the text is on, as a selected row's tone is.
        let toward_text = |candidate: Rgb| {
            let lighter = candidate.relative_luminance() > self.panel.relative_luminance();
            lighter == (self.appearance == Appearance::Dark)
        };
        let text_reads = |candidate: Rgb| {
            contrast::ContrastRatio::between(self.text, candidate) >= contrast::ContrastRatio::READABLE
                && contrast::ContrastRatio::between(self.text_dim, candidate).value() >= DIM_FLOOR
        };
        let off_panel = |candidate: Rgb| contrast::ContrastRatio::between(candidate, self.panel).value();

        (0..=STEPS)
            .map(|step| {
                Rgb::from_hsl(Hsl { hue: HUE, saturation: SATURATION, lightness: f32::from(step) / f32::from(STEPS) })
            })
            .filter(|candidate| toward_text(*candidate) && text_reads(*candidate))
            .max_by(|a, b| off_panel(*a).total_cmp(&off_panel(*b)))
            .expect("every preset has a blue beside its panel that its text reads on; each is tested")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheme::Scheme;

    #[test]
    fn every_preset_stacks_its_tiers_in_order() {
        for scheme in Scheme::ALL {
            assert_eq!(scheme.palette().check_tier_order(), Ok(()), "{scheme:?}");
        }
    }

    #[test]
    fn every_preset_has_a_text_selection_its_text_reads_on() {
        for scheme in Scheme::ALL {
            let palette = scheme.palette();
            let selection = palette.text_selection();
            let text = contrast::ContrastRatio::between(palette.text, selection);
            let dim = contrast::ContrastRatio::between(palette.text_dim, selection).value();

            assert!(text >= contrast::ContrastRatio::READABLE && dim >= 3.0, "{scheme:?}: {selection}");
            assert_ne!(selection, palette.panel, "{scheme:?}");
        }
    }

    #[test]
    fn a_dark_palette_with_its_card_below_its_panel_is_rejected() {
        let dark = Scheme::Dark.palette();
        let swapped = Palette { panel: dark.card, card: dark.panel, ..*dark };

        assert_eq!(
            swapped.check_tier_order(),
            Err(TierOrderError::OutOfOrder { appearance: Appearance::Dark, lower: Tier::Panel, upper: Tier::Card })
        );
    }

    #[test]
    fn a_light_palette_with_text_lighter_than_its_panel_is_rejected() {
        let light = Scheme::Light.palette();
        let inverted = Palette { text: light.panel, panel: light.text, ..*light };

        assert_eq!(
            inverted.check_tier_order(),
            Err(TierOrderError::TextNotAgainstPanel { appearance: Appearance::Light })
        );
    }

    #[test]
    fn two_surface_steps_sharing_a_color_are_rejected() {
        let dark = Scheme::Dark.palette();
        let flat = Palette { widget_hot: dark.widget, ..*dark };

        assert_eq!(
            flat.check_tier_order(),
            Err(TierOrderError::Indistinct { first: Tier::Widget, second: Tier::WidgetHot })
        );
    }
}

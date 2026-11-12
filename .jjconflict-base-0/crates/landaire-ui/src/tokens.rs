//! A [`Palette`] as design tokens: the `--zui-*` set zgui-ui components read, and the `--lui-*`
//! set this crate's own components read.
//!
//! Lengths are written at zoom 1. Zoom is the window's (`WindowHandle::set_zoom`), which scales
//! every CSS pixel, so no token is scaled for it.

use zgui_ui_tokens::ColorScheme;
use zgui_ui_tokens::Declarations;
use zgui_ui_tokens::MotionTokens;
use zgui_ui_tokens::Theme;

use crate::color::Opacity;
use crate::color::Rgb;
use crate::contrast;
use crate::motion::CubicBezier;
use crate::motion::EASE_PROPERTY;
use crate::motion::Movement;
use crate::motion::REDUCED_MOTION_ATTRIBUTE;
use crate::palette::Appearance;
use crate::palette::Palette;
use crate::palette::Tier;
use crate::scheme::ThemeChoice;
use crate::tone::Tones;

/// Body text size.
pub const BASE_FONT_SIZE: &str = "12.5px";

/// The platform's interface face.
pub const SANS_FAMILY: &str = "system-ui, sans-serif";

pub const MONO_FAMILY: &str = "ui-monospace, monospace";

/// Corner radius of controls.
pub const RADIUS: &str = "2px";

/// Corner radius of cards, dialogs, notifications and overlays.
pub const RADIUS_LARGE: &str = "4px";

/// `palette` as the token set zgui-ui components are styled by.
pub fn zui_theme(palette: &Palette) -> Theme {
    let mut theme = match palette.appearance {
        Appearance::Light => Theme::light(),
        Appearance::Dark => Theme::dark(),
    };
    let tones = Tones::for_appearance(palette.appearance);
    let accent_foreground = palette.accent_foreground().to_string();

    let color = &mut theme.color;
    color.background = palette.panel.to_string();
    color.foreground = palette.text.to_string();
    color.card = palette.card.to_string();
    color.card_foreground = palette.text.to_string();
    // Popovers, menus and tooltips sit one tier above the panel.
    color.popover = palette.card.to_string();
    color.popover_foreground = palette.text.to_string();
    color.tooltip = palette.card.to_string();
    color.tooltip_foreground = palette.text.to_string();
    color.tooltip_border = palette.border.to_string();
    color.primary = palette.accent.to_string();
    color.primary_foreground.clone_from(&accent_foreground);
    color.secondary = palette.widget.to_string();
    color.secondary_foreground = palette.text.to_string();
    color.muted = palette.faint.to_string();
    color.muted_foreground = palette.text_dim.to_string();
    // Hover and selection are separate tones so a keyboard cursor stays distinct from the pointer.
    color.accent = palette.widget_hot.to_string();
    color.accent_foreground = palette.text_bright.to_string();
    color.destructive = tones.error.to_string();
    color.destructive_foreground = contrast::text_on(tones.error).to_string();
    color.success = tones.success.to_string();
    color.success_foreground = contrast::text_on(tones.success).to_string();
    color.warning = tones.warning.to_string();
    color.warning_foreground = contrast::text_on(tones.warning).to_string();
    color.info = tones.info.to_string();
    color.info_foreground = contrast::text_on(tones.info).to_string();
    color.border = palette.border.to_string();
    color.input = palette.border.to_string();
    color.ring = palette.accent.to_string();
    // The chrome behind the panels.
    color.sidebar = palette.surface.to_string();
    color.sidebar_foreground = palette.text.to_string();
    color.sidebar_primary = palette.accent.to_string();
    color.sidebar_primary_foreground = accent_foreground;
    color.sidebar_accent = palette.selection.to_string();
    color.sidebar_accent_foreground = palette.text_bright.to_string();
    color.sidebar_border = palette.border.to_string();
    color.sidebar_ring = palette.accent.to_string();

    let control = &mut theme.control;
    control.ghost_hover = "var(--zui-color-accent)".to_owned();
    control.outline_border = "var(--zui-color-border)".to_owned();
    control.outline_hover = "var(--zui-color-accent)".to_owned();
    // One thumb color serves both switch states, so it is the panel color: it reads against the
    // accent track when on and the bright border track when off, in light and dark alike.
    control.switch_off = palette.border_bright.to_string();
    control.switch_thumb = palette.panel.to_string();
    control.switch_thumb_on = palette.panel.to_string();
    control.tab_ink = palette.text_dim.to_string();
    control.tab_fill = palette.panel.to_string();
    control.tab_border = palette.border.to_string();

    let radius = &mut theme.radius;
    radius.base = RADIUS_LARGE.to_owned();
    radius.sm = RADIUS.to_owned();
    radius.md = RADIUS.to_owned();
    radius.lg = RADIUS_LARGE.to_owned();
    radius.xl = RADIUS_LARGE.to_owned();

    let typography = &mut theme.typography;
    typography.family_sans = SANS_FAMILY.to_owned();
    typography.family_mono = MONO_FAMILY.to_owned();
    for (size, leading, value, line) in [
        (&mut typography.size_xs, &mut typography.leading_xs, "12px", "16px"),
        (&mut typography.size_sm, &mut typography.leading_sm, BASE_FONT_SIZE, "18px"),
        (&mut typography.size_md, &mut typography.leading_md, "14px", "20px"),
        (&mut typography.size_lg, &mut typography.leading_lg, "16px", "24px"),
        (&mut typography.size_xl, &mut typography.leading_xl, "18px", "26px"),
        (&mut typography.size_x2l, &mut typography.leading_x2l, "20px", "28px"),
        (&mut typography.size_x3l, &mut typography.leading_x3l, "24px", "32px"),
    ] {
        *size = value.to_owned();
        *leading = line.to_owned();
    }

    // Controls are flat; a card carries only its lit edge; everything that floats carries the
    // overlay shadow.
    let shadow = &mut theme.shadow;
    shadow.x2s = "none".to_owned();
    shadow.xs = "none".to_owned();
    shadow.sm = card_shadow(palette.appearance);
    for floating in [&mut shadow.md, &mut shadow.lg, &mut shadow.xl, &mut shadow.x2l] {
        *floating = overlay_shadow(palette.appearance);
    }

    theme.motion.ease_standard = CubicBezier::STANDARD.css();

    theme
}

/// `palette` as the `--lui-*` custom properties this crate's components read.
pub fn lui_declarations(palette: &Palette) -> Declarations {
    let tones = Tones::for_appearance(palette.appearance);
    let mut declarations = Declarations::new();

    for tier in Tier::ALL {
        declarations.push(tier_property(tier), &palette.tier(tier).to_string());
    }
    declarations.push("--lui-color-accent-foreground", &palette.accent_foreground().to_string());
    declarations.push("--lui-color-text-faint", &tones.text_faint.to_string());
    // The dimming behind a drawer, the same black on either appearance.
    declarations.push("--lui-color-scrim", &Rgb::BLACK.with_opacity(Opacity(0.25)).to_string());

    declarations.push("--lui-radius", RADIUS);
    declarations.push("--lui-radius-lg", RADIUS_LARGE);

    declarations.push("--lui-shadow-glow", &glow_shadow(palette));
    declarations.push("--lui-shadow-overlay", &overlay_shadow(palette.appearance));
    declarations.push("--lui-shadow-card", &card_shadow(palette.appearance));

    declarations.push("--lui-font-size", BASE_FONT_SIZE);
    declarations.push("--lui-font-family", SANS_FAMILY);
    declarations.push("--lui-font-family-mono", MONO_FAMILY);

    declarations.push(EASE_PROPERTY, &CubicBezier::STANDARD.css());
    for movement in Movement::ALL {
        declarations.push(movement.property(), &format!("{}ms", movement.full_duration().as_millis()));
    }

    declarations
}

/// The `--lui-*` properties for `choice` as a style sheet on `:root`, the window ground and body
/// text drawn from them, and the rule that zeroes every duration under
/// [`REDUCED_MOTION_ATTRIBUTE`].
pub fn lui_sheet(choice: ThemeChoice) -> String {
    let slots = choice.slots();
    let mut css = String::new();

    if slots.scheme.wants_light() {
        css.push_str(&lui_declarations(slots.light).into_rule(":root"));
        css.push('\n');
    }

    if slots.scheme.wants_dark() {
        let rule = lui_declarations(slots.dark).into_rule(":root");
        if slots.scheme == ColorScheme::System {
            css.push_str(&format!("@media (prefers-color-scheme: dark) {{ {rule} }}"));
        } else {
            css.push_str(&rule);
        }
        css.push('\n');
    }

    css.push_str(
        ":root { background-color: var(--lui-color-surface); color: var(--lui-color-text); \
         font-family: var(--lui-font-family); font-size: var(--lui-font-size); }\n",
    );

    let mut stilled = Declarations::new();
    for movement in Movement::ALL {
        stilled.push(movement.property(), "0ms");
    }
    for property in MotionTokens::PROPERTIES.iter().filter(|property| property.contains("-duration-")) {
        stilled.push(property, "0ms");
    }
    // Every descendant too: a nested theme provider declares its own durations on its subtree.
    let reduced = format!(":root[{REDUCED_MOTION_ATTRIBUTE}], :root[{REDUCED_MOTION_ATTRIBUTE}] *");
    css.push_str(&stilled.into_rule(&reduced));

    css
}

/// The custom property a tier is written as.
pub const fn tier_property(tier: Tier) -> &'static str {
    match tier {
        Tier::Surface => "--lui-color-surface",
        Tier::Panel => "--lui-color-panel",
        Tier::Card => "--lui-color-card",
        Tier::Widget => "--lui-color-widget",
        Tier::WidgetHot => "--lui-color-widget-hot",
        Tier::Faint => "--lui-color-faint",
        Tier::Border => "--lui-color-border",
        Tier::BorderBright => "--lui-color-border-bright",
        Tier::Selection => "--lui-color-selection",
        Tier::Accent => "--lui-color-accent",
        Tier::Text => "--lui-color-text",
        Tier::TextDim => "--lui-color-text-dim",
        Tier::TextBright => "--lui-color-text-bright",
    }
}

/// A stationary halo for active controls, on dark palettes only. A CSS blur radius is twice the
/// gaussian sigma, so this is a sigma of 8px.
fn glow_shadow(palette: &Palette) -> String {
    match palette.appearance {
        Appearance::Light => "none".to_owned(),
        Appearance::Dark => format!("0 0 16px 0 {}", palette.accent.with_opacity(Opacity(0.18))),
    }
}

/// The shadow under a popover, tooltip or drawer: small and soft, darker on a dark palette where a
/// light one would not register. A gaussian sigma of 2px.
fn overlay_shadow(appearance: Appearance) -> String {
    let ink = match appearance {
        Appearance::Light => Opacity(0.12),
        Appearance::Dark => Opacity(0.32),
    };

    format!("0 2px 4px 0 {}", Rgb::BLACK.with_opacity(ink))
}

/// A 1px lit edge along a card's top, on dark palettes only: on a light panel the card is already
/// the lightest surface.
fn card_shadow(appearance: Appearance) -> String {
    match appearance {
        Appearance::Light => "none".to_owned(),
        Appearance::Dark => format!("inset 0 1px 0 0 {}", Rgb::WHITE.with_opacity(Opacity(0.05))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheme::Scheme;

    /// The value `declarations` gives `property`.
    fn value_of<'a>(declarations: &'a Declarations, property: &str) -> Option<&'a str> {
        declarations
            .as_str()
            .split(';')
            .filter_map(|declaration| declaration.trim().split_once(": "))
            .find_map(|(name, value)| (name == property).then_some(value))
    }

    fn zui(palette: &Palette) -> Declarations {
        let mut declarations = Declarations::new();
        zui_theme(palette).declare(&mut declarations);

        declarations
    }

    #[test]
    fn zgui_components_take_their_colors_from_the_palette_tiers() {
        for scheme in Scheme::ALL {
            let palette = scheme.palette();
            let declarations = zui(palette);
            let expected = [
                ("--zui-color-background", palette.panel),
                ("--zui-color-foreground", palette.text),
                ("--zui-color-card", palette.card),
                ("--zui-color-popover", palette.card),
                ("--zui-color-primary", palette.accent),
                ("--zui-color-primary-foreground", palette.accent_foreground()),
                ("--zui-color-secondary", palette.widget),
                ("--zui-color-muted", palette.faint),
                ("--zui-color-muted-foreground", palette.text_dim),
                ("--zui-color-accent", palette.widget_hot),
                ("--zui-color-accent-foreground", palette.text_bright),
                ("--zui-color-border", palette.border),
                ("--zui-color-ring", palette.accent),
                ("--zui-color-sidebar", palette.surface),
                ("--zui-color-sidebar-accent", palette.selection),
                ("--zui-color-control-switch-off", palette.border_bright),
            ];
            for (property, color) in expected {
                assert_eq!(
                    value_of(&declarations, property),
                    Some(color.to_string().as_str()),
                    "{scheme:?} {property}"
                );
            }
        }
    }

    #[test]
    fn zgui_components_take_the_compact_metrics() {
        let declarations = zui(Scheme::Dark.palette());

        for (property, value) in [
            ("--zui-radius-md", RADIUS),
            ("--zui-radius-lg", RADIUS_LARGE),
            ("--zui-radius-xl", RADIUS_LARGE),
            ("--zui-type-size-sm", BASE_FONT_SIZE),
            ("--zui-type-family-sans", SANS_FAMILY),
            ("--zui-type-family-mono", MONO_FAMILY),
            ("--zui-shadow-xs", "none"),
            ("--zui-motion-ease-standard", "cubic-bezier(0.4, 0, 0.2, 1)"),
        ] {
            assert_eq!(value_of(&declarations, property), Some(value), "{property}");
        }
    }

    #[test]
    fn every_tier_is_a_custom_property() {
        let palette = Scheme::Nord.palette();
        let declarations = lui_declarations(palette);

        for tier in Tier::ALL {
            assert_eq!(
                value_of(&declarations, tier_property(tier)),
                Some(palette.tier(tier).to_string().as_str()),
                "{tier:?}"
            );
        }
    }

    #[test]
    fn the_glow_and_the_lit_card_edge_are_dark_only_and_overlays_darken_in_dark() {
        let dark = lui_declarations(Scheme::Dark.palette());
        let light = lui_declarations(Scheme::Light.palette());

        assert_eq!(value_of(&dark, "--lui-shadow-glow"), Some("0 0 16px 0 rgb(199 195 184 / 0.18)"));
        assert_eq!(value_of(&light, "--lui-shadow-glow"), Some("none"));
        assert_eq!(value_of(&dark, "--lui-shadow-card"), Some("inset 0 1px 0 0 rgb(255 255 255 / 0.05)"));
        assert_eq!(value_of(&light, "--lui-shadow-card"), Some("none"));
        assert_eq!(value_of(&dark, "--lui-shadow-overlay"), Some("0 2px 4px 0 rgb(0 0 0 / 0.32)"));
        assert_eq!(value_of(&light, "--lui-shadow-overlay"), Some("0 2px 4px 0 rgb(0 0 0 / 0.12)"));
    }

    #[test]
    fn following_the_desktop_puts_the_dark_tokens_behind_the_media_query() {
        let css = lui_sheet(ThemeChoice::System);

        let query = css.find("@media (prefers-color-scheme: dark)").expect("the query is written");
        let light_panel = css.find(&Scheme::Light.palette().panel.to_string()).expect("the light panel is written");
        let dark_panel = css.find(&Scheme::Dark.palette().panel.to_string()).expect("the dark panel is written");
        assert!(light_panel < query && query < dark_panel);
    }

    #[test]
    fn a_fixed_scheme_writes_one_set_and_no_media_query() {
        let css = lui_sheet(ThemeChoice::Fixed(Scheme::GruvboxLight));

        assert!(!css.contains("@media"));
        assert_eq!(css.matches(&Scheme::GruvboxLight.palette().panel.to_string()).count(), 1);
    }
}

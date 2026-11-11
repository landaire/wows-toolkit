use gpui_kit::App;
use gpui_kit::BoxShadow;
use gpui_kit::Hsla;
use gpui_kit::Window;
use gpui_kit::WindowAppearance;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::theme::ThemeTokens;
use gpui_kit::hsla;
use gpui_kit::px;
use gpui_kit::rgb;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use wows_toolkit_viewmodel::settings::ThemeChoice;

use crate::zoom::ZoomFactor;

/// Named color schemes applied to the desktop's shared surface tokens.
///
/// Each scheme keeps distinct surface tiers for window chrome, panels, cards,
/// and controls while using one accent for focus and selection.
struct Palette {
    mode: ThemeMode,
    /// Behind everything: the window itself and the tab strip.
    surface: u32,
    /// A panel of content.
    panel: u32,
    /// A card, popover or menu, one step above the panel.
    card: u32,
    /// A control at rest, and the same control under the pointer.
    widget: u32,
    widget_hot: u32,
    /// Every other row of a list or table.
    faint: u32,
    border: u32,
    /// A rule that has to be seen, such as one over the tab strip.
    border_bright: u32,
    /// What a selected row is filled with.
    selection: u32,
    accent: u32,
    text: u32,
    text_dim: u32,
    text_bright: u32,
}

const DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x101010,
    panel: 0x181816,
    card: 0x1f1f1c,
    widget: 0x252522,
    widget_hot: 0x2f2f29,
    faint: 0x1c1c1a,
    border: 0x383733,
    border_bright: 0x524f4a,
    selection: 0x282820,
    accent: 0xc7c3b8,
    text: 0xc9c6be,
    text_dim: 0x979789,
    text_bright: 0xe8e5dc,
};
const LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    surface: 0xe6e5e0,
    panel: 0xf4f3ef,
    card: 0xffffff,
    widget: 0xeeede8,
    widget_hot: 0xdedcd3,
    faint: 0xf0efeb,
    border: 0xcbc9c1,
    border_bright: 0x9a978d,
    selection: 0xdad8cc,
    accent: 0x26251f,
    text: 0x1a1a17,
    text_dim: 0x5c5a53,
    text_bright: 0x0a0a08,
};
const DRACULA: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x21222c,
    panel: 0x282a36,
    card: 0x343746,
    widget: 0x3b3e50,
    widget_hot: 0x44475a,
    faint: 0x2c2e3b,
    border: 0x44475a,
    border_bright: 0x6272a4,
    selection: 0x343746,
    accent: 0xbd93f9,
    text: 0xf0eef8,
    text_dim: 0xb9b6ca,
    text_bright: 0xf8f8f2,
};
const NORD: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x242933,
    panel: 0x2e3440,
    card: 0x353d4b,
    widget: 0x3b4252,
    widget_hot: 0x434c5e,
    faint: 0x303744,
    border: 0x434c5e,
    border_bright: 0x5e81ac,
    selection: 0x3b4252,
    accent: 0x88c0d0,
    text: 0xd8dee9,
    text_dim: 0xaeb8c7,
    text_bright: 0xeceff4,
};
const MONOKAI: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x1b1d1e,
    panel: 0x272822,
    card: 0x30312a,
    widget: 0x393a32,
    widget_hot: 0x494a40,
    faint: 0x2b2c26,
    border: 0x494a40,
    border_bright: 0x75715e,
    selection: 0x393a32,
    accent: 0xa6e22e,
    text: 0xe4e4dc,
    text_dim: 0xa5a59a,
    text_bright: 0xf8f8f2,
};
const SOLARIZED_DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x002b36,
    panel: 0x073642,
    card: 0x0b3d49,
    widget: 0x104450,
    widget_hot: 0x17505b,
    faint: 0x093944,
    border: 0x24545e,
    border_bright: 0x586e75,
    selection: 0x104450,
    accent: 0x268bd2,
    text: 0xb6c2bf,
    text_dim: 0x93a1a1,
    text_bright: 0xeee8d5,
};
const SOLARIZED_LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    surface: 0xfdf6e3,
    panel: 0xeee8d5,
    card: 0xfff9e8,
    widget: 0xe6dfca,
    widget_hot: 0xded6c0,
    faint: 0xf5eedb,
    border: 0xd4ccb8,
    border_bright: 0x93a1a1,
    selection: 0xe6dfca,
    accent: 0x268bd2,
    text: 0x40545c,
    text_dim: 0x50666d,
    text_bright: 0x586e75,
};
const GRUVBOX_DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x1d2021,
    panel: 0x282828,
    card: 0x32302f,
    widget: 0x3c3836,
    widget_hot: 0x504945,
    faint: 0x2e2b29,
    border: 0x504945,
    border_bright: 0x665c54,
    selection: 0x3c3836,
    accent: 0xd79921,
    text: 0xd5c4a1,
    text_dim: 0xa89984,
    text_bright: 0xebdbb2,
};
const GRUVBOX_LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    surface: 0xfbf1c7,
    panel: 0xf2e5bc,
    card: 0xfff5d6,
    widget: 0xebddb2,
    widget_hot: 0xe2d2a3,
    faint: 0xf7ecc2,
    border: 0xd5c4a1,
    border_bright: 0xa89984,
    selection: 0xebddb2,
    accent: 0xaf3a03,
    text: 0x504945,
    text_dim: 0x665c54,
    text_bright: 0x3c3836,
};
const CATPPUCCIN: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x181825,
    panel: 0x1e1e2e,
    card: 0x252538,
    widget: 0x303047,
    widget_hot: 0x3b3b55,
    faint: 0x222235,
    border: 0x45455f,
    border_bright: 0x6c7086,
    selection: 0x303047,
    accent: 0xcba6f7,
    text: 0xbac2de,
    text_dim: 0x9399b2,
    text_bright: 0xcdd6f4,
};
const TOKYO_NIGHT: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x16161e,
    panel: 0x1a1b26,
    card: 0x222433,
    widget: 0x292e42,
    widget_hot: 0x343b58,
    faint: 0x1e2030,
    border: 0x3b4261,
    border_bright: 0x565f89,
    selection: 0x292e42,
    accent: 0x7aa2f7,
    text: 0xa9b1d6,
    text_dim: 0x7982a9,
    text_bright: 0xc0caf5,
};

static CURRENT_ACCENT: AtomicU32 = AtomicU32::new(DARK.accent);
static CURRENT_SURFACE: AtomicU32 = AtomicU32::new(DARK.surface);
static CURRENT_BORDER_BRIGHT: AtomicU32 = AtomicU32::new(DARK.border_bright);
static CURRENT_TEXT_DIM: AtomicU32 = AtomicU32::new(DARK.text_dim);
static CURRENT_PANEL: AtomicU32 = AtomicU32::new(DARK.panel);
static CURRENT_FAINT: AtomicU32 = AtomicU32::new(DARK.faint);
static CURRENT_CARD: AtomicU32 = AtomicU32::new(DARK.card);

/// The WCAG contrast a colour the app does not choose is lifted to.
const CONTRAST_FLOOR: f32 = 4.5;

/// A light panel must be lighter than the text on it, and the other way round
/// in dark. Checked at compile time, so a palette whose fields were filled in
/// from the wrong column does not build.
const _: () = {
    assert!(LIGHT.panel > LIGHT.text);
    assert!(DARK.panel < DARK.text);
    assert!(DARK.surface < DARK.panel && DARK.panel < DARK.card && DARK.card < DARK.widget);
    assert!(LIGHT.surface < LIGHT.panel && LIGHT.panel < LIGHT.card);
};

/// What a colour means, rather than what it looks like
/// (`crates/wows-toolkit/src/ui/theme/semantic.rs`).
///
/// Each tone is held against the surfaces it is drawn on, and the two sets
/// are not each other's inverse: a light panel caps how light a tone may be,
/// so the light set is tuned apart along lightness instead.
pub struct Semantic {
    pub win: u32,
    pub loss: u32,
    pub draw: u32,
    pub warn: u32,
    pub error: u32,
    pub ok: u32,
    pub text_strong: u32,
    pub text_dim: u32,
    /// Below dim: grammar punctuation and hints that are there to be found,
    /// not read.
    pub text_faint: u32,
    /// Division mates.
    pub division: u32,
    /// Players flagged by the abuse list.
    pub abuser: u32,
    /// A value worth noticing that is not a warning.
    pub notice: u32,
    pub chat_division: u32,
    pub chat_team: u32,
    pub chat_other: u32,
    /// What a shell did to the armor it met. The same four the egui viewer
    /// paints its verdicts in (`ui/theme/semantic.rs`), so the two apps read a
    /// cast the same way.
    pub armor_pen: u32,
    pub armor_overpen: u32,
    pub armor_ricochet: u32,
    pub armor_shatter: u32,
    /// How square a strike was: head-on, angled, glancing.
    pub armor_angle_good: u32,
    pub armor_angle_mid: u32,
    pub armor_angle_bad: u32,
}

pub const DARK_SEMANTIC: Semantic = Semantic {
    win: 0x6fd98a,
    loss: 0xf28389,
    draw: 0xe9e5dd,
    warn: 0xe8a54a,
    error: 0xe8737b,
    ok: 0x6fd98a,
    text_strong: 0xe8e5dc,
    text_dim: 0x7c7c6e,
    text_faint: 0x5c5c52,
    division: 0xe5c158,
    abuser: 0xf09bc0,
    notice: 0xe5c158,
    chat_division: 0xe5c158,
    chat_team: 0x6fd98a,
    chat_other: 0xe8a54a,
    armor_pen: 0x6fd98a,
    armor_overpen: 0xe0be64,
    armor_ricochet: 0x7fb4e8,
    armor_shatter: 0xa9a49a,
    armor_angle_good: 0x64d98a,
    armor_angle_mid: 0xe0be64,
    armor_angle_bad: 0xe8737b,
};

pub const LIGHT_SEMANTIC: Semantic = Semantic {
    // Mid tones rather than near-black ones: at these the two read as green and
    // red at a glance instead of as body text, and both still clear 4.5:1 on
    // the panel and the striped rows.
    win: 0x16703a,
    loss: 0xb42318,
    draw: 0x5c5950,
    warn: 0x8a4b00,
    error: 0xae2230,
    ok: 0x106c34,
    text_strong: 0x0a0a08,
    text_dim: 0x78766f,
    text_faint: 0x9b9890,
    division: 0x775800,
    abuser: 0xa33270,
    notice: 0x775800,
    chat_division: 0x775800,
    chat_team: 0x106c34,
    chat_other: 0x8a4b00,
    armor_pen: 0x106c34,
    armor_overpen: 0x785808,
    armor_ricochet: 0x1b5fa8,
    armor_shatter: 0x5f5c52,
    armor_angle_good: 0x116b34,
    armor_angle_mid: 0x785808,
    armor_angle_bad: 0xae2230,
};

/// The semantic set for whatever is on screen now.
pub fn semantic() -> &'static Semantic {
    if is_dark_mode() { &DARK_SEMANTIC } else { &LIGHT_SEMANTIC }
}

/// Whether the desktop is currently set to a dark appearance.
///
/// GPUI reports vibrant variants separately; both are the same choice as far
/// as a palette is concerned.
pub fn system_is_dark(cx: &App) -> bool {
    matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

/// Pin gpui-component's Theme to the palette `choice` selects, scaled by
/// `zoom`.
///
/// Every surface token is mapped by hand. `ThemeTokens::from` fills the rest
/// in from the colours set here, and anything left at a shadcn default would
/// read as a different app in the middle of this one.
pub fn apply_egui_theme(choice: ThemeChoice, zoom: ZoomFactor, window: &mut Window, cx: &mut App) {
    let system_dark = system_is_dark(cx);
    let dark = choice.is_dark(system_dark);
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
    let palette = palette_for(choice, system_dark);
    CURRENT_ACCENT.store(palette.accent, Ordering::Relaxed);
    CURRENT_SURFACE.store(palette.surface, Ordering::Relaxed);
    CURRENT_BORDER_BRIGHT.store(palette.border_bright, Ordering::Relaxed);
    CURRENT_TEXT_DIM.store(palette.text_dim, Ordering::Relaxed);
    CURRENT_PANEL.store(palette.panel, Ordering::Relaxed);
    CURRENT_FAINT.store(palette.faint, Ordering::Relaxed);
    CURRENT_CARD.store(palette.card, Ordering::Relaxed);
    let semantic = semantic_for(dark);
    Theme::change(palette.mode, Some(window), cx);
    let ui_family = crate::fonts::ui_family(cx);

    let theme = Theme::global_mut(cx);
    if let Some(family) = ui_family {
        theme.font_family = family;
    }
    theme.background = rgb(palette.panel).into();
    theme.foreground = rgb(palette.text).into();
    theme.border = rgb(palette.border).into();
    theme.secondary = rgb(palette.widget).into();
    theme.secondary_hover = rgb(palette.widget_hot).into();
    theme.secondary_active = pressed(rgb(palette.widget_hot).into());
    theme.secondary_foreground = rgb(palette.text).into();
    theme.button = rgb(palette.widget).into();
    theme.button_hover = rgb(palette.widget_hot).into();
    theme.button_active = pressed(rgb(palette.widget_hot).into());
    theme.button_foreground = rgb(palette.text).into();
    theme.button_secondary = theme.secondary;
    theme.button_secondary_hover = theme.secondary_hover;
    theme.button_secondary_active = theme.secondary_active;
    theme.button_secondary_foreground = theme.secondary_foreground;
    theme.input = rgb(palette.border).into();
    theme.ring = rgb(palette.accent).into();
    theme.caret = rgb(palette.accent).into();
    theme.muted = rgb(palette.faint).into();
    theme.muted_foreground = rgb(palette.text_dim).into();

    // A popover and a menu sit one tier above the panel.
    theme.popover = rgb(palette.card).into();
    theme.popover_foreground = rgb(palette.text).into();

    // The chrome behind the panels: window, tab strip and title bar.
    theme.sidebar = rgb(palette.surface).into();
    theme.sidebar_border = rgb(palette.border).into();
    theme.sidebar_foreground = rgb(palette.text).into();
    theme.sidebar_accent = rgb(palette.selection).into();
    theme.sidebar_accent_foreground = rgb(palette.text_bright).into();
    theme.sidebar_primary = rgb(palette.accent).into();
    theme.sidebar_primary_foreground = accent_foreground();
    theme.tab_bar = rgb(palette.surface).into();
    theme.tab = rgb(palette.surface).into();
    theme.tab_active = rgb(palette.panel).into();
    theme.tab_foreground = rgb(palette.text_dim).into();
    theme.tab_active_foreground = rgb(palette.text_bright).into();
    theme.tab_bar_segmented = rgb(palette.surface).into();
    theme.title_bar = rgb(palette.surface).into();
    theme.title_bar_border = rgb(palette.border).into();
    theme.status_bar = rgb(palette.surface).into();
    theme.status_bar_border = rgb(palette.border).into();
    theme.group_box = rgb(palette.card).into();
    theme.group_box_foreground = rgb(palette.text).into();

    // Selection and hover use separate tones so a keyboard cursor remains
    // distinct from the pointer.
    theme.selection = rgb(palette.selection).into();
    theme.accent = rgb(palette.widget_hot).into();
    theme.accent_foreground = rgb(palette.text_bright).into();
    theme.colors.list = rgb(palette.panel).into();
    theme.list_even = rgb(palette.faint).into();
    theme.list_head = rgb(palette.surface).into();
    theme.list_hover = rgb(palette.widget_hot).into();
    theme.list_active = rgb(palette.selection).into();
    theme.list_active_border = rgb(palette.accent).into();
    theme.table = rgb(palette.panel).into();
    theme.table_even = rgb(palette.faint).into();
    theme.table_head = rgb(palette.surface).into();
    theme.table_head_foreground = rgb(palette.text_dim).into();
    theme.table_foot = rgb(palette.surface).into();
    theme.table_foot_foreground = rgb(palette.text_dim).into();
    theme.table_hover = rgb(palette.widget_hot).into();
    theme.table_active = rgb(palette.selection).into();
    theme.table_active_border = rgb(palette.accent).into();
    theme.table_row_border = rgb(palette.border).into();

    theme.primary = rgb(palette.accent).into();
    theme.primary_foreground = accent_foreground();
    theme.primary_hover = accent().opacity(0.9);
    theme.primary_active = accent().opacity(0.8);
    theme.button_primary = theme.primary;
    theme.button_primary_hover = theme.primary_hover;
    theme.button_primary_active = theme.primary_active;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.slider_bar = theme.primary;
    theme.slider_thumb = rgb(palette.text_bright).into();
    theme.progress_bar = theme.primary;
    // One thumb colour serves both states, so it is the page colour: it reads
    // against the accent track when on and the grey track when off, in light
    // and dark palettes alike.
    theme.switch = rgb(palette.border_bright).into();
    theme.switch_thumb = rgb(palette.panel).into();
    theme.scrollbar = rgb(palette.surface).into();
    theme.scrollbar_thumb = rgb(palette.border_bright).into();
    theme.scrollbar_thumb_hover = rgb(palette.text_dim).into();
    theme.drag_border = theme.primary;
    theme.drop_target = rgb(palette.selection).into();

    theme.danger = rgb(semantic.error).into();
    theme.warning = rgb(semantic.warn).into();
    theme.success = rgb(semantic.ok).into();
    theme.info = rgb(semantic.notice).into();

    theme.tokens = ThemeTokens::from(&theme.colors);

    // egui body text 12.5px, small rounding, scaled by zoom.
    let zoom = zoom.get();
    theme.font_size = px(12.5 * zoom);
    theme.radius = px(2.0 * zoom);
    // Dialogs, notifications and the app's cards and overlays.
    theme.radius_lg = px(4.0 * zoom);

    // rem-based helpers scale via rem size; px-valued fields were scaled above.
    window.set_rem_size(px(16.0 * zoom));
    cx.refresh_windows();
}

fn semantic_for(dark: bool) -> &'static Semantic {
    if dark { &DARK_SEMANTIC } else { &LIGHT_SEMANTIC }
}

/// A rule that has to be seen: over the tab strip, or between two controls
/// that are otherwise flush.
pub fn border_bright() -> Hsla {
    rgb(CURRENT_BORDER_BRIGHT.load(Ordering::Relaxed)).into()
}

/// The surface behind the panels, for chrome that is not a panel itself.
pub fn surface() -> Hsla {
    rgb(CURRENT_SURFACE.load(Ordering::Relaxed)).into()
}

/// The tint for an affordance icon that is not status, such as a folder.
pub fn icon_accent() -> Hsla {
    rgb(CURRENT_ACCENT.load(Ordering::Relaxed)).into()
}

/// The active scheme's accent, used for selected modes and focus indicators.
pub fn accent() -> Hsla {
    icon_accent()
}

/// Selects the text tone with the greater contrast against the accent.
pub fn accent_foreground() -> Hsla {
    let color = CURRENT_ACCENT.load(Ordering::Relaxed);
    let linear = |shift: u32| {
        let channel = ((color >> shift) & 0xff_u32) as f32 / 255.;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    let luminance = 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0);
    rgb(if luminance > 0.179 { 0x000000 } else { 0xffffff }).into()
}

/// A stationary halo for active controls on dark surfaces.
pub fn phosphor_glow() -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), accent().opacity(if is_dark_mode() { 0.18 } else { 0. })).blur_radius(px(8.))
}

/// A card: one surface step above the panel it sits on.
pub fn card() -> Hsla {
    rgb(CURRENT_CARD.load(Ordering::Relaxed)).into()
}

/// A 1px lit edge along a card's top, in dark palettes only: on a light
/// panel the card is already the lightest surface. Offset 2px because the
/// card's 1px border paints over the first.
pub fn card_highlight() -> Option<BoxShadow> {
    is_dark_mode().then(|| BoxShadow::new(px(0.), px(2.), hsla(0., 0., 1., 0.05)).inset())
}

/// The shadow under a popover, tooltip or drawer: small and soft, darker on a
/// dark palette where a light one would not register. GPUI takes the blur as
/// a gaussian sigma, so 2px reaches about 6px.
pub fn overlay_shadow() -> Vec<BoxShadow> {
    let ink = if is_dark_mode() { 0.32 } else { 0.12 };

    vec![BoxShadow::new(px(0.), px(2.), hsla(0., 0., 0., ink)).blur_radius(px(2.))]
}

/// A control held down: a step darker than under the pointer, so a press
/// reads as the control going in. A fixed step reads alike on light and
/// dark palettes.
fn pressed(hot: Hsla) -> Hsla {
    const STEP: f32 = 0.035;

    Hsla { l: (hot.l - STEP).max(0.), ..hot }
}

fn palette_for(choice: ThemeChoice, system_dark: bool) -> &'static Palette {
    match choice {
        ThemeChoice::System => {
            if system_dark {
                &DARK
            } else {
                &LIGHT
            }
        }
        ThemeChoice::Dark => &DARK,
        ThemeChoice::Light => &LIGHT,
        ThemeChoice::Dracula => &DRACULA,
        ThemeChoice::Nord => &NORD,
        ThemeChoice::Monokai => &MONOKAI,
        ThemeChoice::SolarizedDark => &SOLARIZED_DARK,
        ThemeChoice::SolarizedLight => &SOLARIZED_LIGHT,
        ThemeChoice::GruvboxDark => &GRUVBOX_DARK,
        ThemeChoice::GruvboxLight => &GRUVBOX_LIGHT,
        ThemeChoice::CatppuccinMocha => &CATPPUCCIN,
        ThemeChoice::TokyoNight => &TOKYO_NIGHT,
    }
}

/// Window, panel, text, and accent colors for a theme preview.
pub fn swatches(choice: ThemeChoice, cx: &App) -> [u32; 4] {
    let palette = palette_for(choice, system_is_dark(cx));
    [palette.surface, palette.panel, palette.text, palette.accent]
}

/// De-emphasised detail text, held against the panel rather than faded out of
/// the body tone with opacity.
pub fn text_dim() -> Hsla {
    rgb(CURRENT_TEXT_DIM.load(Ordering::Relaxed)).into()
}

/// Below [`text_dim`]: grammar punctuation and hints.
pub fn text_faint() -> Hsla {
    rgb(semantic().text_faint).into()
}

/// `color` (`0xRRGGBB`), moved in lightness until it reads on both row grounds
/// of the current palette. For colours the app does not choose, such as the
/// game's clan colours, which include pale tones made for a dark client.
///
/// Hue and saturation are kept, and the smaller of the two lightness moves
/// that clears the floor wins, so the colour changes as little as it must.
/// Memoised per colour and pair of grounds, since every clan row asks on
/// every frame.
pub fn readable_on_rows(color: u32) -> u32 {
    thread_local! {
        static READABLE: RefCell<HashMap<(u32, [u32; 2]), u32>> = RefCell::new(HashMap::new());
    }

    let grounds = [CURRENT_PANEL.load(Ordering::Relaxed), CURRENT_FAINT.load(Ordering::Relaxed)];

    READABLE.with(|cache| *cache.borrow_mut().entry((color, grounds)).or_insert_with(|| readable_on(color, &grounds)))
}

/// `color` moved in lightness until it clears the floor on every one of
/// `grounds`, when any lightness can. Two grounds on either side of a
/// relative luminance of about 0.18 defeat both black and white; then the
/// result is whichever of the two reads better on its worse ground.
fn readable_on(color: u32, grounds: &[u32]) -> u32 {
    let worst =
        |candidate: u32| grounds.iter().map(|ground| contrast_ratio(candidate, *ground)).fold(f32::MAX, f32::min);
    if worst(color) >= CONTRAST_FLOOR {
        return color;
    }

    const STEPS: u16 = 128;
    let hsla: Hsla = rgb(color).into();
    let mut winner: Option<(f32, u32)> = None;
    for lighten in [true, false] {
        for step in 1..=STEPS {
            let t = f32::from(step) / f32::from(STEPS);
            let l = if lighten { hsla.l + (1. - hsla.l) * t } else { hsla.l * (1. - t) };
            let candidate = pack(Hsla { l, ..hsla });
            if worst(candidate) >= CONTRAST_FLOOR {
                let moved = (l - hsla.l).abs();
                if winner.is_none_or(|(least, _)| moved < least) {
                    winner = Some((moved, candidate));
                }
                break;
            }
        }
    }

    match winner {
        Some((_, candidate)) => candidate,
        None if worst(0xffffff) >= worst(0x000000) => 0xffffff,
        None => 0x000000,
    }
}

fn pack(color: Hsla) -> u32 {
    let rgba = gpui_kit::Rgba::from(color);
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
    (channel(rgba.r) << 16) | (channel(rgba.g) << 8) | channel(rgba.b)
}

/// WCAG 2.1 contrast ratio between two `0xRRGGBB` colours.
fn contrast_ratio(a: u32, b: u32) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn relative_luminance(color: u32) -> f32 {
    let linear = |shift: u32| {
        let channel = ((color >> shift) & 0xff) as f32 / 255.;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0)
}

/// Whether what is on screen right now is the dark palette.
///
/// Read at draw time by the surfaces that pick a colour themselves rather
/// than taking one from the theme -- the personal-rating bands and the
/// semantic tones above, which resolve a colour from a value alone
/// (`resolve_color`, the tracker's cell colours) in element builders that
/// have no context to hand. The theme it mirrors is itself global, and
/// `apply_egui_theme` is the only writer.
pub fn is_dark_mode() -> bool {
    DARK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

static DARK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[cfg(test)]
mod tests {
    use super::DARK;
    use super::DARK_SEMANTIC;
    use super::LIGHT;
    use super::LIGHT_SEMANTIC;
    use super::ThemeChoice;

    /// Distinct surface tiers preserve panel and control boundaries.
    #[test]
    fn surface_tiers_are_distinct() {
        for palette in [&DARK, &LIGHT] {
            let tiers = [palette.surface, palette.panel, palette.card, palette.widget, palette.widget_hot];
            for (ix, tier) in tiers.iter().enumerate() {
                assert!(!tiers[..ix].contains(tier), "two surface tiers share a value: {tier:#08x}");
            }
        }
    }

    /// A battle result is read by its colour, so the three cannot collide,
    /// and neither set may be the other's copy.
    #[test]
    fn the_battle_results_are_told_apart_in_both_themes() {
        for semantic in [&DARK_SEMANTIC, &LIGHT_SEMANTIC] {
            assert_ne!(semantic.win, semantic.loss);
            assert_ne!(semantic.win, semantic.draw);
            assert_ne!(semantic.loss, semantic.draw);
        }
        assert_ne!(DARK_SEMANTIC.win, LIGHT_SEMANTIC.win);
        assert_ne!(DARK_SEMANTIC.text_dim, LIGHT_SEMANTIC.text_dim);
    }

    /// The result tones are read as text on the panel and the striped rows,
    /// in every palette.
    #[test]
    fn the_battle_results_clear_the_contrast_floor_on_every_row() {
        for choice in ThemeChoice::ALL {
            for system_dark in [true, false] {
                let palette = super::palette_for(choice, system_dark);
                let semantic = super::semantic_for(palette.mode.is_dark());
                for tone in [semantic.win, semantic.loss] {
                    for ground in [palette.panel, palette.faint] {
                        let ratio = super::contrast_ratio(tone, ground);
                        assert!(
                            ratio >= super::CONTRAST_FLOOR,
                            "{choice:?}: {tone:#08x} on {ground:#08x} is only {ratio:.2}:1"
                        );
                    }
                }
            }
        }
    }

    /// A colour the app does not choose is lifted only as far as the floor,
    /// keeps its hue, and is left alone when it already reads.
    #[test]
    fn a_pale_clan_colour_is_darkened_on_a_light_panel_and_kept_on_a_dark_one() {
        let pale_green = 0xa0e8a0;
        let light = [LIGHT.panel, LIGHT.faint];
        let lifted = super::readable_on(pale_green, &light);
        for ground in light {
            assert!(super::contrast_ratio(lifted, ground) >= super::CONTRAST_FLOOR);
        }
        let hue = |color: u32| gpui_kit::Hsla::from(gpui_kit::rgb(color)).h;
        assert!((hue(lifted) - hue(pale_green)).abs() < 0.01, "the clan's hue is kept");
        assert!(
            super::contrast_ratio(lifted, LIGHT.panel) < super::CONTRAST_FLOOR + 0.5,
            "lifted no further than needed"
        );

        assert_eq!(super::readable_on(pale_green, &[DARK.panel, DARK.faint]), pale_green, "already readable");
    }
}

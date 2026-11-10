use gpui_kit::App;
use gpui_kit::BoxShadow;
use gpui_kit::Hsla;
use gpui_kit::Window;
use gpui_kit::WindowAppearance;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::theme::ThemeTokens;
use gpui_kit::px;
use gpui_kit::rgb;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::theme::DARK;
use wows_toolkit_viewmodel::theme::DARK_SEMANTIC;
use wows_toolkit_viewmodel::theme::LIGHT_SEMANTIC;
use wows_toolkit_viewmodel::theme::Semantic;
use wows_toolkit_viewmodel::theme::palette_for;
use wows_toolkit_viewmodel::theme::semantic_for;

static CURRENT_ACCENT: AtomicU32 = AtomicU32::new(DARK.accent);
static CURRENT_SURFACE: AtomicU32 = AtomicU32::new(DARK.surface);
static CURRENT_BORDER_BRIGHT: AtomicU32 = AtomicU32::new(DARK.border_bright);
static CURRENT_TEXT_DIM: AtomicU32 = AtomicU32::new(DARK.text_dim);

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
pub fn apply_egui_theme(choice: ThemeChoice, zoom: f32, window: &mut Window, cx: &mut App) {
    let system_dark = system_is_dark(cx);
    let dark = choice.is_dark(system_dark);
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
    let palette = palette_for(choice, system_dark);
    CURRENT_ACCENT.store(palette.accent, Ordering::Relaxed);
    CURRENT_SURFACE.store(palette.surface, Ordering::Relaxed);
    CURRENT_BORDER_BRIGHT.store(palette.border_bright, Ordering::Relaxed);
    CURRENT_TEXT_DIM.store(palette.text_dim, Ordering::Relaxed);
    let semantic = semantic_for(dark);
    Theme::change(if palette.dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);

    let theme = Theme::global_mut(cx);
    theme.background = rgb(palette.panel).into();
    theme.foreground = rgb(palette.text).into();
    theme.border = rgb(palette.border).into();
    theme.secondary = rgb(palette.widget).into();
    theme.secondary_hover = rgb(palette.widget_hot).into();
    theme.secondary_active = rgb(palette.widget_hot).into();
    theme.secondary_foreground = rgb(palette.text).into();
    theme.button = rgb(palette.widget).into();
    theme.button_hover = rgb(palette.widget_hot).into();
    theme.button_active = rgb(palette.widget_hot).into();
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
    theme.switch = rgb(palette.widget).into();
    theme.switch_thumb = rgb(palette.text_bright).into();
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
    theme.font_size = px(12.5 * zoom);
    theme.radius = px(2.0 * zoom);

    // rem-based helpers scale via rem size; px-valued fields were scaled above.
    window.set_rem_size(px(16.0 * zoom));
    cx.refresh_windows();
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
    rgb(wows_toolkit_viewmodel::theme::accent_foreground(CURRENT_ACCENT.load(Ordering::Relaxed))).into()
}

/// A stationary halo for active controls on dark surfaces.
pub fn phosphor_glow() -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), accent().opacity(if is_dark_mode() { 0.18 } else { 0. })).blur_radius(px(8.))
}

/// Window, panel, text, and accent colors for a theme preview.
pub fn swatches(choice: ThemeChoice, cx: &App) -> [u32; 4] {
    wows_toolkit_viewmodel::theme::swatches(choice, system_is_dark(cx))
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

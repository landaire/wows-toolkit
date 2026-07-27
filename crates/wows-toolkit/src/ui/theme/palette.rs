//! Graphite & Bone raw constants. Warm achromatic chrome; the only "accent"
//! is bone, which carries active and selected states by inversion rather than
//! by hue. Nothing outside `theme` imports this module directly.

/// Dark theme surfaces and chrome.
pub mod dark {
    use egui::Color32;

    pub const SURFACE: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
    pub const PANEL: Color32 = Color32::from_rgb(0x18, 0x18, 0x16);
    pub const CARD: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1C);
    pub const WIDGET: Color32 = Color32::from_rgb(0x25, 0x25, 0x22);
    pub const WIDGET_HOT: Color32 = Color32::from_rgb(0x2F, 0x2F, 0x29);
    pub const EXTREME: Color32 = Color32::from_rgb(0x0C, 0x0C, 0x0B);
    pub const BORDER: Color32 = Color32::from_rgb(0x38, 0x37, 0x33);
    pub const BORDER_BRIGHT: Color32 = Color32::from_rgb(0x52, 0x4F, 0x4A);
    pub const FAINT: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1A);
    pub const SELECTION: Color32 = Color32::from_rgb(0x28, 0x28, 0x20);
    pub const ACCENT: Color32 = Color32::from_rgb(0xC7, 0xC3, 0xB8);
    pub const TEXT: Color32 = Color32::from_rgb(0xC9, 0xC6, 0xBE);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x97, 0x97, 0x89);
    pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(0xE8, 0xE5, 0xDC);

    /// Fill for the active dock tab. A raised surface rather than an inverted
    /// block; only ever carries the tab label, never dimmed text.
    pub const TAB_ACTIVE: Color32 = Color32::from_rgb(0x34, 0x34, 0x30);
}

/// Light theme surfaces and chrome. Same neutrals, inverted value scale.
pub mod light {
    use egui::Color32;

    pub const SURFACE: Color32 = Color32::from_rgb(0xE6, 0xE5, 0xE0);
    pub const PANEL: Color32 = Color32::from_rgb(0xF4, 0xF3, 0xEF);
    pub const CARD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    pub const WIDGET: Color32 = Color32::from_rgb(0xEE, 0xED, 0xE8);
    pub const WIDGET_HOT: Color32 = Color32::from_rgb(0xDE, 0xDC, 0xD3);
    pub const EXTREME: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    pub const BORDER: Color32 = Color32::from_rgb(0xCB, 0xC9, 0xC1);
    pub const BORDER_BRIGHT: Color32 = Color32::from_rgb(0x9A, 0x97, 0x8D);
    pub const FAINT: Color32 = Color32::from_rgb(0xF0, 0xEF, 0xEB);
    pub const SELECTION: Color32 = Color32::from_rgb(0xDA, 0xD8, 0xCC);
    pub const ACCENT: Color32 = Color32::from_rgb(0x26, 0x25, 0x1F);
    pub const TEXT: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x17);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x5C, 0x5A, 0x53);
    pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x08);

    /// Fill for the active dock tab. A raised surface rather than an inverted
    /// block; only ever carries the tab label, never dimmed text.
    pub const TAB_ACTIVE: Color32 = Color32::from_rgb(0xDF, 0xDC, 0xD3);
}

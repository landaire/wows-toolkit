//! Graphite & Bone raw constants. Warm achromatic chrome; the only "accent"
//! is bone, which carries active and selected states by inversion rather than
//! by hue. Nothing outside `theme` imports this module directly.

/// Dark theme surfaces and chrome.
pub mod dark {
    use egui::Color32;

    pub const SURFACE: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0A);
    pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x12, 0x11);
    pub const CARD: Color32 = Color32::from_rgb(0x19, 0x19, 0x18);
    pub const WIDGET: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1D);
    pub const WIDGET_HOT: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2B);
    pub const EXTREME: Color32 = Color32::from_rgb(0x07, 0x07, 0x06);
    pub const BORDER: Color32 = Color32::from_rgb(0x30, 0x2F, 0x2C);
    pub const BORDER_BRIGHT: Color32 = Color32::from_rgb(0x4A, 0x49, 0x45);
    pub const FAINT: Color32 = Color32::from_rgb(0x16, 0x16, 0x15);
    pub const SELECTION: Color32 = Color32::from_rgb(0x33, 0x32, 0x2D);
    pub const ACCENT: Color32 = Color32::from_rgb(0xE8, 0xE4, 0xD8);
    pub const TEXT: Color32 = Color32::from_rgb(0xDE, 0xDB, 0xD2);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8E, 0x8B, 0x82);
    pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(0xFA, 0xF8, 0xF1);
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
}

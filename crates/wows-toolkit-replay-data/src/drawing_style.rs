//! The inks and nib widths a drawing tool offers, shared by the replay viewport and the tactics
//! board so both match the egui toolbar.

/// One preset ink a swatch offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink {
    pub rgba: [u8; 4],
    /// Lower-case English name, which element ids are built from.
    pub name: &'static str,
    pub label_key: &'static str,
}

/// The egui toolbar's `PRESET_COLORS`, in its order.
pub const INKS: [Ink; 9] = [
    Ink { rgba: [255, 255, 255, 255], name: "white", label_key: "ui.tactics.ink_white" },
    Ink { rgba: [160, 160, 160, 255], name: "gray", label_key: "ui.tactics.ink_gray" },
    Ink { rgba: [230, 50, 50, 255], name: "red", label_key: "ui.tactics.ink_red" },
    Ink { rgba: [240, 140, 30, 255], name: "orange", label_key: "ui.tactics.ink_orange" },
    Ink { rgba: [240, 230, 50, 255], name: "yellow", label_key: "ui.tactics.ink_yellow" },
    Ink { rgba: [50, 200, 50, 255], name: "green", label_key: "ui.tactics.ink_green" },
    Ink { rgba: [50, 120, 230, 255], name: "blue", label_key: "ui.tactics.ink_blue" },
    Ink { rgba: [180, 60, 230, 255], name: "purple", label_key: "ui.tactics.ink_purple" },
    Ink { rgba: [255, 130, 180, 255], name: "pink", label_key: "ui.tactics.ink_pink" },
];

/// What a tool draws with until the reader picks an ink: egui's `Color32::YELLOW`, which is none
/// of the presets, so no swatch reads as chosen at first.
pub const DEFAULT_INK: [u8; 4] = [255, 255, 0, 255];

/// The nib width egui opens on, and the span its slider and bracket keys hold it to.
pub const DEFAULT_NIB: f32 = 2.0;
pub const MIN_NIB: f32 = 1.0;
pub const MAX_NIB: f32 = 8.0;

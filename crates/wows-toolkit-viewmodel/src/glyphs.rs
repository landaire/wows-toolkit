//! The Phosphor glyphs the shared row text carries.
//!
//! A listing row's stats line is assembled here, icons and all, so both front
//! ends render the same string; each draws it with its own copy of the same
//! font (`egui-phosphor` in the egui app, `icons.rs` in the GPUI port).
//! Codepoints are Phosphor's regular set.

/// Precedes a timestamp.
pub const CLOCK: &str = "\u{E19A}";

/// Precedes a damage figure.
pub const CROSSHAIR_SIMPLE: &str = "\u{E1D8}";

/// Precedes a kill count.
pub const SWORD: &str = "\u{E5BA}";

/// Marks a battle played in a division.
pub const USERS_THREE: &str = "\u{E68E}";

/// A sunk ship. The drawn rows do not carry it (the hover text says "sunk" in
/// words instead); the tests hold that line.
pub const SKULL: &str = "\u{E916}";

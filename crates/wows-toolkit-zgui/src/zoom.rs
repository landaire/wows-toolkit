//! The interface zoom: how large the main window draws its text and controls, and the shortcuts
//! that step it.

use rust_i18n::t;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use zgui::prelude::Zoom;
use zgui::vocab::Key;
use zgui::vocab::KeyEvent;
use zgui::vocab::Modifiers;

/// What a zoom shortcut asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
    /// Unscaled, which is what egui's own reset returns to.
    Reset,
}

impl ZoomStep {
    /// The shortcut `key` with `modifiers` held is, if it is one: Ctrl, or Cmd on macOS, with `=`,
    /// `+`, `-` or `0`, the keypad's included, since they arrive as the same characters.
    pub fn of(key: &KeyEvent, modifiers: Modifiers) -> Option<Self> {
        if !modifiers.primary() || modifiers.alt() || key.repeat {
            return None;
        }

        match &key.key_without_modifiers {
            Key::Character(text) => match text.as_ref() {
                // Shift and `=` arrives as `+` on some layouts and as itself on others.
                "=" | "+" => Some(Self::In),
                "-" => Some(Self::Out),
                "0" => Some(Self::Reset),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn applied_to(self, zoom: ZoomFactor) -> ZoomFactor {
        match self {
            Self::In => zoom.zoomed_in(),
            Self::Out => zoom.zoomed_out(),
            Self::Reset => ZoomFactor::ACTUAL_SIZE,
        }
    }
}

/// `zoom` as the window zoom it is applied through.
pub fn to_window(zoom: ZoomFactor) -> Zoom {
    // `ZoomFactor` is held to 0.5 through 2.0, which the window's range covers.
    Zoom::new(zoom.get()).unwrap_or(Zoom::ONE)
}

/// The zoom shortcuts, with the platform's own modifier name.
pub fn shortcuts_hint() -> String {
    let modifier = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };

    t!(
        "ui.settings.panel.zoom_shortcuts",
        zoom_in = format!("{modifier}+="),
        zoom_out = format!("{modifier}+-"),
        reset = format!("{modifier}+0")
    )
    .into_owned()
}

#[cfg(test)]
mod tests {
    use zgui::vocab::KeyCode;
    use zgui::vocab::NamedKey;
    use zgui::vocab::PhysicalKey;

    use super::*;

    fn character(text: &str) -> KeyEvent {
        let mut event = KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter));
        event.key = Key::Character(text.into());
        event.key_without_modifiers = Key::Character(text.into());
        event
    }

    #[test]
    fn the_shortcuts_step_and_reset_with_the_primary_modifier_only() {
        assert_eq!(ZoomStep::of(&character("="), Modifiers::PRIMARY), Some(ZoomStep::In));
        assert_eq!(ZoomStep::of(&character("-"), Modifiers::PRIMARY), Some(ZoomStep::Out));
        assert_eq!(ZoomStep::of(&character("0"), Modifiers::PRIMARY), Some(ZoomStep::Reset));
        assert_eq!(ZoomStep::of(&character("="), Modifiers::NONE), None);
        assert_eq!(ZoomStep::of(&character("k"), Modifiers::PRIMARY), None);
    }

    #[test]
    fn steps_stay_inside_the_range() {
        let mut zoom = ZoomFactor::DEFAULT;
        for _ in 0..30 {
            zoom = ZoomStep::In.applied_to(zoom);
        }
        assert_eq!(zoom, ZoomFactor::MAX);
        assert!(Zoom::new(zoom.get()).is_ok());

        for _ in 0..30 {
            zoom = ZoomStep::Out.applied_to(zoom);
        }
        assert_eq!(zoom, ZoomFactor::MIN);
        assert_eq!(ZoomStep::Reset.applied_to(zoom), ZoomFactor::ACTUAL_SIZE);
    }
}

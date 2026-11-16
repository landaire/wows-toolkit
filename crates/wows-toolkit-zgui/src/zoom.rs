//! The interface zoom: how large the windows draw their text and controls, and the shortcuts
//! that step it.

use landaire_ui::Notice;
use landaire_ui::ToastKey;
use landaire_ui::Toasts;
use rust_i18n::t;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use wows_toolkit_viewmodel::settings::keys;
use zgui::prelude::Zoom;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::WithValue;
use zgui::view::EventCx;
use zgui::view::ListenerGuard;
use zgui::view::ListenerOptions;
use zgui::view::NodeRef;
use zgui::view::events;
use zgui::vocab::Key;
use zgui::vocab::KeyEvent;
use zgui::vocab::Modifiers;

use crate::services::Services;
use crate::shell::Appearance;

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
    /// `+`, `-` or `0`, the keypad's included, since they arrive as the same characters. A held
    /// key's repeats count, so holding it keeps stepping.
    pub fn of(key: &KeyEvent, modifiers: Modifiers) -> Option<Self> {
        if !modifiers.primary() || modifiers.alt() {
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

/// Applies `zoom` to every window and saves it.
pub fn apply(services: Services, appearance: Appearance, zoom: ZoomFactor) {
    appearance.zoom.set(zoom);
    services.settings.edit(keys::ZOOM_FACTOR, |settings| {
        settings.zoom = zoom;
        zoom.get()
    });
}

/// Hears the zoom shortcuts anywhere in the window `root` is in: before anything has focus, in a
/// focused field and in a dialog. The listener is on the window root's capture leg, so nothing
/// below it takes the key first.
pub fn hear_zoom_keys(root: NodeRef, services: Services, appearance: Appearance, toasts: Toasts) {
    let guard = StoredValue::new_local(None::<ListenerGuard>);
    let attaching = RenderEffect::new(move |_| {
        if root.get().is_none() || guard.with_value(Option::is_some) {
            return;
        }
        let Some(window) = root.window_root() else {
            return;
        };

        guard.set_value(window.listen(
            events::KEY_DOWN,
            ListenerOptions::CAPTURE,
            move |ev: &mut EventCx<'_, events::KeyDown>| {
                let modifiers = ev.modifiers;
                let Some(step) = ZoomStep::of(ev, modifiers) else {
                    return;
                };
                take_step(services, appearance, toasts, step);
                ev.prevent_default();
                ev.stop_propagation();
            },
        ));
    });
    on_cleanup_local(move || {
        drop(attaching);
        guard.set_value(None);
    });
}

/// Steps the zoom and says the new one. A step repeats while its key is held.
fn take_step(services: Services, appearance: Appearance, toasts: Toasts, step: ZoomStep) {
    // The message a zoom step says, which the next step's replaces.
    const ZOOM_TOAST: &str = "zoom";

    // The zoom waits for the settings, which hold the zoom it steps from. Settings that could not
    // be read leave the default zoom to step from.
    if services.settings.is_loading_untracked() {
        return;
    }

    let zoom = step.applied_to(appearance.zoom.get_untracked());
    apply(services, appearance, zoom);

    // Said, since the tab with the zoom slider may not be the one open.
    let said = t!("ui.settings.panel.zoom_changed", factor = format!("{:.1}", zoom.get()));
    toasts.show(Notice::info(said).keyed(ToastKey::new(ZOOM_TOAST)));
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

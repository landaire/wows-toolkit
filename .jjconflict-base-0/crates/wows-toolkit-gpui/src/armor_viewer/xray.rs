//! Whether the highlight of a part hovered in the view shows through the
//! geometry in front of it. A hovered listing row's highlight always does.

use gpui_kit::App;
use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;

use crate::status_bar::ModifierKey;
use crate::viewport::renderer::HighlightDepth;

/// When the highlight of a part hovered in the view shows through what is in
/// front of it.
/// Persisted under `keys::MODEL_VIEWER_XRAY_HIGHLIGHT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayHighlight {
    #[default]
    Always,
    /// Only while [`XrayHighlight::KEY`] is held.
    WhileHeld,
    Off,
}

impl XrayHighlight {
    pub const ALL: [Self; 3] = [Self::Always, Self::WhileHeld, Self::Off];

    /// The key [`Self::WhileHeld`] waits for. Shift casts shells, and window
    /// managers often take Alt.
    pub const KEY: ModifierKey = ModifierKey::Secondary;

    /// How the highlight meets the geometry in front of it while
    /// [`Self::KEY`] is or is not held.
    pub fn depth(self, key_held: bool) -> HighlightDepth {
        match self {
            Self::Always => HighlightDepth::XRay,
            Self::WhileHeld if key_held => HighlightDepth::XRay,
            Self::WhileHeld | Self::Off => HighlightDepth::Tested,
        }
    }

    pub fn label(self, cx: &App) -> String {
        match self {
            Self::Always => t!("ui.armor.xray_always").into_owned(),
            Self::WhileHeld => t!("ui.armor.xray_while_held", key = Self::KEY.label(cx)).into_owned(),
            Self::Off => t!("ui.armor.xray_off").into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::armor_viewer::picking_ui::HighlightOrigin;
    use crate::viewport::renderer::DepthState;
    use crate::viewport::renderer::layer_depth_state;

    #[test]
    fn each_mode_chooses_its_highlight_depth() {
        for held in [false, true] {
            assert_eq!(XrayHighlight::Always.depth(held), HighlightDepth::XRay, "held: {held}");
            assert_eq!(XrayHighlight::Off.depth(held), HighlightDepth::Tested, "held: {held}");
        }
        assert_eq!(XrayHighlight::WhileHeld.depth(true), HighlightDepth::XRay);
        assert_eq!(XrayHighlight::WhileHeld.depth(false), HighlightDepth::Tested);
    }

    /// A row hover shows through in every mode and key state; a hover in the
    /// view is depth-tested whenever its mode says so.
    #[test]
    fn only_a_hover_in_the_view_follows_the_mode() {
        let cases = [
            (XrayHighlight::Always, false, DepthState::Ignored),
            (XrayHighlight::Always, true, DepthState::Ignored),
            (XrayHighlight::WhileHeld, false, DepthState::Tested),
            (XrayHighlight::WhileHeld, true, DepthState::Ignored),
            (XrayHighlight::Off, false, DepthState::Tested),
            (XrayHighlight::Off, true, DepthState::Tested),
        ];
        for (mode, held, view) in cases {
            let depth = mode.depth(held);

            assert_eq!(layer_depth_state(HighlightOrigin::Row.layer(), depth), DepthState::Ignored, "{mode:?} {held}");
            assert_eq!(layer_depth_state(HighlightOrigin::ViewHover.layer(), depth), view, "{mode:?} {held}");
        }
    }

    #[test]
    fn the_stored_form_is_stable() {
        let stored: Vec<String> =
            XrayHighlight::ALL.iter().map(|mode| serde_json::to_string(mode).expect("serializes")).collect();
        assert_eq!(stored, ["\"always\"", "\"while_held\"", "\"off\""]);
    }
}

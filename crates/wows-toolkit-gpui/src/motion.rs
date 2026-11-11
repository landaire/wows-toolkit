//! Movement follows the theme's motion tokens and holds still when the system
//! asks for reduced motion.

use std::f32::consts::FRAC_PI_2;

use gpui_kit::Animation;
use gpui_kit::AnimationExt as _;
use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::ElementId;
use gpui_kit::IntoElement;
use gpui_kit::RenderOnce;
use gpui_kit::Styled;
use gpui_kit::Window;
use gpui_kit::base::Transition;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::radians;

/// How far a disclosure has turned, from 0 closed to 1 open.
///
/// Two calls that share `id` in one frame with different `open` values
/// retarget each other every frame and never settle.
pub fn disclosure_turn(id: impl Into<ElementId>, open: bool, window: &mut Window, cx: &mut App) -> f32 {
    let tokens = cx.theme().motion_tokens();
    let policy = Transition::new(tokens.duration_fast).easing(tokens.easing_move.clone());
    // The first draw adopts the state as it is, so a caret scrolled into
    // view or drawn already open does not turn.
    gpui_kit::base::transition((id.into(), "disclosure"), if open { 1.0_f32 } else { 0.0 }, policy, window, cx)
}

/// A caret that points right at `turn` 0 and down at 1.
pub fn chevron(turn: f32) -> Icon {
    Icon::new(IconName::ChevronRight).rotate(radians(turn * FRAC_PI_2))
}

/// A caret that turns as `open` changes. Its state is keyed by `id` under
/// the enclosing element ids, so it suits sites whose parents are not
/// positional; list rows use `disclosure_turn` keyed by a stable row key.
/// The failure mode of `disclosure_turn` applies to `id`.
#[derive(IntoElement)]
pub struct DisclosureChevron {
    id: ElementId,
    open: bool,
    size: Option<Size>,
}

impl DisclosureChevron {
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self { id: id.into(), open, size: None }
    }
}

impl Sizable for DisclosureChevron {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = Some(size.into());
        self
    }
}

impl RenderOnce for DisclosureChevron {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let icon = chevron(disclosure_turn(self.id, self.open, window, cx));
        match self.size {
            Some(size) => icon.with_size(size),
            None => icon,
        }
    }
}

/// Fades `element` in once, when it is first drawn under `id`.
pub fn fade_in<E>(id: impl Into<ElementId>, element: E, cx: &App) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let tokens = cx.theme().motion_tokens();
    let easing = tokens.easing_enter.clone();
    let animation = Animation::new(tokens.duration_fast).with_easing(move |delta| easing.sample(delta));
    element.with_animation(id, animation, |element, delta| element.opacity(delta)).into_any_element()
}

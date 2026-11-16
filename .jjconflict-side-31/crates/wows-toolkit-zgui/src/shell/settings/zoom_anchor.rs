//! Keeping the zoom slider where it is drawn across a zoom change.
//!
//! A zoom rescales the whole window, the list included, so without help the slider moves out from
//! under a pointer that has not moved. Before the change lands the list is told where to scroll,
//! predicted from the zoom ratio, so the first frame at the new zoom is close. Lengths that do not
//! scale exactly (rounding, the window's own edges) leave a few pixels over, which the first scroll
//! report at the new zoom corrects.

use wows_toolkit_viewmodel::settings::ZoomFactor;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Track;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;

use crate::shell::Shell;

/// The zoom slider's element, which the zoom row binds and the list keeps in place.
#[derive(Clone, Copy)]
pub struct ZoomAnchor {
    pub slider: NodeRef,
}

/// A zoom change whose first layout has not been measured yet.
#[derive(Clone, Copy)]
struct Pending {
    /// The window's device pixel ratio before the change, which tells a report of the old layout
    /// from one of the new.
    scale_before: f32,
    /// Where the slider's top is to be drawn, in window device pixels.
    wanted_top: f32,
}

impl ZoomAnchor {
    /// Follows the shell's zoom while the list over `viewport` is drawn.
    pub fn follow(shell: Shell, viewport: NodeRef) -> Self {
        let slider = NodeRef::new();
        let pending = StoredValue::new_local(None::<Pending>);
        let zoom = shell.appearance.zoom;

        let following = RenderEffect::new(move |before: Option<ZoomFactor>| {
            let now = zoom.get();
            if let Some(before) = before
                && before != now
            {
                pending.set_value(keep_in_place(viewport, slider, before, now));
            }

            now
        });
        on_cleanup_local(move || drop(following));

        let scrolled = viewport.observe_scroll();
        let correcting = RenderEffect::new(move |_| {
            scrolled.track();
            correct(viewport, slider, pending);
        });
        on_cleanup_local(move || drop(correcting));

        Self { slider }
    }
}

/// Scrolls `viewport` so `slider`, drawn now at `before`, is drawn at about the same height at
/// `now`, and returns what the correction after the change needs. A slider scrolled out of view is
/// left out of view.
fn keep_in_place(viewport: NodeRef, slider: NodeRef, before: ZoomFactor, now: ZoomFactor) -> Option<Pending> {
    // Both from the last frame drawn, which is at the old zoom.
    let port = viewport.window_bounds()?;
    let drawn = slider.window_bounds()?;
    let port_top = port.origin.y.0;
    let port_bottom = port_top + port.size.height.0;
    let slider_top = drawn.origin.y.0;
    if slider_top + drawn.size.height.0 < port_top || slider_top > port_bottom {
        return None;
    }

    let ratio = now.get() / before.get();
    let offset = viewport.scroll_offset();
    // Where the slider is in the list's content, and where the list's top will be.
    let in_content = slider_top - port_top + offset.y.0;
    let port_top_after = port_top * ratio;
    // Not above the list's own top, which a zoom in moves down as the header over it grows.
    let wanted_top = slider_top.max(port_top_after);
    let scrolled = (in_content * ratio + port_top_after - wanted_top).max(0.0);

    // Carried out before the window takes the new ratio, which then carries every scroll offset
    // across by the same ratio; so the offset asked for is the one at the old zoom.
    let asked = Point::new(offset.x, DevicePx(scrolled / ratio));
    viewport.scroll_to(ScrollTarget::Offset(asked), ScrollBehavior::Instant);

    Some(Pending { scale_before: viewport.scale(), wanted_top })
}

/// Scrolls by what the prediction missed, once a layout at the new zoom has been reported.
fn correct(viewport: NodeRef, slider: NodeRef, pending: StoredValue<Option<Pending>, LocalStorage>) {
    let Some(asked) = pending.get_value() else { return };
    if viewport.scale() == asked.scale_before {
        return;
    }
    pending.set_value(None);

    let Some(drawn) = slider.window_bounds() else { return };
    let missed = drawn.origin.y.0 - asked.wanted_top;
    if missed.abs() >= 0.5 {
        viewport.scroll_to(ScrollTarget::By(Point::new(DevicePx(0.0), DevicePx(missed))), ScrollBehavior::Instant);
    }
}

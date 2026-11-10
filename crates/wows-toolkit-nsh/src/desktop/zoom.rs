//! The UI zoom: a container that lays its child out in a viewport `1 / scale`
//! of its own size and draws it scaled up by `scale`, so every length the
//! interface writes (sizes, spacing, radii, borders and text) grows together,
//! as the other front ends' zoom does. nsh maps pointer input through the
//! transform, so hit testing follows.

use gtk::glib;
use gtk::prelude::WidgetExt as _;
use gtk::subclass::prelude::ObjectSubclassIsExt as _;
use nsh_platform_linux::gtk4 as gtk;

use crate::shell::DEFAULT_ZOOM;
use crate::shell::ZoomFactor;

mod imp {
    use std::cell::Cell;

    use gtk::glib;
    use gtk::gsk;
    use gtk::prelude::WidgetExt as _;
    use gtk::subclass::prelude::ObjectImpl;
    use gtk::subclass::prelude::ObjectSubclass;
    use gtk::subclass::prelude::ObjectSubclassExt as _;
    use gtk::subclass::prelude::WidgetImpl;
    use nsh_platform_linux::gtk4 as gtk;

    pub struct ZoomBin {
        pub scale: Cell<f32>,
    }

    impl Default for ZoomBin {
        fn default() -> Self {
            Self { scale: Cell::new(1.0) }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZoomBin {
        const NAME: &'static str = "WowsToolkitZoomBin";
        type Type = super::ZoomBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ZoomBin {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ZoomBin {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let scale = self.scale.get();
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };

            let for_size = if for_size < 0 { -1 } else { (for_size as f32 / scale).floor() as i32 };
            let (minimum, natural, _, _) = child.measure(orientation, for_size);

            ((minimum as f32 * scale).ceil() as i32, (natural as f32 * scale).ceil() as i32, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let scale = self.scale.get();
            let Some(child) = self.obj().first_child() else {
                return;
            };

            // Rounded up, so the scaled child reaches the edge; the bin clips
            // the fraction past it.
            let inner_width = (width as f32 / scale).ceil() as i32;
            let inner_height = (height as f32 / scale).ceil() as i32;
            let transform = (scale != 1.0).then(|| gsk::Transform::new().scale(scale, scale));
            child.allocate(inner_width, inner_height, -1, transform);
        }
    }
}

glib::wrapper! {
    pub struct ZoomBin(ObjectSubclass<imp::ZoomBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ZoomBin {
    /// `child` drawn at `zoom`, or at its designed size when `zoom` cannot be
    /// drawn at.
    pub fn new(child: &gtk::Widget, zoom: ZoomFactor) -> Self {
        let bin: Self = glib::Object::new();
        bin.set_overflow(gtk::Overflow::Hidden);
        if let Some(scale) = scale(zoom) {
            bin.imp().scale.set(scale);
        }
        child.set_parent(&bin);

        bin
    }

    /// Draws the child at `zoom`, laying it out again for it. A zoom that
    /// cannot be drawn at is ignored.
    pub fn set_zoom(&self, zoom: ZoomFactor) {
        let Some(scale) = scale(zoom) else {
            return;
        };
        if scale == self.imp().scale.get() {
            return;
        }

        self.imp().scale.set(scale);
        self.queue_resize();
    }
}

/// The scale a zoom factor draws at, `None` for one that is not a positive
/// number. The zoom factor is the one the other front ends store, whose
/// default is 1.15; this view's sizes are designed at that default, so it is
/// the 1.0 of this scale.
fn scale(zoom: ZoomFactor) -> Option<f32> {
    let scale = zoom.get() / DEFAULT_ZOOM.get();

    (scale.is_finite() && scale > 0.0).then_some(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_default_zoom_draws_at_the_designed_size() {
        assert_eq!(scale(DEFAULT_ZOOM), Some(1.0));
        assert!(scale(ZoomFactor::new(2.0)).is_some_and(|scale| scale > 1.0));
        assert!(scale(ZoomFactor::new(0.5)).is_some_and(|scale| scale < 1.0));
        assert_eq!(scale(ZoomFactor::new(0.0)), None);
        assert_eq!(scale(ZoomFactor::new(f32::NAN)), None);
    }
}

//! Items a context menu shows.

use std::rc::Rc;

use zgui::view::AnyView;
use zgui::view::IntoView;

/// Builds application items each time a context menu shows them.
#[derive(Clone)]
pub struct MenuItems(Rc<dyn Fn() -> AnyView>);

impl MenuItems {
    pub fn new<V>(build: impl Fn() -> V + 'static) -> Self
    where
        V: IntoView + 'static,
    {
        Self(Rc::new(move || AnyView::new(build())))
    }

    pub(crate) fn build(&self) -> AnyView {
        (self.0)()
    }
}

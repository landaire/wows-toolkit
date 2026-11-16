//! The items a row's context menu shows.

use std::rc::Rc;

use zgui::view::AnyView;
use zgui::view::IntoView;

/// Builds the items of a row's context menu each time the menu shows them. A tree's or a grid's
/// menu builder returns one for a row that has a menu and `None` for a row that has none, so a
/// row with nothing to offer opens no menu.
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

//! Opening a view headlessly to drive it and to look at it.

mod shot;
mod window;

pub use crate::test_support::shot::render_png;
pub use crate::test_support::window::A11yState;
pub use crate::test_support::window::DisplayScale;
pub use crate::test_support::window::Drawing;
pub use crate::test_support::window::Element;
pub use crate::test_support::window::HeadlessOptions;
pub use crate::test_support::window::HeadlessWindow;
pub use crate::test_support::window::TEST_ID_ATTRIBUTE;
pub use crate::test_support::window::WindowSize;

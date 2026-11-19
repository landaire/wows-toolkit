//! Controls that edit one value: a number, a number on a slider, a span on a slider, a color, a
//! password, a switch or checkbox named by its label, the Escape behavior of a filter field, and
//! moving the focus to a field once it is built.

// A component's props are its function's arguments.
#![allow(clippy::too_many_arguments)]

pub(crate) mod color_picker;
pub(crate) mod escape;
pub(crate) mod focus;
pub(crate) mod number;
pub(crate) mod password;
pub(crate) mod range_slider;
pub(crate) mod slider_field;
pub(crate) mod toggle_field;

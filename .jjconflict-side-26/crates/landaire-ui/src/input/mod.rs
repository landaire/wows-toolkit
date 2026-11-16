//! Controls that edit one value: a number, a number on a slider, a span on a slider, a color, and
//! the Escape behavior of a filter field.

// A component's props are its function's arguments.
#![allow(clippy::too_many_arguments)]

pub(crate) mod color_picker;
pub(crate) mod escape;
pub(crate) mod number;
pub(crate) mod range_slider;
pub(crate) mod slider_field;

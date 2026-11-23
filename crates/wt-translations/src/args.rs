//! Conversion of `t!` arguments into Fluent values.
//!
//! `t!` calls `(&Arg(&value)).fluent_value()`. Method lookup tries the
//! receiver `&Arg<T>` before auto-referencing to `&&Arg<T>`, so
//! [`ArgFromNumber`] wins whenever `T` is numeric and [`ArgFromDisplay`]
//! serves everything else. Numbers stay numbers so plural selectors work.

use std::borrow::Cow;
use std::fmt::Display;
use std::str::FromStr;

use fluent_bundle::FluentValue;
use fluent_bundle::types::FluentNumber;
use fluent_bundle::types::FluentNumberOptions;

pub struct Arg<'a, T: ?Sized>(pub &'a T);

pub trait ArgFromNumber {
    fn fluent_value(&self) -> FluentValue<'static>;
}

impl<T> ArgFromNumber for Arg<'_, T>
where
    T: Numeric + ?Sized,
{
    fn fluent_value(&self) -> FluentValue<'static> {
        self.0.to_fluent()
    }
}

pub trait ArgFromDisplay {
    fn fluent_value(&self) -> FluentValue<'static>;
}

impl<T> ArgFromDisplay for &Arg<'_, T>
where
    T: Display + ?Sized,
{
    fn fluent_value(&self) -> FluentValue<'static> {
        FluentValue::String(Cow::Owned(self.0.to_string()))
    }
}

/// A primitive number, or a reference to one.
pub trait Numeric {
    fn to_fluent(&self) -> FluentValue<'static>;
}

impl<T> Numeric for &T
where
    T: Numeric + ?Sized,
{
    fn to_fluent(&self) -> FluentValue<'static> {
        (**self).to_fluent()
    }
}

macro_rules! numeric_integer {
    ($($ty:ty)*) => {$(
        impl Numeric for $ty {
            fn to_fluent(&self) -> FluentValue<'static> {
                integer_value(i128::try_from(*self).ok(), self)
            }
        }
    )*};
}

numeric_integer!(i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize);

macro_rules! numeric_float {
    ($($ty:ty)*) => {$(
        impl Numeric for $ty {
            fn to_fluent(&self) -> FluentValue<'static> {
                float_value(self)
            }
        }
    )*};
}

numeric_float!(f32 f64);

/// Fluent numbers are `f64`, so an integer past 2^53 would print rounded.
/// Such a value is passed as its text instead.
fn integer_value(value: Option<i128>, display: &dyn Display) -> FluentValue<'static> {
    const EXACT_LIMIT: u128 = 1 << f64::MANTISSA_DIGITS;

    match value {
        Some(value) if value.unsigned_abs() <= EXACT_LIMIT => {
            FluentValue::Number(FluentNumber::new(value as f64, FluentNumberOptions::default()))
        }
        _ => FluentValue::String(Cow::Owned(display.to_string())),
    }
}

/// Parsed from the value's own `Display` text, so an `f32` prints as it
/// always has rather than as its widened `f64`.
fn float_value(value: &dyn Display) -> FluentValue<'static> {
    let text = value.to_string();
    match FluentNumber::from_str(&text) {
        Ok(number) => FluentValue::Number(number),
        Err(_) => FluentValue::String(Cow::Owned(text)),
    }
}

// The tests spell out the `t!` expansion, whose explicit borrow is what picks the impl.
#[cfg(test)]
#[allow(clippy::needless_borrow)]
mod tests {
    use super::*;

    fn text(value: FluentValue<'static>) -> String {
        match value {
            FluentValue::Number(number) => number.as_string().into_owned(),
            FluentValue::String(text) => text.into_owned(),
            other => panic!("unexpected value {other:?}"),
        }
    }

    #[test]
    fn integers_become_numbers_that_print_as_before() {
        let value = (&Arg(&12345_usize)).fluent_value();
        assert!(matches!(value, FluentValue::Number(_)));
        assert_eq!(text(value), "12345");
        assert_eq!(text((&Arg(&-7_i32)).fluent_value()), "-7");
    }

    #[test]
    fn a_reference_to_a_number_is_still_a_number() {
        let count = 3_u32;
        let by_ref = &count;
        assert!(matches!((&Arg(&by_ref)).fluent_value(), FluentValue::Number(_)));
    }

    #[test]
    fn floats_print_as_their_display_text() {
        assert_eq!(text((&Arg(&0.1_f32)).fluent_value()), "0.1");
        assert_eq!(text((&Arg(&2.5_f64)).fluent_value()), "2.5");
        assert_eq!(text((&Arg(&3.0_f64)).fluent_value()), "3");
    }

    #[test]
    fn integers_past_f64_precision_keep_every_digit() {
        assert_eq!(text((&Arg(&u64::MAX)).fluent_value()), "18446744073709551615");
    }

    #[test]
    fn other_values_become_strings() {
        let value = (&Arg(&"ship")).fluent_value();
        assert!(matches!(value, FluentValue::String(_)));
        assert_eq!(text(value), "ship");
    }
}

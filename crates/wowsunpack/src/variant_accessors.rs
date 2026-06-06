//! `variant_accessors!` generates ergonomic inherent accessor methods for enums
//! whose variants each carry a single payload.
//!
//! For every variant it emits the predicates `is_{name}` / `is_not_{name}` and
//! the variant-selecting combinators `and_{name}` / `or_{name}`. Tuple variants
//! additionally get the Option/Result extractors (`{name}`, `{name}_ref`,
//! `{name}_mut`, `{name}_or`, `{name}_or_else`, `expect_{name}`,
//! `unwrap_{name}`, `and_then_{name}`, ...). The `{name}` token is the
//! snake_case method base name chosen for the variant.

/// Generate accessor methods on an enum with single-payload variants.
///
/// `tuple $Variant($Ty) => $name` declares a single-payload tuple variant and
/// the snake_case base name used for its methods. `unit $Variant => $name`
/// declares a fieldless variant, which only receives the predicate and
/// combinator methods. An optional single lifetime may follow the enum name
/// (e.g. for `ArgValue<'a>`).
macro_rules! variant_accessors {
    (
        $enum:ident $(<$lt:lifetime>)? {
            $( tuple $variant:ident($ty:ty) => $name:ident; )*
            $( unit $uvariant:ident => $uname:ident; )*
        }
    ) => {
        ::paste::paste! {
            impl $(<$lt>)? $enum $(<$lt>)? {
                $(
                    #[inline]
                    pub fn [<is_ $name>](&self) -> bool {
                        matches!(self, $enum::$variant(..))
                    }

                    #[inline]
                    pub fn [<is_not_ $name>](&self) -> bool {
                        !self.[<is_ $name>]()
                    }

                    #[inline]
                    pub fn [<and_ $name>](self, and: Self) -> Self {
                        match (&self, &and) {
                            ($enum::$variant(..), $enum::$variant(..)) => and,
                            _ => self,
                        }
                    }

                    #[inline]
                    pub fn [<or_ $name>](self, or: Self) -> Self {
                        match &self {
                            $enum::$variant(..) => self,
                            _ => or,
                        }
                    }

                    #[inline]
                    pub fn $name(self) -> ::std::option::Option<$ty> {
                        match self {
                            $enum::$variant(value) => ::std::option::Option::Some(value),
                            _ => ::std::option::Option::None,
                        }
                    }

                    #[inline]
                    pub fn [<$name _ref>](&self) -> ::std::option::Option<&$ty> {
                        match self {
                            $enum::$variant(value) => ::std::option::Option::Some(value),
                            _ => ::std::option::Option::None,
                        }
                    }

                    #[inline]
                    pub fn [<$name _mut>](&mut self) -> ::std::option::Option<&mut $ty> {
                        match self {
                            $enum::$variant(value) => ::std::option::Option::Some(value),
                            _ => ::std::option::Option::None,
                        }
                    }

                    #[inline]
                    pub fn [<$name _or>]<E>(self, or: E) -> ::std::result::Result<$ty, E> {
                        self.[<$name _or_else>](|| or)
                    }

                    #[inline]
                    pub fn [<$name _or_else>]<E, F: ::std::ops::FnOnce() -> E>(self, or_else: F) -> ::std::result::Result<$ty, E> {
                        match self {
                            $enum::$variant(value) => ::std::result::Result::Ok(value),
                            _ => ::std::result::Result::Err(or_else()),
                        }
                    }

                    #[inline]
                    pub fn [<$name _ref_or>]<E>(&self, or: E) -> ::std::result::Result<&$ty, E> {
                        self.[<$name _ref_or_else>](|| or)
                    }

                    #[inline]
                    pub fn [<$name _ref_or_else>]<E, F: ::std::ops::FnOnce() -> E>(&self, or_else: F) -> ::std::result::Result<&$ty, E> {
                        match self {
                            $enum::$variant(value) => ::std::result::Result::Ok(value),
                            _ => ::std::result::Result::Err(or_else()),
                        }
                    }

                    #[inline]
                    pub fn [<$name _mut_or>]<E>(&mut self, or: E) -> ::std::result::Result<&mut $ty, E> {
                        self.[<$name _mut_or_else>](|| or)
                    }

                    #[inline]
                    pub fn [<$name _mut_or_else>]<E, F: ::std::ops::FnOnce() -> E>(&mut self, or_else: F) -> ::std::result::Result<&mut $ty, E> {
                        match self {
                            $enum::$variant(value) => ::std::result::Result::Ok(value),
                            _ => ::std::result::Result::Err(or_else()),
                        }
                    }

                    #[inline]
                    pub fn [<and_then_ $name>]<F: ::std::ops::FnOnce($ty) -> $ty>(self, and_then: F) -> Self {
                        match self {
                            $enum::$variant(value) => $enum::$variant(and_then(value)),
                            _ => self,
                        }
                    }

                    #[inline]
                    pub fn [<expect_ $name>](self, msg: &str) -> $ty {
                        self.[<unwrap_or_else_ $name>](|| ::std::panic!("{}", msg))
                    }

                    #[inline]
                    pub fn [<or_else_ $name>]<F: ::std::ops::FnOnce() -> $ty>(self, or_else: F) -> Self {
                        match self {
                            $enum::$variant(value) => $enum::$variant(value),
                            _ => $enum::$variant(or_else()),
                        }
                    }

                    #[inline]
                    pub fn [<unwrap_ $name>](self) -> $ty {
                        self.[<unwrap_or_else_ $name>](|| ::std::panic!())
                    }

                    #[inline]
                    pub fn [<unwrap_or_ $name>](self, or: $ty) -> $ty {
                        self.[<unwrap_or_else_ $name>](|| or)
                    }

                    #[inline]
                    pub fn [<unwrap_or_else_ $name>]<F: ::std::ops::FnOnce() -> $ty>(self, or_else: F) -> $ty {
                        match self {
                            $enum::$variant(value) => value,
                            _ => or_else(),
                        }
                    }
                )*

                $(
                    #[inline]
                    pub fn [<is_ $uname>](&self) -> bool {
                        matches!(self, $enum::$uvariant)
                    }

                    #[inline]
                    pub fn [<is_not_ $uname>](&self) -> bool {
                        !self.[<is_ $uname>]()
                    }

                    #[inline]
                    pub fn [<and_ $uname>](self, and: Self) -> Self {
                        match (&self, &and) {
                            ($enum::$uvariant, $enum::$uvariant) => and,
                            _ => self,
                        }
                    }

                    #[inline]
                    pub fn [<or_ $uname>](self, or: Self) -> Self {
                        match &self {
                            $enum::$uvariant => self,
                            _ => or,
                        }
                    }
                )*
            }
        }
    };
}

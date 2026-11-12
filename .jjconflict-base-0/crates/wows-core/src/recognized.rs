use std::fmt;

/// A value that was either successfully recognized as a known variant `T`,
/// or is an unrecognized raw value `Raw`.
///
/// This is conceptually similar to `Result<T, Raw>`, but for cases where the
/// "error" isn't really an error — it's just a value we don't have a typed
/// representation for. The raw value is preserved so callers can inspect or
/// display it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum Recognized<T, Raw = String> {
    Known(T),
    Unknown(Raw),
}

impl<T: Copy, Raw: Copy> Copy for Recognized<T, Raw> {}

impl<T, Raw> Recognized<T, Raw> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Recognized::Known(t) => Some(t),
            Recognized::Unknown(_) => None,
        }
    }

    pub fn into_known(self) -> Option<T> {
        match self {
            Recognized::Known(t) => Some(t),
            Recognized::Unknown(_) => None,
        }
    }

    pub fn unknown(&self) -> Option<&Raw> {
        match self {
            Recognized::Known(_) => None,
            Recognized::Unknown(raw) => Some(raw),
        }
    }

    pub fn into_unknown(self) -> Option<Raw> {
        match self {
            Recognized::Known(_) => None,
            Recognized::Unknown(raw) => Some(raw),
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Recognized::Known(_))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Recognized::Unknown(_))
    }

    pub fn unwrap(self) -> T
    where
        Raw: fmt::Debug,
    {
        match self {
            Recognized::Known(t) => t,
            Recognized::Unknown(raw) => {
                panic!("called `Recognized::unwrap()` on an `Unknown` value: {raw:?}")
            }
        }
    }

    pub fn expect(self, msg: &str) -> T
    where
        Raw: fmt::Debug,
    {
        match self {
            Recognized::Known(t) => t,
            Recognized::Unknown(raw) => panic!("{msg}: {raw:?}"),
        }
    }

    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Recognized::Known(t) => t,
            Recognized::Unknown(_) => default,
        }
    }

    pub fn unwrap_or_else<F: FnOnce(Raw) -> T>(self, f: F) -> T {
        match self {
            Recognized::Known(t) => t,
            Recognized::Unknown(raw) => f(raw),
        }
    }

    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Recognized<U, Raw> {
        match self {
            Recognized::Known(t) => Recognized::Known(f(t)),
            Recognized::Unknown(raw) => Recognized::Unknown(raw),
        }
    }

    pub fn map_unknown<U, F: FnOnce(Raw) -> U>(self, f: F) -> Recognized<T, U> {
        match self {
            Recognized::Known(t) => Recognized::Known(t),
            Recognized::Unknown(raw) => Recognized::Unknown(f(raw)),
        }
    }

    pub fn and_then<U, F: FnOnce(T) -> Recognized<U, Raw>>(self, f: F) -> Recognized<U, Raw> {
        match self {
            Recognized::Known(t) => f(t),
            Recognized::Unknown(raw) => Recognized::Unknown(raw),
        }
    }

    pub fn as_ref(&self) -> Recognized<&T, &Raw> {
        match self {
            Recognized::Known(t) => Recognized::Known(t),
            Recognized::Unknown(raw) => Recognized::Unknown(raw),
        }
    }
}

impl<T, Raw> From<T> for Recognized<T, Raw> {
    fn from(value: T) -> Self {
        Recognized::Known(value)
    }
}

impl<T: fmt::Display, Raw: fmt::Display> fmt::Display for Recognized<T, Raw> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Recognized::Known(t) => t.fmt(f),
            Recognized::Unknown(raw) => raw.fmt(f),
        }
    }
}

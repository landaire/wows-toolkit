use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// Wargaming realm (server region family). Only the global realm is in scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Realm {
    /// Global (Wargaming.net) realm, spelled `WW` in product ids.
    Ww,
}

impl Realm {
    pub fn parse(s: &str) -> Option<Realm> {
        match s.to_ascii_uppercase().as_str() {
            "WW" => Some(Realm::Ww),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Realm::Ww => "WW",
        }
    }
}

/// WGC product identity, e.g. `WOWS.WW.PRODUCTION`.
///
/// Named `ProductId` rather than `AppId` so it does not collide with
/// steamroom's Steam `AppId(u32)`, a distinct concept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductId {
    game: String,
    realm: Realm,
    channel: String,
}

impl ProductId {
    pub fn parse(s: &str) -> Result<ProductId, ProductIdParseError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(ProductIdParseError::WrongPartCount { found: parts.len() });
        }
        let realm =
            Realm::parse(parts[1]).ok_or_else(|| ProductIdParseError::UnknownRealm { realm: parts[1].to_string() })?;
        Ok(ProductId { game: parts[0].to_ascii_uppercase(), realm, channel: parts[2].to_ascii_uppercase() })
    }

    pub fn game(&self) -> &str {
        &self.game
    }

    pub fn realm(&self) -> Realm {
        self.realm
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }
}

impl fmt::Display for ProductId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.game, self.realm.as_str(), self.channel)
    }
}

impl FromStr for ProductId {
    type Err = ProductIdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProductId::parse(s)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProductIdParseError {
    #[error("product id must have 3 dot-separated parts, found {found}")]
    WrongPartCount { found: usize },
    #[error("unknown realm: {realm}")]
    UnknownRealm { realm: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips_wows_ww_production() {
        let id = ProductId::parse("WOWS.WW.PRODUCTION").expect("valid");
        assert_eq!(id.game(), "WOWS");
        assert_eq!(id.realm(), Realm::Ww);
        assert_eq!(id.channel(), "PRODUCTION");
        assert_eq!(id.to_string(), "WOWS.WW.PRODUCTION");
    }

    #[test]
    fn parse_is_case_insensitive_but_display_is_upper() {
        let id = ProductId::parse("wows.ww.production").expect("valid");
        assert_eq!(id.to_string(), "WOWS.WW.PRODUCTION");
    }

    #[test]
    fn rejects_wrong_part_count() {
        assert!(matches!(ProductId::parse("WOWS.WW"), Err(ProductIdParseError::WrongPartCount { found: 2 })));
    }

    #[test]
    fn rejects_unknown_realm() {
        assert!(matches!(ProductId::parse("WOWS.ZZ.PRODUCTION"), Err(ProductIdParseError::UnknownRealm { .. })));
    }
}

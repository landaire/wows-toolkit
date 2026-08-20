use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeviceIdParseError {
    #[error("device id is empty")]
    Empty,
    #[error("expected 64 lowercase hex chars, got {len}")]
    NotHex64 { len: usize },
}

macro_rules! nonempty_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            pub fn parse(s: &str) -> Result<$name, DeviceIdParseError> {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return Err(DeviceIdParseError::Empty);
                }
                Ok($name(trimmed.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

nonempty_id!(
    /// Value of `data/wgc_id.dat`.
    WgcId
);
nonempty_id!(
    /// Value of `data/pc_id.dat`.
    PcId
);
nonempty_id!(
    /// Value of `data/wgc_tracking_id.dat`.
    TrackingId
);
nonempty_id!(
    /// Value of `data/wgc_vending_id.dat`.
    VendingId
);

/// The sha256 hex string naming an entry under `apps/<realm>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallId(String);

impl InstallId {
    pub fn parse(s: &str) -> Result<InstallId, DeviceIdParseError> {
        let t = s.trim();
        let is_hex64 = t.len() == 64 && t.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if is_hex64 { Ok(InstallId(t.to_string())) } else { Err(DeviceIdParseError::NotHex64 { len: t.len() }) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgc_id_accepts_nonempty_and_trims() {
        let id = WgcId::parse(" 41C682322A67B3AC ").expect("valid");
        assert_eq!(id.as_str(), "41C682322A67B3AC");
    }

    #[test]
    fn wgc_id_rejects_empty() {
        assert_eq!(WgcId::parse("   "), Err(DeviceIdParseError::Empty));
    }

    #[test]
    fn install_id_accepts_64_hex() {
        let hex = "9322734250c76609014e9cbbaf99dd38f3fc39cfe38633942a7506e3f079c28b";
        assert_eq!(InstallId::parse(hex).expect("valid").as_str(), hex);
    }

    #[test]
    fn install_id_rejects_non_64_hex() {
        assert!(matches!(InstallId::parse("abc"), Err(DeviceIdParseError::NotHex64 { len: 3 })));
        assert!(matches!(InstallId::parse(&"g".repeat(64)), Err(DeviceIdParseError::NotHex64 { .. })));
    }
}

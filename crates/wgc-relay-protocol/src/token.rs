use std::fmt;

use crate::error::TokenParseError;

pub const TOKEN_LEN: usize = 32;

/// Proves that a loopback connection comes from the relay this launch started.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayToken([u8; TOKEN_LEN]);

impl RelayToken {
    pub fn from_bytes(bytes: [u8; TOKEN_LEN]) -> RelayToken {
        RelayToken(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; TOKEN_LEN] {
        &self.0
    }

    pub fn parse_hex(text: &str) -> Result<RelayToken, TokenParseError> {
        let digits = text.as_bytes();
        if digits.len() != TOKEN_LEN * 2 {
            return Err(TokenParseError::Length { found: digits.len(), expected: TOKEN_LEN * 2 });
        }

        let mut bytes = [0u8; TOKEN_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let high = hex_value(digits[index * 2]).ok_or(TokenParseError::NotHex { offset: index * 2 })?;
            let low = hex_value(digits[index * 2 + 1]).ok_or(TokenParseError::NotHex { offset: index * 2 + 1 })?;
            *byte = (high << 4) | low;
        }

        Ok(RelayToken(bytes))
    }

    pub fn to_hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";

        let mut text = String::with_capacity(TOKEN_LEN * 2);
        for byte in self.0 {
            text.push(DIGITS[usize::from(byte >> 4)] as char);
            text.push(DIGITS[usize::from(byte & 0xf)] as char);
        }

        text
    }

    /// Compares without an early exit, so timing does not reveal a prefix.
    pub fn matches(&self, candidate: &[u8; TOKEN_LEN]) -> bool {
        self.0.iter().zip(candidate).fold(0u8, |diff, (left, right)| diff | (left ^ right)) == 0
    }
}

impl fmt::Debug for RelayToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RelayToken(..)")
    }
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let token = RelayToken::from_bytes(std::array::from_fn(|index| (index * 37) as u8));

        assert_eq!(RelayToken::parse_hex(&token.to_hex()), Ok(token));
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert_eq!(RelayToken::parse_hex("abc"), Err(TokenParseError::Length { found: 3, expected: 64 }));
        let mut text = "0".repeat(64);
        text.replace_range(9..10, "g");
        assert_eq!(RelayToken::parse_hex(&text), Err(TokenParseError::NotHex { offset: 9 }));
    }

    #[test]
    fn matches_only_the_same_bytes() {
        let token = RelayToken::from_bytes([7; TOKEN_LEN]);
        let mut other = [7; TOKEN_LEN];
        other[TOKEN_LEN - 1] = 8;

        assert!(token.matches(&[7; TOKEN_LEN]));
        assert!(!token.matches(&other));
    }
}

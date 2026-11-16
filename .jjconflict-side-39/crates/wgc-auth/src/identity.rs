//! Account identity: the numeric SPA account id and display nickname
//!.

use serde::Deserialize;
use serde::Serialize;

/// The numeric SPA (single platform account) id -- the wire's `user`
/// (login response) / `sub` (account/info response) field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountId(u64);

impl AccountId {
    pub fn new(id: u64) -> AccountId {
        AccountId(id)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// Display name from `account/info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nickname(String);

impl Nickname {
    pub fn new(name: impl Into<String>) -> Nickname {
        Nickname(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

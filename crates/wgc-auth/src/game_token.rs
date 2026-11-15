//! Game login tokens WGC hands to a game client over its IPC pipe.
//!
//! A game logs in to its own login server with a one-time `GameToken1` that
//! WGC mints from the account session. The server answers with a
//! `GameToken2`, which the game hands back for storage and replays on the
//! next launch. Every token2 login rotates it.

use std::borrow::Cow;
use std::fmt;

use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

use crate::realm::AccountRealm;

/// The game a token1 is minted for: the `requested_for` form field. WGC takes
/// it from `<game_id>` in the install's `game_metadata/metadata.xml`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameName(Cow<'static, str>);

impl GameName {
    pub const WOWS: GameName = GameName(Cow::Borrowed("wows"));

    pub fn new(name: impl Into<String>) -> GameName {
        GameName(Cow::Owned(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The realm-specific title a token1 is minted for: the `title_code` form
/// field. WGC takes it from `<title_id>` of the realm instance in the
/// install's `game_metadata/metadata.xml`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TitleCode(Cow<'static, str>);

impl TitleCode {
    pub fn new(code: impl Into<String>) -> TitleCode {
        TitleCode(Cow::Owned(code.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything `create/token1` needs to name the game instance on one realm.
/// The title code is realm-specific, so the realm travels with it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameTitle {
    realm: AccountRealm,
    name: GameName,
    title_code: TitleCode,
}

impl GameTitle {
    pub fn new(realm: AccountRealm, name: GameName, title_code: TitleCode) -> GameTitle {
        GameTitle { realm, name, title_code }
    }

    /// World of Warships for an account realm, per the `realms_settings`
    /// instances of the WoWs `metadata.xml`. `None` for realms the WoWs
    /// client does not serve.
    pub fn wows(realm: AccountRealm) -> Option<GameTitle> {
        let title_code = match realm {
            AccountRealm::Eu => "eu.wows",
            AccountRealm::Na => "us.wows",
            AccountRealm::Asia => "sg.wows",
            AccountRealm::Ru => return None,
        };

        Some(GameTitle { realm, name: GameName::WOWS, title_code: TitleCode(Cow::Borrowed(title_code)) })
    }

    pub fn realm(&self) -> AccountRealm {
        self.realm
    }

    pub fn name(&self) -> &GameName {
        &self.name
    }

    pub fn title_code(&self) -> &TitleCode {
        &self.title_code
    }
}

/// One-time game login token minted by `create/token1`.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct GameToken1(String);

impl GameToken1 {
    pub fn new(token: String) -> GameToken1 {
        GameToken1(token)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GameToken1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GameToken1").field(&"<redacted>").finish()
    }
}

/// Reusable game login token issued by the game's login server.
///
/// `Clone` is allowed because the launcher hands the same token to the IPC
/// reply and to the keyring.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct GameToken2(String);

impl GameToken2 {
    pub fn new(token: String) -> GameToken2 {
        GameToken2(token)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GameToken2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GameToken2").field(&"<redacted>").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_token_debug_is_redacted() {
        let token1 = format!("{:?}", GameToken1::new("ABCD1234".to_string()));
        let token2 = format!("{:?}", GameToken2::new("token2-secret".to_string()));
        assert!(!token1.contains("ABCD1234"), "debug output leaked token1: {token1}");
        assert!(!token2.contains("token2-secret"), "debug output leaked token2: {token2}");
    }
}

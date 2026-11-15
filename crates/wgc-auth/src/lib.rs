//! WGNI account authentication: secret-bearing newtypes, the account-realm
//! map, the Keccak-512 hashcash proof-of-work solver, the async WGNI HTTP
//! client, game token minting, the keyring-backed session and token2 stores,
//! and the `wgc-launch` launch-gate adapter. The protocol is
//! reverse-engineered from the WGC client and its live endpoints.

pub mod client;
pub mod error;
pub mod game_token;
pub mod identity;
mod models;
pub mod pow;
pub mod realm;
pub mod session;
pub mod store;
pub mod token;

pub use client::AccountCredentials;
pub use client::AuthClient;
pub use client::TwoFactorCode;
pub use error::AuthError;
pub use game_token::GameName;
pub use game_token::GameTitle;
pub use game_token::GameToken1;
pub use game_token::GameToken2;
pub use game_token::TitleCode;
pub use identity::AccountId;
pub use identity::Nickname;
pub use pow::PowChallenge;
pub use pow::PowError;
pub use pow::PowSolution;
pub use realm::AccountRealm;
pub use realm::ClientId;
pub use realm::RealmConfig;
pub use session::StoredSession;
pub use session::launch_token_status;
pub use store::GameTokenStore;
pub use store::SessionStore;
pub use token::AccessToken;
pub use token::ExchangeCode;
pub use token::SecretPassword;
pub use token::TokenExpiry;
pub use token::TokenPair;

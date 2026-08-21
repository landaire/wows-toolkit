//! Pure, fully-testable core for WGNI account authentication.
//!
//! Scope of this phase: secret-bearing newtypes, the account-realm map, and
//! the Keccak-512 hashcash proof-of-work solver. No network, no keyring, no
//! async -- see `.superpowers/sdd/launcher-auth-discovery/` for the protocol
//! report and design draft this crate implements against.

pub mod identity;
pub mod pow;
pub mod realm;
pub mod token;

pub use identity::AccountId;
pub use identity::Nickname;
pub use pow::PowChallenge;
pub use pow::PowError;
pub use pow::PowSolution;
pub use realm::AccountRealm;
pub use realm::ClientId;
pub use realm::RealmConfig;
pub use token::AccessToken;
pub use token::ExchangeCode;
pub use token::SecretPassword;
pub use token::TokenExpiry;
pub use token::TokenPair;

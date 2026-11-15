//! The link between `wgc-relay.exe`, which serves `\\.\pipe\WGC_IPC` inside
//! the Wine prefix, and the launcher, which answers the requests.
//!
//! The launcher listens on a loopback TCP port and passes its address and a
//! per-launch token to the relay through [`RELAY_ADDR_ENV`] and
//! [`RELAY_TOKEN_ENV`]. For every pipe connection the relay opens one TCP
//! connection, sends the raw token bytes, then forwards each pipe message as a
//! frame and writes the answering frame back to the pipe.

mod env;
mod error;
mod frame;
mod install;
mod token;

pub use env::LAUNCH_MARKER_ENV;
pub use env::RELAY_ADDR_ENV;
pub use env::RELAY_TOKEN_ENV;
pub use error::FrameError;
pub use error::TokenParseError;
pub use frame::FRAME_HEADER_LEN;
pub use frame::MAX_MESSAGE_LEN;
pub use frame::encode_frame_header;
pub use frame::parse_frame_header;
pub use frame::read_frame;
pub use frame::write_frame;
pub use install::GAME_CENTER_DIR;
pub use token::RelayToken;
pub use token::TOKEN_LEN;

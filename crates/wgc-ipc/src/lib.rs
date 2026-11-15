//! The WGC side of the Game Center IPC protocol, which `wgc_api.dll` in World
//! of Warships speaks over `\\.\pipe\WGC_IPC`: the message codec, typed
//! command models, the [`IpcResponder`] the launcher implements, the server
//! loop, the loopback listener `wgc-relay.exe` connects to, and Wine prefix
//! preparation.

mod account;
mod answer;
mod buck_resources;
pub mod command;
mod error;
pub mod message;
mod prefix;
mod relay;
mod responder;
mod server;
mod wire;

pub use account::AccountEvent;
pub use account::AccountObserver;
pub use account::AccountResponder;
pub use error::CodecError;
pub use error::IpcError;
pub use error::PrefixError;
pub use message::Message;
pub use prefix::PreparedPrefix;
pub use prefix::RELAY_DIR_ENV;
pub use prefix::RelayBinaries;
pub use prefix::RelayBinary;
pub use prefix::prepare_prefix;
pub use relay::RelayListener;
pub use responder::BoxFuture;
pub use responder::GameLanguage;
pub use responder::IpcResponder;
pub use responder::LoginInfo;
pub use responder::Token1Failure;
pub use responder::Token1Grant;
pub use responder::Token2Grant;
pub use server::handle_message;
pub use server::serve_stream;
pub use wgc_relay_protocol::GAME_CENTER_DIR;
pub use wgc_relay_protocol::LAUNCH_MARKER_ENV;

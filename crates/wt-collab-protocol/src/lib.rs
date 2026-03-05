//! Wire protocol types for WoWs Toolkit collaborative sessions.
//!
//! This crate contains all serializable types, constants, framing helpers,
//! stream I/O, token encoding, and validation logic for the collab protocol.
//! It is usable from both the desktop app and the WASM web client.

pub mod protocol;
pub mod types;
pub mod validation;

// Re-export key items at crate root for convenience.
pub use protocol::*;
pub use types::Annotation;
pub use types::PaintTool;
pub use validation::ValidationError;
pub use validation::validate_annotation;
pub use validation::validate_frame_commands_count;
pub use validation::validate_peer_message;
pub use validation::validate_replay_info;

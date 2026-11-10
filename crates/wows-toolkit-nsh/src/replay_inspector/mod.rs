//! The Replay Inspector tab's native half: the open-replay documents, the
//! settings and game data they are read with, and the components and
//! services `ui/replay_inspector.tsx` lays out.

pub mod auto_export;
pub mod browser;
pub mod columns;
pub mod constants;
pub mod document;
pub mod error;
pub mod landed;
pub mod service;
pub mod settings;
pub mod summary;
pub mod table;
#[cfg(feature = "test-support")]
pub mod test_support;

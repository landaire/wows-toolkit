//! Wrapper types that bridge data sources to the VFS abstraction.

/// In-memory PKG source. Pure Rust, works in any environment (wasm, embedded).
pub mod bytes;
/// Memory-mapped PKG source. Native only (uses memmap2).
#[cfg(feature = "vfs-mmap")]
pub mod mmap;

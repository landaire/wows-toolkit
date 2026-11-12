//! The query bar's framework-neutral half.
//!
//! Turning a parsed query into the pills a bar shows, what each pill reads
//! as, what a caret position may be completed to, and how a seeded query
//! resolves its bare ids. None of it draws anything, so both front ends can
//! build their own bar over one implementation of the rules.

pub mod label;
pub mod seed;
pub mod select;
pub mod suggest;
pub mod tokens;

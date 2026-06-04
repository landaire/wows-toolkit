//! Per-version consumable id -> name tables.
//!
//! Recovered by static analysis of the obfuscated consumable-constants module in
//! each shipped client build (the `ConsumablesTypes` id ordering combined with
//! `ConsumableNamesMap`). Keyed on friendly base version (major.minor.patch): a
//! replay resolves to the latest layout whose version it `is_at_least`.
//!
//! The data lives in `crates/wowsunpack/consumable_layouts.toml` (checked in) and
//! is turned into the lookup table below at compile time by `build.rs`. To update
//! the data, run `scripts/extract_consumable_ids.py`, which rewrites that TOML.

use crate::data::Version;

include!(concat!(env!("OUT_DIR"), "/consumable_versions.rs"));

//! Typed access to per-build game data for the replay parsing layer.
//!
//! Parsing a replay needs data that ships with the game build the replay was
//! recorded on: entity definitions for the packet parser, game constants for
//! the decoder. Callers hold that data in very different ways (a live
//! install, a dump archive, a per-build cache directory), so the parsing
//! layer asks a [`GameDataContext`] for typed values and stays out of the
//! sourcing business.
//!
//! The layers are opt-in:
//! - [`FnContext`] adapts two closures with no further behavior; a one-shot
//!   tool that parses a single replay pays nothing beyond its own load.
//! - [`CachedContext`] wraps any inner context and memoizes per build,
//!   including negative results, so directory-scale runs pay each build's
//!   load cost once.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

use crate::game_constants::GameConstants;

/// Why a context could not produce data for a build.
#[derive(Debug, Clone, thiserror::Error)]
#[error("failed to load {what} for {}: {message}", version.to_path())]
pub struct GameDataContextError {
    pub what: &'static str,
    pub version: Version,
    pub message: Arc<str>,
}

impl GameDataContextError {
    pub fn new(what: &'static str, version: Version, message: impl std::fmt::Display) -> Self {
        Self { what, version, message: message.to_string().into() }
    }
}

pub trait GameDataContext {
    /// Entity definitions for the build the replay was recorded with.
    fn entity_specs(&self, version: &Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError>;

    /// Game constants for the build, with any caller-configured overrides
    /// (e.g. a constants.json) already merged.
    fn game_constants(&self, version: &Version) -> Result<Arc<GameConstants>, GameDataContextError>;
}

/// A [`GameDataContext`] built from two closures. Every call loads fresh;
/// wrap in [`CachedContext`] when parsing more than one replay.
pub struct FnContext<S, C>
where
    S: Fn(&Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError>,
    C: Fn(&Version) -> Result<Arc<GameConstants>, GameDataContextError>,
{
    specs: S,
    constants: C,
}

impl<S, C> FnContext<S, C>
where
    S: Fn(&Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError>,
    C: Fn(&Version) -> Result<Arc<GameConstants>, GameDataContextError>,
{
    pub fn new(specs: S, constants: C) -> Self {
        Self { specs, constants }
    }
}

impl<S, C> GameDataContext for FnContext<S, C>
where
    S: Fn(&Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError>,
    C: Fn(&Version) -> Result<Arc<GameConstants>, GameDataContextError>,
{
    fn entity_specs(&self, version: &Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError> {
        (self.specs)(version)
    }

    fn game_constants(&self, version: &Version) -> Result<Arc<GameConstants>, GameDataContextError> {
        (self.constants)(version)
    }
}

type Build = Option<u32>;
type Cache<T> = Mutex<HashMap<Build, Result<T, GameDataContextError>>>;

/// Per-build memoization over any inner context. Failures are cached too, so
/// a build whose data is absent is looked for once, not once per replay.
pub struct CachedContext<T: GameDataContext> {
    inner: T,
    specs: Cache<Arc<Vec<EntitySpec>>>,
    constants: Cache<Arc<GameConstants>>,
}

impl<T: GameDataContext> CachedContext<T> {
    pub fn new(inner: T) -> Self {
        Self { inner, specs: Mutex::new(HashMap::new()), constants: Mutex::new(HashMap::new()) }
    }
}

impl<T: GameDataContext> GameDataContext for CachedContext<T> {
    fn entity_specs(&self, version: &Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError> {
        let mut cache = self.specs.lock().expect("specs cache poisoned");
        cache.entry(version.build_number()).or_insert_with(|| self.inner.entity_specs(version)).clone()
    }

    fn game_constants(&self, version: &Version) -> Result<Arc<GameConstants>, GameDataContextError> {
        let mut cache = self.constants.lock().expect("constants cache poisoned");
        cache.entry(version.build_number()).or_insert_with(|| self.inner.game_constants(version)).clone()
    }
}

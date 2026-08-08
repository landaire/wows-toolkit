//! Typed access to per-build game data for the replay parsing layer.
//!
//! Parsing a replay needs data that ships with the game build the replay was
//! recorded on: entity definitions for the packet parser, game constants for
//! the decoder. Callers hold that data in very different ways (a live
//! install, a dump archive, a per-build cache directory), so the parsing
//! layer asks a [`GameDataContext`] for typed values and stays out of the
//! sourcing business.
//!
//! The layering contract:
//! - Parsing APIs (`Parser`, `BattleWorld`, the decoder) never take a
//!   context. They borrow pre-resolved data, so a consumer with its own
//!   per-build state pays nothing for this module.
//! - A context returns shared handles (`Arc`) out of whatever store the
//!   consumer runs; it imposes no caching policy of its own.
//! - [`CachedContext`] is the opt-in policy for consumers with no store of
//!   their own (one-shot and batch tools). It wraps loaders, never caches:
//!   wrapping an already-caching context double-caches with conflicting
//!   eviction.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
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

    /// Full metadata provider (game params, localization, entity specs) for
    /// the build. A context that cannot source game params returns an error
    /// saying so; callers that only drive the packet parser should ask for
    /// [`GameDataContext::entity_specs`] instead, which is far cheaper to
    /// load.
    fn metadata_provider(&self, version: &Version) -> Result<Arc<GameMetadataProvider>, GameDataContextError>;
}

type Cache<T> = Mutex<HashMap<Version, Result<T, GameDataContextError>>>;

/// Per-version memoization over any inner context. Failures are cached too,
/// so a build whose data is absent is looked for once, not once per replay.
/// That makes this layer a fit for batch runs and short-lived tools; a
/// long-lived process that wants to retry transient failures should hold its
/// own cache policy instead.
pub struct CachedContext<T: GameDataContext> {
    inner: T,
    specs: Cache<Arc<Vec<EntitySpec>>>,
    constants: Cache<Arc<GameConstants>>,
    providers: Cache<Arc<GameMetadataProvider>>,
}

impl<T: GameDataContext> CachedContext<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            specs: Mutex::new(HashMap::new()),
            constants: Mutex::new(HashMap::new()),
            providers: Mutex::new(HashMap::new()),
        }
    }
}

impl<T: GameDataContext> GameDataContext for CachedContext<T> {
    fn entity_specs(&self, version: &Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError> {
        let mut cache = self.specs.lock().expect("specs cache poisoned");
        cache.entry(*version).or_insert_with(|| self.inner.entity_specs(version)).clone()
    }

    fn game_constants(&self, version: &Version) -> Result<Arc<GameConstants>, GameDataContextError> {
        let mut cache = self.constants.lock().expect("constants cache poisoned");
        cache.entry(*version).or_insert_with(|| self.inner.game_constants(version)).clone()
    }

    fn metadata_provider(&self, version: &Version) -> Result<Arc<GameMetadataProvider>, GameDataContextError> {
        let mut cache = self.providers.lock().expect("providers cache poisoned");
        cache.entry(*version).or_insert_with(|| self.inner.metadata_provider(version)).clone()
    }
}

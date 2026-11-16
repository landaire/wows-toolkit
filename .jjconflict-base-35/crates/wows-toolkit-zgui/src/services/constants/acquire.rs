//! The order a build's constants are had in: the cache, then GitHub, then generation from the
//! installed client's scripts. Synchronous and free of the UI; the constants worker runs it.

use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use wows_toolkit_viewmodel::index_rows::ConstantsFit;
use wows_toolkit_viewmodel::index_rows::constants_fit;
use wowsunpack::data::Version;

/// A build whose constants are wanted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub build: u32,
    /// The release its replays name, as `major.minor.patch`. Another region's build of the same
    /// release shares its constants.
    pub release: Option<String>,
}

/// Where a build's current constants came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// The cache already held them.
    Cached,
    /// GitHub published them.
    GitHub,
    /// They were generated from the installed client's scripts, which took `took`.
    Generated { took: Duration },
}

/// Why GitHub's copy was not used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unpublished {
    #[error("GitHub could not be asked: {reason}")]
    Unreachable { reason: String },
    #[error("GitHub only has the constants of the older build {published}")]
    Stale { published: u32 },
}

/// Why generating constants from the client failed.
#[derive(Debug, thiserror::Error)]
pub enum GenerationError {
    #[error(transparent)]
    Extraction(#[from] wowsdeob::constants::ConstantsError),
    #[error("generation panicked: {message}")]
    Panicked { message: String },
}

/// What decoding uses when no current constants could be had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// The file already cached for the build stays.
    Kept,
    /// The nearest older build GitHub published, cached since nothing was.
    Published { build: u32 },
    /// The constants built into the app.
    Embedded,
}

/// Why no current constants were had.
#[derive(Debug, thiserror::Error)]
pub enum Missed {
    #[error("{unpublished}, and the game install has no client for build {build}")]
    NoClient { build: u32, unpublished: Unpublished },
    #[error("{error}")]
    Generation {
        unpublished: Unpublished,
        #[source]
        error: GenerationError,
    },
    #[error("the constants could not be saved: {reason}")]
    Store { reason: String },
    #[error("the constants worker stopped: {reason}")]
    WorkerStopped { reason: String },
}

/// What became of one build's constants.
#[derive(Debug)]
pub enum Outcome {
    /// Constants that fit the build are cached. `unpublished` says why GitHub's were not used when
    /// they were generated.
    Current { provenance: Provenance, unpublished: Option<Unpublished> },
    /// None that fit could be had.
    Fallback { fallback: Fallback, missed: Missed },
}

impl Outcome {
    pub fn provenance(&self) -> Option<Provenance> {
        match self {
            Self::Current { provenance, .. } => Some(*provenance),
            Self::Fallback { .. } => None,
        }
    }
}

/// What [`acquire`] reports while it works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Generation from the client has started.
    Generating,
}

/// GitHub's copy of a build's constants, published under `build`.
pub struct Published {
    pub constants: Value,
    pub build: u32,
}

/// What [`acquire`] reads and writes.
pub trait Sources {
    /// A client to generate constants from.
    type Client;

    /// The constants cached for `build`, when a file that parses is there.
    fn cached(&self, build: u32) -> Option<Value>;

    /// GitHub's constants for `wanted`, which may be an older build's. `Err` names why GitHub
    /// could not be asked.
    fn fetch(&mut self, wanted: &Wanted) -> Result<Published, String>;

    /// The installed client of `build`, when there is one to generate from.
    fn client(&self, build: u32) -> Option<Self::Client>;

    /// Generates `client`'s `constants.json`.
    fn generate(&mut self, client: &Self::Client) -> Result<Vec<u8>, GenerationError>;

    /// Caches `bytes` as `build`'s constants.
    fn store(&mut self, build: u32, bytes: &[u8]) -> Result<(), String>;
}

/// Whether `constants` fit `wanted`: they name its build, or the same release. The replay loader
/// judges the file it decodes with the same way.
pub fn is_current(constants: &Value, wanted: &Wanted) -> bool {
    let version = wanted.release.as_deref().and_then(|release| {
        let (version, patch) = release.rsplit_once('.')?;
        Version::from_constants_block(version, patch.parse().ok()?, Some(wanted.build))
    });

    constants_fit(constants, wanted.build, version) == ConstantsFit::Matched
}

/// Has `wanted`'s constants cached: those already cached when they fit, else GitHub's when they
/// fit, else ones generated from the installed client. When none of those can be had, an older
/// build's published constants are cached only if nothing is, and decoding falls back to what is
/// cached or to the built-in constants.
pub fn acquire<S: Sources>(wanted: &Wanted, sources: &mut S, mut step: impl FnMut(Step)) -> Outcome {
    let build = wanted.build;
    let cached = sources.cached(build);
    if cached.as_ref().is_some_and(|constants| is_current(constants, wanted)) {
        return Outcome::Current { provenance: Provenance::Cached, unpublished: None };
    }

    let (unpublished, stale) = match sources.fetch(wanted) {
        Ok(published) if is_current(&published.constants, wanted) => {
            return match store_value(sources, build, &published.constants) {
                Ok(()) => Outcome::Current { provenance: Provenance::GitHub, unpublished: None },
                Err(reason) => {
                    Outcome::Fallback { fallback: kept_or_embedded(cached.is_some()), missed: Missed::Store { reason } }
                }
            };
        }
        Ok(published) => (Unpublished::Stale { published: published.build }, Some(published)),
        Err(reason) => (Unpublished::Unreachable { reason }, None),
    };

    let missed = match sources.client(build) {
        None => Missed::NoClient { build, unpublished },
        Some(client) => {
            step(Step::Generating);
            let started = Instant::now();
            match sources.generate(&client) {
                Ok(bytes) => {
                    let took = started.elapsed();
                    tracing::info!(build, took_ms = took.as_millis(), "constants: generated from the client");
                    match sources.store(build, &bytes) {
                        Ok(()) => {
                            return Outcome::Current {
                                provenance: Provenance::Generated { took },
                                unpublished: Some(unpublished),
                            };
                        }
                        Err(reason) => Missed::Store { reason },
                    }
                }
                Err(error) => {
                    tracing::warn!(build, took_ms = started.elapsed().as_millis(), %error, "constants: generation failed");
                    Missed::Generation { unpublished, error }
                }
            }
        }
    };

    let fallback = match stale {
        _ if cached.is_some() => Fallback::Kept,
        Some(published) => match store_value(sources, build, &published.constants) {
            Ok(()) => Fallback::Published { build: published.build },
            Err(reason) => {
                tracing::warn!(build, %reason, "constants: the older published constants could not be saved");
                Fallback::Embedded
            }
        },
        None => Fallback::Embedded,
    };

    Outcome::Fallback { fallback, missed }
}

/// What decoding uses when nothing new was cached.
pub fn kept_or_embedded(cached: bool) -> Fallback {
    if cached { Fallback::Kept } else { Fallback::Embedded }
}

fn store_value<S: Sources>(sources: &mut S, build: u32, constants: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(constants).map_err(|err| err.to_string())?;

    sources.store(build, &bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    const BUILD: u32 = 13_533_378;
    const OLDER: u32 = 13_357_625;

    fn constants_of(build: u32, version: &str) -> Value {
        json!({ "VERSION": { "VERSION": version, "BUILD": build, "PATCH": 0.0 } })
    }

    fn wanted() -> Wanted {
        Wanted { build: BUILD, release: Some("15.10.0".to_owned()) }
    }

    /// In-memory sources that record what was asked of them.
    struct Fake {
        cache: BTreeMap<u32, Value>,
        github: Result<Value, String>,
        has_client: bool,
        generated: Result<Value, String>,
        fetches: usize,
        generations: usize,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                cache: BTreeMap::new(),
                github: Err("offline".to_owned()),
                has_client: true,
                generated: Ok(constants_of(BUILD, "15.10")),
                fetches: 0,
                generations: 0,
            }
        }
    }

    impl Sources for Fake {
        type Client = ();

        fn cached(&self, build: u32) -> Option<Value> {
            self.cache.get(&build).cloned()
        }

        fn fetch(&mut self, _wanted: &Wanted) -> Result<Published, String> {
            self.fetches += 1;
            let constants = self.github.clone()?;
            let build =
                constants["VERSION"]["BUILD"].as_u64().and_then(|build| u32::try_from(build).ok()).expect("a build");

            Ok(Published { constants, build })
        }

        fn client(&self, _build: u32) -> Option<()> {
            self.has_client.then_some(())
        }

        fn generate(&mut self, _client: &()) -> Result<Vec<u8>, GenerationError> {
            self.generations += 1;
            match &self.generated {
                Ok(constants) => Ok(serde_json::to_vec(constants).expect("serializes")),
                Err(path) => Err(GenerationError::Extraction(wowsdeob::constants::ConstantsError::NoBuild {
                    path: PathBuf::from(path),
                })),
            }
        }

        fn store(&mut self, build: u32, bytes: &[u8]) -> Result<(), String> {
            self.cache.insert(build, serde_json::from_slice(bytes).map_err(|err| err.to_string())?);
            Ok(())
        }
    }

    fn acquire_counting(fake: &mut Fake) -> (Outcome, Vec<Step>) {
        let mut steps = Vec::new();
        let outcome = acquire(&wanted(), fake, |step| steps.push(step));

        (outcome, steps)
    }

    #[test]
    fn a_cached_file_that_fits_asks_nothing_else() {
        let mut fake = Fake::new();
        fake.cache.insert(BUILD, constants_of(BUILD, "15.10"));

        let (outcome, steps) = acquire_counting(&mut fake);

        assert_eq!(outcome.provenance(), Some(Provenance::Cached));
        assert_eq!((fake.fetches, fake.generations), (0, 0));
        assert!(steps.is_empty());
    }

    #[test]
    fn another_regions_build_of_the_same_release_fits() {
        let mut fake = Fake::new();
        fake.cache.insert(BUILD, constants_of(BUILD + 1, "15.10"));

        let (outcome, _) = acquire_counting(&mut fake);

        assert_eq!(outcome.provenance(), Some(Provenance::Cached));
    }

    #[test]
    fn a_current_github_copy_is_cached_without_generating() {
        let mut fake = Fake::new();
        fake.cache.insert(BUILD, constants_of(OLDER, "15.9"));
        fake.github = Ok(constants_of(BUILD, "15.10"));

        let (outcome, steps) = acquire_counting(&mut fake);

        assert_eq!(outcome.provenance(), Some(Provenance::GitHub));
        assert_eq!(fake.cache.get(&BUILD), Some(&constants_of(BUILD, "15.10")));
        assert_eq!(fake.generations, 0);
        assert!(steps.is_empty());
    }

    #[test]
    fn a_stale_github_copy_leads_to_generation() {
        let mut fake = Fake::new();
        fake.github = Ok(constants_of(OLDER, "15.9"));

        let (outcome, steps) = acquire_counting(&mut fake);

        let Outcome::Current { provenance: Provenance::Generated { .. }, unpublished } = outcome else {
            panic!("expected generated constants, got {outcome:?}");
        };
        assert_eq!(unpublished, Some(Unpublished::Stale { published: OLDER }));
        assert_eq!(steps, vec![Step::Generating]);
        assert_eq!(fake.cache.get(&BUILD), Some(&constants_of(BUILD, "15.10")));
    }

    #[test]
    fn a_failed_fetch_leads_to_generation() {
        let mut fake = Fake::new();

        let (outcome, steps) = acquire_counting(&mut fake);

        let Outcome::Current { provenance: Provenance::Generated { .. }, unpublished } = outcome else {
            panic!("expected generated constants, got {outcome:?}");
        };
        assert_eq!(unpublished, Some(Unpublished::Unreachable { reason: "offline".to_owned() }));
        assert_eq!(steps, vec![Step::Generating]);
    }

    #[test]
    fn a_second_request_after_generation_reads_the_cache() {
        let mut fake = Fake::new();

        acquire_counting(&mut fake);
        let (outcome, steps) = acquire_counting(&mut fake);

        assert_eq!(outcome.provenance(), Some(Provenance::Cached));
        assert_eq!((fake.fetches, fake.generations), (1, 1));
        assert!(steps.is_empty());
    }

    #[test]
    fn a_failed_generation_falls_back_to_the_embedded_constants() {
        let mut fake = Fake::new();
        fake.generated = Err("/nowhere".to_owned());

        let (outcome, steps) = acquire_counting(&mut fake);

        let Outcome::Fallback { fallback, missed } = outcome else {
            panic!("expected a fallback, got {outcome:?}");
        };
        assert_eq!(fallback, Fallback::Embedded);
        assert!(matches!(missed, Missed::Generation { .. }));
        assert_eq!(missed.to_string(), "no game build under /nowhere has res/scripts.zip");
        assert_eq!(steps, vec![Step::Generating]);
        assert!(fake.cache.is_empty());
    }

    #[test]
    fn a_failed_generation_keeps_what_is_cached() {
        let mut fake = Fake::new();
        let imported = constants_of(OLDER, "15.9");
        fake.cache.insert(BUILD, imported.clone());
        fake.github = Ok(constants_of(OLDER - 1, "15.9"));
        fake.generated = Err("/nowhere".to_owned());

        let (outcome, _) = acquire_counting(&mut fake);

        assert!(matches!(outcome, Outcome::Fallback { fallback: Fallback::Kept, .. }));
        assert_eq!(fake.cache.get(&BUILD), Some(&imported));
    }

    #[test]
    fn without_a_client_the_older_published_copy_is_cached() {
        let mut fake = Fake::new();
        fake.github = Ok(constants_of(OLDER, "15.9"));
        fake.has_client = false;

        let (outcome, steps) = acquire_counting(&mut fake);

        let Outcome::Fallback { fallback, missed } = outcome else {
            panic!("expected a fallback, got {outcome:?}");
        };
        assert_eq!(fallback, Fallback::Published { build: OLDER });
        assert!(matches!(missed, Missed::NoClient { .. }));
        assert!(steps.is_empty());
        assert_eq!(fake.cache.get(&BUILD), Some(&constants_of(OLDER, "15.9")));
    }
}

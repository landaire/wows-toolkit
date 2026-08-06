//! Turn a probe result, user preferences, and a render mode into the
//! configuration to launch with.
//!
//! Everything here is pure so the fallback can be tested without a GPU.

use std::cmp::Ordering;

use thiserror::Error;

use super::probe::AdapterDescription;
use super::probe::AdapterRecord;
use super::probe::IcdManifestPath;

/// A render configuration to try, in order.
///
/// Ordered from the most desirable to the one most likely to start at all. A
/// launch that fails to present a frame drops to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderMode {
    /// Preferred adapter, Vulkan, other vendors' ICDs never loaded.
    PinnedVulkan,
    /// Preferred adapter on D3D12. Pinning is weaker here: there is no loader
    /// filter for D3D, so DXGI enumeration still loads the other vendor's
    /// user-mode driver and only the device choice is honoured.
    PinnedDx12,
    /// The next adapter after the preferred one, in case the preferred device
    /// is the broken one.
    PinnedVulkanAlternate,
    /// Full enumeration with process mitigations applied.
    UnpinnedHardened,
    /// Full enumeration, nothing suppressed, every vendor's stack admitted.
    Unpinned,
    /// WARP. Microsoft-signed, loads no vendor code, always available.
    CpuRenderer,
}

impl RenderMode {
    pub const FIRST: Self = Self::PinnedVulkan;

    /// The next mode to try, or `None` at the last one.
    pub fn next(self) -> Option<Self> {
        match self {
            Self::PinnedVulkan => Some(Self::PinnedDx12),
            Self::PinnedDx12 => Some(Self::PinnedVulkanAlternate),
            Self::PinnedVulkanAlternate => Some(Self::UnpinnedHardened),
            Self::UnpinnedHardened => Some(Self::Unpinned),
            Self::Unpinned => Some(Self::CpuRenderer),
            Self::CpuRenderer => None,
        }
    }

    /// Stable token for the on-disk record.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::PinnedVulkan => "pinned-vulkan",
            Self::PinnedDx12 => "pinned-dx12",
            Self::PinnedVulkanAlternate => "pinned-vulkan-alternate",
            Self::UnpinnedHardened => "unpinned-hardened",
            Self::Unpinned => "unpinned",
            Self::CpuRenderer => "cpu-renderer",
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        [
            Self::PinnedVulkan,
            Self::PinnedDx12,
            Self::PinnedVulkanAlternate,
            Self::UnpinnedHardened,
            Self::Unpinned,
            Self::CpuRenderer,
        ]
        .into_iter()
        .find(|mode| mode.as_token() == token)
    }
}

/// Which graphics API to initialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackend {
    Vulkan,
    Dx12,
}

/// The backend set handed to wgpu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendSelection {
    /// Exactly one backend, so no other vendor's stack is enumerated.
    Only(RenderBackend),
    /// The wide default set, overridable by `WGPU_BACKEND`.
    Default,
}

/// Which physical device to render on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterChoice {
    Pinned {
        adapter: AdapterDescription,
        icd: IcdManifestPath,
    },
    /// Let wgpu enumerate and let the existing selector rank the result.
    Enumerate,
    /// WARP, selected by device type rather than by name.
    Cpu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderConfig {
    pub mode: RenderMode,
    pub backends: BackendSelection,
    pub adapter: AdapterChoice,
}

/// Command line render overrides.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    pub cpu_renderer: bool,
    pub gpu_safe_mode: bool,
    pub gpu_adapter: Option<String>,
}

impl Overrides {
    /// Whether the command line asked for a specific render configuration.
    pub fn are_set(&self) -> bool {
        self.cpu_renderer || self.gpu_safe_mode || self.gpu_adapter.is_some()
    }
}

#[derive(Debug, Error)]
pub enum SelectError {
    #[error("no display adapter matches {requested}. Available: {}", describe(available))]
    NoAdapterMatch { requested: String, available: Vec<AdapterDescription> },
    #[error("{requested} matches more than one display adapter: {}", describe(candidates))]
    AmbiguousAdapter { requested: String, candidates: Vec<AdapterDescription> },
}

/// The adapter names an error carries, for its message. The fields stay
/// structured; this only decides how they read.
fn describe(adapters: &[AdapterDescription]) -> String {
    if adapters.is_empty() {
        return "none".to_string();
    }
    adapters.iter().map(AdapterDescription::as_str).collect::<Vec<_>>().join(", ")
}

/// Rank adapters by desirability.
///
/// Larger reported memory wins. An adapter whose driver published no size sorts
/// after every adapter that did: an unknown size could be anything, and
/// promoting it would hand the render session to a device nothing is known
/// about. Ties break on description so the order is deterministic across runs.
fn preference_order(a: &AdapterRecord, b: &AdapterRecord) -> Ordering {
    match (a.memory, b.memory) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| a.description.cmp(&b.description))
}

/// Adapters ordered by preference, honouring a persisted choice when it still
/// names a present adapter.
fn ranked<'a>(adapters: &'a [AdapterRecord], preferred: Option<&AdapterDescription>) -> Vec<&'a AdapterRecord> {
    let mut ranked: Vec<&AdapterRecord> = adapters.iter().collect();
    ranked.sort_by(|a, b| preference_order(a, b));
    // A persisted preference is a preference, not a cache: it only reorders
    // adapters the probe found this launch, so a swapped GPU cannot strand it.
    if let Some(preferred) = preferred
        && let Some(position) = ranked.iter().position(|record| &record.description == preferred)
    {
        let chosen = ranked.remove(position);
        ranked.insert(0, chosen);
    }
    ranked
}

fn pin(record: &AdapterRecord, backend: RenderBackend, mode: RenderMode) -> RenderConfig {
    RenderConfig {
        mode,
        backends: BackendSelection::Only(backend),
        adapter: AdapterChoice::Pinned { adapter: record.description.clone(), icd: record.icd.clone() },
    }
}

fn cpu_config() -> RenderConfig {
    RenderConfig {
        mode: RenderMode::CpuRenderer,
        backends: BackendSelection::Only(RenderBackend::Dx12),
        adapter: AdapterChoice::Cpu,
    }
}

fn unpinned_config(mode: RenderMode) -> RenderConfig {
    RenderConfig { mode, backends: BackendSelection::Default, adapter: AdapterChoice::Enumerate }
}

/// Find the single adapter whose description contains `requested`.
fn match_requested<'a>(adapters: &'a [AdapterRecord], requested: &str) -> Result<&'a AdapterRecord, SelectError> {
    let needle = requested.to_lowercase();
    let matches: Vec<&AdapterRecord> =
        adapters.iter().filter(|record| record.description.as_str().to_lowercase().contains(&needle)).collect();
    match matches.as_slice() {
        [single] => Ok(single),
        [] => Err(SelectError::NoAdapterMatch {
            requested: requested.to_string(),
            available: adapters.iter().map(|record| record.description.clone()).collect(),
        }),
        many => Err(SelectError::AmbiguousAdapter {
            requested: requested.to_string(),
            candidates: many.iter().map(|record| record.description.clone()).collect(),
        }),
    }
}

/// Resolve the configuration to launch with.
///
/// A mode that cannot be satisfied by the adapters actually present is skipped
/// rather than attempted, so a single-GPU machine does not burn a launch on
/// `PinnedVulkanAlternate` and a machine with no probed adapters at all lands
/// directly on full enumeration.
pub fn resolve(
    adapters: &[AdapterRecord],
    overrides: &Overrides,
    preferred: Option<&AdapterDescription>,
    mode: RenderMode,
) -> Result<RenderConfig, SelectError> {
    if overrides.cpu_renderer {
        return Ok(cpu_config());
    }
    if let Some(requested) = &overrides.gpu_adapter {
        let record = match_requested(adapters, requested)?;
        return Ok(pin(record, RenderBackend::Vulkan, mode));
    }
    if overrides.gpu_safe_mode {
        return Ok(unpinned_config(RenderMode::Unpinned));
    }

    let ranked = ranked(adapters, preferred);
    let mut mode = mode;
    loop {
        let satisfied = match mode {
            RenderMode::PinnedVulkan => ranked.first().map(|record| pin(record, RenderBackend::Vulkan, mode)),
            RenderMode::PinnedDx12 => ranked.first().map(|record| pin(record, RenderBackend::Dx12, mode)),
            RenderMode::PinnedVulkanAlternate => ranked.get(1).map(|record| pin(record, RenderBackend::Vulkan, mode)),
            RenderMode::UnpinnedHardened | RenderMode::Unpinned => Some(unpinned_config(mode)),
            RenderMode::CpuRenderer => Some(cpu_config()),
        };
        if let Some(config) = satisfied {
            return Ok(config);
        }
        // The terminal mode is always satisfiable, so this cannot run out.
        mode = mode.next().unwrap_or(RenderMode::CpuRenderer);
    }
}

#[cfg(test)]
mod tests {
    use super::super::probe::VideoMemoryBytes;
    use super::*;

    fn record(description: &str, icd: &str, memory: Option<u64>) -> AdapterRecord {
        AdapterRecord {
            description: AdapterDescription::new(description),
            icd: IcdManifestPath::new(icd),
            discarded_icds: Vec::new(),
            memory: memory.map(|bytes| VideoMemoryBytes::new(bytes).expect("test sizes are non-zero")),
        }
    }

    /// The development machine's actual configuration.
    fn hybrid() -> Vec<AdapterRecord> {
        vec![
            record("AMD Radeon(TM) Graphics", r"C:\amd\amd-vulkan64.json", Some(536_870_912)),
            record("NVIDIA GeForce RTX 5080", r"C:\nvidia\nv-vk64.json", Some(17_094_934_528)),
        ]
    }

    fn pinned_name(config: &RenderConfig) -> &str {
        match &config.adapter {
            AdapterChoice::Pinned { adapter, .. } => adapter.as_str(),
            other => panic!("expected a pinned adapter, got {other:?}"),
        }
    }

    #[test]
    fn every_mode_is_reachable_and_the_list_terminates() {
        let mut mode = RenderMode::FIRST;
        let mut visited = vec![mode];
        while let Some(next) = mode.next() {
            mode = next;
            visited.push(mode);
            assert!(visited.len() <= 6, "mode list failed to terminate: {visited:?}");
        }

        assert_eq!(visited.last(), Some(&RenderMode::CpuRenderer));
        assert_eq!(visited.len(), 6);
    }

    #[test]
    fn mode_tokens_round_trip() {
        let mut mode = Some(RenderMode::FIRST);
        while let Some(current) = mode {
            assert_eq!(RenderMode::from_token(current.as_token()), Some(current));
            mode = current.next();
        }
        assert_eq!(RenderMode::from_token("nonsense"), None);
    }

    #[test]
    fn largest_reported_memory_wins_by_default() {
        let config = resolve(&hybrid(), &Overrides::default(), None, RenderMode::FIRST).unwrap();

        assert_eq!(pinned_name(&config), "NVIDIA GeForce RTX 5080");
        assert_eq!(config.backends, BackendSelection::Only(RenderBackend::Vulkan));
    }

    #[test]
    fn adapters_with_unknown_memory_rank_below_every_known_adapter() {
        let adapters = vec![
            record("Mystery Display Adapter", r"C:\mystery\vk.json", None),
            record("AMD Radeon(TM) Graphics", r"C:\amd\amd-vulkan64.json", Some(536_870_912)),
        ];

        let config = resolve(&adapters, &Overrides::default(), None, RenderMode::FIRST).unwrap();

        assert_eq!(pinned_name(&config), "AMD Radeon(TM) Graphics");
    }

    #[test]
    fn a_persisted_preference_outranks_reported_memory() {
        let preferred = AdapterDescription::new("AMD Radeon(TM) Graphics");

        let config = resolve(&hybrid(), &Overrides::default(), Some(&preferred), RenderMode::FIRST).unwrap();

        assert_eq!(pinned_name(&config), "AMD Radeon(TM) Graphics");
    }

    #[test]
    fn a_preference_naming_an_absent_adapter_falls_through_to_memory() {
        let preferred = AdapterDescription::new("NVIDIA GeForce GTX 1080");

        let config = resolve(&hybrid(), &Overrides::default(), Some(&preferred), RenderMode::FIRST).unwrap();

        assert_eq!(pinned_name(&config), "NVIDIA GeForce RTX 5080");
    }

    #[test]
    fn dx12_mode_pins_the_same_adapter_on_the_other_backend() {
        let config = resolve(&hybrid(), &Overrides::default(), None, RenderMode::PinnedDx12).unwrap();

        assert_eq!(pinned_name(&config), "NVIDIA GeForce RTX 5080");
        assert_eq!(config.backends, BackendSelection::Only(RenderBackend::Dx12));
    }

    #[test]
    fn alternate_mode_pins_the_other_adapter() {
        let config = resolve(&hybrid(), &Overrides::default(), None, RenderMode::PinnedVulkanAlternate).unwrap();

        assert_eq!(pinned_name(&config), "AMD Radeon(TM) Graphics");
    }

    #[test]
    fn alternate_mode_is_skipped_when_there_is_only_one_adapter() {
        let adapters = vec![record("NVIDIA GeForce RTX 5080", r"C:\nvidia\nv-vk64.json", Some(17_094_934_528))];

        let config = resolve(&adapters, &Overrides::default(), None, RenderMode::PinnedVulkanAlternate).unwrap();

        assert_eq!(config.mode, RenderMode::UnpinnedHardened);
        assert_eq!(config.adapter, AdapterChoice::Enumerate);
    }

    #[test]
    fn no_probed_adapters_lands_on_full_enumeration() {
        let config = resolve(&[], &Overrides::default(), None, RenderMode::FIRST).unwrap();

        assert_eq!(config.mode, RenderMode::UnpinnedHardened);
        assert_eq!(config.adapter, AdapterChoice::Enumerate);
        assert_eq!(config.backends, BackendSelection::Default);
    }

    #[test]
    fn terminal_mode_selects_warp() {
        let config = resolve(&hybrid(), &Overrides::default(), None, RenderMode::CpuRenderer).unwrap();

        assert_eq!(config.adapter, AdapterChoice::Cpu);
        assert_eq!(config.backends, BackendSelection::Only(RenderBackend::Dx12));
    }

    #[test]
    fn cpu_override_wins_over_every_mode() {
        let overrides = Overrides { cpu_renderer: true, ..Overrides::default() };

        let config = resolve(&hybrid(), &overrides, None, RenderMode::FIRST).unwrap();

        assert_eq!(config.adapter, AdapterChoice::Cpu);
        assert_eq!(config.mode, RenderMode::CpuRenderer);
    }

    #[test]
    fn safe_mode_override_enumerates_everything() {
        let overrides = Overrides { gpu_safe_mode: true, ..Overrides::default() };

        let config = resolve(&hybrid(), &overrides, None, RenderMode::FIRST).unwrap();

        assert_eq!(config.adapter, AdapterChoice::Enumerate);
        assert_eq!(config.mode, RenderMode::Unpinned);
    }

    #[test]
    fn adapter_override_matches_a_case_insensitive_substring() {
        let overrides = Overrides { gpu_adapter: Some("nvidia".into()), ..Overrides::default() };

        let config = resolve(&hybrid(), &overrides, None, RenderMode::FIRST).unwrap();

        assert_eq!(pinned_name(&config), "NVIDIA GeForce RTX 5080");
    }

    #[test]
    fn an_unmatched_adapter_override_reports_what_was_available() {
        let overrides = Overrides { gpu_adapter: Some("intel".into()), ..Overrides::default() };

        let error = resolve(&hybrid(), &overrides, None, RenderMode::FIRST).unwrap_err();

        match error {
            SelectError::NoAdapterMatch { requested, available } => {
                assert_eq!(requested, "intel");
                assert_eq!(available.len(), 2);
            }
            other => panic!("expected NoAdapterMatch, got {other:?}"),
        }
    }

    #[test]
    fn an_ambiguous_adapter_override_reports_the_candidates() {
        let adapters = vec![
            record("NVIDIA GeForce RTX 5080", r"C:\nvidia\nv-vk64.json", Some(17_094_934_528)),
            record("NVIDIA GeForce RTX 4090", r"C:\nvidia\nv-vk64.json", Some(25_769_803_776)),
        ];
        let overrides = Overrides { gpu_adapter: Some("geforce".into()), ..Overrides::default() };

        let error = resolve(&adapters, &overrides, None, RenderMode::FIRST).unwrap_err();

        match error {
            SelectError::AmbiguousAdapter { candidates, .. } => assert_eq!(candidates.len(), 2),
            other => panic!("expected AmbiguousAdapter, got {other:?}"),
        }
    }
}

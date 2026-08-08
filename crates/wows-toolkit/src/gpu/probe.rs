//! Enumerate display adapters from the driver registry.

use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

use crate::util::win32::Win32Status;

/// Registry class key holding one subkey per installed display adapter.
#[cfg(windows)]
const DISPLAY_CLASS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

// The decoding below is pure and stays under test on every platform, so these
// are reachable from the tests but not from a non-Windows build of the library.
#[cfg_attr(not(windows), allow(dead_code))]
const REG_BINARY: u32 = 3;
#[cfg_attr(not(windows), allow(dead_code))]
const REG_DWORD: u32 = 4;
#[cfg_attr(not(windows), allow(dead_code))]
const REG_QWORD: u32 = 11;

/// An adapter's marketing name, from the driver's `DriverDesc` value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdapterDescription(String);

impl AdapterDescription {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AdapterDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Path to a Vulkan ICD manifest, the value `VK_DRIVER_FILES` accepts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IcdManifestPath(PathBuf);

impl IcdManifestPath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for IcdManifestPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Adapter-local video memory as reported by the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VideoMemoryBytes(u64);

impl VideoMemoryBytes {
    /// `None` for zero: no adapter has zero bytes of memory, so the type refuses
    /// to represent a size that would rank a working adapter below one whose
    /// size is merely unreported.
    pub fn new(bytes: u64) -> Option<Self> {
        (bytes > 0).then_some(Self(bytes))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for VideoMemoryBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1} GiB", self.0 as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Identifies the set of adapters present, so a recorded render mode can be
/// discarded when the hardware or a driver changes underneath it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AdapterFingerprint(String);

impl AdapterFingerprint {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AdapterFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One adapter that can actually be pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterRecord {
    pub description: AdapterDescription,
    pub icd: IcdManifestPath,
    /// Manifests named by the registry that were passed over. Retained so a
    /// wrong pin is diagnosable from `--list-gpus` without a registry dump.
    pub discarded_icds: Vec<IcdManifestPath>,
    /// `None` when the driver reported no usable size. Never defaulted, because
    /// a fabricated zero would rank a real adapter last while reading as a
    /// measurement.
    pub memory: Option<VideoMemoryBytes>,
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("failed to open registry key {key}: {status}")]
    OpenKey { key: String, status: Win32Status },
    #[error("failed to enumerate subkeys of {key}: {status}")]
    EnumerateKeys { key: String, status: Win32Status },
}

/// Decode `HardwareInformation.qwMemorySize`, which drivers publish under
/// several registry types.
#[cfg_attr(not(windows), allow(dead_code))]
fn decode_memory_size(reg_type: u32, data: &[u8]) -> Option<VideoMemoryBytes> {
    let bytes = match reg_type {
        REG_QWORD | REG_BINARY => u64::from_le_bytes(data.get(..8)?.try_into().ok()?),
        REG_DWORD => u64::from(u32::from_le_bytes(data.get(..4)?.try_into().ok()?)),
        _ => return None,
    };
    VideoMemoryBytes::new(bytes)
}

/// Whether a class subkey name identifies an adapter instance.
///
/// The class key also carries siblings such as `Properties`, which denies read
/// access entirely. Only the four-digit instance keys are adapters.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_adapter_instance_key(name: &str) -> bool {
    name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit())
}

/// Build a fingerprint that is stable under probe ordering but changes when an
/// adapter or its driver path changes.
pub fn fingerprint(adapters: &[AdapterRecord]) -> AdapterFingerprint {
    let mut entries: Vec<String> =
        adapters.iter().map(|a| format!("{}={}", a.description.as_str(), a.icd.as_path().display())).collect();
    entries.sort();
    AdapterFingerprint(entries.join(";"))
}

/// Adapters that can be pinned, in registry enumeration order.
#[cfg(windows)]
pub fn probe() -> Result<Vec<AdapterRecord>, ProbeError> {
    windows_probe::probe()
}

#[cfg(not(windows))]
pub fn probe() -> Result<Vec<AdapterRecord>, ProbeError> {
    Ok(Vec::new())
}

#[cfg(windows)]
mod windows_probe {
    use std::path::PathBuf;

    use crate::util::registry::HKEY_LOCAL_MACHINE;
    use crate::util::registry::RegKey;

    use super::AdapterDescription;
    use super::AdapterRecord;
    use super::DISPLAY_CLASS_KEY;
    use super::IcdManifestPath;
    use super::ProbeError;
    use super::decode_memory_size;
    use super::is_adapter_instance_key;

    pub(super) fn probe() -> Result<Vec<AdapterRecord>, ProbeError> {
        let class_key = RegKey::open(HKEY_LOCAL_MACHINE, DISPLAY_CLASS_KEY)
            .map_err(|status| ProbeError::OpenKey { key: DISPLAY_CLASS_KEY.to_string(), status })?;
        let names = class_key
            .subkey_names()
            .map_err(|status| ProbeError::EnumerateKeys { key: DISPLAY_CLASS_KEY.to_string(), status })?;

        let mut adapters = Vec::new();
        for name in names.iter().filter(|name| is_adapter_instance_key(name)) {
            // A subkey that cannot be opened is one adapter's worth of missing
            // information, not a reason to report no adapters at all.
            let Ok(instance) = RegKey::open(class_key.raw(), name) else {
                continue;
            };

            let manifests: Vec<PathBuf> =
                instance.query_strings("VulkanDriverName").into_iter().map(PathBuf::from).collect();
            // Without a Vulkan manifest there is nothing to pin, which is what
            // separates a real adapter from the remote-display placeholders that
            // fill most of this class key.
            let Some(position) = manifests.iter().position(|path| path.exists()) else {
                continue;
            };

            let Some(description) = instance.query_strings("DriverDesc").into_iter().next() else {
                continue;
            };

            adapters.push(AdapterRecord {
                description: AdapterDescription::new(description),
                icd: IcdManifestPath::new(manifests[position].clone()),
                discarded_icds: manifests
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != position)
                    .map(|(_, path)| IcdManifestPath::new(path.clone()))
                    .collect(),
                memory: instance
                    .query("HardwareInformation.qwMemorySize")
                    .and_then(|(reg_type, data)| decode_memory_size(reg_type, &data)),
            });
        }

        Ok(adapters)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A string type, which carries no memory size.
    const REG_SZ: u32 = 1;

    #[test]
    fn memory_size_decodes_from_every_published_registry_type() {
        let qword = 17_094_934_528u64;
        assert_eq!(decode_memory_size(REG_QWORD, &qword.to_le_bytes()), VideoMemoryBytes::new(qword));
        assert_eq!(decode_memory_size(REG_BINARY, &qword.to_le_bytes()), VideoMemoryBytes::new(qword));
        assert_eq!(decode_memory_size(REG_DWORD, &536_870_912u32.to_le_bytes()), VideoMemoryBytes::new(536_870_912));
    }

    #[test]
    fn unusable_memory_size_is_absent_rather_than_zero() {
        assert_eq!(decode_memory_size(REG_QWORD, &0u64.to_le_bytes()), None);
        assert_eq!(decode_memory_size(REG_SZ, b"lots"), None);
        assert_eq!(decode_memory_size(REG_QWORD, &[1, 2, 3]), None);
    }

    #[test]
    fn only_four_digit_subkeys_are_adapter_instances() {
        assert!(is_adapter_instance_key("0000"));
        assert!(is_adapter_instance_key("0060"));
        assert!(!is_adapter_instance_key("Properties"));
        assert!(!is_adapter_instance_key("060"));
        assert!(!is_adapter_instance_key("00600"));
    }

    fn record(description: &str, icd: &str) -> AdapterRecord {
        AdapterRecord {
            description: AdapterDescription::new(description),
            icd: IcdManifestPath::new(icd),
            discarded_icds: Vec::new(),
            memory: None,
        }
    }

    #[test]
    fn fingerprint_ignores_probe_order() {
        let amd = record("AMD Radeon(TM) Graphics", r"C:\amd\amd-vulkan64.json");
        let nvidia = record("NVIDIA GeForce RTX 5080", r"C:\nvidia\nv-vk64.json");

        assert_eq!(fingerprint(&[amd.clone(), nvidia.clone()]), fingerprint(&[nvidia, amd]));
    }

    #[test]
    fn fingerprint_changes_when_a_driver_path_changes() {
        let before = fingerprint(&[record("NVIDIA GeForce RTX 5080", r"C:\nvidia\b7cca836\nv-vk64.json")]);
        let after = fingerprint(&[record("NVIDIA GeForce RTX 5080", r"C:\nvidia\c8ddb947\nv-vk64.json")]);

        assert_ne!(before, after);
    }

    /// Runs against whatever hardware the test host has, including none.
    #[cfg(windows)]
    #[test]
    fn probing_real_hardware_yields_only_pinnable_adapters() {
        let adapters = probe().expect("the display class key is readable on every supported Windows");

        // Visible under --nocapture, which is the only way to see what this
        // machine actually reports without a registry dump.
        for adapter in &adapters {
            println!("{} {:?} {}", adapter.description, adapter.memory, adapter.icd);
        }

        for adapter in &adapters {
            assert!(adapter.icd.as_path().exists(), "pinned a manifest that does not exist: {}", adapter.icd);
            assert!(!adapter.description.as_str().is_empty());
            assert!(adapter.memory.is_none_or(|memory| memory.get() > 0));
        }
    }

    #[test]
    fn fingerprint_changes_when_an_adapter_is_removed() {
        let amd = record("AMD Radeon(TM) Graphics", r"C:\amd\amd-vulkan64.json");
        let nvidia = record("NVIDIA GeForce RTX 5080", r"C:\nvidia\nv-vk64.json");

        assert_ne!(fingerprint(&[amd.clone(), nvidia]), fingerprint(&[amd]));
    }
}

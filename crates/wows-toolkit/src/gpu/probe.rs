//! Enumerate display adapters from the driver registry.

use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

/// Registry class key holding one subkey per installed display adapter.
const DISPLAY_CLASS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
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
    #[error("failed to open registry key {key}: win32 status {status}")]
    OpenKey { key: String, status: u32 },
    #[error("failed to enumerate subkeys of {key}: win32 status {status}")]
    EnumerateKeys { key: String, status: u32 },
}

/// Decode a `REG_SZ` or `REG_MULTI_SZ` payload into its component strings.
///
/// `VulkanDriverName` is `REG_SZ` on some drivers and `REG_MULTI_SZ` on others,
/// and the sibling `UserModeDriverName` is reliably multi. Both forms are
/// trailing-null terminated, so the same splitter serves each.
fn decode_reg_strings(units: &[u16]) -> Vec<String> {
    units.split(|unit| *unit == 0).filter(|part| !part.is_empty()).map(String::from_utf16_lossy).collect()
}

/// Decode `HardwareInformation.qwMemorySize`, which drivers publish under
/// several registry types.
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

    use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
    use windows_sys::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::HKEY;
    use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    use windows_sys::Win32::System::Registry::KEY_READ;
    use windows_sys::Win32::System::Registry::RegCloseKey;
    use windows_sys::Win32::System::Registry::RegEnumKeyExW;
    use windows_sys::Win32::System::Registry::RegOpenKeyExW;
    use windows_sys::Win32::System::Registry::RegQueryValueExW;

    use super::AdapterDescription;
    use super::AdapterRecord;
    use super::DISPLAY_CLASS_KEY;
    use super::IcdManifestPath;
    use super::ProbeError;
    use super::decode_memory_size;
    use super::decode_reg_strings;
    use super::is_adapter_instance_key;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Owns an open registry key so every early return closes it.
    struct RegKey(HKEY);

    impl RegKey {
        fn open(parent: HKEY, path: &str) -> Result<Self, u32> {
            let mut key: HKEY = std::ptr::null_mut();
            let status = unsafe { RegOpenKeyExW(parent, wide(path).as_ptr(), 0, KEY_READ, &mut key) };
            if status == ERROR_SUCCESS { Ok(Self(key)) } else { Err(status) }
        }

        fn raw(&self) -> HKEY {
            self.0
        }

        /// Raw value bytes and their registry type, or `None` when the value is
        /// absent.
        fn query(&self, name: &str) -> Option<(u32, Vec<u8>)> {
            let name = wide(name);
            let mut reg_type: u32 = 0;
            let mut size: u32 = 0;
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut reg_type,
                    std::ptr::null_mut(),
                    &mut size,
                )
            };
            if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
                return None;
            }

            let mut data = vec![0u8; size as usize];
            let status = unsafe {
                RegQueryValueExW(self.0, name.as_ptr(), std::ptr::null(), &mut reg_type, data.as_mut_ptr(), &mut size)
            };
            if status != ERROR_SUCCESS {
                return None;
            }
            data.truncate(size as usize);
            Some((reg_type, data))
        }

        /// A string value, decoded from either single or multi form.
        fn query_strings(&self, name: &str) -> Vec<String> {
            let Some((_, data)) = self.query(name) else {
                return Vec::new();
            };
            let units: Vec<u16> = data.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            decode_reg_strings(&units)
        }
    }

    impl Drop for RegKey {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn subkey_names(key: &RegKey) -> Result<Vec<String>, u32> {
        let mut names = Vec::new();
        // Registry key names are capped at 255 characters plus a terminator.
        let mut buffer = [0u16; 256];
        for index in 0.. {
            let mut length = buffer.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    key.raw(),
                    index,
                    buffer.as_mut_ptr(),
                    &mut length,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            match status {
                ERROR_SUCCESS => names.push(String::from_utf16_lossy(&buffer[..length as usize])),
                ERROR_NO_MORE_ITEMS => break,
                other => return Err(other),
            }
        }
        Ok(names)
    }

    pub(super) fn probe() -> Result<Vec<AdapterRecord>, ProbeError> {
        let class_key = RegKey::open(HKEY_LOCAL_MACHINE, DISPLAY_CLASS_KEY)
            .map_err(|status| ProbeError::OpenKey { key: DISPLAY_CLASS_KEY.to_string(), status })?;
        let names = subkey_names(&class_key)
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

    fn utf16(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    #[test]
    fn single_string_value_decodes_to_one_entry() {
        let mut units = utf16(r"C:\drivers\nv-vk64.json");
        units.push(0);

        assert_eq!(decode_reg_strings(&units), vec![r"C:\drivers\nv-vk64.json".to_string()]);
    }

    #[test]
    fn multi_string_value_decodes_to_every_entry() {
        let mut units = Vec::new();
        for path in [r"C:\a.json", r"C:\b.json"] {
            units.extend(utf16(path));
            units.push(0);
        }
        units.push(0);

        assert_eq!(decode_reg_strings(&units), vec![r"C:\a.json".to_string(), r"C:\b.json".to_string()]);
    }

    #[test]
    fn empty_value_decodes_to_no_entries() {
        assert!(decode_reg_strings(&[]).is_empty());
        assert!(decode_reg_strings(&[0, 0]).is_empty());
    }

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

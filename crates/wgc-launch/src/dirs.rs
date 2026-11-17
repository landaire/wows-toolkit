//! Toolkit-owned directories for runners, prefixes and install overlays.

use std::path::Path;
use std::path::PathBuf;

/// Everything the launcher writes lives under one directory in the app data
/// dir, so nothing lands in `~/.local/share/umu` or Steam's own folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDirs {
    root: PathBuf,
}

impl RuntimeDirs {
    pub fn new(app_data_dir: &Path) -> Self {
        Self { root: app_data_dir.join("game-runtime") }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `STEAM_COMPAT_DATA_PATH` for Steam's Proton. Proton keeps the Wine prefix in `pfx/`.
    pub fn steam_compat_data(&self) -> PathBuf {
        self.root.join("proton").join("compatdata")
    }

    /// `WINEPREFIX` for umu. umu keeps its prefix separate from Steam Proton's,
    /// because the two runners ship different Wine builds.
    pub fn umu_prefix(&self) -> PathBuf {
        self.root.join("umu").join("prefix")
    }

    /// `UMU_FOLDERS_PATH`: where umu puts UMU-Proton and the Steam runtimes.
    pub fn umu_folders(&self) -> PathBuf {
        self.root.join("umu").join("folders")
    }

    /// Where umu unpacks the UMU-Proton builds it downloads.
    pub(crate) fn umu_compat_tools(&self) -> PathBuf {
        self.umu_folders().join("Steam").join("compatibilitytools.d")
    }

    /// `XDG_CACHE_HOME` for umu, which caches its downloads there.
    pub fn umu_cache(&self) -> PathBuf {
        self.root.join("umu").join("cache")
    }

    /// Locked by the toolkit process that prepares the prefix and an overlay and runs a session.
    pub fn session_lock(&self) -> PathBuf {
        self.root.join("session.lock")
    }

    /// Per-install overlay state for the install at `canonical_root`.
    pub fn install_state(&self, canonical_root: &Path) -> InstallStateDir {
        let key = InstallKey::from_canonical_root(canonical_root);
        let dir = self.root.join("installs").join(key.as_str());
        InstallStateDir { key, dir }
    }
}

/// Stable directory name for an install: a readable tail of the path plus a
/// hash of the whole path. The name persists on disk, so the hash must not
/// change between toolchains; `std`'s hashers do not promise that.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstallKey(String);

impl InstallKey {
    pub fn from_canonical_root(canonical_root: &Path) -> Self {
        const TAIL_COMPONENTS: usize = 2;

        let mut tail: Vec<String> = canonical_root
            .components()
            .rev()
            .filter_map(|component| match component {
                std::path::Component::Normal(name) => Some(sanitize(&name.to_string_lossy())),
                _ => None,
            })
            .take(TAIL_COMPONENTS)
            .collect();
        tail.reverse();

        let hash = fnv1a64(canonical_root.as_os_str().as_encoded_bytes());
        let name = if tail.is_empty() { format!("{hash:016x}") } else { format!("{}-{hash:016x}", tail.join("-")) };

        Self(name)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallStateDir {
    key: InstallKey,
    dir: PathBuf,
}

impl InstallStateDir {
    pub fn key(&self) -> &InstallKey {
        &self.key
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The same directory by its resolved path, which the mount table uses. It must exist.
    pub fn canonicalize(&self) -> std::io::Result<InstallStateDir> {
        Ok(InstallStateDir { key: self.key.clone(), dir: self.dir.canonicalize()? })
    }

    /// Everything the game writes: preferences, `profile/`, replays.
    pub fn upper(&self) -> PathBuf {
        self.dir.join("upper")
    }

    pub fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    /// Mount point of the merged view the game runs from.
    pub fn merged(&self) -> PathBuf {
        self.dir.join("merged")
    }

    /// Records which overlay implementation owns `upper`; their whiteout formats differ.
    pub fn backend_record(&self) -> PathBuf {
        self.dir.join("overlay-backend")
    }

    /// `DXVK_SHADER_CACHE_PATH` for the install. Outside the prefix, so the
    /// cache survives a new prefix and a switch between Proton and umu.
    pub fn shader_cache(&self) -> PathBuf {
        self.dir.join("shader-cache")
    }

    /// Overlay state for runs with a delta layer. Its upper is apart from
    /// plain runs', so the two keep separate preferences and replays.
    pub fn with_delta(&self) -> InstallStateDir {
        InstallStateDir { key: self.key.clone(), dir: self.dir.join("delta") }
    }

    /// Names the delta layer the install's views use, while one is selected.
    pub fn delta_record(&self) -> PathBuf {
        self.dir.join("delta-layer")
    }
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '.' { c } else { '-' }).collect()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_key_is_stable_across_releases() {
        let key = InstallKey::from_canonical_root(Path::new("/var/mnt/e/WoWs/World_of_Warships"));
        assert_eq!(key.as_str(), "WoWs-World_of_Warships-5be47e570d27a77d");
    }

    #[test]
    fn install_key_distinguishes_paths_with_the_same_tail() {
        let a = InstallKey::from_canonical_root(Path::new("/a/WoWs/World_of_Warships"));
        let b = InstallKey::from_canonical_root(Path::new("/b/WoWs/World_of_Warships"));
        assert_ne!(a, b);
    }
}

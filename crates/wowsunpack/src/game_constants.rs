//! Game constants. The struct definitions, XML parsing, and hardcoded defaults
//! live in `wows-core::game_constants`; this module re-exports them and adds the
//! VFS-backed loaders, which depend on the `vfs` crate that the foundation crate
//! deliberately avoids.

pub use wows_core::game_constants::*;

#[cfg(feature = "vfs")]
use std::io::Read;

#[cfg(feature = "vfs")]
fn read_vfs_file(vfs: &vfs::VfsPath, path: &str) -> Result<Vec<u8>, vfs::VfsError> {
    let mut buf = Vec::new();
    vfs.join(path)?.open_file()?.read_to_end(&mut buf).map_err(vfs::VfsError::from)?;
    Ok(buf)
}

/// Load battle constants from game files, falling back to defaults if the file
/// can't be read.
#[cfg(feature = "vfs")]
pub fn load_battle_constants(vfs: &vfs::VfsPath) -> BattleConstants {
    if let Ok(buf) = read_vfs_file(vfs, BATTLE_CONSTANTS_PATH) {
        BattleConstants::from_xml(&buf)
    } else {
        BattleConstants::defaults()
    }
}

/// Load ship constants from game files, falling back to defaults if the file
/// can't be read.
#[cfg(feature = "vfs")]
pub fn load_ships_constants(vfs: &vfs::VfsPath) -> ShipsConstants {
    if let Ok(buf) = read_vfs_file(vfs, SHIPS_CONSTANTS_PATH) {
        ShipsConstants::from_xml(&buf)
    } else {
        ShipsConstants::defaults()
    }
}

/// Load weapons constants from game files, falling back to defaults if the file
/// can't be read.
#[cfg(feature = "vfs")]
pub fn load_weapons_constants(vfs: &vfs::VfsPath) -> WeaponsConstants {
    if let Ok(buf) = read_vfs_file(vfs, WEAPONS_CONSTANTS_PATH) {
        WeaponsConstants::from_xml(&buf)
    } else {
        WeaponsConstants::defaults()
    }
}

/// Load common constants from game files, falling back to defaults if the file
/// can't be read.
#[cfg(feature = "vfs")]
pub fn load_common_constants(vfs: &vfs::VfsPath) -> CommonConstants {
    if let Ok(buf) = read_vfs_file(vfs, COMMON_CONSTANTS_PATH) {
        CommonConstants::from_xml(&buf)
    } else {
        CommonConstants::defaults()
    }
}

/// Load channel constants from game files, falling back to defaults if the file
/// can't be read.
#[cfg(feature = "vfs")]
pub fn load_channel_constants(vfs: &vfs::VfsPath) -> ChannelConstants {
    if let Ok(buf) = read_vfs_file(vfs, CHANNEL_CONSTANTS_PATH) {
        ChannelConstants::from_xml(&buf)
    } else {
        ChannelConstants::defaults()
    }
}

/// Replace `common`'s consumable id -> name map with the layout recovered for
/// `version` (static analysis of the obfuscated game scripts; see
/// [`crate::consumable_versions`]). Lets a replay resolve consumable ids the way
/// its own client did rather than against the latest layout, which matters for
/// older replays whose id ordering differs. No-op if the version predates every
/// known layout and no floor applies. Kept here (not on `CommonConstants` in
/// `wows-core`) because the version table is build-script generated in this crate.
pub fn apply_version_consumables(common: &mut CommonConstants, version: crate::data::Version) {
    if let Some(table) = crate::consumable_versions::consumable_ids_for_version(version) {
        *common.consumable_types_mut() =
            table.iter().map(|(id, name)| (*id, std::borrow::Cow::Borrowed(*name))).collect();
    }
}

#[cfg(test)]
mod version_consumable_tests {
    use super::*;
    use crate::data::Version;

    fn v(major: u32, minor: u32, patch: u32, build: u32) -> Version {
        Version { major, minor, patch, build }
    }

    #[test]
    fn legacy_client_resolves_its_own_consumable_layout() {
        // The 0.9.10 client (e.g. the Smaland replay, build 3052606) ordered consumables
        // differently from current builds: depth charges sat at id 27 and the plane /
        // tactical block did not exist yet. Resolving against the modern default would
        // mislabel every id past the submarine block.
        let mut common = CommonConstants::defaults();
        apply_version_consumables(&mut common, v(0, 9, 10, 3052606));
        assert_eq!(common.consumable_type(22), Some("callFighters"));
        assert_eq!(common.consumable_type(23), Some("regenerateHealth"));
        assert_eq!(common.consumable_type(27), Some("depthCharges"));
        // The modern default placed trigger7 at 27; confirm we replaced it.
        assert_ne!(common.consumable_type(27), Some("trigger7"));
    }

    #[test]
    fn modern_client_keeps_the_full_layout() {
        let mut common = CommonConstants::defaults();
        apply_version_consumables(&mut common, v(15, 2, 0, 12116141));
        assert_eq!(common.consumable_type(0), Some("crashCrew"));
        // The modern layout is much larger than the legacy one.
        assert!(common.consumable_types().len() > 40);
    }
}

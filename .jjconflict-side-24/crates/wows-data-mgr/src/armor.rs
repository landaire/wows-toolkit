//! Armor extracts in a dump.
//!
//! A dump holds one [`armor_extract::VehicleArmor`] object per ship and one
//! [`armor_extract::MountModelArmor`] object per mount model, recorded in
//! `metadata.toml` under [`ARMOR_DIR`] like any other dumped file, so readers
//! reach them through the dump's VFS. They replace the `.geometry` files and
//! `assets.bin` an armor query would otherwise need, which are several
//! gigabytes per build.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use rootcause::prelude::*;
use wowsunpack::export::armor_extract;
use wowsunpack::export::armor_extract::VehicleArmorExtract;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use crate::cas;

/// The dump directory every armor object sits under.
pub const ARMOR_DIR: &str = "armor";

/// The prototype database extraction needs. Builds without it carry no
/// extractable armor.
pub const ASSETS_BIN: &str = "content/assets.bin";

/// Game files the extraction reads: GameParams, the prototype database that
/// names each model's visuals, and the hull and mount geometry. `*` spans `/`
/// in `wowsunpack pkgs` globs.
pub const SOURCE_GLOBS: &[&str] = &[
    "content/GameParams.data",
    "content/assets.bin",
    "content/gameplay/*/ship/*.geometry",
    "content/gameplay/*/gun/*.geometry",
    "content/gameplay/*/director/*.geometry",
    "content/gameplay/*/finder/*.geometry",
    "content/gameplay/*/radar/*.geometry",
    "content/gameplay/*/catapult/*.geometry",
    "content/gameplay/*/cap/*.geometry",
];

/// The dump path of a ship's armor object.
pub fn vehicle_path(param_index: &str) -> String {
    format!("{ARMOR_DIR}/vehicles/{param_index}.armor")
}

/// The dump path of a mount model's armor object, from the model path
/// GameParams gives the mount.
pub fn mount_model_path(model_path: &str) -> String {
    let stem = model_path.strip_suffix(".model").unwrap_or(model_path);

    format!("{ARMOR_DIR}/mounts/{stem}.armor")
}

/// What an extraction recorded.
#[derive(Debug, Default)]
pub struct ExtractSummary {
    pub vehicles: usize,
    pub mount_models: usize,

    /// Ships whose models could not be read, by GameParams index, with the
    /// reason. A build's event and test ships often reference models the
    /// packages do not ship.
    pub failures: Vec<(String, Report)>,
}

/// Extracts the armor of every ship in `vfs` into the store and replaces the
/// armor entries of `files` with them. `vfs` is a game install's VFS for the
/// build whose `files` these are. Errors when no ship could be extracted,
/// which means the install lacks the vehicle packages.
pub fn extract_armor(
    vfs: &VfsPath,
    cas_root: &Path,
    files: &mut BTreeMap<String, String>,
    workers: NonZeroUsize,
) -> Result<ExtractSummary, Report> {
    let assets = ShipAssets::load(vfs).context("loading assets.bin and GameParams for armor extraction")?;
    let vehicles: Vec<_> = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|param| {
            let vehicle = param.vehicle()?;
            vehicle.model_path()?;
            Some((param.index().to_owned(), vehicle))
        })
        .collect();

    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    let mut summary = ExtractSummary::default();
    let next = AtomicUsize::new(0);
    // Bounded so a slow store does not let workers queue whole ships in memory.
    let (sender, receiver) = mpsc::sync_channel::<(String, Result<VehicleArmorExtract, Report>)>(workers.get());

    std::thread::scope(|scope| -> Result<(), Report> {
        for _ in 0..workers.get() {
            let sender = sender.clone();
            let assets = &assets;
            let vehicles = &vehicles;
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((param_index, vehicle)) = vehicles.get(index) else {
                        break;
                    };
                    let extract = assets.vehicle_armor(param_index, vehicle);
                    if sender.send((param_index.clone(), extract)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        for (param_index, extract) in receiver {
            let extract = match extract {
                Ok(extract) => extract,
                Err(error) => {
                    summary.failures.push((param_index, error));
                    continue;
                }
            };

            for model in &extract.mount_models {
                let path = mount_model_path(&model.model_path);
                if entries.contains_key(&path) {
                    continue;
                }
                let hash = cas::store(cas_root, &armor_extract::encode_mount_model(model))?;
                entries.insert(path, hash);
                summary.mount_models += 1;
            }

            let hash = cas::store(cas_root, &armor_extract::encode_vehicle(&extract.vehicle))?;
            entries.insert(vehicle_path(&param_index), hash);
            summary.vehicles += 1;
        }

        Ok(())
    })?;

    if summary.vehicles == 0 {
        bail!(
            "no ship armor could be extracted ({} failures); the install is missing the vehicle packages",
            summary.failures.len()
        );
    }

    let prefix = format!("{ARMOR_DIR}/");
    files.retain(|path, _| !path.starts_with(&prefix));
    files.extend(entries);
    summary.failures.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths_sit_under_the_armor_directory() {
        assert_eq!(vehicle_path("PJSB018"), "armor/vehicles/PJSB018.armor");
        assert_eq!(
            mount_model_path("content/gameplay/japan/gun/main/JGM178/JGM178.model"),
            "armor/mounts/content/gameplay/japan/gun/main/JGM178/JGM178.armor"
        );
    }
}

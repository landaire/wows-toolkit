//! Ship armor models from the dump's armor extract.

use std::io::Read;

use wows_data_mgr::armor;
use wowsunpack::export::armor_extract;
use wowsunpack::export::armor_extract::MountModelArmor;
use wowsunpack::export::armor_extract::VehicleArmor;

use crate::data::GameData;
use crate::error::ArmorReadError;
use crate::error::QueryError;
use crate::ships::Ship;

impl GameData {
    /// The armor extract of `ship`, or `None` when the dump holds none for it
    /// (a dump made without armor, or a ship whose models the build lacks).
    pub(crate) fn vehicle_armor(&self, ship: &Ship) -> Result<Option<VehicleArmor>, QueryError> {
        let path = armor::vehicle_path(ship.param.index());
        let Some(bytes) = self.read_armor_object(&path).map_err(|source| armor_error(ship, source))? else {
            return Ok(None);
        };

        armor_extract::decode_vehicle(&bytes).map(Some).map_err(|source| armor_error(ship, source.into()))
    }

    /// The armor of the mount model at `model_path`, as a vehicle extract names
    /// it, or `None` when the dump does not hold it.
    pub(crate) fn mount_model_armor(
        &self,
        ship: &Ship,
        model_path: &str,
    ) -> Result<Option<MountModelArmor>, QueryError> {
        let path = armor::mount_model_path(model_path);
        let Some(bytes) = self.read_armor_object(&path).map_err(|source| armor_error(ship, source))? else {
            return Ok(None);
        };

        armor_extract::decode_mount_model(&bytes).map(Some).map_err(|source| armor_error(ship, source.into()))
    }

    fn read_armor_object(&self, path: &str) -> Result<Option<Vec<u8>>, ArmorReadError> {
        let entry = self.vfs.join(path)?;
        if !entry.exists()? {
            return Ok(None);
        }

        let mut bytes = Vec::new();
        entry.open_file()?.read_to_end(&mut bytes)?;

        Ok(Some(bytes))
    }
}

fn armor_error(ship: &Ship, source: ArmorReadError) -> QueryError {
    QueryError::ArmorUnreadable { ship: ship.name.clone(), source }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn yamato_mounts_sit_on_named_hardpoints() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
            .expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");
        let ship = game.ships.iter().find(|ship| ship.param.index() == "PJSB018").expect("Yamato exists");

        let armor = game.vehicle_armor(ship).expect("readable").expect("the dump carries Yamato's armor");

        let hull = armor.hull_upgrade(None).expect("a stock hull");
        let hardpoints = &armor.hull_models[hull.hull_model.0].hardpoints;
        let main_turrets: Vec<(String, i32, String)> = hull
            .mounts
            .iter()
            .filter(|mount| mount.hp_name.starts_with("HP_JGM"))
            .map(|mount| {
                let hardpoint =
                    hardpoints.iter().find(|hardpoint| hardpoint.name == mount.hp_name).expect("on the hull");
                let model = game.mount_model_armor(ship, &mount.model_path).expect("readable").expect("stored");
                (mount.hp_name.clone(), hardpoint.yaw().value().round() as i32, model.model_path)
            })
            .collect();
        insta::assert_yaml_snapshot!(main_turrets);
    }
}

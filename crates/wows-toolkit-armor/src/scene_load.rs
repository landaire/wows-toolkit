//! A scene load in two stages. The first reads what the opening view draws;
//! the second finishes the scene in the background, each of its jobs storing
//! what it read in the scene as it lands. Both stop at their next step once
//! cancelled, so a load that is superseded or whose pane closes stops doing
//! work.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use rootcause::Report;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::export::texture::TextureLod;

use crate::hull_picking::HullPickIndex;
use crate::hull_picking::MeshSource;
use crate::load_ship::LoadedShipArmor;
use crate::surface_maps::decode_ship_surface_maps;

/// Asks a running load to stop. Clones share one flag.
#[derive(Clone, Debug)]
pub struct LoadCancel(Arc<AtomicBool>);

/// A load stopped because it was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the load was cancelled")]
pub struct LoadCancelled;

/// A scene as the first stage of its load leaves it, and the jobs that
/// finish it.
pub struct FirstStage {
    pub scene: LoadedShipArmor,
    pub rest: Vec<RestJob>,
}

/// One part of a scene the background stage reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RestPiece {
    /// The hover-picking index.
    Pick,
    /// The camouflage schemes a ship can wear.
    Camo,
    /// The hull's normal and metal-gloss maps.
    SurfaceMaps,
}

/// A job of the background stage, with what it reads from.
pub enum RestJob {
    /// Builds the hover-picking index from the scene's hull meshes, one
    /// source per mesh.
    Pick(Vec<MeshSource>),
    /// Lists a ship's camouflage schemes.
    Camo(Arc<ShipModelContext>),
    /// Decodes the surface maps of a ship's hull materials at the detail its
    /// albedos were decoded at.
    SurfaceMaps { ctx: Arc<ShipModelContext>, lod: TextureLod },
}

/// Why a background job stored nothing.
#[derive(Debug, thiserror::Error)]
pub enum RestJobError {
    #[error(transparent)]
    Cancelled(#[from] LoadCancelled),
    #[error("failed to resolve camouflage textures: {report}")]
    Camo { report: Report },
}

/// The load running for one purpose. Starting another, or dropping the slot
/// with its owner, cancels the one held.
#[derive(Debug, Default)]
pub struct LoadSlot(Option<LoadCancel>);

/// The background stage of one scene, held by every viewport showing the
/// scene. Cancelled once the last of them lets it go.
#[derive(Debug)]
pub struct SharedLoad(LoadCancel);

impl LoadCancel {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// `Err` once cancelled, for `?` between the steps of a load.
    pub fn checkpoint(&self) -> Result<(), LoadCancelled> {
        if self.is_cancelled() { Err(LoadCancelled) } else { Ok(()) }
    }
}

impl Default for LoadCancel {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadSlot {
    /// Cancels the load held and holds a new one.
    pub fn start(&mut self) -> LoadCancel {
        self.cancel();
        let cancel = LoadCancel::new();
        self.0 = Some(cancel.clone());

        cancel
    }

    pub fn cancel(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel.cancel();
        }
    }
}

impl Drop for LoadSlot {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl SharedLoad {
    pub fn start() -> Arc<Self> {
        Arc::new(Self(LoadCancel::new()))
    }

    pub fn cancel_flag(&self) -> &LoadCancel {
        &self.0
    }
}

impl Drop for SharedLoad {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl RestJob {
    pub fn piece(&self) -> RestPiece {
        match self {
            Self::Pick(_) => RestPiece::Pick,
            Self::Camo(_) => RestPiece::Camo,
            Self::SurfaceMaps { .. } => RestPiece::SurfaceMaps,
        }
    }

    /// Reads this job's piece and stores it in `scene`. Synchronous and
    /// CPU/IO-bound; callers run it off the UI thread. Nothing is stored once
    /// `cancel` is set.
    pub fn run(self, scene: &LoadedShipArmor, cancel: &LoadCancel) -> Result<(), RestJobError> {
        cancel.checkpoint()?;

        match self {
            Self::Pick(sources) => {
                let pick = HullPickIndex::build(&scene.hull_meshes, sources);
                cancel.checkpoint()?;
                // A scene is finished once, so a value already stored is this one.
                let _ = scene.hull_pick.set(pick);
            }
            Self::Camo(ctx) => {
                let source = ctx.camo_texture_source().map_err(|report| RestJobError::Camo { report })?;
                let mut infos = source.scheme_infos();
                number_repeated_names(&mut infos);
                cancel.checkpoint()?;
                let _ = scene.camo_source.set(source);
                let _ = scene.camo_scheme_infos.set(infos);
            }
            Self::SurfaceMaps { ctx, lod } => {
                let maps = decode_ship_surface_maps(&ctx, &scene.hull_meshes, lod, cancel)?;
                cancel.checkpoint()?;
                let _ = scene.hull_surface_maps.set(maps);
            }
        }

        Ok(())
    }
}

/// Numbers the second and later schemes sharing a name, so the picker tells
/// them apart.
fn number_repeated_names(infos: &mut [CamoSchemeInfo]) {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for info in infos {
        let count = counts.entry(info.display_name.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            info.display_name = format!("{} ({count})", info.display_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_a_load_cancels_the_one_held() {
        let mut slot = LoadSlot::default();
        let first = slot.start();
        let second = slot.start();

        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        assert_eq!(first.checkpoint(), Err(LoadCancelled));
    }

    #[test]
    fn dropping_the_slot_cancels_its_load() {
        let mut slot = LoadSlot::default();
        let held = slot.start();
        drop(slot);

        assert!(held.is_cancelled());
    }
}

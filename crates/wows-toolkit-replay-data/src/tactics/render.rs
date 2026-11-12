//! Drawing a board, and the art a session is sent for it.

use std::sync::Arc;

use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::viewport::MapViewport;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::GameParamProvider;
use wt_collab_client::types::Annotation;

use crate::frame::BgraFrame;
use crate::load::GameDataCache;
use crate::tactics::BoardCapPoint;

/// Everything one rasterisation of a board needs, taken off the board so it runs on another
/// thread while the reader goes on editing.
pub struct FrameRequest {
    pub(super) space: String,
    /// Art a peer sent, for a board this build has none for.
    pub(super) art: Option<Arc<image::RgbImage>>,
    /// What the map measures, where it came off the wire with the art.
    pub(super) info: Option<MapInfo>,
    /// Names the entry a renderer over peer art is kept under, since the art belongs to the board
    /// rather than to a map this build ships.
    pub(super) board_id: u64,
    pub(super) game_data: GameDataCache,
    /// The version the ranges are read at.
    pub(super) version: Option<Version>,
    pub(super) view: MapViewport,
    pub(super) caps: Vec<BoardCapPoint>,
    pub(super) annotations: Vec<Annotation>,
}

impl FrameRequest {
    /// The space this request draws.
    pub fn space(&self) -> &str {
        &self.space
    }

    /// Draws the map with what is on it. `None` when the loaded build ships no art for that map,
    /// which the board says rather than claiming to still be drawing it.
    pub fn rasterise(&self) -> Option<BgraFrame> {
        let loaded = self.game_data.newest_loaded()?;
        // The measurements a peer sent, where this build ships none for the map.
        let map = match self.info.clone() {
            Some(sent) => sent,
            None => wows_minimap_renderer::assets::load_map_info(&self.space, loaded.vfs())?,
        };
        let mut commands: Vec<DrawCommand> = self.caps.iter().map(|cap| cap.command(&map)).collect();
        // Under the markers, so a ship is not buried under its own circles.
        commands.extend(ship_ranges(&self.annotations, &self.game_data, self.version, map.space_size as f32));
        // Over the zones, so a line drawn across one is not buried under it.
        commands.extend(self.annotations.iter().flat_map(wt_collab_client::geometry::annotation_commands));
        match self.art.as_ref() {
            Some(art) => crate::minimap_preview::render_map_art(
                &format!("board-{}", self.board_id),
                art,
                &self.game_data,
                self.view,
                &commands,
            ),
            None => crate::minimap_preview::render_map(&self.space, &self.game_data, self.view, &commands),
        }
    }
}

/// The range circles the placed ships show.
///
/// Read from each ship's own params, so what is drawn is what that ship sees and shoots rather
/// than a figure typed in.
fn ship_ranges(
    annotations: &[Annotation],
    game_data: &GameDataCache,
    version: Option<Version>,
    space_size: f32,
) -> Vec<DrawCommand> {
    // Without a version there is no saying what a ship's ranges are: they are gated on it, and a
    // circle drawn at the wrong one reads as a fact.
    let Some(version) = version else { return Vec::new() };
    let Some(loaded) = game_data.newest_loaded() else { return Vec::new() };
    let provider = loaded.provider();

    let mut circles = Vec::new();
    for annotation in annotations {
        let Annotation::Ship { config: Some(config), .. } = annotation else { continue };
        let Some(param) = GameParamProvider::game_param_by_id(provider.as_ref(), config.param_id) else {
            continue;
        };
        let Some(vehicle) = param.vehicle() else { continue };
        let hull = config.hull_name.as_deref();
        let ranges = vehicle.resolve_ranges(Some(provider.as_ref()), hull, version);
        circles.extend(wt_collab_client::geometry::ship_range_commands(annotation, &ranges, space_size));
    }
    circles
}

/// What a session is told a board is drawn on, read off the build in the background.
pub struct ArtRequest {
    pub(super) space: String,
    pub(super) game_data: GameDataCache,
}

/// The map's art as a PNG and its measurements, either absent where no build is open, with the
/// space they were read for.
pub struct BoardArt {
    pub(super) space: String,
    pub(super) png: Option<Vec<u8>>,
    pub(super) info: Option<MapInfo>,
}

impl ArtRequest {
    /// Reads the art and the measurements. Both absent where no build is open, which is a board a
    /// peer can still draw from the map name if it has that build.
    pub fn load(&self) -> BoardArt {
        let space = self.space.clone();
        let Some(loaded) = self.game_data.newest_loaded() else { return BoardArt { space, png: None, info: None } };
        let art = wows_minimap_renderer::assets::load_map_image(&self.space, loaded.vfs());
        let info = wows_minimap_renderer::assets::load_map_info(&self.space, loaded.vfs());
        BoardArt { space, png: art.as_ref().and_then(encode_png), info }
    }
}

/// A map image as a PNG, for sending to a peer.
///
/// `None` when it cannot be encoded, which leaves the board announced without art rather than not
/// announced at all.
fn encode_png(art: &image::RgbImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

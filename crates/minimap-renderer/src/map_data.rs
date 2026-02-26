pub use wows_replays::types::WorldPos;

/// Original minimap image size in pixels (before resizing to output).
/// The game's coordinate system is based on this size.
pub const NATIVE_MINIMAP_SIZE: u32 = 760;

/// Map metadata for coordinate conversion.
#[derive(Debug, Clone)]
pub struct MapInfo {
    pub space_size: i32,
}

/// Pixel position on the minimap image.
/// (0,0) is top-left, positive X = right, positive Y = down.
/// Does NOT include HUD offset — that's applied at draw time.
#[derive(Debug, Clone, Copy)]
pub struct MinimapPos {
    pub x: i32,
    pub y: i32,
}

impl MapInfo {
    /// Convert world coordinates to minimap pixel coordinates.
    ///
    /// Uses the native minimap size (760) for scaling to match the game's
    /// coordinate system, then rescales to the output size.
    pub fn world_to_minimap(&self, pos: WorldPos, output_size: u32) -> MinimapPos {
        let native = NATIVE_MINIMAP_SIZE as f64;
        let scale = native / self.space_size as f64;
        let half = native / 2.0;
        let rescale = output_size as f64 / native;
        MinimapPos {
            x: ((pos.x as f64 * scale + half) * rescale) as i32,
            y: ((-pos.z as f64 * scale + half) * rescale) as i32,
        }
    }

    /// Convert a NormalizedPos (from `updateMinimapVisionInfo` packets) to minimap pixels.
    ///
    /// The decoder stores raw 11-bit values as `raw / 512.0 - 1.5`. The game's actual
    /// pack format maps those 11-bit values to world coordinates in [-2500, 2500]:
    ///   `world = raw_11bit / 2047.0 * 5000.0 - 2500.0`
    ///
    /// This method recovers the world coordinate and routes through `world_to_minimap`
    /// so both coordinate paths produce identical pixel positions.
    pub fn normalized_to_minimap(&self, pos: &wows_replays::types::NormalizedPos, output_size: u32) -> MinimapPos {
        // Recover raw 11-bit value: raw = (stored + 1.5) * 512
        // Convert to world: world = raw / 2047 * 5000 - 2500
        let raw_x = (pos.x + 1.5) * 512.0;
        let raw_y = (pos.y + 1.5) * 512.0;
        let world_x = raw_x as f64 / 2047.0 * 5000.0 - 2500.0;
        let world_z = raw_y as f64 / 2047.0 * 5000.0 - 2500.0;
        // NormalizedPos.y maps to world Z (north-south axis), but the minimap Y axis
        // is inverted relative to world Z. world_to_minimap handles -Z -> +Y, so we
        // pass z directly (world_to_minimap negates it internally).
        self.world_to_minimap(WorldPos { x: world_x as f32, y: 0.0, z: world_z as f32 }, output_size)
    }
}

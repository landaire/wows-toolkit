// ─── Armor Viewer Constants ──────────────────────────────────────────────────
//
// Extracted from ui/armor_viewer.rs to avoid magic numbers and enable reuse
// by the realtime armor viewer.

// ─── Trajectory Visualization ────────────────────────────────────────────────

/// Color palette for distinguishing multiple trajectories in the 3D view.
/// Each color is [R, G, B, A] in 0.0–1.0 range.
pub const TRAJECTORY_PALETTE: [[f32; 4]; 8] = [
    [1.0, 0.8, 0.2, 1.0], // gold
    [0.3, 0.7, 1.0, 1.0], // sky blue
    [1.0, 0.4, 0.4, 1.0], // coral
    [0.4, 0.9, 0.4, 1.0], // lime
    [1.0, 0.5, 0.8, 1.0], // pink
    [1.0, 0.6, 0.2, 1.0], // orange
    [0.3, 0.9, 0.9, 1.0], // cyan
    [0.7, 0.5, 1.0, 1.0], // lavender
];

/// Color palette for distinguishing comparison ships in detonation markers and UI labels.
/// Each color is [R, G, B] in 0.0–1.0 range.
pub const SHIP_COLORS: [[f32; 3]; 8] = [
    [1.0, 0.5, 0.1], // orange
    [0.3, 0.6, 1.0], // blue
    [1.0, 0.3, 0.5], // pink
    [0.3, 0.9, 0.4], // green
    [0.9, 0.3, 0.9], // magenta
    [1.0, 0.9, 0.2], // yellow
    [0.2, 0.9, 0.8], // teal
    [0.8, 0.5, 1.0], // purple
];

// ─── Impact Angle Thresholds (degrees from normal) ───────────────────────────

/// Impacts shallower than this angle are considered favorable (green).
pub const SHALLOW_ANGLE_DEG: f32 = 30.0;

/// Impacts steeper than this angle are in the ricochet danger zone (red).
pub const STEEP_ANGLE_DEG: f32 = 45.0;

/// Color for favorable-angle impacts (< SHALLOW_ANGLE_DEG). Green.
pub const IMPACT_COLOR_SHALLOW: [f32; 3] = [0.3, 0.9, 0.3];

/// Color for medium-angle impacts (SHALLOW..STEEP). Yellow-orange.
pub const IMPACT_COLOR_MEDIUM: [f32; 3] = [0.9, 0.7, 0.2];

/// Color for steep/ricochet-zone impacts (>= STEEP_ANGLE_DEG). Red.
pub const IMPACT_COLOR_STEEP: [f32; 3] = [0.9, 0.3, 0.3];

// ─── Plate Boundary Edge Rendering ───────────────────────────────────────────

/// Half-width of plate boundary edge quads in world-space units.
pub const PLATE_EDGE_HALF_WIDTH: f32 = 0.003;

/// Normal offset for plate edges to prevent z-fighting with the armor surface.
pub const PLATE_EDGE_NORMAL_OFFSET: f32 = 0.005;

/// Color for plate boundary edges. Black, fully opaque.
pub const EDGE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

// ─── Gap Edge Rendering ─────────────────────────────────────────────────────

/// Half-width of gap indicator edge quads (wider than plate edges).
pub const GAP_EDGE_HALF_WIDTH: f32 = 0.006;

/// Normal offset for gap edges.
pub const GAP_EDGE_NORMAL_OFFSET: f32 = 0.008;

/// Color for gap indicator edges. Red.
pub const GAP_COLOR: [f32; 4] = [1.0, 0.15, 0.1, 1.0];

/// Maximum edge length before filtering out (mesh outer boundaries). World-space units.
pub const MAX_GAP_EDGE_LENGTH: f32 = 5.0;

/// Minimum gap width to display. Gaps narrower than this are filtered out.
/// Based on Småland's 120mm shell diameter (0.12 model units).
pub const MIN_GAP_WIDTH: f32 = 0.12;

// ─── Trajectory Overlay ─────────────────────────────────────────────────────

/// Normal offset for trajectory visualization overlays (z-fighting prevention).
pub const TRAJECTORY_NORMAL_OFFSET: f32 = 0.01;

/// Trajectory arc line width, multiplied by the camera-distance scale factor.
pub const TRAJECTORY_LINE_WIDTH_FACTOR: f32 = 0.12;

/// Hit marker diamond size, multiplied by the camera-distance scale factor.
pub const MARKER_SIZE_FACTOR: f32 = 0.15;

/// Detonation burst marker size, multiplied by the camera-distance scale factor.
pub const DETONATION_BURST_SIZE_FACTOR: f32 = 0.25;

//! Session state for the web client, mirroring the relevant parts of
//! the desktop `SessionState`.

use std::collections::HashMap;

use wt_collab_protocol::protocol::CollabRenderOptions;
use wt_collab_protocol::protocol::WireCapPoint;

use crate::types::Annotation;

// Re-export shared types so existing `crate::state::*` imports keep working.
pub use wt_collab_egui::types::MapPing;
pub use wt_collab_egui::types::Permissions;
pub use wt_collab_egui::types::UserCursor;

/// Connected user info.
#[derive(Clone)]
pub struct ConnectedUser {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
}

/// State for an open tactics board.
pub struct TacticsBoard {
    pub board_id: u64,
    pub map_name: String,
    /// Human-readable map name for display (sent by the host).
    pub display_name: String,
    pub map_id: u32,
    pub map_image_png: Option<Vec<u8>>,
    pub map_texture: Option<egui::TextureHandle>,
    pub map_info: Option<wows_minimap_renderer::map_data::MapInfo>,
    pub annotations: Vec<Annotation>,
    pub annotation_ids: Vec<u64>,
    pub annotation_owners: Vec<u64>,
    pub cap_points: Vec<WireCapPoint>,
}

/// State for an open replay.
pub struct ReplayView {
    pub replay_id: u64,
    pub replay_name: String,
    /// Human-readable translated map name for display.
    pub display_name: String,
    pub map_image_png: Option<Vec<u8>>,
    pub map_texture: Option<egui::TextureHandle>,
    pub annotations: Vec<Annotation>,
    pub annotation_ids: Vec<u64>,
    pub annotation_owners: Vec<u64>,
    pub current_frame: Option<FrameState>,
}

/// Current replay frame data.
pub struct FrameState {
    pub clock: f32,
    pub frame_index: u32,
    pub total_frames: u32,
    pub game_duration: f32,
    pub commands: Vec<wows_minimap_renderer::DrawCommand>,
}

/// Top-level session state.
pub struct SessionState {
    pub my_user_id: u64,
    pub host_user_id: u64,
    pub frame_source_id: u64,
    pub connected_users: Vec<ConnectedUser>,
    pub cursors: Vec<UserCursor>,
    pub pings: Vec<MapPing>,
    pub permissions: Permissions,
    pub render_options: Option<CollabRenderOptions>,
    pub replay_views: HashMap<u64, ReplayView>,
    pub tactics_boards: HashMap<u64, TacticsBoard>,
    pub active_view: ActiveView,
}

/// Which view is currently displayed.
pub enum ActiveView {
    /// No map/replay open yet — show lobby.
    Lobby,
    /// Showing a replay.
    Replay(u64),
    /// Showing a tactics board.
    TacticsBoard(u64),
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            my_user_id: 0,
            host_user_id: 0,
            frame_source_id: 0,
            connected_users: Vec::new(),
            cursors: Vec::new(),
            pings: Vec::new(),
            permissions: Permissions::default(),
            render_options: None,
            replay_views: HashMap::new(),
            tactics_boards: HashMap::new(),
            active_view: ActiveView::Lobby,
        }
    }
}

impl SessionState {
    /// Prune expired pings (older than 1 second).
    pub fn prune_pings(&mut self) {
        let now = web_time::Instant::now();
        self.pings.retain(|p| now.duration_since(p.time).as_secs_f32() < 1.0);
    }
}

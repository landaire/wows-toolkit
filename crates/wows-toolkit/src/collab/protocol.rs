//! Wire protocol for collaborative replay sessions (mesh topology, v2).
//!
//! All messages are serialized with rkyv and framed as `[u32 length][rkyv payload]`
//! on a QUIC bidirectional stream. Frame draw commands are additionally compressed
//! with zlib (flate2) before being placed in the `compressed_commands` field.

use crate::collab::types::Annotation;

/// ALPN protocol identifier for the collab session.
pub const COLLAB_ALPN: &[u8] = b"/wows-toolkit-collab/1";

/// Maximum total message size (16 MB — generous for SessionInfo with map PNG).
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Maximum compressed frame payload size (4 MB).
pub const MAX_FRAME_SIZE: usize = 4 * 1024 * 1024;

/// Maximum decompressed frame payload size (32 MB).
pub const MAX_DECOMPRESSED_FRAME_SIZE: usize = 32 * 1024 * 1024;

/// Maximum draw commands per frame.
pub const MAX_COMMANDS_PER_FRAME: usize = 5000;

/// Maximum annotations in a sync message.
pub const MAX_ANNOTATIONS: usize = 1000;

/// Maximum freehand stroke points per annotation.
pub const MAX_FREEHAND_POINTS: usize = 10_000;

/// Maximum string length for names and labels.
pub const MAX_STRING_LEN: usize = 200;

/// Maximum display name length.
pub const MAX_DISPLAY_NAME_LEN: usize = 50;

/// Maximum map image PNG size (10 MB).
pub const MAX_MAP_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Coordinate bounds for annotation positions and cursor locations.
/// The native minimap is 760x760 but annotations may extend slightly beyond.
pub const COORD_MIN: f32 = -2000.0;
pub const COORD_MAX: f32 = 2000.0;

/// Maximum annotation stroke width.
pub const MAX_STROKE_WIDTH: f32 = 100.0;

/// Maximum annotation radius.
pub const MAX_RADIUS: f32 = 2000.0;

/// Maximum peers in a session (bounded by the color palette).
pub const MAX_PEERS: usize = 12;

/// Maximum length of a serialized EndpointAddr JSON string.
pub const MAX_ENDPOINT_ADDR_LEN: usize = 4096;

/// Maximum cap points in a tactics board sync message.
pub const MAX_CAP_POINTS: usize = 50;

// ─── Wire cap point ─────────────────────────────────────────────────────────

/// A serializable capture point for tactics board collab sync.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct WireCapPoint {
    pub id: u64,
    pub index: u32,
    pub world_x: f32,
    pub world_z: f32,
    pub radius: f32,
    pub team_id: i64,
    pub frozen: bool,
}

// ─── Peer identity types ───────────────────────────────────────────────────

/// Information needed to connect to a peer in the mesh.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PeerInfo {
    pub user_id: u64,
    pub name: String,
    pub color: [u8; 3],
    /// Serialized `iroh::EndpointAddr` JSON for connecting to this peer.
    pub endpoint_addr_json: String,
}

/// A peer's assigned identity (user_id, name, color).
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PeerIdentity {
    pub user_id: u64,
    pub name: String,
    pub color: [u8; 3],
}

/// Metadata for an open replay in the session.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ReplayInfo {
    pub replay_id: u64,
    pub replay_name: String,
    pub map_image_png: Vec<u8>,
    pub game_version: String,
}

// ─── Unified peer message ──────────────────────────────────────────────────

/// Messages exchanged between any two peers in the mesh.
///
/// In the mesh topology, all participants run the same code. Message validity
/// depends on the sender's role, which is enforced by the *receiver* locally.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PeerMessage {
    // ── Handshake (joiner → host only) ──────────────────────────────────
    /// Join request sent to the host. Must be the first message on a host connection.
    Join { toolkit_version: String, name: String },

    /// Session metadata. Sent by host after accepting a Join.
    SessionInfo {
        toolkit_version: String,
        /// All currently connected peers (including the host).
        peers: Vec<PeerInfo>,
        /// The new joiner's assigned identity.
        assigned_identity: PeerIdentity,
        /// Who is currently the frame source (user_id).
        frame_source_id: u64,
        /// Currently open replays in the session.
        open_replays: Vec<ReplayInfo>,
    },

    /// Connection rejected by host (version mismatch, name invalid, etc.).
    Rejected { reason: String },

    // ── Mesh establishment ──────────────────────────────────────────────
    /// Host tells existing peers about a new joiner so they accept the incoming connection.
    PeerAnnounce { peer: PeerInfo },

    /// Sent by a new joiner to each existing peer to identify itself.
    MeshHello { user_id: u64, name: String, color: [u8; 3] },

    // ── Regular messages (any peer → all peers) ────────────────────────
    /// Cursor position on the minimap. None = cursor left the map area.
    CursorPosition(Option<[f32; 2]>),

    /// Upsert an annotation (add new or update existing) by unique ID.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    SetAnnotation { id: u64, annotation: Annotation, owner: u64 },

    /// Remove a specific annotation by ID.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    RemoveAnnotation { id: u64 },

    /// Remove all annotations.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    ClearAnnotations,

    /// Toggle a display option.
    /// Receiver drops if `settings_locked` and sender is not host/co-host.
    ToggleDisplayOption { field: DisplayOptionField, value: bool },

    /// Per-ship range override update.
    /// Receiver drops if `settings_locked` and sender is not host/co-host.
    /// Entries with no ranges enabled should be omitted (= hidden).
    ShipRangeOverrides {
        overrides: Vec<(wows_replays::types::EntityId, wows_minimap_renderer::draw_command::ShipConfigFilter)>,
    },

    /// Per-ship trail visibility override.
    /// Receiver drops if `settings_locked` and sender is not host/co-host.
    /// Contains the set of player names whose trails are hidden.
    ShipTrailOverrides { hidden: Vec<String> },

    /// Map ping — produces a ripple effect at the given position.
    Ping { pos: [f32; 2] },

    // ── Authority messages (host/co-host → all peers) ──────────────────
    /// Permission state change. Receiver drops if sender is not host/co-host.
    Permissions { annotations_locked: bool, settings_locked: bool },

    /// Current display settings. Receiver drops if sender is not host/co-host.
    RenderOptions(CollabRenderOptions),

    /// Full annotation state replacement. Receiver drops if sender is not host/co-host.
    AnnotationSync {
        annotations: Vec<Annotation>,
        /// Parallel vec: which user_id created each annotation.
        owners: Vec<u64>,
        /// Parallel vec: unique ID for each annotation.
        ids: Vec<u64>,
    },

    /// Playback state. Receiver drops if sender is not host/co-host.
    PlaybackState { playing: bool, speed: f32 },

    /// A user joined the session. Only the host sends this.
    UserJoined { user_id: u64, name: String, color: [u8; 3] },

    /// A user left the session. Only the host sends this.
    UserLeft { user_id: u64 },

    // ── Co-host promotion (host only → all peers) ──────────────────────
    /// Promote a peer to co-host. Only valid from the original host.
    /// Receiver drops if sender is not `host_user_id`.
    PromoteToCoHost { user_id: u64 },

    // ── Frame sourcing (host/co-host → all peers) ──────────────────────
    /// Declares that a specific user is now the frame source.
    /// Receiver drops if sender is not host/co-host.
    FrameSourceChanged { source_user_id: u64 },

    /// A single playback frame with compressed draw commands.
    /// Receiver drops if sender is not the current frame source.
    Frame {
        replay_id: u64,
        clock: f32,
        frame_index: u32,
        total_frames: u32,
        game_duration: f32,
        /// `flate2::write::ZlibEncoder(rkyv::to_bytes(Vec<DrawCommand>))`
        compressed_commands: Vec<u8>,
    },

    // ── Replay lifecycle (host → all peers) ─────────────────────────────
    /// A new replay was opened on the host.
    ReplayOpened {
        replay_id: u64,
        replay_name: String,
        /// PNG-encoded map background image.
        map_image_png: Vec<u8>,
        game_version: String,
    },

    /// A replay was closed on the host.
    ReplayClosed { replay_id: u64 },

    // ── Tactics board (any peer → all peers) ────────────────────────────
    /// A tactics board was opened with a specific map.
    /// Receiver loads the same map if they have a tactics board open.
    TacticsMapOpened {
        map_name: String,
        map_id: u32,
        /// PNG-encoded map background image.
        map_image_png: Vec<u8>,
        /// Map metadata for coordinate transforms.
        map_info: Option<wows_minimap_renderer::map_data::MapInfo>,
    },

    /// The tactics board was closed.
    TacticsMapClosed,

    /// Upsert a cap point on the tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    SetCapPoint(WireCapPoint),

    /// Remove a cap point from the tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    RemoveCapPoint { id: u64 },

    /// Full cap point state sync (used after undo or bulk operations).
    /// Receiver drops if sender is not host/co-host.
    CapPointSync { cap_points: Vec<WireCapPoint> },
}

// ─── Display option field enum ──────────────────────────────────────────────

/// Exhaustive enum of toggleable display options.
///
/// Using an enum instead of a string prevents typos and makes validation
/// trivial — rkyv deserialization rejects unknown variants automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum DisplayOptionField {
    ShowHpBars,
    ShowTracers,
    ShowTorpedoes,
    ShowPlanes,
    ShowSmoke,
    ShowScore,
    ShowTimer,
    ShowKillFeed,
    ShowPlayerNames,
    ShowShipNames,
    ShowCapturePoints,
    ShowBuildings,
    ShowTurretDirection,
    ShowConsumables,
    ShowDeadShips,
    ShowDeadShipNames,
    ShowArmament,
    ShowTrails,
    ShowDeadTrails,
    ShowSpeedTrails,
    ShowBattleResult,
    ShowBuffs,
    ShowShipConfig,
    ShowChat,
    ShowAdvantage,
    ShowScoreTimer,
    // Self-range toggles
    ShowSelfDetection,
    ShowSelfMainBattery,
    ShowSelfSecondary,
    ShowSelfTorpedo,
    ShowSelfRadar,
    ShowSelfHydro,
}

// ─── Serializable render options ────────────────────────────────────────────

/// Serializable subset of render options for network sync.
///
/// Excludes `prefer_cpu_encoder` (local-only setting).
#[derive(Debug, Clone, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CollabRenderOptions {
    pub show_hp_bars: bool,
    pub show_tracers: bool,
    pub show_torpedoes: bool,
    pub show_planes: bool,
    pub show_smoke: bool,
    pub show_score: bool,
    pub show_timer: bool,
    pub show_kill_feed: bool,
    pub show_player_names: bool,
    pub show_ship_names: bool,
    pub show_capture_points: bool,
    pub show_buildings: bool,
    pub show_turret_direction: bool,
    pub show_consumables: bool,
    pub show_dead_ships: bool,
    pub show_dead_ship_names: bool,
    pub show_armament: bool,
    pub show_trails: bool,
    pub show_dead_trails: bool,
    pub show_speed_trails: bool,
    pub show_battle_result: bool,
    pub show_buffs: bool,
    pub show_ship_config: bool,
    pub show_chat: bool,
    pub show_advantage: bool,
    pub show_score_timer: bool,
    pub show_self_detection_range: bool,
    pub show_self_main_battery_range: bool,
    pub show_self_secondary_range: bool,
    pub show_self_torpedo_range: bool,
    pub show_self_radar_range: bool,
    pub show_self_hydro_range: bool,
}

impl CollabRenderOptions {
    /// Convert from the persisted settings format.
    pub fn from_saved(s: &crate::settings::SavedRenderOptions) -> Self {
        Self {
            show_hp_bars: s.show_hp_bars,
            show_tracers: s.show_tracers,
            show_torpedoes: s.show_torpedoes,
            show_planes: s.show_planes,
            show_smoke: s.show_smoke,
            show_score: s.show_score,
            show_timer: s.show_timer,
            show_kill_feed: s.show_kill_feed,
            show_player_names: s.show_player_names,
            show_ship_names: s.show_ship_names,
            show_capture_points: s.show_capture_points,
            show_buildings: s.show_buildings,
            show_turret_direction: s.show_turret_direction,
            show_consumables: s.show_consumables,
            show_dead_ships: s.show_dead_ships,
            show_dead_ship_names: s.show_dead_ship_names,
            show_armament: s.show_armament,
            show_trails: s.show_trails,
            show_dead_trails: s.show_dead_trails,
            show_speed_trails: s.show_speed_trails,
            show_battle_result: s.show_battle_result,
            show_buffs: s.show_buffs,
            show_ship_config: s.show_ship_config,
            show_chat: s.show_chat,
            show_advantage: s.show_advantage,
            show_score_timer: s.show_score_timer,
            show_self_detection_range: s.show_self_detection_range,
            show_self_main_battery_range: s.show_self_main_battery_range,
            show_self_secondary_range: s.show_self_secondary_range,
            show_self_torpedo_range: s.show_self_torpedo_range,
            show_self_radar_range: s.show_self_radar_range,
            show_self_hydro_range: s.show_self_hydro_range,
        }
    }

    /// Build from a `RenderOptions` and the separate `show_dead_ships` flag.
    pub fn from_render_options(opts: &wows_minimap_renderer::renderer::RenderOptions, show_dead_ships: bool) -> Self {
        Self {
            show_hp_bars: opts.show_hp_bars,
            show_tracers: opts.show_tracers,
            show_torpedoes: opts.show_torpedoes,
            show_planes: opts.show_planes,
            show_smoke: opts.show_smoke,
            show_score: opts.show_score,
            show_timer: opts.show_timer,
            show_kill_feed: opts.show_kill_feed,
            show_player_names: opts.show_player_names,
            show_ship_names: opts.show_ship_names,
            show_capture_points: opts.show_capture_points,
            show_buildings: opts.show_buildings,
            show_turret_direction: opts.show_turret_direction,
            show_consumables: opts.show_consumables,
            show_dead_ships,
            show_dead_ship_names: opts.show_dead_ship_names,
            show_armament: opts.show_armament,
            show_trails: opts.show_trails,
            show_dead_trails: opts.show_dead_trails,
            show_speed_trails: opts.show_speed_trails,
            show_battle_result: opts.show_battle_result,
            show_buffs: opts.show_buffs,
            show_ship_config: opts.show_ship_config,
            show_chat: opts.show_chat,
            show_advantage: opts.show_advantage,
            show_score_timer: opts.show_score_timer,
            // Self-range fields are not part of RenderOptions; default to false.
            show_self_detection_range: false,
            show_self_main_battery_range: false,
            show_self_secondary_range: false,
            show_self_torpedo_range: false,
            show_self_radar_range: false,
            show_self_hydro_range: false,
        }
    }

    /// Return the list of `(field, value)` pairs that differ between `self` and `other`.
    pub fn diff(&self, other: &Self) -> Vec<(DisplayOptionField, bool)> {
        let mut out = Vec::new();
        macro_rules! cmp {
            ($field:ident, $variant:ident) => {
                if self.$field != other.$field {
                    out.push((DisplayOptionField::$variant, other.$field));
                }
            };
        }
        cmp!(show_hp_bars, ShowHpBars);
        cmp!(show_tracers, ShowTracers);
        cmp!(show_torpedoes, ShowTorpedoes);
        cmp!(show_planes, ShowPlanes);
        cmp!(show_smoke, ShowSmoke);
        cmp!(show_score, ShowScore);
        cmp!(show_timer, ShowTimer);
        cmp!(show_kill_feed, ShowKillFeed);
        cmp!(show_player_names, ShowPlayerNames);
        cmp!(show_ship_names, ShowShipNames);
        cmp!(show_capture_points, ShowCapturePoints);
        cmp!(show_buildings, ShowBuildings);
        cmp!(show_turret_direction, ShowTurretDirection);
        cmp!(show_consumables, ShowConsumables);
        cmp!(show_dead_ships, ShowDeadShips);
        cmp!(show_dead_ship_names, ShowDeadShipNames);
        cmp!(show_armament, ShowArmament);
        cmp!(show_trails, ShowTrails);
        cmp!(show_dead_trails, ShowDeadTrails);
        cmp!(show_speed_trails, ShowSpeedTrails);
        cmp!(show_battle_result, ShowBattleResult);
        cmp!(show_buffs, ShowBuffs);
        cmp!(show_ship_config, ShowShipConfig);
        cmp!(show_chat, ShowChat);
        cmp!(show_advantage, ShowAdvantage);
        cmp!(show_score_timer, ShowScoreTimer);
        cmp!(show_self_detection_range, ShowSelfDetection);
        cmp!(show_self_main_battery_range, ShowSelfMainBattery);
        cmp!(show_self_secondary_range, ShowSelfSecondary);
        cmp!(show_self_torpedo_range, ShowSelfTorpedo);
        cmp!(show_self_radar_range, ShowSelfRadar);
        cmp!(show_self_hydro_range, ShowSelfHydro);
        out
    }

    /// Apply a display option toggle by field.
    pub fn set_field(&mut self, field: DisplayOptionField, value: bool) {
        match field {
            DisplayOptionField::ShowHpBars => self.show_hp_bars = value,
            DisplayOptionField::ShowTracers => self.show_tracers = value,
            DisplayOptionField::ShowTorpedoes => self.show_torpedoes = value,
            DisplayOptionField::ShowPlanes => self.show_planes = value,
            DisplayOptionField::ShowSmoke => self.show_smoke = value,
            DisplayOptionField::ShowScore => self.show_score = value,
            DisplayOptionField::ShowTimer => self.show_timer = value,
            DisplayOptionField::ShowKillFeed => self.show_kill_feed = value,
            DisplayOptionField::ShowPlayerNames => self.show_player_names = value,
            DisplayOptionField::ShowShipNames => self.show_ship_names = value,
            DisplayOptionField::ShowCapturePoints => self.show_capture_points = value,
            DisplayOptionField::ShowBuildings => self.show_buildings = value,
            DisplayOptionField::ShowTurretDirection => self.show_turret_direction = value,
            DisplayOptionField::ShowConsumables => self.show_consumables = value,
            DisplayOptionField::ShowDeadShips => self.show_dead_ships = value,
            DisplayOptionField::ShowDeadShipNames => self.show_dead_ship_names = value,
            DisplayOptionField::ShowArmament => self.show_armament = value,
            DisplayOptionField::ShowTrails => self.show_trails = value,
            DisplayOptionField::ShowDeadTrails => self.show_dead_trails = value,
            DisplayOptionField::ShowSpeedTrails => self.show_speed_trails = value,
            DisplayOptionField::ShowBattleResult => self.show_battle_result = value,
            DisplayOptionField::ShowBuffs => self.show_buffs = value,
            DisplayOptionField::ShowShipConfig => self.show_ship_config = value,
            DisplayOptionField::ShowChat => self.show_chat = value,
            DisplayOptionField::ShowAdvantage => self.show_advantage = value,
            DisplayOptionField::ShowScoreTimer => self.show_score_timer = value,
            DisplayOptionField::ShowSelfDetection => self.show_self_detection_range = value,
            DisplayOptionField::ShowSelfMainBattery => self.show_self_main_battery_range = value,
            DisplayOptionField::ShowSelfSecondary => self.show_self_secondary_range = value,
            DisplayOptionField::ShowSelfTorpedo => self.show_self_torpedo_range = value,
            DisplayOptionField::ShowSelfRadar => self.show_self_radar_range = value,
            DisplayOptionField::ShowSelfHydro => self.show_self_hydro_range = value,
        }
    }
}

// ─── Wire framing helpers ───────────────────────────────────────────────────

/// Serialize a value to length-prefixed rkyv bytes.
///
/// Returns `[u32 LE length][rkyv payload]`.
fn frame_message(bytes: &rkyv::util::AlignedVec) -> Vec<u8> {
    let len = bytes.len() as u32;
    let mut framed = Vec::with_capacity(4 + bytes.len());
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(bytes);
    framed
}

/// Write a length-prefixed rkyv-serialized `PeerMessage` to a QUIC send stream.
pub async fn write_peer_message(
    send: &mut iroh::endpoint::SendStream,
    msg: &PeerMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(msg).map_err(|e| format!("rkyv serialize: {e}"))?;
    if bytes.len() > MAX_MESSAGE_SIZE {
        tracing::error!("Outgoing message too large: {} bytes > {} max", bytes.len(), MAX_MESSAGE_SIZE,);
        return Err(format!("outgoing message too large: {} > {}", bytes.len(), MAX_MESSAGE_SIZE,).into());
    }
    let framed = frame_message(&bytes);
    send.write_all(&framed).await.map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Serialize a `PeerMessage` to length-prefixed rkyv bytes (for broadcast).
pub fn frame_peer_message(msg: &PeerMessage) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(msg).map_err(|e| format!("rkyv serialize: {e}"))?;
    if bytes.len() > MAX_MESSAGE_SIZE {
        tracing::error!("Outgoing broadcast message too large: {} bytes > {} max", bytes.len(), MAX_MESSAGE_SIZE,);
        return Err(format!("outgoing message too large: {} > {}", bytes.len(), MAX_MESSAGE_SIZE,).into());
    }
    Ok(frame_message(&bytes))
}

/// Read a length-prefixed rkyv `PeerMessage` from a QUIC receive stream.
///
/// Returns `None` if the stream is cleanly closed.
pub async fn read_peer_message(
    recv: &mut iroh::endpoint::RecvStream,
    max_size: usize,
) -> Result<Option<PeerMessage>, Box<dyn std::error::Error + Send + Sync>> {
    read_framed_message(recv, max_size).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── CollabRenderOptions default ─────────────────────────────────────

    #[test]
    fn render_options_default_all_false() {
        let opts = CollabRenderOptions::default();
        assert!(!opts.show_hp_bars);
        assert!(!opts.show_tracers);
        assert!(!opts.show_torpedoes);
        assert!(!opts.show_planes);
        assert!(!opts.show_smoke);
        assert!(!opts.show_score);
        assert!(!opts.show_timer);
        assert!(!opts.show_kill_feed);
        assert!(!opts.show_player_names);
        assert!(!opts.show_ship_names);
        assert!(!opts.show_capture_points);
        assert!(!opts.show_buildings);
        assert!(!opts.show_turret_direction);
        assert!(!opts.show_consumables);
        assert!(!opts.show_dead_ships);
        assert!(!opts.show_dead_ship_names);
        assert!(!opts.show_armament);
        assert!(!opts.show_trails);
        assert!(!opts.show_dead_trails);
        assert!(!opts.show_speed_trails);
        assert!(!opts.show_battle_result);
        assert!(!opts.show_buffs);
        assert!(!opts.show_ship_config);
        assert!(!opts.show_chat);
        assert!(!opts.show_advantage);
        assert!(!opts.show_score_timer);
        assert!(!opts.show_self_detection_range);
        assert!(!opts.show_self_main_battery_range);
        assert!(!opts.show_self_secondary_range);
        assert!(!opts.show_self_torpedo_range);
        assert!(!opts.show_self_radar_range);
        assert!(!opts.show_self_hydro_range);
    }

    // ─── set_field ───────────────────────────────────────────────────────

    #[test]
    fn set_field_sets_correct_field() {
        let mut opts = CollabRenderOptions::default();
        opts.set_field(DisplayOptionField::ShowHpBars, true);
        assert!(opts.show_hp_bars);
        assert!(!opts.show_tracers);

        opts.set_field(DisplayOptionField::ShowSelfRadar, true);
        assert!(opts.show_self_radar_range);

        opts.set_field(DisplayOptionField::ShowHpBars, false);
        assert!(!opts.show_hp_bars);
    }

    #[test]
    fn set_field_all_variants() {
        // Verify every DisplayOptionField variant can be set and read back.
        let fields = [
            DisplayOptionField::ShowHpBars,
            DisplayOptionField::ShowTracers,
            DisplayOptionField::ShowTorpedoes,
            DisplayOptionField::ShowPlanes,
            DisplayOptionField::ShowSmoke,
            DisplayOptionField::ShowScore,
            DisplayOptionField::ShowTimer,
            DisplayOptionField::ShowKillFeed,
            DisplayOptionField::ShowPlayerNames,
            DisplayOptionField::ShowShipNames,
            DisplayOptionField::ShowCapturePoints,
            DisplayOptionField::ShowBuildings,
            DisplayOptionField::ShowTurretDirection,
            DisplayOptionField::ShowConsumables,
            DisplayOptionField::ShowDeadShips,
            DisplayOptionField::ShowDeadShipNames,
            DisplayOptionField::ShowArmament,
            DisplayOptionField::ShowTrails,
            DisplayOptionField::ShowDeadTrails,
            DisplayOptionField::ShowSpeedTrails,
            DisplayOptionField::ShowBattleResult,
            DisplayOptionField::ShowBuffs,
            DisplayOptionField::ShowShipConfig,
            DisplayOptionField::ShowChat,
            DisplayOptionField::ShowAdvantage,
            DisplayOptionField::ShowScoreTimer,
            DisplayOptionField::ShowSelfDetection,
            DisplayOptionField::ShowSelfMainBattery,
            DisplayOptionField::ShowSelfSecondary,
            DisplayOptionField::ShowSelfTorpedo,
            DisplayOptionField::ShowSelfRadar,
            DisplayOptionField::ShowSelfHydro,
        ];

        for field in fields {
            let mut opts = CollabRenderOptions::default();
            opts.set_field(field, true);
            // Verify exactly one field changed via diff.
            let diff = CollabRenderOptions::default().diff(&opts);
            assert_eq!(diff.len(), 1, "Expected exactly 1 diff for {:?}", field);
            assert_eq!(diff[0].0, field);
            assert!(diff[0].1);
        }
    }

    // ─── diff ────────────────────────────────────────────────────────────

    #[test]
    fn diff_identical_returns_empty() {
        let a = CollabRenderOptions::default();
        let b = CollabRenderOptions::default();
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn diff_single_change() {
        let a = CollabRenderOptions::default();
        let mut b = CollabRenderOptions::default();
        b.show_tracers = true;
        let diff = a.diff(&b);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], (DisplayOptionField::ShowTracers, true));
    }

    #[test]
    fn diff_multiple_changes() {
        let a = CollabRenderOptions::default();
        let mut b = CollabRenderOptions::default();
        b.show_tracers = true;
        b.show_chat = true;
        b.show_self_hydro_range = true;
        let diff = a.diff(&b);
        assert_eq!(diff.len(), 3);
        let fields: Vec<_> = diff.iter().map(|(f, _)| *f).collect();
        assert!(fields.contains(&DisplayOptionField::ShowTracers));
        assert!(fields.contains(&DisplayOptionField::ShowChat));
        assert!(fields.contains(&DisplayOptionField::ShowSelfHydro));
    }

    #[test]
    fn diff_detects_false_to_true_and_true_to_false() {
        let mut a = CollabRenderOptions::default();
        a.show_hp_bars = true;
        let b = CollabRenderOptions::default();
        // a has show_hp_bars=true, b has false
        let diff = a.diff(&b);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], (DisplayOptionField::ShowHpBars, false));
    }

    // ─── Message framing ─────────────────────────────────────────────────

    /// Copy payload into an AlignedVec for rkyv deserialization (mirrors production read path).
    fn deserialize_payload(framed: &[u8]) -> PeerMessage {
        let len = u32::from_le_bytes(framed[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, framed.len() - 4);
        let payload = &framed[4..];
        let mut aligned = rkyv::util::AlignedVec::<16>::new();
        aligned.extend_from_slice(payload);
        rkyv::from_bytes::<PeerMessage, rkyv::rancor::Error>(&aligned).unwrap()
    }

    #[test]
    fn frame_peer_message_roundtrip() {
        let msg = PeerMessage::CursorPosition(Some([100.0, 200.0]));
        let framed = frame_peer_message(&msg).unwrap();

        // Should start with a 4-byte LE length prefix.
        assert!(framed.len() > 4);

        let decoded = deserialize_payload(&framed);
        match decoded {
            PeerMessage::CursorPosition(Some(pos)) => {
                assert!((pos[0] - 100.0).abs() < f32::EPSILON);
                assert!((pos[1] - 200.0).abs() < f32::EPSILON);
            }
            _ => panic!("Expected CursorPosition(Some(...))"),
        }
    }

    #[test]
    fn frame_peer_message_various_types() {
        // Verify several message types can be framed without error.
        let messages = vec![
            PeerMessage::Join { toolkit_version: "1.0".into(), name: "Test".into() },
            PeerMessage::CursorPosition(None),
            PeerMessage::ClearAnnotations,
            PeerMessage::Permissions { annotations_locked: true, settings_locked: false },
            PeerMessage::UserLeft { user_id: 42 },
            PeerMessage::PromoteToCoHost { user_id: 3 },
            PeerMessage::Ping { pos: [100.0, 200.0] },
            PeerMessage::RemoveAnnotation { id: 999 },
            PeerMessage::PlaybackState { playing: true, speed: 1.5 },
        ];
        for msg in &messages {
            assert!(frame_peer_message(msg).is_ok(), "Failed to frame: {:?}", msg);
        }
    }

    #[test]
    fn frame_annotation_sync_roundtrip() {
        let msg = PeerMessage::AnnotationSync {
            annotations: vec![Annotation::Circle {
                center: [100.0, 200.0],
                radius: 50.0,
                color: [255, 0, 0, 128],
                width: 3.0,
                filled: true,
            }],
            owners: vec![0],
            ids: vec![42],
        };
        let framed = frame_peer_message(&msg).unwrap();
        let decoded = deserialize_payload(&framed);
        match decoded {
            PeerMessage::AnnotationSync { annotations, owners, ids } => {
                assert_eq!(annotations.len(), 1);
                assert_eq!(owners, vec![0]);
                assert_eq!(ids, vec![42]);
            }
            _ => panic!("Expected AnnotationSync"),
        }
    }

    // ─── Constants sanity ────────────────────────────────────────────────

    #[test]
    fn constants_are_reasonable() {
        assert!(MAX_MESSAGE_SIZE > MAX_FRAME_SIZE);
        assert!(MAX_DECOMPRESSED_FRAME_SIZE > MAX_FRAME_SIZE);
        assert!(MAX_DISPLAY_NAME_LEN <= MAX_STRING_LEN);
        assert!(COORD_MIN < COORD_MAX);
        assert!(MAX_PEERS >= 2);
        assert!(MAX_ANNOTATIONS > 0);
        assert!(MAX_FREEHAND_POINTS > 0);
    }
}

/// Read raw bytes from the stream then deserialize with rkyv.
async fn read_framed_message<T>(
    recv: &mut iroh::endpoint::RecvStream,
    max_size: usize,
) -> Result<Option<T>, Box<dyn std::error::Error + Send + Sync>>
where
    T: rkyv::Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::Error>>
        + rkyv::Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::Error>>,
{
    // Read 4-byte length prefix.
    let mut len_buf = [0u8; 4];
    if let Err(e) = read_exact_chunked(recv, &mut len_buf).await {
        let msg = e.to_string();
        if msg.contains("closed") || msg.contains("finished") || msg.contains("reset") {
            return Ok(None);
        }
        return Err(format!("read len: {e}").into());
    }

    let len = u32::from_le_bytes(len_buf) as usize;
    if len > max_size {
        return Err(format!("message too large: {len} > {max_size}").into());
    }

    // Read payload using chunked reads.
    let mut buf = vec![0u8; len];
    read_exact_chunked(recv, &mut buf).await.map_err(|e| format!("read payload ({len} bytes): {e}"))?;

    let msg = rkyv::from_bytes::<T, rkyv::rancor::Error>(&buf).map_err(|e| format!("rkyv deserialize: {e}"))?;
    Ok(Some(msg))
}

/// Read exactly `buf.len()` bytes using chunked reads.
async fn read_exact_chunked(
    recv: &mut iroh::endpoint::RecvStream,
    buf: &mut [u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let total = buf.len();
    let mut pos = 0;
    while pos < total {
        match recv.read(&mut buf[pos..]).await {
            Ok(Some(n)) => {
                if n == 0 {
                    return Err(format!("unexpected zero-length read at byte {pos}/{total}").into());
                }
                pos += n;
            }
            Ok(None) => {
                return Err(format!("stream ended at byte {pos}/{total}").into());
            }
            Err(e) => {
                return Err(format!("read error at byte {pos}/{total}: {e}").into());
            }
        }
    }
    Ok(())
}

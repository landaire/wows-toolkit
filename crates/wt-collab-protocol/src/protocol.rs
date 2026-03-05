//! Wire protocol for collaborative replay sessions (mesh topology).
//!
//! All messages are serialized with rkyv, zlib-compressed, and framed as
//! `[u32 compressed_length][zlib(rkyv payload)]` on a QUIC bidirectional stream.

use crate::types::Annotation;

// Re-export types from external crates used in the protocol.
pub use wows_minimap_renderer::draw_command::DrawCommand;
pub use wows_minimap_renderer::draw_command::ShipConfigFilter;
pub use wows_minimap_renderer::map_data::MapInfo;
pub use wowsunpack::game_types::EntityId;

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

/// Maximum tactics boards in a session.
pub const MAX_TACTICS_BOARDS: usize = 4;

/// Heartbeat send interval in seconds.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// Heartbeat timeout in seconds (3 missed heartbeats).
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 30;

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

// ─── Client type ────────────────────────────────────────────────────────────

/// Identifies the type of client connecting to the session.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum ClientType {
    /// Desktop toolkit client with a specific version.
    Desktop { toolkit_version: String },
    /// Web browser client (WASM).
    Web,
}

// ─── Asset bundle types ─────────────────────────────────────────────────────

/// A single RGBA image asset for wire transport.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RgbaAssetWire {
    /// Raw RGBA pixel data.
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Game font data for wire transport.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct GameFontsWire {
    /// Primary game font (TTF bytes).
    pub primary: Vec<u8>,
    /// Optional Korean fallback font (TTF bytes).
    pub fallback_ko: Option<Vec<u8>>,
    /// Optional Japanese fallback font (TTF bytes).
    pub fallback_ja: Option<Vec<u8>>,
    /// Optional Chinese fallback font (TTF bytes).
    pub fallback_zh: Option<Vec<u8>>,
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
    /// Raw map name (e.g. "spaces/16_OC_bees_to_honey").
    pub map_name: String,
    /// Human-readable translated map name for display.
    pub display_name: String,
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
    Join { name: String, client_type: ClientType },

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
    CursorPosition { user_id: u64, pos: Option<[f32; 2]> },

    /// Upsert an annotation (add new or update existing) by unique ID.
    /// `board_id`: `None` = replay context, `Some(id)` = tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    SetAnnotation { board_id: Option<u64>, id: u64, annotation: Annotation, owner: u64 },

    /// Remove a specific annotation by ID.
    /// `board_id`: `None` = replay context, `Some(id)` = tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    RemoveAnnotation { board_id: Option<u64>, id: u64 },

    /// Remove all annotations.
    /// `board_id`: `None` = replay context, `Some(id)` = tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    ClearAnnotations { board_id: Option<u64> },

    /// Toggle a display option.
    /// Receiver drops if `settings_locked` and sender is not host/co-host.
    ToggleDisplayOption { field: DisplayOptionField, value: bool },

    /// Per-ship range override update.
    /// Receiver drops if `settings_locked` and sender is not host/co-host.
    /// Entries with no ranges enabled should be omitted (= hidden).
    ShipRangeOverrides { overrides: Vec<(EntityId, ShipConfigFilter)> },

    /// Per-ship trail visibility override.
    /// Receiver drops if `settings_locked` and sender is not host/co-host.
    /// Contains the set of player names whose trails are hidden.
    ShipTrailOverrides { hidden: Vec<String> },

    /// Map ping — produces a ripple effect at the given position.
    Ping { user_id: u64, pos: [f32; 2], color: [u8; 3] },

    // ── Authority messages (host/co-host → all peers) ──────────────────
    /// Permission state change. Receiver drops if sender is not host/co-host.
    Permissions { annotations_locked: bool, settings_locked: bool },

    /// Current display settings. Receiver drops if sender is not host/co-host.
    RenderOptions(CollabRenderOptions),

    /// Full annotation state replacement. Receiver drops if sender is not host/co-host.
    /// `board_id`: `None` = replay context, `Some(id)` = tactics board.
    AnnotationSync {
        board_id: Option<u64>,
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

    /// A single playback frame with draw commands.
    /// Receiver drops if sender is not the current frame source.
    Frame {
        replay_id: u64,
        clock: f32,
        frame_index: u32,
        total_frames: u32,
        game_duration: f32,
        commands: Vec<DrawCommand>,
    },

    // ── Replay lifecycle (host → all peers) ─────────────────────────────
    /// A new replay was opened on the host.
    ReplayOpened {
        replay_id: u64,
        replay_name: String,
        /// PNG-encoded map background image.
        map_image_png: Vec<u8>,
        game_version: String,
        /// Raw map name (e.g. "spaces/16_OC_bees_to_honey").
        map_name: String,
        /// Human-readable translated map name for display.
        display_name: String,
    },

    /// A replay was closed on the host.
    ReplayClosed { replay_id: u64 },

    // ── Tactics board (any peer → all peers) ────────────────────────────
    /// A tactics board was opened with a specific map.
    TacticsMapOpened {
        board_id: u64,
        owner_user_id: u64,
        map_name: String,
        /// Human-readable map name for display (translated or prettified by the host).
        display_name: String,
        map_id: u32,
        /// PNG-encoded map background image.
        map_image_png: Vec<u8>,
        /// Map metadata for coordinate transforms.
        map_info: Option<MapInfo>,
    },

    /// A tactics board was closed.
    TacticsMapClosed { board_id: u64 },

    /// Upsert a cap point on a tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    SetCapPoint { board_id: u64, cap_point: WireCapPoint },

    /// Remove a cap point from a tactics board.
    /// Receiver drops if `annotations_locked` and sender is not host/co-host.
    RemoveCapPoint { board_id: u64, id: u64 },

    /// Full cap point state sync for a tactics board (used after undo or bulk operations).
    /// Receiver drops if sender is not host/co-host.
    CapPointSync { board_id: u64, cap_points: Vec<WireCapPoint> },

    // ── Session window management (host/co-host → all peers) ─────────
    /// Request all peers to open a specific session window (replay or tactics board).
    /// The `window_id` matches either a `replay_id` in `open_replays` or a `board_id`
    /// in `tactics_boards`. Peers respect their `disable_auto_open_session_windows` setting.
    OpenWindowForEveryone { window_id: u64 },

    // ── Asset delivery (host → web clients) ─────────────────────────────
    /// Request from a client to the host to (re-)send the AssetBundle.
    /// Used when assets were not received after connecting or on reconnection.
    /// Host may ignore if too many requests have been made by this peer.
    RequestAssets,

    /// Asset bundle sent by the host to web clients after SessionInfo.
    /// Contains all icons and fonts needed for rendering.
    AssetBundle {
        /// Ship icons keyed by name (e.g. "Destroyer", "minimap_Destroyer_dead").
        ship_icons: Vec<(String, RgbaAssetWire)>,
        /// Plane icons keyed by name (e.g. "fighter_he_ally").
        plane_icons: Vec<(String, RgbaAssetWire)>,
        /// Consumable icons keyed by PCY name (e.g. "PCY015_SpeedBoosterPremium").
        consumable_icons: Vec<(String, RgbaAssetWire)>,
        /// Death cause icons keyed by name (e.g. "main_caliber").
        death_cause_icons: Vec<(String, RgbaAssetWire)>,
        /// Powerup icons keyed by name.
        powerup_icons: Vec<(String, RgbaAssetWire)>,
        /// Game fonts (TTF bytes).
        game_fonts: Option<GameFontsWire>,
    },

    // ── Connection keepalive ──────────────────────────────────────────
    /// Heartbeat keepalive. Sent every 10s; NOT relayed between peers.
    Heartbeat,
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

/// Serialize a `PeerMessage` to length-prefixed zlib-compressed rkyv bytes.
///
/// Returns `[u32 LE compressed_length][zlib(rkyv payload)]`.
pub fn serialize_message(msg: &PeerMessage) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(msg).map_err(|e| format!("rkyv serialize: {e}"))?;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&bytes).map_err(|e| format!("zlib compress: {e}"))?;
    let compressed = encoder.finish().map_err(|e| format!("zlib finish: {e}"))?;

    if compressed.len() > MAX_MESSAGE_SIZE {
        return Err(
            format!("outgoing message too large (compressed): {} > {}", compressed.len(), MAX_MESSAGE_SIZE).into()
        );
    }
    let len = compressed.len() as u32;
    let mut framed = Vec::with_capacity(4 + compressed.len());
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(&compressed);
    Ok(framed)
}

/// Deserialize a `PeerMessage` from zlib-compressed rkyv payload bytes (no length prefix).
pub fn deserialize_message(buf: &[u8]) -> Result<PeerMessage, Box<dyn std::error::Error + Send + Sync>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(buf);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| format!("zlib decompress: {e}"))?;

    if decompressed.len() > MAX_DECOMPRESSED_FRAME_SIZE {
        return Err(format!(
            "decompressed message too large: {} > {}",
            decompressed.len(),
            MAX_DECOMPRESSED_FRAME_SIZE
        )
        .into());
    }

    rkyv::from_bytes::<PeerMessage, rkyv::rancor::Error>(&decompressed)
        .map_err(|e| format!("rkyv deserialize: {e}").into())
}

/// Alias for [`serialize_message`] kept for backward compatibility.
pub fn frame_peer_message(msg: &PeerMessage) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    serialize_message(msg)
}

// ─── Stream I/O helpers ─────────────────────────────────────────────────────

/// Write a length-prefixed compressed `PeerMessage` to a QUIC send stream.
pub async fn write_peer_message(
    send: &mut iroh::endpoint::SendStream,
    msg: &PeerMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let framed = serialize_message(msg)?;
    send.write_all(&framed).await.map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Read a length-prefixed compressed `PeerMessage` from a QUIC receive stream.
///
/// Returns `None` if the stream is cleanly closed.
pub async fn read_peer_message(
    recv: &mut iroh::endpoint::RecvStream,
    max_size: usize,
) -> Result<Option<PeerMessage>, Box<dyn std::error::Error + Send + Sync>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    // Read 4-byte length prefix (compressed size).
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

    // Read compressed payload using chunked reads.
    let mut buf = vec![0u8; len];
    read_exact_chunked(recv, &mut buf).await.map_err(|e| format!("read payload ({len} bytes): {e}"))?;

    // Decompress.
    let mut decoder = ZlibDecoder::new(&buf[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| format!("zlib decompress: {e}"))?;

    let msg = rkyv::from_bytes::<PeerMessage, rkyv::rancor::Error>(&decompressed)
        .map_err(|e| format!("rkyv deserialize: {e}"))?;
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

// ─── Token encoding ─────────────────────────────────────────────────────────

const TOKEN_PREFIX: &str = "toolkit-";

/// Encode a public key into a session token: `toolkit-<base64url_nopad(zlib(key_bytes))>`.
///
/// Encodes the raw 32-byte public key (not the string representation) for compact tokens.
pub fn encode_token(public_key: &iroh::PublicKey) -> String {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(public_key.as_bytes()).expect("zlib write");
    let compressed = encoder.finish().expect("zlib finish");
    let b64 = data_encoding::BASE64URL_NOPAD.encode(&compressed);
    format!("{TOKEN_PREFIX}{b64}")
}

/// Decode a session token back to a public key.
pub fn decode_token(token: &str) -> Result<iroh::PublicKey, String> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let token = token.trim();
    let b64 = token.strip_prefix(TOKEN_PREFIX).ok_or_else(|| format!("Token must start with \"{TOKEN_PREFIX}\""))?;
    let compressed =
        data_encoding::BASE64URL_NOPAD.decode(b64.as_bytes()).map_err(|e| format!("Invalid base64: {e}"))?;
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw).map_err(|e| format!("Decompression failed: {e}"))?;
    let bytes: [u8; 32] = raw.try_into().map_err(|v: Vec<u8>| format!("Expected 32 bytes, got {}", v.len()))?;
    iroh::PublicKey::from_bytes(&bytes).map_err(|e| format!("Invalid public key: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_encode_decode_roundtrip() {
        let key = iroh::SecretKey::generate(&mut rand::rng());
        let public = key.public();
        let token = encode_token(&public);
        let decoded = decode_token(&token).unwrap();
        assert_eq!(public, decoded);
    }

    #[test]
    fn token_has_prefix() {
        let key = iroh::SecretKey::generate(&mut rand::rng());
        let token = encode_token(&key.public());
        assert!(token.starts_with(TOKEN_PREFIX));
    }

    #[test]
    fn decode_token_rejects_missing_prefix() {
        let err = decode_token("not-a-toolkit-token").unwrap_err();
        assert!(err.contains("toolkit-"), "Error should mention prefix: {err}");
    }

    #[test]
    fn decode_token_rejects_invalid_base64() {
        let err = decode_token("toolkit-!!!invalid!!!").unwrap_err();
        assert!(err.contains("base64") || err.contains("Base64"), "Error should mention base64: {err}");
    }

    #[test]
    fn decode_token_rejects_empty_payload() {
        let err = decode_token("toolkit-").unwrap_err();
        assert!(err.is_empty() || !err.is_empty()); // Just shouldn't panic
    }

    #[test]
    fn decode_token_trims_whitespace() {
        let key = iroh::SecretKey::generate(&mut rand::rng());
        let token = encode_token(&key.public());
        let padded = format!("  {token}  \n");
        let decoded = decode_token(&padded).unwrap();
        assert_eq!(key.public(), decoded);
    }

    #[test]
    fn different_keys_produce_different_tokens() {
        let key1 = iroh::SecretKey::generate(&mut rand::rng());
        let key2 = iroh::SecretKey::generate(&mut rand::rng());
        let token1 = encode_token(&key1.public());
        let token2 = encode_token(&key2.public());
        assert_ne!(token1, token2);
    }
}

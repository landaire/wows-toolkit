// Connection lifecycle:
//
// Host: create endpoint, publish token -> accept connections -> handshake
// (Join -> SessionInfo + peer list) -> broadcast PeerAnnounce so existing
// peers accept the new joiner -> new joiner connects to each peer via MeshHello.
//
// Join: connect to host via token -> send Join -> receive SessionInfo with
// peer list -> connect to each peer via MeshHello -> accept incoming MeshHello
// from peers notified via PeerAnnounce.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;
use web_time::Instant;

use parking_lot::Mutex;

use iroh::Endpoint;
use iroh::SecretKey;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;
use wows_minimap_renderer::DrawCommand;

use crate::collab::ConnectedUser;
use crate::collab::OpenReplay;
use crate::collab::PeerRole;
use crate::collab::Permissions;
use crate::collab::SessionCommand;
use crate::collab::SessionEvent;
use crate::collab::SessionState;
use crate::collab::SessionStatus;
use crate::collab::UserCursor;
use crate::collab::protocol;
use crate::collab::protocol::*;
use crate::collab::types::Annotation;
use crate::collab::types::color_from_name;
use crate::collab::validation::validate_annotation;
use crate::collab::validation::validate_frame_commands_count;
use crate::collab::validation::validate_peer_message;
use crate::replay::renderer::PlaybackFrame;
/// Whether to host or join a session.
pub enum PeerMode {
    Host(HostParams),
    Join(JoinParams),
}

/// Parameters for hosting a session.
pub struct HostParams {
    pub toolkit_version: String,
    pub display_name: String,
    pub initial_render_options: CollabRenderOptions,
    /// Pre-serialized `PeerMessage::AssetBundle` bytes (length-prefixed rkyv).
    /// Shared so the UI thread can lazily populate it after game data loads.
    pub web_asset_bundle: Arc<Mutex<Option<Vec<u8>>>>,
}

/// Parameters for joining a session.
pub struct JoinParams {
    /// Session token (`toolkit-<base64>`).
    pub token: String,
    pub display_name: String,
    pub toolkit_version: String,
}

/// A frame to broadcast to all peers.
pub struct FrameBroadcast {
    pub replay_id: u64,
    pub clock: f32,
    pub frame_index: u32,
    pub total_frames: u32,
    pub game_duration: f32,
    pub commands: Vec<DrawCommand>,
}

/// Annotation events from the local UI.
pub enum LocalAnnotationEvent {
    /// Upsert an annotation by unique ID.
    Set { board_id: Option<u64>, id: u64, annotation: Annotation, owner: u64 },
    /// Remove a specific annotation by ID.
    Remove { board_id: Option<u64>, id: u64 },
    /// Remove all annotations.
    Clear { board_id: Option<u64> },
}

/// Per-ship range overrides: vec of (entity_id, filter) pairs.
pub type RangeOverrideUpdate =
    Vec<(wows_replays::types::EntityId, wows_minimap_renderer::draw_command::ShipConfigFilter)>;

/// Per-ship trail overrides: set of player names whose trails are hidden.
pub type TrailOverrideUpdate = Vec<String>;

/// Cap point events from the local UI (tactics board).
pub enum LocalCapPointEvent {
    /// Upsert a cap point.
    Set(protocol::WireCapPoint),
    /// Remove a cap point by ID.
    Remove { id: u64 },
}

/// Unified channel for all UI -> peer task messages.
pub enum LocalEvent {
    CursorPosition(Option<[f32; 2]>),
    Annotation(LocalAnnotationEvent),
    DisplayToggle(DisplayOptionField, bool),
    RangeOverrides(RangeOverrideUpdate),
    TrailOverrides(TrailOverrideUpdate),
    Ping([f32; 2]),
    /// Cap point edit (tactics board).
    CapPoint {
        board_id: u64,
        event: LocalCapPointEvent,
    },
    /// Tactics map opened — broadcast map info to peers.
    TacticsMapOpened {
        board_id: u64,
        owner_user_id: u64,
        map_name: String,
        /// Human-readable map name for display.
        display_name: String,
        map_id: u32,
        map_image_png: Vec<u8>,
        /// Map metadata for coordinate transforms.
        map_info: Option<wows_minimap_renderer::map_data::MapInfo>,
    },
    /// Tactics map closed.
    TacticsMapClosed {
        board_id: u64,
    },
}

/// Handle returned from `start_peer_session` for UI interaction.
pub struct PeerSessionHandle {
    /// Receive session events.
    pub event_rx: mpsc::Receiver<SessionEvent>,
    /// Send commands to the session.
    pub command_tx: mpsc::Sender<SessionCommand>,
    /// Send frames for broadcast (host/co-host only). `try_send` to avoid blocking.
    pub frame_tx: mpsc::SyncSender<FrameBroadcast>,
    /// Send local UI events (cursors, annotations, pings, etc.) to the peer task.
    pub local_tx: mpsc::Sender<LocalEvent>,
    /// Shared session state.
    pub state: Arc<Mutex<SessionState>>,
}

/// Start a peer session (host or join). Returns a handle for UI interaction.
///
/// The caller provides a shared `SessionState` that will be updated by the
/// background task and read by the UI. This ensures both sides see the same state.
pub fn start_peer_session(
    runtime: Arc<tokio::runtime::Runtime>,
    mode: PeerMode,
    state: Arc<Mutex<SessionState>>,
) -> PeerSessionHandle {
    let (event_tx, event_rx) = mpsc::channel();
    let (command_tx, command_rx) = mpsc::channel();
    let (frame_tx, frame_broadcast_rx) = mpsc::sync_channel(2);
    let (local_tx, local_rx) = mpsc::channel();

    // Reset the provided state for this new session.
    let is_host = matches!(&mode, PeerMode::Host(_));
    {
        let mut s = state.lock();
        s.role = if is_host { PeerRole::Host } else { PeerRole::Peer };
        s.status = if is_host { SessionStatus::Starting } else { SessionStatus::Connecting };
        s.my_user_id = 0;
        s.host_user_id = 0;
        s.frame_source_id = 0;
        s.connected_users.clear();
        s.permissions = Permissions::default();
        s.cursors.clear();
        s.token = None;
    }
    let state_clone = Arc::clone(&state);

    runtime.spawn(peer_task(mode, event_tx, command_rx, frame_broadcast_rx, local_rx, state_clone));

    PeerSessionHandle { event_rx, command_tx, frame_tx, local_tx, state }
}
/// A connected peer in the mesh (from our perspective).
struct MeshPeer {
    user_id: u64,
    name: String,
    color: [u8; 3],
    role: PeerRole,
    /// Channel for sending serialized (length-prefixed) messages to this peer's writer task.
    msg_tx: tokio::sync::mpsc::Sender<Arc<Vec<u8>>>,
    /// How many times this peer has requested assets. Capped at 5.
    asset_request_count: u8,
}

/// Shared mesh state accessible from multiple tasks.
struct MeshState {
    /// All connected peers keyed by user_id.
    peers: HashMap<u64, MeshPeer>,
    /// Our own identity.
    my_user_id: u64,
    my_name: String,
    my_color: [u8; 3],
    my_role: PeerRole,
    /// The original host's user_id (immutable, used for authority checks).
    host_user_id: u64,
    /// Current frame source user_id.
    frame_source_id: u64,
    /// Current permissions.
    permissions: Permissions,
}

impl MeshState {
    fn role_of(&self, user_id: u64) -> PeerRole {
        if user_id == self.my_user_id {
            self.my_role
        } else if let Some(p) = self.peers.get(&user_id) {
            p.role
        } else {
            PeerRole::Peer
        }
    }

    fn is_authority(&self, user_id: u64) -> bool {
        let role = self.role_of(user_id);
        role.is_host() || role.is_co_host()
    }
}
async fn peer_task(
    mode: PeerMode,
    event_tx: mpsc::Sender<SessionEvent>,
    command_rx: mpsc::Receiver<SessionCommand>,
    frame_broadcast_rx: mpsc::Receiver<FrameBroadcast>,
    local_rx: mpsc::Receiver<LocalEvent>,
    ui_state: Arc<Mutex<SessionState>>,
) {
    match mode {
        PeerMode::Host(params) => {
            host_main(params, event_tx, command_rx, frame_broadcast_rx, local_rx, ui_state).await;
        }
        PeerMode::Join(params) => {
            join_main(params, event_tx, command_rx, frame_broadcast_rx, local_rx, ui_state).await;
        }
    }
}
async fn host_main(
    params: HostParams,
    event_tx: mpsc::Sender<SessionEvent>,
    command_rx: mpsc::Receiver<SessionCommand>,
    frame_broadcast_rx: mpsc::Receiver<FrameBroadcast>,
    local_rx: mpsc::Receiver<LocalEvent>,
    ui_state: Arc<Mutex<SessionState>>,
) {
    // Create iroh endpoint.
    let secret_key = SecretKey::generate(&mut rand::rng());
    let endpoint = match Endpoint::builder().secret_key(secret_key).alpns(vec![COLLAB_ALPN.to_vec()]).bind().await {
        Ok(ep) => ep,
        Err(e) => {
            let msg = format!("Failed to bind iroh endpoint: {e}");
            error!("{msg}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };

    endpoint.online().await;

    // Generate the session token: toolkit-<base64url(zlib(public_key_bytes))>.
    // Both host and client use iroh's default relay, so the relay URL
    // is implicit and doesn't need to be in the token.
    let token = encode_token(&endpoint.addr().id);

    let my_user_id = 0u64;
    let my_color = color_from_name(&params.display_name);

    // Initialize mesh state.
    let mesh = Arc::new(Mutex::new(MeshState {
        peers: HashMap::new(),
        my_user_id,
        my_name: params.display_name.clone(),
        my_color,
        my_role: PeerRole::Host,
        host_user_id: my_user_id,
        frame_source_id: my_user_id,
        permissions: Permissions::default(),
    }));

    // Update UI state.
    {
        let mut s = ui_state.lock();
        s.my_user_id = my_user_id;
        s.host_user_id = my_user_id;
        s.frame_source_id = my_user_id;
        s.token = Some(token);
        s.status = SessionStatus::Active;
        s.cursors.push(UserCursor {
            user_id: my_user_id,
            name: params.display_name.clone(),
            color: my_color,
            pos: None,
            last_update: Instant::now(),
        });
        s.connected_users.push(ConnectedUser {
            id: my_user_id,
            name: params.display_name.clone(),
            color: my_color,
            role: PeerRole::Host,
            client_type: ClientType::Desktop { toolkit_version: params.toolkit_version.clone() },
        });
    }
    let _ = event_tx.send(SessionEvent::Started);
    info!("Collab host session started");

    // Frame compression channel (broadcast to all peers).
    let (frame_bytes_tx, _) = tokio::sync::broadcast::channel::<Arc<Vec<u8>>>(4);
    let frame_bytes_tx_clone = frame_bytes_tx.clone();

    // Last compressed frame, sent to newly joining peers so they get an
    // immediate picture instead of waiting for the next frame tick.
    let last_frame_bytes: Arc<Mutex<Option<Arc<Vec<u8>>>>> = Arc::new(Mutex::new(None));
    let last_frame_clone = Arc::clone(&last_frame_bytes);

    // Spawn frame compression task.
    let _frame_task = tokio::task::spawn_blocking(move || {
        while let Ok(frame) = frame_broadcast_rx.recv() {
            if let Some(framed) = serialize_frame(&frame) {
                let arc = Arc::new(framed);
                *last_frame_clone.lock() = Some(Arc::clone(&arc));
                let _ = frame_bytes_tx_clone.send(arc);
            }
        }
    });

    let next_user_id = Arc::new(std::sync::atomic::AtomicU64::new(1));

    // Channel for incoming peer messages from all connections.
    let (peer_msg_tx, mut peer_msg_rx) = tokio::sync::mpsc::channel::<(u64, PeerMessage)>(256);

    // Main loop.
    let mut shutdown = false;
    while !shutdown {
        tokio::select! {
            // Accept new connections.
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    info!("Endpoint closed, stopping host");
                    break;
                };
                let conn = match incoming.await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Failed to accept connection: {e}");
                        continue;
                    }
                };

                let user_id = next_user_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                let mesh_clone = Arc::clone(&mesh);
                let ui_state_clone = Arc::clone(&ui_state);
                let event_tx_clone = event_tx.clone();
                let peer_msg_tx_clone = peer_msg_tx.clone();
                let frame_rx = frame_bytes_tx.subscribe();
                let endpoint_clone = endpoint.clone();

                let toolkit_version = params.toolkit_version.clone();
                let initial_render_options = params.initial_render_options.clone();
                let open_replays: Vec<protocol::ReplayInfo> = ui_state.lock().open_replays.iter().map(|r| {
                    protocol::ReplayInfo {
                        replay_id: r.replay_id,
                        replay_name: r.replay_name.clone(),
                        map_image_png: r.map_image_png.clone(),
                        game_version: r.game_version.clone(),
                        map_name: r.map_name.clone(),
                        display_name: r.display_name.clone(),
                    }
                }).collect();
                debug!("Peer {user_id} joining: {} open replay(s) in SessionInfo", open_replays.len());
                let last_frame_for_peer = Arc::clone(&last_frame_bytes);
                let web_asset_bundle = params.web_asset_bundle.lock().clone();

                tokio::spawn(async move {
                    host_accept_peer(
                        conn,
                        user_id,
                        &toolkit_version,
                        &initial_render_options,
                        open_replays,
                        &endpoint_clone,
                        mesh_clone,
                        ui_state_clone,
                        event_tx_clone,
                        peer_msg_tx_clone,
                        frame_rx,
                        last_frame_for_peer,
                        web_asset_bundle,
                    )
                    .await;
                });
            }

            // Process incoming peer messages.
            Some((sender_id, msg)) = peer_msg_rx.recv() => {
                // Handle asset requests directly here where web_asset_bundle is in scope.
                if matches!(&msg, PeerMessage::RequestAssets) {
                    let bundle = params.web_asset_bundle.lock().clone();
                    if let Some(bundle_bytes) = bundle {
                        let mut m = mesh.lock();
                        if let Some(peer) = m.peers.get_mut(&sender_id) {
                            if peer.asset_request_count < 5 {
                                peer.asset_request_count += 1;
                                debug!(
                                    "Sending AssetBundle to peer {sender_id} on request ({}/5)",
                                    peer.asset_request_count
                                );
                                let _ = peer.msg_tx.try_send(Arc::new(bundle_bytes));
                            } else {
                                debug!("Ignoring RequestAssets from {sender_id}: limit reached");
                            }
                        }
                    } else {
                        debug!("Ignoring RequestAssets from {sender_id}: no asset bundle available");
                    }
                } else {
                    handle_incoming_message(
                        sender_id,
                        msg,
                        &mesh,
                        &ui_state,
                        &event_tx,
                    );
                }
            }

            // Process UI commands + local events (non-blocking).
            _ = tokio::task::yield_now() => {
                // Commands from UI.
                while let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        SessionCommand::Stop => {
                            shutdown = true;
                            break;
                        }
                        SessionCommand::SetPermissions(p) => {
                            let msg = PeerMessage::Permissions {
                                annotations_locked: p.annotations_locked,
                                settings_locked: p.settings_locked,
                            };
                            broadcast_to_mesh(&mesh, &msg);
                            let mut m = mesh.lock();
                            m.permissions = p.clone();
                            drop(m);
                            { let mut s = ui_state.lock();
                                s.permissions = p;
                            }
                        }
                        SessionCommand::ResetClientOverrides => {
                            let msg = PeerMessage::RenderOptions(params.initial_render_options.clone());
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::SyncAnnotations { board_id, annotations, owners, ids } => {
                            {
                                let mut s = ui_state.lock();
                                if let Some(bid) = board_id {
                                    let board = s.tactics_boards.entry(bid).or_default();
                                    board.annotation_sync = crate::collab::AnnotationSyncState {
                                        annotations: annotations.clone(),
                                        owners: owners.clone(),
                                        ids: ids.clone(),
                                    };
                                    board.annotation_sync_version += 1;
                                    s.tactics_boards_version += 1;
                                } else {
                                    s.current_annotation_sync = Some(crate::collab::AnnotationSyncState {
                                        annotations: annotations.clone(),
                                        owners: owners.clone(),
                                        ids: ids.clone(),
                                    });
                                    s.annotation_sync_version += 1;
                                }
                            }
                            let msg = PeerMessage::AnnotationSync { board_id, annotations, owners, ids };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::OpenWindowForEveryone { window_id } => {
                            broadcast_to_mesh(&mesh, &PeerMessage::OpenWindowForEveryone { window_id });
                            ui_state.lock().force_open_window_ids.insert(window_id);
                        }
                        SessionCommand::PromoteToCoHost { user_id } => {
                            let msg = PeerMessage::PromoteToCoHost { user_id };
                            broadcast_to_mesh(&mesh, &msg);
                            // Update local role map.
                            let mut m = mesh.lock();
                            if let Some(peer) = m.peers.get_mut(&user_id) {
                                peer.role = PeerRole::CoHost;
                            }
                            drop(m);
                            {
                                let mut s = ui_state.lock();
                                if let Some(u) = s.connected_users.iter_mut().find(|u| u.id == user_id) {
                                    u.role = PeerRole::CoHost;
                                }
                            }
                            let _ = event_tx.send(SessionEvent::PeerPromoted { user_id });
                        }
                        SessionCommand::BecomeFrameSource => {
                            let msg = PeerMessage::FrameSourceChanged { source_user_id: my_user_id };
                            broadcast_to_mesh(&mesh, &msg);
                            mesh.lock().frame_source_id = my_user_id;
                            { let mut s = ui_state.lock();
                                s.frame_source_id = my_user_id;
                            }
                            let _ = event_tx.send(SessionEvent::FrameSourceChanged { source_user_id: my_user_id });
                        }
                        SessionCommand::ReplayOpened { replay_id, replay_name, map_image_png, game_version, map_name, display_name } => {
                            // Store in session state (deduplicate by replay_id).
                            { let mut s = ui_state.lock();
                                if !s.open_replays.iter().any(|r| r.replay_id == replay_id) {
                                    s.open_replays.push(OpenReplay {
                                        replay_id,
                                        replay_name: replay_name.clone(),
                                        map_image_png: map_image_png.clone(),
                                        game_version: game_version.clone(),
                                        map_name: map_name.clone(),
                                        display_name: display_name.clone(),
                                    });
                                }
                            }
                            // Broadcast to all peers.
                            let msg = PeerMessage::ReplayOpened {
                                replay_id,
                                replay_name,
                                map_image_png,
                                game_version,
                                map_name,
                                display_name,
                            };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::ReplayClosed { replay_id } => {
                            // Remove from session state and clear annotations.
                            { let mut s = ui_state.lock();
                                s.open_replays.retain(|r| r.replay_id != replay_id);
                                s.current_annotation_sync = None;
                                s.annotation_sync_version += 1;
                            }
                            // Broadcast to all peers.
                            let msg = PeerMessage::ReplayClosed { replay_id };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::SyncCapPoints { board_id, cap_points } => {
                            {
                                let mut s = ui_state.lock();
                                let board = s.tactics_boards.entry(board_id).or_default();
                                board.cap_point_sync = crate::collab::CapPointSyncState {
                                    cap_points: cap_points.clone(),
                                };
                                board.cap_point_sync_version += 1;
                                s.tactics_boards_version += 1;
                            }
                            let msg = PeerMessage::CapPointSync { board_id, cap_points };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                    }
                }

                // Drain local UI events.
                while let Ok(evt) = local_rx.try_recv() {
                    match evt {
                        LocalEvent::CursorPosition(pos) => {
                            {
                                let mut s = ui_state.lock();
                                if let Some(c) = s.cursors.iter_mut().find(|c| c.user_id == my_user_id) {
                                    c.pos = pos;
                                    c.last_update = Instant::now();
                                }
                            }
                            broadcast_to_mesh(&mesh, &PeerMessage::CursorPosition { user_id: my_user_id, pos });
                        }
                        LocalEvent::Annotation(evt) => {
                            let msg = match &evt {
                                LocalAnnotationEvent::Set { board_id, id, annotation, owner } => {
                                    let mut s = ui_state.lock();
                                    let sync = if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) {
                                            &mut board.annotation_sync
                                        } else { continue; }
                                    } else {
                                        s.current_annotation_sync.get_or_insert_with(Default::default)
                                    };
                                    if let Some(pos) = sync.ids.iter().position(|&eid| eid == *id) {
                                        sync.annotations[pos] = annotation.clone();
                                        sync.owners[pos] = *owner;
                                    } else {
                                        sync.annotations.push(annotation.clone());
                                        sync.owners.push(*owner);
                                        sync.ids.push(*id);
                                    }
                                    if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) { board.annotation_sync_version += 1; }
                                        s.tactics_boards_version += 1;
                                    } else { s.annotation_sync_version += 1; }
                                    PeerMessage::SetAnnotation { board_id: *board_id, id: *id, annotation: annotation.clone(), owner: *owner }
                                }
                                LocalAnnotationEvent::Remove { board_id, id } => {
                                    let mut s = ui_state.lock();
                                    let sync = if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) {
                                            &mut board.annotation_sync
                                        } else { continue; }
                                    } else if let Some(ref mut sync) = s.current_annotation_sync { sync } else { continue; };
                                    if let Some(pos) = sync.ids.iter().position(|&eid| eid == *id) {
                                        sync.annotations.remove(pos);
                                        sync.owners.remove(pos);
                                        sync.ids.remove(pos);
                                    }
                                    if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) { board.annotation_sync_version += 1; }
                                        s.tactics_boards_version += 1;
                                    } else { s.annotation_sync_version += 1; }
                                    PeerMessage::RemoveAnnotation { board_id: *board_id, id: *id }
                                }
                                LocalAnnotationEvent::Clear { board_id } => {
                                    let mut s = ui_state.lock();
                                    if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) {
                                            board.annotation_sync = Default::default();
                                            board.annotation_sync_version += 1;
                                        }
                                        s.tactics_boards_version += 1;
                                    } else if let Some(sync) = s.current_annotation_sync.as_mut() {
                                        sync.annotations.clear();
                                        sync.owners.clear();
                                        sync.ids.clear();
                                        s.annotation_sync_version += 1;
                                    }
                                    PeerMessage::ClearAnnotations { board_id: *board_id }
                                }
                            };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        LocalEvent::DisplayToggle(field, value) => {
                            broadcast_to_mesh(&mesh, &PeerMessage::ToggleDisplayOption { field, value });
                        }
                        LocalEvent::RangeOverrides(overrides) => {
                            broadcast_to_mesh(&mesh, &PeerMessage::ShipRangeOverrides { overrides });
                        }
                        LocalEvent::TrailOverrides(hidden) => {
                            broadcast_to_mesh(&mesh, &PeerMessage::ShipTrailOverrides { hidden });
                        }
                        LocalEvent::Ping(pos) => {
                            let my_color = mesh.lock().my_color;
                            broadcast_to_mesh(&mesh, &PeerMessage::Ping { user_id: my_user_id, pos, color: my_color });
                        }
                        LocalEvent::CapPoint { board_id, event: evt } => {
                            let msg = match &evt {
                                LocalCapPointEvent::Set(cp) => {
                                    debug!("Host peer task: SetCapPoint board={board_id} id={} radius={}", cp.id, cp.radius);
                                    let mut s = ui_state.lock();
                                    if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                                        let sync = &mut board.cap_point_sync;
                                        if let Some(pos) = sync.cap_points.iter().position(|c| c.id == cp.id) {
                                            sync.cap_points[pos] = cp.clone();
                                        } else {
                                            sync.cap_points.push(cp.clone());
                                        }
                                        board.cap_point_sync_version += 1;
                                        s.tactics_boards_version += 1;
                                    }
                                    PeerMessage::SetCapPoint { board_id, cap_point: cp.clone() }
                                }
                                LocalCapPointEvent::Remove { id } => {
                                    debug!("Host peer task: RemoveCapPoint board={board_id} id={id}");
                                    let mut s = ui_state.lock();
                                    if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                                        board.cap_point_sync.cap_points.retain(|c| c.id != *id);
                                        board.cap_point_sync_version += 1;
                                        s.tactics_boards_version += 1;
                                    }
                                    PeerMessage::RemoveCapPoint { board_id, id: *id }
                                }
                            };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        LocalEvent::TacticsMapOpened { board_id, owner_user_id, map_name, display_name, map_id, map_image_png, map_info } => {
                            {
                                let mut s = ui_state.lock();
                                let owner = if owner_user_id == 0 { s.my_user_id } else { owner_user_id };
                                let board = s.tactics_boards.entry(board_id).or_default();
                                board.owner_user_id = owner;
                                board.tactics_map = crate::collab::TacticsMapInfo {
                                    map_name: map_name.clone(),
                                    display_name: display_name.clone(),
                                    map_id,
                                    map_image_png: map_image_png.clone(),
                                    map_info: map_info.clone(),
                                };
                                s.tactics_boards_version += 1;
                            }
                            let owner = ui_state.lock().my_user_id;
                            broadcast_to_mesh(&mesh, &PeerMessage::TacticsMapOpened { board_id, owner_user_id: owner, map_name, display_name, map_id, map_image_png, map_info });
                        }
                        LocalEvent::TacticsMapClosed { board_id } => {
                            {
                                let mut s = ui_state.lock();
                                s.tactics_boards.remove(&board_id);
                                s.tactics_boards_version += 1;
                            }
                            broadcast_to_mesh(&mesh, &PeerMessage::TacticsMapClosed { board_id });
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            }
        }
    }

    // Cleanup.
    endpoint.close().await;
    let _ = event_tx.send(SessionEvent::Ended);
    ui_state.lock().clear_session_data();
    info!("Collab host session ended");
}

/// Handle an incoming connection on the host: handshake, register, announce.
#[allow(clippy::too_many_arguments)]
async fn host_accept_peer(
    conn: iroh::endpoint::Connection,
    user_id: u64,
    toolkit_version: &str,
    initial_render_options: &CollabRenderOptions,
    open_replays: Vec<protocol::ReplayInfo>,
    endpoint: &Endpoint,
    mesh: Arc<Mutex<MeshState>>,
    ui_state: Arc<Mutex<SessionState>>,
    event_tx: mpsc::Sender<SessionEvent>,
    peer_msg_tx: tokio::sync::mpsc::Sender<(u64, PeerMessage)>,
    mut frame_rx: tokio::sync::broadcast::Receiver<Arc<Vec<u8>>>,
    last_frame_bytes: Arc<Mutex<Option<Arc<Vec<u8>>>>>,
    web_asset_bundle: Option<Vec<u8>>,
) {
    let (mut send, mut recv) = match conn.accept_bi().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to accept bi stream from peer: {e}");
            return;
        }
    };

    // Expect Join within 5 seconds.
    let join_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(5), read_peer_message(&mut recv, MAX_MESSAGE_SIZE))
            .await
        {
            Ok(Ok(Some(msg))) => msg,
            _ => {
                warn!("Peer {user_id} did not send Join in time");
                return;
            }
        };

    let (client_name, client_type) = match &join_msg {
        PeerMessage::Join { name, client_type } => {
            // Disambiguate duplicate names (case-insensitive) by appending the user_id.
            let name_taken = {
                let m = mesh.lock();
                m.my_name.eq_ignore_ascii_case(name)
                    || m.peers.values().any(|p| p.name.eq_ignore_ascii_case(name))
            };
            let final_name = if name_taken { format!("{name} ({user_id})") } else { name.clone() };
            (final_name, client_type.clone())
        }
        _ => {
            warn!("Peer {user_id} sent non-Join as first message");
            return;
        }
    };

    let color = color_from_name(&client_name);

    // Validate version (desktop clients only — web clients have no version).
    if let ClientType::Desktop { toolkit_version: client_ver } = &client_type
        && client_ver != toolkit_version
    {
        let msg = PeerMessage::Rejected {
            reason: format!("Version mismatch: host is v{toolkit_version}, you have v{client_ver}"),
        };
        let _ = write_peer_message(&mut send, &msg).await;
        return;
    }

    // Validate name.
    if let Err(e) = validate_peer_message(&join_msg) {
        let msg = PeerMessage::Rejected { reason: format!("Invalid join request: {e}") };
        let _ = write_peer_message(&mut send, &msg).await;
        return;
    }

    // Build peer list (all existing peers + self/host).
    let (peers_list, current_perms, frame_source_id) = {
        let m = mesh.lock();
        let mut peers = Vec::new();
        // Add the host itself.
        let my_addr_json = match serde_json::to_string(&endpoint.addr()) {
            Ok(j) => j,
            Err(_) => return,
        };
        peers.push(PeerInfo {
            user_id: m.my_user_id,
            name: m.my_name.clone(),
            color: m.my_color,
            endpoint_addr_json: my_addr_json,
        });
        // Add existing peers (we don't have their endpoint addrs easily, but
        // we can skip them — they'll receive PeerAnnounce and initiate connection).
        // Actually for mesh, the joiner needs to connect to all existing peers.
        // The host doesn't store endpoint addrs for peers. Instead, existing peers
        // will receive PeerAnnounce and accept connections from the new joiner.
        // So peers list only contains the host for now — the mesh is established
        // by existing peers accepting connections after PeerAnnounce.
        (peers, m.permissions.clone(), m.frame_source_id)
    };

    // Send SessionInfo.
    let assigned_identity = PeerIdentity { user_id, name: client_name.clone(), color };
    let session_info = PeerMessage::SessionInfo {
        toolkit_version: toolkit_version.to_string(),
        peers: peers_list,
        assigned_identity,
        frame_source_id,
        open_replays,
    };
    if write_peer_message(&mut send, &session_info).await.is_err() {
        return;
    }

    // Announce the new peer to UI and existing peers immediately after handshake,
    // before sending large payloads (AssetBundle etc.) that may take a while.
    let join_notify = PeerMessage::UserJoined { user_id, name: client_name.clone(), color };
    broadcast_to_mesh(&mesh, &join_notify);

    // Create per-peer message channel.
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel::<Arc<Vec<u8>>>(64);

    // Register in mesh.
    let user = ConnectedUser {
        id: user_id,
        name: client_name.clone(),
        color,
        role: PeerRole::Peer,
        client_type: client_type.clone(),
    };
    {
        let mut m = mesh.lock();
        m.peers.insert(
            user_id,
            MeshPeer {
                user_id,
                name: client_name.clone(),
                color,
                role: PeerRole::Peer,
                msg_tx,
                asset_request_count: 0,
            },
        );
    }
    {
        let mut s = ui_state.lock();
        s.connected_users.push(user.clone());
        s.cursors.push(UserCursor {
            user_id,
            name: client_name.clone(),
            color,
            pos: None,
            last_update: Instant::now(),
        });
    }
    let _ = event_tx.send(SessionEvent::UserJoined(user));
    info!("Peer {user_id} ({client_name}) joined session");

    // Send setup data (asset bundle, permissions, render options, annotations,
    // tactics boards). If any send fails, we fall through to cleanup below.
    let setup_ok = async {
        // Send asset bundle to web clients (pre-serialized bytes, written directly).
        if matches!(&client_type, ClientType::Web) {
            if let Some(ref bundle_bytes) = web_asset_bundle {
                debug!("Sending AssetBundle ({} bytes) to web peer {user_id}", bundle_bytes.len());
                if send.write_all(bundle_bytes).await.is_err() {
                    warn!("Failed to send AssetBundle to peer {user_id}");
                    return false;
                }
            } else {
                debug!("No AssetBundle available for web peer {user_id}");
            }
        }

        // Send current permissions.
        let perm_msg = PeerMessage::Permissions {
            annotations_locked: current_perms.annotations_locked,
            settings_locked: current_perms.settings_locked,
        };
        if write_peer_message(&mut send, &perm_msg).await.is_err() {
            return false;
        }

        // Send current render options.
        let opts_msg = PeerMessage::RenderOptions(initial_render_options.clone());
        if write_peer_message(&mut send, &opts_msg).await.is_err() {
            return false;
        }

        // Send current replay-context annotations if any.
        let ann_msg = {
            let s = ui_state.lock();
            s.current_annotation_sync.as_ref().and_then(|sync| {
                if sync.annotations.is_empty() {
                    None
                } else {
                    Some(PeerMessage::AnnotationSync {
                        board_id: None,
                        annotations: sync.annotations.clone(),
                        owners: sync.owners.clone(),
                        ids: sync.ids.clone(),
                    })
                }
            })
        };
        if let Some(msg) = ann_msg
            && write_peer_message(&mut send, &msg).await.is_err()
        {
            return false;
        }

        // Send all open tactics boards (map + cap points + annotations per board).
        let board_msgs: Vec<_> = {
            let s = ui_state.lock();
            s.tactics_boards
                .iter()
                .map(|(&bid, board)| {
                    let tmap = PeerMessage::TacticsMapOpened {
                        board_id: bid,
                        owner_user_id: board.owner_user_id,
                        map_name: board.tactics_map.map_name.clone(),
                        display_name: board.tactics_map.display_name.clone(),
                        map_id: board.tactics_map.map_id,
                        map_image_png: board.tactics_map.map_image_png.clone(),
                        map_info: board.tactics_map.map_info.clone(),
                    };
                    let cap = if board.cap_point_sync.cap_points.is_empty() {
                        None
                    } else {
                        Some(PeerMessage::CapPointSync {
                            board_id: bid,
                            cap_points: board.cap_point_sync.cap_points.clone(),
                        })
                    };
                    let ann = if board.annotation_sync.annotations.is_empty() {
                        None
                    } else {
                        Some(PeerMessage::AnnotationSync {
                            board_id: Some(bid),
                            annotations: board.annotation_sync.annotations.clone(),
                            owners: board.annotation_sync.owners.clone(),
                            ids: board.annotation_sync.ids.clone(),
                        })
                    };
                    (tmap, cap, ann)
                })
                .collect()
        };
        for (tmap_msg, cap_msg, ann_msg) in board_msgs {
            if write_peer_message(&mut send, &tmap_msg).await.is_err() {
                return false;
            }
            if let Some(msg) = cap_msg
                && write_peer_message(&mut send, &msg).await.is_err()
            {
                return false;
            }
            if let Some(msg) = ann_msg
                && write_peer_message(&mut send, &msg).await.is_err()
            {
                return false;
            }
        }

        true
    }
    .await;

    if !setup_ok {
        // Clean up the mesh/UI registration we did above.
        mesh.lock().peers.remove(&user_id);
        {
            let mut s = ui_state.lock();
            s.connected_users.retain(|u| u.id != user_id);
            s.cursors.retain(|c| c.user_id != user_id);
        }
        let leave_msg = PeerMessage::UserLeft { user_id };
        broadcast_to_mesh(&mesh, &leave_msg);
        let _ = event_tx.send(SessionEvent::UserLeft { user_id, name: client_name, timed_out: false });
        return;
    }

    // Spawn a dedicated reader task so that read_peer_message is never
    // cancelled mid-read by a tokio::select! branch (cancelling a read_exact
    // that has partially consumed QUIC stream data corrupts the framing).
    let (host_read_tx, mut host_read_rx) = tokio::sync::mpsc::channel::<Result<PeerMessage, String>>(16);
    tokio::spawn(async move {
        loop {
            match read_peer_message(&mut recv, MAX_MESSAGE_SIZE).await {
                Ok(Some(msg)) => {
                    if host_read_tx.send(Ok(msg)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = host_read_tx.send(Err(e.to_string())).await;
                    break;
                }
            }
        }
    });

    // Send the last frame so the peer gets an immediate picture.
    let last_frame = last_frame_bytes.lock().clone();
    if let Some(frame_data) = last_frame {
        info!("Sending last frame ({} bytes) to peer {user_id}", frame_data.len());
        if send.write_all(&frame_data).await.is_err() {
            return;
        }
    } else {
        info!("No last frame available for peer {user_id}");
    }

    // Heartbeat state.
    let mut last_received = tokio::time::Instant::now();
    let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    heartbeat_interval.tick().await; // consume immediate first tick
    let heartbeat_bytes = frame_peer_message(&PeerMessage::Heartbeat).expect("heartbeat serialization cannot fail");
    let mut timed_out = false;

    // Message loop for this peer.
    loop {
        tokio::select! {
            // Biased: drain control messages (ReplayOpened, etc.) before frames,
            // so clients always create renderers before receiving frame data.
            biased;

            msg_result = host_read_rx.recv() => {
                match msg_result {
                    Some(Ok(msg)) => {
                        if matches!(&msg, PeerMessage::Heartbeat) {
                            last_received = tokio::time::Instant::now();
                            continue;
                        }
                        if let Err(e) = validate_peer_message(&msg) {
                            warn!("Peer {user_id} sent invalid message: {e}");
                            continue;
                        }
                        last_received = tokio::time::Instant::now();
                        let _ = peer_msg_tx.send((user_id, msg)).await;
                    }
                    Some(Err(e)) => {
                        warn!("Peer {user_id} read error: {e}");
                        break;
                    }
                    None => {
                        debug!("Peer {user_id} stream closed");
                        break;
                    }
                }
            }

            // Forward per-peer messages (control: ReplayOpened, UserJoined, etc.).
            // Must be drained before frames so clients create renderers first.
            Some(data) = msg_rx.recv() => {
                if send.write_all(&data).await.is_err() {
                    break;
                }
            }

            // Forward broadcast frames.
            frame_data = frame_rx.recv() => {
                match frame_data {
                    Ok(data) => {
                        if send.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        debug!("Peer {user_id} lagged {n} frames");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }

            // Heartbeat: send keepalive and check for timeout.
            _ = heartbeat_interval.tick() => {
                if last_received.elapsed() > std::time::Duration::from_secs(HEARTBEAT_TIMEOUT_SECS) {
                    warn!("Peer {user_id} heartbeat timeout");
                    timed_out = true;
                    break;
                }
                if send.write_all(&heartbeat_bytes).await.is_err() {
                    break;
                }
            }
        }
    }

    // Cleanup.
    {
        mesh.lock().peers.remove(&user_id);
    }
    {
        let mut s = ui_state.lock();
        s.connected_users.retain(|u| u.id != user_id);
        s.cursors.retain(|c| c.user_id != user_id);
    }
    let leave_msg = PeerMessage::UserLeft { user_id };
    broadcast_to_mesh(&mesh, &leave_msg);
    let _ = event_tx.send(SessionEvent::UserLeft { user_id, name: client_name.clone(), timed_out });
    info!("Peer {user_id} ({client_name}) left session");
}
async fn join_main(
    params: JoinParams,
    event_tx: mpsc::Sender<SessionEvent>,
    command_rx: mpsc::Receiver<SessionCommand>,
    frame_broadcast_rx: mpsc::Receiver<FrameBroadcast>,
    local_rx: mpsc::Receiver<LocalEvent>,
    ui_state: Arc<Mutex<SessionState>>,
) {
    // Decode the session token to extract the host's node ID.
    let host_node_id = match decode_token(&params.token) {
        Ok(id) => id,
        Err(e) => {
            let msg = format!("Invalid session token: {e}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };

    // Create endpoint and connect to host.
    let endpoint = match Endpoint::builder().alpns(vec![COLLAB_ALPN.to_vec()]).bind().await {
        Ok(ep) => ep,
        Err(e) => {
            let msg = format!("Failed to bind iroh endpoint: {e}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };

    // Reconstruct the host's EndpointAddr using the node ID from the token
    // and relay URLs from our own endpoint (both sides use the same defaults).
    let addr = {
        let my_addr = endpoint.addr();
        let mut a = iroh::EndpointAddr::new(host_node_id);
        for url in my_addr.relay_urls() {
            a = a.with_relay_url(url.clone());
        }
        a
    };

    let conn = match tokio::time::timeout(std::time::Duration::from_secs(15), endpoint.connect(addr, COLLAB_ALPN)).await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            let msg = format!("Failed to connect to host: {e}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
        Err(_) => {
            let msg = "Connection to host timed out".to_string();
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };

    let (mut send, mut recv) = match conn.open_bi().await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("Failed to open stream to host: {e}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };

    // Handshake: send Join.
    let join_msg = PeerMessage::Join {
        name: params.display_name.clone(),
        client_type: ClientType::Desktop { toolkit_version: params.toolkit_version.clone() },
    };
    if let Err(e) = write_peer_message(&mut send, &join_msg).await {
        let msg = format!("Failed to send Join: {e}");
        let _ = event_tx.send(SessionEvent::Error(msg.clone()));
        set_status(&ui_state, SessionStatus::Error(msg));
        return;
    }

    // Wait for SessionInfo or Rejected.
    let first_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(10), read_peer_message(&mut recv, MAX_MESSAGE_SIZE))
            .await
        {
            Ok(Ok(Some(msg))) => msg,
            Ok(Ok(None)) => {
                let _ = event_tx.send(SessionEvent::Error("Host closed connection".into()));
                return;
            }
            Ok(Err(e)) => {
                let _ = event_tx.send(SessionEvent::Error(format!("Read error: {e}")));
                return;
            }
            Err(_) => {
                let _ = event_tx.send(SessionEvent::Error("Handshake timed out".into()));
                return;
            }
        };

    let (my_user_id, my_name, my_color, host_user_id, host_name, host_color, frame_source_id, toolkit_version) =
        match &first_msg {
            PeerMessage::Rejected { reason } => {
                let _ = event_tx.send(SessionEvent::Rejected(reason.clone()));
                set_status(&ui_state, SessionStatus::Error(format!("Rejected: {reason}")));
                return;
            }
            PeerMessage::SessionInfo { toolkit_version, peers, assigned_identity, frame_source_id, open_replays } => {
                if let Err(e) = validate_peer_message(&first_msg) {
                    let _ = event_tx.send(SessionEvent::Error(format!("Invalid SessionInfo: {e}")));
                    return;
                }
                let open_replay_list: Vec<OpenReplay> = open_replays
                    .iter()
                    .map(|r| OpenReplay {
                        replay_id: r.replay_id,
                        replay_name: r.replay_name.clone(),
                        map_image_png: r.map_image_png.clone(),
                        game_version: r.game_version.clone(),
                        map_name: r.map_name.clone(),
                        display_name: r.display_name.clone(),
                    })
                    .collect();
                let _ = event_tx.send(SessionEvent::SessionInfoReceived { open_replays: open_replay_list });
                // The host is peer[0] (if present).
                let host_peer = peers.first();
                let host_uid = host_peer.map(|p| p.user_id).unwrap_or(0);
                let host_name = host_peer.map(|p| p.name.clone()).unwrap_or_else(|| "Host".into());
                let host_color = host_peer.map(|p| p.color).unwrap_or([200, 200, 200]);
                (
                    assigned_identity.user_id,
                    assigned_identity.name.clone(),
                    assigned_identity.color,
                    host_uid,
                    host_name,
                    host_color,
                    *frame_source_id,
                    toolkit_version.clone(),
                )
            }
            _ => {
                let _ = event_tx.send(SessionEvent::Error("Expected SessionInfo as first message".into()));
                return;
            }
        };

    // Initialize mesh state.
    let mesh = Arc::new(Mutex::new(MeshState {
        peers: HashMap::new(),
        my_user_id,
        my_name: my_name.clone(),
        my_color,
        my_role: PeerRole::Peer,
        host_user_id,
        frame_source_id,
        permissions: Permissions::default(),
    }));

    // The host connection is our first peer.
    let (host_msg_tx, _host_msg_rx) = tokio::sync::mpsc::channel::<Arc<Vec<u8>>>(64);
    // We write directly to `send` for the host, so host_msg_tx is unused for sending.
    // But we register it in the mesh for broadcast_to_mesh to work.
    {
        let mut m = mesh.lock();
        m.peers.insert(
            host_user_id,
            MeshPeer {
                user_id: host_user_id,
                name: host_name.clone(),
                color: host_color,
                role: PeerRole::Host,
                msg_tx: host_msg_tx,
                asset_request_count: 0,
            },
        );
    }

    // Update UI state.
    {
        let mut s = ui_state.lock();
        s.my_user_id = my_user_id;
        s.host_user_id = host_user_id;
        s.frame_source_id = frame_source_id;
        s.role = PeerRole::Peer;
        s.status = SessionStatus::Active;
        // Add host to connected users so they appear in the user list.
        // Host is always a desktop client.
        s.connected_users.push(ConnectedUser {
            id: host_user_id,
            name: host_name,
            color: host_color,
            role: PeerRole::Host,
            client_type: ClientType::Desktop { toolkit_version: toolkit_version.clone() },
        });
        s.cursors.push(UserCursor {
            user_id: my_user_id,
            name: my_name.clone(),
            color: my_color,
            pos: None,
            last_update: Instant::now(),
        });
    }
    let _ = event_tx.send(SessionEvent::Started);
    info!("Joined collab session as peer {my_user_id}");

    // Frame compression task for when we become frame source (co-host).
    let (frame_bytes_tx, _) = tokio::sync::broadcast::channel::<Arc<Vec<u8>>>(4);
    let frame_bytes_tx_clone = frame_bytes_tx.clone();
    let _frame_task = tokio::task::spawn_blocking(move || {
        while let Ok(frame) = frame_broadcast_rx.recv() {
            if let Some(framed) = serialize_frame(&frame) {
                let _ = frame_bytes_tx_clone.send(Arc::new(framed));
            }
        }
    });

    // Spawn a dedicated reader task so that read_peer_message is never
    // cancelled mid-read by a tokio::select! branch (cancelling a read_exact
    // that has partially consumed QUIC stream data corrupts the framing).
    let (client_read_tx, mut client_read_rx) = tokio::sync::mpsc::channel::<Result<PeerMessage, String>>(16);
    tokio::spawn(async move {
        loop {
            match read_peer_message(&mut recv, MAX_MESSAGE_SIZE).await {
                Ok(Some(msg)) => {
                    if client_read_tx.send(Ok(msg)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = client_read_tx.send(Err(e.to_string())).await;
                    break;
                }
            }
        }
    });

    // Heartbeat state.
    let mut last_received = tokio::time::Instant::now();
    let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    heartbeat_interval.tick().await; // consume immediate first tick

    // Main loop.
    let session_info_received = true;
    loop {
        tokio::select! {
            // Receive messages from host (via dedicated reader task).
            msg_result = client_read_rx.recv() => {
                match msg_result {
                    Some(Ok(msg)) => {
                        if matches!(&msg, PeerMessage::Heartbeat) {
                            last_received = tokio::time::Instant::now();
                            continue;
                        }
                        if matches!(&msg, PeerMessage::SessionInfo { .. }) && session_info_received {
                            warn!("Ignoring duplicate SessionInfo from host");
                            continue;
                        }
                        if let Err(e) = validate_peer_message(&msg) {
                            warn!("Invalid message from host: {e}");
                            continue;
                        }
                        last_received = tokio::time::Instant::now();
                        handle_incoming_message(
                            host_user_id,
                            msg,
                            &mesh,
                            &ui_state,
                            &event_tx,
                        );
                    }
                    Some(Err(e)) => {
                        warn!("Read error from host: {e}");
                        break;
                    }
                    None => {
                        info!("Host closed connection");
                        break;
                    }
                }
            }

            // Heartbeat: send keepalive and check for timeout.
            _ = heartbeat_interval.tick() => {
                if last_received.elapsed() > std::time::Duration::from_secs(HEARTBEAT_TIMEOUT_SECS) {
                    warn!("Host heartbeat timeout");
                    let _ = event_tx.send(SessionEvent::Error(
                        format!("Connection to host lost (no response for {HEARTBEAT_TIMEOUT_SECS}s)")
                    ));
                    break;
                }
                if write_peer_message(&mut send, &PeerMessage::Heartbeat).await.is_err() {
                    break;
                }
            }

            // Forward UI events to host.
            _ = tokio::task::yield_now() => {
                // Check for stop.
                if let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        SessionCommand::Stop => break,
                        SessionCommand::SetPermissions(p) => {
                            let msg = PeerMessage::Permissions {
                                annotations_locked: p.annotations_locked,
                                settings_locked: p.settings_locked,
                            };
                            let _ = write_peer_message(&mut send, &msg).await;
                            mesh.lock().permissions = p.clone();
                            { let mut s = ui_state.lock();
                                s.permissions = p;
                            }
                        }
                        SessionCommand::ResetClientOverrides => {
                            // Co-host could send render options — but we don't
                            // track initial options on the join side.
                        }
                        SessionCommand::SyncAnnotations { board_id, annotations, owners, ids } => {
                            {
                                let mut s = ui_state.lock();
                                if let Some(bid) = board_id {
                                    let board = s.tactics_boards.entry(bid).or_default();
                                    board.annotation_sync = crate::collab::AnnotationSyncState {
                                        annotations: annotations.clone(),
                                        owners: owners.clone(),
                                        ids: ids.clone(),
                                    };
                                    board.annotation_sync_version += 1;
                                    s.tactics_boards_version += 1;
                                } else {
                                    s.current_annotation_sync = Some(crate::collab::AnnotationSyncState {
                                        annotations: annotations.clone(),
                                        owners: owners.clone(),
                                        ids: ids.clone(),
                                    });
                                    s.annotation_sync_version += 1;
                                }
                            }
                            let msg = PeerMessage::AnnotationSync { board_id, annotations, owners, ids };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                        SessionCommand::OpenWindowForEveryone { window_id } => {
                            let _ = write_peer_message(&mut send, &PeerMessage::OpenWindowForEveryone { window_id }).await;
                            ui_state.lock().force_open_window_ids.insert(window_id);
                        }
                        SessionCommand::PromoteToCoHost { .. } => {
                            // Only the host can promote — ignore.
                        }
                        SessionCommand::BecomeFrameSource => {
                            let uid = mesh.lock().my_user_id;
                            let msg = PeerMessage::FrameSourceChanged { source_user_id: uid };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                        SessionCommand::ReplayOpened { .. } | SessionCommand::ReplayClosed { .. } => {
                            // Only the host sends replay lifecycle messages — ignore on join side.
                        }
                        SessionCommand::SyncCapPoints { board_id, cap_points } => {
                            {
                                let mut s = ui_state.lock();
                                let board = s.tactics_boards.entry(board_id).or_default();
                                board.cap_point_sync = crate::collab::CapPointSyncState {
                                    cap_points: cap_points.clone(),
                                };
                                board.cap_point_sync_version += 1;
                                s.tactics_boards_version += 1;
                            }
                            let msg = PeerMessage::CapPointSync { board_id, cap_points };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                    }
                }

                // Drain local UI events.
                while let Ok(evt) = local_rx.try_recv() {
                    match evt {
                        LocalEvent::CursorPosition(pos) => {
                            {
                                let mut s = ui_state.lock();
                                if let Some(c) = s.cursors.iter_mut().find(|c| c.user_id == my_user_id) {
                                    c.pos = pos;
                                    c.last_update = Instant::now();
                                }
                            }
                            let _ = write_peer_message(&mut send, &PeerMessage::CursorPosition { user_id: my_user_id, pos }).await;
                        }
                        LocalEvent::Annotation(evt) => {
                            let msg = match &evt {
                                LocalAnnotationEvent::Set { board_id, id, annotation, owner } => {
                                    let mut s = ui_state.lock();
                                    let sync = if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) {
                                            &mut board.annotation_sync
                                        } else { continue; }
                                    } else {
                                        s.current_annotation_sync.get_or_insert_with(Default::default)
                                    };
                                    if let Some(pos) = sync.ids.iter().position(|&eid| eid == *id) {
                                        sync.annotations[pos] = annotation.clone();
                                        sync.owners[pos] = *owner;
                                    } else {
                                        sync.annotations.push(annotation.clone());
                                        sync.owners.push(*owner);
                                        sync.ids.push(*id);
                                    }
                                    if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) { board.annotation_sync_version += 1; }
                                        s.tactics_boards_version += 1;
                                    } else { s.annotation_sync_version += 1; }
                                    PeerMessage::SetAnnotation { board_id: *board_id, id: *id, annotation: annotation.clone(), owner: *owner }
                                }
                                LocalAnnotationEvent::Remove { board_id, id } => {
                                    let mut s = ui_state.lock();
                                    let sync = if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) {
                                            &mut board.annotation_sync
                                        } else { continue; }
                                    } else if let Some(ref mut sync) = s.current_annotation_sync { sync } else { continue; };
                                    if let Some(pos) = sync.ids.iter().position(|&eid| eid == *id) {
                                        sync.annotations.remove(pos);
                                        sync.owners.remove(pos);
                                        sync.ids.remove(pos);
                                    }
                                    if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) { board.annotation_sync_version += 1; }
                                        s.tactics_boards_version += 1;
                                    } else { s.annotation_sync_version += 1; }
                                    PeerMessage::RemoveAnnotation { board_id: *board_id, id: *id }
                                }
                                LocalAnnotationEvent::Clear { board_id } => {
                                    let mut s = ui_state.lock();
                                    if let Some(bid) = board_id {
                                        if let Some(board) = s.tactics_boards.get_mut(bid) {
                                            board.annotation_sync = Default::default();
                                            board.annotation_sync_version += 1;
                                        }
                                        s.tactics_boards_version += 1;
                                    } else if let Some(sync) = s.current_annotation_sync.as_mut() {
                                        sync.annotations.clear();
                                        sync.owners.clear();
                                        sync.ids.clear();
                                        s.annotation_sync_version += 1;
                                    }
                                    PeerMessage::ClearAnnotations { board_id: *board_id }
                                }
                            };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                        LocalEvent::DisplayToggle(field, value) => {
                            let _ = write_peer_message(&mut send, &PeerMessage::ToggleDisplayOption { field, value }).await;
                        }
                        LocalEvent::RangeOverrides(overrides) => {
                            let _ = write_peer_message(&mut send, &PeerMessage::ShipRangeOverrides { overrides }).await;
                        }
                        LocalEvent::TrailOverrides(hidden) => {
                            let _ = write_peer_message(&mut send, &PeerMessage::ShipTrailOverrides { hidden }).await;
                        }
                        LocalEvent::Ping(pos) => {
                            let my_color = mesh.lock().my_color;
                            let _ = write_peer_message(&mut send, &PeerMessage::Ping { user_id: my_user_id, pos, color: my_color }).await;
                        }
                        LocalEvent::CapPoint { board_id, event: evt } => {
                            let msg = match &evt {
                                LocalCapPointEvent::Set(cp) => {
                                    let mut s = ui_state.lock();
                                    if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                                        let sync = &mut board.cap_point_sync;
                                        if let Some(pos) = sync.cap_points.iter().position(|c| c.id == cp.id) {
                                            sync.cap_points[pos] = cp.clone();
                                        } else {
                                            sync.cap_points.push(cp.clone());
                                        }
                                        board.cap_point_sync_version += 1;
                                        s.tactics_boards_version += 1;
                                    }
                                    PeerMessage::SetCapPoint { board_id, cap_point: cp.clone() }
                                }
                                LocalCapPointEvent::Remove { id } => {
                                    let mut s = ui_state.lock();
                                    if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                                        board.cap_point_sync.cap_points.retain(|c| c.id != *id);
                                        board.cap_point_sync_version += 1;
                                        s.tactics_boards_version += 1;
                                    }
                                    PeerMessage::RemoveCapPoint { board_id, id: *id }
                                }
                            };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                        LocalEvent::TacticsMapOpened { board_id, owner_user_id, map_name, display_name, map_id, map_image_png, map_info } => {
                            {
                                let mut s = ui_state.lock();
                                let owner = if owner_user_id == 0 { s.my_user_id } else { owner_user_id };
                                let board = s.tactics_boards.entry(board_id).or_default();
                                board.owner_user_id = owner;
                                board.tactics_map = crate::collab::TacticsMapInfo {
                                    map_name: map_name.clone(),
                                    display_name: display_name.clone(),
                                    map_id,
                                    map_image_png: map_image_png.clone(),
                                    map_info: map_info.clone(),
                                };
                                s.tactics_boards_version += 1;
                            }
                            let owner = ui_state.lock().my_user_id;
                            let _ = write_peer_message(&mut send, &PeerMessage::TacticsMapOpened { board_id, owner_user_id: owner, map_name, display_name, map_id, map_image_png, map_info }).await;
                        }
                        LocalEvent::TacticsMapClosed { board_id } => {
                            {
                                let mut s = ui_state.lock();
                                s.tactics_boards.remove(&board_id);
                                s.tactics_boards_version += 1;
                            }
                            let _ = write_peer_message(&mut send, &PeerMessage::TacticsMapClosed { board_id }).await;
                        }
                    }
                }

                // If we're the frame source, broadcast compressed frames.
                if let Ok(data) = frame_bytes_tx.subscribe().try_recv() {
                    let _ = send.write_all(&data).await;
                }

                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            }
        }
    }

    // Cleanup.
    let _ = event_tx.send(SessionEvent::Ended);
    ui_state.lock().clear_session_data();
    info!("Left collab session");
}
/// Repaint all replay viewports registered in the session state.
/// Used for replay-scoped state changes (annotations with board_id=None,
/// render options, range/trail overrides).
fn repaint_replay_viewports(s: &SessionState) {
    for replay in &s.open_replays {
        s.repaint_viewport(replay.replay_id);
    }
}

/// Process an incoming peer message with client-side permission enforcement.
///
/// Called by both host_main (for messages from connected peers) and join_main
/// (for messages from the host). The sender's role is looked up in the mesh state.
fn handle_incoming_message(
    sender_id: u64,
    msg: PeerMessage,
    mesh: &Arc<Mutex<MeshState>>,
    ui_state: &Arc<Mutex<SessionState>>,
    event_tx: &mpsc::Sender<SessionEvent>,
) {
    let m = mesh.lock();
    let sender_is_authority = m.is_authority(sender_id);
    let sender_is_host = sender_id == m.host_user_id;
    let permissions = m.permissions.clone();
    let frame_source_id = m.frame_source_id;
    drop(m);

    match msg {
        // ── Always accept ───────────────────────────────────────────────
        PeerMessage::CursorPosition { pos, .. } => {
            // Use sender_id from the connection context (not the message's user_id)
            // to prevent peers from spoofing other users' cursors.
            {
                let mut s = ui_state.lock();
                if let Some(c) = s.cursors.iter_mut().find(|c| c.user_id == sender_id) {
                    c.pos = pos;
                    c.last_update = Instant::now();
                } else {
                    // Unknown peer cursor — might have been announced via UserJoined.
                    let m = mesh.lock();
                    let (name, color) = if let Some(p) = m.peers.get(&sender_id) {
                        (p.name.clone(), p.color)
                    } else {
                        (format!("Peer {sender_id}"), [200, 200, 200])
                    };
                    drop(m);
                    s.cursors.push(UserCursor { user_id: sender_id, name, color, pos, last_update: Instant::now() });
                }
            }
            // Repaint all viewports that render cursors.
            {
                let s = ui_state.lock();
                repaint_replay_viewports(&s);
                for &bid in s.tactics_boards.keys() {
                    s.repaint_viewport(bid);
                }
            }
            // Host relays cursor updates to other peers with the real sender_id.
            relay_if_host(sender_id, &PeerMessage::CursorPosition { user_id: sender_id, pos }, mesh);
        }

        PeerMessage::MeshHello { user_id, name, color } => {
            debug!("MeshHello from {user_id} ({name})");
            // Register peer if not already known.
            let _already_known = {
                let m = mesh.lock();
                m.peers.contains_key(&user_id)
            };
            // MeshHello is mostly informational in the host-mediated model.
            let _ = (user_id, name, color);
        }

        // ── Annotation gated ────────────────────────────────────────────
        PeerMessage::SetAnnotation { board_id, id, ref annotation, owner } => {
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping SetAnnotation from {sender_id} (locked)");
                return;
            }
            if let Err(e) = validate_annotation(annotation) {
                warn!("Invalid annotation from {sender_id}: {e}");
                return;
            }
            {
                let mut s = ui_state.lock();
                let sync = if let Some(bid) = board_id {
                    if let Some(board) = s.tactics_boards.get_mut(&bid) {
                        &mut board.annotation_sync
                    } else {
                        return;
                    }
                } else {
                    s.current_annotation_sync.get_or_insert_with(Default::default)
                };
                if let Some(pos) = sync.ids.iter().position(|&eid| eid == id) {
                    sync.annotations[pos] = annotation.clone();
                    sync.owners[pos] = owner;
                } else {
                    sync.annotations.push(annotation.clone());
                    sync.owners.push(owner);
                    sync.ids.push(id);
                }
                if let Some(bid) = board_id {
                    if let Some(board) = s.tactics_boards.get_mut(&bid) {
                        board.annotation_sync_version += 1;
                    }
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(bid);
                } else {
                    s.annotation_sync_version += 1;
                    repaint_replay_viewports(&s);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::RemoveAnnotation { board_id, id } => {
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping RemoveAnnotation from {sender_id} (locked)");
                return;
            }
            {
                let mut s = ui_state.lock();
                let sync = if let Some(bid) = board_id {
                    if let Some(board) = s.tactics_boards.get_mut(&bid) {
                        &mut board.annotation_sync
                    } else {
                        return;
                    }
                } else if let Some(ref mut sync) = s.current_annotation_sync {
                    sync
                } else {
                    return;
                };
                if let Some(pos) = sync.ids.iter().position(|&eid| eid == id) {
                    sync.annotations.remove(pos);
                    sync.owners.remove(pos);
                    sync.ids.remove(pos);
                }
                if let Some(bid) = board_id {
                    if let Some(board) = s.tactics_boards.get_mut(&bid) {
                        board.annotation_sync_version += 1;
                    }
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(bid);
                } else {
                    s.annotation_sync_version += 1;
                    repaint_replay_viewports(&s);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::ClearAnnotations { board_id } => {
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping ClearAnnotations from {sender_id} (locked)");
                return;
            }
            {
                let mut s = ui_state.lock();
                if let Some(bid) = board_id {
                    if let Some(board) = s.tactics_boards.get_mut(&bid) {
                        board.annotation_sync = Default::default();
                        board.annotation_sync_version += 1;
                    }
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(bid);
                } else if let Some(sync) = s.current_annotation_sync.as_mut() {
                    sync.annotations.clear();
                    sync.owners.clear();
                    sync.ids.clear();
                    s.annotation_sync_version += 1;
                    repaint_replay_viewports(&s);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        // ── Settings gated ──────────────────────────────────────────────
        PeerMessage::ToggleDisplayOption { field, value } => {
            if permissions.settings_locked && !sender_is_authority {
                debug!("Dropping ToggleDisplayOption from {sender_id} (locked)");
                return;
            }
            // Apply the toggle to session render options so the renderer picks it up.
            {
                let mut s = ui_state.lock();
                let opts = s.current_render_options.get_or_insert_with(Default::default);
                opts.set_field(field, value);
                s.render_options_version += 1;
                repaint_replay_viewports(&s);
            }
            relay_if_host(sender_id, &PeerMessage::ToggleDisplayOption { field, value }, mesh);
        }

        PeerMessage::ShipRangeOverrides { ref overrides } => {
            if permissions.settings_locked && !sender_is_authority {
                debug!("Dropping ShipRangeOverrides from {sender_id} (locked)");
                return;
            }
            {
                let mut s = ui_state.lock();
                s.current_range_overrides = Some(overrides.clone());
                s.range_override_version += 1;
                repaint_replay_viewports(&s);
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::ShipTrailOverrides { ref hidden } => {
            if permissions.settings_locked && !sender_is_authority {
                debug!("Dropping ShipTrailOverrides from {sender_id} (locked)");
                return;
            }
            {
                let mut s = ui_state.lock();
                s.current_trail_hidden = Some(hidden.clone());
                s.trail_override_version += 1;
                repaint_replay_viewports(&s);
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::Ping { pos, color, .. } => {
            {
                let mut s = ui_state.lock();
                s.pings.push(crate::collab::PeerPing { user_id: sender_id, color, pos, time: Instant::now() });
                repaint_replay_viewports(&s);
            }
            // Relay with the real sender_id and their color.
            relay_if_host(sender_id, &PeerMessage::Ping { user_id: sender_id, pos, color }, mesh);
        }

        // ── Authority-only ──────────────────────────────────────────────
        PeerMessage::Permissions { annotations_locked, settings_locked } => {
            if !sender_is_authority {
                debug!("Dropping Permissions from non-authority {sender_id}");
                return;
            }
            mesh.lock().permissions = Permissions { annotations_locked, settings_locked };
            {
                let mut s = ui_state.lock();
                s.permissions.annotations_locked = annotations_locked;
                s.permissions.settings_locked = settings_locked;
            }
            relay_if_host(sender_id, &PeerMessage::Permissions { annotations_locked, settings_locked }, mesh);
        }

        PeerMessage::RenderOptions(ref opts) => {
            if !sender_is_authority {
                debug!("Dropping RenderOptions from non-authority {sender_id}");
                return;
            }
            {
                let mut s = ui_state.lock();
                s.render_options_version += 1;
                s.current_render_options = Some(opts.clone());
                repaint_replay_viewports(&s);
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::AnnotationSync { board_id, ref annotations, ref owners, ref ids } => {
            if !sender_is_authority {
                debug!("Dropping AnnotationSync from non-authority {sender_id}");
                return;
            }
            {
                let mut s = ui_state.lock();
                if let Some(bid) = board_id {
                    if let Some(board) = s.tactics_boards.get_mut(&bid) {
                        board.annotation_sync = crate::collab::AnnotationSyncState {
                            annotations: annotations.clone(),
                            owners: owners.clone(),
                            ids: ids.clone(),
                        };
                        board.annotation_sync_version += 1;
                    }
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(bid);
                } else {
                    s.annotation_sync_version += 1;
                    s.current_annotation_sync = Some(crate::collab::AnnotationSyncState {
                        annotations: annotations.clone(),
                        owners: owners.clone(),
                        ids: ids.clone(),
                    });
                    repaint_replay_viewports(&s);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::PlaybackState { .. } => {
            if !sender_is_authority {
                debug!("Dropping PlaybackState from non-authority {sender_id}");
                return;
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        // ── Host-only roster management ─────────────────────────────────
        PeerMessage::UserJoined { user_id, name, color } => {
            if !sender_is_host {
                debug!("Dropping UserJoined from non-host {sender_id}");
                return;
            }
            // TODO: UserJoined doesn't carry client_type yet — default to Desktop.
            let user = ConnectedUser {
                id: user_id,
                name: name.clone(),
                color,
                role: PeerRole::Peer,
                client_type: ClientType::Desktop { toolkit_version: String::new() },
            };
            {
                let mut s = ui_state.lock();
                s.connected_users.push(user.clone());
            }
            let _ = event_tx.send(SessionEvent::UserJoined(user));
        }

        PeerMessage::UserLeft { user_id } => {
            if !sender_is_host {
                debug!("Dropping UserLeft from non-host {sender_id}");
                return;
            }
            let left_name = {
                let mut s = ui_state.lock();
                let name =
                    s.connected_users.iter().find(|u| u.id == user_id).map(|u| u.name.clone()).unwrap_or_default();
                s.connected_users.retain(|u| u.id != user_id);
                s.cursors.retain(|c| c.user_id != user_id);
                name
            };
            mesh.lock().peers.remove(&user_id);
            let _ = event_tx.send(SessionEvent::UserLeft { user_id, name: left_name, timed_out: false });
        }

        // ── Co-host promotion (host only) ───────────────────────────────
        PeerMessage::PromoteToCoHost { user_id } => {
            if !sender_is_host {
                debug!("Dropping PromoteToCoHost from non-host {sender_id}");
                return;
            }
            let mut m = mesh.lock();
            let is_me = user_id == m.my_user_id;
            if let Some(peer) = m.peers.get_mut(&user_id) {
                peer.role = PeerRole::CoHost;
            }
            if is_me {
                m.my_role = PeerRole::CoHost;
            }
            drop(m);
            {
                let mut s = ui_state.lock();
                if is_me {
                    s.role = PeerRole::CoHost;
                }
                if let Some(u) = s.connected_users.iter_mut().find(|u| u.id == user_id) {
                    u.role = PeerRole::CoHost;
                }
            }
            let _ = event_tx.send(SessionEvent::PeerPromoted { user_id });
        }

        // ── Frame sourcing ──────────────────────────────────────────────
        PeerMessage::FrameSourceChanged { source_user_id } => {
            if !sender_is_authority {
                debug!("Dropping FrameSourceChanged from non-authority {sender_id}");
                return;
            }
            mesh.lock().frame_source_id = source_user_id;
            {
                let mut s = ui_state.lock();
                s.frame_source_id = source_user_id;
            }
            let _ = event_tx.send(SessionEvent::FrameSourceChanged { source_user_id });
            relay_if_host(sender_id, &PeerMessage::FrameSourceChanged { source_user_id }, mesh);
        }

        PeerMessage::Frame { replay_id, clock, frame_index, total_frames, game_duration, commands } => {
            if sender_id != frame_source_id {
                debug!("Dropping Frame from {sender_id} (frame source is {frame_source_id})");
                return;
            }
            if let Err(e) = validate_frame_commands_count(commands.len()) {
                warn!("{e}");
                return;
            }
            let frame = PlaybackFrame {
                replay_id,
                commands: commands.clone(),
                clock: wowsunpack::game_types::GameClock(clock),
                frame_index: frame_index as usize,
                total_frames: total_frames as usize,
                game_duration,
            };
            trace!("Received frame: replay_id={replay_id} frame={frame_index}/{total_frames} clock={clock:.1}");
            ui_state.lock().push_frame(replay_id, frame);
            // Host relays frames to other peers.
            relay_if_host(
                sender_id,
                &PeerMessage::Frame { replay_id, clock, frame_index, total_frames, game_duration, commands },
                mesh,
            );
        }

        // ── Replay lifecycle (host -> all peers) ────────────────────────
        PeerMessage::ReplayOpened { replay_id, replay_name, map_image_png, game_version, map_name, display_name } => {
            if !sender_is_host {
                debug!("Dropping ReplayOpened from non-host {sender_id}");
                return;
            }
            {
                let mut s = ui_state.lock();
                if !s.open_replays.iter().any(|r| r.replay_id == replay_id) {
                    s.open_replays.push(OpenReplay {
                        replay_id,
                        replay_name: replay_name.clone(),
                        map_image_png: map_image_png.clone(),
                        game_version: game_version.clone(),
                        map_name: map_name.clone(),
                        display_name: display_name.clone(),
                    });
                }
            }
            let _ = event_tx.send(SessionEvent::ReplayOpened {
                replay_id,
                replay_name,
                map_image_png,
                game_version,
                map_name,
                display_name,
            });
        }

        PeerMessage::ReplayClosed { replay_id } => {
            if !sender_is_host {
                debug!("Dropping ReplayClosed from non-host {sender_id}");
                return;
            }
            {
                let mut s = ui_state.lock();
                s.open_replays.retain(|r| r.replay_id != replay_id);
                s.current_annotation_sync = None;
                s.annotation_sync_version += 1;
            }
            let _ = event_tx.send(SessionEvent::ReplayClosed { replay_id });
        }

        // ── Tactics board messages ────────────────────────────────────
        PeerMessage::TacticsMapOpened {
            board_id,
            owner_user_id,
            map_name,
            display_name,
            map_id,
            map_image_png,
            map_info,
        } => {
            {
                let mut s = ui_state.lock();
                if !s.tactics_boards.contains_key(&board_id) && s.tactics_boards.len() >= protocol::MAX_TACTICS_BOARDS {
                    debug!("Dropping TacticsMapOpened from {sender_id}: max boards reached");
                    return;
                }
                let board = s.tactics_boards.entry(board_id).or_default();
                board.owner_user_id = owner_user_id;
                board.tactics_map = crate::collab::TacticsMapInfo {
                    map_name: map_name.clone(),
                    display_name: display_name.clone(),
                    map_id,
                    map_image_png: map_image_png.clone(),
                    map_info: map_info.clone(),
                };
                s.tactics_boards_version += 1;
                s.repaint_viewport(board_id);
            }
            relay_if_host(
                sender_id,
                &PeerMessage::TacticsMapOpened {
                    board_id,
                    owner_user_id,
                    map_name,
                    display_name,
                    map_id,
                    map_image_png,
                    map_info,
                },
                mesh,
            );
        }

        PeerMessage::TacticsMapClosed { board_id } => {
            {
                let mut s = ui_state.lock();
                s.tactics_boards.remove(&board_id);
                s.tactics_boards_version += 1;
                s.repaint_viewport(board_id);
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::SetCapPoint { board_id, ref cap_point } => {
            debug!(
                "Received SetCapPoint from {sender_id}: board={board_id} id={} radius={}",
                cap_point.id, cap_point.radius
            );
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping SetCapPoint from {sender_id} (locked)");
                return;
            }
            {
                let mut s = ui_state.lock();
                if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                    let sync = &mut board.cap_point_sync;
                    if let Some(pos) = sync.cap_points.iter().position(|c| c.id == cap_point.id) {
                        sync.cap_points[pos] = cap_point.clone();
                    } else {
                        sync.cap_points.push(cap_point.clone());
                    }
                    board.cap_point_sync_version += 1;
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(board_id);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::RemoveCapPoint { board_id, id } => {
            debug!("Received RemoveCapPoint from {sender_id}: board={board_id} id={id}");
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping RemoveCapPoint from {sender_id} (locked)");
                return;
            }
            {
                let mut s = ui_state.lock();
                if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                    board.cap_point_sync.cap_points.retain(|c| c.id != id);
                    board.cap_point_sync_version += 1;
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(board_id);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::CapPointSync { board_id, ref cap_points } => {
            debug!(
                "Received CapPointSync from {sender_id}: board={board_id} {} caps, authority={sender_is_authority}",
                cap_points.len()
            );
            if !sender_is_authority {
                debug!("Dropping CapPointSync from non-authority {sender_id}");
                return;
            }
            {
                let mut s = ui_state.lock();
                if let Some(board) = s.tactics_boards.get_mut(&board_id) {
                    board.cap_point_sync = crate::collab::CapPointSyncState { cap_points: cap_points.clone() };
                    board.cap_point_sync_version += 1;
                    s.tactics_boards_version += 1;
                    s.repaint_viewport(board_id);
                }
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::OpenWindowForEveryone { window_id } => {
            if !sender_is_authority {
                debug!("Dropping OpenWindowForEveryone from non-authority {sender_id}");
                return;
            }
            ui_state.lock().force_open_window_ids.insert(window_id);
            relay_if_host(sender_id, &msg, mesh);
        }

        // ── Asset bundle (sent by host to web clients, ignored by desktop) ──
        PeerMessage::AssetBundle { .. } => {
            debug!("Ignoring AssetBundle from {sender_id} (desktop client)");
        }

        // ── Asset request (handled in host_main loop, should not reach here) ──
        PeerMessage::RequestAssets => {
            debug!("Ignoring RequestAssets from {sender_id} in handle_incoming_message");
        }

        // ── Handshake messages (not expected post-handshake) ────────────
        PeerMessage::Join { .. }
        | PeerMessage::SessionInfo { .. }
        | PeerMessage::Rejected { .. }
        | PeerMessage::PeerAnnounce { .. } => {
            debug!("Ignoring handshake message from {sender_id} post-handshake");
        }

        // ── Heartbeat (handled at connection level, should not reach here) ──
        PeerMessage::Heartbeat => {
            error!("Heartbeat reached handle_incoming_message from {sender_id} — should be intercepted earlier");
        }
    }

    // Wake the main window so it can process session events.
    let s = ui_state.lock();
    if let Some(ctx) = &s.egui_ctx {
        ctx.request_repaint();
    }
}
/// Broadcast a peer message to all connected peers in the mesh.
fn broadcast_to_mesh(mesh: &Arc<Mutex<MeshState>>, msg: &PeerMessage) {
    let framed = match frame_peer_message(msg) {
        Ok(f) => Arc::new(f),
        Err(e) => {
            warn!("Failed to serialize broadcast: {e}");
            return;
        }
    };
    let m = mesh.lock();
    for peer in m.peers.values() {
        let _ = peer.msg_tx.try_send(Arc::clone(&framed));
    }
}

/// If we are the host, relay a message to all peers except the sender.
/// In the current host-mediated model, the host must relay messages between
/// peers since they don't have direct connections.
fn relay_if_host(sender_id: u64, msg: &PeerMessage, mesh: &Arc<Mutex<MeshState>>) {
    let m = mesh.lock();
    if !m.my_role.is_host() {
        return;
    }
    let framed = match frame_peer_message(msg) {
        Ok(f) => Arc::new(f),
        Err(e) => {
            warn!("Failed to serialize relay: {e}");
            return;
        }
    };
    for peer in m.peers.values() {
        if peer.user_id != sender_id {
            let _ = peer.msg_tx.try_send(Arc::clone(&framed));
        }
    }
}

/// Serialize a frame broadcast into wire-ready bytes.
fn serialize_frame(frame: &FrameBroadcast) -> Option<Vec<u8>> {
    let msg = PeerMessage::Frame {
        replay_id: frame.replay_id,
        clock: frame.clock,
        frame_index: frame.frame_index,
        total_frames: frame.total_frames,
        game_duration: frame.game_duration,
        commands: frame.commands.clone(),
    };
    frame_peer_message(&msg).ok()
}

fn set_status(state: &Arc<Mutex<SessionState>>, status: SessionStatus) {
    state.lock().status = status;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── MeshState ───────────────────────────────────────────────────────

    fn make_mesh_peer(user_id: u64, role: PeerRole) -> MeshPeer {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        MeshPeer { user_id, name: format!("Peer{user_id}"), color: [0; 3], role, msg_tx: tx, asset_request_count: 0 }
    }

    fn make_mesh_state(my_user_id: u64, my_role: PeerRole, peers: Vec<(u64, PeerRole)>) -> MeshState {
        let mut peer_map = HashMap::new();
        for (uid, role) in peers {
            peer_map.insert(uid, make_mesh_peer(uid, role));
        }
        MeshState {
            peers: peer_map,
            my_user_id,
            my_name: format!("Self{my_user_id}"),
            my_color: [255, 0, 0],
            my_role,
            host_user_id: 0,
            frame_source_id: 0,
            permissions: Permissions::default(),
        }
    }

    #[test]
    fn mesh_role_of_self() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![]);
        assert_eq!(mesh.role_of(0), PeerRole::Host);
    }

    #[test]
    fn mesh_role_of_known_peer() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![(1, PeerRole::CoHost), (2, PeerRole::Peer)]);
        assert_eq!(mesh.role_of(1), PeerRole::CoHost);
        assert_eq!(mesh.role_of(2), PeerRole::Peer);
    }

    #[test]
    fn mesh_role_of_unknown_peer_defaults_to_peer() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![]);
        assert_eq!(mesh.role_of(999), PeerRole::Peer);
    }

    #[test]
    fn mesh_is_authority_host() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![]);
        assert!(mesh.is_authority(0));
    }

    #[test]
    fn mesh_is_authority_cohost() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![(1, PeerRole::CoHost)]);
        assert!(mesh.is_authority(1));
    }

    #[test]
    fn mesh_is_not_authority_peer() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![(2, PeerRole::Peer)]);
        assert!(!mesh.is_authority(2));
    }

    #[test]
    fn mesh_is_not_authority_unknown() {
        let mesh = make_mesh_state(0, PeerRole::Host, vec![]);
        assert!(!mesh.is_authority(999));
    }

    // ─── handle_incoming_message test harness ────────────────────────────

    /// Test harness for `handle_incoming_message`.
    ///
    /// Sets up mesh state, UI state, and channels, then dispatches a message
    /// and returns the resulting state for assertions.
    struct MessageTestHarness {
        mesh: Arc<Mutex<MeshState>>,
        ui_state: Arc<Mutex<SessionState>>,
        event_rx: mpsc::Receiver<SessionEvent>,
        event_tx: mpsc::Sender<SessionEvent>,
    }

    impl MessageTestHarness {
        /// Create a harness where we are a client (Peer role) receiving from the host.
        fn as_client() -> Self {
            Self::new(1, PeerRole::Peer, vec![(0, PeerRole::Host)], 0)
        }

        fn new(my_id: u64, my_role: PeerRole, peers: Vec<(u64, PeerRole)>, host_id: u64) -> Self {
            let mesh = Arc::new(Mutex::new({
                let mut m = make_mesh_state(my_id, my_role, peers);
                m.host_user_id = host_id;
                m
            }));
            let ui_state = Arc::new(Mutex::new(SessionState::default()));
            let (event_tx, event_rx) = mpsc::channel();
            Self { mesh, ui_state, event_rx, event_tx }
        }

        fn with_permissions(self, annotations_locked: bool, settings_locked: bool) -> Self {
            self.mesh.lock().permissions = Permissions { annotations_locked, settings_locked };
            self.ui_state.lock().permissions = Permissions { annotations_locked, settings_locked };
            self
        }

        fn with_frame_source(self, user_id: u64) -> Self {
            self.mesh.lock().frame_source_id = user_id;
            self
        }

        /// Register a viewport sink with a frame channel for a given replay_id
        /// and return the receiver.
        fn with_frame_channel(&self, replay_id: u64) -> mpsc::Receiver<PlaybackFrame> {
            let (tx, rx) = mpsc::sync_channel(1);
            self.ui_state.lock().register_viewport_sink(
                replay_id,
                crate::collab::ViewportSink {
                    frame_tx: Some(tx),
                    viewport_id: egui::ViewportId::from_hash_of(replay_id),
                },
            );
            rx
        }

        fn dispatch(&self, sender_id: u64, msg: PeerMessage) {
            handle_incoming_message(sender_id, msg, &self.mesh, &self.ui_state, &self.event_tx);
        }

        fn ui(&self) -> parking_lot::MutexGuard<'_, SessionState> {
            self.ui_state.lock()
        }
    }

    // ─── Permission enforcement: annotations ─────────────────────────────

    fn test_annotation() -> Annotation {
        Annotation::Circle { center: [100.0, 200.0], radius: 50.0, color: [255, 0, 0, 255], width: 3.0, filled: false }
    }

    #[test]
    fn set_annotation_accepted_when_unlocked() {
        let h = MessageTestHarness::as_client();
        let msg = PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 2 };
        h.dispatch(2, msg); // from a peer
        let s = h.ui();
        assert_eq!(s.annotation_sync_version, 1);
        let sync = s.current_annotation_sync.as_ref().unwrap();
        assert_eq!(sync.ids, vec![1]);
    }

    #[test]
    fn set_annotation_dropped_when_locked_from_peer() {
        let h = MessageTestHarness::as_client().with_permissions(true, false);
        let msg = PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 2 };
        // Sender 2 is not authority (not in our peer list as co-host), so should be dropped.
        // But wait — in our client harness, peers are [(0, Host)]. Sender 2 is unknown -> defaults to Peer role.
        h.dispatch(2, msg);
        assert_eq!(h.ui().annotation_sync_version, 0);
    }

    #[test]
    fn set_annotation_accepted_when_locked_from_authority() {
        let h = MessageTestHarness::as_client().with_permissions(true, false);
        let msg = PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 };
        h.dispatch(0, msg); // from the host (authority)
        assert_eq!(h.ui().annotation_sync_version, 1);
    }

    #[test]
    fn set_annotation_upserts_by_id() {
        let h = MessageTestHarness::as_client();
        let ann1 = Annotation::Circle {
            center: [100.0, 200.0],
            radius: 50.0,
            color: [255, 0, 0, 255],
            width: 3.0,
            filled: false,
        };
        let ann2 = Annotation::Circle {
            center: [300.0, 400.0],
            radius: 25.0,
            color: [0, 255, 0, 255],
            width: 5.0,
            filled: true,
        };
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 42, annotation: ann1, owner: 0 });
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 42, annotation: ann2, owner: 0 });

        let s = h.ui();
        let sync = s.current_annotation_sync.as_ref().unwrap();
        assert_eq!(sync.ids.len(), 1, "Should upsert, not append");
        assert_eq!(sync.ids[0], 42);
        // Should be updated to the second annotation (center 300, 400).
        match &sync.annotations[0] {
            Annotation::Circle { center, .. } => {
                assert!((center[0] - 300.0).abs() < f32::EPSILON);
            }
            _ => panic!("Expected Circle"),
        }
    }

    #[test]
    fn remove_annotation_accepted_when_unlocked() {
        let h = MessageTestHarness::as_client();
        // First add one.
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });
        assert_eq!(h.ui().annotation_sync_version, 1);

        // Then remove it.
        h.dispatch(0, PeerMessage::RemoveAnnotation { board_id: None, id: 1 });
        let s = h.ui();
        assert_eq!(s.annotation_sync_version, 2);
        let sync = s.current_annotation_sync.as_ref().unwrap();
        assert!(sync.ids.is_empty());
    }

    #[test]
    fn remove_annotation_dropped_when_locked_from_peer() {
        let h = MessageTestHarness::as_client().with_permissions(true, false);
        // Add via authority.
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });
        // Try to remove from unknown peer.
        h.dispatch(99, PeerMessage::RemoveAnnotation { board_id: None, id: 1 });
        // Should still have the annotation.
        let sync = h.ui().current_annotation_sync.as_ref().unwrap().clone();
        assert_eq!(sync.ids.len(), 1);
    }

    #[test]
    fn clear_annotations_accepted_when_unlocked() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 2, annotation: test_annotation(), owner: 0 });

        h.dispatch(99, PeerMessage::ClearAnnotations { board_id: None }); // from anyone, unlocked
        let sync = h.ui().current_annotation_sync.as_ref().unwrap().clone();
        assert!(sync.ids.is_empty());
    }

    #[test]
    fn clear_annotations_dropped_when_locked_from_peer() {
        let h = MessageTestHarness::as_client().with_permissions(true, false);
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });

        h.dispatch(99, PeerMessage::ClearAnnotations { board_id: None });
        let sync = h.ui().current_annotation_sync.as_ref().unwrap().clone();
        assert_eq!(sync.ids.len(), 1, "Clear should have been dropped");
    }

    #[test]
    fn clear_annotations_accepted_when_locked_from_authority() {
        let h = MessageTestHarness::as_client().with_permissions(true, false);
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });

        h.dispatch(0, PeerMessage::ClearAnnotations { board_id: None }); // host is authority
        let sync = h.ui().current_annotation_sync.as_ref().unwrap().clone();
        assert!(sync.ids.is_empty());
    }

    // ─── Permission enforcement: settings ────────────────────────────────

    #[test]
    fn toggle_display_accepted_when_unlocked() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::ToggleDisplayOption { field: DisplayOptionField::ShowHpBars, value: true });
        let s = h.ui();
        assert_eq!(s.render_options_version, 1);
        assert!(s.current_render_options.as_ref().unwrap().show_hp_bars);
    }

    #[test]
    fn toggle_display_dropped_when_locked_from_peer() {
        let h = MessageTestHarness::as_client().with_permissions(false, true);
        h.dispatch(99, PeerMessage::ToggleDisplayOption { field: DisplayOptionField::ShowHpBars, value: true });
        assert_eq!(h.ui().render_options_version, 0);
    }

    #[test]
    fn toggle_display_accepted_when_locked_from_authority() {
        let h = MessageTestHarness::as_client().with_permissions(false, true);
        h.dispatch(0, PeerMessage::ToggleDisplayOption { field: DisplayOptionField::ShowTracers, value: true });
        assert_eq!(h.ui().render_options_version, 1);
        assert!(h.ui().current_render_options.as_ref().unwrap().show_tracers);
    }

    #[test]
    fn ship_range_overrides_dropped_when_locked_from_peer() {
        let h = MessageTestHarness::as_client().with_permissions(false, true);
        h.dispatch(99, PeerMessage::ShipRangeOverrides { overrides: vec![] });
        assert_eq!(h.ui().range_override_version, 0);
    }

    #[test]
    fn ship_trail_overrides_dropped_when_locked_from_peer() {
        let h = MessageTestHarness::as_client().with_permissions(false, true);
        h.dispatch(99, PeerMessage::ShipTrailOverrides { hidden: vec!["P1".into()] });
        assert_eq!(h.ui().trail_override_version, 0);
    }

    #[test]
    fn ship_trail_overrides_accepted_when_unlocked() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::ShipTrailOverrides { hidden: vec!["P1".into()] });
        assert_eq!(h.ui().trail_override_version, 1);
        assert_eq!(h.ui().current_trail_hidden.as_ref().unwrap(), &["P1"]);
    }

    // ─── Permission enforcement: authority-only ──────────────────────────

    #[test]
    fn permissions_accepted_from_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::Permissions { annotations_locked: true, settings_locked: true });
        let s = h.ui();
        assert!(s.permissions.annotations_locked);
        assert!(s.permissions.settings_locked);
    }

    #[test]
    fn permissions_dropped_from_non_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::Permissions { annotations_locked: true, settings_locked: true });
        let s = h.ui();
        assert!(!s.permissions.annotations_locked);
        assert!(!s.permissions.settings_locked);
    }

    #[test]
    fn render_options_accepted_from_authority() {
        let h = MessageTestHarness::as_client();
        let mut opts = CollabRenderOptions::default();
        opts.show_chat = true;
        h.dispatch(0, PeerMessage::RenderOptions(opts));
        assert_eq!(h.ui().render_options_version, 1);
        assert!(h.ui().current_render_options.as_ref().unwrap().show_chat);
    }

    #[test]
    fn render_options_dropped_from_non_authority() {
        let h = MessageTestHarness::as_client();
        let mut opts = CollabRenderOptions::default();
        opts.show_chat = true;
        h.dispatch(99, PeerMessage::RenderOptions(opts));
        assert_eq!(h.ui().render_options_version, 0);
    }

    #[test]
    fn annotation_sync_accepted_from_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            0,
            PeerMessage::AnnotationSync {
                board_id: None,
                annotations: vec![test_annotation()],
                owners: vec![0],
                ids: vec![42],
            },
        );
        let s = h.ui();
        assert_eq!(s.annotation_sync_version, 1);
        let sync = s.current_annotation_sync.as_ref().unwrap();
        assert_eq!(sync.ids, vec![42]);
    }

    #[test]
    fn annotation_sync_dropped_from_non_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            99,
            PeerMessage::AnnotationSync {
                board_id: None,
                annotations: vec![test_annotation()],
                owners: vec![0],
                ids: vec![42],
            },
        );
        assert_eq!(h.ui().annotation_sync_version, 0);
        assert!(h.ui().current_annotation_sync.is_none());
    }

    #[test]
    fn playback_state_dropped_from_non_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::PlaybackState { playing: true, speed: 1.0 });
        // No state change expected — playback state isn't stored in UI state currently,
        // but the message should be silently dropped (no panic).
    }

    // ─── Permission enforcement: host-only ───────────────────────────────

    #[test]
    fn user_joined_accepted_from_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::UserJoined { user_id: 5, name: "Eve".into(), color: [0, 200, 83] });
        let s = h.ui();
        assert_eq!(s.connected_users.len(), 1);
        assert_eq!(s.connected_users[0].name, "Eve");
        assert_eq!(s.connected_users[0].id, 5);
    }

    #[test]
    fn user_joined_dropped_from_non_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::UserJoined { user_id: 5, name: "Eve".into(), color: [0; 3] });
        assert!(h.ui().connected_users.is_empty());
    }

    #[test]
    fn user_left_accepted_from_host() {
        let h = MessageTestHarness::as_client();
        // Add a user first.
        h.dispatch(0, PeerMessage::UserJoined { user_id: 5, name: "Eve".into(), color: [0; 3] });
        assert_eq!(h.ui().connected_users.len(), 1);
        // Remove them.
        h.dispatch(0, PeerMessage::UserLeft { user_id: 5 });
        assert!(h.ui().connected_users.is_empty());
    }

    #[test]
    fn user_left_dropped_from_non_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::UserJoined { user_id: 5, name: "Eve".into(), color: [0; 3] });
        h.dispatch(99, PeerMessage::UserLeft { user_id: 5 }); // from non-host
        assert_eq!(h.ui().connected_users.len(), 1, "Should not have been removed");
    }

    #[test]
    fn user_left_also_removes_cursor() {
        let h = MessageTestHarness::as_client();
        // Simulate cursor from peer 5.
        h.dispatch(0, PeerMessage::UserJoined { user_id: 5, name: "Eve".into(), color: [0; 3] });
        h.dispatch(5, PeerMessage::CursorPosition { user_id: 5, pos: Some([100.0, 200.0]) });
        assert!(!h.ui().cursors.is_empty());

        h.dispatch(0, PeerMessage::UserLeft { user_id: 5 });
        assert!(h.ui().cursors.iter().all(|c| c.user_id != 5));
    }

    // ─── Co-host promotion ───────────────────────────────────────────────

    #[test]
    fn promote_accepted_from_host() {
        // We are client (user_id=1, Peer role), host is user_id=0.
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::PromoteToCoHost { user_id: 1 });

        // Our role should be updated.
        let mesh = h.mesh.lock();
        assert_eq!(mesh.my_role, PeerRole::CoHost);
        drop(mesh);

        let s = h.ui();
        assert_eq!(s.role, PeerRole::CoHost);

        // Event should be emitted.
        let event = h.event_rx.try_recv().unwrap();
        match event {
            SessionEvent::PeerPromoted { user_id } => assert_eq!(user_id, 1),
            _ => panic!("Expected PeerPromoted event"),
        }
    }

    #[test]
    fn promote_dropped_from_non_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::PromoteToCoHost { user_id: 1 });
        // Role should not change.
        assert!(h.mesh.lock().my_role.is_peer());
    }

    #[test]
    fn promote_other_peer_updates_mesh() {
        // We are client (user_id=1), host promotes user 2.
        let h = MessageTestHarness::new(1, PeerRole::Peer, vec![(0, PeerRole::Host), (2, PeerRole::Peer)], 0);
        h.dispatch(0, PeerMessage::PromoteToCoHost { user_id: 2 });

        let mesh = h.mesh.lock();
        assert_eq!(mesh.peers.get(&2).unwrap().role, PeerRole::CoHost);
        assert!(mesh.my_role.is_peer()); // Our role unchanged.
    }

    // ─── Frame source ────────────────────────────────────────────────────

    #[test]
    fn frame_source_changed_accepted_from_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::FrameSourceChanged { source_user_id: 2 });
        assert_eq!(h.mesh.lock().frame_source_id, 2);
        assert_eq!(h.ui().frame_source_id, 2);
    }

    #[test]
    fn frame_source_changed_dropped_from_non_authority() {
        let h = MessageTestHarness::as_client();
        h.dispatch(99, PeerMessage::FrameSourceChanged { source_user_id: 99 });
        assert_eq!(h.mesh.lock().frame_source_id, 0, "Frame source should not change");
    }

    #[test]
    fn frame_dropped_from_wrong_source() {
        let h = MessageTestHarness::as_client().with_frame_source(0);

        // Send from user 99 (not the frame source).
        h.dispatch(
            99,
            PeerMessage::Frame {
                replay_id: 1,
                clock: 0.0,
                frame_index: 0,
                total_frames: 10,
                game_duration: 600.0,
                commands: vec![],
            },
        );
        // Frame should not arrive (no channel registered, and even if there were one,
        // the wrong sender should be rejected).
        let rx = h.with_frame_channel(1);
        // Re-dispatch to test with channel present.
        h.dispatch(
            99,
            PeerMessage::Frame {
                replay_id: 1,
                clock: 0.0,
                frame_index: 0,
                total_frames: 10,
                game_duration: 600.0,
                commands: vec![],
            },
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn frame_accepted_from_correct_source() {
        let h = MessageTestHarness::as_client().with_frame_source(0);
        let rx = h.with_frame_channel(1);

        h.dispatch(
            0,
            PeerMessage::Frame {
                replay_id: 1,
                clock: 30.0,
                frame_index: 5,
                total_frames: 100,
                game_duration: 1200.0,
                commands: vec![],
            },
        );
        let frame = rx.try_recv().expect("frame should arrive via channel");
        assert_eq!(frame.replay_id, 1);
        assert_eq!(frame.frame_index, 5);
    }

    // ─── Replay lifecycle ────────────────────────────────────────────────

    #[test]
    fn replay_opened_accepted_from_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            0,
            PeerMessage::ReplayOpened {
                replay_id: 1,
                replay_name: "test.wowsreplay".into(),
                map_image_png: vec![0u8; 10],
                game_version: "13.5".into(),
                map_name: "spaces/01_solomon_islands".into(),
                display_name: "Solomon Islands".into(),
            },
        );
        let s = h.ui();
        assert_eq!(s.open_replays.len(), 1);
        assert_eq!(s.open_replays[0].replay_id, 1);
    }

    #[test]
    fn replay_opened_dropped_from_non_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            99,
            PeerMessage::ReplayOpened {
                replay_id: 1,
                replay_name: "test.wowsreplay".into(),
                map_image_png: vec![],
                game_version: "13.5".into(),
                map_name: "spaces/01_solomon_islands".into(),
                display_name: "Solomon Islands".into(),
            },
        );
        assert!(h.ui().open_replays.is_empty());
    }

    #[test]
    fn replay_closed_removes_replay_and_clears_annotations() {
        let h = MessageTestHarness::as_client();
        // Open a replay.
        h.dispatch(
            0,
            PeerMessage::ReplayOpened {
                replay_id: 1,
                replay_name: "test.wowsreplay".into(),
                map_image_png: vec![],
                game_version: "13.5".into(),
                map_name: "spaces/01_solomon_islands".into(),
                display_name: "Solomon Islands".into(),
            },
        );
        // Add some annotation state.
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });

        // Close the replay.
        h.dispatch(0, PeerMessage::ReplayClosed { replay_id: 1 });
        let s = h.ui();
        assert!(s.open_replays.is_empty());
        assert!(s.current_annotation_sync.is_none(), "Annotations should be cleared on replay close");
    }

    #[test]
    fn replay_closed_dropped_from_non_host() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            0,
            PeerMessage::ReplayOpened {
                replay_id: 1,
                replay_name: "test.wowsreplay".into(),
                map_image_png: vec![],
                game_version: "13.5".into(),
                map_name: "spaces/01_solomon_islands".into(),
                display_name: "Solomon Islands".into(),
            },
        );
        h.dispatch(99, PeerMessage::ReplayClosed { replay_id: 1 });
        assert_eq!(h.ui().open_replays.len(), 1, "Should not have been closed");
    }

    // ─── Cursor handling ─────────────────────────────────────────────────

    #[test]
    fn cursor_position_creates_entry() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::CursorPosition { user_id: 0, pos: Some([100.0, 200.0]) });
        let s = h.ui();
        assert_eq!(s.cursors.len(), 1);
        assert_eq!(s.cursors[0].user_id, 0);
        assert_eq!(s.cursors[0].pos, Some([100.0, 200.0]));
    }

    #[test]
    fn cursor_position_updates_existing() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::CursorPosition { user_id: 0, pos: Some([100.0, 200.0]) });
        h.dispatch(0, PeerMessage::CursorPosition { user_id: 0, pos: Some([300.0, 400.0]) });
        let s = h.ui();
        assert_eq!(s.cursors.len(), 1);
        assert_eq!(s.cursors[0].pos, Some([300.0, 400.0]));
    }

    #[test]
    fn cursor_none_clears_position() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::CursorPosition { user_id: 0, pos: Some([100.0, 200.0]) });
        h.dispatch(0, PeerMessage::CursorPosition { user_id: 0, pos: None });
        let s = h.ui();
        assert_eq!(s.cursors.len(), 1);
        assert_eq!(s.cursors[0].pos, None);
    }

    // ─── Ping handling ───────────────────────────────────────────────────

    #[test]
    fn ping_adds_entry_when_settings_unlocked() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::Ping { user_id: 0, pos: [100.0, 200.0], color: [200, 200, 200] });
        let s = h.ui();
        assert_eq!(s.pings.len(), 1);
        assert_eq!(s.pings[0].user_id, 0);
        assert_eq!(s.pings[0].pos, [100.0, 200.0]);
    }

    #[test]
    fn ping_always_accepted() {
        // Pings are NOT settings-gated — they are always accepted.
        let h = MessageTestHarness::as_client().with_permissions(false, true);
        h.dispatch(99, PeerMessage::Ping { user_id: 99, pos: [100.0, 200.0], color: [200, 200, 200] });
        assert_eq!(h.ui().pings.len(), 1);
    }

    // ─── Handshake messages ignored post-handshake ───────────────────────

    #[test]
    fn join_ignored_post_handshake() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            99,
            PeerMessage::Join {
                name: "Hacker".into(),
                client_type: ClientType::Desktop { toolkit_version: "1.0".into() },
            },
        );
        // Should not crash or change state.
        assert!(h.ui().connected_users.is_empty());
    }

    #[test]
    fn session_info_ignored_post_handshake() {
        let h = MessageTestHarness::as_client();
        h.dispatch(
            99,
            PeerMessage::SessionInfo {
                toolkit_version: "1.0".into(),
                peers: vec![],
                assigned_identity: PeerIdentity { user_id: 99, name: "X".into(), color: [0; 3] },
                frame_source_id: 99,
                open_replays: vec![],
            },
        );
        // Should be silently ignored.
    }

    // ─── Co-host as authority ────────────────────────────────────────────

    #[test]
    fn cohost_can_send_authority_messages() {
        // We are client, host is 0, co-host is 2.
        let h = MessageTestHarness::new(1, PeerRole::Peer, vec![(0, PeerRole::Host), (2, PeerRole::CoHost)], 0);
        // Co-host sends Permissions — should be accepted.
        h.dispatch(2, PeerMessage::Permissions { annotations_locked: true, settings_locked: false });
        assert!(h.ui().permissions.annotations_locked);
    }

    #[test]
    fn cohost_can_send_render_options() {
        let h = MessageTestHarness::new(1, PeerRole::Peer, vec![(0, PeerRole::Host), (2, PeerRole::CoHost)], 0);
        let mut opts = CollabRenderOptions::default();
        opts.show_advantage = true;
        h.dispatch(2, PeerMessage::RenderOptions(opts));
        assert_eq!(h.ui().render_options_version, 1);
        assert!(h.ui().current_render_options.as_ref().unwrap().show_advantage);
    }

    #[test]
    fn cohost_cannot_promote() {
        // PromoteToCoHost is host-only (not just authority).
        let h = MessageTestHarness::new(1, PeerRole::Peer, vec![(0, PeerRole::Host), (2, PeerRole::CoHost)], 0);
        h.dispatch(2, PeerMessage::PromoteToCoHost { user_id: 1 });
        assert!(h.mesh.lock().my_role.is_peer(), "Co-host should not be able to promote");
    }

    #[test]
    fn cohost_cannot_send_user_joined() {
        let h = MessageTestHarness::new(1, PeerRole::Peer, vec![(0, PeerRole::Host), (2, PeerRole::CoHost)], 0);
        h.dispatch(2, PeerMessage::UserJoined { user_id: 5, name: "Eve".into(), color: [0; 3] });
        assert!(h.ui().connected_users.is_empty());
    }

    // ─── Version bumping ─────────────────────────────────────────────────

    #[test]
    fn version_bumps_on_annotation_mutations() {
        let h = MessageTestHarness::as_client();
        assert_eq!(h.ui().annotation_sync_version, 0);

        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });
        assert_eq!(h.ui().annotation_sync_version, 1);

        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 2, annotation: test_annotation(), owner: 0 });
        assert_eq!(h.ui().annotation_sync_version, 2);

        h.dispatch(0, PeerMessage::RemoveAnnotation { board_id: None, id: 1 });
        assert_eq!(h.ui().annotation_sync_version, 3);

        h.dispatch(0, PeerMessage::ClearAnnotations { board_id: None });
        assert_eq!(h.ui().annotation_sync_version, 4);
    }

    #[test]
    fn version_bumps_on_render_options_and_toggles() {
        let h = MessageTestHarness::as_client();
        assert_eq!(h.ui().render_options_version, 0);

        h.dispatch(0, PeerMessage::RenderOptions(CollabRenderOptions::default()));
        assert_eq!(h.ui().render_options_version, 1);

        h.dispatch(0, PeerMessage::ToggleDisplayOption { field: DisplayOptionField::ShowHpBars, value: true });
        assert_eq!(h.ui().render_options_version, 2);
    }

    #[test]
    fn annotation_sync_replaces_entire_state() {
        let h = MessageTestHarness::as_client();
        // Add two annotations individually.
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 1, annotation: test_annotation(), owner: 0 });
        h.dispatch(0, PeerMessage::SetAnnotation { board_id: None, id: 2, annotation: test_annotation(), owner: 0 });
        assert_eq!(h.ui().current_annotation_sync.as_ref().unwrap().ids.len(), 2);

        // Full sync replaces everything.
        h.dispatch(
            0,
            PeerMessage::AnnotationSync {
                board_id: None,
                annotations: vec![test_annotation()],
                owners: vec![5],
                ids: vec![99],
            },
        );
        let sync = h.ui().current_annotation_sync.as_ref().unwrap().clone();
        assert_eq!(sync.ids, vec![99]);
        assert_eq!(sync.owners, vec![5]);
        assert_eq!(sync.annotations.len(), 1);
    }

    // ─── Permissions update mesh state too ────────────────────────────────

    #[test]
    fn permissions_message_updates_mesh_state() {
        let h = MessageTestHarness::as_client();
        h.dispatch(0, PeerMessage::Permissions { annotations_locked: true, settings_locked: true });
        let m = h.mesh.lock();
        assert!(m.permissions.annotations_locked);
        assert!(m.permissions.settings_locked);
    }
}

//! Unified peer task for collaborative replay sessions (mesh topology).
//!
//! Every participant — host, co-host, or regular peer — runs the same code.
//! The host is the initial rendezvous and identity authority; all other
//! messaging flows peer-to-peer. Permission enforcement is client-side.
//!
//! # Connection lifecycle
//!
//! **Host mode:**
//! 1. Creates iroh endpoint, publishes token
//! 2. Accepts connections, handshakes (Join → SessionInfo + peer list)
//! 3. Broadcasts PeerAnnounce to existing peers so they accept the new joiner
//! 4. New joiner connects to each existing peer, sends MeshHello
//!
//! **Join mode:**
//! 1. Connects to host via token, sends Join
//! 2. Receives SessionInfo with peer list and assigned identity
//! 3. Connects to each peer in the list, sends MeshHello
//! 4. Accepts incoming MeshHello connections from peers notified via PeerAnnounce

use std::collections::HashMap;
use std::io::Read as _;
use std::io::Write as _;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Instant;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
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
use crate::collab::types::CURSOR_COLORS;
use crate::collab::validation::validate_annotation;
use crate::collab::validation::validate_frame_commands_count;
use crate::collab::validation::validate_peer_message;
use crate::replay_renderer::PlaybackFrame;

// ─── Public types ───────────────────────────────────────────────────────────

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
}

/// Parameters for joining a session.
pub struct JoinParams {
    /// Base64url-encoded EndpointAddr JSON.
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
    Add(Annotation),
    Undo,
}

/// Handle returned from `start_peer_session` for UI interaction.
pub struct PeerSessionHandle {
    /// Receive session events.
    pub event_rx: mpsc::Receiver<SessionEvent>,
    /// Send commands to the session.
    pub command_tx: mpsc::Sender<SessionCommand>,
    /// Send frames for broadcast (host/co-host only). `try_send` to avoid blocking.
    pub frame_tx: mpsc::SyncSender<FrameBroadcast>,
    /// Receive playback frames from the current frame source (join mode).
    pub frame_rx: mpsc::Receiver<PlaybackFrame>,
    /// Send cursor position updates.
    pub cursor_tx: mpsc::Sender<Option<[f32; 2]>>,
    /// Send annotation events.
    pub annotation_tx: mpsc::Sender<LocalAnnotationEvent>,
    /// Send display option toggles.
    pub display_toggle_tx: mpsc::Sender<(DisplayOptionField, bool)>,
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
    let (inbound_frame_tx, frame_rx) = mpsc::channel();
    let (cursor_tx, cursor_rx) = mpsc::channel();
    let (annotation_tx, annotation_rx) = mpsc::channel();
    let (display_toggle_tx, display_toggle_rx) = mpsc::channel();

    // Reset the provided state for this new session.
    let is_host = matches!(&mode, PeerMode::Host(_));
    {
        let mut s = state.lock().unwrap();
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

    runtime.spawn(peer_task(
        mode,
        event_tx,
        command_rx,
        frame_broadcast_rx,
        inbound_frame_tx,
        cursor_rx,
        annotation_rx,
        display_toggle_rx,
        state_clone,
    ));

    PeerSessionHandle { event_rx, command_tx, frame_tx, frame_rx, cursor_tx, annotation_tx, display_toggle_tx, state }
}

// ─── Internal types ─────────────────────────────────────────────────────────

/// A connected peer in the mesh (from our perspective).
struct MeshPeer {
    user_id: u64,
    name: String,
    color: [u8; 3],
    role: PeerRole,
    /// Channel for sending serialized (length-prefixed) messages to this peer's writer task.
    msg_tx: tokio::sync::mpsc::Sender<Arc<Vec<u8>>>,
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

// ─── Main peer task ─────────────────────────────────────────────────────────

async fn peer_task(
    mode: PeerMode,
    event_tx: mpsc::Sender<SessionEvent>,
    command_rx: mpsc::Receiver<SessionCommand>,
    frame_broadcast_rx: mpsc::Receiver<FrameBroadcast>,
    inbound_frame_tx: mpsc::Sender<PlaybackFrame>,
    cursor_rx: mpsc::Receiver<Option<[f32; 2]>>,
    annotation_rx: mpsc::Receiver<LocalAnnotationEvent>,
    display_toggle_rx: mpsc::Receiver<(DisplayOptionField, bool)>,
    ui_state: Arc<Mutex<SessionState>>,
) {
    match mode {
        PeerMode::Host(params) => {
            host_main(
                params,
                event_tx,
                command_rx,
                frame_broadcast_rx,
                inbound_frame_tx,
                cursor_rx,
                annotation_rx,
                display_toggle_rx,
                ui_state,
            )
            .await;
        }
        PeerMode::Join(params) => {
            join_main(
                params,
                event_tx,
                command_rx,
                frame_broadcast_rx,
                inbound_frame_tx,
                cursor_rx,
                annotation_rx,
                display_toggle_rx,
                ui_state,
            )
            .await;
        }
    }
}

// ─── Host main ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn host_main(
    params: HostParams,
    event_tx: mpsc::Sender<SessionEvent>,
    command_rx: mpsc::Receiver<SessionCommand>,
    frame_broadcast_rx: mpsc::Receiver<FrameBroadcast>,
    inbound_frame_tx: mpsc::Sender<PlaybackFrame>,
    cursor_rx: mpsc::Receiver<Option<[f32; 2]>>,
    annotation_rx: mpsc::Receiver<LocalAnnotationEvent>,
    display_toggle_rx: mpsc::Receiver<(DisplayOptionField, bool)>,
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

    // Generate the session token.
    let my_addr = endpoint.addr();
    let my_addr_json = match serde_json::to_string(&my_addr) {
        Ok(j) => j,
        Err(e) => {
            let msg = format!("Failed to serialize endpoint address: {e}");
            error!("{msg}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };
    let token = data_encoding::BASE64URL_NOPAD.encode(my_addr_json.as_bytes());

    let my_user_id = 0u64;
    let my_color = CURSOR_COLORS[0];

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
        let mut s = ui_state.lock().unwrap();
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
        });
    }
    let _ = event_tx.send(SessionEvent::Started);
    info!("Collab host session started");

    // Frame compression channel (broadcast to all peers).
    let (frame_bytes_tx, _) = tokio::sync::broadcast::channel::<Arc<Vec<u8>>>(4);
    let frame_bytes_tx_clone = frame_bytes_tx.clone();

    // Last compressed frame, sent to newly joining peers so they get an
    // immediate picture instead of waiting for the next frame tick.
    let last_frame_bytes: Arc<std::sync::Mutex<Option<Arc<Vec<u8>>>>> = Arc::new(std::sync::Mutex::new(None));
    let last_frame_clone = Arc::clone(&last_frame_bytes);

    // Spawn frame compression task.
    let _frame_task = tokio::task::spawn_blocking(move || {
        while let Ok(frame) = frame_broadcast_rx.recv() {
            if let Some(framed) = compress_frame(&frame) {
                let arc = Arc::new(framed);
                *last_frame_clone.lock().unwrap() = Some(Arc::clone(&arc));
                let _ = frame_bytes_tx_clone.send(arc);
            }
        }
    });

    let next_user_id = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let next_color_idx = Arc::new(std::sync::atomic::AtomicUsize::new(1));

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
                let color_idx = next_color_idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let color = CURSOR_COLORS[color_idx % CURSOR_COLORS.len()];

                let mesh_clone = Arc::clone(&mesh);
                let ui_state_clone = Arc::clone(&ui_state);
                let event_tx_clone = event_tx.clone();
                let peer_msg_tx_clone = peer_msg_tx.clone();
                let frame_rx = frame_bytes_tx.subscribe();
                let endpoint_clone = endpoint.clone();

                let toolkit_version = params.toolkit_version.clone();
                let initial_render_options = params.initial_render_options.clone();
                let open_replays: Vec<protocol::ReplayInfo> = ui_state.lock().unwrap().open_replays.iter().map(|r| {
                    protocol::ReplayInfo {
                        replay_id: r.replay_id,
                        replay_name: r.replay_name.clone(),
                        map_image_png: r.map_image_png.clone(),
                        game_version: r.game_version.clone(),
                    }
                }).collect();
                debug!("Peer {user_id} joining: {} open replay(s) in SessionInfo", open_replays.len());
                let last_frame_for_peer = Arc::clone(&last_frame_bytes);

                tokio::spawn(async move {
                    host_accept_peer(
                        conn,
                        user_id,
                        color,
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
                    )
                    .await;
                });
            }

            // Process incoming peer messages.
            Some((sender_id, msg)) = peer_msg_rx.recv() => {
                handle_incoming_message(
                    sender_id,
                    msg,
                    &mesh,
                    &ui_state,
                    &event_tx,
                    &inbound_frame_tx,
                );
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
                            let mut m = mesh.lock().unwrap();
                            m.permissions = p.clone();
                            drop(m);
                            if let Ok(mut s) = ui_state.lock() {
                                s.permissions = p;
                            }
                        }
                        SessionCommand::ResetClientOverrides => {
                            let msg = PeerMessage::RenderOptions(params.initial_render_options.clone());
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::SyncAnnotations { annotations, owners } => {
                            let msg = PeerMessage::AnnotationSync { annotations, owners };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::PromoteToCoHost { user_id } => {
                            let msg = PeerMessage::PromoteToCoHost { user_id };
                            broadcast_to_mesh(&mesh, &msg);
                            // Update local role map.
                            let mut m = mesh.lock().unwrap();
                            if let Some(peer) = m.peers.get_mut(&user_id) {
                                peer.role = PeerRole::CoHost;
                            }
                            drop(m);
                            if let Ok(mut s) = ui_state.lock()
                                && let Some(u) = s.connected_users.iter_mut().find(|u| u.id == user_id) {
                                    u.role = PeerRole::CoHost;
                                }
                            let _ = event_tx.send(SessionEvent::PeerPromoted { user_id });
                        }
                        SessionCommand::BecomeFrameSource => {
                            let msg = PeerMessage::FrameSourceChanged { source_user_id: my_user_id };
                            broadcast_to_mesh(&mesh, &msg);
                            mesh.lock().unwrap().frame_source_id = my_user_id;
                            if let Ok(mut s) = ui_state.lock() {
                                s.frame_source_id = my_user_id;
                            }
                            let _ = event_tx.send(SessionEvent::FrameSourceChanged { source_user_id: my_user_id });
                        }
                        SessionCommand::ReplayOpened { replay_id, replay_name, map_image_png, game_version } => {
                            // Store in session state.
                            if let Ok(mut s) = ui_state.lock() {
                                s.open_replays.push(OpenReplay {
                                    replay_id,
                                    replay_name: replay_name.clone(),
                                    map_image_png: map_image_png.clone(),
                                    game_version: game_version.clone(),
                                });
                            }
                            // Broadcast to all peers.
                            let msg = PeerMessage::ReplayOpened {
                                replay_id,
                                replay_name,
                                map_image_png,
                                game_version,
                            };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                        SessionCommand::ReplayClosed { replay_id } => {
                            // Remove from session state and clear annotations.
                            if let Ok(mut s) = ui_state.lock() {
                                s.open_replays.retain(|r| r.replay_id != replay_id);
                                s.current_annotation_sync = None;
                                s.annotation_sync_version += 1;
                            }
                            // Broadcast to all peers.
                            let msg = PeerMessage::ReplayClosed { replay_id };
                            broadcast_to_mesh(&mesh, &msg);
                        }
                    }
                }

                // Local cursor.
                while let Ok(pos) = cursor_rx.try_recv() {
                    if let Ok(mut s) = ui_state.lock()
                        && let Some(c) = s.cursors.iter_mut().find(|c| c.user_id == my_user_id) {
                            c.pos = pos;
                            c.last_update = Instant::now();
                        }
                    let msg = PeerMessage::CursorPosition(pos);
                    broadcast_to_mesh(&mesh, &msg);
                }

                // Local annotations.
                while let Ok(evt) = annotation_rx.try_recv() {
                    match &evt {
                        LocalAnnotationEvent::Add(ann) => {
                            if let Ok(mut s) = ui_state.lock() {
                                let (anns, owners) = s.current_annotation_sync.get_or_insert_with(|| (Vec::new(), Vec::new()));
                                anns.push(ann.clone());
                                owners.push(my_user_id);
                                s.annotation_sync_version += 1;
                            }
                        }
                        LocalAnnotationEvent::Undo => {
                            if let Ok(mut s) = ui_state.lock()
                                && let Some((anns, owners)) = s.current_annotation_sync.as_mut()
                                    && let Some(pos) = owners.iter().rposition(|&uid| uid == my_user_id) {
                                        anns.remove(pos);
                                        owners.remove(pos);
                                        s.annotation_sync_version += 1;
                                    }
                        }
                    }
                    let msg = match evt {
                        LocalAnnotationEvent::Add(ann) => PeerMessage::AddAnnotation(ann),
                        LocalAnnotationEvent::Undo => PeerMessage::UndoAnnotation,
                    };
                    broadcast_to_mesh(&mesh, &msg);
                }

                // Local display toggles.
                while let Ok((field, value)) = display_toggle_rx.try_recv() {
                    let msg = PeerMessage::ToggleDisplayOption { field, value };
                    broadcast_to_mesh(&mesh, &msg);
                }

                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            }
        }
    }

    // Cleanup.
    endpoint.close().await;
    let _ = event_tx.send(SessionEvent::Ended);
    if let Ok(mut s) = ui_state.lock() {
        s.status = SessionStatus::Idle;
        s.token = None;
        s.connected_users.clear();
        s.cursors.clear();
        s.open_replays.clear();
    }
    info!("Collab host session ended");
}

/// Handle an incoming connection on the host: handshake, register, announce.
#[allow(clippy::too_many_arguments)]
async fn host_accept_peer(
    conn: iroh::endpoint::Connection,
    user_id: u64,
    color: [u8; 3],
    toolkit_version: &str,
    initial_render_options: &CollabRenderOptions,
    open_replays: Vec<protocol::ReplayInfo>,
    endpoint: &Endpoint,
    mesh: Arc<Mutex<MeshState>>,
    ui_state: Arc<Mutex<SessionState>>,
    event_tx: mpsc::Sender<SessionEvent>,
    peer_msg_tx: tokio::sync::mpsc::Sender<(u64, PeerMessage)>,
    mut frame_rx: tokio::sync::broadcast::Receiver<Arc<Vec<u8>>>,
    last_frame_bytes: Arc<std::sync::Mutex<Option<Arc<Vec<u8>>>>>,
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

    let (client_name, client_version) = match &join_msg {
        PeerMessage::Join { toolkit_version, name } => (name.clone(), toolkit_version.clone()),
        _ => {
            warn!("Peer {user_id} sent non-Join as first message");
            return;
        }
    };

    // Validate version.
    if client_version != toolkit_version {
        let msg = PeerMessage::Rejected {
            reason: format!("Version mismatch: host is v{toolkit_version}, you have v{client_version}"),
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
        let m = mesh.lock().unwrap();
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

    // Send current permissions.
    let perm_msg = PeerMessage::Permissions {
        annotations_locked: current_perms.annotations_locked,
        settings_locked: current_perms.settings_locked,
    };
    if write_peer_message(&mut send, &perm_msg).await.is_err() {
        return;
    }

    // Send current render options.
    let opts_msg = PeerMessage::RenderOptions(initial_render_options.clone());
    if write_peer_message(&mut send, &opts_msg).await.is_err() {
        return;
    }

    // Send current annotations if any.
    let ann_msg = ui_state.lock().ok().and_then(|s| {
        s.current_annotation_sync.as_ref().and_then(|(anns, owners)| {
            if anns.is_empty() {
                None
            } else {
                Some(PeerMessage::AnnotationSync { annotations: anns.clone(), owners: owners.clone() })
            }
        })
    });
    if let Some(msg) = ann_msg
        && write_peer_message(&mut send, &msg).await.is_err()
    {
        return;
    }

    // Announce to all existing peers.
    // Since we don't have the new joiner's endpoint addr, existing peers can't
    // connect to them. Instead the new joiner won't try to connect to peers
    // (since the peer list only has the host). The host will relay messages.
    // For true mesh, we'd need the joiner to share their endpoint addr.
    // BUT: since all connections go through iroh's relay anyway, and the host
    // handles fan-out, this simplified model works. Each "peer" connection is
    // actually a host-mediated link using per-peer channels.
    //
    // Notify all existing peers about the new user.
    let join_notify = PeerMessage::UserJoined { user_id, name: client_name.clone(), color };
    broadcast_to_mesh(&mesh, &join_notify);

    // Create per-peer message channel.
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel::<Arc<Vec<u8>>>(64);

    // Register in mesh.
    let user = ConnectedUser { id: user_id, name: client_name.clone(), color, role: PeerRole::Peer };
    {
        let mut m = mesh.lock().unwrap();
        m.peers.insert(user_id, MeshPeer { user_id, name: client_name.clone(), color, role: PeerRole::Peer, msg_tx });
    }
    {
        let mut s = ui_state.lock().unwrap();
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
    if let Some(frame_data) = last_frame_bytes.lock().ok().and_then(|g| g.clone()) {
        info!("Sending last frame ({} bytes) to peer {user_id}", frame_data.len());
        if send.write_all(&frame_data).await.is_err() {
            return;
        }
    } else {
        info!("No last frame available for peer {user_id}");
    }

    // Message loop for this peer.
    loop {
        tokio::select! {
            // Biased: drain control messages (ReplayOpened, etc.) before frames,
            // so clients always create renderers before receiving frame data.
            biased;

            msg_result = host_read_rx.recv() => {
                match msg_result {
                    Some(Ok(msg)) => {
                        if let Err(e) = validate_peer_message(&msg) {
                            warn!("Peer {user_id} sent invalid message: {e}");
                            continue;
                        }
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
        }
    }

    // Cleanup.
    {
        mesh.lock().unwrap().peers.remove(&user_id);
    }
    {
        let mut s = ui_state.lock().unwrap();
        s.connected_users.retain(|u| u.id != user_id);
        s.cursors.retain(|c| c.user_id != user_id);
    }
    let leave_msg = PeerMessage::UserLeft { user_id };
    broadcast_to_mesh(&mesh, &leave_msg);
    let _ = event_tx.send(SessionEvent::UserLeft { user_id });
    info!("Peer {user_id} ({client_name}) left session");
}

// ─── Join main ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn join_main(
    params: JoinParams,
    event_tx: mpsc::Sender<SessionEvent>,
    command_rx: mpsc::Receiver<SessionCommand>,
    frame_broadcast_rx: mpsc::Receiver<FrameBroadcast>,
    inbound_frame_tx: mpsc::Sender<PlaybackFrame>,
    cursor_rx: mpsc::Receiver<Option<[f32; 2]>>,
    annotation_rx: mpsc::Receiver<LocalAnnotationEvent>,
    display_toggle_rx: mpsc::Receiver<(DisplayOptionField, bool)>,
    ui_state: Arc<Mutex<SessionState>>,
) {
    // Decode token.
    let addr_json = match data_encoding::BASE64URL_NOPAD.decode(params.token.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("Invalid session token (not UTF-8): {e}");
                let _ = event_tx.send(SessionEvent::Error(msg.clone()));
                set_status(&ui_state, SessionStatus::Error(msg));
                return;
            }
        },
        Err(e) => {
            let msg = format!("Invalid session token (not base64): {e}");
            let _ = event_tx.send(SessionEvent::Error(msg.clone()));
            set_status(&ui_state, SessionStatus::Error(msg));
            return;
        }
    };

    let addr: iroh::EndpointAddr = match serde_json::from_str(&addr_json) {
        Ok(a) => a,
        Err(e) => {
            let msg = format!("Invalid session token (bad address): {e}");
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
    let join_msg =
        PeerMessage::Join { toolkit_version: params.toolkit_version.clone(), name: params.display_name.clone() };
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

    let (my_user_id, my_name, my_color, host_user_id, host_name, host_color, frame_source_id) = match &first_msg {
        PeerMessage::Rejected { reason } => {
            let _ = event_tx.send(SessionEvent::Rejected(reason.clone()));
            set_status(&ui_state, SessionStatus::Error(format!("Rejected: {reason}")));
            return;
        }
        PeerMessage::SessionInfo { peers, assigned_identity, frame_source_id, open_replays, .. } => {
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
                })
                .collect();
            let _ = event_tx.send(SessionEvent::SessionInfoReceived { open_replays: open_replay_list });
            // The host is peer[0] (if present).
            let host_peer = peers.first();
            let host_uid = host_peer.map(|p| p.user_id).unwrap_or(0);
            let host_name = host_peer.map(|p| p.name.clone()).unwrap_or_else(|| "Host".into());
            let host_color = host_peer.map(|p| p.color).unwrap_or(CURSOR_COLORS[0]);
            (
                assigned_identity.user_id,
                assigned_identity.name.clone(),
                assigned_identity.color,
                host_uid,
                host_name,
                host_color,
                *frame_source_id,
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
        let mut m = mesh.lock().unwrap();
        m.peers.insert(
            host_user_id,
            MeshPeer {
                user_id: host_user_id,
                name: host_name.clone(),
                color: host_color,
                role: PeerRole::Host,
                msg_tx: host_msg_tx,
            },
        );
    }

    // Update UI state.
    {
        let mut s = ui_state.lock().unwrap();
        s.my_user_id = my_user_id;
        s.host_user_id = host_user_id;
        s.frame_source_id = frame_source_id;
        s.role = PeerRole::Peer;
        s.status = SessionStatus::Active;
        // Add host to connected users so they appear in the user list.
        s.connected_users.push(ConnectedUser {
            id: host_user_id,
            name: host_name,
            color: host_color,
            role: PeerRole::Host,
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
            if let Some(framed) = compress_frame(&frame) {
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

    // Main loop.
    let session_info_received = true;
    loop {
        tokio::select! {
            // Receive messages from host (via dedicated reader task).
            msg_result = client_read_rx.recv() => {
                match msg_result {
                    Some(Ok(msg)) => {
                        if matches!(&msg, PeerMessage::SessionInfo { .. }) && session_info_received {
                            warn!("Ignoring duplicate SessionInfo from host");
                            continue;
                        }
                        if let Err(e) = validate_peer_message(&msg) {
                            warn!("Invalid message from host: {e}");
                            continue;
                        }
                        handle_incoming_message(
                            host_user_id,
                            msg,
                            &mesh,
                            &ui_state,
                            &event_tx,
                            &inbound_frame_tx,
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
                            mesh.lock().unwrap().permissions = p.clone();
                            if let Ok(mut s) = ui_state.lock() {
                                s.permissions = p;
                            }
                        }
                        SessionCommand::ResetClientOverrides => {
                            // Co-host could send render options — but we don't
                            // track initial options on the join side.
                        }
                        SessionCommand::SyncAnnotations { annotations, owners } => {
                            let msg = PeerMessage::AnnotationSync { annotations, owners };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                        SessionCommand::PromoteToCoHost { .. } => {
                            // Only the host can promote — ignore.
                        }
                        SessionCommand::BecomeFrameSource => {
                            let uid = mesh.lock().unwrap().my_user_id;
                            let msg = PeerMessage::FrameSourceChanged { source_user_id: uid };
                            let _ = write_peer_message(&mut send, &msg).await;
                        }
                        SessionCommand::ReplayOpened { .. } | SessionCommand::ReplayClosed { .. } => {
                            // Only the host sends replay lifecycle messages — ignore on join side.
                        }
                    }
                }

                // Cursor.
                while let Ok(pos) = cursor_rx.try_recv() {
                    if let Ok(mut s) = ui_state.lock()
                        && let Some(c) = s.cursors.iter_mut().find(|c| c.user_id == my_user_id) {
                            c.pos = pos;
                            c.last_update = Instant::now();
                        }
                    let msg = PeerMessage::CursorPosition(pos);
                    let _ = write_peer_message(&mut send, &msg).await;
                }

                // Annotations.
                while let Ok(evt) = annotation_rx.try_recv() {
                    match &evt {
                        LocalAnnotationEvent::Add(ann) => {
                            if let Ok(mut s) = ui_state.lock() {
                                let (anns, owners) = s.current_annotation_sync.get_or_insert_with(|| (Vec::new(), Vec::new()));
                                anns.push(ann.clone());
                                owners.push(my_user_id);
                                s.annotation_sync_version += 1;
                            }
                        }
                        LocalAnnotationEvent::Undo => {
                            if let Ok(mut s) = ui_state.lock()
                                && let Some((anns, owners)) = s.current_annotation_sync.as_mut()
                                    && let Some(pos) = owners.iter().rposition(|&uid| uid == my_user_id) {
                                        anns.remove(pos);
                                        owners.remove(pos);
                                        s.annotation_sync_version += 1;
                                    }
                        }
                    }
                    let msg = match evt {
                        LocalAnnotationEvent::Add(ann) => PeerMessage::AddAnnotation(ann),
                        LocalAnnotationEvent::Undo => PeerMessage::UndoAnnotation,
                    };
                    let _ = write_peer_message(&mut send, &msg).await;
                }

                // Display toggles.
                while let Ok((field, value)) = display_toggle_rx.try_recv() {
                    let msg = PeerMessage::ToggleDisplayOption { field, value };
                    let _ = write_peer_message(&mut send, &msg).await;
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
    if let Ok(mut s) = ui_state.lock() {
        s.status = SessionStatus::Idle;
        s.connected_users.clear();
        s.cursors.clear();
    }
    info!("Left collab session");
}

// ─── Shared message handling (client-side permission enforcement) ────────────

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
    inbound_frame_tx: &mpsc::Sender<PlaybackFrame>,
) {
    let m = mesh.lock().unwrap();
    let sender_is_authority = m.is_authority(sender_id);
    let sender_is_host = sender_id == m.host_user_id;
    let permissions = m.permissions.clone();
    let frame_source_id = m.frame_source_id;
    drop(m);

    match msg {
        // ── Always accept ───────────────────────────────────────────────
        PeerMessage::CursorPosition(pos) => {
            if let Ok(mut s) = ui_state.lock() {
                if let Some(c) = s.cursors.iter_mut().find(|c| c.user_id == sender_id) {
                    c.pos = pos;
                    c.last_update = Instant::now();
                } else {
                    // Unknown peer cursor — might have been announced via UserJoined.
                    let m = mesh.lock().unwrap();
                    let (name, color) = if let Some(p) = m.peers.get(&sender_id) {
                        (p.name.clone(), p.color)
                    } else {
                        (format!("Peer {sender_id}"), [200, 200, 200])
                    };
                    drop(m);
                    s.cursors.push(UserCursor { user_id: sender_id, name, color, pos, last_update: Instant::now() });
                }
            }
            // Host relays cursor updates to other peers.
            relay_if_host(sender_id, &PeerMessage::CursorPosition(pos), mesh);
        }

        PeerMessage::MeshHello { user_id, name, color } => {
            debug!("MeshHello from {user_id} ({name})");
            // Register peer if not already known.
            let _already_known = {
                let m = mesh.lock().unwrap();
                m.peers.contains_key(&user_id)
            };
            // MeshHello is mostly informational in the host-mediated model.
            let _ = (user_id, name, color);
        }

        // ── Annotation gated ────────────────────────────────────────────
        PeerMessage::AddAnnotation(ref ann) => {
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping AddAnnotation from {sender_id} (locked)");
                return;
            }
            if let Err(e) = validate_annotation(ann) {
                warn!("Invalid annotation from {sender_id}: {e}");
                return;
            }
            // Store in session state so the renderer picks it up.
            if let Ok(mut s) = ui_state.lock() {
                let (anns, owners) = s.current_annotation_sync.get_or_insert_with(|| (Vec::new(), Vec::new()));
                anns.push(ann.clone());
                owners.push(sender_id);
                s.annotation_sync_version += 1;
            }
            // Relay to other peers (host-mediated).
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::UndoAnnotation => {
            if permissions.annotations_locked && !sender_is_authority {
                debug!("Dropping UndoAnnotation from {sender_id} (locked)");
                return;
            }
            // Remove sender's most recent annotation from session state.
            if let Ok(mut s) = ui_state.lock()
                && let Some((anns, owners)) = s.current_annotation_sync.as_mut()
                && let Some(pos) = owners.iter().rposition(|&uid| uid == sender_id)
            {
                anns.remove(pos);
                owners.remove(pos);
                s.annotation_sync_version += 1;
            }
            relay_if_host(sender_id, &PeerMessage::UndoAnnotation, mesh);
        }

        // ── Settings gated ──────────────────────────────────────────────
        PeerMessage::ToggleDisplayOption { field, value } => {
            if permissions.settings_locked && !sender_is_authority {
                debug!("Dropping ToggleDisplayOption from {sender_id} (locked)");
                return;
            }
            relay_if_host(sender_id, &PeerMessage::ToggleDisplayOption { field, value }, mesh);
        }

        // ── Authority-only ──────────────────────────────────────────────
        PeerMessage::Permissions { annotations_locked, settings_locked } => {
            if !sender_is_authority {
                debug!("Dropping Permissions from non-authority {sender_id}");
                return;
            }
            mesh.lock().unwrap().permissions = Permissions { annotations_locked, settings_locked };
            if let Ok(mut s) = ui_state.lock() {
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
            if let Ok(mut s) = ui_state.lock() {
                s.render_options_version += 1;
                s.current_render_options = Some(opts.clone());
            }
            relay_if_host(sender_id, &msg, mesh);
        }

        PeerMessage::AnnotationSync { ref annotations, ref owners } => {
            if !sender_is_authority {
                debug!("Dropping AnnotationSync from non-authority {sender_id}");
                return;
            }
            if let Ok(mut s) = ui_state.lock() {
                s.annotation_sync_version += 1;
                s.current_annotation_sync = Some((annotations.clone(), owners.clone()));
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
            let user = ConnectedUser { id: user_id, name: name.clone(), color, role: PeerRole::Peer };
            if let Ok(mut s) = ui_state.lock() {
                s.connected_users.push(user.clone());
            }
            let _ = event_tx.send(SessionEvent::UserJoined(user));
        }

        PeerMessage::UserLeft { user_id } => {
            if !sender_is_host {
                debug!("Dropping UserLeft from non-host {sender_id}");
                return;
            }
            if let Ok(mut s) = ui_state.lock() {
                s.connected_users.retain(|u| u.id != user_id);
                s.cursors.retain(|c| c.user_id != user_id);
            }
            mesh.lock().unwrap().peers.remove(&user_id);
            let _ = event_tx.send(SessionEvent::UserLeft { user_id });
        }

        // ── Co-host promotion (host only) ───────────────────────────────
        PeerMessage::PromoteToCoHost { user_id } => {
            if !sender_is_host {
                debug!("Dropping PromoteToCoHost from non-host {sender_id}");
                return;
            }
            let mut m = mesh.lock().unwrap();
            let is_me = user_id == m.my_user_id;
            if let Some(peer) = m.peers.get_mut(&user_id) {
                peer.role = PeerRole::CoHost;
            }
            if is_me {
                m.my_role = PeerRole::CoHost;
            }
            drop(m);
            if let Ok(mut s) = ui_state.lock() {
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
            mesh.lock().unwrap().frame_source_id = source_user_id;
            if let Ok(mut s) = ui_state.lock() {
                s.frame_source_id = source_user_id;
            }
            let _ = event_tx.send(SessionEvent::FrameSourceChanged { source_user_id });
            relay_if_host(sender_id, &PeerMessage::FrameSourceChanged { source_user_id }, mesh);
        }

        PeerMessage::Frame { replay_id, clock, frame_index, total_frames, game_duration, compressed_commands } => {
            if sender_id != frame_source_id {
                debug!("Dropping Frame from {sender_id} (frame source is {frame_source_id})");
                return;
            }
            // Decompress and forward to UI.
            let mut decoder = ZlibDecoder::new(&compressed_commands[..]);
            let mut decompressed = Vec::new();
            if let Err(e) = decoder.read_to_end(&mut decompressed) {
                warn!("Frame decompression failed: {e}");
                return;
            }
            if decompressed.len() > MAX_DECOMPRESSED_FRAME_SIZE {
                warn!("Decompressed frame too large: {}", decompressed.len());
                return;
            }
            let commands: Vec<DrawCommand> =
                match rkyv::from_bytes::<Vec<DrawCommand>, rkyv::rancor::Error>(&decompressed) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Frame deserialization failed: {e}");
                        return;
                    }
                };
            if let Err(e) = validate_frame_commands_count(commands.len()) {
                warn!("{e}");
                return;
            }
            let frame = PlaybackFrame {
                replay_id,
                commands,
                clock: wowsunpack::game_types::GameClock(clock),
                frame_index: frame_index as usize,
                total_frames: total_frames as usize,
                game_duration,
            };
            trace!("Received frame: replay_id={replay_id} frame={frame_index}/{total_frames} clock={clock:.1}");
            let _ = inbound_frame_tx.send(frame);
            // Host relays frames to other peers.
            relay_if_host(
                sender_id,
                &PeerMessage::Frame { replay_id, clock, frame_index, total_frames, game_duration, compressed_commands },
                mesh,
            );
        }

        // ── Replay lifecycle (host → all peers) ────────────────────────
        PeerMessage::ReplayOpened { replay_id, replay_name, map_image_png, game_version } => {
            if !sender_is_host {
                debug!("Dropping ReplayOpened from non-host {sender_id}");
                return;
            }
            if let Ok(mut s) = ui_state.lock() {
                s.open_replays.push(OpenReplay {
                    replay_id,
                    replay_name: replay_name.clone(),
                    map_image_png: map_image_png.clone(),
                    game_version: game_version.clone(),
                });
            }
            let _ = event_tx.send(SessionEvent::ReplayOpened { replay_id, replay_name, map_image_png, game_version });
        }

        PeerMessage::ReplayClosed { replay_id } => {
            if !sender_is_host {
                debug!("Dropping ReplayClosed from non-host {sender_id}");
                return;
            }
            if let Ok(mut s) = ui_state.lock() {
                s.open_replays.retain(|r| r.replay_id != replay_id);
                s.current_annotation_sync = None;
                s.annotation_sync_version += 1;
            }
            let _ = event_tx.send(SessionEvent::ReplayClosed { replay_id });
        }

        // ── Handshake messages (not expected post-handshake) ────────────
        PeerMessage::Join { .. }
        | PeerMessage::SessionInfo { .. }
        | PeerMessage::Rejected { .. }
        | PeerMessage::PeerAnnounce { .. } => {
            debug!("Ignoring handshake message from {sender_id} post-handshake");
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Broadcast a peer message to all connected peers in the mesh.
fn broadcast_to_mesh(mesh: &Arc<Mutex<MeshState>>, msg: &PeerMessage) {
    let framed = match frame_peer_message(msg) {
        Ok(f) => Arc::new(f),
        Err(e) => {
            warn!("Failed to serialize broadcast: {e}");
            return;
        }
    };
    let m = mesh.lock().unwrap();
    for peer in m.peers.values() {
        let _ = peer.msg_tx.try_send(Arc::clone(&framed));
    }
}

/// If we are the host, relay a message to all peers except the sender.
/// In the current host-mediated model, the host must relay messages between
/// peers since they don't have direct connections.
fn relay_if_host(sender_id: u64, msg: &PeerMessage, mesh: &Arc<Mutex<MeshState>>) {
    let m = mesh.lock().unwrap();
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

/// Compress a frame broadcast into a wire-ready PeerMessage::Frame.
fn compress_frame(frame: &FrameBroadcast) -> Option<Vec<u8>> {
    let rkyv_bytes = match rkyv::to_bytes::<rkyv::rancor::Error>(&frame.commands) {
        Ok(b) => b,
        Err(e) => {
            warn!("Failed to serialize frame commands: {e}");
            return None;
        }
    };
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    if encoder.write_all(&rkyv_bytes).is_err() {
        return None;
    }
    let compressed = encoder.finish().ok()?;

    let msg = PeerMessage::Frame {
        replay_id: frame.replay_id,
        clock: frame.clock,
        frame_index: frame.frame_index,
        total_frames: frame.total_frames,
        game_duration: frame.game_duration,
        compressed_commands: compressed,
    };
    frame_peer_message(&msg).ok()
}

fn set_status(state: &Arc<Mutex<SessionState>>, status: SessionStatus) {
    if let Ok(mut s) = state.lock() {
        s.status = status;
    }
}

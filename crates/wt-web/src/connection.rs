//! Iroh WASM connection to the desktop host.
//!
//! Handles connecting via the iroh relay, handshaking (Join → SessionInfo),
//! receiving messages, and sending local events.
//!
//! Reliability features:
//! - Waits for relay readiness (`endpoint.online()`) before connecting
//! - Retries initial connection with exponential backoff
//! - Auto-reconnects on disconnect (up to `MAX_RECONNECT_ATTEMPTS`)

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wt_collab_protocol::protocol::*;

/// Maximum number of attempts for the initial connect + handshake.
const MAX_CONNECT_RETRIES: u32 = 3;

/// Maximum number of auto-reconnect attempts after a connection drops.
const MAX_RECONNECT_ATTEMPTS: u32 = 5;

/// Messages from the connection task to the UI.
pub enum ConnectionEvent {
    /// Successfully connected and received SessionInfo.
    Connected { my_user_id: u64, my_name: String, my_color: [u8; 3], host_user_id: u64, frame_source_id: u64 },
    /// Received a PeerMessage from the host.
    Message(PeerMessage),
    /// Connection was rejected by the host.
    Rejected(String),
    /// Connection error (terminal — will not auto-reconnect).
    Error(String),
    /// Connection closed.
    Disconnected,
    /// Auto-reconnecting after a drop.
    Reconnecting { attempt: u32, max_attempts: u32 },
}

/// Commands from the UI to the connection task.
pub enum ConnectionCommand {
    /// Send a PeerMessage to the host.
    Send(Box<PeerMessage>),
    /// Disconnect.
    Disconnect,
}

/// Shared message queue (single-threaded on WASM, so Rc<RefCell> is fine).
pub type EventQueue = Rc<RefCell<VecDeque<ConnectionEvent>>>;
pub type CommandQueue = Rc<RefCell<VecDeque<ConnectionCommand>>>;

/// Why the message loop exited.
enum DisconnectReason {
    /// User explicitly requested disconnect.
    UserRequested,
    /// Connection lost (network error, heartbeat timeout, etc.).
    ConnectionLost(String),
}

/// Start the connection task. Returns event/command queues for communicating
/// with the UI thread.
///
/// On WASM, this spawns an async task via `wasm_bindgen_futures::spawn_local`.
/// The `egui_ctx` is used to wake the UI thread (via `request_repaint()`) when
/// a message arrives, ensuring the web client repaints even when the tab is
/// not focused.
#[cfg(target_arch = "wasm32")]
pub fn start_connection(token: String, display_name: String, egui_ctx: egui::Context) -> (EventQueue, CommandQueue) {
    let events: EventQueue = Rc::new(RefCell::new(VecDeque::new()));
    let commands: CommandQueue = Rc::new(RefCell::new(VecDeque::new()));

    let events_clone = Rc::clone(&events);
    let commands_clone = Rc::clone(&commands);

    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = run_connection(token, display_name, events_clone, commands_clone, egui_ctx).await {
            tracing::error!("Connection task failed: {e}");
        }
    });

    (events, commands)
}

#[cfg(target_arch = "wasm32")]
async fn run_connection(
    token: String,
    display_name: String,
    events: EventQueue,
    commands: CommandQueue,
    egui_ctx: egui::Context,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Decode token to get host's public key.
    let host_public_key = decode_token(&token)?;
    tracing::info!("Connecting to host: {}", host_public_key.fmt_short());

    // Create iroh endpoint.
    let secret_key = iroh::SecretKey::generate(&mut rand::rng());
    let endpoint = iroh::Endpoint::builder()
        .secret_key(secret_key)
        .alpns(vec![COLLAB_ALPN.to_vec()])
        .bind()
        .await
        .map_err(|e| format!("Failed to bind endpoint: {e}"))?;

    // Wait for the relay WebSocket to be established before attempting to
    // connect. Without this, the connect() call can fail on slow networks
    // because the relay isn't ready yet.
    tracing::info!("Waiting for relay connection...");
    endpoint.online().await;
    tracing::info!("Relay connected");

    // Build the address with all default relay URLs as hints.
    let addr = {
        let mut a = iroh::EndpointAddr::new(host_public_key);
        for url in iroh::defaults::prod::default_relay_map().urls::<Vec<_>>() {
            a = a.with_relay_url(url);
        }
        a
    };

    let mut reconnect_count: u32;

    loop {
        // Check for user disconnect before (re)connecting.
        if has_disconnect_command(&commands) {
            break;
        }

        // Connect + handshake with retries.
        let (send, recv) = match connect_with_retries(&endpoint, &addr, &display_name, &events, &egui_ctx).await {
            Ok(pair) => pair,
            Err(e) => {
                events.borrow_mut().push_back(ConnectionEvent::Error(e));
                egui_ctx.request_repaint();
                break;
            }
        };

        // Successfully connected — reset reconnect counter.
        reconnect_count = 0;

        // Run the message loop until disconnect.
        let reason = run_message_loop(send, recv, &events, &commands, &egui_ctx).await;

        match reason {
            DisconnectReason::UserRequested => break,
            DisconnectReason::ConnectionLost(msg) => {
                reconnect_count += 1;
                if reconnect_count > MAX_RECONNECT_ATTEMPTS {
                    events.borrow_mut().push_back(ConnectionEvent::Error(format!(
                        "Connection lost after {MAX_RECONNECT_ATTEMPTS} reconnection attempts: {msg}"
                    )));
                    egui_ctx.request_repaint();
                    break;
                }

                tracing::info!("Connection lost ({msg}), reconnecting ({reconnect_count}/{MAX_RECONNECT_ATTEMPTS})...");
                events.borrow_mut().push_back(ConnectionEvent::Reconnecting {
                    attempt: reconnect_count,
                    max_attempts: MAX_RECONNECT_ATTEMPTS,
                });
                egui_ctx.request_repaint();

                // Exponential backoff: 1s, 2s, 4s, 8s, 16s.
                let delay = 1000 * 2u32.pow(reconnect_count.min(5) - 1);
                wasm_sleep_ms(delay).await;
            }
        }
    }

    Ok(())
}

/// Try to connect and complete the handshake, retrying up to `MAX_CONNECT_RETRIES`
/// times with exponential backoff.
#[cfg(target_arch = "wasm32")]
async fn connect_with_retries(
    endpoint: &iroh::Endpoint,
    addr: &iroh::EndpointAddr,
    display_name: &str,
    events: &EventQueue,
    ctx: &egui::Context,
) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream), String> {
    let mut last_err = String::new();

    for attempt in 1..=MAX_CONNECT_RETRIES {
        if attempt > 1 {
            let delay = 1000 * 2u32.pow(attempt - 2);
            tracing::info!("Retrying connection (attempt {attempt}/{MAX_CONNECT_RETRIES}) after {delay}ms...");
            wasm_sleep_ms(delay).await;
        }

        match try_connect_and_handshake(endpoint, addr, display_name, events, ctx).await {
            Ok(pair) => return Ok(pair),
            Err(ConnectError::Rejected(reason)) => {
                // Rejection is not retryable.
                events.borrow_mut().push_back(ConnectionEvent::Rejected(reason));
                ctx.request_repaint();
                return Err("Connection rejected by host".to_string());
            }
            Err(ConnectError::Transient(e)) => {
                tracing::warn!("Connection attempt {attempt}/{MAX_CONNECT_RETRIES} failed: {e}");
                last_err = e;
            }
        }
    }

    Err(format!("Failed to connect after {MAX_CONNECT_RETRIES} attempts: {last_err}"))
}

enum ConnectError {
    /// Transient failure — can retry.
    Transient(String),
    /// Host explicitly rejected us — don't retry.
    Rejected(String),
}

/// Single attempt to connect to the host and complete the Join → SessionInfo handshake.
/// On success, pushes `ConnectionEvent::Connected` + `UserJoined` events and returns
/// the send/recv streams for the message loop.
#[cfg(target_arch = "wasm32")]
async fn try_connect_and_handshake(
    endpoint: &iroh::Endpoint,
    addr: &iroh::EndpointAddr,
    display_name: &str,
    events: &EventQueue,
    ctx: &egui::Context,
) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream), ConnectError> {
    let conn = endpoint
        .connect(addr.clone(), COLLAB_ALPN)
        .await
        .map_err(|e| ConnectError::Transient(format!("Failed to connect: {e}")))?;

    let (mut send, mut recv) =
        conn.open_bi().await.map_err(|e| ConnectError::Transient(format!("Failed to open bi stream: {e}")))?;

    // Send Join message.
    let join_msg = PeerMessage::Join { name: display_name.to_string(), client_type: ClientType::Web };
    write_peer_message(&mut send, &join_msg)
        .await
        .map_err(|e| ConnectError::Transient(format!("Failed to send Join: {e}")))?;

    // Read SessionInfo.
    let session_info = read_peer_message(&mut recv, MAX_MESSAGE_SIZE)
        .await
        .map_err(|e| ConnectError::Transient(format!("Failed to read SessionInfo: {e}")))?
        .ok_or_else(|| ConnectError::Transient("Host closed connection before sending SessionInfo".to_string()))?;

    match session_info {
        PeerMessage::SessionInfo { peers, assigned_identity, frame_source_id, .. } => {
            let host_user_id = peers.first().map(|p| p.user_id).unwrap_or(0);
            events.borrow_mut().push_back(ConnectionEvent::Connected {
                my_user_id: assigned_identity.user_id,
                my_name: assigned_identity.name,
                my_color: assigned_identity.color,
                host_user_id,
                frame_source_id,
            });

            // Add host and peers as connected users.
            for peer in &peers {
                events.borrow_mut().push_back(ConnectionEvent::Message(PeerMessage::UserJoined {
                    user_id: peer.user_id,
                    name: peer.name.clone(),
                    color: peer.color,
                }));
            }

            ctx.request_repaint();
            Ok((send, recv))
        }
        PeerMessage::Rejected { reason } => Err(ConnectError::Rejected(reason)),
        other => Err(ConnectError::Transient(format!("Expected SessionInfo, got: {other:?}"))),
    }
}

/// Run the message read/write loop until disconnect.
///
/// Returns the reason the loop exited.
#[cfg(target_arch = "wasm32")]
async fn run_message_loop(
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
    events: &EventQueue,
    commands: &CommandQueue,
    egui_ctx: &egui::Context,
) -> DisconnectReason {
    let done = Rc::new(Cell::new(false));
    let disconnect_reason: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let last_received = Rc::new(Cell::new(web_time::Instant::now()));

    // Spawn read task.
    {
        let done = Rc::clone(&done);
        let events = Rc::clone(events);
        let last_received = Rc::clone(&last_received);
        let disconnect_reason = Rc::clone(&disconnect_reason);
        let ctx = egui_ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                if done.get() {
                    return;
                }
                match read_peer_message(&mut recv, MAX_MESSAGE_SIZE).await {
                    Ok(Some(msg)) => {
                        last_received.set(web_time::Instant::now());
                        events.borrow_mut().push_back(ConnectionEvent::Message(msg));
                        ctx.request_repaint();
                    }
                    Ok(None) => {
                        if !done.get() {
                            tracing::warn!("Host closed connection");
                            *disconnect_reason.borrow_mut() = Some("Host closed connection".to_string());
                        }
                        done.set(true);
                        ctx.request_repaint();
                        return;
                    }
                    Err(e) => {
                        if !done.get() {
                            tracing::warn!("Connection lost: {e}");
                            *disconnect_reason.borrow_mut() = Some(format!("Connection lost: {e}"));
                        }
                        done.set(true);
                        ctx.request_repaint();
                        return;
                    }
                }
            }
        });
    }

    // Write loop: processes UI commands, sends heartbeats, detects host timeout.
    let mut last_heartbeat = web_time::Instant::now();
    let mut user_requested = false;

    loop {
        if done.get() {
            break;
        }

        // Yield to the event loop briefly so the read task can run.
        wasm_sleep_ms(10).await;

        if done.get() {
            break;
        }

        // Drain commands.
        let pending_cmds: Vec<_> = commands.borrow_mut().drain(..).collect();
        for cmd in pending_cmds {
            match cmd {
                ConnectionCommand::Send(msg) => {
                    if let Err(e) = write_peer_message(&mut send, &msg).await {
                        if !done.get() {
                            tracing::error!("Failed to send message: {e}");
                            *disconnect_reason.borrow_mut() = Some(format!("Send failed: {e}"));
                        }
                        done.set(true);
                        break;
                    }
                }
                ConnectionCommand::Disconnect => {
                    user_requested = true;
                    done.set(true);
                    break;
                }
            }
        }

        if done.get() {
            break;
        }

        // Send heartbeat if interval has elapsed.
        if last_heartbeat.elapsed().as_secs() >= HEARTBEAT_INTERVAL_SECS {
            if write_peer_message(&mut send, &PeerMessage::Heartbeat).await.is_err() {
                if !done.get() {
                    *disconnect_reason.borrow_mut() = Some("Heartbeat send failed".to_string());
                }
                done.set(true);
                break;
            }
            last_heartbeat = web_time::Instant::now();
        }

        // Check for host timeout.
        if last_received.get().elapsed().as_secs() >= HEARTBEAT_TIMEOUT_SECS {
            tracing::warn!("Host heartbeat timeout");
            *disconnect_reason.borrow_mut() = Some(format!("No response from host for {HEARTBEAT_TIMEOUT_SECS}s"));
            done.set(true);
            break;
        }
    }

    if user_requested {
        events.borrow_mut().push_back(ConnectionEvent::Disconnected);
        egui_ctx.request_repaint();
        DisconnectReason::UserRequested
    } else {
        let reason = disconnect_reason.borrow().clone().unwrap_or_else(|| "Unknown".to_string());
        DisconnectReason::ConnectionLost(reason)
    }
}

/// Check if the command queue contains a Disconnect command.
#[cfg(target_arch = "wasm32")]
fn has_disconnect_command(commands: &CommandQueue) -> bool {
    let mut cmds = commands.borrow_mut();
    if cmds.iter().any(|c| matches!(c, ConnectionCommand::Disconnect)) {
        cmds.clear();
        true
    } else {
        false
    }
}

/// Sleep for the given number of milliseconds (WASM-compatible).
///
/// Uses `setTimeout` via JS interop to yield to the browser event loop.
#[cfg(target_arch = "wasm32")]
async fn wasm_sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let _ = web_sys::window()
            .expect("no global window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Placeholder for non-WASM targets (compile check only).
#[cfg(not(target_arch = "wasm32"))]
pub fn start_connection(_token: String, _display_name: String, _egui_ctx: egui::Context) -> (EventQueue, CommandQueue) {
    let events: EventQueue = Rc::new(RefCell::new(VecDeque::new()));
    let commands: CommandQueue = Rc::new(RefCell::new(VecDeque::new()));
    (events, commands)
}

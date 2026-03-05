//! Iroh WASM connection to the desktop host.
//!
//! Handles connecting via the iroh relay, handshaking (Join → SessionInfo),
//! receiving messages, and sending local events.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wt_collab_protocol::protocol::*;

/// Messages from the connection task to the UI.
pub enum ConnectionEvent {
    /// Successfully connected and received SessionInfo.
    Connected { my_user_id: u64, my_name: String, my_color: [u8; 3], host_user_id: u64, frame_source_id: u64 },
    /// Received a PeerMessage from the host.
    Message(PeerMessage),
    /// Connection was rejected by the host.
    Rejected(String),
    /// Connection error.
    Error(String),
    /// Connection closed.
    Disconnected,
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
    // Decode token to get host's public key
    let host_public_key = decode_token(&token)?;
    tracing::info!("Connecting to host: {}", host_public_key.fmt_short());

    events.borrow_mut().push_back(ConnectionEvent::Error("Connecting...".to_string()));

    // Create iroh endpoint
    let secret_key = iroh::SecretKey::generate(&mut rand::rng());
    let endpoint = iroh::Endpoint::builder()
        .secret_key(secret_key)
        .alpns(vec![COLLAB_ALPN.to_vec()])
        .bind()
        .await
        .map_err(|e| format!("Failed to bind endpoint: {e}"))?;

    // Add all default relay URLs as hints so iroh knows which relay servers
    // to try. On WASM the endpoint's own relay discovery may not have
    // completed yet, so we use the hardcoded production defaults (the host
    // uses the same defaults).
    let addr = {
        let mut a = iroh::EndpointAddr::new(host_public_key);
        for url in iroh::defaults::prod::default_relay_map().urls::<Vec<_>>() {
            a = a.with_relay_url(url);
        }
        a
    };
    let conn = endpoint.connect(addr, COLLAB_ALPN).await.map_err(|e| format!("Failed to connect to host: {e}"))?;

    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| format!("Failed to open bi stream: {e}"))?;

    // Send Join message
    let join_msg = PeerMessage::Join { name: display_name.clone(), client_type: ClientType::Web };
    write_peer_message(&mut send, &join_msg).await.map_err(|e| format!("Failed to send Join: {e}"))?;

    // Read SessionInfo (with timeout handled by the host)
    let session_info = read_peer_message(&mut recv, MAX_MESSAGE_SIZE)
        .await?
        .ok_or("Host closed connection before sending SessionInfo")?;
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

            // Add host and peers as connected users
            for peer in &peers {
                events.borrow_mut().push_back(ConnectionEvent::Message(PeerMessage::UserJoined {
                    user_id: peer.user_id,
                    name: peer.name.clone(),
                    color: peer.color,
                }));
            }
        }
        PeerMessage::Rejected { reason } => {
            events.borrow_mut().push_back(ConnectionEvent::Rejected(reason));
            return Ok(());
        }
        other => {
            events.borrow_mut().push_back(ConnectionEvent::Error(format!("Expected SessionInfo, got: {other:?}")));
            return Ok(());
        }
    }

    // Shared flag so read/write tasks can signal each other to stop.
    let done = Rc::new(Cell::new(false));
    // Track when the last message was received (for host timeout detection).
    let last_received = Rc::new(Cell::new(web_time::Instant::now()));

    // Spawn read task (owns recv, reads messages continuously).
    {
        let done = Rc::clone(&done);
        let events = Rc::clone(&events);
        let last_received = Rc::clone(&last_received);
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
                        // Wake the UI thread so it polls this event even when
                        // the browser tab is not focused.
                        ctx.request_repaint();
                    }
                    Ok(None) => {
                        if !done.get() {
                            tracing::warn!("Host closed connection");
                            events.borrow_mut().push_back(ConnectionEvent::Disconnected);
                            ctx.request_repaint();
                        }
                        done.set(true);
                        return;
                    }
                    Err(e) => {
                        if !done.get() {
                            tracing::warn!("Connection lost: {e}");
                            events.borrow_mut().push_back(ConnectionEvent::Disconnected);
                            ctx.request_repaint();
                        }
                        done.set(true);
                        return;
                    }
                }
            }
        });
    }

    // Write loop (owns send): processes UI commands, sends heartbeats,
    // and detects host timeout.
    let mut last_heartbeat = web_time::Instant::now();

    loop {
        if done.get() {
            break;
        }

        // Yield to the event loop briefly so the read task can run.
        wasm_sleep_ms(10).await;

        if done.get() {
            break;
        }

        // Drain commands into a local vec so we don't hold the borrow across awaits.
        let pending_cmds: Vec<_> = commands.borrow_mut().drain(..).collect();
        for cmd in pending_cmds {
            match cmd {
                ConnectionCommand::Send(msg) => {
                    if let Err(e) = write_peer_message(&mut send, &msg).await {
                        if !done.get() {
                            tracing::error!("Failed to send message: {e}");
                            events.borrow_mut().push_back(ConnectionEvent::Disconnected);
                        }
                        done.set(true);
                        break;
                    }
                }
                ConnectionCommand::Disconnect => {
                    events.borrow_mut().push_back(ConnectionEvent::Disconnected);
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
                    events.borrow_mut().push_back(ConnectionEvent::Disconnected);
                }
                done.set(true);
                break;
            }
            last_heartbeat = web_time::Instant::now();
        }

        // Check for host timeout.
        if last_received.get().elapsed().as_secs() >= HEARTBEAT_TIMEOUT_SECS {
            tracing::warn!("Host heartbeat timeout");
            events.borrow_mut().push_back(ConnectionEvent::Error(format!(
                "Connection to host lost (no response for {HEARTBEAT_TIMEOUT_SECS}s)"
            )));
            done.set(true);
            break;
        }
    }

    Ok(())
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

# Collaborative Sessions

Real-time collaborative viewing of replay minimaps and tactics boards over
peer-to-peer networking. A host shares their replay renderer and/or tactics
boards and other users watch and annotate together. Both desktop and web
(WASM) clients are supported.

## Network topology

Sessions use **iroh** (QUIC + NAT traversal) in a mesh topology. The host is
the initial rendezvous point and identity authority; after the handshake every
participant maintains a direct connection to every other participant.

| Layer | Detail |
|-------|--------|
| ALPN | `b"/wows-toolkit-collab/1"` |
| Serialization | rkyv, zlib-compressed, length-prefixed (`[u32 compressed_length][zlib(rkyv payload)]`) |
| Limits | 16 MB max message, 4 MB compressed frame, 32 MB decompressed |

## Module layout

Protocol types live in shared crates; desktop-specific code stays in the main
crate. The desktop `collab/` modules re-export from the shared crates for
backward compatibility.

```
crates/wt-collab-protocol/src/
  protocol.rs     PeerMessage enum, wire helpers, CollabRenderOptions, token encoding
  types.rs        Annotation, PaintTool, color_from_name()
  validation.rs   Structural field validation for all PeerMessage variants

crates/wt-collab-egui/src/
  types.rs        UserCursor, Permissions, ViewportZoomPan, MapTransform
  toolbar.rs      Shared annotation toolbar UI
  transforms.rs   Canvas layout, clip rect computation
  interaction.rs  Zoom/pan input handling
  rendering.rs    Map background drawing
  draw_commands.rs DrawCommand → epaint shape conversion

crates/wows-toolkit/src/collab/
  mod.rs          SessionState, PeerRole, events, commands (desktop)
  protocol.rs     Re-exports from wt-collab-protocol + desktop conversion helpers
  peer.rs         Background peer task, PeerSessionHandle, start_peer_session
  types.rs        Re-exports from wt-collab-protocol::types
  validation.rs   Re-exports from wt-collab-protocol::validation
```

## Threading model

```
┌──────────────┐   Arc<Mutex<SessionState>>   ┌───────────────┐
│  UI thread   │◄────────────────────────────►│  peer_task     │
│  (egui)      │  mpsc channels (events,      │  (tokio spawn) │
│              │   commands, frames, cursors)  │                │
└──────────────┘                              └───────────────┘
```

Both threads share a single `Arc<Mutex<SessionState>>`. The UI thread reads
it each frame; the background task writes it. One-shot data (render option
overrides, annotation syncs) uses `Option` fields that the UI `.take()`s.

## Roles and permissions

```rust
enum PeerRole { Host, CoHost, Peer }

struct Permissions {
    annotations_locked: bool,   // peers cannot add/remove annotations
    settings_locked: bool,      // peers cannot toggle display options
}
```

Permission enforcement is **client-side**: each receiver checks the sender's
role and drops unauthorized messages. The host can promote a peer to co-host
via `PromoteToCoHost`. Only the original host user ID (immutable after
`SessionInfo`) is accepted as the source of promotions.

## Host flow

1. User clicks **Start Session** in the **Session** popover (replay inspector
   header bar, same row as Group By / Column Filters).
2. The popover accesses the first open renderer's `RendererSessionAccess`,
   encodes the map image to PNG, gathers the game version and display name,
   and calls:

   ```rust
   start_peer_session(runtime, PeerMode::Host(params), session_state_arc)
   ```

3. `start_peer_session` resets the provided `SessionState`, spawns the
   background `peer_task`, and returns a `PeerSessionHandle`.
4. The handle's `frame_tx` is stored in `SharedRendererState.session_frame_tx`
   so the playback thread can broadcast frames to peers.
5. The UI reads `session_state.token` to display a copyable session token.
   A web client URL is also generated (`WEB_CLIENT_URL#{token}`).
6. When the session ends (user clicks **Stop**, or the background task
   detects a disconnect), the handle is cleared and `SessionState` is reset.

## Client flow

### Desktop client

1. User clicks **Join** in the **Session** popover (same popover as host controls).
2. The inline form collects the session token and display name. After an
   optional IP disclosure warning, `do_join_session()` calls:

   ```rust
   start_peer_session(runtime, PeerMode::Join(params), shared_session_state())
   ```

3. The background task connects to the host, receives `SessionInfo` (peer
   list, open replays, assigned identity, frame source), and emits
   `SessionEvent::SessionInfoReceived`.
4. On receiving that event, the app creates a client viewer for each open
   replay.
5. Frames arrive via `handle.frame_rx` and are rendered by the client viewer.

### Web client

1. User opens the web client URL with the session token as a hash fragment.
2. The WASM client sends `Join` with `ClientType::Web`.
3. After `SessionInfo`, the host sends an `AssetBundle` containing ship/plane/
   consumable/death cause icons and game fonts needed for rendering.
4. The web client renders frames using the same draw command pipeline.

## Message protocol

### Handshake (joiner → host)

| Message | Direction | Purpose |
|---------|-----------|---------|
| `Join` | joiner → host | Client type (Desktop/Web) + display name |
| `SessionInfo` | host → joiner | Peer list, assigned identity, open replays, frame source |
| `Rejected` | host → joiner | Connection denied with reason |
| `PeerAnnounce` | host → existing peers | Introduces the new joiner |
| `MeshHello` | joiner → existing peers | Establishes direct connections |

### Regular messages (any peer → all)

| Message | Gated by |
|---------|----------|
| `CursorPosition { user_id, pos }` | — |
| `SetAnnotation { board_id, id, annotation, owner }` | `annotations_locked` (unless authority) |
| `RemoveAnnotation { board_id, id }` | `annotations_locked` (unless authority) |
| `ClearAnnotations { board_id }` | `annotations_locked` (unless authority) |
| `ToggleDisplayOption { field, value }` | `settings_locked` (unless authority) |
| `ShipRangeOverrides { overrides }` | `settings_locked` (unless authority) |
| `ShipTrailOverrides { hidden }` | `settings_locked` (unless authority) |
| `Ping { user_id, pos, color }` | — |
| `Heartbeat` | — |

### Authority messages (host/co-host → all)

| Message | Purpose |
|---------|---------|
| `Permissions { .. }` | Update lock state |
| `RenderOptions(CollabRenderOptions)` | Full display options bundle (31 fields) |
| `AnnotationSync { board_id, annotations, owners, ids }` | Replace annotation state |
| `PlaybackState { playing, speed }` | Playback control |
| `UserJoined` / `UserLeft` | Roster (host only) |
| `PromoteToCoHost { user_id }` | Role change (host only) |
| `FrameSourceChanged { source_user_id }` | Declares new frame source |
| `OpenWindowForEveryone { window_id }` | Request all peers to open a window |

### Frame sourcing

```rust
Frame { replay_id, clock, frame_index, total_frames, game_duration, commands }
```

Receivers only accept `Frame` from the current `frame_source_id`. The host is
the initial frame source; the frame source is changed via `FrameSourceChanged`.
Draw commands are serialized as part of the `PeerMessage` and compressed at
the framing layer (all messages are zlib-compressed).

### Replay lifecycle (host → all)

| Message | Purpose |
|---------|---------|
| `ReplayOpened { replay_id, replay_name, map_image_png, ... }` | A new replay was opened |
| `ReplayClosed { replay_id }` | A replay was closed |

### Tactics boards (any peer → all)

| Message | Purpose |
|---------|---------|
| `TacticsMapOpened { board_id, map_name, map_image_png, map_info, ... }` | A tactics board was opened |
| `TacticsMapClosed { board_id }` | A tactics board was closed |
| `SetCapPoint { board_id, cap_point }` | Add/update a capture point |
| `RemoveCapPoint { board_id, id }` | Remove a capture point |
| `CapPointSync { board_id, cap_points }` | Full cap point state replacement |

### Asset delivery (host → web clients)

| Message | Purpose |
|---------|---------|
| `RequestAssets` | Client requests the asset bundle |
| `AssetBundle { ship_icons, plane_icons, consumable_icons, ... }` | Icons + fonts for rendering |

## Render options sync

`CollabRenderOptions` mirrors `RenderOptions` from the minimap renderer
(31 boolean display toggles, including 6 self-range toggles). When an
authority peer sends `PeerMessage::RenderOptions`, the receiver stores it in
`SessionState.pending_render_options`. The renderer viewport `.take()`s it
each frame and applies field-by-field to `SharedRendererState.options`.

## Annotation model

A single `Annotation` enum in `wt-collab-protocol::types` is used for both
wire transport and UI rendering. Fields use primitive arrays (`[f32; 2]`,
`[u8; 4]`) that are rkyv-serializable. When egui types are needed, conversion
helpers in `wt-collab-protocol::types` (behind the `egui` feature flag)
provide `arr_to_pos2()`, `arr_to_color32()`, etc.

Annotation variants: `Ship`, `FreehandStroke`, `Line`, `Circle`, `Rectangle`,
`Triangle`, `Arrow`, `Measurement`.

Annotations are identified by unique `u64` IDs and managed with `SetAnnotation`
(upsert), `RemoveAnnotation` (delete by ID), and `ClearAnnotations` (delete
all). The `board_id` field distinguishes between replay context (`None`) and
tactics board context (`Some(id)`). Annotation ownership is tracked in a
parallel `Vec<u64>` (`owners`) so each annotation can be attributed to its
creator.

## Cursor rendering

Each user's cursor position is tracked in `SessionState.cursors` as:

```rust
struct UserCursor {
    user_id: u64,
    name: String,
    color: [u8; 3],          // from color_from_name() FNV hash
    pos: Option<[f32; 2]>,   // minimap-space; None = off-map
    last_update: Instant,     // for fade-out
}
```

Cursor colors are derived from the user's display name via `color_from_name()`
which uses an FNV-1a hash to pick a hue, then converts to RGB with fixed
saturation (0.75) and value (0.90).

## Validation

`validate_peer_message()` in `wt-collab-protocol::validation` performs
structural checks on every incoming message before it is processed:

- Coordinate bounds (`COORD_MIN`..`COORD_MAX`)
- String length limits
- Float finiteness
- Collection size caps (annotations, freehand points, frame size)

Validation is **separate** from permission enforcement. A message must pass
validation first, then the receiver checks the sender's role.

## Key files

| File | Responsibility |
|------|---------------|
| `wt-collab-protocol::protocol` | `PeerMessage`, wire read/write, `CollabRenderOptions`, token encoding |
| `wt-collab-protocol::types` | `Annotation`, `PaintTool`, `color_from_name()` |
| `wt-collab-protocol::validation` | `validate_peer_message`, `validate_annotation` |
| `wt-collab-egui::types` | `UserCursor`, `Permissions`, `ViewportZoomPan`, `MapTransform` |
| `wt-collab-egui::toolbar` | Shared annotation toolbar UI |
| `collab/mod.rs` | `SessionState`, `PeerRole`, `SessionEvent`, `SessionCommand` |
| `collab/peer.rs` | `start_peer_session`, `PeerSessionHandle`, background task |
| `collab/protocol.rs` | Re-exports + desktop-specific conversion helpers |
| `replay_renderer.rs` | `RendererSessionAccess`, `launch_client_renderer`, pending field consumption |
| `app.rs` | IP warning dialog, `do_join_session`, client event polling |
| `ui/replay_parser/mod.rs` | "Session" popover in replay inspector header (host + join + active controls) |

# Collaborative Replay Sessions

Real-time collaborative viewing of replay minimaps over peer-to-peer
networking. A host shares their replay renderer and other users watch and
annotate together.

## Network topology

Sessions use **iroh** (QUIC + NAT traversal) in a mesh topology. The host is
the initial rendezvous point and identity authority; after the handshake every
participant maintains a direct connection to every other participant.

| Layer | Detail |
|-------|--------|
| ALPN | `b"/wows-toolkit-collab/1"` |
| Serialization | rkyv, length-prefixed (`[u32 len][payload]`) |
| Frame compression | zlib via flate2 |
| Limits | 16 MB max message, 4 MB compressed frame, 32 MB decompressed |

## Module layout

```
src/collab/
  mod.rs          SessionState, PeerRole, events, commands
  protocol.rs     PeerMessage enum, wire helpers, CollabRenderOptions
  peer.rs         Background peer task, PeerSessionHandle, start_peer_session
  types.rs        Wire-format Annotation, PaintTool, CURSOR_COLORS palette
  validation.rs   Structural field validation for all PeerMessage variants
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
    annotations_locked: bool,   // peers cannot add/undo annotations
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
6. When the session ends (user clicks **Stop**, or the background task
   detects a disconnect), the handle is cleared and `SessionState` is reset.

## Client flow

1. User clicks **Join** in the **Session** popover (same popover as host controls).
2. The inline form collects the session token and display name. After an
   optional IP disclosure warning, `do_join_session()` calls:

   ```rust
   start_peer_session(runtime, PeerMode::Join(params), shared_session_state())
   ```

3. The background task connects to the host, receives `SessionInfo` (map PNG,
   peer list, assigned identity), and emits `SessionEvent::SessionInfoReceived`.
4. On receiving that event, the app calls `launch_client_renderer()` which
   creates a lightweight `ReplayRendererViewer`:
   - Decodes the map PNG to an RGBA asset.
   - Initializes empty icon maps (no game data needed).
   - Sets renderer status to `Ready` immediately (no playback thread).
   - Has no `video_export_data` (export buttons are hidden).
5. Frames arrive via `handle.frame_rx` and are rendered by the client viewer.

## Message protocol

### Handshake (joiner → host)

| Message | Direction | Purpose |
|---------|-----------|---------|
| `Join` | joiner → host | Version check + display name |
| `SessionInfo` | host → joiner | Peer list, map PNG, assigned identity |
| `Rejected` | host → joiner | Connection denied with reason |
| `PeerAnnounce` | host → existing peers | Introduces the new joiner |
| `MeshHello` | joiner → existing peers | Establishes direct connections |

### Regular messages (any peer → all)

| Message | Gated by |
|---------|----------|
| `CursorPosition(Option<[f32; 2]>)` | — |
| `AddAnnotation(Annotation)` | `annotations_locked` (unless authority) |
| `UndoAnnotation` | `annotations_locked` (unless authority) |
| `ToggleDisplayOption { field, value }` | `settings_locked` (unless authority) |

### Authority messages (host/co-host → all)

| Message | Purpose |
|---------|---------|
| `Permissions { .. }` | Update lock state |
| `RenderOptions(CollabRenderOptions)` | Full display options bundle (27 fields) |
| `AnnotationSync { annotations, owners }` | Replace annotation state |
| `PlaybackState { playing, speed }` | Playback control |
| `UserJoined` / `UserLeft` | Roster (host only) |
| `PromoteToCoHost { user_id }` | Role change (host only) |
| `FrameSourceChanged { source_user_id }` | Declares new frame source |

### Frame sourcing

```rust
Frame { clock, frame_index, total_frames, game_duration, compressed_commands }
```

Receivers only accept `Frame` from the current `frame_source_id`. The host is
the initial frame source; co-hosts can claim the role via `BecomeFrameSource`.
Draw commands are zlib-compressed rkyv payloads.

## Render options sync

`CollabRenderOptions` mirrors `RenderOptions` from the minimap renderer
(27 boolean display toggles). When an authority peer sends
`PeerMessage::RenderOptions`, the receiver stores it in
`SessionState.pending_render_options`. The renderer viewport `.take()`s it
each frame and applies field-by-field to `SharedRendererState.options`.

## Annotation model

There are two `Annotation` enums with identical variants but different field
types:

| Type | Module | Fields | Used for |
|------|--------|--------|----------|
| `collab::types::Annotation` | `collab/types.rs` | `[f32; 2]`, `[u8; 4]` | Wire transport (rkyv-serializable) |
| `replay_renderer::Annotation` | `replay_renderer.rs` | `Vec2`, `Color32` | UI rendering (egui types) |

Conversion functions in `replay_renderer.rs`:

- `collab_annotation_to_local()` — wire → UI (used when applying syncs)
- `local_annotation_to_collab()` — UI → wire (used when broadcasting)

Annotation ownership is tracked in a parallel `Vec<u64>` (`annotation_owners`)
so each annotation can be attributed to its creator.

## Cursor rendering

Each user's cursor position is tracked in `SessionState.cursors` as:

```rust
struct UserCursor {
    user_id: u64,
    name: String,
    color: [u8; 3],          // from CURSOR_COLORS palette
    pos: Option<[f32; 2]>,   // minimap-space; None = off-map
    last_update: Instant,     // for fade-out
}
```

The `CURSOR_COLORS` palette has 12 distinct colors. Index 0 (red) is reserved
for the host; clients are assigned round-robin from index 1.

## Validation

`validate_peer_message()` in `validation.rs` performs structural checks on
every incoming message before it is processed:

- Coordinate bounds (`COORD_MIN`..`COORD_MAX`)
- String length limits
- Float finiteness
- Collection size caps (annotations, freehand points, frame size)

Validation is **separate** from permission enforcement. A message must pass
validation first, then the receiver checks the sender's role.

## Key files

| File | Responsibility |
|------|---------------|
| `collab/mod.rs` | `SessionState`, `PeerRole`, `SessionEvent`, `SessionCommand` |
| `collab/peer.rs` | `start_peer_session`, `PeerSessionHandle`, background task |
| `collab/protocol.rs` | `PeerMessage`, wire read/write, `CollabRenderOptions` |
| `collab/types.rs` | Wire-format `Annotation`, `PaintTool`, `CURSOR_COLORS` |
| `collab/validation.rs` | `validate_peer_message`, `validate_annotation` |
| `replay_renderer.rs` | `RendererSessionAccess`, `launch_client_renderer`, annotation conversion, pending field consumption |
| `app.rs` | IP warning dialog, `do_join_session`, client event polling |
| `tab_state.rs` | `pending_join`, `client_session` fields |
| `ui/replay_parser/mod.rs` | "Session" popover in replay inspector header (host + join + active controls) |

pub mod shapes;
pub mod tactics;

use std::sync::Arc;
use std::sync::mpsc;

use egui::Vec2;
use parking_lot::Mutex;

use crate::collab;
use crate::collab::peer::LocalAnnotationEvent;
use crate::collab::peer::LocalEvent;

// Re-export shared types from wt-collab-egui so existing consumers compile unchanged.
pub use wt_collab_egui::transforms::MapTransform;
pub use wt_collab_egui::transforms::ViewportZoomPan;
pub use wt_collab_egui::types::Annotation;
pub use wt_collab_egui::types::AnnotationState;
pub use wt_collab_egui::types::ENEMY_COLOR;
pub use wt_collab_egui::types::FRIENDLY_COLOR;
pub use wt_collab_egui::types::PaintTool;
/// Overlay controls visibility state. Persists across frames.
pub struct OverlayState {
    /// Last time the mouse moved or a control was interacted with (ctx.input time).
    pub last_activity: f64,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self { last_activity: 0.0 }
    }
}
/// Convert a collab wire annotation (primitive arrays) to the local annotation (egui types).
pub fn collab_annotation_to_local(ca: crate::collab::types::Annotation) -> Annotation {
    wt_collab_egui::types::wire_to_local(ca)
}

/// Convert a local annotation (egui types) to collab wire annotation (primitive arrays).
pub fn local_annotation_to_collab(a: &Annotation) -> crate::collab::types::Annotation {
    wt_collab_egui::types::local_to_wire(a)
}
/// Send a `SetAnnotation` event for the annotation at `idx` via the collab channel.
pub fn send_annotation_update(
    tx: &Option<mpsc::Sender<LocalEvent>>,
    ann: &AnnotationState,
    idx: usize,
    board_id: Option<u64>,
) {
    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Annotation(LocalAnnotationEvent::Set {
            board_id,
            id: ann.annotation_ids[idx],
            annotation: local_annotation_to_collab(&ann.annotations[idx]),
            owner: ann.annotation_owners.get(idx).copied().unwrap_or(0),
        }));
    }
}

/// Send a `RemoveAnnotation` event for the given annotation ID via the collab channel.
pub fn send_annotation_remove(tx: &Option<mpsc::Sender<LocalEvent>>, id: u64, board_id: Option<u64>) {
    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Annotation(LocalAnnotationEvent::Remove { board_id, id }));
    }
}

/// Send a `ClearAnnotations` event via the collab channel.
pub fn send_annotation_clear(tx: &Option<mpsc::Sender<LocalEvent>>, board_id: Option<u64>) {
    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Annotation(LocalAnnotationEvent::Clear { board_id }));
    }
}

/// Send a full annotation sync (used after undo to broadcast the complete state).
pub fn send_annotation_full_sync(
    tx: &Option<mpsc::Sender<collab::SessionCommand>>,
    ann: &AnnotationState,
    board_id: Option<u64>,
) {
    if let Some(tx) = tx {
        let collab_anns: Vec<_> = ann.annotations.iter().map(local_annotation_to_collab).collect();
        let _ = tx.send(collab::SessionCommand::SyncAnnotations {
            board_id,
            annotations: collab_anns,
            owners: ann.annotation_owners.clone(),
            ids: ann.annotation_ids.clone(),
        });
    }
}

/// Get the local user's ID from the collab session state, or 0 if not in a session.
pub fn get_my_user_id(session: &Option<Arc<Mutex<collab::SessionState>>>) -> u64 {
    session.as_ref().map(|ss| ss.lock().my_user_id).unwrap_or(0)
}

/// Handle a click on empty map space: create a visible ping and optionally
/// notify collab peers.
///
/// - In a session: uses the user's cursor color, pushes to session pings,
///   and sends `LocalEvent::Ping` so peers see it too.
/// - Not in a session: creates a white local-only ping in `local_pings`.
///
/// The relay system does not echo pings back to the sender, so the sender
/// must always add their own ping locally.
pub fn handle_map_click_ping(
    click_pos: Vec2,
    local_pings: &mut Vec<shapes::MapPing>,
    session: &Option<Arc<Mutex<collab::SessionState>>>,
    tx: &Option<mpsc::Sender<LocalEvent>>,
) {
    let pos = [click_pos.x, click_pos.y];

    if let Some(ss_arc) = session {
        let mut ss = ss_arc.lock();
        let my_id = ss.my_user_id;
        let color = ss.cursors.iter().find(|c| c.user_id == my_id).map(|c| c.color).unwrap_or([255, 255, 255]);
        ss.pings.push(collab::PeerPing { user_id: my_id, color, pos, time: web_time::Instant::now() });
    } else {
        local_pings.push(shapes::MapPing { pos, color: [255, 255, 255], time: web_time::Instant::now() });
    }

    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Ping(pos));
    }
}

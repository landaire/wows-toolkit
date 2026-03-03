//! Sanity checks for all collaborative session messages.
//!
//! Every receiver validates incoming `PeerMessage`s before processing them.
//! This prevents malformed data from a single peer from affecting other
//! participants. Permission / role enforcement is separate from structural
//! validation and is handled at the message-routing layer.

use std::fmt;

use crate::collab::protocol::*;
use crate::collab::types::Annotation;

/// Validation error with a human-readable description.
#[derive(Debug)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ValidationError {}

// ─── Primitive helpers ──────────────────────────────────────────────────────

fn check_finite(v: f32, name: &str) -> Result<(), ValidationError> {
    if !v.is_finite() {
        return Err(ValidationError(format!("{name} is not finite: {v}")));
    }
    Ok(())
}

fn check_coord(v: f32, name: &str) -> Result<(), ValidationError> {
    check_finite(v, name)?;
    if !(COORD_MIN..=COORD_MAX).contains(&v) {
        return Err(ValidationError(format!("{name} out of range: {v} (expected {COORD_MIN}..{COORD_MAX})")));
    }
    Ok(())
}

fn check_position(pos: &[f32; 2], name: &str) -> Result<(), ValidationError> {
    check_coord(pos[0], &format!("{name}.x"))?;
    check_coord(pos[1], &format!("{name}.y"))?;
    Ok(())
}

fn check_string_len(s: &str, max: usize, name: &str) -> Result<(), ValidationError> {
    if s.len() > max {
        return Err(ValidationError(format!("{name} too long: {} > {max}", s.len())));
    }
    Ok(())
}

// ─── Annotation validation ──────────────────────────────────────────────────

/// Validate a single annotation's fields.
pub fn validate_annotation(ann: &Annotation) -> Result<(), ValidationError> {
    match ann {
        Annotation::Ship { pos, yaw, species, .. } => {
            check_position(pos, "Ship.pos")?;
            check_finite(*yaw, "Ship.yaw")?;
            check_string_len(species, MAX_STRING_LEN, "Ship.species")?;
        }
        Annotation::FreehandStroke { points, width, .. } => {
            if points.len() > MAX_FREEHAND_POINTS {
                return Err(ValidationError(format!(
                    "FreehandStroke has {} points (max {MAX_FREEHAND_POINTS})",
                    points.len()
                )));
            }
            for (i, p) in points.iter().enumerate() {
                check_position(p, &format!("FreehandStroke.points[{i}]"))?;
            }
            check_finite(*width, "FreehandStroke.width")?;
            if *width <= 0.0 || *width > MAX_STROKE_WIDTH {
                return Err(ValidationError(format!("FreehandStroke.width out of range: {width}")));
            }
        }
        Annotation::Line { start, end, width, .. } => {
            check_position(start, "Line.start")?;
            check_position(end, "Line.end")?;
            check_finite(*width, "Line.width")?;
            if *width <= 0.0 || *width > MAX_STROKE_WIDTH {
                return Err(ValidationError(format!("Line.width out of range: {width}")));
            }
        }
        Annotation::Circle { center, radius, width, .. } => {
            check_position(center, "Circle.center")?;
            check_finite(*radius, "Circle.radius")?;
            if *radius <= 0.0 || *radius > MAX_RADIUS {
                return Err(ValidationError(format!("Circle.radius out of range: {radius}")));
            }
            check_finite(*width, "Circle.width")?;
            if *width <= 0.0 || *width > MAX_STROKE_WIDTH {
                return Err(ValidationError(format!("Circle.width out of range: {width}")));
            }
        }
        Annotation::Rectangle { center, half_size, rotation, width, .. } => {
            check_position(center, "Rectangle.center")?;
            check_position(half_size, "Rectangle.half_size")?;
            check_finite(*rotation, "Rectangle.rotation")?;
            check_finite(*width, "Rectangle.width")?;
            if *width <= 0.0 || *width > MAX_STROKE_WIDTH {
                return Err(ValidationError(format!("Rectangle.width out of range: {width}")));
            }
        }
        Annotation::Triangle { center, radius, rotation, width, .. } => {
            check_position(center, "Triangle.center")?;
            check_finite(*radius, "Triangle.radius")?;
            if *radius <= 0.0 || *radius > MAX_RADIUS {
                return Err(ValidationError(format!("Triangle.radius out of range: {radius}")));
            }
            check_finite(*rotation, "Triangle.rotation")?;
            check_finite(*width, "Triangle.width")?;
            if *width <= 0.0 || *width > MAX_STROKE_WIDTH {
                return Err(ValidationError(format!("Triangle.width out of range: {width}")));
            }
        }
    }
    Ok(())
}

// ─── Unified peer message validation ─────────────────────────────────────────

/// Validate a `PeerMessage`'s structural fields.
///
/// This does NOT check role-based permissions — only that all field values are
/// within acceptable bounds. Role enforcement is done at the routing layer.
pub fn validate_peer_message(msg: &PeerMessage) -> Result<(), ValidationError> {
    match msg {
        PeerMessage::Join { toolkit_version, name } => {
            check_string_len(toolkit_version, 100, "Join.toolkit_version")?;
            if name.is_empty() {
                return Err(ValidationError("Join.name is empty".into()));
            }
            check_string_len(name, MAX_DISPLAY_NAME_LEN, "Join.name")?;
        }

        PeerMessage::SessionInfo { toolkit_version, peers, assigned_identity, open_replays, .. } => {
            check_string_len(toolkit_version, 100, "SessionInfo.toolkit_version")?;
            if peers.len() > MAX_PEERS {
                return Err(ValidationError(format!("too many peers: {} > {MAX_PEERS}", peers.len())));
            }
            for p in peers {
                check_string_len(&p.name, MAX_DISPLAY_NAME_LEN, "PeerInfo.name")?;
                check_string_len(&p.endpoint_addr_json, MAX_ENDPOINT_ADDR_LEN, "PeerInfo.endpoint_addr_json")?;
            }
            check_string_len(&assigned_identity.name, MAX_DISPLAY_NAME_LEN, "assigned_identity.name")?;
            for r in open_replays {
                validate_replay_info(r)?;
            }
        }

        PeerMessage::Rejected { reason } => {
            check_string_len(reason, 500, "Rejected.reason")?;
        }

        PeerMessage::PeerAnnounce { peer } => {
            check_string_len(&peer.name, MAX_DISPLAY_NAME_LEN, "PeerAnnounce.name")?;
            check_string_len(&peer.endpoint_addr_json, MAX_ENDPOINT_ADDR_LEN, "PeerAnnounce.endpoint_addr_json")?;
        }

        PeerMessage::MeshHello { name, .. } => {
            check_string_len(name, MAX_DISPLAY_NAME_LEN, "MeshHello.name")?;
        }

        PeerMessage::CursorPosition(pos) => {
            if let Some(p) = pos {
                check_position(p, "CursorPosition")?;
            }
        }

        PeerMessage::SetAnnotation { annotation, .. } => {
            validate_annotation(annotation)?;
        }

        PeerMessage::RemoveAnnotation { .. } => {}

        PeerMessage::ClearAnnotations => {}

        PeerMessage::ToggleDisplayOption { .. } => {
            // DisplayOptionField is exhaustive — rkyv rejects unknown variants.
        }

        PeerMessage::Permissions { .. } => {}

        PeerMessage::RenderOptions(_) => {}

        PeerMessage::AnnotationSync { annotations, owners, ids } => {
            if annotations.len() > MAX_ANNOTATIONS {
                return Err(ValidationError(format!(
                    "too many annotations: {} > {MAX_ANNOTATIONS}",
                    annotations.len()
                )));
            }
            if annotations.len() != owners.len() {
                return Err(ValidationError(format!(
                    "annotations/owners length mismatch: {} vs {}",
                    annotations.len(),
                    owners.len()
                )));
            }
            if annotations.len() != ids.len() {
                return Err(ValidationError(format!(
                    "annotations/ids length mismatch: {} vs {}",
                    annotations.len(),
                    ids.len()
                )));
            }
            for ann in annotations {
                validate_annotation(ann)?;
            }
        }

        PeerMessage::PlaybackState { speed, .. } => {
            check_finite(*speed, "PlaybackState.speed")?;
        }

        PeerMessage::UserJoined { name, .. } => {
            check_string_len(name, MAX_DISPLAY_NAME_LEN, "UserJoined.name")?;
        }

        PeerMessage::UserLeft { .. } => {}

        PeerMessage::PromoteToCoHost { .. } => {}

        PeerMessage::FrameSourceChanged { .. } => {}

        PeerMessage::Frame { clock, game_duration, frame_index, total_frames, compressed_commands, .. } => {
            check_finite(*clock, "Frame.clock")?;
            check_finite(*game_duration, "Frame.game_duration")?;
            if *frame_index > *total_frames {
                return Err(ValidationError(format!("frame_index ({frame_index}) > total_frames ({total_frames})")));
            }
            if compressed_commands.len() > MAX_FRAME_SIZE {
                return Err(ValidationError(format!(
                    "compressed_commands too large: {} > {MAX_FRAME_SIZE}",
                    compressed_commands.len()
                )));
            }
        }

        PeerMessage::ReplayOpened { replay_name, map_image_png, game_version, .. } => {
            validate_replay_info_fields(replay_name, map_image_png, game_version)?;
        }

        PeerMessage::ReplayClosed { .. } => {}

        PeerMessage::ShipRangeOverrides { .. } => {}

        PeerMessage::ShipTrailOverrides { hidden } => {
            if hidden.len() > MAX_PEERS * 24 {
                return Err(ValidationError(format!(
                    "ShipTrailOverrides too many entries: {} > {}",
                    hidden.len(),
                    MAX_PEERS * 24
                )));
            }
            for name in hidden {
                check_string_len(name, MAX_DISPLAY_NAME_LEN, "ShipTrailOverrides.hidden")?;
            }
        }

        PeerMessage::Ping { pos } => {
            check_position(pos, "Ping.pos")?;
        }
    }
    Ok(())
}

/// Validate a `ReplayInfo` struct's fields.
pub fn validate_replay_info(r: &ReplayInfo) -> Result<(), ValidationError> {
    validate_replay_info_fields(&r.replay_name, &r.map_image_png, &r.game_version)
}

fn validate_replay_info_fields(
    replay_name: &str,
    map_image_png: &[u8],
    game_version: &str,
) -> Result<(), ValidationError> {
    check_string_len(replay_name, MAX_STRING_LEN, "ReplayInfo.replay_name")?;
    check_string_len(game_version, 100, "ReplayInfo.game_version")?;
    if map_image_png.len() > MAX_MAP_IMAGE_SIZE {
        return Err(ValidationError(format!(
            "ReplayInfo.map_image_png too large: {} > {MAX_MAP_IMAGE_SIZE}",
            map_image_png.len()
        )));
    }
    Ok(())
}

/// Validate decompressed frame commands count.
pub fn validate_frame_commands_count(count: usize) -> Result<(), ValidationError> {
    if count > MAX_COMMANDS_PER_FRAME {
        return Err(ValidationError(format!("too many draw commands: {count} > {MAX_COMMANDS_PER_FRAME}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Helper builders ─────────────────────────────────────────────────

    fn valid_circle() -> Annotation {
        Annotation::Circle { center: [100.0, 200.0], radius: 50.0, color: [255, 0, 0, 255], width: 3.0, filled: false }
    }

    fn valid_line() -> Annotation {
        Annotation::Line { start: [0.0, 0.0], end: [100.0, 100.0], color: [0, 255, 0, 255], width: 2.0 }
    }

    fn valid_rect() -> Annotation {
        Annotation::Rectangle {
            center: [400.0, 400.0],
            half_size: [50.0, 30.0],
            rotation: 0.5,
            color: [0, 0, 255, 255],
            width: 2.0,
            filled: true,
        }
    }

    fn valid_triangle() -> Annotation {
        Annotation::Triangle {
            center: [300.0, 300.0],
            radius: 40.0,
            rotation: 1.0,
            color: [255, 255, 0, 255],
            width: 4.0,
            filled: false,
        }
    }

    fn valid_ship() -> Annotation {
        Annotation::Ship { pos: [380.0, 380.0], yaw: 1.57, species: "Destroyer".into(), friendly: true }
    }

    fn valid_freehand() -> Annotation {
        Annotation::FreehandStroke {
            points: vec![[10.0, 10.0], [20.0, 20.0], [30.0, 15.0]],
            color: [128, 128, 128, 255],
            width: 5.0,
        }
    }

    // ─── Annotation: valid cases ─────────────────────────────────────────

    #[test]
    fn valid_annotations_pass() {
        assert!(validate_annotation(&valid_circle()).is_ok());
        assert!(validate_annotation(&valid_line()).is_ok());
        assert!(validate_annotation(&valid_rect()).is_ok());
        assert!(validate_annotation(&valid_triangle()).is_ok());
        assert!(validate_annotation(&valid_ship()).is_ok());
        assert!(validate_annotation(&valid_freehand()).is_ok());
    }

    #[test]
    fn annotation_at_coord_boundaries() {
        let at_min = Annotation::Circle {
            center: [COORD_MIN, COORD_MIN],
            radius: 1.0,
            color: [0; 4],
            width: 1.0,
            filled: false,
        };
        let at_max = Annotation::Circle {
            center: [COORD_MAX, COORD_MAX],
            radius: MAX_RADIUS,
            color: [0; 4],
            width: MAX_STROKE_WIDTH,
            filled: false,
        };
        assert!(validate_annotation(&at_min).is_ok());
        assert!(validate_annotation(&at_max).is_ok());
    }

    // ─── Annotation: invalid coordinates ─────────────────────────────────

    #[test]
    fn annotation_nan_position_rejected() {
        let ann =
            Annotation::Circle { center: [f32::NAN, 100.0], radius: 10.0, color: [0; 4], width: 2.0, filled: false };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn annotation_infinity_position_rejected() {
        let ann = Annotation::Line { start: [f32::INFINITY, 0.0], end: [0.0, 0.0], color: [0; 4], width: 2.0 };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn annotation_coord_too_low_rejected() {
        let ann = Annotation::Circle {
            center: [COORD_MIN - 1.0, 0.0],
            radius: 10.0,
            color: [0; 4],
            width: 2.0,
            filled: false,
        };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn annotation_coord_too_high_rejected() {
        let ann = Annotation::Circle {
            center: [0.0, COORD_MAX + 1.0],
            radius: 10.0,
            color: [0; 4],
            width: 2.0,
            filled: false,
        };
        assert!(validate_annotation(&ann).is_err());
    }

    // ─── Annotation: invalid width ───────────────────────────────────────

    #[test]
    fn annotation_zero_width_rejected() {
        let ann = Annotation::Line { start: [0.0, 0.0], end: [10.0, 10.0], color: [0; 4], width: 0.0 };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn annotation_negative_width_rejected() {
        let ann =
            Annotation::Circle { center: [100.0, 100.0], radius: 10.0, color: [0; 4], width: -1.0, filled: false };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn annotation_width_exceeds_max_rejected() {
        let ann =
            Annotation::Line { start: [0.0, 0.0], end: [10.0, 10.0], color: [0; 4], width: MAX_STROKE_WIDTH + 0.01 };
        assert!(validate_annotation(&ann).is_err());
    }

    // ─── Annotation: invalid radius ──────────────────────────────────────

    #[test]
    fn annotation_zero_radius_rejected() {
        let ann = Annotation::Circle { center: [100.0, 100.0], radius: 0.0, color: [0; 4], width: 2.0, filled: false };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn annotation_radius_exceeds_max_rejected() {
        let ann = Annotation::Triangle {
            center: [100.0, 100.0],
            radius: MAX_RADIUS + 1.0,
            rotation: 0.0,
            color: [0; 4],
            width: 2.0,
            filled: false,
        };
        assert!(validate_annotation(&ann).is_err());
    }

    // ─── Annotation: freehand specifics ──────────────────────────────────

    #[test]
    fn freehand_too_many_points_rejected() {
        let points: Vec<[f32; 2]> = (0..MAX_FREEHAND_POINTS + 1).map(|i| [i as f32, 0.0]).collect();
        let ann = Annotation::FreehandStroke { points, color: [0; 4], width: 2.0 };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn freehand_at_max_points_accepted() {
        let points: Vec<[f32; 2]> = (0..MAX_FREEHAND_POINTS).map(|i| [(i % 2000) as f32, 0.0]).collect();
        let ann = Annotation::FreehandStroke { points, color: [0; 4], width: 2.0 };
        assert!(validate_annotation(&ann).is_ok());
    }

    #[test]
    fn freehand_invalid_point_position_rejected() {
        let ann = Annotation::FreehandStroke { points: vec![[0.0, 0.0], [f32::NAN, 0.0]], color: [0; 4], width: 2.0 };
        assert!(validate_annotation(&ann).is_err());
    }

    // ─── Annotation: ship specifics ──────────────────────────────────────

    #[test]
    fn ship_nan_yaw_rejected() {
        let ann = Annotation::Ship { pos: [100.0, 100.0], yaw: f32::NAN, species: "BB".into(), friendly: true };
        assert!(validate_annotation(&ann).is_err());
    }

    #[test]
    fn ship_species_too_long_rejected() {
        let ann =
            Annotation::Ship { pos: [100.0, 100.0], yaw: 0.0, species: "x".repeat(MAX_STRING_LEN + 1), friendly: true };
        assert!(validate_annotation(&ann).is_err());
    }

    // ─── Annotation: rectangle specifics ─────────────────────────────────

    #[test]
    fn rectangle_nan_rotation_rejected() {
        let ann = Annotation::Rectangle {
            center: [100.0, 100.0],
            half_size: [50.0, 30.0],
            rotation: f32::NAN,
            color: [0; 4],
            width: 2.0,
            filled: false,
        };
        assert!(validate_annotation(&ann).is_err());
    }

    // ─── PeerMessage: valid cases ────────────────────────────────────────

    #[test]
    fn valid_join_message() {
        let msg = PeerMessage::Join { toolkit_version: "1.0.0".into(), name: "Alice".into() };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_cursor_position_some() {
        let msg = PeerMessage::CursorPosition(Some([380.0, 380.0]));
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_cursor_position_none() {
        let msg = PeerMessage::CursorPosition(None);
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_set_annotation() {
        let msg = PeerMessage::SetAnnotation { id: 42, annotation: valid_circle(), owner: 1 };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_remove_annotation() {
        let msg = PeerMessage::RemoveAnnotation { id: 42 };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_clear_annotations() {
        assert!(validate_peer_message(&PeerMessage::ClearAnnotations).is_ok());
    }

    #[test]
    fn valid_annotation_sync() {
        let msg = PeerMessage::AnnotationSync {
            annotations: vec![valid_circle(), valid_line()],
            owners: vec![0, 1],
            ids: vec![100, 101],
        };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_playback_state() {
        let msg = PeerMessage::PlaybackState { playing: true, speed: 2.0 };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_ping() {
        let msg = PeerMessage::Ping { pos: [380.0, 380.0] };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_frame() {
        let msg = PeerMessage::Frame {
            replay_id: 1,
            clock: 60.0,
            frame_index: 5,
            total_frames: 100,
            game_duration: 1200.0,
            compressed_commands: vec![0u8; 1024],
        };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_replay_opened() {
        let msg = PeerMessage::ReplayOpened {
            replay_id: 1,
            replay_name: "my_replay.wowsreplay".into(),
            map_image_png: vec![0u8; 100],
            game_version: "13.5.0".into(),
        };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn valid_ship_trail_overrides() {
        let msg = PeerMessage::ShipTrailOverrides { hidden: vec!["Player1".into(), "Player2".into()] };
        assert!(validate_peer_message(&msg).is_ok());
    }

    // ─── PeerMessage: invalid cases ──────────────────────────────────────

    #[test]
    fn join_empty_name_rejected() {
        let msg = PeerMessage::Join { toolkit_version: "1.0".into(), name: String::new() };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn join_name_too_long_rejected() {
        let msg = PeerMessage::Join { toolkit_version: "1.0".into(), name: "x".repeat(MAX_DISPLAY_NAME_LEN + 1) };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn join_name_at_max_accepted() {
        let msg = PeerMessage::Join { toolkit_version: "1.0".into(), name: "x".repeat(MAX_DISPLAY_NAME_LEN) };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn join_version_too_long_rejected() {
        let msg = PeerMessage::Join { toolkit_version: "x".repeat(101), name: "Alice".into() };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn cursor_out_of_range_rejected() {
        let msg = PeerMessage::CursorPosition(Some([COORD_MAX + 1.0, 0.0]));
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn set_annotation_with_invalid_annotation_rejected() {
        let msg = PeerMessage::SetAnnotation {
            id: 1,
            annotation: Annotation::Circle {
                center: [f32::NAN, 0.0],
                radius: 10.0,
                color: [0; 4],
                width: 2.0,
                filled: false,
            },
            owner: 0,
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn annotation_sync_too_many_annotations_rejected() {
        let anns: Vec<Annotation> = (0..MAX_ANNOTATIONS + 1)
            .map(|i| Annotation::Circle {
                center: [(i % 2000) as f32, 0.0],
                radius: 10.0,
                color: [0; 4],
                width: 2.0,
                filled: false,
            })
            .collect();
        let count = anns.len();
        let msg =
            PeerMessage::AnnotationSync { annotations: anns, owners: vec![0; count], ids: (0..count as u64).collect() };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn annotation_sync_owners_length_mismatch_rejected() {
        let msg = PeerMessage::AnnotationSync {
            annotations: vec![valid_circle()],
            owners: vec![0, 1], // 2 owners for 1 annotation
            ids: vec![1],
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn annotation_sync_ids_length_mismatch_rejected() {
        let msg = PeerMessage::AnnotationSync {
            annotations: vec![valid_circle()],
            owners: vec![0],
            ids: vec![1, 2], // 2 IDs for 1 annotation
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn annotation_sync_empty_is_valid() {
        let msg = PeerMessage::AnnotationSync { annotations: vec![], owners: vec![], ids: vec![] };
        assert!(validate_peer_message(&msg).is_ok());
    }

    #[test]
    fn annotation_sync_invalid_annotation_rejected() {
        let msg = PeerMessage::AnnotationSync {
            annotations: vec![
                valid_circle(),
                Annotation::Circle { center: [f32::NAN, 0.0], radius: 10.0, color: [0; 4], width: 2.0, filled: false },
            ],
            owners: vec![0, 0],
            ids: vec![1, 2],
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn playback_speed_nan_rejected() {
        let msg = PeerMessage::PlaybackState { playing: true, speed: f32::NAN };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn frame_index_exceeds_total_rejected() {
        let msg = PeerMessage::Frame {
            replay_id: 1,
            clock: 0.0,
            frame_index: 101,
            total_frames: 100,
            game_duration: 1200.0,
            compressed_commands: vec![],
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn frame_clock_nan_rejected() {
        let msg = PeerMessage::Frame {
            replay_id: 1,
            clock: f32::NAN,
            frame_index: 0,
            total_frames: 100,
            game_duration: 1200.0,
            compressed_commands: vec![],
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn frame_compressed_commands_too_large_rejected() {
        let msg = PeerMessage::Frame {
            replay_id: 1,
            clock: 0.0,
            frame_index: 0,
            total_frames: 100,
            game_duration: 1200.0,
            compressed_commands: vec![0u8; MAX_FRAME_SIZE + 1],
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn user_joined_name_too_long_rejected() {
        let msg = PeerMessage::UserJoined { user_id: 1, name: "x".repeat(MAX_DISPLAY_NAME_LEN + 1), color: [0; 3] };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn replay_opened_name_too_long_rejected() {
        let msg = PeerMessage::ReplayOpened {
            replay_id: 1,
            replay_name: "x".repeat(MAX_STRING_LEN + 1),
            map_image_png: vec![],
            game_version: "13.5".into(),
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn replay_opened_map_too_large_rejected() {
        let msg = PeerMessage::ReplayOpened {
            replay_id: 1,
            replay_name: "test.wowsreplay".into(),
            map_image_png: vec![0u8; MAX_MAP_IMAGE_SIZE + 1],
            game_version: "13.5".into(),
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn ship_trail_overrides_too_many_rejected() {
        let msg =
            PeerMessage::ShipTrailOverrides { hidden: (0..MAX_PEERS * 24 + 1).map(|i| format!("P{i}")).collect() };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn ship_trail_overrides_name_too_long_rejected() {
        let msg = PeerMessage::ShipTrailOverrides { hidden: vec!["x".repeat(MAX_DISPLAY_NAME_LEN + 1)] };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn ping_out_of_range_rejected() {
        let msg = PeerMessage::Ping { pos: [0.0, COORD_MAX + 1.0] };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn rejected_reason_too_long_rejected() {
        let msg = PeerMessage::Rejected { reason: "x".repeat(501) };
        assert!(validate_peer_message(&msg).is_err());
    }

    #[test]
    fn session_info_too_many_peers_rejected() {
        let peers: Vec<PeerInfo> = (0..MAX_PEERS + 1)
            .map(|i| PeerInfo {
                user_id: i as u64,
                name: format!("Peer{i}"),
                color: [0; 3],
                endpoint_addr_json: "{}".into(),
            })
            .collect();
        let msg = PeerMessage::SessionInfo {
            toolkit_version: "1.0".into(),
            peers,
            assigned_identity: PeerIdentity { user_id: 99, name: "Me".into(), color: [0; 3] },
            frame_source_id: 0,
            open_replays: vec![],
        };
        assert!(validate_peer_message(&msg).is_err());
    }

    // ─── ReplayInfo validation ───────────────────────────────────────────

    #[test]
    fn valid_replay_info_passes() {
        let info = ReplayInfo {
            replay_id: 1,
            replay_name: "test.wowsreplay".into(),
            map_image_png: vec![0u8; 100],
            game_version: "13.5.0".into(),
        };
        assert!(validate_replay_info(&info).is_ok());
    }

    // ─── Frame commands count ────────────────────────────────────────────

    #[test]
    fn frame_commands_at_max_accepted() {
        assert!(validate_frame_commands_count(MAX_COMMANDS_PER_FRAME).is_ok());
    }

    #[test]
    fn frame_commands_over_max_rejected() {
        assert!(validate_frame_commands_count(MAX_COMMANDS_PER_FRAME + 1).is_err());
    }

    #[test]
    fn frame_commands_zero_accepted() {
        assert!(validate_frame_commands_count(0).is_ok());
    }
}

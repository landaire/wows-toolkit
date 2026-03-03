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

        PeerMessage::AddAnnotation(ann) => {
            validate_annotation(ann)?;
        }

        PeerMessage::UndoAnnotation => {}

        PeerMessage::ToggleDisplayOption { .. } => {
            // DisplayOptionField is exhaustive — rkyv rejects unknown variants.
        }

        PeerMessage::Permissions { .. } => {}

        PeerMessage::RenderOptions(_) => {}

        PeerMessage::AnnotationSync { annotations, owners } => {
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

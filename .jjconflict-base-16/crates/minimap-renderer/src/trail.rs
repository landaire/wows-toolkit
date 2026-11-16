//! Position trails derived from frames already drawn.
//!
//! Pure functions of the draw commands, so a viewer with no replay parser (the
//! web demo) derives trails the same way the desktop viewports do.

use std::collections::HashMap;

use wows_replays::types::EntityId;

use crate::draw_command::DrawCommand;
use crate::map_data::MinimapPos;

/// A hue on the blue-to-red ramp a trail is coloured along.
pub fn hue_to_rgb(hue: f32) -> [u8; 3] {
    let h = hue / 60.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let q = (1.0 - f) * 255.0;
    let t = f * 255.0;
    match i % 6 {
        0 => [255, t as u8, 0],
        1 => [q as u8, 255, 0],
        2 => [0, 255, t as u8],
        3 => [0, q as u8, 255],
        4 => [t as u8, 0, 255],
        _ => [255, 0, q as u8],
    }
}

/// Where each ship has been, read back out of the frames already baked.
///
/// A trail command carries every point so far, so baking one per frame would
/// cost the square of the track's length. The positions are already in the
/// track's own ship commands, so a viewport derives the trails for the frame
/// it is about to draw instead, at the cost of one walk of the frames behind
/// it.
///
/// Oldest points are blue and newest red, as the renderer's own trails are.
pub fn trails_through(frames: &[Vec<DrawCommand>]) -> Vec<DrawCommand> {
    let mut order: Vec<EntityId> = Vec::new();
    let mut tracks: HashMap<EntityId, (Option<String>, Vec<MinimapPos>)> = HashMap::new();

    for commands in frames {
        for command in commands {
            let (entity_id, pos, player_name) = match command {
                DrawCommand::Ship { entity_id, pos, player_name, .. } => (entity_id, pos, player_name.clone()),
                // A sunk ship keeps the trail it made while it was alive.
                DrawCommand::DeadShip { entity_id, pos, player_name, .. } => (entity_id, pos, player_name.clone()),
                _ => continue,
            };
            let entry = tracks.entry(*entity_id).or_insert_with(|| {
                order.push(*entity_id);
                (None, Vec::new())
            });
            if entry.0.is_none() {
                entry.0 = player_name;
            }
            // A ship that has not moved since the last frame adds nothing.
            if entry.1.last().map(|last| (last.x, last.y)) != Some((pos.x, pos.y)) {
                entry.1.push(*pos);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|entity_id| {
            let (player_name, points) = tracks.remove(&entity_id)?;
            // One point is a position, not a trail.
            if points.len() < 2 {
                return None;
            }
            let last = points.len() - 1;
            let points = points
                .into_iter()
                .enumerate()
                .map(|(index, pos)| (pos, hue_to_rgb(240.0 * (1.0 - index as f32 / last as f32))))
                .collect();
            Some(DrawCommand::PositionTrail { entity_id, player_name, points })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_command::ShipVisibility;

    fn ship(entity: u32, x: f32, y: f32) -> DrawCommand {
        DrawCommand::Ship {
            entity_id: EntityId::from(entity),
            pos: MinimapPos { x, y },
            yaw: 0.0,
            species: None,
            color: None,
            visibility: ShipVisibility::Visible,
            opacity: 1.0,
            is_self: false,
            player_name: Some(format!("player{entity}")),
            ship_name: None,
            is_detected_teammate: false,
            is_disconnected: false,
            name_color: None,
        }
    }

    /// A trail is where a ship has been across the frames behind this one.
    #[test]
    fn a_trail_follows_one_ship_through_the_frames() {
        let frames = vec![
            vec![ship(1, 0.0, 0.0), ship(2, 100.0, 100.0)],
            vec![ship(1, 10.0, 0.0), ship(2, 110.0, 100.0)],
            vec![ship(1, 20.0, 0.0)],
        ];

        let trails = trails_through(&frames);
        assert_eq!(trails.len(), 2, "one per ship that moved");

        let DrawCommand::PositionTrail { points, player_name, .. } = &trails[0] else {
            panic!("expected a trail, got {:?}", trails[0]);
        };
        assert_eq!(player_name.as_deref(), Some("player1"));
        assert_eq!(points.len(), 3, "one point per frame it moved in");
        assert_eq!(points[0].0.x, 0.0);
        assert_eq!(points[2].0.x, 20.0);
        assert_ne!(points[0].1, points[2].1, "oldest and newest are coloured differently");
    }

    /// A ship that has not moved is a position, not a trail.
    #[test]
    fn a_ship_that_never_moved_has_no_trail() {
        let frames = vec![vec![ship(1, 5.0, 5.0)], vec![ship(1, 5.0, 5.0)], vec![ship(1, 5.0, 5.0)]];
        assert!(trails_through(&frames).is_empty());
    }

    /// Deriving costs one walk of the frames behind, where baking a trail per
    /// frame would have cost the square of the track's length.
    #[test]
    fn deriving_costs_one_walk_rather_than_a_point_per_frame_per_frame() {
        let frames: Vec<Vec<DrawCommand>> = (0..200).map(|step| vec![ship(1, step as f32, 0.0)]).collect();

        let derived = trails_through(&frames);
        let DrawCommand::PositionTrail { points, .. } = &derived[0] else { panic!("expected a trail") };
        assert_eq!(points.len(), 200);

        // Baked, every frame would have carried its own copy of the trail so
        // far, which is what this avoids.
        let baked_points: usize = (1..=200).sum();
        assert!(points.len() < baked_points / 100, "{} against {}", points.len(), baked_points);
    }
}

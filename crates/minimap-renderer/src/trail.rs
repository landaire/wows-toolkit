//! Position trails, read back out of frames already drawn or out of the
//! positions a bake recorded.
//!
//! Pure functions of their input, so a viewer with no replay parser (the web
//! demo) derives trails from the frames it has.

use std::collections::HashMap;

use wows_replays::types::EntityId;
use wows_replays::types::GameClock;

use crate::config::RenderOptions;
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

/// Where each ship has been across the frames walked so far.
///
/// A trail command carries every point so far, so baking one per frame would
/// cost the square of the track's length. The positions are already in the
/// track's own ship commands, so a viewer derives the trails for the frame it
/// is about to draw instead.
#[derive(Debug, Default)]
struct TrailHistory {
    order: Vec<EntityId>,
    ships: HashMap<EntityId, ShipHistory>,
    walked: usize,
}

impl TrailHistory {
    /// The history of every frame in `frames`.
    fn walk(frames: &[Vec<DrawCommand>]) -> Self {
        let mut history = Self::default();
        for commands in frames {
            history.push(commands);
        }

        history
    }

    /// Adds the next frame's ship positions.
    fn push(&mut self, commands: &[DrawCommand]) {
        let frame = self.walked;
        self.walked += 1;

        for command in commands {
            let (entity_id, pos, player_name, dead) = match command {
                DrawCommand::Ship { entity_id, pos, player_name, .. } => (entity_id, pos, player_name, false),
                // A sunk ship keeps the trail it made while it was alive.
                DrawCommand::DeadShip { entity_id, pos, player_name, .. } => (entity_id, pos, player_name, true),
                _ => continue,
            };
            let ship = self.ships.entry(*entity_id).or_insert_with(|| {
                self.order.push(*entity_id);
                ShipHistory::default()
            });
            if ship.player_name.is_none() {
                ship.player_name.clone_from(player_name);
            }
            ship.dead = dead;

            // Frames are evenly spaced in time, so distance per frame is a speed. A ship missing
            // from some frames is measured over all of them.
            let speed = match ship.last_seen {
                Some((last, seen_in)) => {
                    let frames_between = (frame - seen_in).max(1) as f32;
                    ((pos.x - last.x).powi(2) + (pos.y - last.y).powi(2)).sqrt() / frames_between
                }
                None => 0.0,
            };
            ship.last_seen = Some((*pos, frame));

            // A ship that has not moved since the last frame adds nothing.
            if ship.points.last().map(|last| (last.pos.x, last.pos.y)) != Some((pos.x, pos.y)) {
                ship.points.push(TrailPoint { pos: *pos, speed });
            }
        }
    }

    /// The trails `options` ask for, as of the last frame walked.
    ///
    /// Speed trails colour each point by how fast the ship was moving there against the fastest
    /// it went, and otherwise oldest points are blue and newest red, as the renderer's own
    /// trails are. A sunk ship's trail is left out unless dead-ship trails are on.
    fn trails(&self, options: &RenderOptions) -> Vec<DrawCommand> {
        if !options.show_trails && !options.show_speed_trails {
            return Vec::new();
        }

        self.order
            .iter()
            .filter_map(|entity_id| {
                let ship = self.ships.get(entity_id)?;
                if ship.dead && !options.show_dead_trails {
                    return None;
                }
                // One point is a position, not a trail.
                if ship.points.len() < 2 {
                    return None;
                }
                let points = if options.show_speed_trails { ship.by_speed() } else { ship.by_age() };
                Some(DrawCommand::PositionTrail {
                    entity_id: *entity_id,
                    player_name: ship.player_name.clone(),
                    points,
                })
            })
            .collect()
    }
}

/// One ship's part of a [`TrailHistory`].
#[derive(Debug, Default)]
struct ShipHistory {
    player_name: Option<String>,
    points: Vec<TrailPoint>,
    /// Where it was last drawn and in which frame, which the next speed is measured from.
    last_seen: Option<(MinimapPos, usize)>,
    /// Whether it was sunk in the latest frame it was drawn in.
    dead: bool,
}

impl ShipHistory {
    fn by_age(&self) -> Vec<(MinimapPos, [u8; 3])> {
        let last = self.points.len() - 1;
        self.points
            .iter()
            .enumerate()
            .map(|(index, point)| (point.pos, hue_to_rgb(240.0 * (1.0 - index as f32 / last as f32))))
            .collect()
    }

    fn by_speed(&self) -> Vec<(MinimapPos, [u8; 3])> {
        let fastest = self.points.iter().map(|point| point.speed).fold(0.0f32, f32::max);
        self.points
            .iter()
            .map(|point| {
                let fraction = if fastest > 0.0 { (point.speed / fastest).clamp(0.0, 1.0) } else { 0.0 };
                (point.pos, hue_to_rgb(240.0 * (1.0 - fraction)))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct TrailPoint {
    pos: MinimapPos,
    /// Map pixels moved per frame on the way here.
    speed: f32,
}

/// The trails `options` ask for, read back out of the frames already drawn.
pub fn trails_through(frames: &[Vec<DrawCommand>], options: &RenderOptions) -> Vec<DrawCommand> {
    if !options.show_trails && !options.show_speed_trails {
        return Vec::new();
    }

    TrailHistory::walk(frames).trails(options)
}

/// Every ship's position across a whole battle, at the resolution the egui renderer records it
/// (each packet step while the ship is visible and afloat), kept compactly for a baked track.
///
/// A track keeps a thinned set of frames, so trails read from those frames turn corners in
/// straight lines a minute long. These samples are recorded on the forward pass instead.
/// One sample is 12 bytes, and a ship keeps at most [`BakedTrails::MAX_SAMPLES_PER_SHIP`] of
/// them, so a ship's trail costs at most about 48 KiB however long the battle ran.
#[derive(Debug, Default, Clone)]
pub struct BakedTrails {
    ships: Vec<ShipTrail>,
}

impl BakedTrails {
    /// Most samples one ship keeps. A longer history keeps an evenly spaced subset that starts and
    /// ends where the recording did.
    pub const MAX_SAMPLES_PER_SHIP: usize = 4096;

    /// Compacts what a renderer recorded, `(position, clock, server speed)` per sample in the
    /// order recorded. `player_name` and `sunk_at` say whose each ship is and when it sank.
    pub fn from_history(
        history: HashMap<EntityId, Vec<(MinimapPos, GameClock, u16)>>,
        player_name: impl Fn(EntityId) -> Option<String>,
        sunk_at: impl Fn(EntityId) -> Option<GameClock>,
    ) -> Self {
        let mut ships: Vec<ShipTrail> = history
            .into_iter()
            .map(|(entity_id, samples)| {
                let samples = evenly_spaced(&samples, Self::MAX_SAMPLES_PER_SHIP)
                    .map(|(pos, clock, speed_raw)| TrailSample::new(*pos, *clock, *speed_raw))
                    .collect();

                ShipTrail { entity_id, player_name: player_name(entity_id), sunk_at: sunk_at(entity_id), samples }
            })
            .collect();
        // Recorded in a hash map; drawn in a fixed order so overlapping trails do not swap.
        ships.sort_by_key(|ship| ship.entity_id);

        Self { ships }
    }

    /// How many samples are kept across every ship.
    pub fn sample_count(&self) -> usize {
        self.ships.iter().map(|ship| ship.samples.len()).sum()
    }

    /// The trails `options` ask for, as they stood at `clock`.
    ///
    /// Colored as the egui renderer colors them: by age from blue to red, or for speed trails by
    /// the server speed against the fastest the ship went so far. A sunk ship's trail is left out
    /// unless dead-ship trails are on.
    pub fn trails_at(&self, clock: GameClock, options: &RenderOptions) -> Vec<DrawCommand> {
        if !options.show_trails && !options.show_speed_trails {
            return Vec::new();
        }

        self.ships
            .iter()
            .filter_map(|ship| {
                if !options.show_dead_trails && ship.sunk_at.is_some_and(|sunk| clock >= sunk) {
                    return None;
                }
                let seen = ship.samples.partition_point(|sample| sample.clock <= clock.seconds());
                let samples = &ship.samples[..seen];
                // One point is a position, not a trail.
                if samples.len() < 2 {
                    return None;
                }
                let points = if options.show_speed_trails { by_server_speed(samples) } else { by_sample_age(samples) };

                Some(DrawCommand::PositionTrail {
                    entity_id: ship.entity_id,
                    player_name: ship.player_name.clone(),
                    points,
                })
            })
            .collect()
    }
}

/// At most `most` of `items`, evenly spaced, keeping the first and the last.
fn evenly_spaced<T>(items: &[T], most: usize) -> impl Iterator<Item = &T> {
    let kept = items.len().min(most);
    let span = items.len().saturating_sub(1);
    (0..kept).map(move |index| match kept {
        1 => &items[0],
        _ => &items[index * span / (kept - 1)],
    })
}

/// One ship's part of a [`BakedTrails`].
#[derive(Debug, Clone)]
struct ShipTrail {
    entity_id: EntityId,
    player_name: Option<String>,
    sunk_at: Option<GameClock>,
    samples: Vec<TrailSample>,
}

/// One recorded position. The minimap position is held in sixteenths of a pixel, which is finer
/// than the quarter pixel a ship has to move before another sample is recorded.
#[derive(Debug, Clone, Copy)]
struct TrailSample {
    clock: f32,
    x: u16,
    y: u16,
    speed_raw: u16,
}

impl TrailSample {
    const SUBPIXELS: f32 = 16.0;

    fn new(pos: MinimapPos, clock: GameClock, speed_raw: u16) -> Self {
        // A ship stays on the map, so the clamp only guards the conversion.
        let fixed = |value: f32| (value * Self::SUBPIXELS).round().clamp(0.0, f32::from(u16::MAX)) as u16;

        Self { clock: clock.seconds(), x: fixed(pos.x), y: fixed(pos.y), speed_raw }
    }

    fn pos(self) -> MinimapPos {
        MinimapPos { x: f32::from(self.x) / Self::SUBPIXELS, y: f32::from(self.y) / Self::SUBPIXELS }
    }
}

fn by_sample_age(samples: &[TrailSample]) -> Vec<(MinimapPos, [u8; 3])> {
    let last = (samples.len() - 1) as f32;
    samples
        .iter()
        .enumerate()
        .map(|(index, sample)| (sample.pos(), hue_to_rgb(240.0 * (1.0 - index as f32 / last))))
        .collect()
}

fn by_server_speed(samples: &[TrailSample]) -> Vec<(MinimapPos, [u8; 3])> {
    let fastest = samples.iter().map(|sample| f32::from(sample.speed_raw)).fold(0.0f32, f32::max);
    samples
        .iter()
        .map(|sample| {
            let fraction = if fastest > 0.0 { (f32::from(sample.speed_raw) / fastest).clamp(0.0, 1.0) } else { 0.0 };
            (sample.pos(), hue_to_rgb(240.0 * (1.0 - fraction)))
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

    fn sunk(entity: u32, x: f32, y: f32) -> DrawCommand {
        DrawCommand::DeadShip {
            entity_id: EntityId::from(entity),
            pos: MinimapPos { x, y },
            yaw: 0.0,
            species: None,
            color: None,
            is_self: false,
            player_name: Some(format!("player{entity}")),
            ship_name: None,
        }
    }

    fn age_trails() -> RenderOptions {
        RenderOptions {
            show_trails: true,
            show_speed_trails: false,
            show_dead_trails: true,
            ..RenderOptions::default()
        }
    }

    fn colors(trail: &DrawCommand) -> Vec<[u8; 3]> {
        let DrawCommand::PositionTrail { points, .. } = trail else { panic!("expected a trail, got {trail:?}") };
        points.iter().map(|(_, color)| *color).collect()
    }

    /// A trail is where a ship has been across the frames behind this one.
    #[test]
    fn a_trail_follows_one_ship_through_the_frames() {
        let frames = vec![
            vec![ship(1, 0.0, 0.0), ship(2, 100.0, 100.0)],
            vec![ship(1, 10.0, 0.0), ship(2, 110.0, 100.0)],
            vec![ship(1, 20.0, 0.0)],
        ];

        let trails = trails_through(&frames, &age_trails());
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
        assert!(trails_through(&frames, &age_trails()).is_empty());
    }

    /// With neither kind of trail asked for there is nothing to derive.
    #[test]
    fn no_trail_is_derived_unless_one_is_asked_for() {
        let frames = vec![vec![ship(1, 0.0, 0.0)], vec![ship(1, 10.0, 0.0)]];
        let off = RenderOptions { show_trails: false, show_speed_trails: false, ..RenderOptions::default() };
        assert!(trails_through(&frames, &off).is_empty());
    }

    /// A speed trail is hottest where the ship moved fastest and coldest where it crawled,
    /// whatever order those came in.
    #[test]
    fn a_speed_trail_is_coloured_by_how_fast_the_ship_moved() {
        let frames =
            vec![vec![ship(1, 0.0, 0.0)], vec![ship(1, 30.0, 0.0)], vec![ship(1, 31.0, 0.0)], vec![ship(1, 32.0, 0.0)]];
        let speed = RenderOptions { show_speed_trails: true, ..age_trails() };

        let by_speed = colors(&trails_through(&frames, &speed)[0]);
        assert_eq!(by_speed[1], hue_to_rgb(0.0), "the fastest stretch is red");
        assert_eq!(by_speed[2], by_speed[3], "two equally slow stretches match");
        assert_ne!(by_speed[2], by_speed[1]);

        let by_age = colors(&trails_through(&frames, &age_trails())[0]);
        assert_eq!(by_age[3], hue_to_rgb(0.0), "an age trail is red at its newest point instead");
        assert_ne!(by_age, by_speed);
    }

    /// A sunk ship's trail stays only while dead-ship trails are on.
    #[test]
    fn a_sunk_ships_trail_follows_the_dead_trail_switch() {
        let frames = vec![vec![ship(1, 0.0, 0.0)], vec![ship(1, 10.0, 0.0)], vec![sunk(1, 10.0, 0.0)]];

        assert_eq!(trails_through(&frames, &age_trails()).len(), 1, "kept while the switch is on");
        let hidden = RenderOptions { show_dead_trails: false, ..age_trails() };
        assert!(trails_through(&frames, &hidden).is_empty(), "dropped once the ship is sunk");
        assert_eq!(trails_through(&frames[..2], &hidden).len(), 1, "but drawn while it was alive");
    }

    /// Deriving costs one walk of the frames behind, where baking a trail per
    /// frame would have cost the square of the track's length.
    #[test]
    fn deriving_costs_one_walk_rather_than_a_point_per_frame_per_frame() {
        let frames: Vec<Vec<DrawCommand>> = (0..200).map(|step| vec![ship(1, step as f32, 0.0)]).collect();

        let derived = trails_through(&frames, &age_trails());
        let DrawCommand::PositionTrail { points, .. } = &derived[0] else { panic!("expected a trail") };
        assert_eq!(points.len(), 200);

        // Baked, every frame would have carried its own copy of the trail so
        // far, which is what this avoids.
        let baked_points: usize = (1..=200).sum();
        assert!(points.len() < baked_points / 100, "{} against {}", points.len(), baked_points);
    }

    fn recorded(samples: &[(f32, f32, u16)]) -> Vec<(MinimapPos, GameClock, u16)> {
        samples.iter().map(|&(clock, x, speed)| (MinimapPos { x, y: 0.0 }, GameClock(clock), speed)).collect()
    }

    fn baked(samples: &[(f32, f32, u16)], sunk_at: Option<f32>) -> BakedTrails {
        let history = HashMap::from([(EntityId::from(1u32), recorded(samples))]);
        BakedTrails::from_history(history, |_| Some("player1".to_owned()), |_| sunk_at.map(GameClock))
    }

    fn xs(trail: &DrawCommand) -> Vec<f32> {
        let DrawCommand::PositionTrail { points, .. } = trail else { panic!("expected a trail, got {trail:?}") };
        points.iter().map(|(pos, _)| pos.x).collect()
    }

    /// A baked trail holds what was recorded up to the clock asked about, at sub-pixel precision.
    #[test]
    fn a_baked_trail_holds_what_was_recorded_up_to_the_clock() {
        let trails = baked(&[(1.0, 10.25, 0), (2.0, 20.5, 0), (3.0, 30.75, 0)], None);

        assert!(trails.trails_at(GameClock(1.5), &age_trails()).is_empty(), "one sample is not a trail");
        let trail = &trails.trails_at(GameClock(2.0), &age_trails())[0];
        assert_eq!(xs(trail), [10.25, 20.5]);
        assert_eq!(xs(&trails.trails_at(GameClock(9.0), &age_trails())[0]), [10.25, 20.5, 30.75]);
        let off = RenderOptions { show_trails: false, show_speed_trails: false, ..age_trails() };
        assert!(trails.trails_at(GameClock(9.0), &off).is_empty(), "nothing while trails are off");
    }

    /// A long history keeps a bounded, evenly spaced subset that still ends where the ship was last.
    #[test]
    fn a_long_history_is_thinned_to_the_per_ship_bound() {
        let samples: Vec<(f32, f32, u16)> = (0..20_000).map(|step| (step as f32, step as f32 / 16.0, 0)).collect();
        let trails = baked(&samples, None);

        assert_eq!(trails.sample_count(), BakedTrails::MAX_SAMPLES_PER_SHIP);
        let last = *xs(&trails.trails_at(GameClock(1e9), &age_trails())[0]).last().expect("points");
        assert_eq!(last, 1249.9375, "the newest sample is kept");
    }

    /// Twice the bound, the case where keeping every other sample and the last would overshoot.
    #[test]
    fn a_history_of_twice_the_bound_keeps_exactly_the_bound() {
        // A quarter pixel a step keeps every position inside what a sample can hold.
        let samples: Vec<(f32, f32, u16)> =
            (0..2 * BakedTrails::MAX_SAMPLES_PER_SHIP).map(|step| (step as f32, step as f32 / 4.0, 0)).collect();
        let trails = baked(&samples, None);

        assert_eq!(trails.sample_count(), BakedTrails::MAX_SAMPLES_PER_SHIP);
        let kept = xs(&trails.trails_at(GameClock(1e9), &age_trails())[0]);
        assert_eq!(kept.first(), Some(&0.0));
        assert_eq!(kept.last(), Some(&((2 * BakedTrails::MAX_SAMPLES_PER_SHIP - 1) as f32 / 4.0)));
    }

    /// A speed trail colours each point by the server speed recorded there against the fastest so
    /// far, as the egui renderer does.
    #[test]
    fn a_baked_speed_trail_is_coloured_by_server_speed() {
        let trails = baked(&[(1.0, 0.0, 0), (2.0, 10.0, 400), (3.0, 20.0, 200)], None);
        let speed = RenderOptions { show_speed_trails: true, ..age_trails() };

        let colours = colors(&trails.trails_at(GameClock(3.0), &speed)[0]);
        assert_eq!(colours, [hue_to_rgb(240.0), hue_to_rgb(0.0), hue_to_rgb(120.0)]);
    }

    /// A sunk ship's baked trail is drawn while it was afloat, and after only with dead-ship trails.
    #[test]
    fn a_baked_trail_of_a_sunk_ship_follows_the_dead_trail_switch() {
        let trails = baked(&[(1.0, 0.0, 0), (2.0, 10.0, 0)], Some(5.0));
        let hidden = RenderOptions { show_dead_trails: false, ..age_trails() };

        assert_eq!(trails.trails_at(GameClock(4.0), &hidden).len(), 1, "drawn while afloat");
        assert!(trails.trails_at(GameClock(5.0), &hidden).is_empty(), "dropped once sunk");
        assert_eq!(trails.trails_at(GameClock(5.0), &age_trails()).len(), 1, "kept with dead-ship trails on");
    }
}

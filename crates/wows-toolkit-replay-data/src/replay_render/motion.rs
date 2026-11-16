//! What moves on the map between two baked frames, and where it is part of the way.
//!
//! A bake keeps a frame every half second of game time or coarser, and playback and an export
//! draw many times between two of them. What changes continuously (ships and what is drawn at
//! them, planes, shells, torpedoes, smoke and capture progress) is taken the part of the way from
//! the earlier frame to the later one that the clock has reached, and the game timer counts on.
//! Everything else is drawn as the earlier frame has it, so a kill, a ribbon or a chat line shows
//! once the frame that recorded it is reached.

use std::collections::HashMap;

use wows_minimap_renderer::ShipVisibility;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::PlaneId;

use crate::replay_render::track::FrameIndex;
use crate::replay_render::track::Track;

/// How far a shell's tracer may lie from the line the other frame's tracer of it lies on, in
/// minimap pixels. A shell flies straight, so both lie on one line; a sibling of the same salvo
/// aimed elsewhere leaves it by more. Also how far a smoke puff may lie from itself.
const ON_LINE: f32 = 0.5;

/// How much a pixel off the line counts against a pairing of two tracers, measured against a pixel
/// of flight along it. The nearest line decides before the shortest flight does.
const OFF_LINE_COST: f32 = 100.0;

/// Torpedoes carry no identity, so one is paired with the nearest of its colour in the next frame
/// within this reach, in minimap pixels per second of game time.
const TORPEDO_REACH: f32 = 8.0;

/// Past this part of the way to the next frame, a shell, torpedo or smoke puff only one of the two
/// frames has is drawn as the later frame has it: one that landed is gone, one that was fired is
/// out. The frames do not say when in between either happened, and halfway is never more than
/// half the gap wrong.
const HALFWAY: f32 = 0.5;

/// How each command of a baked frame moves on the way to the next one, worked out once for every
/// draw between the two.
#[derive(Debug)]
pub struct FrameMotion {
    base: FrameIndex,

    /// The base frame's clock and the game time from it to the next frame, in seconds. No time on
    /// the last frame, or between frames a catch-up burst drew at one clock.
    from: f32,
    span: f32,

    /// One per command of the base frame.
    steps: Vec<Step>,
    /// Shells, torpedoes and smoke only the next frame has, ordered by where they go.
    arrivals: Vec<Arrival>,
}

impl FrameMotion {
    /// How frame `base` of `track` moves on the way to the frame after it. `None` past the end.
    pub fn between(track: &Track, base: FrameIndex) -> Option<Self> {
        let commands = track.commands(base)?;
        let from = track.seconds_at(base);
        let mut motion = Self { base, from, span: 0.0, steps: vec![Step::Stays; commands.len()], arrivals: Vec::new() };

        let following = FrameIndex::new(base.get() + 1);
        let Some(next) = track.commands(following) else { return Some(motion) };
        let span = track.seconds_at(following) - from;
        if span <= 0.0 {
            return Some(motion);
        }
        motion.span = span;

        motion.follow_ships(commands, next);
        motion.follow_planes(commands, next);
        motion.follow_tracers(commands, next);
        motion.follow_torpedoes(commands, next, span);
        motion.follow_smoke(commands, next);
        motion.follow_captures(commands, next);
        motion.arrivals.sort_by_key(|arrival| (arrival.before, arrival.index));

        Some(motion)
    }

    /// The frame this moves from.
    pub fn base(&self) -> FrameIndex {
        self.base
    }

    /// The base frame's commands that `keep` lets through, as they stand at `clock`.
    ///
    /// `clock` is held to the stretch between the base frame and the next, so a clock outside it
    /// draws the nearer end. Filtered before anything is copied, so a hidden layer costs nothing.
    pub fn commands_at<'a>(
        &'a self,
        track: &'a Track,
        clock: GameClock,
        keep: impl Fn(&DrawCommand) -> bool + Copy + 'a,
    ) -> impl Iterator<Item = DrawCommand> + 'a {
        // `between` read both frames, so neither is missing here; an empty slice draws nothing.
        let commands = track.commands(self.base).unwrap_or_default();
        let next = track.commands(FrameIndex::new(self.base.get() + 1)).unwrap_or_default();
        let past = (clock.seconds() - self.from).clamp(0.0, self.span);
        let fraction = if self.span > 0.0 { past / self.span } else { 0.0 };
        let arrived = fraction >= HALFWAY;

        (0..=commands.len()).flat_map(move |index| {
            let arrivals = if arrived { self.arrivals_before(index) } else { &[] };
            let arriving = arrivals
                .iter()
                .filter_map(move |arrival| next.get(arrival.index))
                .filter(move |command| keep(command))
                .cloned();
            let own = commands
                .get(index)
                .zip(self.steps.get(index))
                .filter(|(command, _)| keep(command))
                .and_then(move |(command, step)| placed(command, *step, fraction, past));
            arriving.chain(own)
        })
    }

    fn arrivals_before(&self, index: usize) -> &[Arrival] {
        let start = self.arrivals.partition_point(|arrival| arrival.before < index);
        let end = self.arrivals.partition_point(|arrival| arrival.before <= index);

        &self.arrivals[start..end]
    }

    /// Ships, and what is drawn at each: its health bar, ranges, consumables and camera line.
    fn follow_ships(&mut self, commands: &[DrawCommand], next: &[DrawCommand]) {
        let mut ahead: HashMap<EntityId, ShipPlace> = HashMap::new();
        let mut cameras_ahead: HashMap<EntityId, f32> = HashMap::new();
        for command in next {
            match command {
                DrawCommand::Ship { entity_id, pos, yaw, visibility, .. } => {
                    ahead.insert(*entity_id, ShipPlace { pos: *pos, yaw: *yaw, visibility: *visibility });
                }
                DrawCommand::CameraDirection { entity_id, yaw, .. } => {
                    cameras_ahead.insert(*entity_id, *yaw);
                }
                _ => {}
            }
        }

        let ships: HashMap<EntityId, Shift> = commands
            .iter()
            .filter_map(|command| {
                let DrawCommand::Ship { entity_id, pos, yaw, visibility, .. } = command else { return None };
                let place = ahead.get(entity_id)?;
                // A ship coming into or going out of sight jumps between where it was last seen
                // and where it is, which is not a path it sailed.
                if place.visibility != *visibility {
                    return None;
                }
                Some((*entity_id, Shift { turn: turn_between(*yaw, place.yaw), ..Shift::between(*pos, place.pos) }))
            })
            .collect();

        for (step, command) in self.steps.iter_mut().zip(commands) {
            let shift = match command {
                DrawCommand::Ship { entity_id, .. } => ships.get(entity_id).copied(),
                DrawCommand::CameraDirection { entity_id, yaw, .. } => ships.get(entity_id).map(|ship| {
                    // A camera line the next frame does not draw keeps its heading.
                    let turn = cameras_ahead.get(entity_id).map_or(0.0, |ahead| turn_between(*yaw, *ahead));
                    Shift { turn, ..*ship }
                }),
                DrawCommand::HealthBar { entity_id, .. }
                | DrawCommand::ConsumableRadius { entity_id, .. }
                | DrawCommand::ConsumableIcons { entity_id, .. }
                | DrawCommand::ShipConfigCircle { entity_id, .. } => {
                    ships.get(entity_id).map(|ship| Shift { turn: 0.0, ..*ship })
                }
                _ => None,
            };
            if let Some(shift) = shift {
                *step = Step::Moves(shift);
            }
        }
    }

    /// Planes, and the circle a fighter patrols, by the squadron each belongs to.
    fn follow_planes(&mut self, commands: &[DrawCommand], next: &[DrawCommand]) {
        let mut planes: HashMap<PlaneId, MinimapPos> = HashMap::new();
        let mut patrols: HashMap<PlaneId, MinimapPos> = HashMap::new();
        for command in next {
            match command {
                DrawCommand::Plane { plane_id, pos, .. } => {
                    planes.insert(*plane_id, *pos);
                }
                DrawCommand::PatrolRadius { plane_id, pos, .. } => {
                    patrols.insert(*plane_id, *pos);
                }
                _ => {}
            }
        }

        for (step, command) in self.steps.iter_mut().zip(commands) {
            let ahead = match command {
                DrawCommand::Plane { plane_id, pos, .. } => planes.get(plane_id).map(|ahead| (*pos, *ahead)),
                DrawCommand::PatrolRadius { plane_id, pos, .. } => patrols.get(plane_id).map(|ahead| (*pos, *ahead)),
                _ => None,
            };
            if let Some((pos, ahead)) = ahead {
                *step = Step::Moves(Shift::between(pos, ahead));
            }
        }
    }

    /// Shell tracers, each paired with the tracer on the same line further along it.
    fn follow_tracers(&mut self, commands: &[DrawCommand], next: &[DrawCommand]) {
        let here = tracers(commands);
        let ahead = tracers(next);
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (from, tracer) in here.iter().enumerate() {
            for (to, later) in ahead.iter().enumerate() {
                if tracer.secondary == later.secondary
                    && tracer.color == later.color
                    && let Some(cost) = same_shell(tracer, later)
                {
                    pairs.push((cost, from, to));
                }
            }
        }

        let (paired_here, paired_ahead) = pair_cheapest(pairs, here.len(), ahead.len());
        for (tracer, paired) in here.iter().zip(paired_here) {
            let step = match paired {
                Some(to) => {
                    let later = &ahead[to];
                    let head = Shift::between(tracer.head, later.head);
                    let tail = Shift::between(tracer.tail, later.tail);
                    Step::Moves(Shift { tail_dx: tail.dx, tail_dy: tail.dy, ..head })
                }
                None => Step::Leaves,
            };
            self.steps[tracer.index] = step;
            if let Some(tip) = tracer.tip {
                // The tip rides the head of its tracer.
                self.steps[tip] = match step {
                    Step::Moves(shift) => Step::Moves(Shift { tail_dx: 0.0, tail_dy: 0.0, ..shift }),
                    other => other,
                };
            }
        }

        let before = commands.partition_point(|command| Layer::of(command) <= Layer::Shells);
        for (later, paired) in ahead.iter().zip(paired_ahead) {
            if paired {
                continue;
            }
            self.arrivals.push(Arrival { before, index: later.index });
            if let Some(tip) = later.tip {
                self.arrivals.push(Arrival { before, index: tip });
            }
        }
    }

    /// Torpedoes, each paired with the nearest of its colour within reach in the next frame.
    fn follow_torpedoes(&mut self, commands: &[DrawCommand], next: &[DrawCommand], span: f32) {
        let here = torpedoes(commands);
        let ahead = torpedoes(next);
        let reach = TORPEDO_REACH * span;
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (from, torpedo) in here.iter().enumerate() {
            for (to, later) in ahead.iter().enumerate() {
                let distance = distance(torpedo.pos, later.pos);
                if torpedo.color == later.color && distance <= reach {
                    pairs.push((distance, from, to));
                }
            }
        }

        let (paired_here, paired_ahead) = pair_cheapest(pairs, here.len(), ahead.len());
        for (torpedo, paired) in here.iter().zip(paired_here) {
            self.steps[torpedo.index] = match paired {
                Some(to) => Step::Moves(Shift::between(torpedo.pos, ahead[to].pos)),
                None => Step::Leaves,
            };
        }

        let before = commands.partition_point(|command| Layer::of(command) <= Layer::Torpedoes);
        for (later, paired) in ahead.iter().zip(paired_ahead) {
            if !paired {
                self.arrivals.push(Arrival { before, index: later.index });
            }
        }
    }

    /// Smoke puffs, each paired with the one in the next frame where it lies, growing or thinning
    /// between the two.
    fn follow_smoke(&mut self, commands: &[DrawCommand], next: &[DrawCommand]) {
        let here = smoke(commands);
        let ahead = smoke(next);
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (from, puff) in here.iter().enumerate() {
            for (to, later) in ahead.iter().enumerate() {
                let distance = distance(puff.pos, later.pos);
                if puff.color == later.color && distance <= ON_LINE {
                    pairs.push((distance, from, to));
                }
            }
        }

        let (paired_here, paired_ahead) = pair_cheapest(pairs, here.len(), ahead.len());
        for (puff, paired) in here.iter().zip(paired_here) {
            self.steps[puff.index] = match paired {
                Some(to) => {
                    let later = &ahead[to];
                    Step::Moves(Shift {
                        grow: (later.radius - puff.radius) as f32,
                        fade: later.alpha - puff.alpha,
                        ..Shift::between(puff.pos, later.pos)
                    })
                }
                None => Step::Leaves,
            };
        }

        let before = commands.partition_point(|command| Layer::of(command) <= Layer::Smoke);
        for (later, paired) in ahead.iter().zip(paired_ahead) {
            if !paired {
                self.arrivals.push(Arrival { before, index: later.index });
            }
        }
    }

    /// Capture points, whose progress fills toward what the next frame shows while the same team
    /// holds the point and the same team is taking it.
    fn follow_captures(&mut self, commands: &[DrawCommand], next: &[DrawCommand]) {
        for (step, command) in self.steps.iter_mut().zip(commands) {
            let DrawCommand::CapturePoint { label, color, invader_color, progress, .. } = command else { continue };
            let ahead = next.iter().find_map(|later| match later {
                DrawCommand::CapturePoint {
                    label: later_label,
                    color: later_color,
                    invader_color: later_invader,
                    progress,
                    ..
                } if later_label == label && later_color == color && later_invader == invader_color => Some(*progress),
                _ => None,
            });
            if let Some(ahead) = ahead {
                *step = Step::Moves(Shift { fill: ahead - progress, ..Shift::default() });
            }
        }
    }
}

/// Ends each trail where its ship is drawn, part of the way to the next baked frame, so the trail
/// does not stop short of the ship.
pub fn reach_ships(trails: &mut [DrawCommand], frame: &[DrawCommand]) {
    let drawn: HashMap<EntityId, MinimapPos> = frame
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Ship { entity_id, pos, .. } => Some((*entity_id, *pos)),
            _ => None,
        })
        .collect();
    for trail in trails {
        let DrawCommand::PositionTrail { entity_id, points, .. } = trail else { continue };
        let (Some(pos), Some(&(last, color))) = (drawn.get(entity_id), points.last()) else { continue };
        if (last.x, last.y) != (pos.x, pos.y) {
            points.push((*pos, color));
        }
    }
}

/// What one command of the base frame does on the way to the next.
#[derive(Clone, Copy, Debug)]
enum Step {
    Stays,
    Moves(Shift),
    /// Gone by the next frame: a shell that landed, a torpedo that hit, smoke that cleared.
    Leaves,
}

/// How far a command moves by the next frame.
#[derive(Clone, Copy, Debug, Default)]
struct Shift {
    /// Of its position, or of a tracer's head.
    dx: f32,
    dy: f32,
    /// Of a tracer's tail.
    tail_dx: f32,
    tail_dy: f32,
    /// Of its heading, in radians.
    turn: f32,
    /// Of a smoke puff's radius in pixels, and its opacity.
    grow: f32,
    fade: f32,
    /// Of a capture point's progress.
    fill: f32,
}

impl Shift {
    fn between(from: MinimapPos, to: MinimapPos) -> Self {
        Self { dx: to.x - from.x, dy: to.y - from.y, ..Self::default() }
    }
}

/// A command only the next frame has, and where it goes among the base frame's.
#[derive(Clone, Copy, Debug)]
struct Arrival {
    /// Drawn before this command of the base frame, or after all of them at its length.
    before: usize,
    /// Its place in the next frame.
    index: usize,
}

/// Where a ship is drawn in a baked frame.
#[derive(Clone, Copy, Debug)]
struct ShipPlace {
    pos: MinimapPos,
    yaw: f32,
    visibility: ShipVisibility,
}

/// One shell's tracer in a frame, and the tip drawn at its head.
#[derive(Clone, Copy, Debug)]
struct Tracer {
    index: usize,
    tip: Option<usize>,
    secondary: bool,
    color: [u8; 3],
    tail: MinimapPos,
    head: MinimapPos,
}

/// One torpedo in a frame.
#[derive(Clone, Copy, Debug)]
struct Torpedo {
    index: usize,
    color: [u8; 3],
    pos: MinimapPos,
}

/// One smoke puff in a frame.
#[derive(Clone, Copy, Debug)]
struct Puff {
    index: usize,
    color: [u8; 3],
    pos: MinimapPos,
    radius: i32,
    alpha: f32,
}

/// Where a command falls in the order the renderer emits a frame in, as far as placing a shell,
/// torpedo or smoke puff that arrives between frames among the base frame's commands goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    BeforeShells,
    Shells,
    Torpedoes,
    Smoke,
    After,
}

impl Layer {
    fn of(command: &DrawCommand) -> Self {
        match command {
            DrawCommand::ScoreBar { .. }
            | DrawCommand::TeamAdvantage { .. }
            | DrawCommand::TeamBuffs { .. }
            | DrawCommand::CapturePoint { .. }
            | DrawCommand::BuffZone { .. }
            | DrawCommand::PositionTrail { .. } => Self::BeforeShells,
            DrawCommand::ShotTracer { .. }
            | DrawCommand::ShotTracerTip { .. }
            | DrawCommand::SecondaryShotTracer { .. }
            | DrawCommand::SecondaryShotTracerTip { .. } => Self::Shells,
            DrawCommand::Torpedo { .. } => Self::Torpedoes,
            DrawCommand::Smoke { .. } => Self::Smoke,
            _ => Self::After,
        }
    }
}

/// `command` as it stands `fraction` of the way to the next frame, `past` seconds after the base
/// frame. `None` when it is gone by then.
fn placed(command: &DrawCommand, step: Step, fraction: f32, past: f32) -> Option<DrawCommand> {
    let mut command = command.clone();
    match step {
        Step::Leaves if fraction >= HALFWAY => return None,
        Step::Moves(shift) => shift_by(&mut command, shift, fraction),
        Step::Stays | Step::Leaves => count_on(&mut command, past),
    }

    Some(command)
}

fn shift_by(command: &mut DrawCommand, shift: Shift, fraction: f32) {
    let nudge = |pos: &mut MinimapPos, dx: f32, dy: f32| {
        pos.x += dx * fraction;
        pos.y += dy * fraction;
    };
    match command {
        DrawCommand::Ship { pos, yaw, .. } | DrawCommand::CameraDirection { pos, yaw, .. } => {
            nudge(pos, shift.dx, shift.dy);
            *yaw += shift.turn * fraction;
        }
        DrawCommand::ShotTracer { from, to, .. } | DrawCommand::SecondaryShotTracer { from, to, .. } => {
            nudge(from, shift.tail_dx, shift.tail_dy);
            nudge(to, shift.dx, shift.dy);
        }
        DrawCommand::ShotTracerTip { at, .. } | DrawCommand::SecondaryShotTracerTip { at, .. } => {
            nudge(at, shift.dx, shift.dy);
        }
        DrawCommand::HealthBar { pos, .. }
        | DrawCommand::ConsumableRadius { pos, .. }
        | DrawCommand::ConsumableIcons { pos, .. }
        | DrawCommand::ShipConfigCircle { pos, .. }
        | DrawCommand::Plane { pos, .. }
        | DrawCommand::PatrolRadius { pos, .. }
        | DrawCommand::Torpedo { pos, .. } => nudge(pos, shift.dx, shift.dy),
        DrawCommand::Smoke { pos, radius, alpha, .. } => {
            nudge(pos, shift.dx, shift.dy);
            *radius += (shift.grow * fraction).round() as i32;
            *alpha += shift.fade * fraction;
        }
        DrawCommand::CapturePoint { progress, .. } => *progress += shift.fill * fraction,
        // Only the commands above are given a shift.
        _ => {}
    }
}

/// The game timer `past` seconds on from where the base frame read it.
fn count_on(command: &mut DrawCommand, past: f32) {
    let DrawCommand::Timer { time_remaining, elapsed } = command else { return };
    let counted = elapsed.seconds() + past;
    // The game's countdown loses a second each time its clock passes one.
    let ticks = (counted.floor() - elapsed.seconds().floor()) as i64;
    *time_remaining = time_remaining.map(|left| (left - ticks).max(0));
    *elapsed = ElapsedClock(counted);
}

/// Every shell tracer of a frame, with the tip drawn right after each.
fn tracers(commands: &[DrawCommand]) -> Vec<Tracer> {
    let mut found: Vec<Tracer> = Vec::new();
    for (index, command) in commands.iter().enumerate() {
        match command {
            DrawCommand::ShotTracer { from, to, color } | DrawCommand::SecondaryShotTracer { from, to, color } => {
                let secondary = matches!(command, DrawCommand::SecondaryShotTracer { .. });
                found.push(Tracer { index, tip: None, secondary, color: *color, tail: *from, head: *to });
            }
            DrawCommand::ShotTracerTip { .. } | DrawCommand::SecondaryShotTracerTip { .. } => {
                if let Some(tracer) = found.last_mut().filter(|tracer| tracer.index + 1 == index) {
                    tracer.tip = Some(index);
                }
            }
            _ => {}
        }
    }

    found
}

fn torpedoes(commands: &[DrawCommand]) -> Vec<Torpedo> {
    commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| match command {
            DrawCommand::Torpedo { pos, color } => Some(Torpedo { index, color: *color, pos: *pos }),
            _ => None,
        })
        .collect()
}

fn smoke(commands: &[DrawCommand]) -> Vec<Puff> {
    commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| match command {
            DrawCommand::Smoke { pos, radius, color, alpha } => {
                Some(Puff { index, color: *color, pos: *pos, radius: *radius, alpha: *alpha })
            }
            _ => None,
        })
        .collect()
}

/// What pairing `earlier` with `later` as one shell costs, or `None` when they cannot be one.
///
/// Both lie on the line the longer of the two fixes, and the later head is no further back along
/// it. The cost is how far the shell flew, with any distance off the line counted heavily, so of
/// two salvos on one line each tracer pairs with the next one ahead rather than the one after it.
fn same_shell(earlier: &Tracer, later: &Tracer) -> Option<f32> {
    let longer = if length(earlier) >= length(later) { earlier } else { later };
    let along = (longer.head.x - longer.tail.x, longer.head.y - longer.tail.y);
    let reach = (along.0 * along.0 + along.1 * along.1).sqrt();
    if reach <= f32::EPSILON {
        // Two tracers not yet drawn out: one shell if they start at one muzzle.
        let apart = distance(earlier.head, later.head);
        return (apart <= ON_LINE).then_some(apart * OFF_LINE_COST);
    }
    let unit = (along.0 / reach, along.1 / reach);
    let off_line = |point: MinimapPos| ((point.x - longer.tail.x) * unit.1 - (point.y - longer.tail.y) * unit.0).abs();
    let off = [earlier.tail, earlier.head, later.tail, later.head].map(off_line);
    if off.iter().any(|off| *off > ON_LINE) {
        return None;
    }
    // Both point the way the shell flies: a tracer heading back down the line is another shell.
    let pointing =
        |tracer: &Tracer| (tracer.head.x - tracer.tail.x) * unit.0 + (tracer.head.y - tracer.tail.y) * unit.1;
    let flown = (later.head.x - earlier.head.x) * unit.0 + (later.head.y - earlier.head.y) * unit.1;
    if flown < -ON_LINE || pointing(earlier) < 0.0 || pointing(later) < 0.0 {
        return None;
    }

    Some(flown.max(0.0) + off.iter().sum::<f32>() * OFF_LINE_COST)
}

/// The cheapest one-to-one pairing of `pairs`, taken greedily. Says, for each of the earlier
/// frame's items, which of the later frame's it paired with, and for each of the later frame's
/// whether any did.
fn pair_cheapest(mut pairs: Vec<(f32, usize, usize)>, earlier: usize, later: usize) -> (Vec<Option<usize>>, Vec<bool>) {
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut paired_earlier = vec![None; earlier];
    let mut paired_later = vec![false; later];
    for (_, from, to) in pairs {
        if paired_earlier[from].is_some() || paired_later[to] {
            continue;
        }
        paired_earlier[from] = Some(to);
        paired_later[to] = true;
    }

    (paired_earlier, paired_later)
}

fn length(tracer: &Tracer) -> f32 {
    distance(tracer.tail, tracer.head)
}

fn distance(a: MinimapPos, b: MinimapPos) -> f32 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

/// The shortest turn from heading `from` to heading `to`, in radians.
fn turn_between(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

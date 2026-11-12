//! What happened in the battle, as the transport steps between it and the
//! timeline lists it.

use std::collections::HashMap;

use wows_replay_insights::timeline::EventTone;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineExtractionResult;
use wows_replay_insights::timeline::TimelineFilter;
use wows_replay_insights::timeline::format_timeline_event;
use wows_replay_insights::timeline::row_text;
use wows_replay_insights::timeline::row_tone;
use wows_replays::types::EntityId;
use wowsunpack::game_types::TeamId;

/// One row of the timeline, already worded and sided.
#[derive(Clone, Debug)]
pub struct TimelineRow {
    /// Where in the battle it happened, in seconds.
    pub at: f32,
    pub label: String,
    pub hover: String,
    pub tone: EventTone,
}

/// A battle's events and the hits each ship took, read by a second walk of
/// the replay, and which of the events the timeline shows.
pub struct BattleEvents {
    /// Whether the second walk has finished, which is what the timeline says
    /// instead of an empty list while it runs.
    read: bool,
    events: Vec<TimelineEvent>,
    /// Which side the reader was on, which is what makes a row friendly.
    viewer_team: Option<TeamId>,
    /// What each ship took, keyed by the entity the frame's own ship commands
    /// carry, so a ship the reader points at is looked up directly.
    shots: HashMap<EntityId, ShipShotTimeline>,
    /// Which of the events the timeline shows.
    pub filter: TimelineFilter,
}

impl BattleEvents {
    /// Nothing read yet. Empty, which is what leaves the event controls
    /// refused.
    pub fn new() -> Self {
        Self {
            read: false,
            events: Vec::new(),
            viewer_team: None,
            shots: HashMap::new(),
            filter: TimelineFilter::default(),
        }
    }

    /// What the second walk read.
    pub fn from_read(
        events: Vec<TimelineEvent>,
        viewer_team: Option<TeamId>,
        shots: HashMap<EntityId, ShipShotTimeline>,
    ) -> Self {
        Self { read: true, events, viewer_team, shots, filter: TimelineFilter::default() }
    }

    /// What [`crate::minimap_preview::extract_events`] returns.
    pub fn from_extraction(read: TimelineExtractionResult, shots: HashMap<EntityId, ShipShotTimeline>) -> Self {
        Self::from_read(read.events, read.viewer_team, shots)
    }

    pub fn is_read(&self) -> bool {
        self.read
    }

    /// Every event, oldest first.
    pub fn events(&self) -> &[TimelineEvent] {
        &self.events
    }

    pub fn viewer_team(&self) -> Option<TeamId> {
        self.viewer_team
    }

    pub fn shots(&self) -> &HashMap<EntityId, ShipShotTimeline> {
        &self.shots
    }

    /// The events the filter admits, newest last.
    pub fn visible(&self) -> Vec<&TimelineEvent> {
        self.events.iter().filter(|event| self.filter.matches(event)).collect()
    }

    /// The rows the timeline lists.
    pub fn rows(&self) -> Vec<TimelineRow> {
        self.visible()
            .into_iter()
            .map(|event| {
                let (label, hover) = row_text(&event.kind);
                TimelineRow { at: event.clock.seconds(), label, hover, tone: row_tone(&event.kind, self.viewer_team) }
            })
            .collect()
    }

    /// The listed events as text, one per line, for the clipboard.
    pub fn copy_text(&self) -> String {
        self.visible().into_iter().map(format_timeline_event).collect::<Vec<_>>().join("\n")
    }

    /// The event before `elapsed` seconds into the battle, if there is one.
    ///
    /// Half a second back, as the egui renderer looks: without it, landing on
    /// an event and pressing back again would find the same one.
    pub fn previous(&self, elapsed: f32) -> Option<&TimelineEvent> {
        self.events.iter().rev().find(|event| event.clock.seconds() < elapsed - 0.5)
    }

    /// The event after `elapsed` seconds into the battle, if there is one.
    pub fn next(&self, elapsed: f32) -> Option<&TimelineEvent> {
        self.events.iter().find(|event| event.clock.seconds() > elapsed)
    }
}

impl Default for BattleEvents {
    fn default() -> Self {
        Self::new()
    }
}

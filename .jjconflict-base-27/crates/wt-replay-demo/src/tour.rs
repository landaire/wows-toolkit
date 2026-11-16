//! A short scripted tour that runs when the visitor leaves the demo alone, so
//! a passive viewer sees the frame is drawn live rather than played as video.

use crate::playback::Speed;

/// How long the demo waits without input before touring.
const IDLE_SECONDS: f64 = 8.0;

/// A change the tour makes, as a visitor could have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TourAction {
    /// Sets the layer switch with this [`wows_minimap_renderer::toggles::Toggle::id`].
    Toggle {
        id: &'static str,
        on: bool,
    },
    SeekBack {
        game_seconds: f32,
    },
    SetSpeed(Speed),
    /// Puts back the switches and speed the visitor had before the tour.
    Restore,
}

#[derive(Debug, PartialEq)]
pub struct TourStep {
    /// Seconds after the tour starts.
    pub at: f64,
    pub action: TourAction,
    pub caption: &'static str,
}

const STEPS: &[TourStep] = &[
    TourStep { at: 0.0, action: TourAction::Toggle { id: "renderer-opt-trails", on: true }, caption: "Heat trails on" },
    TourStep {
        at: 3.0,
        action: TourAction::Toggle { id: "renderer-opt-player-names", on: true },
        caption: "Player names on",
    },
    TourStep { at: 6.0, action: TourAction::SeekBack { game_seconds: 30.0 }, caption: "Back 30 seconds" },
    TourStep { at: 9.0, action: TourAction::SetSpeed(Speed::new(5)), caption: "Speed 5x" },
    TourStep { at: 13.0, action: TourAction::SetSpeed(Speed::new(40)), caption: "Speed 40x" },
    TourStep { at: 16.0, action: TourAction::Restore, caption: "Back to your settings" },
];

/// Times are seconds on the UI clock (`egui::InputState::time`).
pub struct Tour {
    idle_since: f64,
    running: Option<Run>,
}

struct Run {
    started: f64,
    /// The next step to perform.
    next: usize,
}

impl Tour {
    pub fn new(now: f64) -> Self {
        Self { idle_since: now, running: None }
    }

    /// The visitor did something: any running tour stops where it is.
    pub fn interrupt(&mut self, now: f64) {
        self.idle_since = now;
        self.running = None;
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The caption of the step last performed while a tour runs.
    pub fn caption(&self) -> Option<&'static str> {
        let run = self.running.as_ref()?;

        run.next.checked_sub(1).map(|step| STEPS[step].caption)
    }

    /// The steps due by `now`, starting a tour once the demo has been idle long
    /// enough. A finished tour counts as idle time starting over.
    pub fn due(&mut self, now: f64) -> Vec<&'static TourStep> {
        if self.running.is_none() && now - self.idle_since >= IDLE_SECONDS {
            self.running = Some(Run { started: now, next: 0 });
        }
        let Some(run) = self.running.as_mut() else {
            return Vec::new();
        };

        let first = run.next;
        while run.next < STEPS.len() && STEPS[run.next].at <= now - run.started {
            run.next += 1;
        }
        let due = STEPS[first..run.next].iter().collect();

        if run.next == STEPS.len() {
            self.running = None;
            self.idle_since = now;
        }

        due
    }

    /// Seconds until [`Self::due`] next has something to do.
    pub fn until_next(&self, now: f64) -> f64 {
        match &self.running {
            Some(run) => STEPS.get(run.next).map_or(0.0, |step| run.started + step.at - now),
            None => self.idle_since + IDLE_SECONDS - now,
        }
        .max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_happens_before_the_demo_has_been_idle() {
        let mut tour = Tour::new(0.0);

        assert!(tour.due(7.9).is_empty());
        assert!(!tour.is_running());
        assert!((tour.until_next(7.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_idle_demo_tours_every_step_then_waits_again() {
        let mut tour = Tour::new(0.0);

        let first = tour.due(8.0);
        assert_eq!(first, vec![&STEPS[0]]);
        assert_eq!(tour.caption(), Some(STEPS[0].caption));

        let rest = tour.due(8.0 + 60.0);
        assert_eq!(rest.len(), STEPS.len() - 1);
        assert_eq!(rest.last().map(|step| step.action), Some(TourAction::Restore));
        assert!(!tour.is_running());

        assert!(tour.due(68.0 + 7.0).is_empty(), "the next tour waits out another idle period");
    }

    #[test]
    fn input_stops_the_tour_where_it_is() {
        let mut tour = Tour::new(0.0);
        tour.due(8.0);

        tour.interrupt(9.0);

        assert!(!tour.is_running());
        assert_eq!(tour.caption(), None);
        assert!(tour.due(16.9).is_empty());
    }
}

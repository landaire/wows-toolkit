//! When a hovered row has been watched long enough to be worth previewing.
//!
//! Baking a minimap preview reads and parses a replay, so it is not something
//! to start for every row the pointer crosses on its way somewhere else. Both
//! front ends wait for the pointer to settle first, and they wait the same
//! amount: a preview that appeared after a different delay in each app would
//! be a different feature in each.
//!
//! Pure on purpose. The front end reports hovers and leaves as its own event
//! model produces them, and asks what to do; nothing here draws or measures a
//! frame.

use std::time::Duration;

/// How long the pointer has to settle on one row before its preview is worth
/// baking.
pub const DWELL: Duration = Duration::from_millis(300);

/// Tracks how long the pointer has continuously dwelled on one row.
///
/// `K` is whatever identifies a row to the caller; the egui app keys on the
/// replay's path and mtime, so a file rewritten under the same name is a
/// different preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dwell<K> {
    /// The row currently under the pointer and how long it has been watched
    /// there.
    watched: Option<(K, Duration)>,
}

/// Derived `Default` would demand `K: Default`, which a row key has no
/// reason to be: nothing is watched to begin with either way.
impl<K> Default for Dwell<K> {
    fn default() -> Self {
        Self { watched: None }
    }
}

impl<K: Clone + PartialEq> Dwell<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `key` was under the pointer for another `elapsed` of wall
    /// time.
    ///
    /// A key different from the one already being watched replaces it rather
    /// than adding to it: the dwell belongs to whichever row is currently
    /// under the pointer, not to a sum across rows visited earlier.
    pub fn hover(&mut self, key: K, elapsed: Duration) {
        match &mut self.watched {
            Some((watched, dwelled)) if *watched == key => *dwelled += elapsed,
            _ => self.watched = Some((key, elapsed)),
        }
    }

    /// The row a preview should be requested for, once the pointer has
    /// settled on it for at least [`DWELL`].
    pub fn pending_request(&self) -> Option<K> {
        let (key, dwelled) = self.watched.as_ref()?;
        (*dwelled >= DWELL).then(|| key.clone())
    }

    /// The pointer left the rows this frame, so nothing is dwelling any more
    /// and whatever was pending is cancelled.
    pub fn leave(&mut self) {
        self.watched = None;
    }

    /// Whether anything is being watched at all, dwelled or not.
    pub fn is_watching(&self) -> bool {
        self.watched.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::DWELL;
    use super::Dwell;
    use std::time::Duration;

    const HALF: Duration = Duration::from_millis(150);

    #[test]
    fn a_row_is_not_previewed_until_the_pointer_settles() {
        let mut dwell = Dwell::new();
        assert_eq!(dwell.pending_request(), None, "nothing is hovered yet");

        dwell.hover("a", HALF);
        assert_eq!(dwell.pending_request(), None, "half the dwell is not enough");

        dwell.hover("a", HALF);
        assert_eq!(dwell.pending_request(), Some("a"), "the rest of it is");
    }

    /// The pointer crossing a row on its way to another must not hand the
    /// second row the first one's dwell.
    #[test]
    fn moving_to_another_row_starts_its_dwell_over() {
        let mut dwell = Dwell::new();
        dwell.hover("a", HALF);
        dwell.hover("b", HALF);

        assert_eq!(dwell.pending_request(), None, "the second row starts from nothing");

        dwell.hover("b", HALF);
        assert_eq!(dwell.pending_request(), Some("b"));
    }

    #[test]
    fn leaving_cancels_what_was_pending() {
        let mut dwell = Dwell::new();
        dwell.hover("a", DWELL);
        assert!(dwell.pending_request().is_some());

        dwell.leave();
        assert_eq!(dwell.pending_request(), None);
        assert!(!dwell.is_watching());
    }

    /// Returning to a row after leaving it waits again rather than firing on
    /// the first frame.
    #[test]
    fn coming_back_to_a_row_waits_again() {
        let mut dwell = Dwell::new();
        dwell.hover("a", DWELL);
        dwell.leave();

        dwell.hover("a", HALF);
        assert_eq!(dwell.pending_request(), None);
    }

    /// Exactly the dwell counts, so a front end reporting one frame of
    /// precisely that length is not made to wait for a second.
    #[test]
    fn the_boundary_itself_counts() {
        let mut dwell = Dwell::new();
        dwell.hover("a", DWELL);
        assert_eq!(dwell.pending_request(), Some("a"));
    }
}

//! Announces each battle written to the replay index, so a view that read the index or the
//! tracker can read again.

use std::sync::LazyLock;

use tokio::sync::watch;

/// Counts the battles written to the index in this process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordingRevision(u64);

/// Process-wide: every path a battle enters the index by goes through one hook, and the views
/// that read the index are not reachable from it.
static RECORDED: LazyLock<watch::Sender<RecordingRevision>> =
    LazyLock::new(|| watch::Sender::new(RecordingRevision::default()));

/// A receiver that wakes on each battle recorded after this call. Runtime-agnostic, so a UI
/// executor can await it.
pub fn subscribe() -> watch::Receiver<RecordingRevision> {
    RECORDED.subscribe()
}

/// Notes one battle written to the index.
pub(crate) fn note_recorded() {
    // `send_modify` rather than `send`: it updates the value even while nobody is subscribed.
    RECORDED.send_modify(|revision| revision.0 = revision.0.wrapping_add(1));
}

/// Announces a battle as the index does, for the tests of views that follow it.
#[cfg(any(test, feature = "test-support"))]
pub fn announce_for_test() {
    note_recorded();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_battle_wakes_a_subscriber() {
        let mut receiver = subscribe();
        let before = *receiver.borrow_and_update();

        note_recorded();

        assert!(receiver.has_changed().expect("the sender lives for the process"));
        assert_ne!(*receiver.borrow_and_update(), before);
    }
}

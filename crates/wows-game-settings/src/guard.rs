use crate::error::GameRunning;

/// The caller's view of whether the game is running.
pub trait GameActivity {
    /// Keeps the game from starting while it lives, for example a lock the
    /// launcher must take to start the game.
    type Hold<'state>
    where
        Self: 'state;

    /// `None` while the game is starting, running or in a battle.
    fn hold_stopped(&self) -> Option<Self::Hold<'_>>;
}

/// Proof that the game is stopped, holding whatever keeps it from starting
/// until the save that consumes the proof finishes.
#[derive(Debug)]
pub struct GameStopped<H> {
    _hold: H,
}

impl<H> GameStopped<H> {
    pub fn check<'state, S>(state: &'state S) -> Result<GameStopped<H>, GameRunning>
    where
        S: GameActivity<Hold<'state> = H> + ?Sized,
    {
        let Some(hold) = state.hold_stopped() else {
            return Err(GameRunning);
        };

        Ok(GameStopped { _hold: hold })
    }
}

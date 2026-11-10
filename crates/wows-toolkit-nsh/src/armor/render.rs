//! The thread that drives one pane: it reads the pane surface's input, ticks
//! held-key motion and the gizmo snap, renders when the scene is dirty and
//! presents the frame.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use crate::frames::PixelSize;
use crate::frames::Surface;
use crate::frames::SurfaceInput;

use super::frame;
use super::input::InputMapper;
use super::input::SceneInput;
use super::state;
use super::state::PaneId;

/// What a pane's render thread is told.
#[derive(Debug)]
pub(super) enum Signal {
    Input(SurfaceInput),
    /// Something changed the scene from outside: look again.
    Wake,
    Stop,
}

/// Starts the threads that drive pane `id` and returns where to signal them.
pub(super) fn spawn(id: PaneId, surface: Arc<Surface>) -> mpsc::Sender<Signal> {
    let (sender, signals) = mpsc::channel();

    match surface.take_input() {
        Some(mut input) => {
            let forward = sender.clone();
            state::spawn_named("armor-pane-input", move || {
                while let Some(event) = input.blocking_recv() {
                    if forward.send(Signal::Input(event)).is_err() {
                        return;
                    }
                }
            });
        }
        None => tracing::warn!("armor viewer: pane {id} surface input is already being read"),
    }

    state::spawn_named("armor-pane-render", move || run(id, &surface, &signals));

    sender
}

fn run(id: PaneId, surface: &Surface, signals: &mpsc::Receiver<Signal>) {
    const TICK: Duration = Duration::from_millis(16);
    // A stall longer than this is not motion the reader asked for.
    const LONGEST_STEP: Duration = Duration::from_millis(100);

    let mut mapper = InputMapper::new();
    let mut view: Option<(PixelSize, f32)> = None;
    let mut rendered: Option<(PixelSize, f32)> = None;
    let mut last_tick: Option<Instant> = None;

    loop {
        let first = match last_tick {
            Some(_) => match signals.recv_timeout(TICK) {
                Ok(signal) => Some(signal),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match signals.recv() {
                Ok(signal) => Some(signal),
                Err(mpsc::RecvError) => return,
            },
        };

        let now = Instant::now();
        let mut inputs = Vec::new();
        for signal in first.into_iter().chain(signals.try_iter()) {
            match signal {
                Signal::Stop => return,
                Signal::Wake => {}
                Signal::Input(event) => match mapper.map(event, now) {
                    Some(SceneInput::Resize { size, scale }) => view = Some((size, scale)),
                    Some(input) => inputs.push(input),
                    None => {}
                },
            }
        }

        let step = last_tick.map(|at| now.saturating_duration_since(at).min(LONGEST_STEP));
        let resized = view != rendered;
        let Some(driven) = state::drive(id, inputs, view, step, resized) else { return };

        last_tick = driven.wants_tick.then_some(now);
        let Some(readback) = driven.readback else { continue };
        rendered = view;
        let scale = view.map(|(_, scale)| scale).unwrap_or(1.0);
        match frame::compose(readback, driven.gizmo.as_ref(), scale) {
            Ok(frame) => surface.present(frame),
            Err(error) => tracing::warn!("armor viewer: pane {id} rendered an unusable frame: {error}"),
        }
    }
}

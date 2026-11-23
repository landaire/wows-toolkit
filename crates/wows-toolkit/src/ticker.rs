//! A callback at a fixed rate, told how much time really passed since the last one.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::view::FrameHandle;
use zgui::view::IntervalHandle;
use zgui::view::Timers;
use zgui::vocab::Timestamp;

/// The most one tick reports, so a window that stalled does not leap ahead by the stall.
const LONGEST_TICK: Duration = Duration::from_millis(250);

/// Calls back every `every`, with the time since the last call read off the window's clock.
///
/// An interval re-arms from when it last ran, so a window busy for a moment runs it late and a
/// count of ticks falls behind the clock. Each tick instead measures from the timestamp of the
/// frame it runs in, so a late tick reports the longer gap. Dropping this stops it.
pub struct Ticker {
    _interval: IntervalHandle,
    _frame: Rc<RefCell<Option<FrameHandle>>>,
}

impl Ticker {
    pub fn start(timers: &Timers, every: Duration, on_tick: impl Fn(Duration) + 'static) -> Self {
        let on_tick: Rc<dyn Fn(Duration)> = Rc::new(on_tick);
        let last: Rc<Cell<Option<Timestamp>>> = Rc::new(Cell::new(None));
        let frame: Rc<RefCell<Option<FrameHandle>>> = Rc::new(RefCell::new(None));
        let pending = Rc::clone(&frame);
        let requesting = timers.clone();

        let interval = timers.set_interval(every, move || {
            let on_tick = Rc::clone(&on_tick);
            let last = Rc::clone(&last);
            let requested = requesting.request_frame(move |now| {
                // The first tick has no earlier frame to measure from, and was asked for `every` after
                // the start.
                let elapsed = last.replace(Some(now)).map_or(every, |last| now.saturating_since(last));
                on_tick(elapsed.min(LONGEST_TICK));
            });
            *pending.borrow_mut() = Some(requested);
        });

        Self { _interval: interval, _frame: frame }
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view::FrameHandle;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;

use crate::motion::MotionPreference;
use crate::motion::Movement;
use crate::motion::motion_preference;

/// A width in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct PanelWidth(pub f32);

/// The edge of its container a sliding panel is attached to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SideEdge {
    #[default]
    Left,
    Right,
}

impl SideEdge {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Where a surface that slides in and out is in its movement.
///
/// A surface entering is built hidden and shown one frame later, so the style engine has a hidden
/// style to move from; one leaving stays built until its movement has had time to finish.
#[derive(Clone, Copy)]
pub(crate) struct Slide {
    /// Whether the surface is built.
    pub(crate) mounted: Signal<bool, LocalStorage>,
    /// Whether the surface is at, or moving towards, its open place.
    pub(crate) shown: Signal<bool, LocalStorage>,
    /// Whether the last movement has finished.
    pub(crate) settled: Signal<bool, LocalStorage>,
}

impl Slide {
    /// Follows `open`, timed as `movement` under the enclosing motion preference. Called from a
    /// component's body. The first value is adopted without moving, and a movement under way when
    /// motion is reduced ends where it was going.
    pub(crate) fn follow(open: Signal<bool, LocalStorage>, movement: Movement) -> Self {
        let preference = motion_preference();
        let timers = Timers::current();
        let initially = open.get_untracked();
        let mounted = RwSignal::new_local(initially);
        let shown = RwSignal::new_local(initially);
        let settled = RwSignal::new_local(true);

        let frame: Rc<RefCell<Option<FrameHandle>>> = Rc::new(RefCell::new(None));
        let settling: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));

        let finish = {
            let frame = Rc::clone(&frame);
            let settling = Rc::clone(&settling);
            move |open: bool| {
                frame.borrow_mut().take();
                settling.borrow_mut().take();
                mounted.set(open);
                shown.set(open);
                settled.set(true);
            }
        };

        let following = {
            let frame = Rc::clone(&frame);
            let settling = Rc::clone(&settling);
            RenderEffect::new(move |previous: Option<Followed>| {
                let now = Followed { open: open.get(), preference: preference.get() };
                let Some(previous) = previous else {
                    return now;
                };

                let reduced = now.preference == MotionPreference::Reduced;
                if previous.open == now.open {
                    if reduced && !settled.get_untracked() {
                        finish(now.open);
                    }
                    return now;
                }

                let Some(timers) = timers.as_ref().filter(|_| !reduced) else {
                    finish(now.open);
                    return now;
                };

                frame.borrow_mut().take();
                settling.borrow_mut().take();
                settled.set(false);
                let open = now.open;
                let duration = movement.full_duration();
                let settle = {
                    let timers = timers.clone();
                    move || {
                        timers.set_timeout(duration, move || {
                            settled.set(true);
                            if !open {
                                mounted.set(false);
                            }
                        })
                    }
                };

                if open && !mounted.get_untracked() {
                    mounted.set(true);
                    let settling = Rc::clone(&settling);
                    *frame.borrow_mut() = Some(timers.request_frame(move |_| {
                        shown.set(true);
                        *settling.borrow_mut() = Some(settle());
                    }));
                } else {
                    shown.set(open);
                    *settling.borrow_mut() = Some(settle());
                }

                now
            })
        };

        on_cleanup_local(move || {
            drop(following);
            frame.borrow_mut().take();
            settling.borrow_mut().take();
        });

        Self { mounted: mounted.into(), shown: shown.into(), settled: settled.into() }
    }
}

/// What a movement that follows an open state last saw.
#[derive(Clone, Copy)]
pub(crate) struct Followed {
    pub(crate) open: bool,
    pub(crate) preference: MotionPreference,
}

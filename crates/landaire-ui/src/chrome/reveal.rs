use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::ChildrenFn;
use zgui::view::Classes;
use zgui::view::FrameHandle;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;
use zgui::view::install_stylesheet;

use crate::motion::Movement;
use crate::motion::motion_preference;

const SHEET: &str = "landaire-ui-reveal";

/// Fades an element in once, as it is first built.
pub const FADE_IN_CLASS: &str = "lui-fade-in";

const CSS: &str = css!(
    ".lui-reveal {
        display: block;
        overflow: hidden;
        height: 0;
        transition: height var(--lui-motion-reveal) var(--lui-motion-ease);
    }
    .lui-reveal[data-phase=\"opening\"] {
        height: var(--lui-reveal-height, 0px);
    }
    .lui-reveal[data-phase=\"holding\"] {
        height: var(--lui-reveal-height, 0px);
        transition: none;
    }
    .lui-reveal[data-phase=\"open\"] {
        height: auto;
        transition: none;
    }
    .lui-reveal__content {
        display: flex;
        flex-direction: column;
    }
    .lui-fade-in {
        animation: lui-fade-in var(--lui-motion-fade) var(--lui-motion-ease) both;
    }
    .lui-fade-in-box {
        display: flex;
        min-width: 0;
    }
    @keyframes lui-fade-in {
        from { opacity: 0; }
        to { opacity: 1; }
    }"
);

/// Where a [`Reveal`] is between shut and open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Nothing built, no height.
    Closed,
    /// Growing to the content's measured height.
    Opening,
    /// At whatever height the content takes.
    Open,
    /// At the measured height without moving, the frame before a close: the style engine cannot
    /// move from `auto`.
    Holding,
    /// Shrinking to nothing.
    Closing,
}

impl Phase {
    const fn name(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Opening => "opening",
            Self::Open => "open",
            Self::Holding => "holding",
            Self::Closing => "closing",
        }
    }
}

/// A block of content that opens to its own height and closes to nothing, clipped as it moves.
///
/// The content is laid out at its full height throughout, so it wraps the same while moving as at
/// rest. Once open it stays built at whatever height it takes; once shut it is dropped.
#[component]
pub fn Reveal(
    #[prop(into)] open: Signal<bool, LocalStorage>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let preference = motion_preference();
    let timers = Timers::current();
    let phase = RwSignal::new_local(if open.get_untracked() { Phase::Open } else { Phase::Closed });
    let frame: Rc<RefCell<Option<FrameHandle>>> = Rc::new(RefCell::new(None));
    let settling: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));

    let following = {
        let frame = Rc::clone(&frame);
        let settling = Rc::clone(&settling);
        RenderEffect::new(move |previous: Option<bool>| {
            let open = open.get();
            if previous.is_none_or(|previous| previous == open) {
                return open;
            }

            frame.borrow_mut().take();
            settling.borrow_mut().take();
            let Some(timers) = timers.as_ref() else {
                phase.set(if open { Phase::Open } else { Phase::Closed });
                return open;
            };

            let duration = Movement::Reveal.duration(preference.get_untracked());
            if open {
                phase.set(Phase::Opening);
                // A frame more than the movement: content built by this open is measured a frame
                // after it is built, and its movement starts then.
                let after = duration + Duration::from_millis(17);
                *settling.borrow_mut() = Some(timers.set_timeout(after, move || phase.set(Phase::Open)));
                return open;
            }

            let close = {
                let timers = timers.clone();
                move || {
                    phase.set(Phase::Closing);
                    timers.set_timeout(duration, move || phase.set(Phase::Closed))
                }
            };
            if phase.get_untracked() == Phase::Open {
                phase.set(Phase::Holding);
                let settling = Rc::clone(&settling);
                *frame.borrow_mut() = Some(timers.request_frame(move |_| *settling.borrow_mut() = Some(close())));
            } else {
                *settling.borrow_mut() = Some(close());
            }

            open
        })
    };

    on_cleanup_local(move || {
        drop(following);
        frame.borrow_mut().take();
        settling.borrow_mut().take();
    });

    let content = NodeRef::new();
    let measured = content.observe_border_size();
    let height = move || {
        let scale = content.scale();
        let scale = if scale > 0.0 { scale } else { 1.0 };
        // Rounded up, so a hairline at the foot of the content stays inside the clip.
        measured.get().map(|size| format!("{}px", (size.height.0 / scale).ceil()))
    };
    let children = StoredValue::new_local(children);
    let own = Attrs::new().a11y_from(A11yBinding::unspecified().hidden(move || !open.get()));

    view! {
        box(
            class = "lui-reveal",
            attr:data-phase = move || Some(phase.get().name().to_owned()),
            var:--lui-reveal-height = height,
            {..own},
            {..attrs},
            class = class
        ) {
            column(class = "lui-reveal__content", node_ref = content) {
                if move || phase.get() != Phase::Closed {
                    {children.get_value().view()}
                } else {}
            }
        }
    }
}

/// Fades its content in once, as it is first built. [`FADE_IN_CLASS`] does the same to an
/// element of the caller's own.
#[component]
pub fn FadeIn(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        box(class = "lui-fade-in lui-fade-in-box", {..attrs}, class = class) {
            {children.into_view_once()}
        }
    }
}

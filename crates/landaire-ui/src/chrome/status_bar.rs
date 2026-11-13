use std::cell::RefCell;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Update;
use zgui::slot;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::Classes;
use zgui::view::IntervalHandle;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::Timers;
use zgui::view::install_stylesheet;
use zgui::vocab::Role;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;

use crate::motion::MotionPreference;
use crate::motion::motion_preference;
use crate::tooltip::TooltipContentProps;
use crate::tooltip::TooltipProps;
use crate::tooltip::TooltipTriggerProps;

const SHEET: &str = "landaire-ui-status-bar";

const CSS: &str = css!(
    ".lui-status-bar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
        min-height: 26px;
        padding: 0 8px;
        border-top: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-surface);
        color: var(--lui-color-text-dim);
        font-family: var(--lui-font-family);
        font-size: 12px;
        white-space: nowrap;
    }
    .lui-status-bar__start, .lui-status-bar__end {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 4px;
    }
    .lui-status-bar__middle {
        display: flex;
        flex-direction: row;
        flex: 1 1 auto;
        align-items: center;
        gap: 4px;
        min-width: 0;
        overflow: hidden;
    }
    .lui-status-bar .lui-status-toggle[data-size] {
        height: 20px;
        min-width: 20px;
        padding: 0;
        border-radius: var(--lui-radius);
        color: var(--lui-color-text-dim);
    }
    .lui-status-bar .lui-status-toggle[data-size]:hover {
        background-color: var(--lui-color-widget-hot);
        color: var(--lui-color-text);
    }
    .lui-status-bar .lui-status-toggle[data-size]:checked {
        background-color: var(--lui-color-selection);
        color: var(--lui-color-text-bright);
    }
    .lui-running-bar {
        position: relative;
        flex: none;
        width: 64px;
        height: 4px;
        overflow: hidden;
        border-radius: var(--lui-radius);
        background-color: color-mix(in srgb, var(--lui-color-accent) 20%, transparent);
    }
    .lui-running-bar__fill {
        position: absolute;
        top: 0;
        bottom: 0;
        left: 0;
        overflow: hidden;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-accent);
        transition: width var(--zui-motion-duration-normal) var(--lui-motion-ease);
    }
    .lui-running-bar__sheen {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 40%;
        background-image: linear-gradient(
            90deg,
            transparent,
            color-mix(in srgb, var(--lui-color-accent-foreground) 22%, transparent),
            transparent
        );
    }"
);

/// The control group at a [`StatusBar`]'s start, such as the toggles for panels on that side.
#[slot]
pub struct StatusBarStart {
    children: Children,
}

/// The control group at a [`StatusBar`]'s end, such as what is running.
#[slot]
pub struct StatusBarEnd {
    children: Children,
}

/// A share of some work that is done, from nothing to all of it.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Fraction(f32);

impl Fraction {
    pub const NONE: Self = Self(0.0);
    pub const WHOLE: Self = Self(1.0);

    /// `None` outside `0.0..=1.0`.
    pub fn new(value: f32) -> Option<Self> {
        (0.0..=1.0).contains(&value).then_some(Self(value))
    }

    /// `done` of `total`. Work reported past its total is all done.
    pub fn of(done: u64, total: NonZeroU64) -> Self {
        Self(done.min(total.get()) as f32 / total.get() as f32)
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// The strip along the bottom of a window: a group of controls at each end and whatever is
/// between them, truncated rather than wrapped when it runs out of room.
#[component(slot_aware)]
pub fn StatusBar(
    #[prop(optional)] status_bar_start: Option<StatusBarStart>,
    #[prop(optional)] status_bar_end: Option<StatusBarEnd>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let start = status_bar_start.map(|start| start.children.into_view_once());
    let end = status_bar_end.map(|end| end.children.into_view_once());

    view! {
        row(class = "lui-status-bar", {..attrs}, class = class) {
            row(class = "lui-status-bar__start") {{start}}
            row(class = "lui-status-bar__middle") {{children.into_view_once()}}
            row(class = "lui-status-bar__end") {{end}}
        }
    }
}

/// A compact button in a [`StatusBar`] that stays pressed while what it controls is on, such as
/// a side panel being open. `label` names it for a screen reader and in its tooltip.
#[component]
pub fn StatusToggle(
    pressed: RwSignal<bool, LocalStorage>,
    icon: IconData,
    #[prop(into)] label: String,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let tip = label.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                Toggle(pressed = pressed, size = ToggleSize::Sm, label = label, class = "lui-status-toggle", {..attrs}) {
                    Icon(icon = icon, size = IconSize::Sm)
                }
            }
            TooltipContent {{tip.clone()}}
        }
    }
}

/// A small bar showing how much of some running work is done. A faint sheen crosses the filled
/// part until the work is finished, at a few frames a second since it runs as long as the work.
#[component]
pub fn RunningBar(
    #[prop(into)] done: Signal<Fraction, LocalStorage>,
    /// What the work is called, for a screen reader.
    #[prop(into)]
    label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    const FRAME: Duration = Duration::from_millis(50);

    install_stylesheet(SHEET, CSS);

    let preference = motion_preference();
    let running = move || done.get() < Fraction::WHOLE && preference.get() == MotionPreference::Full;
    let ticks = RwSignal::new_local(0_u32);
    let timers = Timers::current();
    let held: Rc<RefCell<Option<IntervalHandle>>> = Rc::new(RefCell::new(None));
    let ticking = {
        let held = Rc::clone(&held);
        RenderEffect::new(move |_| {
            if !running() {
                held.borrow_mut().take();
                return;
            }

            if held.borrow().is_some() {
                return;
            }

            *held.borrow_mut() =
                timers.as_ref().map(|timers| timers.set_interval(FRAME, move || ticks.update(|ticks| *ticks += 1)));
        })
    };
    on_cleanup_local(move || {
        drop(ticking);
        held.borrow_mut().take();
    });

    let percent = move || done.get().get() * 100.0;
    let semantics = A11yBinding::new(Role::ProgressIndicator)
        .label(label)
        .numeric_value(move || f64::from(percent()))
        .step(|a11y| a11y.numeric_range(0.0, 100.0));
    let own = Attrs::new().a11y_from(semantics);
    let sheen_left = move || {
        let elapsed = FRAME * ticks.get();
        Some(format!("{}%", sheen_position(elapsed)))
    };

    view! {
        box(class = "lui-running-bar", {..own}, {..attrs}, class = class) {
            box(class = "lui-running-bar__fill", style:width = move || Some(format!("{}%", percent()))) {
                Show(when = running) {
                    box(class = "lui-running-bar__sheen", style:left = sheen_left)
                }
            }
        }
    }
}

/// Where the sheen's left edge is, as a percentage of the filled part, `elapsed` into its loop.
/// It crosses in the first half of each cycle and rests out of sight for the second.
fn sheen_position(elapsed: Duration) -> f32 {
    const CYCLE: Duration = Duration::from_millis(2400);
    const CROSSING: f32 = 0.5;
    const WIDTH: f32 = 40.0;

    let phase = (elapsed.as_millis() % CYCLE.as_millis()) as f32 / CYCLE.as_millis() as f32;
    let travel = (phase / CROSSING).min(1.0);
    let eased = travel * travel * (3.0 - 2.0 * travel);

    -WIDTH + eased * (100.0 + WIDTH)
}

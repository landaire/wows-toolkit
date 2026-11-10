//! The strip along the bottom of the window: the active tab's side panel
//! toggles on the left, and what is running on the right.
//!
//! The egui app's status panel (`app.rs`'s `build_bottom_panel`) names every
//! background task. Here one task is named outright; several are counted, and
//! a hover lists them.

use std::num::NonZeroU64;
use std::rc::Rc;

use gpui_kit::AnyElement;
use gpui_kit::AnyWeakEntity;
use gpui_kit::App;
use gpui_kit::ElementId;
use gpui_kit::Entity;
use gpui_kit::EntityId;
use gpui_kit::Global;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement as _;
use gpui_kit::ParentElement as _;
use gpui_kit::SharedString;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use rust_i18n::t;

use crate::icons::Glyph;
use crate::ui::PanelSide;

/// What a toggle does when clicked.
pub type Toggle = Rc<dyn Fn(&mut Window, &mut App)>;

/// One side panel the active tab can show or put away.
pub struct PanelToggle {
    pub id: SharedString,
    /// What the panel is called, for the button's hover.
    pub name: SharedString,
    pub side: PanelSide,
    pub open: bool,
    pub toggle: Toggle,
}

impl PanelToggle {
    /// A toggle for each side dock of `dock` that holds something.
    ///
    /// The docks are named by side: a panel can be dragged from one to
    /// another, so what a dock holds is not a stable name for it.
    pub fn for_docks(id: &str, dock: &Entity<DockArea>, cx: &App) -> Vec<Self> {
        let area = dock.read(cx);
        [
            (DockPlacement::Left, PanelSide::Left, t!("ui.app.panel_left")),
            (DockPlacement::Right, PanelSide::Right, t!("ui.app.panel_right")),
            (DockPlacement::Bottom, PanelSide::Bottom, t!("ui.app.panel_bottom")),
        ]
        .into_iter()
        .filter(|(placement, _, _)| !area.is_empty(*placement, cx))
        .map(|(placement, side, name)| {
            let dock = dock.downgrade();
            Self {
                id: SharedString::from(format!("{id}-{}", side.id_fragment())),
                name: name.into_owned().into(),
                side,
                open: area.is_dock_open(placement),
                toggle: Rc::new(move |window, cx| {
                    let _ = dock.update(cx, |area, cx| area.toggle_dock(placement, window, cx));
                }),
            }
        })
        .collect()
    }

    fn render(self) -> AnyElement {
        let glyph = match self.side {
            PanelSide::Left => Glyph::PanelLeft,
            PanelSide::Right => Glyph::PanelRight,
            PanelSide::Bottom => Glyph::PanelBottom,
        };
        let said = if self.open {
            t!("ui.app.status_hide_panel", panel = self.name)
        } else {
            t!("ui.app.status_show_panel", panel = self.name)
        };
        let toggle = self.toggle;
        Button::new(ElementId::Name(self.id))
            .icon(glyph)
            .ghost()
            .xsmall()
            .selected(self.open)
            .tooltip(said.into_owned())
            .on_click(move |_event, window, cx| toggle(window, cx))
            .into_any_element()
    }
}

/// How far a job has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: NonZeroU64,
}

impl Progress {
    /// `None` for a total of zero: nothing is known yet about how far along it is.
    pub fn new(done: u64, total: u64) -> Option<Self> {
        Some(Self { done, total: NonZeroU64::new(total)? })
    }
}

/// One piece of background work.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub name: SharedString,
    pub progress: Option<Progress>,
}

impl Job {
    fn said(&self) -> String {
        match self.progress {
            Some(Progress { done, total }) => {
                format!("{} {}", self.name, t!("ui.app.status_of", done = done, total = total))
            }
            None => self.name.to_string(),
        }
    }
}

/// Distinguishes runs of one kind that a single owner can have going at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunNumber(pub u64);

/// What a reported job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    UpdateCheck,
    UpdateDownload,
    BatchRender(RunNumber),
    Contribute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JobKey {
    owner: EntityId,
    kind: JobKind,
}

struct Entry {
    key: JobKey,
    /// Work whose owner is gone is not shown: a task dropped with its owner
    /// never reports [`finished`].
    owner: AnyWeakEntity,
    job: Job,
}

/// Work started from somewhere that does not own the window's status bar,
/// keyed so a later report replaces the earlier one.
#[derive(Default)]
struct Running(Vec<Entry>);

impl Global for Running {}

/// Reports `owner`'s `kind` of work as running under `name`, replacing what it
/// said before.
pub fn working(
    owner: AnyWeakEntity,
    kind: JobKind,
    name: impl Into<SharedString>,
    progress: Option<Progress>,
    cx: &mut App,
) {
    let key = JobKey { owner: owner.entity_id(), kind };
    let job = Job { name: name.into(), progress };
    let running = &mut cx.default_global::<Running>().0;
    running.retain(|entry| entry.owner.is_upgradable());
    match running.iter_mut().find(|entry| entry.key == key) {
        Some(entry) if entry.job == job => return,
        Some(entry) => entry.job = job,
        None => running.push(Entry { key, owner, job }),
    }
    cx.refresh_windows();
}

/// Takes `owner`'s `kind` of work off the status bar. Nothing happens for work
/// that is not running.
pub fn finished(owner: EntityId, kind: JobKind, cx: &mut App) {
    let key = JobKey { owner, kind };
    let running = &mut cx.default_global::<Running>().0;
    let before = running.len();
    running.retain(|entry| entry.key != key && entry.owner.is_upgradable());
    if running.len() != before {
        cx.refresh_windows();
    }
}

/// The work reported through [`working`] whose owner is still alive.
pub fn running(cx: &App) -> Vec<Job> {
    let Some(running) = cx.try_global::<Running>() else {
        return Vec::new();
    };

    running.0.iter().filter(|entry| entry.owner.is_upgradable()).map(|entry| entry.job.clone()).collect()
}

/// The bar: `toggles` on the left, then `extras`, then `jobs` on the right.
pub fn render(toggles: Vec<PanelToggle>, extras: Vec<AnyElement>, jobs: Vec<Job>, cx: &App) -> AnyElement {
    h_flex()
        .id("app-status-strip")
        .test_support()
        .flex_none()
        .h(px(26.))
        .gap_1()
        .items_center()
        .px_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .bg(crate::theme::surface())
        .text_xs()
        .text_color(crate::theme::text_dim())
        .children(toggles.into_iter().map(PanelToggle::render))
        .children(extras)
        .child(div().flex_1())
        .children(work(jobs))
        .into_any_element()
}

/// One job by name; several by count, listed on hover.
fn work(jobs: Vec<Job>) -> Option<AnyElement> {
    let said = match jobs.as_slice() {
        [] => return None,
        [only] => only.said(),
        _ => t!("ui.app.status_tasks", count = jobs.len()).into_owned(),
    };
    let several = jobs.len() > 1;
    let listed: Vec<String> = jobs.iter().map(Job::said).collect();
    Some(
        h_flex()
            .id("app-status-work")
            .test_support()
            .aria_label(said.clone())
            .gap_1()
            .items_center()
            .min_w_0()
            .child(Spinner::new().xsmall())
            .child(div().min_w_0().truncate().child(said))
            .when(several, |this| {
                this.tooltip(move |window, cx| {
                    let listed = listed.clone();
                    Tooltip::element(move |_window, _cx| {
                        v_flex().gap_1().children(listed.iter().map(|line| {
                            h_flex().gap_1().items_center().child(Spinner::new().xsmall()).child(line.clone())
                        }))
                    })
                    .build(window, cx)
                })
            })
            .into_any_element(),
    )
}

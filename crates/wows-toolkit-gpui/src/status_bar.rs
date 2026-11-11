//! The strip along the bottom of the window: the active tab's side panel
//! toggles on the left, the mode a held modifier key puts the active tab in,
//! and what is running on the right.
//!
//! The egui app's status panel (`app.rs`'s `build_bottom_panel`) names every
//! background task. Here one task is named outright; several are counted, and
//! a hover lists them.

use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::AnyElement;
use gpui_kit::AnyWeakEntity;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::ElementId;
use gpui_kit::Entity;
use gpui_kit::EntityId;
use gpui_kit::Global;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::Modifiers;
use gpui_kit::ParentElement as _;
use gpui_kit::Pixels;
use gpui_kit::RenderOnce;
use gpui_kit::SharedString;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Subscription;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::Transition;
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
    /// A chosen replay directory being indexed into its own source.
    FolderIndex,
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

/// A modifier key that switches a view to an alternate reading while held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierKey {
    Alt,
    /// Ctrl, or Cmd on macOS.
    Secondary,
    Shift,
}

impl ModifierKey {
    /// The key as the platform's keyboard labels it.
    pub fn label(self, cx: &App) -> &'static str {
        let mac = cx.platform_info().is_mac();
        match self {
            Self::Alt if mac => "Option",
            Self::Alt => "Alt",
            Self::Secondary if mac => "Cmd",
            Self::Secondary => "Ctrl",
            Self::Shift => "Shift",
        }
    }

    fn is_down(self, modifiers: &Modifiers, cx: &App) -> bool {
        match self {
            Self::Alt => modifiers.alt,
            Self::Secondary => modifiers.secondary(cx),
            Self::Shift => modifiers.shift,
        }
    }
}

/// What a view shows while `key` is held, named for the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeHint {
    pub key: ModifierKey,
    /// Translation key of the mode's name, translated where the bar is drawn
    /// so a view reporting every frame does not translate every frame.
    pub name_key: &'static str,
}

struct ModeEntry {
    owner: AnyWeakEntity,
    mode: ModeHint,
}

/// The modes views have reported. Cleared when the active tab changes, as a
/// hidden tab's views stay alive but no longer render to take theirs back.
#[derive(Default)]
struct Modes(Vec<ModeEntry>);

impl Global for Modes {}

/// Whether `key` is held in `window`. The window keeps its modifier state
/// whichever element has focus, and a view that reads this while rendering
/// renders again when it changes. An inactive window never sees the release
/// of a key held as it lost focus, so it counts nothing as held.
pub fn is_held(key: ModifierKey, window: &Window, cx: &App) -> bool {
    window.is_window_active() && key.is_down(&window.modifiers(), cx)
}

/// Reports the mode `owner` is in, or that it is in none. Called from the
/// owner's render; nothing happens when the report is unchanged.
pub fn set_mode(owner: AnyWeakEntity, mode: Option<ModeHint>, cx: &mut App) {
    let id = owner.entity_id();
    let reported = cx
        .try_global::<Modes>()
        .and_then(|modes| modes.0.iter().find(|entry| entry.owner.entity_id() == id))
        .map(|entry| entry.mode);
    if reported == mode {
        return;
    }

    let modes = &mut cx.default_global::<Modes>().0;
    modes.retain(|entry| entry.owner.entity_id() != id && entry.owner.is_upgradable());
    if let Some(mode) = mode {
        modes.push(ModeEntry { owner, mode });
    }
}

/// Drops every reported mode. The views now on screen report theirs again as
/// they render.
pub fn clear_modes(cx: &mut App) {
    if cx.try_global::<Modes>().is_some_and(|modes| !modes.0.is_empty()) {
        cx.set_global(Modes::default());
    }
}

/// Renders the view that draws the bar again whenever a mode is reported.
/// Views report while they render, after the bar's own render has read the
/// modes.
pub fn observe_modes<V: 'static>(cx: &mut Context<V>) -> Subscription {
    cx.observe_global::<Modes>(|_view, cx| cx.notify())
}

/// The mode to show: the latest one reported whose key is held and whose
/// owner is still alive.
fn shown_mode(window: &Window, cx: &App) -> Option<ModeHint> {
    cx.try_global::<Modes>()?
        .0
        .iter()
        .rev()
        .find(|entry| entry.owner.is_upgradable() && is_held(entry.mode.key, window, cx))
        .map(|entry| entry.mode)
}

/// The bar: the toggles for left and bottom panels on the left, then
/// `extras` and the held-modifier mode; `jobs`, then the toggles for right
/// panels, at the right end, so each toggle sits on the side of the panel it
/// opens.
pub fn render(
    toggles: Vec<PanelToggle>,
    extras: Vec<AnyElement>,
    jobs: Vec<Job>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let (right, near): (Vec<PanelToggle>, Vec<PanelToggle>) =
        toggles.into_iter().partition(|toggle| toggle.side == PanelSide::Right);
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
        .children(near.into_iter().map(PanelToggle::render))
        .children(extras)
        .children(shown_mode(window, cx).map(|mode| mode_indicator(mode, cx)))
        .child(div().flex_1())
        .children(work(jobs))
        .children(right.into_iter().map(PanelToggle::render))
        .into_any_element()
}

/// The held key as a key cap, then the name of the mode it selects.
fn mode_indicator(mode: ModeHint, cx: &App) -> AnyElement {
    let key = mode.key.label(cx);
    let name = t!(mode.name_key);
    h_flex()
        .id("app-status-mode")
        .test_support()
        .aria_label(format!("{key}: {name}"))
        .gap_1()
        .items_center()
        .min_w_0()
        .child(
            div()
                .flex_none()
                .px_1()
                .rounded(px(3.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .text_color(cx.theme().foreground)
                .child(key),
        )
        .child(div().min_w_0().truncate().child(name.into_owned()))
        .into_any_element()
}

/// One job by name; several by count, listed on hover.
fn work(jobs: Vec<Job>) -> Option<AnyElement> {
    let said = match jobs.as_slice() {
        [] => return None,
        [only] => only.said(),
        _ => t!("ui.app.status_tasks", count = jobs.len()).into_owned(),
    };
    let shown = match jobs.as_slice() {
        [only] => JobLine { job: only.clone() }.into_any_element(),
        _ => crate::ui::tabular(div().min_w_0().truncate()).child(said.clone()).into_any_element(),
    };
    let several = jobs.len() > 1;
    let listed: Vec<String> = jobs.iter().map(Job::said).collect();
    let indicator =
        h_flex()
            .id("app-status-work")
            .test_support()
            .aria_label(said)
            .gap_1()
            .items_center()
            .min_w_0()
            .child(Spinner::new().xsmall())
            .child(shown)
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
            });
    Some(crate::motion::fade_in("app-status-work-fade", indicator))
}

/// A job by name, and for one that counts, how far it has got beside a bar
/// of the share done. The count rises to each new figure rather than jumping
/// to it; the bar follows the count.
#[derive(IntoElement)]
struct JobLine {
    job: Job,
}

impl RenderOnce for JobLine {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        const BAR_WIDTH: Pixels = px(64.);
        const BAR_HEIGHT: Pixels = px(4.);

        let line = crate::ui::tabular(div().min_w_0().truncate());
        let Some(progress) = self.job.progress else {
            return line.child(self.job.name).into_any_element();
        };

        let done = ticked(progress, window, cx);
        let total = progress.total.get();
        let said = format!("{} {}", self.job.name, t!("ui.app.status_of", done = done, total = total));
        let percent = done.min(total) as f32 / total as f32 * 100.;
        h_flex()
            .gap_2()
            .items_center()
            .min_w_0()
            .child(line.child(said))
            .child(
                div()
                    .flex_none()
                    .w(BAR_WIDTH)
                    .child(crate::loading::RunningBar::new("app-status-work-bar", percent).height(BAR_HEIGHT)),
            )
            .into_any_element()
    }
}

/// The finished count to show for `progress`, rising to it over a short
/// time from the count shown before.
fn ticked(progress: Progress, window: &mut Window, cx: &mut App) -> u64 {
    const TICK: Duration = Duration::from_millis(400);

    // Keyed by the total, so a new run starts at its own count rather than
    // counting from the last run's.
    let key = ElementId::NamedInteger("app-status-work-done".into(), progress.total.get());
    let shown = gpui_kit::base::transition((key, "done"), progress.done as f32, Transition::new(TICK), window, cx);

    // Floored, so a figure is not shown before it is reached.
    (shown.floor() as u64).min(progress.done)
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::Modifiers;
    use gpui_kit::ModifiersChangedEvent;
    use gpui_kit::PlatformInput;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::test::TestWindowExt as _;

    use super::*;

    const MODE: &str = "app-status-mode";

    fn press(modifiers: Modifiers, window: &mut Window, cx: &mut App) {
        window.dispatch_event(
            PlatformInput::ModifiersChanged(ModifiersChangedEvent { modifiers, ..Default::default() }),
            cx,
        );
        window.render_frame(cx);
    }

    /// The app, in a window that has focus: an inactive one holds no keys.
    fn open_active_app(
        cx: &mut TestAppContext,
    ) -> (gpui_kit::WindowHandle<gpui_kit::component::Root>, gpui_kit::Entity<crate::app::App>) {
        let (window, app) = crate::interaction_tests::open_app_in_root(cx);
        cx.update_window(window.into(), |_, window, _cx| window.activate_window()).expect("the test window stays open");
        cx.run_until_parked();
        (window, app)
    }

    const ALT_MODE: ModeHint = ModeHint { key: ModifierKey::Alt, name_key: "ui.replay.damage_share.mode" };

    fn shown(window: &Window) -> bool {
        window.try_find(MODE).is_some()
    }

    /// The bar names the mode a view reports while its key is down, and
    /// drops it when the key comes up.
    #[gpui_kit::test]
    fn a_held_modifier_names_its_mode_in_the_status_bar(cx: &mut TestAppContext) {
        let (window, app) = open_active_app(cx);
        let owner: AnyWeakEntity = app.downgrade().into();

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!shown(window), "no mode before a key is held");

            press(Modifiers::alt(), window, cx);
            assert!(is_held(ModifierKey::Alt, window, cx));
            set_mode(owner.clone(), Some(ALT_MODE), cx);
            window.render_frame(cx);
            let expected = format!("{}: {}", ModifierKey::Alt.label(cx), t!(ALT_MODE.name_key));
            assert_eq!(window.find(MODE).label(), Some(expected.as_str()));

            press(Modifiers::none(), window, cx);
            assert!(!is_held(ModifierKey::Alt, window, cx));
            assert!(!shown(window), "the mode goes with the key");

            // A mode reported for another key than the one held is not shown.
            press(Modifiers::shift(), window, cx);
            set_mode(owner, Some(ALT_MODE), cx);
            window.render_frame(cx);
            assert!(!shown(window), "shift does not select an alt mode");
        })
        .expect("the test window stays open");
    }

    /// A key let go while a dialog has focus is let go: the dialog sits
    /// outside the app view, where none of its listeners hear the release.
    #[gpui_kit::test]
    fn a_key_released_over_a_dialog_takes_its_mode_with_it(cx: &mut TestAppContext) {
        let (window, app) = open_active_app(cx);
        let owner: AnyWeakEntity = app.downgrade().into();

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            press(Modifiers::alt(), window, cx);
            set_mode(owner, Some(ALT_MODE), cx);
            window.render_frame(cx);
            assert!(shown(window), "the mode shows while alt is held");

            app.update(cx, |app, cx| app.open_palette(window, cx));
            window.render_frame(cx);
        })
        .expect("the test window stays open");
        cx.run_until_parked();

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("command").is_some(), "the palette is up and holds focus");
            press(Modifiers::none(), window, cx);
            assert!(!shown(window), "the release over the dialog clears the mode");
        })
        .expect("the test window stays open");
    }

    /// A hidden tab's view stays alive, so what it reported must not follow
    /// the reader to the next tab while the key is still down.
    #[gpui_kit::test]
    fn a_mode_stays_with_the_tab_that_reported_it(cx: &mut TestAppContext) {
        let (window, app) = open_active_app(cx);
        let owner = cx.new(|_cx| ());

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            press(Modifiers::alt(), window, cx);
            set_mode(owner.downgrade().into(), Some(ALT_MODE), cx);
            window.render_frame(cx);
            assert!(shown(window), "the mode shows on the tab that reported it");

            app.update(cx, |app, cx| {
                app.run_palette_action(crate::palette::PaletteAction::GoTo(crate::app::AppTab::Stats), window, cx);
            });
            window.render_frame(cx);
            assert!(is_held(ModifierKey::Alt, window, cx), "alt is still down");
            assert!(!shown(window), "the other tab does not show it");
        })
        .expect("the test window stays open");
    }
}

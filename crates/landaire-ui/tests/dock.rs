//! A dock in a real document: tabs pressed and closed, regions folded, layouts saved and put back.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::CssLength;
use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::FoldingRegion;
use landaire_ui::PanelEntry;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::RegionExtent;
use landaire_ui::RegionLimits;
use landaire_ui::SavedNode;
use landaire_ui::SkipReason;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 800.0, height: 500.0 };

const FRAME: Duration = Duration::from_millis(16);

fn kind() -> PanelKind {
    PanelKind::new("text")
}

fn entry(id: &str) -> PanelEntry {
    PanelEntry::new(PanelId::new(id), kind())
}

fn registry() -> PanelRegistry {
    let mut registry = PanelRegistry::new();
    registry.register(
        kind(),
        PanelFactory::new(
            |id: &PanelId| id.as_str().to_owned(),
            |id: &PanelId| {
                let test_id = format!("content-{}", id.as_str());
                AnyView::new(view! { box(attr:data-testid = test_id, style = "height: 40px") {"content"} })
            },
        ),
    );

    registry
}

/// A window over a dock that `fill` puts panels in, and the dock's state.
fn open(fill: impl Fn(DockState) + 'static) -> (HeadlessWindow, DockState) {
    let held = Rc::new(Cell::new(None));
    let window = {
        let held = Rc::clone(&held);
        HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
            let state = DockState::new(registry());
            fill(state);
            held.set(Some(state));
            view! {
                ThemeRoot {
                    Dock(state = state)
                }
            }
        })
        .expect("opens")
    };
    let state = held.get().expect("the view was built");

    (window, state)
}

fn three_tabs(state: DockState) {
    let group = state.add_panel(DockTarget::Region(DockRegion::Center), entry("a")).expect("adds");
    state.add_panel(DockTarget::Group(group), entry("b")).expect("adds");
    state.add_panel(DockTarget::Group(group), PanelEntry { closable: false, ..entry("c") }).expect("adds");
    state.activate(&PanelId::new("b")).expect("b is in the dock");
}

fn ids(state: DockState) -> Vec<String> {
    state.panels().into_iter().map(|entry| entry.id.as_str().to_owned()).collect()
}

fn is_selected(window: &mut HeadlessWindow, label: &str) -> bool {
    let tab = window.by_role_and_label(Role::Tab, label).expect("the tab is announced");

    window.a11y(tab).and_then(|state| state.selected) == Some(true)
}

#[test]
fn closing_the_shown_tab_shows_the_next_one_and_a_fixed_tab_has_no_close_button() {
    let (mut window, state) = open(three_tabs);
    assert!(is_selected(&mut window, "b"));

    let close = window.by_role_and_label(Role::Button, "Close b").expect("b can be closed");
    window.click(close).expect("the button has a box");
    window.advance(FRAME);

    assert_eq!(ids(state), ["a", "c"]);
    assert!(window.by_role_and_label(Role::Tab, "b").is_none());
    assert!(is_selected(&mut window, "c"));
    assert!(window.by_role_and_label(Role::Button, "Close c").is_none());
}

#[test]
fn a_middle_click_closes_a_dock_tab_and_leaves_a_fixed_one() {
    let (mut window, state) = open(three_tabs);

    let a = window.by_role_and_label(Role::Tab, "a").expect("the tab is announced");
    window.middle_click(a).expect("the tab has a box");
    window.advance(FRAME);
    assert_eq!(ids(state), ["b", "c"]);
    assert!(is_selected(&mut window, "b"), "the shown tab stayed shown");

    let c = window.by_role_and_label(Role::Tab, "c").expect("the tab is announced");
    window.middle_click(c).expect("the tab has a box");
    window.advance(FRAME);
    assert_eq!(ids(state), ["b", "c"]);
}

#[test]
fn closing_the_others_from_a_dock_tab_menu_keeps_fixed_panels() {
    let (mut window, state) = open(three_tabs);

    let c = window.by_role_and_label(Role::Tab, "c").expect("the tab is announced");
    window.right_click(c).expect("the tab has a box");
    window.advance(Duration::from_millis(400));
    let others = window.by_role_and_label(Role::MenuItem, "Close others").expect("the menu is open");
    window.click(others).expect("the item has a box");
    window.advance(FRAME);

    assert_eq!(ids(state), ["c"]);
}

#[test]
fn delete_on_a_tab_closes_it() {
    let (mut window, state) = open(three_tabs);
    let tab = window.by_role_and_label(Role::Tab, "a").expect("the tab is announced");

    window.click(tab).expect("the tab has a box");
    window.press_key(NamedKey::Delete, Modifiers::NONE);
    window.advance(FRAME);

    assert_eq!(ids(state), ["b", "c"]);
}

#[test]
fn activating_and_removing_by_id_reach_the_tabs() {
    let (mut window, state) = open(three_tabs);

    state.activate(&PanelId::new("a")).expect("a is in the dock");
    window.advance(FRAME);
    assert!(is_selected(&mut window, "a"));

    state.remove(&PanelId::new("c")).expect("c is removed whether or not it is closable");
    window.advance(FRAME);
    assert!(window.by_role_and_label(Role::Tab, "c").is_none());
    assert!(window.by_test_id("content-c").is_none());
}

#[test]
fn a_folded_region_comes_back_at_the_width_it_had() {
    let (mut window, state) = open(|state| {
        state.add_panel(DockTarget::Region(DockRegion::Center), entry("main")).expect("adds");
        state.add_panel(DockTarget::Region(DockRegion::Left), entry("side")).expect("adds");
    });
    let width = |window: &HeadlessWindow, id: &str| {
        let element = window.by_test_id(id)?;
        window.bounds(element).map(|bounds| bounds.size.width.0)
    };
    let side = width(&window, "content-side").expect("the side panel is laid out");
    let main = width(&window, "content-main").expect("the main panel is laid out");
    assert!(side > 0.0 && side < main, "the side takes its share beside the center ({side}, {main})");

    state.toggle_region(DockRegion::Left);
    window.advance(FRAME);
    assert!(!state.is_region_open(DockRegion::Left));
    assert!(width(&window, "content-side").is_none_or(|width| width == 0.0));
    assert!(width(&window, "content-main").expect("the center is laid out") > main);

    state.toggle_region(DockRegion::Left);
    window.advance(FRAME);
    let back = width(&window, "content-side").expect("the side panel is laid out again");
    assert!((back - side).abs() < 1.0, "came back at {back}, was {side}");
}

fn width_of(window: &HeadlessWindow, id: &str) -> Option<f32> {
    let element = window.by_test_id(id)?;

    window.bounds(element).map(|bounds| bounds.size.width.0)
}

fn pixels(length: f64) -> CssLength {
    CssLength::new(length).expect("a length")
}

/// A center and a right region 500 pixels wide that may not be dragged under 280.
fn chart_dock(state: DockState) {
    state.add_panel(DockTarget::Region(DockRegion::Center), entry("main")).expect("adds");
    state.add_panel(DockTarget::Region(DockRegion::Right), entry("chart")).expect("adds");
    state.set_region_extent(FoldingRegion::Right, RegionExtent::Length(pixels(500.0)));
    state.set_region_limits(FoldingRegion::Right, RegionLimits { min: Some(pixels(280.0)), max: None });
}

#[test]
fn a_region_given_a_length_is_laid_out_at_it() {
    let (mut window, state) = open(chart_dock);
    window.advance(FRAME);

    let chart = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((chart - 500.0).abs() < 1.0, "the chart is {chart} wide");

    state.set_region_extent(FoldingRegion::Right, RegionExtent::Length(pixels(320.0)));
    window.advance(FRAME);
    let chart = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((chart - 320.0).abs() < 1.0, "a new extent reaches a drawn dock: the chart is {chart} wide");
}

#[test]
fn a_region_dragged_past_its_least_stops_there_and_keeps_its_length() {
    let (mut window, state) = open(chart_dock);
    window.advance(FRAME);
    let handle = window
        .by_role(Role::Splitter)
        .into_iter()
        .filter_map(|handle| Some((handle, window.bounds(handle)?)))
        .filter(|(_, bounds)| bounds.size.width.0 > 0.0)
        .max_by(|(_, a), (_, b)| a.origin.x.0.total_cmp(&b.origin.x.0))
        .map(|(handle, _)| handle)
        .expect("the right region's divider shows");

    window.drag(handle, Point::new(DevicePx(790.0), DevicePx(250.0)), 8).expect("the divider has a box");
    window.advance(FRAME);

    let chart = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((chart - 280.0).abs() < 1.0, "the chart is {chart} wide");
    let RegionExtent::Length(saved) = state.dump().extents.right else {
        panic!("a region given a length is saved as one");
    };
    assert!((saved.pixels() - 280.0).abs() < 1.0, "saved at {}", saved.pixels());
}

#[test]
fn limits_outlast_a_restore() {
    let (mut window, state) = open(chart_dock);
    let mut saved = state.dump();
    saved.extents.right = RegionExtent::Length(pixels(100.0));

    state.restore(&saved).expect("the dock is alive");
    window.advance(FRAME);
    window.advance(FRAME);

    let chart = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((chart - 280.0).abs() < 1.0, "the chart is {chart} wide");
}

#[test]
fn a_dumped_layout_restores_to_itself_and_unknown_panels_are_reported() {
    let (mut window, state) = open(three_tabs);
    let saved = state.dump();

    state.remove(&PanelId::new("a")).expect("removes");
    let report = state.restore(&saved).expect("the dock is alive");
    window.advance(FRAME);
    assert!(report.skipped.is_empty());
    assert_eq!(state.dump(), saved);
    assert!(window.by_role_and_label(Role::Tab, "a").is_some());

    let mut stranger = saved.clone();
    let Some(SavedNode::Tabs { panels, .. }) = stranger.center.root.as_mut() else {
        panic!("the center is one group");
    };
    panels.push(PanelEntry::new(PanelId::new("plot"), PanelKind::new("plot")));

    let report = state.restore(&stranger).expect("the dock is alive");
    window.advance(FRAME);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].entry.id, PanelId::new("plot"));
    assert_eq!(report.skipped[0].reason, SkipReason::UnknownKind);
    assert_eq!(ids(state), ["a", "b", "c"]);
    assert!(window.by_role_and_label(Role::Tab, "plot").is_none());
}

#[test]
fn a_change_of_language_reaches_dock_tabs_and_their_close_buttons() {
    let handed = Rc::new(Cell::new(None));
    let sent = Rc::clone(&handed);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let french = RwSignal::new_local(false);
        sent.set(Some(french));
        let say = move |english: &'static str, other: &'static str| {
            Signal::derive_local(move || if french.get() { other } else { english }.to_owned())
        };
        let mut registry = PanelRegistry::new();
        registry.register(
            kind(),
            PanelFactory::new(
                move |_: &PanelId| if french.get() { "Journal" } else { "Log" }.to_owned(),
                |_: &PanelId| AnyView::new(view! { box {"content"} }),
            ),
        );
        let state = DockState::new(registry);
        state.add_panel(DockTarget::Region(DockRegion::Center), entry("log")).expect("adds");

        view! {
            ThemeRoot {
                Dock(state = state, close_label = say("Close", "Fermer"), tabs_label = say("Panels", "Panneaux"))
            }
        }
    })
    .expect("opens");
    let french = handed.get().expect("the view was built");
    assert!(window.by_role_and_label(Role::Tab, "Log").is_some());
    assert!(window.by_role_and_label(Role::Button, "Close Log").is_some());
    assert!(window.by_role_and_label(Role::Group, "Panels").is_some());

    french.set(true);
    window.advance(FRAME);

    assert!(window.by_role_and_label(Role::Tab, "Journal").is_some(), "the tab is renamed");
    assert!(window.by_role_and_label(Role::Tab, "Log").is_none());
    assert!(window.by_role_and_label(Role::Button, "Fermer Journal").is_some(), "the close button is renamed");
    assert!(window.by_role_and_label(Role::Group, "Panneaux").is_some(), "the strip is renamed");
}

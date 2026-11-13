//! A dock in a real document: tabs pressed and closed, regions folded, layouts saved and put back.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::CssLength;
use landaire_ui::DockMenuStrings;
use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::Edge;
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
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::platform::CursorStyle;
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
    open_dock(None, fill)
}

/// [`open`], with a menu on each tab group when `menu` is given.
fn open_dock(menu: Option<DockMenuStrings>, fill: impl Fn(DockState) + 'static) -> (HeadlessWindow, DockState) {
    let held = Rc::new(Cell::new(None));
    let window = {
        let held = Rc::clone(&held);
        HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
            let state = DockState::new(registry());
            fill(state);
            held.set(Some(state));
            let dock = match menu {
                Some(menu) => AnyView::new(view! { Dock(state = state, menu = menu) }),
                None => AnyView::new(view! { Dock(state = state) }),
            };
            view! {
                ThemeRoot {
                    {dock}
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

/// The dividers that take room, each with its box.
fn shown_dividers(window: &mut HeadlessWindow) -> Vec<(Element, Rect<DevicePx, Device>)> {
    window
        .by_role(Role::Splitter)
        .into_iter()
        .filter_map(|handle| Some((handle, window.bounds(handle)?)))
        .filter(|(_, bounds)| bounds.size.width.0 > 0.0 && bounds.size.height.0 > 0.0)
        .collect()
}

#[test]
fn a_divider_shows_the_resize_cursor_of_its_axis() {
    let (mut window, _state) = open(|state| {
        state.add_panel(DockTarget::Region(DockRegion::Center), entry("main")).expect("adds");
        state.add_panel(DockTarget::Region(DockRegion::Right), entry("side")).expect("adds");
        state.add_panel(DockTarget::Region(DockRegion::Bottom), entry("log")).expect("adds");
    });
    window.advance(FRAME);
    let dividers = shown_dividers(&mut window);
    let upright = dividers.iter().find(|(_, bounds)| bounds.size.height.0 > bounds.size.width.0).expect("a side edge");
    let level = dividers.iter().find(|(_, bounds)| bounds.size.width.0 > bounds.size.height.0).expect("a bottom edge");
    assert!(upright.1.size.width.0 >= 9.0, "the side edge catches the pointer {} wide", upright.1.size.width.0);
    let (upright, level) = (upright.0, level.0);

    window.hover(upright).expect("the divider has a box");
    window.advance(FRAME);
    assert_eq!(window.cursor(), Some(CursorStyle::ResizeColumn));

    window.hover(level).expect("the divider has a box");
    window.advance(FRAME);
    assert_eq!(window.cursor(), Some(CursorStyle::ResizeRow));
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

/// The menu button of the group furthest right.
fn rightmost_menu(window: &mut HeadlessWindow) -> Element {
    let buttons = window.by_role(Role::Button);
    let menus: Vec<Element> = buttons
        .into_iter()
        .filter(|button| window.a11y(*button).and_then(|state| state.label).as_deref() == Some("More"))
        .collect();

    menus
        .into_iter()
        .filter_map(|button| Some((button, window.bounds(button)?)))
        .max_by(|(_, a), (_, b)| a.origin.x.0.total_cmp(&b.origin.x.0))
        .map(|(button, _)| button)
        .expect("each group has a menu")
}

fn choose(window: &mut HeadlessWindow, item: &str) {
    let menu = rightmost_menu(window);
    window.click(menu).expect("the menu button has a box");
    window.advance(Duration::from_millis(400));
    let item = window.by_role_and_label(Role::MenuItem, item).expect("the menu offers the item");
    window.click(item).expect("the item has a box");
    window.advance(Duration::from_millis(400));
}

#[test]
fn a_group_zoomed_from_its_menu_fills_the_dock_until_zoomed_out() {
    let (mut window, state) = open_dock(Some(DockMenuStrings::new()), chart_dock);
    window.advance(FRAME);
    let laid_out = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((laid_out - 500.0).abs() < 1.0, "the chart is {laid_out} wide");

    choose(&mut window, "Zoom In");
    let zoomed = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((zoomed - SIZE.width).abs() < 1.0, "the zoomed chart takes the whole dock, {zoomed} wide");
    assert_eq!(state.dump().extents.right, RegionExtent::Length(pixels(500.0)), "the layout is unchanged");

    let main_tab = window.by_role_and_label(Role::Tab, "main").expect("the main tab is announced");
    assert!(window.bounds(main_tab).is_none(), "the covered group's tab leaves the screen and the focus order");
    assert!(width_of(&window, "content-main").is_none(), "and so does its content");

    choose(&mut window, "Zoom Out");
    let back = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((back - 500.0).abs() < 1.0, "the chart is back at {back}");
    assert!(window.bounds(main_tab).is_some(), "the main tab is back");
}

#[test]
fn closing_the_region_of_a_zoomed_group_puts_the_group_back() {
    let (mut window, state) = open_dock(Some(DockMenuStrings::new()), chart_dock);
    window.advance(FRAME);
    choose(&mut window, "Zoom In");
    assert!(state.zoomed().is_some(), "the chart is zoomed");

    state.toggle_region(DockRegion::Right);
    window.advance(FRAME);
    assert_eq!(state.zoomed(), None, "closing its region ends the zoom");

    state.set_region_open(DockRegion::Right, true);
    window.advance(FRAME);
    let back = width_of(&window, "content-chart").expect("the chart is laid out");
    assert!((back - 500.0).abs() < 1.0, "the reopened chart is laid out at {back}");
    assert!(width_of(&window, "content-main").is_some(), "beside the main panel");

    state.set_region_open(DockRegion::Right, false);
    let chart = state.group_of(&PanelId::new("chart")).expect("the chart is docked");
    state.set_zoomed(Some(chart));
    assert_eq!(state.zoomed(), None, "a group in a closed region is not zoomed");
}

#[test]
fn a_zoomed_group_is_the_only_one_shown_and_covers_the_splits() {
    let (mut window, state) = open_dock(Some(DockMenuStrings::new()), |state| {
        let left = state.add_panel(DockTarget::Region(DockRegion::Center), entry("left")).expect("adds");
        state.add_panel(DockTarget::Beside { group: left, edge: Edge::Right }, entry("right")).expect("splits");
    });
    window.advance(FRAME);
    let splitters = |window: &mut HeadlessWindow| {
        let all = window.by_role(Role::Splitter);
        all.into_iter().filter(|splitter| window.bounds(*splitter).is_some()).count()
    };
    assert_eq!(splitters(&mut window), 1, "the split has a handle");

    choose(&mut window, "Zoom In");
    assert_eq!(state.shown_panels(), [PanelId::new("right")], "only the zoomed group's panel is shown");
    assert_eq!(splitters(&mut window), 0, "the covered split's handle leaves the screen and the focus order");

    choose(&mut window, "Zoom Out");
    assert_eq!(state.shown_panels().len(), 2);
    assert_eq!(splitters(&mut window), 1, "the handle is back");
}

#[test]
fn a_group_menu_closes_a_closable_shown_panel_and_offers_no_close_for_a_fixed_one() {
    let (mut window, state) = open_dock(Some(DockMenuStrings::new()), |state| {
        state
            .add_panel(DockTarget::Region(DockRegion::Center), PanelEntry { closable: false, ..entry("main") })
            .expect("adds");
        state.add_panel(DockTarget::Region(DockRegion::Right), entry("chart")).expect("adds");
    });
    window.advance(FRAME);

    choose(&mut window, "Close");
    assert_eq!(ids(state), ["main"]);

    let menu = rightmost_menu(&mut window);
    window.click(menu).expect("the menu button has a box");
    window.advance(Duration::from_millis(400));
    assert!(window.by_role_and_label(Role::MenuItem, "Zoom In").is_some(), "the menu is open");
    assert!(window.by_role_and_label(Role::MenuItem, "Close").is_none(), "a fixed panel is not closed from it");
}

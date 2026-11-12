//! A drawer driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::DrawerProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::ButtonProps;

const SIZE: WindowSize = WindowSize { width: 600.0, height: 300.0 };
const SETTLE: Duration = Duration::from_millis(400);

#[component]
fn Covered(open: RwSignal<bool, LocalStorage>) -> impl IntoView {
    let pressed = RwSignal::new_local(0_u32);

    view! {
        ThemeRoot {
            box(style = "position: relative; width: 600px; height: 300px") {
                Button(
                    a11y:label = move || format!("Behind {}", pressed.get()),
                    on:click = move |_| pressed.update(|pressed| *pressed += 1)
                ) {"Behind"}
                Drawer(open = open, label = "Settings", width = landaire_ui::PanelWidth(300.0)) {
                    Button(a11y:label = "Inside") {"Inside"}
                }
            }
        }
    }
}

fn open_covered() -> (HeadlessWindow, RwSignal<bool, LocalStorage>) {
    let signal = Rc::new(Cell::new(None));
    let handed = Rc::clone(&signal);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let open = RwSignal::new_local(false);
        handed.set(Some(open));
        view! { Covered(open = open) }
    })
    .expect("opens");

    (window, signal.get().expect("the view was built"))
}

fn focus_is_inside(window: &mut HeadlessWindow) -> bool {
    let dialog = window.by_role_and_label(Role::Dialog, "Settings").expect("the drawer is announced");
    let inside = window.by_role_and_label(Role::Button, "Inside").expect("its content is announced");

    [dialog, inside].into_iter().any(|element| window.a11y(element).is_some_and(|state| state.focused))
}

#[test]
fn escape_closes_the_drawer_and_focus_stays_inside_while_open() {
    let (mut window, open) = open_covered();
    assert!(window.by_role(Role::Dialog).is_empty(), "a shut drawer builds nothing");

    open.set(true);
    window.advance(SETTLE);
    assert!(focus_is_inside(&mut window), "opening moves focus into the drawer");

    window.press_key(NamedKey::Tab, Modifiers::NONE);
    window.press_key(NamedKey::Tab, Modifiers::NONE);
    assert!(focus_is_inside(&mut window), "tabbing stays within the drawer");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    assert!(!open.get(), "escape closes it");
    window.advance(SETTLE);
    assert!(window.by_role(Role::Dialog).is_empty(), "the closed drawer is dropped once it has slid away");
}

#[test]
fn a_press_on_the_scrim_closes_the_drawer_without_reaching_what_it_covers() {
    let (mut window, open) = open_covered();
    open.set(true);
    window.advance(SETTLE);

    let behind = window.by_role_and_label(Role::Button, "Behind 0").expect("the covered button is announced");
    window.click(behind).expect("the covered button has a box");
    assert!(!open.get(), "a press beside the panel closes it");
    window.advance(SETTLE);
    assert!(window.by_role_and_label(Role::Button, "Behind 0").is_some(), "the scrim took the press");

    window.click(behind).expect("the button has a box");
    assert!(window.by_role_and_label(Role::Button, "Behind 1").is_some(), "with the drawer gone it takes presses");
}

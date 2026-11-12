//! A theme root in a real document, driven over the headless platform.

#![cfg(feature = "test-support")]

use std::time::Duration;

use landaire_ui::CollapseButtonProps;
use landaire_ui::MotionPreference;
use landaire_ui::PanelSide;
use landaire_ui::Scheme;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::css;
use zgui::prelude::*;
use zgui::view;
use zgui::vocab::Toggled;
use zgui_ui::SwitchProps;
use zgui_ui_tokens::ThemeProviderProps;

/// Two boxes that change color when clicked, one timed by a landaire-ui duration and one by a
/// zgui-ui duration inside a nested theme provider.
const MOVING: &str = css!(
    ".moving { width: 40px; height: 40px; background-color: rgb(255 0 0); }
     .moving.lit { background-color: rgb(0 0 255); }
     .own { transition: background-color var(--lui-motion-fade) var(--lui-motion-ease); }
     .library { transition: background-color var(--zui-motion-duration-normal); }"
);

const SIZE: WindowSize = WindowSize { width: 320.0, height: 240.0 };

#[component]
fn Moving(motion: MotionPreference) -> impl IntoView {
    install_stylesheet("moving", MOVING);
    let lit = RwSignal::new_local(false);

    view! {
        ThemeRoot(choice = ThemeChoice::Fixed(Scheme::Dark), motion = motion) {
            row {
                box(class = "moving own", class:lit = move || lit.get(), attr:data-testid = "own",
                    on:click = move |_| lit.set(true))
                ThemeProvider {
                    box(class = "moving library", class:lit = move || lit.get(), attr:data-testid = "library",
                        on:click = move |_| lit.set(true))
                }
            }
        }
    }
}

/// How many transitions each box is running a frame after it was clicked.
fn running_after_click(motion: MotionPreference) -> [usize; 2] {
    let mut window =
        HeadlessWindow::open(HeadlessOptions::new(SIZE), move || view! { Moving(motion = motion) }).expect("opens");
    let own = window.by_test_id("own").expect("the box is in the document");
    let library = window.by_test_id("library").expect("the box is in the document");

    window.click(own).expect("the box has a box");
    window.advance(Duration::from_millis(16));

    [window.running_animations(own), window.running_animations(library)]
}

#[test]
fn reduced_motion_zeroes_every_duration() {
    let [own, library] = running_after_click(MotionPreference::Full);
    assert!(own > 0 && library > 0, "with motion allowed both boxes transition ({own}, {library})");

    assert_eq!(running_after_click(MotionPreference::Reduced), [0, 0]);
}

#[test]
fn the_harness_finds_presses_and_reads_back_controls() {
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), || {
        view! {
            ThemeRoot {
                row {
                    Switch(a11y:label = "Enabled")
                    CollapseButton(side = PanelSide::Left, label = "Collapse panel")
                }
            }
        }
    })
    .expect("opens");

    let switch = window.by_role_and_label(Role::Switch, "Enabled").expect("the switch is announced");
    assert_eq!(window.a11y(switch).and_then(|state| state.checked), Some(Toggled::False));

    window.click(switch).expect("the switch has a box");
    assert_eq!(window.a11y(switch).and_then(|state| state.checked), Some(Toggled::True));

    let collapse = window.by_role_and_label(Role::Button, "Collapse panel").expect("the button is announced");
    let bounds = window.bounds(collapse).expect("the button has a box");
    assert!(bounds.size.width.0 > 0.0 && bounds.size.height.0 > 0.0);
}

#[test]
fn the_theme_root_zooms_its_window() {
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), || {
        let zoom = Signal::stored_local(Zoom::new(2.0).expect("2 is in range"));
        view! {
            ThemeRoot(zoom = zoom) {
                box(attr:data-testid = "square", style = "width: 10px; height: 10px")
            }
        }
    })
    .expect("opens");
    let square = window.by_test_id("square").expect("the square is in the document");

    let bounds = window.bounds(square).expect("the square has a box");

    assert_eq!((bounds.size.width.0, bounds.size.height.0), (20.0, 20.0));
}

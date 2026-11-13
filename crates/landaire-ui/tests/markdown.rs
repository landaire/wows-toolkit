//! Markdown links driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::RefCell;
use std::rc::Rc;

use landaire_ui::MarkdownProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::reactive::UnsyncCallback;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 600.0, height: 300.0 };

const SOURCE: &str = "See [the notes](https://example.com/notes) and [the issue](https://example.com/issue).";

/// A document whose links record what they were followed to.
fn open(on_link: bool) -> (HeadlessWindow, Rc<RefCell<Vec<String>>>) {
    let followed = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&followed);
    let window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let recorded = Rc::clone(&recorded);
        let callback = on_link.then(|| UnsyncCallback::new(move |target: String| recorded.borrow_mut().push(target)));
        view! {
            ThemeRoot {
                Markdown(source = SOURCE, on_link = callback)
            }
        }
    })
    .expect("opens");

    (window, followed)
}

#[test]
fn links_are_announced_reached_by_tab_and_followed_by_enter() {
    let (mut window, followed) = open(true);
    let links = window.by_role(Role::Link);
    assert_eq!(links.len(), 2, "each link is announced as a link");
    let label = window.a11y(links[0]).and_then(|state| state.label);
    assert_eq!(label.as_deref(), Some("the notes"), "a link is named by its text");

    window.press_key(NamedKey::Tab, Modifiers::NONE);
    assert!(window.a11y(links[0]).is_some_and(|state| state.focused), "Tab reaches the first link");
    window.press_key(NamedKey::Enter, Modifiers::NONE);
    assert_eq!(*followed.borrow(), ["https://example.com/notes"]);

    window.press_key(NamedKey::Tab, Modifiers::NONE);
    assert!(window.a11y(links[1]).is_some_and(|state| state.focused), "Tab moves on to the next link");
    window.press_key(NamedKey::Enter, Modifiers::NONE);
    assert_eq!(*followed.borrow(), ["https://example.com/notes", "https://example.com/issue"]);
}

#[test]
fn inert_links_take_no_focus() {
    let (mut window, _) = open(false);
    assert!(window.by_role(Role::Link).is_empty(), "a link nothing follows is not announced as one");
}

#[test]
fn a_pressed_link_is_followed() {
    let (mut window, followed) = open(true);
    let links = window.by_role(Role::Link);
    window.click(links[1]).expect("the link can be pressed");
    assert_eq!(*followed.borrow(), ["https://example.com/issue"]);
}

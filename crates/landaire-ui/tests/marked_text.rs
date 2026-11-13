//! Marked text in a real document, rebuilt as its marks move.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::MARK_CLASS;
use landaire_ui::MarkedTextProps;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::view;

/// The marks a test's view reads, once the view is built.
type HeldMarks = Rc<Cell<Option<RwSignal<Rc<[Range<usize>]>, LocalStorage>>>>;

#[test]
fn a_piece_that_was_marked_is_unmarked_when_the_marks_move() {
    let held: HeldMarks = Rc::default();
    let given = Rc::clone(&held);
    let options = HeadlessOptions::new(WindowSize { width: 300.0, height: 100.0 });
    let mut window = HeadlessWindow::open(options, move || {
        let ranges: RwSignal<Rc<[Range<usize>]>, LocalStorage> = RwSignal::new_local(std::iter::once(0..4).collect());
        given.set(Some(ranges));
        view! {
            ThemeRoot {
                {move || view! { MarkedText(text = "Ship ship", ranges = ranges.get()) }}
            }
        }
    })
    .expect("opens");
    window.advance(Duration::from_millis(16));
    assert_eq!(window.texts_by_class(MARK_CLASS), ["Ship"]);

    held.get().expect("the view was built").set(std::iter::once(5..9).collect());
    window.advance(Duration::from_millis(16));
    assert_eq!(window.texts_by_class(MARK_CLASS), ["ship"]);
}

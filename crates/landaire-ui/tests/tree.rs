//! A tree in a real document, driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::MenuItems;
use landaire_ui::MenuSelection;
use landaire_ui::TOOLTIP_DELAY;
use landaire_ui::ThemeRootProps;
use landaire_ui::TreeHandle;
use landaire_ui::TreeMenu;
use landaire_ui::TreeModel;
use landaire_ui::TreeProps;
use landaire_ui::TreeRow;
use landaire_ui::TreeTooltip;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::MenuItemProps;

/// Long enough for every frame a press or a scroll asks for.
const SETTLE: Duration = Duration::from_millis(100);

/// A tree of numbered nodes.
struct Nodes {
    roots: Vec<u32>,
    children: HashMap<u32, Vec<u32>>,
    parents: HashMap<u32, u32>,
    names: HashMap<u32, String>,
    unselectable: HashSet<u32>,
}

impl Nodes {
    fn new() -> Self {
        Self {
            roots: Vec::new(),
            children: HashMap::new(),
            parents: HashMap::new(),
            names: HashMap::new(),
            unselectable: HashSet::new(),
        }
    }

    fn add(&mut self, key: u32, parent: Option<u32>, name: &str) {
        match parent {
            Some(parent) => {
                self.children.entry(parent).or_default().push(key);
                self.parents.insert(key, parent);
            }
            None => self.roots.push(key),
        }
        self.names.insert(key, name.to_owned());
    }

    /// `1 [10 [100, 101], 11], 2 [20], 3`.
    fn small() -> Self {
        let mut nodes = Self::new();
        for (key, parent, name) in [
            (1, None, "Alpha"),
            (10, Some(1), "Bravo"),
            (100, Some(10), "Charlie"),
            (101, Some(10), "Delta"),
            (11, Some(1), "Echo"),
            (2, None, "Foxtrot"),
            (20, Some(2), "Golf"),
            (3, None, "Hotel"),
        ] {
            nodes.add(key, parent, name);
        }

        nodes
    }
}

impl TreeModel for Nodes {
    type Key = u32;

    fn roots(&self) -> &[u32] {
        &self.roots
    }

    fn children(&self, key: &u32) -> &[u32] {
        self.children.get(key).map_or(&[], Vec::as_slice)
    }

    fn parent(&self, key: &u32) -> Option<u32> {
        self.parents.get(key).copied()
    }

    fn text(&self, key: &u32) -> Option<&str> {
        self.names.get(key).map(String::as_str)
    }

    fn is_selectable(&self, key: &u32) -> bool {
        !self.unselectable.contains(key)
    }
}

/// The state a test reads and writes from outside the window.
#[derive(Clone, Copy)]
struct Probe {
    expanded: RwSignal<HashSet<u32>, LocalStorage>,
    selection: RwSignal<HashSet<u32>, LocalStorage>,
    activated: RwSignal<Vec<u32>, LocalStorage>,
    handle: TreeHandle<u32>,
}

fn open(nodes: Nodes, expanded: &[u32]) -> (HeadlessWindow, Probe) {
    open_with_menu(nodes, expanded, MenuSelection::Follow, false)
}

fn open_sticky(nodes: Nodes, expanded: &[u32]) -> (HeadlessWindow, Probe) {
    open_with_menu(nodes, expanded, MenuSelection::Follow, true)
}

fn open_with_menu(
    nodes: Nodes,
    expanded: &[u32],
    menu_selection: MenuSelection,
    sticky_branches: bool,
) -> (HeadlessWindow, Probe) {
    let model = Rc::new(nodes);
    let opened: HashSet<u32> = expanded.iter().copied().collect();
    let probe: Rc<Cell<Option<Probe>>> = Rc::new(Cell::new(None));
    let published = Rc::clone(&probe);

    let options = HeadlessOptions::new(WindowSize { width: 320.0, height: 260.0 });
    let window = HeadlessWindow::open(options, move || {
        let probe = Probe {
            expanded: RwSignal::new_local(opened.clone()),
            selection: RwSignal::new_local(HashSet::new()),
            activated: RwSignal::new_local(Vec::new()),
            handle: TreeHandle::new(),
        };
        published.set(Some(probe));

        let activated = probe.activated;
        let model = Signal::stored_local(Rc::clone(&model));
        view! {
            ThemeRoot {
                Tree(
                    model = model,
                    row = |row: TreeRow<u32>| view! {
                        text(attr:data-testid = format!("row-{}", row.key)) {{row.key.to_string()}}
                    },
                    expanded = probe.expanded,
                    sticky_branches = sticky_branches,
                    selection = probe.selection,
                    handle = probe.handle,
                    on_activate = UnsyncCallback::new(move |key: u32| activated.update(|keys| keys.push(key))),
                    context_menu = TreeMenu::new(|key: &u32| {
                        let key = *key;
                        (key != 20).then(|| MenuItems::new(move || view! {
                            MenuItem(attr:data-testid = format!("menu-{key}")) {"Open"}
                        }))
                    }).with_selection(menu_selection),
                    tooltip = TreeTooltip::new(|key: &u32| (*key != 3).then(|| view! { text {{format!("tip {key}")}} })),
                    attr:data-testid = "tree",
                    style:height = "220px"
                )
            }
        }
    })
    .expect("the window opens");
    let probe = probe.take().expect("the view was built");

    (window, probe)
}

fn row(window: &HeadlessWindow, key: u32) -> Element {
    window.by_test_id(&format!("row-{key}")).unwrap_or_else(|| panic!("row {key} is built"))
}

fn press(window: &mut HeadlessWindow, key: NamedKey, modifiers: Modifiers) {
    window.press_key(key, modifiers);
    window.advance(SETTLE);
}

fn selected(probe: Probe) -> HashSet<u32> {
    probe.selection.get_untracked()
}

#[test]
fn a_rows_tooltip_covers_its_chevron_and_shows_one_at_a_time() {
    let (mut window, _) = open(Nodes::small(), &[]);
    let wait = TOOLTIP_DELAY + SETTLE;

    let chevron = window.by_class("lui-tree__chevron").expect("the first root has a chevron");
    window.hover(chevron).expect("hovers");
    window.advance(wait);
    assert_eq!(window.texts_by_class("lui-tooltip"), ["tip 1"]);

    window.hover(row(&window, 2)).expect("hovers");
    window.advance(wait);
    assert_eq!(window.texts_by_class("lui-tooltip"), ["tip 2"]);

    window.hover(row(&window, 3)).expect("hovers");
    window.advance(wait);
    assert!(window.texts_by_class("lui-tooltip").is_empty(), "a row whose key builds none has no tooltip");
}

#[test]
fn expanding_and_collapsing_shows_and_hides_descendants() {
    let (mut window, probe) = open(Nodes::small(), &[]);
    assert!(window.by_test_id("row-10").is_none(), "a closed node's children are not built");

    window.click(row(&window, 1)).expect("the row has a box");
    press(&mut window, NamedKey::ArrowRight, Modifiers::NONE);
    assert!(probe.expanded.get_untracked().contains(&1));
    assert!(window.by_test_id("row-10").is_some());
    assert!(window.by_test_id("row-100").is_none(), "a grandchild stays hidden while its parent is closed");

    probe.expanded.update(|set| {
        set.insert(10);
    });
    window.advance(SETTLE);
    assert!(window.by_test_id("row-100").is_some(), "an expanded set written by the application is followed");

    press(&mut window, NamedKey::ArrowLeft, Modifiers::NONE);
    assert!(!probe.expanded.get_untracked().contains(&1));
    assert!(window.by_test_id("row-10").is_none());
    assert!(window.by_test_id("row-100").is_none(), "collapsing hides every level below");
}

#[test]
fn shift_click_selects_a_run_and_ctrl_click_toggles() {
    let (mut window, probe) = open(Nodes::small(), &[1, 10]);

    window.click(row(&window, 10)).expect("the row has a box");
    window.click_with(row(&window, 2), Modifiers::SHIFT).expect("the row has a box");
    assert_eq!(selected(probe), HashSet::from([10, 100, 101, 11, 2]), "the visible rows from 10 down to 2");

    window.click_with(row(&window, 100), Modifiers::PRIMARY).expect("the row has a box");
    assert_eq!(selected(probe), HashSet::from([10, 101, 11, 2]), "ctrl takes a selected row out");

    window.click_with(row(&window, 3), Modifiers::PRIMARY).expect("the row has a box");
    assert_eq!(selected(probe), HashSet::from([10, 101, 11, 2, 3]), "and puts an unselected one in");

    // The ctrl-click moved the anchor to 3, so shift runs from there, past the closed node 2.
    window.click_with(row(&window, 11), Modifiers::SHIFT).expect("the row has a box");
    assert_eq!(selected(probe), HashSet::from([11, 2, 3]));

    window.click(row(&window, 1)).expect("the row has a box");
    assert_eq!(selected(probe), HashSet::from([1]), "a plain click replaces the selection");
}

#[test]
fn arrow_keys_walk_the_rows_and_left_steps_out_to_the_parent() {
    let (mut window, probe) = open(Nodes::small(), &[1, 10]);

    window.click(row(&window, 100)).expect("the row has a box");
    press(&mut window, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([10]), "left on a leaf steps to its parent");

    press(&mut window, NamedKey::ArrowLeft, Modifiers::NONE);
    assert!(!probe.expanded.get_untracked().contains(&10), "left on an open node collapses it");
    assert_eq!(selected(probe), HashSet::from([10]));

    press(&mut window, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([1]));

    press(&mut window, NamedKey::ArrowDown, Modifiers::NONE);
    press(&mut window, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([11]), "down skips the collapsed node's children");

    press(&mut window, NamedKey::End, Modifiers::NONE);
    press(&mut window, NamedKey::ArrowUp, Modifiers::SHIFT);
    assert_eq!(selected(probe), HashSet::from([2, 3]));

    press(&mut window, NamedKey::Home, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([1]));

    press(&mut window, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([10]), "right on an open node steps into it");

    press(&mut window, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(probe.activated.get_untracked(), vec![10]);
    // Held, the key repeats, and a modified Enter belongs to whatever holds the tree.
    window.repeat_key(NamedKey::Enter, Modifiers::NONE);
    window.advance(SETTLE);
    press(&mut window, NamedKey::Enter, Modifiers::PRIMARY);
    assert_eq!(probe.activated.get_untracked(), vec![10], "neither opens the row again");

    window.press_character('a', Modifiers::PRIMARY);
    window.advance(SETTLE);
    assert_eq!(selected(probe), HashSet::from([1, 10, 11, 2, 3]), "select all takes the visible rows");
}

#[test]
fn double_click_activates_and_secondary_click_opens_the_rows_menu() {
    let (mut window, probe) = open(Nodes::small(), &[2]);

    window.double_click(row(&window, 20)).expect("the row has a box");
    assert_eq!(probe.activated.get_untracked(), vec![20]);

    window.right_click(row(&window, 3)).expect("the row has a box");
    window.advance(SETTLE);
    assert_eq!(selected(probe), HashSet::from([3]), "the clicked row is what the menu acts on");
    assert!(window.by_test_id("menu-3").is_some(), "the menu is built for the clicked row");

    press(&mut window, NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(500));
    assert!(window.by_test_id("menu-3").is_none(), "escape closes the menu");

    window.click(row(&window, 2)).expect("the row has a box");
    press(&mut window, NamedKey::ContextMenu, Modifiers::NONE);
    assert!(window.by_test_id("menu-2").is_some(), "the menu key opens the cursor row's menu");
}

#[test]
fn a_row_whose_key_builds_no_menu_opens_none_and_keeps_the_selection() {
    let (mut window, probe) = open(Nodes::small(), &[2]);
    window.click(row(&window, 3)).expect("the row has a box");

    window.right_click(row(&window, 20)).expect("the row has a box");
    window.advance(SETTLE);
    assert!(window.by_role(Role::Menu).is_empty(), "a row whose key builds no menu opens none");
    assert_eq!(selected(probe), HashSet::from([3]), "and the selection stays where it was");

    window.click(row(&window, 20)).expect("the row has a box");
    press(&mut window, NamedKey::ContextMenu, Modifiers::NONE);
    assert!(window.by_role(Role::Menu).is_empty(), "nor does the menu key");

    window.right_click(row(&window, 2)).expect("the row has a box");
    window.advance(SETTLE);
    assert!(window.by_test_id("menu-2").is_some(), "a row beside it still has its menu");
}

#[test]
fn a_menu_that_keeps_the_selection_leaves_it_on_its_row() {
    let (mut window, probe) = open_with_menu(Nodes::small(), &[2], MenuSelection::Keep, false);
    window.click(row(&window, 20)).expect("the row has a box");

    window.right_click(row(&window, 3)).expect("the row has a box");
    window.advance(SETTLE);
    assert!(window.by_test_id("menu-3").is_some(), "the menu is built for the clicked row");
    assert_eq!(selected(probe), HashSet::from([20]), "the selection stays where it was");

    press(&mut window, NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(500));
    assert_eq!(selected(probe), HashSet::from([20]), "closing the menu leaves the selection");
    let cursor = window.all_by_attribute("data-cursor", "true");
    assert_eq!(cursor.len(), 1, "one row holds the cursor");
    let cursor = window.bounds(cursor[0]).expect("the cursor row has a box");
    let kept = window.bounds(row(&window, 20)).expect("the row has a box");
    let middle = kept.origin.y.0 + kept.size.height.0 / 2.0;
    assert!(
        cursor.origin.y.0 <= middle && middle < cursor.origin.y.0 + cursor.size.height.0,
        "the cursor stays on the selected row"
    );
}

#[test]
fn tabbing_in_puts_the_cursor_on_the_selected_row() {
    let (mut window, probe) = open(Nodes::small(), &[1]);
    probe.selection.set(HashSet::from([11]));
    window.advance(SETTLE);

    press(&mut window, NamedKey::Tab, Modifiers::NONE);
    press(&mut window, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(probe.activated.get_untracked(), vec![11]);
}

#[test]
fn tabbing_into_a_long_tree_scrolls_the_selected_row_into_view() {
    let mut nodes = Nodes::new();
    for key in 0..2_000 {
        nodes.add(key, None, "item");
    }
    let (mut window, probe) = open(nodes, &[]);
    probe.selection.set(HashSet::from([1_500]));
    window.advance(SETTLE);
    assert!(window.by_test_id("row-1500").is_none());

    press(&mut window, NamedKey::Tab, Modifiers::NONE);
    assert!(window.by_test_id("row-1500").is_some());
}

#[test]
fn collapsing_an_ancestor_moves_the_cursor_onto_it() {
    let (mut window, probe) = open(Nodes::small(), &[1, 10]);

    window.click(row(&window, 100)).expect("the row has a box");
    probe.expanded.set(HashSet::new());
    window.advance(SETTLE);

    press(&mut window, NamedKey::ContextMenu, Modifiers::NONE);
    assert!(window.by_test_id("menu-1").is_some(), "the menu key acts on the row that hid the cursor");
    assert_eq!(selected(probe), HashSet::from([1]));
}

#[test]
fn typing_moves_to_the_row_whose_text_begins_with_it() {
    let (mut window, probe) = open(Nodes::small(), &[1]);

    window.click(row(&window, 1)).expect("the row has a box");
    window.type_text("ho");
    window.advance(SETTLE);
    assert_eq!(selected(probe), HashSet::from([3]));
}

#[test]
fn rows_that_cannot_be_selected_are_passed_over_by_keys_typing_and_presses() {
    let mut nodes = Nodes::small();
    nodes.unselectable.insert(2);
    let (mut window, probe) = open(nodes, &[]);

    window.click(row(&window, 2)).expect("the row has a box");
    assert!(selected(probe).is_empty(), "a press does not select it");

    window.click(row(&window, 1)).expect("the row has a box");
    press(&mut window, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([3]), "down steps over it");
    press(&mut window, NamedKey::ArrowUp, Modifiers::NONE);
    assert_eq!(selected(probe), HashSet::from([1]), "and so does up");

    window.type_text("fox");
    window.advance(SETTLE);
    assert_eq!(selected(probe), HashSet::from([1]), "typing does not find it");

    window.press_character('a', Modifiers::PRIMARY);
    window.advance(SETTLE);
    assert_eq!(selected(probe), HashSet::from([1, 3]), "select all leaves it out");
}

#[test]
fn scroll_to_key_opens_the_ancestors_and_shows_a_deep_row() {
    let mut nodes = Nodes::new();
    for top in 0..50 {
        nodes.add(top, None, "group");
        for middle in 0..50 {
            let middle_key = 1_000 + top * 100 + middle;
            nodes.add(middle_key, Some(top), "set");
            for leaf in 0..10 {
                nodes.add(1_000_000 + middle_key * 10 + leaf, Some(middle_key), "item");
            }
        }
    }
    let deep = 1_000_000 + (1_000 + 40 * 100 + 30) * 10 + 5;
    let (mut window, probe) = open(nodes, &[]);
    assert!(window.by_test_id(&format!("row-{deep}")).is_none());

    probe.handle.scroll_to_key(deep);
    window.advance(SETTLE);

    let expanded = probe.expanded.get_untracked();
    assert!(expanded.contains(&40) && expanded.contains(&(1_000 + 40 * 100 + 30)), "both ancestors open");
    let tree = window.by_test_id("tree").expect("the tree is built");
    let tree = window.bounds(tree).expect("the tree has a box");
    let shown = window.bounds(row(&window, deep)).expect("the row has a box");
    assert!(
        shown.origin.y.0 >= tree.origin.y.0
            && shown.origin.y.0 + shown.size.height.0 <= tree.origin.y.0 + tree.size.height.0,
        "the row {shown:?} lies inside the tree {tree:?}"
    );
}

#[test]
fn ten_thousand_nodes_build_only_the_rows_in_view() {
    let mut nodes = Nodes::new();
    for group in 0..100 {
        nodes.add(group, None, "group");
        for item in 0..100 {
            nodes.add(1_000 + group * 100 + item, Some(group), "item");
        }
    }
    let every_group: Vec<u32> = (0..100).collect();
    let (mut window, _probe) = open(nodes, &every_group);

    let built = window.by_role(Role::TreeItem).len();
    assert!(built < 40, "{built} rows built for a port about ten rows tall");
    assert!(window.by_test_id("row-0").is_some());

    window.click(row(&window, 0)).expect("the row has a box");
    press(&mut window, NamedKey::End, Modifiers::NONE);
    let last = 1_000 + 99 * 100 + 99;
    assert!(window.by_test_id(&format!("row-{last}")).is_some(), "End scrolls the last of 10 100 rows in");
    assert!(window.by_test_id("row-0").is_none(), "and the first one out");
    assert!(window.by_role(Role::TreeItem).len() < 40);
}

/// `1 [10 [100..=129], 200..=209], 2 [300..=329]`, all expanded.
fn nested() -> (Nodes, Vec<u32>) {
    let mut nodes = Nodes::new();
    nodes.add(1, None, "one");
    nodes.add(10, Some(1), "ten");
    for leaf in 100..=129 {
        nodes.add(leaf, Some(10), "leaf");
    }
    for leaf in 200..=209 {
        nodes.add(leaf, Some(1), "leaf");
    }
    nodes.add(2, None, "two");
    for leaf in 300..=329 {
        nodes.add(leaf, Some(2), "leaf");
    }

    (nodes, vec![1, 10, 2])
}

/// Asserts that the row of `key` starts `expected` device pixels below the top of the tree.
fn assert_row_at(window: &HeadlessWindow, key: u32, expected: f32, what: &str) {
    let tree = window.bounds(window.by_test_id("tree").expect("the tree is built")).expect("the tree has a box");
    let label = window.bounds(row(window, key)).expect("the row has a box");
    // The test id is on the label, which is centred in its 22 pixel row and snapped to a pixel.
    let inset = (22.0 - label.size.height.0) / 2.0;
    let actual = label.origin.y.0 - inset - tree.origin.y.0;

    assert!((actual - expected).abs() <= 1.0, "{what}: row {key} starts {actual} below the top, not {expected}");
}

fn scroll_by(window: &mut HeadlessWindow, down: f32) {
    let tree = window.by_test_id("tree").expect("the tree is built");
    window.wheel(tree, down).expect("the tree has a box");
    window.advance(Duration::from_secs(1));
}

#[test]
fn expanded_branches_stay_at_the_top_while_their_rows_scroll_past() {
    let (nodes, expanded) = nested();
    let (mut window, _probe) = open_sticky(nodes, &expanded);

    // Rows are 22 pixels tall: five rows down, 1 and 10 have scrolled away above 100..=104.
    scroll_by(&mut window, 110.0);
    assert_row_at(&window, 1, 0.0, "1 is pinned at the top");
    assert_row_at(&window, 10, 22.0, "10 is pinned below 1");
    assert_row_at(&window, 105, 44.0, "105 is the first row below them");

    // 200 is the 33rd row, so 10's rows have all scrolled past.
    scroll_by(&mut window, 32.0 * 22.0 - 110.0);
    assert_row_at(&window, 1, 0.0, "1 still holds rows below it");
    assert!(window.by_test_id("row-10").is_none(), "10 has no rows left in view");

    // 209, the last row under 1, is the 42nd row; 1 moves up with it.
    scroll_by(&mut window, 41.0 * 22.0 - 32.0 * 22.0 + 8.0);
    assert_row_at(&window, 1, -8.0, "1 is pushed up by its last row");
    assert_row_at(&window, 2, 14.0, "2 follows below 1");
}

#[test]
fn a_pinned_branch_closed_from_its_chevron_stays_where_it_was_pinned() {
    let (nodes, expanded) = nested();
    let (mut window, probe) = open_sticky(nodes, &expanded);
    scroll_by(&mut window, 110.0);

    let chevrons = window.all_by_class("lui-tree__chevron");
    assert!(chevrons.len() >= 2, "1 and 10 are pinned with their chevrons");
    window.click(chevrons[1]).expect("10's chevron has a box");
    window.advance(SETTLE);

    assert!(!probe.expanded.get_untracked().contains(&10), "10 closed");
    assert_row_at(&window, 10, 22.0, "10 stays below 1");
    assert_row_at(&window, 200, 44.0, "the rows after 10 follow it");
}

#[test]
fn moving_up_onto_a_row_under_the_pinned_branches_scrolls_it_below_them() {
    let (nodes, expanded) = nested();
    let (mut window, probe) = open_sticky(nodes, &expanded);
    scroll_by(&mut window, 110.0);

    window.click(row(&window, 105)).expect("the row has a box");
    press(&mut window, NamedKey::ArrowUp, Modifiers::NONE);

    assert_eq!(selected(probe), HashSet::from([104]));
    assert_row_at(&window, 104, 44.0, "104 is shown below 1 and 10");
}

//! The Unpacker tab driven in the headless shell, with its game data handed to its model.

use std::collections::HashSet;
use std::io::Write as _;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use rust_i18n::t;
use wows_toolkit_viewmodel::unpacker::listing::ROOT_PATH;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui::vocab::Toggled;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::shell::palette::TabRequest;
use crate::tabs::unpacker::built_model;
use crate::tabs::unpacker::model::UnpackerModel;
use crate::tabs::unpacker::pane::BrowserSource;
use crate::tabs::unpacker::pane::RowKey;

/// The shell, open on the Unpacker tab.
struct UnpackerWindow {
    window: HeadlessWindow,
    model: UnpackerModel,
}

impl UnpackerWindow {
    fn open(options: HeadlessOptions) -> Self {
        let mut window = open_shell(options);
        let tab = window.by_test_id("unpacker").expect("the strip has an Unpacker tab");
        window.click(tab).expect("the tab can be pressed");
        settle_in_real_time(&mut window);
        let model = built_model().expect("the Unpacker tab was built");
        Self { window, model }
    }

    /// The tab with `vfs` in its package browser.
    fn with_vfs(options: HeadlessOptions, vfs: VfsPath) -> Self {
        let mut this = Self::open(options);
        this.model.pkg.set_vfs(vfs);
        this.settle();
        this
    }

    fn settle(&mut self) {
        self.window.advance(Duration::from_millis(400));
    }

    fn shown(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    fn click(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
        self.settle();
    }

    fn checked(&mut self, test_id: &str) -> Option<Toggled> {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.a11y(element).and_then(|state| state.checked)
    }
}

fn options() -> HeadlessOptions {
    HeadlessOptions::new(REVIEW_SIZE)
}

fn write(root: &VfsPath, path: &str, body: &str) {
    root.join(path).unwrap().create_file().unwrap().write_all(body.as_bytes()).unwrap();
}

/// A package tree with one nested directory and two files, enough for the breadcrumbs and both
/// kinds of queue control.
fn fixture() -> VfsPath {
    let root: VfsPath = MemoryFS::new().into();
    root.join("content/gameplay").unwrap().create_dir_all().unwrap();
    write(&root, "content/a.xml", "<a/>");
    write(&root, "content/gameplay/b.xml", "<b/>");
    root
}

#[test]
fn an_empty_queue_refuses_its_actions_and_offers_no_cancel() {
    let mut tab = UnpackerWindow::open(options());
    assert!(!tab.shown("unpacker-cancel"), "cancel belongs to a run in progress");
    for action in ["unpacker-extract", "unpacker-queue-clear-all"] {
        tab.click(action);
    }
    assert!(tab.shown("unpacker-extract"), "the queue panel survives presses on its refused buttons");
    assert!(tab.model.extract.with_untracked(|state| !state.is_busy()), "no run started");
    assert!(tab.shown("unpacker-queue-empty"), "the empty queue says how to fill it");
}

/// The run controls sit at the foot of the queue panel, as they do with entries queued.
#[test]
fn the_extract_button_sits_at_the_foot_of_an_empty_queue() {
    let tab = UnpackerWindow::open(options());
    let panel = tab.window.by_test_id("unpacker-queue-panel").expect("the queue panel is up");
    let extract = tab.window.by_test_id("unpacker-extract").expect("the extract button is up");
    let panel = tab.window.bounds(panel).expect("the panel has a box");
    let extract = tab.window.bounds(extract).expect("the button has a box");

    let panel_bottom = panel.origin.y.0 + panel.size.height.0;
    let extract_bottom = extract.origin.y.0 + extract.size.height.0;
    let gap = panel_bottom - extract_bottom;
    assert!((0.0..80.0).contains(&gap), "{extract:?} is inside the foot of {panel:?}");
}

/// Each column heading starts where its column's values do, whether or not the listing scrolls.
#[test]
fn the_column_headings_line_up_with_their_values() {
    let root: VfsPath = MemoryFS::new().into();
    root.join("few").unwrap().create_dir_all().unwrap();
    write(&root, "few/a.xml", "<a/>");
    root.join("many").unwrap().create_dir_all().unwrap();
    for index in 0..80 {
        write(&root, &format!("many/{index}.xml"), "<a/>");
    }
    let mut tab = UnpackerWindow::with_vfs(options(), root);

    for dir in ["/few", "/many"] {
        tab.model.pkg.select_dir(dir.to_string());
        tab.settle();
        for class in ["wt-file__name", "wt-file__size", "wt-file__type"] {
            let cells = tab.window.all_by_class(class);
            let [heading, value, ..] = cells.as_slice() else {
                panic!("{dir}: {class} has a heading and a value");
            };
            let heading = tab.window.bounds(*heading).expect("the heading has a box");
            let value = tab.window.bounds(*value).expect("the value has a box");
            let (heading_x, value_x) = (heading.origin.x.0, value.origin.x.0);
            let (heading_end, value_end) = (heading_x + heading.size.width.0, value_x + value.size.width.0);
            assert!((heading_x - value_x).abs() < 1.0, "{dir}: {class} heading at {heading_x}, values at {value_x}");
            assert!(
                (heading_end - value_end).abs() < 1.0,
                "{dir}: {class} heading ends at {heading_end}, values at {value_end}"
            );
        }
    }
}

#[test]
fn the_queue_panel_holds_the_destination_beside_the_listing() {
    let mut tab = UnpackerWindow::open(options());
    tab.model.adopt_output_dir("C:/extracted".to_string());
    tab.settle();
    assert!(tab.shown("unpacker-queue-clear-all"), "the queue panel is open beside the listing");
    let field = tab.window.by_test_id("unpacker-output-dir").expect("the destination is on the queue panel");
    let value = tab.window.a11y(field).and_then(|state| state.value);
    assert_eq!(value.as_deref(), Some("C:/extracted"), "the field shows the saved directory");
}

#[test]
fn the_browsers_say_there_is_no_game_data_when_the_directory_is_unset() {
    let tab = UnpackerWindow::open(options());
    assert!(tab.shown("unpacker-pkg-empty"), "the package pane says why it is empty");
    assert!(!tab.shown("unpacker-pkg-filter"), "the filter appears only once a VFS is loaded");
}

#[test]
fn the_dock_opens_on_the_package_listing() {
    let tab = UnpackerWindow::open(options());
    let pkg = landaire_ui::PanelId::new(BrowserSource::Pkg.id_fragment());
    let group = tab.model.dock.group_of(&pkg).expect("the package browser is docked");
    assert_eq!(tab.model.dock.active_panel(group), Some(pkg), "the package browser is in front");
    assert!(tab.shown("unpacker-pkg-pane"));
}

#[test]
fn the_folder_tree_collapses_to_a_rail_and_expands_again() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    assert!(tab.shown("unpacker-pkg-filter"), "the tree sidebar is up once a VFS is loaded");
    assert!(!tab.shown("unpacker-pkg-tree-rail"), "no rail while the sidebar is up");

    tab.click("unpacker-pkg-tree-collapse");
    assert!(!tab.model.pkg.sidebar_open.get_untracked(), "collapsing puts the sidebar away");
    assert!(tab.shown("unpacker-pkg-tree-rail"), "the rail stands in for it");
    assert!(tab.shown("unpacker-pkg-extract-listed"), "the listing stays when the sidebar goes");

    tab.click("unpacker-pkg-tree-rail");
    assert!(tab.model.pkg.sidebar_open.get_untracked(), "the rail brings the sidebar back");
    assert!(!tab.shown("unpacker-pkg-tree-rail"), "and goes away with it");
}

/// Escape in the file filter, the content search and the path filter empties what was typed.
#[test]
fn escape_empties_the_filter_and_search_fields() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    let pane = tab.model.pkg;
    for (id, value) in [
        ("unpacker-pkg-filter", pane.filter),
        ("unpacker-pkg-search", pane.search_query),
        ("unpacker-pkg-path-filter", pane.path_filter),
    ] {
        tab.click(id);
        tab.window.type_text("xml");
        tab.settle();
        assert_eq!(value.get_untracked(), "xml", "{id} takes typing");
        tab.window.press_key(NamedKey::Escape, Modifiers::NONE);
        tab.settle();
        assert_eq!(value.get_untracked(), "", "Escape empties {id}");
    }
}

#[test]
fn the_status_bar_closes_the_queue_dock_and_opens_it_again() {
    let mut tab = UnpackerWindow::open(options());
    assert!(tab.shown("unpacker-queue-clear-all"), "the queue is up");
    tab.click("unpacker-dock-toggle-right");
    assert!(!tab.model.dock.is_region_open(landaire_ui::DockRegion::Right), "the toggle closes the queue dock");
    tab.click("unpacker-dock-toggle-right");
    assert!(tab.model.dock.is_region_open(landaire_ui::DockRegion::Right), "and opens it again");
}

#[test]
fn revealing_a_file_selects_the_directory_it_is_in() {
    let tab = UnpackerWindow::open(options());
    let root: VfsPath = MemoryFS::new().into();
    tab.model.pkg.reveal(&root.join("content/gameplay/a.xml").unwrap());
    assert_eq!(tab.model.pkg.selected_dir.get_untracked(), "/content/gameplay");
    // A file at the root belongs to the root, not to a directory named by the whole path.
    tab.model.pkg.reveal(&root.join("top.txt").unwrap());
    assert_eq!(tab.model.pkg.selected_dir.get_untracked(), ROOT_PATH);
}

#[test]
fn the_breadcrumbs_follow_the_selected_directory_and_navigate() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    assert!(tab.shown("unpacker-pkg-crumb-0"), "the root crumb is always there");
    assert!(!tab.shown("unpacker-pkg-crumb-1"), "the root has no deeper crumb");

    tab.model.pkg.select_dir("/content/gameplay".to_string());
    tab.settle();
    assert!(tab.shown("unpacker-pkg-crumb-2"), "one crumb per level");

    tab.click("unpacker-pkg-crumb-1");
    assert_eq!(tab.model.pkg.selected_dir.get_untracked(), "/content", "a crumb navigates to its level");
    assert!(!tab.shown("unpacker-pkg-crumb-2"), "and drops the level below it");
}

#[test]
fn a_rows_queue_control_reports_and_changes_the_queue() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    tab.model.pkg.select_dir("/content".to_string());
    tab.settle();
    // Directories sort first, so row 0 is "gameplay" and row 1 "a.xml".
    assert_ne!(tab.checked("unpacker-pkg-queue-1"), Some(Toggled::True), "nothing is queued yet");

    tab.click("unpacker-pkg-queue-0");
    tab.click("unpacker-pkg-queue-1");
    let queued: Vec<String> =
        tab.model.queue.with_untracked(|queue| queue.entries().iter().map(|path| path.as_str().to_string()).collect());
    assert_eq!(queued, vec!["/content/gameplay", "/content/a.xml"], "the folder and the file are queued");
    assert_eq!(tab.checked("unpacker-pkg-queue-1"), Some(Toggled::True), "the queued file reads as queued");
    assert!(tab.shown("unpacker-queue-entry-1"), "the queue panel lists both");

    tab.click("unpacker-queue-remove-0");
    assert_eq!(tab.model.queue.with_untracked(|queue| queue.len()), 1, "a row's remove button takes it out");
}

#[test]
fn a_filter_lists_matching_files_from_the_whole_tree() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    let pane = tab.model.pkg;

    pane.filter.set(".xml".to_string());
    tab.settle();
    let rows = pane.rows.with_untracked(|rows| rows.all());
    let mut labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["/content/a.xml", "/content/gameplay/b.xml"], "matched at any depth");
    assert!(rows.iter().all(|row| !row.is_dir && row.size.is_some()), "files only, each with its size");

    pane.filter.set("gameplay".to_string());
    tab.settle();
    assert_eq!(pane.rows.with_untracked(|rows| rows.len()), 1, "the filter narrows to the nested file");
    assert_eq!(tab.window.texts_by_class(landaire_ui::MARK_CLASS), ["gameplay"], "the row marks what matched");

    pane.filter.set(String::new());
    tab.settle();
    let cleared: Vec<String> = pane.rows.with_untracked(|rows| rows.all()).into_iter().map(|row| row.label).collect();
    assert_eq!(cleared, vec!["content"], "clearing the filter goes back to the selected directory");
}

#[test]
fn a_search_opens_a_results_panel_that_reveals_its_hits() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    let files = tab.model.pkg.files().expect("the pane has a VFS");
    tab.model.open_search(BrowserSource::Pkg, "<b".to_string(), String::new(), files);
    settle_in_real_time(&mut tab.window);
    assert!(tab.shown("unpacker-search-panel"), "the results open in the bottom dock");
    assert!(tab.shown("unpacker-hit-0"), "the one hit is listed");
    assert!(!tab.shown("unpacker-hit-1"));

    tab.click("unpacker-dock-toggle-bottom");
    assert!(
        !tab.model.dock.is_region_open(landaire_ui::DockRegion::Bottom),
        "the bottom dock has a toggle once it holds a search"
    );
}

#[test]
fn double_clicking_a_file_opens_it_in_a_viewer() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    tab.model.pkg.select_dir("/content".to_string());
    tab.settle();
    let row = tab.window.by_test_id("unpacker-pkg-row-1").expect("the file has a row");
    tab.window.double_click(row).expect("the row can be pressed");
    settle_in_real_time(&mut tab.window);
    assert!(tab.shown("unpacker-viewer-text"), "the XML opens as text");
}

/// The shell with a file named `name` holding `body` open in a viewer.
fn open_text(options: HeadlessOptions, name: &str, body: &str) -> UnpackerWindow {
    let root: VfsPath = MemoryFS::new().into();
    write(&root, name, body);
    let mut tab = UnpackerWindow::with_vfs(options, root.clone());
    tab.model.open_viewer(root.join(name).expect("the path is valid"));
    settle_in_real_time(&mut tab.window);
    tab
}

/// The shell with `SHIP_XML` open in a viewer.
fn open_ship_xml(options: HeadlessOptions) -> UnpackerWindow {
    open_text(options, "ship.xml", SHIP_XML)
}

/// A tab is two columns wide, and a press lands on the character drawn under it after tabs and
/// wide characters.
#[test]
fn the_viewer_places_presses_after_tabs_and_wide_characters() {
    let mut tab = open_text(options(), "indent.xml", "\t\tvalue = 1\n    value = 2\n\u{4e2d}\u{6587}value = 3");
    let lines: Vec<_> = tab
        .window
        .all_by_class("wt-code-line__text")
        .into_iter()
        .map(|line| tab.window.bounds(line).expect("the line is laid out"))
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(
        (lines[0].size.width.0 - lines[1].size.width.0).abs() < 0.5,
        "two tabs are as wide as four spaces: {lines:?}"
    );

    // Every line ends in nine one-column characters, and the second line is thirteen of them.
    let column = lines[1].size.width.0 / 13.0;
    for (at, line) in lines.iter().enumerate() {
        let middle = line.origin.y + DevicePx(line.size.height.0 / 2.0);
        let right = line.origin.x.0 + line.size.width.0;
        tab.window.click_at(Point::new(DevicePx(right - 8.7 * column), middle));
        tab.window.click_at_with(Point::new(DevicePx(right + 2.0 * column), middle), Modifiers::SHIFT);
        tab.settle();
        let selected = tab.window.texts_by_class("wt-code-sel").concat();
        assert_eq!(selected, format!("value = {}", at + 1), "the press lands where the text is drawn");
    }
}

/// Each indent level has a guide, and a blank line keeps the guides of the line above it.
#[test]
fn the_viewer_draws_indent_guides() {
    let tab = open_text(options(), "guides.xml", "a\n    b\n\n\tc");
    let guides = tab.window.all_by_class("wt-code-line__guide");
    let xs: Vec<f32> =
        guides.into_iter().map(|guide| tab.window.bounds(guide).expect("the guide is laid out").origin.x.0).collect();
    assert_eq!(xs.len(), 5, "two guides on the indented line and the blank one, one on the tab: {xs:?}");
    assert!(xs[1] > xs[0], "the second level's guide is right of the first: {xs:?}");
    assert_eq!(xs[2..4], xs[0..2], "the blank line repeats the guides above it");
    assert_eq!(xs[4], xs[0], "a tab is one indent level");
}

/// The viewer with a selection across lines and the find bar marking its matches.
#[test]
#[ignore = "draws on the graphics device; run by hand to review the viewer"]
fn review_viewer_selection_and_find() {
    for (appearance, name) in [(Appearance::Dark, "unpacker-viewer-dark"), (Appearance::Light, "unpacker-viewer-light")]
    {
        let mut tab = open_ship_xml(review_options(appearance));
        let text = tab.window.by_test_id("unpacker-viewer-text").expect("the XML opens as text");
        let bounds = tab.window.bounds(text).expect("the text is laid out");
        let to = Point::new(bounds.origin.x + DevicePx(120.0), bounds.origin.y + DevicePx(27.0));
        tab.window.drag(text, to, 8).expect("the text can be dragged across");
        tab.window.press_character('f', Modifiers::PRIMARY);
        tab.settle();
        tab.window.type_text("hull");
        settle_in_real_time(&mut tab.window);
        println!("{}", save_shot(&mut tab.window, name).display());
    }
}

/// A drag across the text selects its characters, ctrl+A selects all of it and Escape lets go.
#[test]
fn the_viewer_text_is_selected_by_dragging_and_by_select_all() {
    let mut tab = open_ship_xml(options());
    let text = tab.window.by_test_id("unpacker-viewer-text").expect("the XML opens as text");
    let bounds = tab.window.bounds(text).expect("the text is laid out");
    // From the middle of the list, below the last line, to a few characters into the first.
    let to = Point::new(bounds.origin.x + DevicePx(80.0), bounds.origin.y + DevicePx(9.0));
    tab.window.drag(text, to, 8).expect("the text can be dragged across");
    tab.settle();
    let selected = tab.window.texts_by_class("wt-code-sel").concat();
    assert!(selected.contains("<ship>"), "the lines between the ends are selected: {selected:?}");
    assert!(!selected.contains("<?xml"), "the first line is selected from where the drag ended: {selected:?}");

    tab.window.press_character('a', Modifiers::PRIMARY);
    tab.settle();
    let selected = tab.window.texts_by_class("wt-code-sel").concat();
    assert!(selected.starts_with("<?xml"), "ctrl+A selects from the first character: {selected:?}");

    tab.window.press_key(NamedKey::Escape, Modifiers::NONE);
    tab.settle();
    assert!(tab.window.texts_by_class("wt-code-sel").is_empty(), "Escape clears the selection");
}

/// Ctrl+F opens a find bar that marks every match and steps through them, wrapping at the ends.
#[test]
fn the_viewer_finds_text_and_steps_through_the_matches() {
    let mut tab = open_ship_xml(options());
    let text = tab.window.by_test_id("unpacker-viewer-text").expect("the XML opens as text");
    tab.window.click(text).expect("the text can be pressed");
    tab.window.press_character('f', Modifiers::PRIMARY);
    tab.settle();
    assert!(tab.shown("unpacker-find"), "ctrl+F opens the find bar");

    tab.window.type_text("HULL");
    tab.settle();
    let count = |tab: &UnpackerWindow| tab.window.texts_by_class("wt-text__find-count").concat();
    let marked = tab.window.texts_by_class("wt-code-match").concat();
    assert_eq!(marked, "hullhullhull", "every match is marked, in any case");
    assert_eq!(count(&tab), "1 of 3");

    tab.window.press_key(NamedKey::Enter, Modifiers::NONE);
    tab.settle();
    assert_eq!(count(&tab), "2 of 3", "Enter goes to the next match");
    tab.window.press_key(NamedKey::Enter, Modifiers::SHIFT);
    tab.window.press_key(NamedKey::Enter, Modifiers::SHIFT);
    tab.settle();
    assert_eq!(count(&tab), "3 of 3", "shift+Enter goes back, wrapping to the last match");
    assert_eq!(tab.window.texts_by_class("wt-code-current"), vec!["hull".to_owned()]);

    tab.window.type_text("zzz");
    tab.settle();
    assert_eq!(count(&tab), "No matches");

    tab.window.press_key(NamedKey::Escape, Modifiers::NONE);
    tab.settle();
    assert!(!tab.shown("unpacker-find"), "Escape closes the find bar");
}

/// Ctrl+F in the viewer with the find bar up and its query typed.
fn find_in(tab: &mut UnpackerWindow, query: &str) {
    let text = tab.window.by_test_id("unpacker-viewer-text").expect("the file opens as text");
    tab.window.click(text).expect("the text can be pressed");
    tab.window.press_character('f', Modifiers::PRIMARY);
    tab.settle();
    tab.window.type_text(query);
    tab.settle();
}

fn find_count(tab: &UnpackerWindow) -> String {
    tab.window.texts_by_class("wt-text__find-count").concat()
}

fn disabled(tab: &mut UnpackerWindow, test_id: &str) -> bool {
    let element = tab.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
    tab.window.a11y(element).is_some_and(|state| state.disabled)
}

/// The case toggle limits the matches to the query's own case, and the step buttons only work
/// while there is a match to step to.
#[test]
fn the_find_bar_matches_case_on_request() {
    let mut tab = open_text(options(), "case.xml", "Hull hull HULL\n");
    find_in(&mut tab, "hull");
    assert_eq!(find_count(&tab), "1 of 3", "case is ignored to begin with");
    assert!(!disabled(&mut tab, "unpacker-find-next"));

    tab.click("unpacker-find-match-case");
    assert_eq!(find_count(&tab), "1 of 1", "only the lower-case match is left");
    assert_eq!(tab.window.texts_by_class("wt-code-current"), vec!["hull".to_owned()]);

    tab.click("unpacker-find-match-case");
    assert_eq!(find_count(&tab), "1 of 3", "the toggle lets go again");

    let field = tab.window.by_test_id("unpacker-find-field").expect("the find field is up");
    tab.window.click(field).expect("the field can be pressed");
    tab.window.type_text("zzz");
    tab.settle();
    assert!(disabled(&mut tab, "unpacker-find-previous"), "no match, nothing to step back to");
    assert!(disabled(&mut tab, "unpacker-find-next"), "no match, nothing to step on to");
}

/// Text selected on one line is what ctrl+F looks for.
#[test]
fn find_starts_from_the_selected_text() {
    let mut tab = open_text(options(), "pick.xml", "speed\nhull speed\n");
    let first = tab.window.all_by_class("wt-code-line__text")[0];
    let line = tab.window.bounds(first).expect("the line is laid out");
    let middle = line.origin.y + DevicePx(line.size.height.0 / 2.0);
    tab.window.click_at(Point::new(line.origin.x + DevicePx(1.0), middle));
    tab.window.click_at_with(Point::new(line.origin.x + DevicePx(line.size.width.0 + 4.0), middle), Modifiers::SHIFT);
    tab.settle();
    assert_eq!(tab.window.texts_by_class("wt-code-sel").concat(), "speed");

    tab.window.press_character('f', Modifiers::PRIMARY);
    tab.settle();
    let field = tab.window.by_test_id("unpacker-find-field").expect("the find field is up");
    let query = tab.window.a11y(field).and_then(|state| state.value);
    assert_eq!(query.as_deref(), Some("speed"), "the selection is the query");
    assert_eq!(find_count(&tab), "1 of 2");
}

/// The text's context menu copies the selection, once there is one, and selects all of it.
#[test]
fn the_viewer_context_menu_selects_all_and_copies() {
    let mut tab = open_ship_xml(options());
    let text = tab.window.by_test_id("unpacker-viewer-text").expect("the XML opens as text");
    tab.window.right_click(text).expect("the text can be right-clicked");
    tab.settle();
    let copy_label = t!("ui.buttons.copy").into_owned();
    let copy = tab.window.by_role_and_label(Role::MenuItem, &copy_label).expect("the menu offers Copy");
    assert!(tab.window.a11y(copy).is_some_and(|state| state.disabled), "nothing is selected to copy");

    let select_all_label = t!("ui.buttons.select_all").into_owned();
    let select_all =
        tab.window.by_role_and_label(Role::MenuItem, &select_all_label).expect("the menu offers Select All");
    tab.window.click(select_all).expect("Select All can be chosen");
    tab.settle();
    let selected = tab.window.texts_by_class("wt-code-sel").concat();
    assert!(selected.starts_with("<?xml") && selected.contains("</ship>"), "every line is selected: {selected:?}");

    tab.window.right_click(text).expect("the text can be right-clicked");
    tab.settle();
    let copy = tab.window.by_role_and_label(Role::MenuItem, &copy_label).expect("the menu offers Copy");
    assert!(tab.window.a11y(copy).is_some_and(|state| !state.disabled), "the selection can be copied");
}

/// The words beside the decode box tick and untick it, as the box itself does.
#[test]
fn the_decode_prototypes_label_toggles_its_box() {
    let mut tab = UnpackerWindow::open(options());
    let before = tab.checked("unpacker-decode-prototypes");
    tab.click("unpacker-decode-prototypes-label");
    assert_ne!(tab.checked("unpacker-decode-prototypes"), before, "the label ticks the box");
    tab.click("unpacker-decode-prototypes-label");
    assert_eq!(tab.checked("unpacker-decode-prototypes"), before, "and unticks it");
}

/// A listing as an install looks: nested directories, files of several kinds, some queued, a
/// search with hits, and an open XML file.
fn review_fixture() -> VfsPath {
    let root: VfsPath = MemoryFS::new().into();
    for dir in ["content/gameplay/common", "content/GameParams", "gui/maps", "gui/ship_icons", "scripts/entities"] {
        root.join(dir).unwrap().create_dir_all().unwrap();
    }
    write(&root, "content/gameplay/common/ship_params.xml", SHIP_XML);
    write(&root, "content/gameplay/common/consumables.json", "{\"engineBoost\": {\"workTime\": 60, \"reload\": 180}}");
    write(&root, "content/GameParams.data", "binary");
    write(&root, "gui/maps/00_CO_ocean.png", "png");
    write(&root, "gui/ship_icons/PASD013.png", "png");
    write(&root, "scripts/entities/Avatar.def", "<root/>");
    write(&root, "readme.txt", "World of Warships resources");
    for ix in 0..12 {
        write(&root, &format!("content/gameplay/common/space_{ix:02}.xml"), "<space><name>ocean</name></space>");
    }
    root
}

const SHIP_XML: &str = "<?xml version=\"1.0\"?>\n<ship>\n  <!-- hull parameters -->\n  <hull name=\"A\" tier=\"10\">\n    <speed>35.5</speed>\n    <health>51300</health>\n  </hull>\n</ship>\n";

fn review(appearance: Appearance, name: &str) {
    let mut tab = UnpackerWindow::with_vfs(review_options(appearance), review_fixture());
    let model = tab.model;
    model.pkg.select_dir("/content/gameplay/common".to_string());
    model.pkg.expanded.set(
        ["/", "/content", "/content/gameplay"]
            .into_iter()
            .map(|dir| crate::tabs::unpacker::pane::DirKey(dir.to_string()))
            .collect(),
    );
    model.adopt_output_dir("/home/reader/wows-extracted".to_string());
    tab.settle();
    let rows = model.pkg.rows.with_untracked(|rows| rows.all());
    for entry in rows.iter().take(3) {
        model.queue_path(entry.path.clone());
    }
    let files = model.pkg.files().expect("the pane has a VFS");
    model.open_search(BrowserSource::Pkg, "ocean".to_string(), String::new(), files);
    let ship = rows.iter().find(|row| row.label == "ship_params.xml").expect("the fixture has a ship file");
    model.open_viewer(ship.path.clone());
    model.dock.activate(&landaire_ui::PanelId::new(BrowserSource::Pkg.id_fragment())).expect("the pane is docked");
    settle_in_real_time(&mut tab.window);
    save_shot(&mut tab.window, name);
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn review_unpacker_dark() {
    review(Appearance::Dark, "unpacker-dark");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn review_unpacker_light() {
    review(Appearance::Light, "unpacker-light");
}

#[test]
fn revealing_a_file_selects_its_row() {
    let vfs = fixture();
    let mut tab = UnpackerWindow::with_vfs(options(), vfs.clone());

    tab.model.pkg.reveal(&vfs.join("content/gameplay/b.xml").unwrap());
    tab.settle();
    assert_eq!(tab.model.pkg.listing_selection.get_untracked(), HashSet::from([RowKey(0)]));

    // Directories sort first, so a.xml is the second row of its directory.
    tab.model.pkg.select_dir("/content".to_string());
    tab.settle();
    tab.model.pkg.reveal(&vfs.join("content/a.xml").unwrap());
    tab.settle();
    assert_eq!(tab.model.pkg.listing_selection.get_untracked(), HashSet::from([RowKey(1)]));
}

#[test]
fn another_tab_can_ask_for_a_package_file_to_be_shown() {
    let mut tab = UnpackerWindow::with_vfs(options(), fixture());
    let shell = tab.model.shell;

    shell.palette.send(shell, TabRequest::RevealInUnpacker { path: "content/gameplay/b.xml".to_string() });
    tab.settle();

    assert_eq!(tab.model.pkg.selected_dir.get_untracked(), "/content/gameplay");
    assert_eq!(tab.model.pkg.listing_selection.get_untracked(), HashSet::from([RowKey(0)]));
}

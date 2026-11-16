//! The gallery's tree section: a file hierarchy with one row selected, and a grouped list of
//! two-line rows.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use landaire_ui::BranchClick;
use landaire_ui::LeafLead;
use landaire_ui::RowHeight;
use landaire_ui::TreeModel;
use landaire_ui::TreeProps;
use landaire_ui::TreeRow;
use landaire_ui::TreeStripes;
use landaire_ui::test_support::WindowSize;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;

use crate::Section;

pub const SECTION: Section =
    Section { name: "tree", title: "Tree", page: WindowSize { width: 1280.0, height: 300.0 }, view };

const SHEET: &str = css!(
    ".gallery-tree { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery-tree__frame { display: flex; flex-direction: column; width: 280px; height: 200px;
                            border: 1px solid var(--lui-color-border); }
     .gallery-tree__lines { display: flex; flex-direction: column; justify-content: center; min-width: 0; }
     .gallery-tree__line-dim { font-size: 12px; color: var(--lui-color-text-dim); }
     .gallery-tree__thumb { flex: none; box-sizing: border-box; width: 27px; height: 27px;
                            border-radius: var(--lui-radius); border: 1px solid var(--lui-color-border);
                            background-color: var(--lui-color-faint); }"
);

/// A hierarchy of named nodes.
struct Outline {
    roots: Vec<u32>,
    children: HashMap<u32, Vec<u32>>,
    parents: HashMap<u32, u32>,
    names: HashMap<u32, (&'static str, &'static str)>,
}

impl Outline {
    /// Each node is a key, its parent, its name and a second line.
    fn new(nodes: &[(u32, Option<u32>, &'static str, &'static str)]) -> Self {
        let mut outline =
            Self { roots: Vec::new(), children: HashMap::new(), parents: HashMap::new(), names: HashMap::new() };
        for &(key, parent, name, detail) in nodes {
            match parent {
                Some(parent) => {
                    outline.children.entry(parent).or_default().push(key);
                    outline.parents.insert(key, parent);
                }
                None => outline.roots.push(key),
            }
            outline.names.insert(key, (name, detail));
        }

        outline
    }

    fn name(&self, key: u32) -> &'static str {
        self.names.get(&key).map_or("", |(name, _)| name)
    }

    fn detail(&self, key: u32) -> &'static str {
        self.names.get(&key).map_or("", |(_, detail)| detail)
    }
}

impl TreeModel for Outline {
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
        self.names.get(key).map(|(name, _)| *name)
    }
}

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-tree", SHEET);

    let files = Rc::new(Outline::new(&[
        (1, None, "Projects", ""),
        (10, Some(1), "Website", ""),
        (100, Some(10), "index.html", ""),
        (101, Some(10), "style.css", ""),
        (102, Some(10), "logo.svg", ""),
        (11, Some(1), "Reports", ""),
        (110, Some(11), "2026-q1.pdf", ""),
        (111, Some(11), "2026-q2.pdf", ""),
        (12, Some(1), "notes.txt", ""),
        (2, None, "Music", ""),
        (20, Some(2), "playlist.m3u", ""),
        (3, None, "readme.md", ""),
    ]));
    let file_names = Rc::clone(&files);
    let files_open = RwSignal::new_local(HashSet::from([1, 10, 11]));
    let file_selection = RwSignal::new_local(HashSet::from([101]));

    let sessions = Rc::new(Outline::new(&[
        (1, None, "Today", ""),
        (10, Some(1), "Session 14", "12 entries, 4 min"),
        (11, Some(1), "Session 13", "8 entries, 2 min"),
        (12, Some(1), "Session 12", "31 entries, 11 min"),
        (2, None, "Yesterday", ""),
        (20, Some(2), "Session 11", "5 entries, 1 min"),
        (21, Some(2), "Session 10", "17 entries, 6 min"),
    ]));
    let session_lines = Rc::clone(&sessions);
    let sessions_open = RwSignal::new_local(HashSet::from([1, 2]));
    let session_selection = RwSignal::new_local(HashSet::from([11, 12]));

    AnyView::new(view! {
        row(class = "gallery-tree") {
            box(class = "gallery-tree__frame") {
                Tree(
                    model = Signal::stored_local(files),
                    row = move |row: TreeRow<u32>| file_names.name(row.key),
                    expanded = files_open,
                    selection = file_selection,
                    label = "Files"
                )
            }
            box(class = "gallery-tree__frame") {
                Tree(
                    model = Signal::stored_local(sessions),
                    row = move |row: TreeRow<u32>| {
                        let name = session_lines.name(row.key);
                        let detail = session_lines.detail(row.key);
                        if detail.is_empty() {
                            return AnyView::new(name);
                        }
                        AnyView::new(view! {
                            row(style:gap = "6px", style:align-items = "center") {
                                box(class = "gallery-tree__thumb")
                                column(class = "gallery-tree__lines") {
                                    text {{name}}
                                    text(class = "gallery-tree__line-dim") {{detail}}
                                }
                            }
                        })
                    },
                    row_height = RowHeight(35.0),
                    expanded = sessions_open,
                    selection = session_selection,
                    stripes = TreeStripes::Leaves,
                    leaf_lead = LeafLead::Nothing,
                    branch_click = BranchClick::SelectAndToggle,
                    label = "Sessions"
                )
            }
        }
    })
}

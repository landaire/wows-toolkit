//! Every scheme's palette, and the surfaces in one of them.
//!
//! `cargo run -p landaire-ui --features test-support --example gallery` opens a window that
//! follows the desktop. With `-- --png <directory>` it writes `gallery-dark.png` and
//! `gallery-light.png` there instead.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use landaire_ui::BranchClick;
use landaire_ui::CardSurfaceProps;
use landaire_ui::CollapseButtonProps;
use landaire_ui::ColumnAlign;
use landaire_ui::ColumnId;
use landaire_ui::ColumnLayout;
use landaire_ui::ColumnSizing;
use landaire_ui::ColumnSort;
use landaire_ui::ColumnWidth;
use landaire_ui::ExpandRailProps;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::GridSelectionMode;
use landaire_ui::IndentGuidesProps;
use landaire_ui::IndentWidth;
use landaire_ui::LeafLead;
use landaire_ui::OverlaySurfaceProps;
use landaire_ui::PanelSide;
use landaire_ui::RowHeight;
use landaire_ui::RowMode;
use landaire_ui::Scheme;
use landaire_ui::Selection;
use landaire_ui::SortDirection;
use landaire_ui::StripedRowsProps;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::Tier;
use landaire_ui::TreeDepth;
use landaire_ui::TreeModel;
use landaire_ui::TreeProps;
use landaire_ui::TreeRow;
use landaire_ui::TreeStripes;
use landaire_ui::WorkspaceInsetProps;
use landaire_ui::test_support::DisplayScale;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::WindowSize;
use landaire_ui::test_support::render_png;
use zgui::component;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::BadgeProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonVariant;
use zgui_ui::SwitchProps;

use crate::chrome::ChromeProps;

#[path = "gallery_parts/chrome.rs"]
mod chrome;

const SHEET: &str = css!(
    ".gallery { display: flex; flex-direction: column; gap: 12px; padding: 12px; overflow: hidden;
                font-size: var(--lui-font-size); font-family: var(--lui-font-family); }
     .gallery__heading { font-size: 14px; font-weight: 600; color: var(--lui-color-text-bright); }
     .gallery__section { font-size: 12px; font-weight: 600; color: var(--lui-color-text-dim); }
     .gallery__schemes { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
     .gallery__scheme { display: flex; flex-direction: column; gap: 6px; padding: 8px; }
     .gallery__sample { display: flex; flex-direction: row; gap: 8px; padding: 6px 8px;
                        border-radius: var(--lui-radius); }
     .gallery__tiers { display: flex; flex-direction: row; gap: 2px; }
     .gallery__tier { width: 18px; height: 18px; border-radius: var(--lui-radius);
                      border: 1px solid rgb(127 127 127 / 0.35); }
     .gallery__surfaces { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery__column { display: flex; flex-direction: column; gap: 8px; }
     .gallery__padded { padding: 10px; display: flex; flex-direction: column; gap: 6px; }
     .gallery__rows { width: 220px; border: 1px solid var(--lui-color-border); }
     .gallery__row { display: flex; flex-direction: row; justify-content: space-between;
                     padding: 2px 8px; }
     .gallery__tree { width: 200px; border: 1px solid var(--lui-color-border); }
     .gallery__node { display: flex; flex-direction: row; height: 20px; align-items: center; }
     .gallery__node-label { padding-left: 4px; }
     .gallery__dim { color: var(--lui-color-text-dim); }
     .gallery__controls { display: flex; flex-direction: row; gap: 8px; align-items: center; }
     .gallery__grid { display: flex; width: 560px; height: 236px; border: 1px solid var(--lui-color-border); }"
);

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let directory = match (arguments.next().as_deref(), arguments.next()) {
        (None, _) => None,
        (Some("--png"), Some(directory)) => Some(PathBuf::from(directory)),
        _ => {
            eprintln!("usage: gallery [--png <directory>]");
            return ExitCode::FAILURE;
        }
    };

    let outcome = match directory {
        Some(directory) => write_pngs(&directory),
        None => zgui::app()
            .with_title("landaire-ui gallery")
            .with_size(1280.0, 860.0)
            .run(|| view! { Gallery(choice = ThemeChoice::System) })
            .map_err(|error| error.to_string()),
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn write_pngs(directory: &Path) -> Result<(), String> {
    let options = HeadlessOptions {
        scale: DisplayScale(1.0),
        ..HeadlessOptions::new(WindowSize { width: 1280.0, height: 860.0 })
    };

    for (scheme, name) in [(Scheme::Dark, "gallery-dark.png"), (Scheme::Light, "gallery-light.png")] {
        let path = directory.join(name);
        render_png(options, move || view! { Gallery(choice = ThemeChoice::Fixed(scheme)) }, &path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

#[component]
fn Gallery(choice: ThemeChoice) -> impl IntoView {
    install_stylesheet("landaire-ui-gallery", SHEET);

    let schemes: Vec<AnyView> =
        Scheme::ALL.into_iter().map(|scheme| AnyView::new(view! { SchemeCard(scheme = scheme) })).collect();

    view! {
        ThemeRoot(choice = choice) {
            WorkspaceInset {
                column(class = "gallery") {
                    text(class = "gallery__heading") {{format!("landaire-ui: {}", choice.label())}}
                    text(class = "gallery__section") {"Schemes"}
                    box(class = "gallery__schemes") {{schemes}}
                    text(class = "gallery__section") {"Surfaces"}
                    Surfaces()
                    text(class = "gallery__section") {"Chrome"}
                    Chrome()
                    text(class = "gallery__section") {"Tree"}
                    TreeSample()
                    text(class = "gallery__section") {"Grid"}
                    GridSample()
                }
            }
        }
    }
}

/// One scheme's tiers in order, and its body text on its panel.
#[component]
fn SchemeCard(scheme: Scheme) -> impl IntoView {
    let palette = scheme.palette();
    let tiers: Vec<AnyView> = Tier::ALL
        .into_iter()
        .map(|tier| {
            let color = palette.tier(tier).to_string();
            AnyView::new(view! { box(class = "gallery__tier", style:background-color = color) })
        })
        .collect();
    let sample = format!("background-color: {}; color: {}", palette.panel, palette.text);
    let accent = format!("color: {}", palette.accent);
    let dim = format!("color: {}", palette.text_dim);

    view! {
        CardSurface(class = "gallery__scheme") {
            text {{scheme.label()}}
            row(class = "gallery__tiers") {{tiers}}
            row(class = "gallery__sample", style = sample) {
                text {"Body 1,234.5"}
                text(style = dim) {"dim"}
                text(style = accent) {"accent"}
            }
        }
    }
}

/// Each surface once, in the theme around it.
#[component]
fn Surfaces() -> impl IntoView {
    let rows: Vec<AnyView> =
        [("Alpha", 1204.5), ("Bravo", 87.25), ("Charlie", 15.0), ("Delta", 3300.75), ("Echo", 9.5)]
            .into_iter()
            .map(|(name, figure)| {
                AnyView::new(view! {
                    row(class = "gallery__row") {
                        text {{name}}
                        text(class = TABULAR_CLASS) {{format!("{figure:.2}")}}
                    }
                })
            })
            .collect();
    let tree: Vec<AnyView> = [(0, "Root"), (1, "Group"), (2, "Item"), (2, "Item"), (1, "Group"), (2, "Item")]
        .into_iter()
        .map(|(depth, label)| {
            AnyView::new(view! {
                row(class = "gallery__node") {
                    IndentGuides(depth = TreeDepth(depth), indent = IndentWidth(14.0))
                    text(class = "gallery__node-label") {{label}}
                }
            })
        })
        .collect();
    let on = RwSignal::new_local(true);

    view! {
        row(class = "gallery__surfaces") {
            column(class = "gallery__column") {
                CardSurface(class = "gallery__padded") {
                    text {"Card"}
                    text(class = "gallery__dim") {"One step above the panel"}
                }
                OverlaySurface(class = "gallery__padded") {
                    text {"Overlay"}
                    text(class = "gallery__dim") {"A popover the application draws"}
                }
            }
            StripedRows(class = "gallery__rows") {{rows}}
            column(class = "gallery__tree") {{tree}}
            column(class = "gallery__column") {
                row(class = "gallery__controls") {
                    CollapseButton(side = PanelSide::Left, label = "Collapse panel")
                    ExpandRail(side = PanelSide::Right, label = "Expand panel")
                    CollapseButton(side = PanelSide::Bottom, label = "Collapse bottom panel")
                }
                row(class = "gallery__controls") {
                    Button {"Primary"}
                    Button(variant = ButtonVariant::Secondary) {"Secondary"}
                    Button(variant = ButtonVariant::Outline) {"Outline"}
                }
                row(class = "gallery__controls") {
                    Switch(checked = on, a11y:label = "Enabled")
                    Badge {"Badge"}
                }
            }
        }
    }
}

const TREE_SHEET: &str = css!(
    ".gallery__trees { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery__tree-frame { display: flex; flex-direction: column; width: 280px; height: 200px;
                            border: 1px solid var(--lui-color-border); }
     .gallery__lines { display: flex; flex-direction: column; justify-content: center; min-width: 0; }
     .gallery__line-dim { font-size: 12px; color: var(--lui-color-text-dim); }
     .gallery__thumb { flex: none; box-sizing: border-box; width: 27px; height: 27px;
                       border-radius: var(--lui-radius); border: 1px solid var(--lui-color-border);
                       background-color: var(--lui-color-faint); }"
);

/// A hierarchy of named nodes for the tree section.
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

/// A file hierarchy with one row selected, and a grouped list of two-line rows.
#[component]
fn TreeSample() -> impl IntoView {
    install_stylesheet("landaire-ui-gallery-tree", TREE_SHEET);

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

    view! {
        row(class = "gallery__trees") {
            box(class = "gallery__tree-frame") {
                Tree(
                    model = Signal::stored_local(files),
                    row = move |row: TreeRow<u32>| file_names.name(row.key),
                    expanded = files_open,
                    selection = file_selection,
                    label = "Files"
                )
            }
            box(class = "gallery__tree-frame") {
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
                                box(class = "gallery__thumb")
                                column(class = "gallery__lines") {
                                    text {{name}}
                                    text(class = "gallery__line-dim") {{detail}}
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
    }
}

/// One running program, as a task manager lists it.
#[derive(Clone, Debug, PartialEq)]
struct Process {
    pid: u32,
    name: &'static str,
    user: &'static str,
    cpu: f32,
    memory: u32,
    threads: u32,
    state: &'static str,
}

/// A grid with its first column pinned, a resized column, a sort and two rows selected. A double
/// click opens a row.
#[component]
fn GridSample() -> impl IntoView {
    let all = vec![
        Process { pid: 412, name: "compositor", user: "root", cpu: 3.2, memory: 182, threads: 14, state: "Running" },
        Process { pid: 1088, name: "indexer", user: "daemon", cpu: 11.7, memory: 640, threads: 6, state: "Running" },
        Process { pid: 2210, name: "editor", user: "alice", cpu: 0.4, memory: 1210, threads: 31, state: "Sleeping" },
        Process {
            pid: 2231,
            name: "language-server",
            user: "alice",
            cpu: 7.9,
            memory: 905,
            threads: 18,
            state: "Running",
        },
        Process { pid: 3007, name: "backup", user: "root", cpu: 0.0, memory: 54, threads: 2, state: "Stopped" },
        Process { pid: 3150, name: "browser", user: "alice", cpu: 22.5, memory: 2480, threads: 64, state: "Running" },
        Process { pid: 3302, name: "mail", user: "alice", cpu: 0.1, memory: 310, threads: 9, state: "Sleeping" },
    ];
    let initial = Some(ColumnSort { column: ColumnId::from("cpu"), direction: SortDirection::Descending });
    let rows = RwSignal::new_local(sorted(all.clone(), initial.as_ref()));
    let sort = RwSignal::new_local(initial);
    let on_sort = UnsyncCallback::new(move |next: Option<ColumnSort>| {
        rows.set(sorted(all.clone(), next.as_ref()));
        sort.set(next);
    });

    let columns = vec![
        GridColumn::new("name", "Name", |process: &Process| process.name)
            .sortable(true)
            .text(|process| process.name.to_owned()),
        GridColumn::new(
            "pid",
            "PID",
            |process: &Process| view! { text(class = TABULAR_CLASS) {{process.pid.to_string()}} },
        )
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new("user", "User", |process: &Process| process.user),
        GridColumn::new(
            "cpu",
            "CPU %",
            |process: &Process| view! { text(class = TABULAR_CLASS) {{format!("{:.1}", process.cpu)}} },
        )
        .align(ColumnAlign::End)
        .sortable(true),
        GridColumn::new(
            "memory",
            "Memory",
            |process: &Process| view! { text(class = TABULAR_CLASS) {{format!("{} MB", process.memory)}} },
        )
        .align(ColumnAlign::End)
        .sizing(ColumnSizing::Fixed(ColumnWidth(96.0)))
        .sortable(true),
        GridColumn::new("threads", "Threads", |process: &Process| process.threads.to_string()).align(ColumnAlign::End),
        GridColumn::new("state", "State", |process: &Process| process.state),
    ];
    let mut arranged = ColumnLayout::new(columns.iter().map(|column| column.id().clone()));
    arranged.pin(&ColumnId::from("name"));
    arranged.set_width(&ColumnId::from("user"), Some(ColumnWidth(72.0)));
    let layout = RwSignal::new_local(arranged);
    let mut chosen = Selection::new();
    chosen.select_only(3150);
    chosen.toggle(2231);
    let selection = RwSignal::new_local(chosen);
    let detail = RowMode::with_detail(|process: &Process| {
        view! {
            text(class = "gallery__dim") {{format!("{} threads, {} MB resident, owned by {}", process.threads, process.memory, process.user)}}
        }
    });

    view! {
        box(class = "gallery__grid") {
            Grid(
                rows = rows,
                columns = columns,
                row_key = |process: &Process| process.pid,
                mode = detail,
                layout = layout,
                sort = sort,
                on_sort = on_sort,
                selection_mode = GridSelectionMode::Multiple,
                selection = selection,
                label = "Processes"
            )
        }
    }
}

/// `rows` in the order `sort` asks for.
fn sorted(mut rows: Vec<Process>, sort: Option<&ColumnSort>) -> Vec<Process> {
    let Some(sort) = sort else {
        return rows;
    };

    rows.sort_by(|a, b| {
        let order = match sort.column.as_str() {
            "name" => a.name.cmp(b.name),
            "pid" => a.pid.cmp(&b.pid),
            "cpu" => a.cpu.total_cmp(&b.cpu),
            "memory" => a.memory.cmp(&b.memory),
            _ => std::cmp::Ordering::Equal,
        };
        match sort.direction {
            SortDirection::Ascending => order,
            SortDirection::Descending => order.reverse(),
        }
    });

    rows
}

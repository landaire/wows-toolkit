use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nsh_base::{
    DataTable, DataTableApi, DataTableClasses, DataTableColumn, DataTableRows, DataTableSelection, DataTableSort,
    DataTableSortDirection, data_table,
};
use nsh_core::AccessibilityState;
use nsh_core::file_dialog::{FileFilter, OpenDialog, SaveDialog};
use nsh_core::prelude::*;
use wows_core::game_types::GameParamId;
use wows_replays::ReplayFile;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplayPlayer {
    name: String,
    ship_id: GameParamId,
    relation: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplayDocument {
    path: PathBuf,
    file_name: String,
    date: String,
    date_iso: Option<String>,
    map_name: String,
    map_key: String,
    game_mode: String,
    scenario: String,
    player_name: String,
    player_ship: String,
    client_version: String,
    client_version_key: Option<ClientVersionKey>,
    duration_seconds: u32,
    players_per_team: u32,
    teams_count: u32,
    players: Vec<ReplayPlayer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplayDirectory {
    path: PathBuf,
    documents: Vec<Arc<ReplayDocument>>,
    failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ClientVersionKey(Vec<u32>);

#[derive(Clone, PartialEq)]
struct ReplayOrder(Arc<Vec<Arc<ReplayDocument>>>);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayColumn {
    File,
    Date,
    Ship,
    Map,
    Mode,
    Player,
    Version,
}

impl ReplayColumn {
    const ALL: [ReplayColumn; 7] = [
        ReplayColumn::File,
        ReplayColumn::Date,
        ReplayColumn::Ship,
        ReplayColumn::Map,
        ReplayColumn::Mode,
        ReplayColumn::Player,
        ReplayColumn::Version,
    ];

    fn key(self) -> &'static str {
        match self {
            ReplayColumn::File => "file",
            ReplayColumn::Date => "date",
            ReplayColumn::Ship => "ship",
            ReplayColumn::Map => "map",
            ReplayColumn::Mode => "mode",
            ReplayColumn::Player => "player",
            ReplayColumn::Version => "version",
        }
    }

    fn title(self) -> &'static str {
        match self {
            ReplayColumn::File => "Replay",
            ReplayColumn::Date => "Date",
            ReplayColumn::Ship => "Ship",
            ReplayColumn::Map => "Map",
            ReplayColumn::Mode => "Mode",
            ReplayColumn::Player => "Player",
            ReplayColumn::Version => "Version",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaySortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReplaySort {
    column: ReplayColumn,
    direction: ReplaySortDirection,
}

#[derive(Debug, Clone, PartialEq, Props)]
pub struct ReplayInspectorProps {}

#[component]
pub fn ReplayInspector(_: ReplayInspectorProps) -> impl IntoElement {
    let document = signal(None::<Arc<ReplayDocument>>);
    let directory = signal(None::<Arc<ReplayDirectory>>);
    let error = signal(None::<String>);
    let status = signal(None::<String>);
    let picking = signal(false);
    let exporting = signal(false);
    let compact = signal(false);

    view()
        .app_class(|| Some("flex-1 min-h-0 p-3 gap-3".into()))
        .child(
            view()
                .app_class(|| Some("flex-row items-center justify-end gap-2".into()))
                .child(
                    view()
                        .accessible(true)
                        .role(AccessibilityRole::Button)
                        .disabled(move || document.get().is_none() || picking.get() || exporting.get())
                        .app_class(|| {
                            Some(
                                "min-h-8 px-3 rounded-(--radius) border border-border bg-secondary \
                                 text-secondary-foreground items-center justify-center hover:opacity-85 active:opacity-70"
                                    .into(),
                            )
                        })
                        .on_press(move |_: &()| {
                            if picking.get_untracked() || exporting.get_untracked() {
                                return;
                            }
                            let Some(replay) = document.get_untracked() else {
                                return;
                            };
                            exporting.set(true);
                            error.set(None);
                            status.set(None);
                            spawn(async move {
                                match export_replay_metadata(replay).await {
                                    Ok(Some(path)) => {
                                        status.set(Some(format!("Replay metadata exported to {}.", path.display())));
                                    }
                                    Ok(None) => {}
                                    Err(message) => error.set(Some(message)),
                                }
                                exporting.set(false);
                            });
                        })
                        .child(text(move || {
                            if exporting.get() { "Exporting..." } else { "Export Metadata..." }
                        })),
                )
                .child(
                    view()
                        .accessible(true)
                        .role(AccessibilityRole::Button)
                        .disabled(move || picking.get() || exporting.get())
                        .app_class(|| {
                            Some(
                                "min-h-8 px-3 rounded-(--radius) border border-border bg-secondary \
                                 text-secondary-foreground items-center justify-center hover:opacity-85 active:opacity-70"
                                    .into(),
                            )
                        })
                        .on_press(move |_: &()| {
                            if picking.get_untracked() || exporting.get_untracked() {
                                return;
                            }
                            picking.set(true);
                            error.set(None);
                            status.set(None);
                            spawn(async move {
                                match open_replay_directory().await {
                                    Ok(Some(listing)) => {
                                        document.set(listing.documents.first().cloned());
                                        if listing.failed > 0 {
                                            let prefix = if listing.documents.is_empty() {
                                                "No readable replay files. "
                                            } else {
                                                ""
                                            };
                                            error.set(Some(format!(
                                                "{prefix}{} replay file(s) could not be read.",
                                                listing.failed
                                            )));
                                        } else if listing.documents.is_empty() {
                                            error.set(Some("The selected directory contains no replay files.".into()));
                                        }
                                        directory.set(Some(Arc::new(listing)));
                                    }
                                    Ok(None) => {}
                                    Err(message) => error.set(Some(message)),
                                }
                                picking.set(false);
                            });
                        })
                        .child(text(move || {
                            if picking.get() { "Opening..." } else { "Open Replay Directory..." }
                        })),
                )
                .child(
                    view()
                        .accessible(true)
                        .role(AccessibilityRole::Button)
                        .disabled(move || picking.get() || exporting.get())
                        .app_class(|| {
                            Some(
                                "min-h-8 px-3 rounded-(--radius) border border-border bg-primary \
                                 text-primary-foreground items-center justify-center hover:opacity-85 active:opacity-70"
                                    .into(),
                            )
                        })
                        .on_press(move |_: &()| {
                            if picking.get_untracked() || exporting.get_untracked() {
                                return;
                            }
                            picking.set(true);
                            error.set(None);
                            status.set(None);
                            spawn(async move {
                                match open_replay().await {
                                    Ok(Some(replay)) => {
                                        directory.set(None);
                                        document.set(Some(Arc::new(replay)));
                                    }
                                    Ok(None) => {}
                                    Err(message) => error.set(Some(message)),
                                }
                                picking.set(false);
                            });
                        })
                        .child(text(move || {
                            if picking.get() { "Opening..." } else { "Open Replay File..." }
                        })),
                ),
        )
        .child(
            text(move || error.get().unwrap_or_default())
                .accessible(true)
                .role(AccessibilityRole::Alert)
                .app_class(move || {
                    Some(
                        if error.get().is_some() && document.get().is_some() {
                            "px-3 py-2 rounded-(--radius) border border-destructive text-sm text-destructive"
                        } else {
                            "hidden"
                        }
                        .into(),
                    )
                }),
        )
        .child(
            text(move || status.get().unwrap_or_default())
                .accessible(true)
                .role(AccessibilityRole::Alert)
                .app_class(move || {
                    Some(
                        if status.get().is_some() {
                            "px-3 py-2 rounded-(--radius) border border-primary bg-muted text-sm text-foreground"
                        } else {
                            "hidden"
                        }
                        .into(),
                    )
                }),
        )
        .child(
            view()
                .on_layout(move |bounds: &Bounds<Points>| compact.set(bounds.size.width.0 < 1280.0))
                .app_class(move || {
                    Some(
                        if compact.get() {
                            "flex-1 min-h-0 gap-3"
                        } else {
                            "flex-1 min-h-0 flex-row gap-3"
                        }
                        .into(),
                    )
                })
                .child(directory_browser(directory, document, compact))
                .child(
                    scroll()
                        .app_class(|| Some("flex-1 min-h-0".into()))
                        .child(empty_view(document, error))
                        .child(replay_view(document, compact)),
                ),
        )
}

fn directory_browser(
    directory: Signal<Option<Arc<ReplayDirectory>>>,
    document: Signal<Option<Arc<ReplayDocument>>>,
    compact: Signal<bool>,
) -> AnyElement {
    let sort = signal(Some(ReplaySort { column: ReplayColumn::Date, direction: ReplaySortDirection::Descending }));
    let order =
        computed(move || ReplayOrder(Arc::new(directory.with(|value| ordered_replays(value.as_deref(), sort.get())))));
    let table_ref = component_ref::<DataTable>();
    let focus_ref = table_ref;
    let sort_buttons = ReplayColumn::ALL.map(|column| {
        view()
            .accessible(true)
            .role(AccessibilityRole::Button)
            .label(move || format!("Sort by {}", replay_sort_label(column, sort.get())))
            .accessibility_state(move || AccessibilityState {
                selected: sort.get().is_some_and(|current| current.column == column),
                ..AccessibilityState::default()
            })
            .app_class(move || {
                Some(
                    if sort.get().is_some_and(|current| current.column == column) {
                        "min-h-7 px-2 rounded-(--radius) bg-primary text-xs text-primary-foreground active:opacity-70"
                    } else {
                        "min-h-7 px-2 rounded-(--radius) bg-secondary text-xs text-secondary-foreground \
                         hover:bg-muted active:opacity-70"
                    }
                    .into(),
                )
            })
            .on_press(move |_: &()| sort.set(Some(next_replay_sort(sort.get_untracked(), column))))
            .child(text(move || replay_sort_label(column, sort.get())))
    });
    let controls = scroll().horizontal().scrollbar(false).child(
        view()
            .app_class(|| Some("flex-row items-center gap-1".into()))
            .child(
                view()
                    .accessible(true)
                    .role(AccessibilityRole::Button)
                    .label("Browse replay rows")
                    .app_class(|| {
                        Some(
                            "min-h-7 px-2 rounded-(--radius) border border-border bg-secondary text-xs \
                             text-secondary-foreground hover:bg-muted active:opacity-70"
                                .into(),
                        )
                    })
                    .on_press(move |_: &()| {
                        if let Some(api) = focus_ref.api::<DataTableApi>() {
                            api.focus();
                        }
                    })
                    .child(text("Browse rows")),
            )
            .children(sort_buttons),
    );
    let data_order = order;
    let selection_order = order;
    let changed_order = order;
    let table = data_table()
        .flex_1()
        .columns(replay_columns())
        .data(move || data_order.with(|order| replay_rows(&order.0)))
        .sort(move || sort.get().map(to_table_sort))
        .selection(move || {
            let row = document.with(|current| {
                let current = current.as_ref()?;
                selection_order.with(|order| order.0.iter().position(|candidate| candidate.path == current.path))
            });
            let mut selection = DataTableSelection::default();
            selection.row = row.map(|row| row as u32);
            Some(selection)
        })
        .column_selectable(false)
        .stripe(true)
        .component_ref(&table_ref)
        .classes(replay_table_classes())
        .accessibility_label("Replay directory")
        .on_selection_change(move |selection: &Option<DataTableSelection>| {
            let selected = selection
                .and_then(|selection| selection.row)
                .and_then(|row| changed_order.with_untracked(|order| order.0.get(row as usize).cloned()));
            document.set(selected);
        });

    view()
        .app_class(move || {
            Some(
                if directory.get().is_none() {
                    "hidden"
                } else if compact.get() {
                    "h-56 min-h-48 w-full p-3 rounded-(--radius) border border-border bg-card"
                } else {
                    "w-1/2 min-h-0 p-3 rounded-(--radius) border border-border bg-card"
                }
                .into(),
            )
        })
        .child(
            text(move || {
                directory.with(|value| {
                    value.as_ref().map_or_else(String::new, |value| {
                        value.path.file_name().map_or_else(
                            || value.path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        )
                    })
                })
            })
            .app_class(|| Some("text-sm font-semibold text-card-foreground".into())),
        )
        .child(
            text(move || {
                directory.with(|value| {
                    value.as_ref().map_or_else(String::new, |value| format!("{} replay file(s)", value.documents.len()))
                })
            })
            .app_class(|| Some("mb-2 text-xs text-muted-foreground".into())),
        )
        .child(controls)
        .child(table)
        .into_element()
}

fn next_replay_sort(current: Option<ReplaySort>, column: ReplayColumn) -> ReplaySort {
    let direction = match current {
        Some(current) if current.column == column && current.direction == ReplaySortDirection::Ascending => {
            ReplaySortDirection::Descending
        }
        _ => ReplaySortDirection::Ascending,
    };
    ReplaySort { column, direction }
}

fn replay_sort_label(column: ReplayColumn, current: Option<ReplaySort>) -> String {
    match current {
        Some(current) if current.column == column => {
            let direction = match current.direction {
                ReplaySortDirection::Ascending => "ascending",
                ReplaySortDirection::Descending => "descending",
            };
            format!("{} {direction}", column.title())
        }
        _ => column.title().to_owned(),
    }
}

fn replay_columns() -> Vec<DataTableColumn> {
    ReplayColumn::ALL
        .into_iter()
        .map(|column| {
            let mut definition = DataTableColumn::default();
            definition.key = column.key().into();
            definition.title = Some(column.title().into());
            definition.resizable = Some(true);
            definition.width = Some(px(match column {
                ReplayColumn::File => 250.0,
                ReplayColumn::Date => 160.0,
                ReplayColumn::Ship => 150.0,
                ReplayColumn::Map => 140.0,
                ReplayColumn::Mode => 120.0,
                ReplayColumn::Player => 140.0,
                ReplayColumn::Version => 110.0,
            }));
            definition
        })
        .collect()
}

fn ordered_replays(directory: Option<&ReplayDirectory>, sort: Option<ReplaySort>) -> Vec<Arc<ReplayDocument>> {
    let Some(directory) = directory else {
        return Vec::new();
    };
    let mut documents = directory.documents.clone();
    if let Some(sort) = sort {
        documents.sort_by(|left, right| compare_replays(left, right, sort));
    }
    documents
}

fn replay_table_classes() -> DataTableClasses {
    let mut classes = DataTableClasses::default();
    classes.root = Some("border border-border rounded-(--radius) bg-card".into());
    classes.header = Some("bg-muted".into());
    classes.header_cell = Some("px-2 border-b border-border text-xs font-semibold text-muted-foreground".into());
    classes.header_cell_sorted = Some("bg-accent text-accent-foreground".into());
    classes.sort_indicator = Some("text-muted-foreground".into());
    classes.sort_ascending = Some("opacity-30 selected:opacity-100 selected:text-accent-foreground".into());
    classes.sort_descending = Some("opacity-30 selected:opacity-100 selected:text-accent-foreground".into());
    classes.row = Some("border-b border-border text-card-foreground".into());
    classes.row_odd = Some("bg-secondary".into());
    classes.row_hover = Some("bg-muted".into());
    classes.row_selected = Some("bg-accent text-accent-foreground".into());
    classes.cell = Some("px-2 text-xs".into());
    classes
}

fn compare_replays(left: &ReplayDocument, right: &ReplayDocument, sort: ReplaySort) -> std::cmp::Ordering {
    let ordering = if sort.column == ReplayColumn::Date {
        compare_optional_key(left.date_iso.as_ref(), right.date_iso.as_ref(), &left.date, &right.date, sort.direction)
    } else if sort.column == ReplayColumn::Version {
        compare_optional_key(
            left.client_version_key.as_ref(),
            right.client_version_key.as_ref(),
            &left.client_version,
            &right.client_version,
            sort.direction,
        )
    } else {
        directed_order(replay_field(left, sort.column), replay_field(right, sort.column), sort.direction)
    };
    ordering.then_with(|| left.path.cmp(&right.path))
}

fn compare_optional_key<T: Ord>(
    left: Option<&T>,
    right: Option<&T>,
    left_fallback: &str,
    right_fallback: &str,
    direction: ReplaySortDirection,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => directed_order(left, right, direction),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => directed_order(left_fallback, right_fallback, direction),
    }
}

fn directed_order<T: Ord + ?Sized>(left: &T, right: &T, direction: ReplaySortDirection) -> std::cmp::Ordering {
    match direction {
        ReplaySortDirection::Ascending => left.cmp(right),
        ReplaySortDirection::Descending => right.cmp(left),
    }
}

fn replay_rows(documents: &[Arc<ReplayDocument>]) -> DataTableRows {
    let mut rows = DataTableRows::new(documents.len());
    for column in ReplayColumn::ALL {
        rows = rows.texts(column.key(), documents.iter().map(|document| replay_field(document, column)));
    }
    rows
}

fn replay_field(document: &ReplayDocument, column: ReplayColumn) -> &str {
    match column {
        ReplayColumn::File => &document.file_name,
        ReplayColumn::Date => document.date_iso.as_deref().unwrap_or(&document.date),
        ReplayColumn::Ship => &document.player_ship,
        ReplayColumn::Map => &document.map_name,
        ReplayColumn::Mode => &document.game_mode,
        ReplayColumn::Player => &document.player_name,
        ReplayColumn::Version => &document.client_version,
    }
}

fn to_table_sort(sort: ReplaySort) -> DataTableSort {
    let mut table_sort = DataTableSort::default();
    table_sort.key = sort.column.key().into();
    table_sort.direction = match sort.direction {
        ReplaySortDirection::Ascending => DataTableSortDirection::Ascending,
        ReplaySortDirection::Descending => DataTableSortDirection::Descending,
    };
    table_sort
}

fn empty_view(document: Signal<Option<Arc<ReplayDocument>>>, error: Signal<Option<String>>) -> AnyElement {
    view()
        .app_class(move || {
            Some(
                if document.get().is_none() { "flex-1 items-center justify-center gap-3 p-8" } else { "hidden" }.into(),
            )
        })
        .child(text("Replay Inspector").app_class(|| Some("text-xl font-semibold text-foreground".into())))
        .child(
            text(move || {
                error.get().unwrap_or_else(|| {
                    "Open a replay file to inspect battle metadata and the player roster.".to_owned()
                })
            })
            .app_class(|| Some("text-sm text-muted-foreground".into())),
        )
        .into_element()
}

fn replay_view(document: Signal<Option<Arc<ReplayDocument>>>, compact: Signal<bool>) -> AnyElement {
    let players = each(
        move || document.with(|value| value.as_ref().map_or_else(Vec::new, |value| value.players.clone())),
        |player| player.name.clone(),
        |player, _| {
            view()
                .app_class(|| Some("min-h-6 flex-row items-center border-b border-border".into()))
                .child(text(move || player.get().name).app_class(|| Some("flex-1 text-xs text-card-foreground".into())))
                .child(
                    text(move || relation_label(player.get().relation))
                        .app_class(|| Some("w-24 text-xs text-muted-foreground".into())),
                )
                .child(
                    text(move || player.get().ship_id.to_string())
                        .app_class(|| Some("w-28 text-xs text-muted-foreground".into())),
                )
        },
    );

    view()
        .app_class(move || {
            Some(
                if document.get().is_none() {
                    "hidden"
                } else if compact.get() {
                    "gap-3"
                } else {
                    "h-full gap-3"
                }
                .into(),
            )
        })
        .child(
            view()
                .child(
                    text(move || {
                        document.with(|value| value.as_ref().map_or_else(String::new, |value| value.file_name.clone()))
                    })
                    .app_class(|| Some("text-lg font-semibold text-foreground".into())),
                )
                .child(
                    text(move || {
                        document.with(|value| {
                            value.as_ref().map_or_else(String::new, |value| value.path.display().to_string())
                        })
                    })
                    .app_class(|| Some("text-xs text-muted-foreground".into())),
                ),
        )
        .child(
            view()
                .app_class(move || Some(if compact.get() { "gap-3" } else { "flex-1 min-h-0 flex-row gap-3" }.into()))
                .child(
                    view()
                        .app_class(move || {
                            Some(
                                if compact.get() {
                                    "w-full p-3 rounded-(--radius) border border-border bg-card"
                                } else {
                                    "w-2/5 p-3 rounded-(--radius) border border-border bg-card"
                                }
                                .into(),
                            )
                        })
                        .child(
                            text("Battle").app_class(|| Some("mb-2 text-sm font-semibold text-card-foreground".into())),
                        )
                        .child(metadata_row("Date", move || {
                            document.with(|value| value.as_ref().map_or_else(String::new, |value| value.date.clone()))
                        }))
                        .child(metadata_row("Map", move || {
                            document
                                .with(|value| value.as_ref().map_or_else(String::new, |value| value.map_name.clone()))
                        }))
                        .child(metadata_row("Map key", move || {
                            document
                                .with(|value| value.as_ref().map_or_else(String::new, |value| value.map_key.clone()))
                        }))
                        .child(metadata_row("Mode", move || {
                            document
                                .with(|value| value.as_ref().map_or_else(String::new, |value| value.game_mode.clone()))
                        }))
                        .child(metadata_row("Scenario", move || {
                            document
                                .with(|value| value.as_ref().map_or_else(String::new, |value| value.scenario.clone()))
                        }))
                        .child(metadata_row("Player", move || {
                            document.with(|value| {
                                value.as_ref().map_or_else(String::new, |value| value.player_name.clone())
                            })
                        }))
                        .child(metadata_row("Ship", move || {
                            document.with(|value| {
                                value.as_ref().map_or_else(String::new, |value| value.player_ship.clone())
                            })
                        }))
                        .child(metadata_row("Duration", move || {
                            document.with(|value| {
                                value
                                    .as_ref()
                                    .map_or_else(String::new, |value| format!("{} seconds", value.duration_seconds))
                            })
                        }))
                        .child(metadata_row("Teams", move || {
                            document.with(|value| {
                                value.as_ref().map_or_else(String::new, |value| {
                                    format!("{} x {}", value.teams_count, value.players_per_team)
                                })
                            })
                        }))
                        .child(metadata_row("Client", move || {
                            document.with(|value| {
                                value.as_ref().map_or_else(String::new, |value| value.client_version.clone())
                            })
                        })),
                )
                .child(
                    view()
                        .app_class(move || {
                            Some(
                                if compact.get() {
                                    "h-72 min-h-56 p-3 rounded-(--radius) border border-border bg-card"
                                } else {
                                    "flex-1 min-h-0 p-3 rounded-(--radius) border border-border bg-card"
                                }
                                .into(),
                            )
                        })
                        .child(
                            text(move || {
                                document.with(|value| {
                                    value.as_ref().map_or_else(
                                        || "Players".to_owned(),
                                        |value| format!("Players ({})", value.players.len()),
                                    )
                                })
                            })
                            .app_class(|| Some("mb-2 text-sm font-semibold text-card-foreground".into())),
                        )
                        .child(
                            view()
                                .app_class(|| Some("h-7 flex-row items-center border-b border-border".into()))
                                .child(
                                    text("Name").app_class(|| {
                                        Some("flex-1 text-xs font-semibold text-muted-foreground".into())
                                    }),
                                )
                                .child(
                                    text("Team")
                                        .app_class(|| Some("w-24 text-xs font-semibold text-muted-foreground".into())),
                                )
                                .child(
                                    text("Ship ID")
                                        .app_class(|| Some("w-28 text-xs font-semibold text-muted-foreground".into())),
                                ),
                        )
                        .child(scroll().app_class(|| Some("flex-1 min-h-0".into())).child(players)),
                ),
        )
        .into_element()
}

fn metadata_row(label: &'static str, value: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .app_class(|| Some("min-h-7 flex-row items-center gap-2 border-b border-border".into()))
        .child(text(label).app_class(|| Some("w-28 text-xs text-muted-foreground".into())))
        .child(text(value).app_class(|| Some("flex-1 text-xs text-card-foreground".into())))
}

fn relation_label(relation: u32) -> &'static str {
    match relation {
        0 => "Player",
        1 => "Ally",
        2 => "Enemy",
        _ => "Unknown",
    }
}

async fn open_replay() -> Result<Option<ReplayDocument>, String> {
    let picked = pick_paths(
        OpenDialog::files()
            .filter(FileFilter::new("World of Warships replay", ["wowsreplay"]))
            .title("Open Replay")
            .button_label("Open"),
    )
    .await
    .map_err(|error| format!("could not show replay file dialog: {error}"))?;
    let Some(path) = picked.and_then(|mut paths| paths.pop()) else {
        return Ok(None);
    };

    blocking(move || parse_replay(&path))
        .await
        .map_err(|error| format!("replay parser task failed: {error}"))?
        .map(Some)
}

async fn open_replay_directory() -> Result<Option<ReplayDirectory>, String> {
    let picked = pick_paths(OpenDialog::folders().title("Open Replay Directory").button_label("Open"))
        .await
        .map_err(|error| format!("could not show replay directory dialog: {error}"))?;
    let Some(path) = picked.and_then(|mut paths| paths.pop()) else {
        return Ok(None);
    };

    blocking(move || scan_replay_directory(path))
        .await
        .map_err(|error| format!("replay directory task failed: {error}"))?
        .map(Some)
}

fn scan_replay_directory(path: PathBuf) -> Result<ReplayDirectory, String> {
    let entries = std::fs::read_dir(&path)
        .map_err(|error| format!("could not read replay directory {}: {error}", path.display()))?;
    let mut replay_paths = Vec::new();
    let mut failed = 0;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let entry_path = entry.path();
        if entry_path.extension().and_then(|extension| extension.to_str()) != Some("wowsreplay")
            || entry_path.file_name().is_some_and(|name| name == "temp.wowsreplay")
        {
            continue;
        }
        replay_paths.push(entry_path);
    }
    replay_paths.sort_by(|left, right| right.cmp(left));

    let mut documents: Vec<_> = replay_paths
        .into_iter()
        .filter_map(|replay_path| match parse_replay(&replay_path) {
            Ok(document) => Some(Arc::new(document)),
            Err(_) => {
                failed += 1;
                None
            }
        })
        .collect();
    documents.sort_by(|left, right| {
        compare_replays(
            left,
            right,
            ReplaySort { column: ReplayColumn::Date, direction: ReplaySortDirection::Descending },
        )
    });
    Ok(ReplayDirectory { path, documents, failed })
}

fn parse_replay(path: &Path) -> Result<ReplayDocument, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("selected replay path has no file name: {}", path.display()))?
        .to_string_lossy()
        .into_owned();
    let replay = ReplayFile::meta_only_from_file(path)
        .map_err(|error| format!("could not parse replay {}: {error}", path.display()))?;
    let meta = replay.meta;
    let game_mode =
        meta.gameType.clone().or_else(|| meta.matchGroup.clone()).unwrap_or_else(|| format!("Mode {}", meta.gameMode));
    let players = meta
        .vehicles
        .iter()
        .map(|vehicle| ReplayPlayer { name: vehicle.name.clone(), ship_id: vehicle.shipId, relation: vehicle.relation })
        .collect();
    let date_iso = iso_date(&meta.dateTime);
    let client_version_key = client_version_key(&meta.clientVersionFromXml);

    Ok(ReplayDocument {
        path: path.to_path_buf(),
        file_name,
        date: meta.dateTime,
        date_iso,
        map_name: meta.mapDisplayName,
        map_key: meta.mapName,
        game_mode,
        scenario: meta.scenario,
        player_name: meta.playerName,
        player_ship: meta.playerVehicle,
        client_version: meta.clientVersionFromXml,
        client_version_key,
        duration_seconds: meta.duration,
        players_per_team: meta.playersPerTeam,
        teams_count: meta.teamsCount,
        players,
    })
}

fn client_version_key(raw: &str) -> Option<ClientVersionKey> {
    let parts = raw.split(',').map(|part| part.trim().parse::<u32>()).collect::<Result<Vec<_>, _>>().ok()?;
    (!parts.is_empty()).then_some(ClientVersionKey(parts))
}

fn iso_date(raw: &str) -> Option<String> {
    let (date, time) = raw.split_once(' ')?;
    let mut parts = date.split('.');
    let (Some(day), Some(month), Some(year), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    Some(format!("{year}-{month}-{day} {time}"))
}

async fn export_replay_metadata(document: Arc<ReplayDocument>) -> Result<Option<PathBuf>, String> {
    let stem = document
        .path
        .file_stem()
        .ok_or_else(|| format!("replay path has no file stem: {}", document.path.display()))?
        .to_string_lossy();
    let picked = pick_save_path(
        SaveDialog::new()
            .file_name(format!("{stem}.metadata.json"))
            .filter(FileFilter::new("JSON", ["json"]))
            .title("Export Replay Metadata")
            .button_label("Export"),
    )
    .await
    .map_err(|error| format!("could not show replay export dialog: {error}"))?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let overwrite_confirmed = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    let path = if overwrite_confirmed { path } else { path.with_extension("json") };

    let players: Vec<_> = document
        .players
        .iter()
        .map(|player| {
            serde_json::json!({
                "name": player.name,
                "shipId": player.ship_id.raw(),
                "relation": player.relation,
                "relationLabel": relation_label(player.relation),
            })
        })
        .collect();
    let value = serde_json::json!({
        "fileName": document.file_name,
        "date": document.date,
        "map": {
            "displayName": document.map_name,
            "key": document.map_key,
        },
        "mode": document.game_mode,
        "scenario": document.scenario,
        "player": {
            "name": document.player_name,
            "ship": document.player_ship,
        },
        "clientVersion": document.client_version,
        "durationSeconds": document.duration_seconds,
        "playersPerTeam": document.players_per_team,
        "teamsCount": document.teams_count,
        "players": players,
    });
    let bytes =
        serde_json::to_vec_pretty(&value).map_err(|error| format!("could not serialize replay metadata: {error}"))?;
    let write_path = path.clone();
    blocking(move || write_atomic(&write_path, &bytes, overwrite_confirmed))
        .await
        .map_err(|error| format!("replay export task failed: {error}"))?
        .map_err(|error| format!("could not write replay metadata to {}: {error}", path.display()))?;
    Ok(Some(path))
}

fn write_atomic(path: &Path, bytes: &[u8], overwrite_confirmed: bool) -> std::io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file_mut().sync_all()?;
    if overwrite_confirmed {
        staged.persist(path).map(|_| ()).map_err(|error| error.error)
    } else {
        staged.persist_noclobber(path).map(|_| ()).map_err(|error| error.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_version_sort_uses_numeric_components() {
        let older = client_version_key("13, 9, 0, 100").unwrap();
        let newer = client_version_key("13, 11, 0, 1").unwrap();

        assert_eq!(directed_order(&older, &newer, ReplaySortDirection::Ascending), std::cmp::Ordering::Less);
        assert!(client_version_key("not a version").is_none());
    }
}

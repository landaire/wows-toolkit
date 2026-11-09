use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    map_name: String,
    map_key: String,
    game_mode: String,
    scenario: String,
    player_name: String,
    player_ship: String,
    client_version: String,
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
                                    Ok(Some(replay)) => document.set(Some(Arc::new(replay))),
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
                .app_class(|| Some("flex-1 min-h-0 flex-row gap-3".into()))
                .child(directory_browser(directory, document))
                .child(
                    view()
                        .app_class(|| Some("flex-1 min-h-0".into()))
                        .child(empty_view(document, error))
                        .child(replay_view(document)),
                ),
        )
}

fn directory_browser(
    directory: Signal<Option<Arc<ReplayDirectory>>>,
    document: Signal<Option<Arc<ReplayDocument>>>,
) -> AnyElement {
    let rows = each(
        move || directory.with(|value| value.as_ref().map_or_else(Vec::new, |value| value.documents.clone())),
        |replay| replay.path.clone(),
        move |replay, _| {
            view()
                .accessible(true)
                .role(AccessibilityRole::Button)
                .accessibility_state(move || AccessibilityState {
                    selected: replay.with(|replay| {
                        document.with(|current| {
                            current.as_ref().is_some_and(|current| current.path == replay.path)
                        })
                    }),
                    ..AccessibilityState::default()
                })
                .app_class(move || {
                    Some(
                        if replay.with(|replay| {
                            document.with(|current| {
                                current.as_ref().is_some_and(|current| current.path == replay.path)
                            })
                        }) {
                            "min-h-12 px-2 py-1 gap-0.5 border-b border-border bg-accent hover:bg-accent active:opacity-70"
                        } else {
                            "min-h-12 px-2 py-1 gap-0.5 border-b border-border hover:bg-muted active:opacity-70"
                        }
                        .into(),
                    )
                })
                .on_press(move |_: &()| document.set(Some(replay.get())))
                .child(
                    text(move || replay.get().file_name.clone()).app_class(move || {
                        Some(
                            if replay.with(|replay| {
                                document.with(|current| {
                                    current.as_ref().is_some_and(|current| current.path == replay.path)
                                })
                            }) {
                                "text-xs text-accent-foreground"
                            } else {
                                "text-xs text-card-foreground"
                            }
                            .into(),
                        )
                    }),
                )
                .child(
                    text(move || {
                        let replay = replay.get();
                        format!("{} - {}", replay.map_name, replay.date)
                    })
                    .app_class(|| Some("text-xs text-muted-foreground".into())),
                )
        },
    );

    view()
        .app_class(move || {
            Some(
                if directory.get().is_some() {
                    "w-80 min-h-0 p-3 rounded-(--radius) border border-border bg-card"
                } else {
                    "hidden"
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
        .child(scroll().app_class(|| Some("flex-1 min-h-0".into())).child(rows))
        .into_element()
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

fn replay_view(document: Signal<Option<Arc<ReplayDocument>>>) -> AnyElement {
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
        .app_class(move || Some(if document.get().is_some() { "flex-1 gap-3" } else { "hidden" }.into()))
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
                .app_class(|| Some("flex-1 min-h-0 flex-row gap-3".into()))
                .child(
                    view()
                        .app_class(|| Some("w-2/5 p-3 rounded-(--radius) border border-border bg-card".into()))
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
                        .app_class(|| Some("flex-1 min-h-0 p-3 rounded-(--radius) border border-border bg-card".into()))
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

    let documents = replay_paths
        .into_iter()
        .filter_map(|replay_path| match parse_replay(&replay_path) {
            Ok(document) => Some(Arc::new(document)),
            Err(_) => {
                failed += 1;
                None
            }
        })
        .collect();
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

    Ok(ReplayDocument {
        path: path.to_path_buf(),
        file_name,
        date: meta.dateTime,
        map_name: meta.mapDisplayName,
        map_key: meta.mapName,
        game_mode,
        scenario: meta.scenario,
        player_name: meta.playerName,
        player_ship: meta.playerVehicle,
        client_version: meta.clientVersionFromXml,
        duration_seconds: meta.duration,
        players_per_team: meta.playersPerTeam,
        teams_count: meta.teamsCount,
        players,
    })
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

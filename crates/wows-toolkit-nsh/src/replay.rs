use nsh_core::file_dialog::{FileFilter, OpenDialog};
use nsh_core::prelude::*;
use std::sync::Arc;
use wows_replays::ReplayFile;

/// One player listed in replay metadata.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ReplayPlayer {
    pub name: String,
    pub ship_id: String,
    pub relation: u32,
}

/// Replay metadata displayed before packet decoding is available.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ReplayDocument {
    pub path: String,
    pub file_name: String,
    pub date: String,
    pub map_name: String,
    pub map_key: String,
    pub game_mode: String,
    pub scenario: String,
    pub player_name: String,
    pub player_ship: String,
    pub client_version: String,
    pub duration_seconds: u32,
    pub players_per_team: u32,
    pub teams_count: u32,
    pub players: Vec<ReplayPlayer>,
}

#[derive(Debug, Clone, PartialEq, Props)]
pub struct ReplayInspectorProps {}

#[component]
pub fn ReplayInspector(_: ReplayInspectorProps) -> impl IntoElement {
    let document = signal(None::<Arc<ReplayDocument>>);
    let error = signal(None::<String>);
    let picking = signal(false);

    view()
        .app_class(|| Some("flex-1 p-3 gap-3".into()))
        .child(
            view().app_class(|| Some("flex-row items-center justify-end".into())).child(
                view()
                    .accessible(true)
                    .role(AccessibilityRole::Button)
                    .disabled(move || picking.get())
                    .app_class(|| {
                        Some(
                            "min-h-8 px-3 rounded-(--radius) border border-border bg-primary \
                                 text-primary-foreground items-center justify-center hover:opacity-85 active:opacity-70"
                                .into(),
                        )
                    })
                    .on_press(move |_: &()| {
                        if picking.get_untracked() {
                            return;
                        }
                        picking.set(true);
                        error.set(None);
                        spawn(async move {
                            match open_replay().await {
                                Ok(Some(replay)) => document.set(Some(Arc::new(replay))),
                                Ok(None) => {}
                                Err(message) => error.set(Some(message)),
                            }
                            picking.set(false);
                        });
                    })
                    .child(text(move || if picking.get() { "Opening..." } else { "Open Replay File..." })),
            ),
        )
        .child(text(move || error.get().unwrap_or_default()).app_class(move || {
            Some(
                if error.get().is_some() && document.get().is_some() {
                    "px-3 py-2 rounded-(--radius) border border-destructive text-sm text-destructive"
                } else {
                    "hidden"
                }
                .into(),
            )
        }))
        .child(empty_view(document, error))
        .child(replay_view(document))
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
                    text(move || player.get().ship_id).app_class(|| Some("w-28 text-xs text-muted-foreground".into())),
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
                        document.with(|value| value.as_ref().map_or_else(String::new, |value| value.path.clone()))
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

    let file_name = path
        .file_name()
        .ok_or_else(|| format!("selected replay path has no file name: {}", path.display()))?
        .to_string_lossy()
        .into_owned();
    let parse_path = path.clone();
    let replay = blocking(move || ReplayFile::meta_only_from_file(&parse_path))
        .await
        .map_err(|error| format!("replay parser task failed: {error}"))?
        .map_err(|error| format!("could not parse replay {}: {error}", path.display()))?;
    let meta = replay.meta;
    let game_mode =
        meta.gameType.clone().or_else(|| meta.matchGroup.clone()).unwrap_or_else(|| format!("Mode {}", meta.gameMode));
    let players = meta
        .vehicles
        .iter()
        .map(|vehicle| ReplayPlayer {
            name: vehicle.name.clone(),
            ship_id: vehicle.shipId.to_string(),
            relation: vehicle.relation,
        })
        .collect();

    Ok(Some(ReplayDocument {
        path: path.to_string_lossy().into_owned(),
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
    }))
}

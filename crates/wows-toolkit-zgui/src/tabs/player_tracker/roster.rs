//! The Current Match section: the battle in progress, two teams side by side, each player with
//! their figures in the scopes the reader chose, whether they have been met before, and the
//! possible-stream-sniper chip.

use std::collections::HashMap;

use jiff::Timestamp;
use landaire_ui::LoadingLineProps;
use landaire_ui::LoadingProps;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::player_tracker::history;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wows_toolkit_viewmodel::player_tracker::live::RowStats;
use wows_toolkit_viewmodel::player_tracker::live::StatsState;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;
use wows_toolkit_viewmodel::player_tracker::live::row_stats;
use wows_toolkit_viewmodel::player_tracker::live::team_average_personal_rating;
use wows_toolkit_viewmodel::player_tracker::live::team_average_win_rate;
use wows_toolkit_viewmodel::player_tracker::live::visible_stat_modes;
use wows_toolkit_viewmodel::player_tracker::live::win_rate_hover;
use wows_toolkit_viewmodel::player_tracker::shipbuilds_player_url;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::player_tracker::wows_numbers_player_url;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use zgui::component;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui_image::ImageBytes;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::ui::ELLIPSIS;

use crate::desktop;
use crate::game_art::ClassIconKey;
use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::player_tracker::model::TrackerModel;
use crate::tabs::player_tracker::tables::HoverTextProps;
use crate::tabs::player_tracker::tables::severity;
use crate::theme::rating_band_name;

/// The `data-tint` a roster name is drawn in: the same tones the replay inspector's names use.
fn tint_name(tint: PlayerTint) -> &'static str {
    match tint {
        PlayerTint::SelfPlayer => "self",
        PlayerTint::Ally => "ally",
        PlayerTint::Enemy => "enemy",
        PlayerTint::DivisionMate => "division",
        PlayerTint::Abuser => "abuser",
    }
}

/// A scoped column heading: the scope, then the column, composed as the egui app composes them.
fn scoped_heading(scope: &str, column_key: &str) -> String {
    format!("{scope} {}", t!(column_key))
}

/// The chip's hover: each candidate login and the minutes from the battle start it was seen.
pub fn sniper_hover_text(candidates: &[SniperCandidate]) -> String {
    let mut out = String::new();
    for candidate in candidates {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let minutes: Vec<String> = candidate.minutes.iter().map(i64::to_string).collect();
        out.push_str(&t!("ui.twitch.possible_name", name = candidate.login));
        out.push('\n');
        out.push_str(&t!("ui.twitch.seen_minutes", minutes = minutes.join(", ")));
    }
    out.push_str("\n\n");
    out.push_str(&t!("ui.twitch.click_to_copy"));
    out
}

/// What every row of the roster reads from.
#[derive(Clone)]
struct RosterContext {
    stats: Option<HashMap<AccountId, PlayerStatsOut>>,
    met: HashMap<AccountId, TrackedPlayer>,
    count_division_mates: bool,
    twitch: HashMap<String, Vec<SniperCandidate>>,
    modes: Vec<WinRateMode>,
    class_icons: HashMap<ClassIconKey, ImageBytes>,
}

/// The Current Match section.
#[component]
pub fn CurrentMatchSection(model: TrackerModel) -> impl IntoView {
    let locale = Services::current().locale;
    let shell = Shell::current();
    let live = model.live;

    let status = move || {
        locale.track();
        if live.source.with(Option::is_none) {
            return Some((t!("ui.player_tracker.live_no_directory").into_owned(), false));
        }
        if !live.checked.get() {
            return Some((t!("ui.player_tracker.live_checking").into_owned(), true));
        }
        live.live_match.with(Option::is_none).then(|| (t!("ui.player_tracker.live_none").into_owned(), false))
    };

    let body = move || {
        if let Some((text, waiting)) = status() {
            if waiting {
                return AnyView::new(view! { Loading(label = text, attr:data-testid = "tracker-live-status") });
            }
            return AnyView::new(view! {
                row(class = "wt-tracker-status", attr:data-testid = "tracker-live-status") {
                    text {{text}}
                }
            });
        }
        AnyView::new(view! { Roster(model = model, shell = shell) })
    };

    view! {
        column(class = "wt-tracker-section", attr:data-testid = "tracker-current-match") {
            ModeBar(model = model)
            box(class = "wt-tracker-fill") {
                {body}
            }
        }
    }
}

/// The view and scope selectors above the roster: they say nothing about the tables beside it.
#[component]
fn ModeBar(model: TrackerModel) -> impl IntoView {
    let locale = Services::current().locale;
    let views = Signal::derive_local(move || {
        locale.track();
        CurrentMatchViewMode::ALL.to_vec()
    });
    // Detailed shows both scopes, so the scope selector would say nothing there.
    let scopes = Signal::derive_local(move || {
        locale.track();
        if model.view_mode.get() == CurrentMatchViewMode::Compact { WinRateMode::ALL.to_vec() } else { Vec::new() }
    });

    view! {
        row(class = "wt-tracker-modes") {
            for mode in move || views.get(), key = |mode: &CurrentMatchViewMode| *mode as usize {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Xs,
                    class = "wt-tracker-mode",
                    attr:data-testid = format!("tracker-view-mode-{mode:?}"),
                    attr:data-selected = move || (model.view_mode.get() == mode).then(String::new),
                    a11y:selected = move || model.view_mode.get() == mode,
                    on:click = move |_| model.set_view_mode(mode)
                ) {{t!(mode.label_key()).into_owned()}}
            }
            if move || !scopes.with(Vec::is_empty) {
                box(class = "wt-tracker-modes__gap")
            } else {}
            for mode in move || scopes.get(), key = |mode: &WinRateMode| *mode as usize {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Xs,
                    class = "wt-tracker-mode",
                    attr:data-testid = format!("tracker-win-rate-mode-{mode:?}"),
                    attr:data-selected = move || (model.win_rate_mode.get() == mode).then(String::new),
                    a11y:selected = move || model.win_rate_mode.get() == mode,
                    on:click = move |_| model.set_win_rate_mode(mode)
                ) {{t!(mode.label_key()).into_owned()}}
            }
        }
    }
}

/// The two teams, with the lookup's progress or failure above them.
#[component]
fn Roster(model: TrackerModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;
    let live = model.live;

    let note = move || {
        locale.track();
        let resolved = live.roster().is_some_and(|roster| roster.ships_resolved);
        live.stats.with(|stats| match stats {
            // A failure is reported even while the ships are unresolved: a build that did not load
            // is why they are unresolved.
            StatsState::Failed(reason) => {
                Some((t!("ui.player_tracker.stats_unavailable", reason = reason).into_owned(), false))
            }
            _ if !resolved => Some((t!("ui.player_tracker.live_loading_build").into_owned(), true)),
            StatsState::Scanning => Some((t!("ui.player_tracker.stats_resolving").into_owned(), true)),
            StatsState::Held | StatsState::Fetching => {
                Some((t!("ui.player_tracker.stats_fetching").into_owned(), true))
            }
            StatsState::Idle | StatsState::Ready(_) => None,
        })
    };

    // The icons are drawn off the UI thread once the build is in; until then the column is blank.
    let wanting = RenderEffect::new(move |_| {
        let Some(roster) = live.roster() else { return };
        let keys: Vec<ClassIconKey> = roster
            .friendly
            .iter()
            .chain(&roster.enemy)
            .filter_map(|row| Some(ClassIconKey::new(row.species?, row.tint)))
            .collect();
        live.want_class_icons(keys);
    });
    on_cleanup_local(move || drop(wanting));

    let teams = move || {
        locale.track();
        let roster = live.roster()?;
        let context = RosterContext {
            stats: live.stats.with(|stats| stats.players().cloned()),
            met: model.tracked.get(),
            count_division_mates: model.show_division_mates.get(),
            twitch: live.twitch.get(),
            modes: visible_stat_modes(model.view_mode.get(), model.win_rate_mode.get()),
            class_icons: live.class_icons.get(),
        };
        Some(
            view! {
                row(class = "wt-tracker-teams") {
                    TeamColumn(
                        title = t!("ui.player_tracker.allies").into_owned(),
                        side = "ally",
                        rows = roster.friendly.clone(),
                        context = context.clone(),
                        shell = shell
                    )
                    box(class = "wt-tracker-teams__rule")
                    TeamColumn(
                        title = t!("ui.player_tracker.enemies").into_owned(),
                        side = "enemy",
                        rows = roster.enemy.clone(),
                        context = context,
                        shell = shell
                    )
                }
            }
            .into_view(),
        )
    };

    view! {
        column(class = "wt-tracker-roster") {
            {move || note().map(|(text, waiting)| view! {
                row(class = "wt-tracker-roster__note wt-tracker-small wt-tracker-dim", attr:data-testid = "tracker-live-note") {
                    {if waiting {
                        AnyView::new(view! { LoadingLine(label = text) })
                    } else {
                        AnyView::new(view! { text {{text}} })
                    }}
                }
            }.into_view())}
            scroll(class = "wt-tracker-roster__scroll") {
                {teams}
            }
        }
    }
}

/// One team's heading, column header and rows.
#[component]
fn TeamColumn(
    title: String,
    side: &'static str,
    rows: Vec<LiveRosterRow>,
    context: RosterContext,
    shell: Shell,
) -> impl IntoView {
    // The averages are the shared ones, over the scope the rows lead with, so the heading and the
    // cells under it cannot disagree. A team nobody has stats for shows no average.
    let lead = context.modes.first().copied().unwrap_or(WinRateMode::Overall);
    let win_rate = context.stats.as_ref().and_then(|stats| team_average_win_rate(&rows, stats, lead));
    let rating = context.stats.as_ref().and_then(|stats| team_average_personal_rating(&rows, stats, lead));
    let players = t!("ui.player_tracker.team_players", count = rows.len()).into_owned();

    let mut headings = vec![
        ("name", t!("ui.player_tracker.column.player_name").into_owned()),
        ("ship", t!("ui.player_tracker.win_rate_ship").into_owned()),
    ];
    for mode in &context.modes {
        let scope = t!(mode.label_key()).into_owned();
        for key in [
            "ui.player_tracker.column.win_rate",
            "ui.player_tracker.column.personal_rating",
            "ui.player_tracker.column.avg_damage",
            "ui.player_tracker.column.battles",
        ] {
            headings.push(("stat", scoped_heading(&scope, key)));
        }
    }
    headings.push(("met", t!("ui.player_tracker.column.encounters").into_owned()));
    let indexed: Vec<(usize, LiveRosterRow)> = rows.into_iter().enumerate().collect();

    view! {
        column(class = "wt-tracker-team", attr:data-testid = format!("tracker-team-{side}")) {
            row(class = "wt-tracker-team__heading") {
                text(class = "wt-tracker-team__title") {{title}}
                text(class = "wt-tracker-small wt-tracker-dim") {{players}}
                {win_rate.map(|rate| view! {
                    text(class = "wt-tracker-small", attr:data-band = rating_band_name(PersonalRatingCategory::from_win_rate(rate))) {
                        {t!("ui.player_tracker.team_average_win_rate", rate = format!("{rate:.1}%")).into_owned()}
                    }
                }.into_view())}
                {rating.map(|pr| view! {
                    text(class = "wt-tracker-small", attr:data-band = rating_band_name(PersonalRatingCategory::from_pr(pr))) {
                        {format!("{}: {pr:.0}", t!("stat.avg_pr"))}
                    }
                }.into_view())}
            }
            row(class = "wt-tracker-team__header") {
                box(class = "wt-tracker-roster-col--class")
                box(class = "wt-tracker-roster-col--mark")
                box(class = "wt-tracker-roster-col--chip")
                for heading in move || headings.clone().into_iter().enumerate().collect::<Vec<_>>(), key = |heading: &(usize, (&'static str, String))| heading.0 {
                    text(class = format!("wt-tracker-roster-col--{}", heading.1.0)) {{heading.1.1}}
                }
                box(class = "wt-tracker-roster-col--chip")
            }
            for entry in move || indexed.clone(), key = |entry: &(usize, LiveRosterRow)| entry.0 {
                RosterRow(side = side, index = entry.0, row = entry.1, context = context.clone(), shell = shell)
            }
        }
    }
}

/// One live roster entry.
#[component]
fn RosterRow(
    side: &'static str,
    index: usize,
    row: LiveRosterRow,
    context: RosterContext,
    shell: Shell,
) -> impl IntoView {
    let name = match row.clan.as_deref() {
        Some(clan) => format!("[{clan}] {}", row.name),
        None => row.name.clone(),
    };
    // A row whose account the scan never named cannot be looked up at all, which is a different
    // absence from a player the service had no data for.
    let player = row.account_id.and_then(|id| context.stats.as_ref()?.get(&id));
    let pending = context.stats.is_none();
    let status = player.map_or(PlayerStatsStatus::Ok, |player| player.status);
    let both_scopes = win_rate_hover(player, None);
    let lead = context.modes.first().copied().unwrap_or(WinRateMode::Overall);
    // The lead scope's band, far fainter than a chip, so a screen of rows reads as bands first.
    let band = row_stats(player, lead).band.map(rating_band_name);

    let cells: Vec<AnyView> = context
        .modes
        .iter()
        .flat_map(|mode| {
            let id = format!("tracker-roster-{side}-{index}-status-{}", *mode as usize);
            scope_cells(id, row_stats(player, *mode), status, pending, *mode, both_scopes.clone())
        })
        .collect();

    // How often this player has been met, counted the way the tables count.
    let tracked = row.tracked.and_then(|account| context.met.get(&account));
    let met = tracked.map_or(0, |tracked| tracked.visible_arena_ids(context.count_division_mates).count());
    let met_hover = match tracked {
        Some(tracked) if met > 0 => {
            let last = tracked
                .last_visible_timestamp(context.count_division_mates)
                .map(|at| history::last_seen_text(Some(at), Timestamp::now()))
                .unwrap_or_else(|| t!("ui.player_tracker.never_encountered").into_owned());
            t!("ui.player_tracker.encounters_hover", total = tracked.arena_ids.len(), range = met, last = last)
                .into_owned()
        }
        _ => t!("ui.player_tracker.never_encountered").into_owned(),
    };
    let met_hover = StoredValue::new(met_hover);
    let note = tracked.map(|tracked| tracked.notes.clone()).filter(|notes| !notes.trim().is_empty());
    let candidates = context.twitch.get(&row.name).cloned().filter(|candidates| !candidates.is_empty());
    let ship = row.ship_name.clone().unwrap_or_else(|| "-".to_owned());
    let class = class_icon_cell(&row, &context.class_icons);

    view! {
        row(
            class = "wt-tracker-roster-row",
            attr:data-testid = format!("tracker-roster-{side}-{index}"),
            attr:data-band = band.map(str::to_owned),
            a11y:label = name.clone()
        ) {
            {class}
            {notes_mark(side, index, note)}
            {twitch_chip(side, index, candidates, shell)}
            text(class = "wt-tracker-roster-col--name", attr:data-tint = tint_name(row.tint)) {{name}}
            text(class = "wt-tracker-roster-col--ship wt-tracker-small wt-tracker-dim") {{ship}}
            {cells}
            Tooltip {
                TooltipTrigger(class = "wt-tracker-roster-col--met") {
                    text(
                        class = "wt-tracker-small",
                        class = TABULAR_CLASS,
                        attr:data-testid = format!("tracker-roster-{side}-{index}-met"),
                        attr:data-severity = severity(met),
                        a11y:label = met_hover.get_value()
                    ) {{if met == 0 { String::new() } else { separate_number(met as i64, None) }}}
                }
                TooltipContent {{met_hover.get_value()}}
            }
            {row_menu(side, index, &row, shell)}
        }
    }
}

/// The row's ship-class icon in its tone, with the class as its hover. The slot keeps its width
/// while the icon is drawn, so the names stay in line.
fn class_icon_cell(row: &LiveRosterRow, icons: &HashMap<ClassIconKey, ImageBytes>) -> AnyView {
    let icon = row.species.and_then(|species| {
        let bytes = icons.get(&ClassIconKey::new(species, row.tint))?;
        Some((species, bytes.url()))
    });
    let Some((species, url)) = icon else {
        return AnyView::new(view! { box(class = "wt-tracker-roster-col--class") });
    };
    let class = StoredValue::new(row.species_text.clone().unwrap_or_else(|| format!("{species:?}")));
    let url = StoredValue::new(url);
    let test_id = StoredValue::new(format!("tracker-class-{}", row.name));
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-tracker-roster-col--class") {
                image(
                    class = "wt-tracker-class-icon",
                    src = Some(url.get_value()),
                    attr:data-testid = test_id.get_value(),
                    a11y:label = class.get_value()
                )
            }
            TooltipContent {{class.get_value()}}
        }
    })
}

/// The pencil a player with a note carries, with the note as its hover. Without it a note written
/// in the tracker's table is invisible in the battle the player turns up in again.
fn notes_mark(side: &'static str, index: usize, note: Option<String>) -> AnyView {
    let Some(note) = note else {
        return AnyView::new(view! { box(class = "wt-tracker-roster-col--mark") });
    };
    let note = StoredValue::new(note);
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-tracker-roster-col--mark") {
                AppIcon(glyph = Glyph::NotebookPen, label = note.get_value(), class = "wt-tracker-dim", attr:data-testid = format!("tracker-roster-{side}-{index}-note"))
            }
            TooltipContent {{note.get_value()}}
        }
    })
}

/// The possible-stream-sniper chip. One candidate copies on click; several open a picker,
/// because copying an arbitrary one of them would be a guess.
fn twitch_chip(side: &'static str, index: usize, candidates: Option<Vec<SniperCandidate>>, shell: Shell) -> AnyView {
    let Some(candidates) = candidates else {
        return AnyView::new(view! { box(class = "wt-tracker-roster-col--chip") });
    };
    let hover = StoredValue::new(sniper_hover_text(&candidates));
    let test_id = StoredValue::new(format!("tracker-twitch-{side}-{index}"));
    let copy = move |login: String| {
        shell.copy_text(login.clone());
        shell.toasts().ok(t!("ui.twitch.copied", name = login).into_owned());
    };

    if let [only] = candidates.as_slice() {
        let login = StoredValue::new(only.login.clone());
        return AnyView::new(view! {
            Tooltip {
                TooltipTrigger(class = "wt-tracker-roster-col--chip") {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconXs,
                        class = "wt-tracker-twitch",
                        attr:data-testid = test_id.get_value(),
                        a11y:label = hover.get_value(),
                        on:click = move |_| copy(login.get_value())
                    ) {
                        AppIcon(glyph = Glyph::Twitch)
                    }
                }
                TooltipContent(class = "wt-tracker-pre") {{hover.get_value()}}
            }
        });
    }

    let logins = StoredValue::new(candidates.iter().map(|candidate| candidate.login.clone()).collect::<Vec<String>>());
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-tracker-roster-col--chip") {
                DropdownMenu {
                    DropdownMenuTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconXs,
                        class = "wt-tracker-twitch",
                        attr:data-testid = test_id.get_value(),
                        a11y:label = hover.get_value()
                    ) {
                        AppIcon(glyph = Glyph::Twitch)
                    }
                    DropdownMenuContent {
                        for entry in move || logins.get_value().into_iter().enumerate().collect::<Vec<_>>(), key = |entry: &(usize, String)| entry.clone() {
                            MenuItem(
                                attr:data-testid = format!("tracker-twitch-pick-{side}-{index}-{}", entry.0),
                                a11y:label = entry.1.clone(),
                                on_select = {
                                    let login = entry.1.clone();
                                    UnsyncCallback::new(move |()| copy(login.clone()))
                                }
                            ) {{entry.1.clone()}}
                        }
                    }
                }
            }
            TooltipContent(class = "wt-tracker-pre") {{hover.get_value()}}
        }
    })
}

/// A roster row's menu: where to read more about this player. Both links need an account id and
/// a region, which only the identity scan supplies; a row it never named has no menu.
fn row_menu(side: &'static str, index: usize, row: &LiveRosterRow, shell: Shell) -> AnyView {
    let Some((account, region)) = row.account_id.zip(row.region) else {
        // The column holds its width, so a row with no menu does not pull the others out of line.
        return AnyView::new(view! { box(class = "wt-tracker-roster-col--chip") });
    };
    let numbers = StoredValue::new(wows_numbers_player_url(region, account, &row.name));
    let builds = StoredValue::new(shipbuilds_player_url(region, account, &row.name));
    let name = StoredValue::new(row.name.clone());

    AnyView::new(view! {
        box(class = "wt-tracker-roster-col--chip") {
            DropdownMenu {
                DropdownMenuTrigger(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconXs,
                    attr:data-testid = format!("tracker-roster-actions-{side}-{index}"),
                    a11y:label = name.get_value()
                ) {
                    Icon(icon = ELLIPSIS, size = IconSize::Sm)
                }
                DropdownMenuContent {
                    MenuItem(on_select = UnsyncCallback::new(move |()| desktop::open_url(&numbers.get_value()))) {
                        AppIcon(glyph = Glyph::SquareArrowOutUpRight)
                        text {{t!("ui.player_tracker.open_wows_numbers").into_owned()}}
                    }
                    MenuItem(on_select = UnsyncCallback::new(move |()| desktop::open_url(&builds.get_value()))) {
                        AppIcon(glyph = Glyph::SquareArrowOutUpRight)
                        text {{t!("ui.player_tracker.open_shipbuilds").into_owned()}}
                    }
                    MenuItem(on_select = UnsyncCallback::new(move |()| TrackerModel::find_player_matches(shell, account))) {
                        AppIcon(glyph = Glyph::Search)
                        text {{t!("ui.player_tracker.find_matches").into_owned()}}
                    }
                }
            }
        }
    })
}

/// A stats figure: the value when the service answered with one, a dash when it did not, and
/// nothing while the lookup is still running.
fn stat_text(text: Option<String>, pending: bool) -> String {
    match (text, pending) {
        (Some(text), _) => text,
        (None, true) => String::new(),
        (None, false) => "-".to_owned(),
    }
}

fn stat_cell(text: Option<String>, pending: bool) -> AnyView {
    let text = stat_text(text, pending);
    AnyView::new(
        view! { text(class = "wt-tracker-roster-col--stat wt-tracker-small wt-tracker-dim", class = TABULAR_CLASS) {{text}} },
    )
}

/// The cells one scope contributes to a row.
fn scope_cells(
    id: String,
    stats: RowStats,
    status: PlayerStatsStatus,
    pending: bool,
    mode: WinRateMode,
    both_scopes: Option<String>,
) -> Vec<AnyView> {
    // A hidden profile, a service with nothing, and a service out of reach are three answers.
    if !matches!(status, PlayerStatsStatus::Ok) {
        let (glyph, hover) = match status {
            PlayerStatsStatus::Hidden => (Glyph::EyeOff, t!("ui.player_tracker.stats_hidden")),
            PlayerStatsStatus::Unavailable => (Glyph::TriangleAlert, t!("ui.player_tracker.stats_no_data")),
            PlayerStatsStatus::Unknown | PlayerStatsStatus::Ok => (Glyph::Info, t!("ui.player_tracker.stats_no_data")),
        };
        let hover = StoredValue::new(hover.into_owned());
        let status_id = StoredValue::new(id.clone());
        let mut cells = vec![AnyView::new(view! {
            Tooltip {
                TooltipTrigger(class = "wt-tracker-roster-col--stat wt-tracker-dim") {
                    AppIcon(glyph = glyph, label = hover.get_value(), attr:data-testid = status_id.get_value())
                }
                TooltipContent {{hover.get_value()}}
            }
        })];
        cells.extend((0..3).map(|_| stat_cell(None, false)));
        return cells;
    }

    let win_rate = stat_text(stats.win_rate.map(|rate| format!("{rate:.1}%")), pending);
    let rate_band = stats.band.map(rating_band_name);
    let rate_cell = match rate_band {
        Some(band) => AnyView::new(view! {
            box(attr:data-band = band.to_owned(), class = "wt-tracker-band wt-tracker-roster-col--stat") {
                HoverText(text = win_rate.clone(), hover = both_scopes.clone().unwrap_or_default(), class = "wt-tracker-small")
            }
        }),
        None => AnyView::new(view! {
            HoverText(text = win_rate, hover = both_scopes.unwrap_or_default(), class = "wt-tracker-roster-col--stat wt-tracker-small")
        }),
    };

    let rating = match stats.pr {
        Some(pr) => {
            let band = rating_band_name(PersonalRatingCategory::from_pr(pr));
            let id = StoredValue::new(format!("{id}-pr"));
            let hover = StoredValue::new(
                t!(match mode {
                    WinRateMode::Overall => "ui.player_tracker.pr_overall_hover",
                    WinRateMode::Ship => "ui.player_tracker.pr_ship_hover",
                })
                .into_owned(),
            );
            AnyView::new(view! {
                Tooltip {
                    TooltipTrigger(class = "wt-tracker-roster-col--stat") {
                        text(
                            class = "wt-tracker-chip wt-tracker-small",
                            class = TABULAR_CLASS,
                            attr:data-band = band.to_owned(),
                            attr:data-testid = id.get_value(),
                            a11y:label = hover.get_value()
                        ) {{format!("{pr:.0}")}}
                    }
                    TooltipContent {{hover.get_value()}}
                }
            })
        }
        None => stat_cell(None, pending),
    };

    vec![
        rate_cell,
        rating,
        stat_cell(stats.avg_damage.map(|damage| separate_number(damage, None)), pending),
        stat_cell(stats.battles.map(|battles| separate_number(battles, None)), pending),
    ]
}

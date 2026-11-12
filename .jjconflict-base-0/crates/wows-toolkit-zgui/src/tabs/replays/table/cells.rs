//! The player table's cells.

use std::rc::Rc;

use rust_i18n::t;
use wows_replay_insights::battle_report::ConnectionNote;
use wows_replay_insights::battle_report::TimeOffline;
use wows_replays::types::AccountId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::columns::CaptainPointsTier;
use wows_toolkit_replay_data::columns::CellValue;
use wows_toolkit_replay_data::columns::ColorRole;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::BattleOutcome;
use wows_toolkit_replay_data::columns::cell_value;
use wows_toolkit_replay_data::columns::player_color_kind;
use wows_toolkit_replay_data::damage_mix::DamageMix;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::SkillWarning;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use zgui::prelude::AnyView;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::events;
use zgui::vocab::PointerButton;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::CHEVRON_RIGHT;

use crate::icons::AppIcon;
use crate::icons::AppIconProps;
use crate::tabs::replays::table::TableCx;
use crate::tabs::replays::table::TableRow;
use crate::tabs::replays::table::damage_mix;
use crate::tabs::replays::table::menu;
use crate::theme::rating_band_name;

/// One column's cell of one row.
pub(super) fn cell(col: ReplayColumn, row: &TableRow, cx: TableCx) -> AnyView {
    match col {
        ReplayColumn::Name => name_cell(row, cx),
        ReplayColumn::Skills => skills_cell(row, cx),
        ReplayColumn::ActualDamage | ReplayColumn::ReceivedDamage => damage_cell(col, row, cx),
        _ => number_cell(col, row, cx),
    }
}

/// The CSS colour a role is drawn in.
pub(super) fn role_color(role: ColorRole) -> String {
    match role {
        ColorRole::Player(kind) => player_color(kind).to_owned(),
        ColorRole::PrTier(category) => format!("var(--wt-pr-{}-text)", rating_band_name(category)),
        ColorRole::PrTierTint(category) => format!("var(--wt-pr-{}-hue)", rating_band_name(category)),
        ColorRole::CaptainPoints(tier) => match tier {
            CaptainPointsTier::Bad => "var(--wt-color-loss)",
            CaptainPointsTier::Warning => "var(--wt-color-warn)",
            CaptainPointsTier::Caution => "var(--wt-color-notice)",
            CaptainPointsTier::Good => "var(--wt-color-win)",
        }
        .to_owned(),
        ColorRole::WinLoss(outcome) => match outcome {
            BattleOutcome::Win => "var(--wt-color-win)",
            BattleOutcome::Loss => "var(--wt-color-loss)",
            BattleOutcome::Draw => "var(--wt-color-draw)",
        }
        .to_owned(),
        ColorRole::Fixed(rgb) => format!("#{rgb:06x}"),
    }
}

/// Self is emphasised body text, an ally reads as a win and an enemy as a loss.
pub(super) const fn player_color(kind: PlayerColorKind) -> &'static str {
    match kind {
        PlayerColorKind::SelfPlayer => "var(--wt-color-text-strong)",
        PlayerColorKind::Ally => "var(--wt-color-win)",
        PlayerColorKind::Enemy => "var(--wt-color-loss)",
        PlayerColorKind::DivisionMate => "var(--wt-color-division)",
        PlayerColorKind::Abuser => "var(--wt-color-abuser)",
    }
}

/// The game's clan-league colour, else the team relation's.
pub(super) fn clan_color(row: &PlayerRow) -> String {
    match row.clan_color_rgb {
        Some(rgb) => format!("#{rgb:06x}"),
        None => {
            let kind = if row.relation.is_self() {
                PlayerColorKind::SelfPlayer
            } else if row.relation.is_ally() {
                PlayerColorKind::Ally
            } else {
                PlayerColorKind::Enemy
            };
            player_color(kind).to_owned()
        }
    }
}

/// Records which cell a right-click landed on, so the row's menu can copy it.
fn note_cell(cx: TableCx, player: AccountId, col: ReplayColumn) -> impl Fn(&mut EventCx<'_, events::PointerDown>) {
    move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button == Some(PointerButton::Secondary) {
            cx.noted.set_value(Some((player, col)));
        }
    }
}

/// `content` with `hover` shown over it, one line per line of the text.
pub(super) fn with_hover(content: AnyView, hover: Option<String>) -> AnyView {
    let Some(hover) = hover else { return content };

    let lines: Vec<AnyView> = hover.lines().map(|line| AnyView::new(view! { text {{line.to_owned()}} })).collect();
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-pt-trigger") {{content}}
            TooltipContent {
                box(class = "wt-pt-hover") {{lines}}
            }
        }
    })
}

/// The text of a cell: selectable, cut off with an ellipsis, and a rating band's chip for a
/// personal rating.
fn cell_text(cell: &CellValue) -> AnyView {
    let chip = match cell.color {
        Some(ColorRole::PrTier(category)) => {
            Some(format!("color-mix(in srgb, var(--wt-pr-{}-hue) 18%, transparent)", rating_band_name(category)))
        }
        _ => None,
    };
    let chipped = chip.is_some().then(String::new);
    let text = cell.text.clone();

    AnyView::new(view! {
        text(class = "wt-pt-text", attr:data-chip = chipped, style:background-color = chip) {{text}}
    })
}

/// A figure over a faint bar of the row's share of its team's total, where the column has one.
fn number_cell(col: ReplayColumn, row: &TableRow, cx: TableCx) -> AnyView {
    let mut cell = cell_value(&row.row, col, row.debug);
    let share = row.share(col);
    if let Some(share) = share {
        let line = share.text();
        cell.hover = Some(match cell.hover.take() {
            Some(hover) => format!("{hover}\n{line}"),
            None => line,
        });
    }
    let hover = cell.hover.clone();

    with_hover(figure_cell(col, row, cx, &cell, None), hover)
}

/// The cell box: its colour, the share bar under it, its text and, for a damage figure, the
/// band of damage types under the text.
fn figure_cell(col: ReplayColumn, row: &TableRow, cx: TableCx, cell: &CellValue, mix: Option<&DamageMix>) -> AnyView {
    let ink = cell.color.map(role_color);
    // A ship name keeps the font's own figures.
    let figures = (col != ReplayColumn::ShipName).then(String::new);
    let share = row.share(col).map(|share| {
        let width = format!("{}%", share.fraction * 100.0);
        AnyView::new(view! { box(class = "wt-pt-share", style:width = width) {} })
    });
    let text = cell_text(cell);
    let content = match mix {
        Some(mix) => {
            let bar = damage_mix::stacked_bar(mix);
            AnyView::new(view! { box(class = "wt-pt-figure") { {text} {bar} } })
        }
        None => text,
    };
    let note = note_cell(cx, row.row.db_id, col);

    AnyView::new(view! {
        box(
            class = "wt-pt-cell",
            attr:data-figures = figures,
            attr:data-column = col.storage_key(),
            var:--wt-pt-ink = ink,
            on:pointer_down = note
        ) {
            {share}
            {content}
        }
    })
}

/// A damage figure over its team-share bar, with the player's damage split by type under it and
/// listed with its colour key on hover. The plain figure when there is no split to show.
fn damage_cell(col: ReplayColumn, row: &TableRow, cx: TableCx) -> AnyView {
    let Some(lines) = wows_toolkit_replay_data::damage_mix::shown_lines(&row.row, col, row.debug) else {
        return number_cell(col, row, cx);
    };
    let Some(mix) = DamageMix::of(lines) else {
        return number_cell(col, row, cx);
    };

    let cell = cell_value(&row.row, col, row.debug);
    let content = figure_cell(col, row, cx, &cell, Some(&mix));
    let listing = damage_mix::listing(&mix, lines);
    let share = row.share(col).map(|share| share.text());

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-pt-trigger") {{content}}
            TooltipContent {
                box(class = "wt-pt-hover") {
                    {listing}
                    {share}
                }
            }
        }
    })
}

/// The Skills cell: the captain's points in their tier colour after the Dazzle, Incoming Fire
/// Alert and build warning markers.
fn skills_cell(row: &TableRow, cx: TableCx) -> AnyView {
    let player = &row.row;
    let cell = cell_value(player, ReplayColumn::Skills, row.debug);
    let shows_real_label = player.has_vehicle_entity && (!player.relation.is_enemy() || row.debug);
    if !shows_real_label {
        let hover = cell.hover.clone();
        return with_hover(figure_cell(ReplayColumn::Skills, row, cx, &cell, None), hover);
    }

    let ink = Some(role_color(cell.color.unwrap_or(ColorRole::CaptainPoints(CaptainPointsTier::Good))));
    let mut markers = Vec::new();
    if player.has_dazzle {
        markers.push(Glyph::Star);
    }
    if player.has_ifa {
        markers.push(Glyph::Bell);
    }
    if let Some(warning) = player.skill_warning {
        // Two separate mistakes, so two separate glyphs.
        markers.push(match warning {
            SkillWarning::TowerDefense => Glyph::Castle,
            SkillWarning::NoHighTier => Glyph::TriangleAlert,
        });
    }
    let markers: Vec<AnyView> = markers
        .into_iter()
        .map(|glyph| AnyView::new(view! { AppIcon(glyph = glyph, class = "wt-pt-marker") }))
        .collect();
    let text = cell_text(&cell);
    let note = note_cell(cx, player.db_id, ReplayColumn::Skills);
    let hover = cell.hover.clone();

    with_hover(
        AnyView::new(view! {
            box(
                class = "wt-pt-cell",
                attr:data-column = ReplayColumn::Skills.storage_key(),
                var:--wt-pt-ink = ink,
                on:pointer_down = note
            ) {
                {markers}
                {text}
            }
        }),
        hover,
    )
}

/// Which side of the battle a row stands for, for the stripe at its leading edge.
const fn row_mark(row: &PlayerRow, self_team_is_row_team: bool) -> &'static str {
    if row.is_self {
        "owner"
    } else if self_team_is_row_team {
        "ally"
    } else {
        "enemy"
    }
}

/// The Name cell: the disclosure, the ship class, the division, the clan tag and the player,
/// then the profile, Twitch and connection markers, with the row's actions at its end.
fn name_cell(row: &TableRow, cx: TableCx) -> AnyView {
    let player = Rc::clone(&row.row);
    let db_id = player.db_id;
    let mark = row_mark(&player, player.relation.is_ally() || player.relation.is_self());

    let expanded = cx.expanded;
    let open = move || expanded.with(|expanded| expanded.contains(&db_id)).then(String::new);
    let toggle = move |ev: &mut EventCx<'_, events::Click>| {
        cx.toggle_expanded(db_id);
        ev.stop_propagation();
    };
    // A double click on the disclosure is two presses of it, not also an activation of the row.
    let keep_double = |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation();
    let caret = view! {
        box(
            class = "wt-pt-caret",
            attr:data-open = open,
            attr:data-testid = format!("player-caret-{}", db_id.raw()),
            on:click = toggle,
            on:double_click = keep_double
        ) {
            Icon(icon = CHEVRON_RIGHT)
        }
    };

    let species = (!player.ship_species_text.is_empty()).then(|| {
        let species = player.ship_species_text.clone();
        AnyView::new(view! { text(class = "wt-pt-species wt-pt-text") {{species}} })
    });
    let division = player.division_label.clone().map(|label| AnyView::new(view! { text(class = "wt-pt-text") {{label}} }));
    let clan = player.clan_tag.clone().map(|clan| {
        let color = clan_color(&player);
        AnyView::new(view! { text(class = "wt-pt-text", style:color = color) {{clan}} })
    });
    let name_color = player.is_abuser.then(|| player_color(PlayerColorKind::Abuser).to_owned());
    let name = player.display_name.clone();
    let hidden = player.is_hidden_profile.then(|| {
        let tip = t!("ui.replay.player.hidden_profile").into_owned();
        AnyView::new(view! {
            Tooltip {
                TooltipTrigger { AppIcon(glyph = Glyph::EyeOff) }
                TooltipContent {{tip}}
            }
        })
    });
    let twitch = twitch_chip(&player.twitch_candidates, cx);
    let connection = player.connection.as_ref().map(|note| connection_chip(note, cx));
    let actions = menu::row_actions(db_id, cx);
    let note = note_cell(cx, db_id, ReplayColumn::Name);

    AnyView::new(view! {
        box(
            class = "wt-pt-cell",
            attr:data-column = ReplayColumn::Name.storage_key(),
            on:pointer_down = note
        ) {
            box(class = "wt-pt-stripe", attr:data-mark = mark) {}
            box(class = "wt-pt-name") {
                {caret}
                {species}
                {division}
                {clan}
                text(class = "wt-pt-text", style:color = name_color) {{name}}
                box(class = "wt-pt-fill") {}
                {hidden}
                {twitch}
                {connection}
            }
            {actions}
        }
    })
}

/// What a connection history says, in the egui app's own wording.
fn connection_hover_text(note: &ConnectionNote) -> String {
    match note {
        ConnectionNote::NeverConnected => t!("ui.replay.player.never_connected").into_owned(),
        ConnectionNote::Interrupted { events, .. } => format!("Player {}", events.join(", ")),
    }
}

/// How long the player was offline while alive, when the replay says.
fn time_offline(note: &ConnectionNote) -> Option<TimeOffline> {
    match note {
        ConnectionNote::NeverConnected => None,
        ConnectionNote::Interrupted { time_offline, .. } => *time_offline,
    }
}

/// `time` as minutes and seconds, "1m 12s", or seconds alone under a minute.
pub(super) fn offline_label(time: TimeOffline) -> String {
    let secs = time.duration().as_secs();
    match (secs / 60, secs % 60) {
        (0, secs) => format!("{secs}s"),
        (mins, secs) => format!("{mins}m {secs}s"),
    }
}

/// How deep a disconnect chip is coloured, from 0 to 1: nothing for a drop under half a minute,
/// rising evenly to full at a minute and a half.
pub(super) fn offline_depth(time: TimeOffline) -> f32 {
    const NEUTRAL_UNDER_SECS: f32 = 30.;
    const FULL_AT_SECS: f32 = 90.;

    let secs = time.duration().as_secs_f32();
    ((secs - NEUTRAL_UNDER_SECS) / (FULL_AT_SECS - NEUTRAL_UNDER_SECS)).clamp(0., 1.)
}

/// The plug and, when known, how long the player was offline, on a ground that reddens with that
/// time. A click copies what its tooltip says.
fn connection_chip(note: &ConnectionNote, cx: TableCx) -> AnyView {
    let hover = connection_hover_text(note);
    let copied = hover.clone();
    let time = time_offline(note);
    let depth = time.map(|time| format!("{}%", offline_depth(time) * 100.0));
    let label = time.map(|time| AnyView::new(view! { text {{offline_label(time)}} }));
    let copy = move |ev: &mut EventCx<'_, events::Click>| {
        ev.stop_propagation();
        cx.copy(copied.clone(), t!("ui.replay.context.copied").into_owned());
    };

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                box(class = "wt-pt-offline", var:--wt-pt-depth = depth, attr:data-testid = "player-disconnect", on:click = copy) {
                    AppIcon(glyph = Glyph::Plug)
                    {label}
                }
            }
            TooltipContent {{hover}}
        }
    })
}

/// The Twitch logo beside the name when a chat login around this battle plausibly names this
/// player. One candidate copies its login on click; several open a menu to pick from.
fn twitch_chip(candidates: &[SniperCandidate], cx: TableCx) -> Option<AnyView> {
    let first = candidates.first()?;

    let mut hover = candidates
        .iter()
        .map(|candidate| {
            let minutes = candidate.minutes.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            format!(
                "{}\n{}",
                t!("ui.twitch.possible_name", name = candidate.login),
                t!("ui.twitch.seen_minutes", minutes = minutes)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    hover.push_str(&format!("\n\n{}", t!("ui.twitch.click_to_copy")));

    if candidates.len() == 1 {
        let login = first.login.clone();
        let copy = move |ev: &mut EventCx<'_, events::Click>| {
            ev.stop_propagation();
            cx.copy(login.clone(), t!("ui.twitch.copied", name = login).into_owned());
        };
        return Some(with_hover(
            AnyView::new(view! {
                box(class = "wt-pt-marker", attr:data-testid = "player-twitch", on:click = copy) {
                    AppIcon(glyph = Glyph::Twitch)
                }
            }),
            Some(hover),
        ));
    }

    let items: Vec<AnyView> = candidates
        .iter()
        .map(|candidate| {
            let login = candidate.login.clone();
            let shown = login.clone();
            let copy = UnsyncCallback::new(move |()| {
                cx.copy(login.clone(), t!("ui.twitch.copied", name = login).into_owned());
            });
            AnyView::new(view! { MenuItem(on_select = copy) {{shown}} })
        })
        .collect();

    Some(with_hover(
        AnyView::new(view! {
            DropdownMenu {
                DropdownMenuTrigger(variant = ButtonVariant::Ghost, size = ButtonSize::IconSm, attr:data-testid = "player-twitch") {
                    AppIcon(glyph = Glyph::Twitch)
                }
                DropdownMenuContent {{items}}
            }
        }),
        Some(hover),
    ))
}

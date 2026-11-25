//! The player table's cells.

use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::Palette;
use landaire_ui::Rgb;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use landaire_ui::text_on;
use wows_replay_insights::battle_report::ConnectionDrop;
use wows_replay_insights::battle_report::ConnectionNote;
use wows_replay_insights::battle_report::TimeOffline;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::columns::BattleOutcome;
use wows_toolkit_replay_data::columns::CaptainPointsTier;
use wows_toolkit_replay_data::columns::CellValue;
use wows_toolkit_replay_data::columns::ColorRole;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::cell_value;
use wows_toolkit_replay_data::columns::player_color_kind;
use wows_toolkit_replay_data::damage_mix::DamageMix;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::SkillWarning;
use wows_toolkit_replay_data::model::separate_number;
use wows_toolkit_replay_data::team_totals::TeamScale;
use wows_toolkit_replay_data::team_totals::TeamSide;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use wt_translations::t;
use zgui::elements::Focus;
use zgui::prelude::AnyView;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::Attrs;
use zgui::view::ClassName;
use zgui::view::CustomPropertyName;
use zgui::view::EventCx;
use zgui::view::events;
use zgui::vocab::PointerButton;
use zgui::vocab::PropValue;
use zgui::vocab::Role;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::icons::AppIconProps;
use crate::tabs::replays::rating_hover::RatedFigure;
use crate::tabs::replays::rating_hover::with_rating_hover;
use crate::tabs::replays::table::RowId;
use crate::tabs::replays::table::TableCx;
use crate::tabs::replays::table::TableRow;
use crate::tabs::replays::table::TeamHeading;
use crate::tabs::replays::table::art;
use crate::tabs::replays::table::damage_mix;
use crate::tabs::replays::table::menu;
use crate::theme::Semantic;
use crate::theme::rating_band_name;
use crate::theme::with_clan_league_ink;

/// One column's cell of one row.
pub(super) fn cell(col: ReplayColumn, row: &TableRow, cx: TableCx) -> AnyView {
    match col {
        ReplayColumn::Name => name_cell(row, cx),
        ReplayColumn::Skills => skills_cell(row, cx),
        ReplayColumn::ActualDamage | ReplayColumn::ReceivedDamage => damage_cell(col, row, cx),
        ReplayColumn::PersonalRating | ReplayColumn::MatchContribution => rating_cell(col, row, cx),
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

/// A clan tag's colour: the game's league colour moved until it reads on the rows of the palette
/// on screen, since the game's colours include pale tones made for its dark client; else the team
/// relation's colour. The table's sheet picks the light or the dark palette's colour.
pub(super) fn clan_ink(row: &PlayerRow, cx: TableCx) -> Attrs {
    let attrs = Attrs::new().class_toggle(ClassName::new("wt-pt-clan"), true);
    let Some(league) = row.clan_color_rgb.map(Rgb::new) else {
        let kind = if row.relation.is_self() {
            PlayerColorKind::SelfPlayer
        } else if row.relation.is_ally() {
            PlayerColorKind::Ally
        } else {
            PlayerColorKind::Enemy
        };
        let color = player_color(kind);
        return attrs
            .custom_property(CustomPropertyName::new("wt-clan-light"), move || Some(color.to_owned()))
            .custom_property(CustomPropertyName::new("wt-clan-dark"), move || Some(color.to_owned()));
    };

    with_clan_league_ink(attrs, cx.theme, league)
}

/// The ship's class: the game's icon tinted for the player, or the species' name until the icon
/// is read or where the build has none.
pub(super) fn class_mark(row: &PlayerRow, cx: TableCx) -> AnyView {
    let species = row.ship_class;
    let kind = player_color_kind(row);
    let text = row.ship_species_text.clone();

    AnyView::new(move || match art::class_icon(cx.art, species, kind) {
        Some(svg) => AnyView::new(view! {
            vector(
                class = "wt-pt-class",
                prop:svg = PropValue::from(svg),
                a11y:role = Role::Image,
                a11y:label = text.clone()
            )
        }),
        None => AnyView::new((!text.is_empty()).then(|| {
            view! {
                field(
                    class = "wt-pt-species wt-pt-text",
                    tabindex = Focus::Programmatic,
                    state:read_only
                ) {{text.clone()}}
            }
        })),
    })
}

/// The heading over a team's rows: the team's name and its Actual Damage.
pub(super) fn team_heading(team: TeamHeading) -> AnyView {
    let mark = match team.side {
        TeamSide::Allies => "ally",
        TeamSide::Enemies => "enemy",
    };
    let label = team.side.label();
    let total = team.damage.side(team.side).map(|total| {
        let text = separate_number(total);
        let hover = format!("{}{text}", t!("ui-replay-team_damage"));
        with_hover(
            AnyView::new(view! {
                field(
                    class = "wt-pt-text wt-pt-team__damage",
                    attr:data-testid = format!("team-damage-{mark}"),
                    tabindex = Focus::Programmatic,
                    state:read_only
                ) {{text}}
            }),
            Some(hover),
        )
    });

    AnyView::new(view! {
        box(class = "wt-pt-team", attr:data-testid = format!("team-heading-{mark}")) {
            box(class = "wt-pt-stripe", attr:data-team = mark) {}
            field(class = "wt-pt-team__name", tabindex = Focus::Programmatic, state:read_only) {{label}}
            {total}
        }
    })
}

/// Records which cell a right-click landed on, so the row's menu can copy it.
fn note_cell(cx: TableCx, row: RowId, col: ReplayColumn) -> impl Fn(&mut EventCx<'_, events::PointerDown>) {
    move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button == Some(PointerButton::Secondary) {
            cx.noted.note(row, col);
        }
    }
}

/// Keeps a double click on a control from opening or closing the row.
fn keep_double(ev: &mut EventCx<'_, events::DoubleClick>) {
    ev.stop_propagation();
}

/// Keeps a press on a control from selecting the row. A right press still opens the row's menu.
fn keep_primary_press(ev: &mut EventCx<'_, events::PointerDown>) {
    if ev.button == Some(PointerButton::Primary) {
        ev.stop_propagation();
    }
}

/// `content` with `hover` shown over it, one line per line of the text.
pub(super) fn with_hover(content: AnyView, hover: Option<String>) -> AnyView {
    let Some(hover) = hover else { return content };

    let hover = StoredValue::new_local(hover);
    let lines = move || {
        hover.with_value(|hover| {
            hover.lines().map(|line| AnyView::new(view! { text {{line.to_owned()}} })).collect::<Vec<_>>()
        })
    };
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-pt-trigger") {{content}}
            TooltipContent {
                box(class = "wt-pt-hover") {{lines()}}
            }
        }
    })
}

/// The read-only text of a cell: selectable and copyable, cut off with an ellipsis, and a rating
/// band's chip for a personal rating.
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
        field(
            class = "wt-pt-text",
            attr:data-chip = chipped,
            style:background-color = chip,
            tabindex = Focus::Programmatic,
            state:read_only
        ) {{text}}
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
fn figure_cell(
    col: ReplayColumn,
    row: &TableRow,
    cx: TableCx,
    cell: &CellValue,
    damage_bar: Option<AnyView>,
) -> AnyView {
    let ink = cell.color.map(role_color);
    // A ship name keeps the font's own figures.
    let figures = (col != ReplayColumn::ShipName).then(String::new);
    let share = row.share(col).map(|share| {
        let width = format!("{}%", share.fraction * 100.0);
        AnyView::new(view! { box(class = "wt-pt-share", style:width = width) {} })
    });
    let text = cell_text(cell);
    let flag = (col == ReplayColumn::ShipName).then(|| row.row.ship_nation.clone()).flatten().map(|nation| {
        AnyView::new(move || {
            if !cx.show_nation_flags.get() {
                return None;
            }
            art::flag(cx.art, &nation).map(|src| view! { image(class = "wt-pt-flag", src = src, a11y:hidden = true) })
        })
    });
    let content = match (damage_bar, flag) {
        (Some(bar), _) => AnyView::new(view! { box(class = "wt-pt-figure") { {text} {bar} } }),
        (None, Some(flag)) => AnyView::new(view! { {flag} {text} }),
        (None, None) => text,
    };
    let note = note_cell(cx, row.id, col);

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
    let scale = row.scale(col).unwrap_or(TeamScale::FULL);
    let bar_mix = mix.clone();
    let bar = AnyView::new(move || cx.shows_damage_bar(col).then(|| damage_mix::stacked_bar(&bar_mix, scale)));
    let content = figure_cell(col, row, cx, &cell, Some(bar));
    let shown = StoredValue::new_local((mix, lines.to_vec(), row.share(col).map(|share| share.text())));
    let explained = move || {
        shown.with_value(|(mix, lines, share)| {
            let listing = damage_mix::listing(mix, lines);
            let share = share.clone();
            AnyView::new(view! {
                box(class = "wt-pt-hover") {
                    {listing}
                    {share}
                }
            })
        })
    };

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-pt-trigger") {{content}}
            TooltipContent {{explained()}}
        }
    })
}

/// A PR or MC figure over its team-share bar, with the rating's parts drawn on hover. The plain
/// figure and its band's name when the rating kept no parts or the row's stats are hidden, and
/// an MC dash says why when the battle was not rated or the recording ended before it did.
fn rating_cell(col: ReplayColumn, row: &TableRow, cx: TableCx) -> AnyView {
    // The parts carry the damage and spotting figures a hidden row keeps back.
    let hidden = row.row.should_hide_stats() && !row.debug;
    let figure = match col {
        ReplayColumn::MatchContribution => {
            row.row.match_contribution.clone().map(|rating| RatedFigure::MatchContribution(Box::new(rating)))
        }
        _ => row.row.personal_rating.clone().filter(|rating| rating.breakdown.is_some()).map(RatedFigure::Personal),
    };
    let cell = cell_value(&row.row, col, row.debug);

    match figure.filter(|_| !hidden) {
        Some(figure) => with_rating_hover(figure_cell(col, row, cx, &cell, None), figure, "wt-pt-trigger"),
        None if col == ReplayColumn::MatchContribution && row.row.match_contribution.is_none() => {
            match row.match_contribution_withheld {
                Some(reason) => with_hover(figure_cell(col, row, cx, &cell, None), Some(reason.explanation())),
                None => number_cell(col, row, cx),
            }
        }
        None => number_cell(col, row, cx),
    }
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
    let note = note_cell(cx, row.id, ReplayColumn::Skills);
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
pub(super) const fn row_mark(row: &PlayerRow, side: TeamSide) -> &'static str {
    match (row.is_self, side) {
        (true, _) => "owner",
        (false, TeamSide::Allies) => "ally",
        (false, TeamSide::Enemies) => "enemy",
    }
}

/// The Name cell: the disclosure, the ship class, the division, the clan tag and the player,
/// then the profile, Twitch and connection markers, with the row's actions at its end.
fn name_cell(row: &TableRow, cx: TableCx) -> AnyView {
    let player = Arc::clone(&row.row);
    let id = row.id;
    let db_id = player.db_id;

    let expanded = cx.expanded;
    let open = move || expanded.with(|expanded| expanded.contains(&id)).then(String::new);
    let toggle = move |ev: &mut EventCx<'_, events::Click>| {
        cx.toggle_expanded(id);
        ev.stop_propagation();
    };
    // A press on the disclosure opens or closes the row and leaves the selection as it was.
    let keep_press = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation();
    let caret = cx.expandable.then(|| {
        AnyView::new(view! {
            box(
                class = "wt-pt-caret",
                attr:data-open = open,
                attr:data-testid = format!("player-caret-{}", db_id.raw()),
                on:pointer_down = keep_press,
                on:click = toggle,
                on:double_click = keep_double
            ) {
                Icon(icon = CHEVRON_RIGHT)
            }
        })
    });

    let species = class_mark(&player, cx);
    let species_name = player.ship_species_text.clone();
    let species = if species_name.is_empty() {
        species
    } else {
        AnyView::new(view! {
            Tooltip {
                TooltipTrigger {{species}}
                TooltipContent {{species_name.clone()}}
            }
        })
    };
    let division = player.division_label.clone().map(|division| {
        AnyView::new(view! {
            field(class = "wt-pt-text", tabindex = Focus::Programmatic, state:read_only) {{division}}
        })
    });
    let clan = player.clan_tag.clone().map(|clan| {
        let ink = clan_ink(&player, cx);
        AnyView::new(view! {
            field(
                class = "wt-pt-text",
                attr:data-testid = format!("player-clan-{}", db_id.raw()),
                tabindex = Focus::Programmatic,
                state:read_only,
                {..ink}
            ) {{clan}}
        })
    });
    let name_color = player.is_abuser.then(|| player_color(PlayerColorKind::Abuser).to_owned());
    let name = player.display_name.clone();
    let hidden = player.is_hidden_profile.then(|| {
        let tip = t!("ui-replay-player-hidden_profile").into_owned();
        AnyView::new(view! {
            Tooltip {
                TooltipTrigger { AppIcon(glyph = Glyph::EyeOff) }
                TooltipContent {{tip.clone()}}
            }
        })
    });
    let twitch = twitch_chip(&player.twitch_candidates, cx);
    let connection = player.connection.as_ref().map(|note| connection_chip(note, cx));
    let actions = menu::row_actions(id, db_id, cx);
    let note = note_cell(cx, id, ReplayColumn::Name);

    AnyView::new(view! {
        box(
            class = "wt-pt-cell",
            attr:data-column = ReplayColumn::Name.storage_key(),
            attr:data-testid = format!("player-name-{}", db_id.raw()),
            on:pointer_down = note
        ) {
            box(class = "wt-pt-name") {
                {caret}
                {species}
                {division}
                {clan}
                field(
                    class = "wt-pt-text",
                    style:color = name_color,
                    attr:data-testid = format!("player-name-text-{}", db_id.raw()),
                    tabindex = Focus::Programmatic,
                    state:read_only
                ) {{name}}
                box(class = "wt-pt-fill") {}
                {hidden}
                {twitch}
                {connection}
            }
            {actions}
        }
    })
}

enum ConnectionHistory {
    NeverConnected(String),
    Rows(Vec<[String; 3]>),
}

impl ConnectionHistory {
    fn from_note(note: &ConnectionNote) -> Self {
        let ConnectionNote::Interrupted { events, .. } = note else {
            return Self::NeverConnected(t!("ui-replay-player-never_connected").into_owned());
        };

        let headings = [
            t!("ui-replay-player-disconnected_at").into_owned(),
            t!("ui-replay-player-reconnected_at").into_owned(),
            t!("ui-replay-player-disconnect_duration").into_owned(),
        ];
        let unknown = t!("ui-replay-player-reconnect_unknown");
        let rows = std::iter::once(headings).chain(ConnectionDrop::from_events(events).map(|drop| {
            [
                connection_clock(drop.disconnected_at),
                drop.reconnected_at.map_or_else(|| unknown.to_string(), connection_clock),
                drop.duration().map_or_else(|| "-".to_owned(), connection_clock),
            ]
        }));

        Self::Rows(rows.collect())
    }

    fn content(&self) -> AnyView {
        let rows = match self {
            Self::NeverConnected(message) => return AnyView::new(view! { text {{message.clone()}} }),
            Self::Rows(rows) => rows,
        };
        let cells: Vec<AnyView> = rows
            .iter()
            .enumerate()
            .flat_map(|(index, row)| {
                row.iter().cloned().map(move |value| {
                    AnyView::new(view! {
                        text(attr:data-heading = {(index == 0).then(String::new)}) {{value}}
                    })
                })
            })
            .collect();

        AnyView::new(view! {
            box(class = "wt-pt-connection-grid", attr:data-testid = "player-disconnect-history") {
                {cells}
            }
        })
    }

    fn copy_text(&self) -> String {
        match self {
            Self::NeverConnected(message) => message.clone(),
            Self::Rows(rows) => rows.iter().map(|row| row.join("\t")).collect::<Vec<_>>().join("\n"),
        }
    }
}

fn connection_clock(time: Duration) -> String {
    let secs = time.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
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

/// Near-black or white for the chip's text, whichever reads on its ground in each palette.
fn offline_ink(depth: f32, cx: TableCx) -> Attrs {
    let theme = cx.theme;
    let ink = move |desktop: Appearance| Some(text_on(offline_ground(theme.get().palette(desktop), depth)).to_string());
    Attrs::new()
        .class_toggle(ClassName::new("wt-pt-offline-ink"), true)
        .custom_property(CustomPropertyName::new("wt-offline-light"), move || ink(Appearance::Light))
        .custom_property(CustomPropertyName::new("wt-offline-dark"), move || ink(Appearance::Dark))
}

/// A disconnect chip's ground in `palette`: the rows' ground under a faint wash of text, reddened
/// by `depth`.
pub(super) fn offline_ground(palette: &Palette, depth: f32) -> Rgb {
    let washed = blend(palette.panel, palette.text, 0.08);
    blend(washed, Semantic::for_appearance(palette.appearance).error, depth)
}

/// `over` laid on `under` at `share` of its strength.
fn blend(under: Rgb, over: Rgb, share: f32) -> Rgb {
    let channel = |under: u8, over: u8| {
        let (under, over) = (f32::from(under), f32::from(over));
        (under + (over - under) * share).round().clamp(0.0, 255.0) as u8
    };
    Rgb::from_channels(
        channel(under.red(), over.red()),
        channel(under.green(), over.green()),
        channel(under.blue(), over.blue()),
    )
}

/// The plug and, when known, how long the player was offline, on a ground that reddens with that
/// time. A click copies what its tooltip says.
fn connection_chip(note: &ConnectionNote, cx: TableCx) -> AnyView {
    let history = StoredValue::new(ConnectionHistory::from_note(note));
    let time = time_offline(note);
    let depth = time.map(offline_depth);
    let ink = depth.map_or_else(Attrs::new, |depth| offline_ink(depth, cx));
    let depth = depth.map(|depth| format!("{}%", depth * 100.0));
    let offline = time.map(|time| AnyView::new(view! { text {{offline_label(time)}} }));
    let copy = move |ev: &mut EventCx<'_, events::Click>| {
        ev.stop_propagation();
        cx.copy(history.with_value(ConnectionHistory::copy_text), t!("ui-replay-context-copied").into_owned());
    };

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                box(
                    class = "wt-pt-offline",
                    var:--wt-pt-depth = depth,
                    attr:data-testid = "player-disconnect",
                    on:click = copy,
                    on:pointer_down = keep_primary_press,
                    on:double_click = keep_double,
                    {..ink}
                ) {
                    box { AppIcon(glyph = Glyph::Plug) }
                    {offline}
                }
            }
            TooltipContent {{history.with_value(ConnectionHistory::content)}}
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
                t!("ui-twitch-possible_name", name = candidate.login),
                t!("ui-twitch-seen_minutes", minutes = minutes)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    hover.push_str(&format!("\n\n{}", t!("ui-twitch-click_to_copy")));

    if candidates.len() == 1 {
        let login = first.login.clone();
        let copy = move |ev: &mut EventCx<'_, events::Click>| {
            ev.stop_propagation();
            cx.copy(login.clone(), t!("ui-twitch-copied", name = login).into_owned());
        };
        return Some(with_hover(
            AnyView::new(view! {
                box(
                    class = "wt-pt-marker",
                    attr:data-testid = "player-twitch",
                    on:click = copy,
                    on:pointer_down = keep_primary_press,
                    on:double_click = keep_double
                ) {
                    AppIcon(glyph = Glyph::Twitch)
                }
            }),
            Some(hover),
        ));
    }

    let logins = StoredValue::new_local(candidates.iter().map(|candidate| candidate.login.clone()).collect::<Vec<_>>());
    let items = move || {
        logins.with_value(|logins| {
            logins
                .iter()
                .map(|login| {
                    let login = login.clone();
                    let shown = login.clone();
                    let copy = UnsyncCallback::new(move |()| {
                        cx.copy(login.clone(), t!("ui-twitch-copied", name = login).into_owned());
                    });
                    AnyView::new(view! {
                        MenuItem(on_select = copy) {
                            box(class = "wt-pt-item-icon") { AppIcon(glyph = Glyph::Copy) }
                            {shown.clone()}
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
    };

    Some(with_hover(
        AnyView::new(view! {
            box(on:pointer_down = keep_primary_press, on:double_click = keep_double) {
                DropdownMenu {
                    DropdownMenuTrigger(variant = ButtonVariant::Ghost, size = ButtonSize::IconSm, attr:data-testid = "player-twitch") {
                        AppIcon(glyph = Glyph::Twitch)
                    }
                    DropdownMenuContent {{items()}}
                }
            }
        }),
        Some(hover),
    ))
}

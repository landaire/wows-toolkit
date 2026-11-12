//! Column set, cell formatting, and number formatting. Mirrors
//! `ui/replay_parser/sorting.rs` (`ReplayColumn`, declaration order) and the
//! per-cell mapping in `ui/replay_parser/mod.rs`'s `TableDelegate::cell_ui`.
//! Colors are represented as `ColorRole`, resolved to real colors by the
//! render layer (Milestone 2); no `gpui_kit`/`egui` color type appears here.

use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replay_insights::personal_rating::PersonalRatingCategory;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::breakdown;

use crate::model::PlayerRow;
use crate::sort::SortColumn;

/// All displayable columns in the replay player list, in the same
/// declaration order as the egui app's `ReplayColumn` (`sorting.rs:101`) minus
/// its `Actions` column: a row's actions are on the row itself here, revealed
/// on hover and on a right-click, so they need no column of their own.
/// `default_columns` re-sorts to this order after filtering, matching
/// `update_visible_columns`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplayColumn {
    Name,
    ShipName,
    Skills,
    PersonalRating,
    BaseXp,
    RawXp,
    Kills,
    ObservedDamage,
    ActualDamage,
    ReceivedDamage,
    SpottingDamage,
    PotentialDamage,
    Hits,
    Heals,
    DistanceTraveled,
    TimeLived,
}

impl ReplayColumn {
    pub const ALL: [ReplayColumn; 16] = [
        ReplayColumn::Name,
        ReplayColumn::ShipName,
        ReplayColumn::Skills,
        ReplayColumn::PersonalRating,
        ReplayColumn::BaseXp,
        ReplayColumn::RawXp,
        ReplayColumn::Kills,
        ReplayColumn::ObservedDamage,
        ReplayColumn::ActualDamage,
        ReplayColumn::ReceivedDamage,
        ReplayColumn::SpottingDamage,
        ReplayColumn::PotentialDamage,
        ReplayColumn::Hits,
        ReplayColumn::Heals,
        ReplayColumn::DistanceTraveled,
        ReplayColumn::TimeLived,
    ];

    /// The `ReplaySettings` flag that gates this column's visibility, for the
    /// five user-toggleable columns. `None` for always-on columns. Mirrors
    /// `update_visible_columns`'s `optional_columns` table.
    fn settings_gate(self) -> Option<fn(&ReplaySettings) -> bool> {
        match self {
            ReplayColumn::RawXp => Some(|s| s.show_raw_xp),
            ReplayColumn::ObservedDamage => Some(|s| s.show_observed_damage),
            ReplayColumn::Heals => Some(|s| s.show_heals),
            ReplayColumn::ReceivedDamage => Some(|s| s.show_received_damage),
            ReplayColumn::DistanceTraveled => Some(|s| s.show_distance_traveled),
            _ => None,
        }
    }

    /// The name this column is stored under in the settings database. Fixed
    /// strings rather than positions or variant names, so a stored
    /// arrangement still reads after a column is added, dropped or renamed.
    pub const fn storage_key(self) -> &'static str {
        match self {
            ReplayColumn::Name => "name",
            ReplayColumn::ShipName => "ship_name",
            ReplayColumn::Skills => "skills",
            ReplayColumn::PersonalRating => "personal_rating",
            ReplayColumn::BaseXp => "base_xp",
            ReplayColumn::RawXp => "raw_xp",
            ReplayColumn::Kills => "kills",
            ReplayColumn::ObservedDamage => "observed_damage",
            ReplayColumn::ActualDamage => "actual_damage",
            ReplayColumn::ReceivedDamage => "received_damage",
            ReplayColumn::SpottingDamage => "spotting_damage",
            ReplayColumn::PotentialDamage => "potential_damage",
            ReplayColumn::Hits => "hits",
            ReplayColumn::Heals => "heals",
            ReplayColumn::DistanceTraveled => "distance_traveled",
            ReplayColumn::TimeLived => "time_lived",
        }
    }

    pub fn from_storage_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|col| col.storage_key() == key)
    }

    /// The translation key of the column's heading.
    pub const fn label_key(self) -> &'static str {
        match self {
            ReplayColumn::Name => "ui.replay.column.player_name",
            ReplayColumn::ShipName => "ui.replay.column.ship_name",
            ReplayColumn::Skills => "ui.replay.column.skills",
            ReplayColumn::PersonalRating => "ui.replay.column.personal_rating",
            ReplayColumn::BaseXp => "stat.base_xp",
            ReplayColumn::RawXp => "stat.raw_xp",
            ReplayColumn::Kills => "ui.replay.column.kills",
            ReplayColumn::ObservedDamage => "ui.replay.column.observed_damage",
            ReplayColumn::ActualDamage => "ui.replay.column.actual_damage",
            ReplayColumn::ReceivedDamage => "ui.replay.column.received_damage",
            ReplayColumn::SpottingDamage => "stat.spotting_damage",
            ReplayColumn::PotentialDamage => "ui.replay.column.potential_damage",
            ReplayColumn::Hits => "ui.replay.column.hits",
            ReplayColumn::Heals => "ui.replay.column.heals",
            ReplayColumn::DistanceTraveled => "ui.replay.column.distance_traveled",
            ReplayColumn::TimeLived => "ui.replay.column.time_lived",
        }
    }

    /// The column's heading in the reader's own language.
    pub fn label(self) -> String {
        t!(self.label_key()).into_owned()
    }

    /// What a press on the column's header sorts by. `None` for the columns
    /// the egui app renders as plain headers: Skills and Time Lived.
    pub const fn sort_column(self) -> Option<SortColumn> {
        match self {
            ReplayColumn::Skills | ReplayColumn::TimeLived => None,
            ReplayColumn::Name => Some(SortColumn::Name),
            ReplayColumn::ShipName => Some(SortColumn::ShipName),
            ReplayColumn::PersonalRating => Some(SortColumn::PersonalRating),
            ReplayColumn::BaseXp => Some(SortColumn::BaseXp),
            ReplayColumn::RawXp => Some(SortColumn::RawXp),
            ReplayColumn::Kills => Some(SortColumn::Kills),
            ReplayColumn::ObservedDamage => Some(SortColumn::ObservedDamage),
            ReplayColumn::ActualDamage => Some(SortColumn::ActualDamage),
            ReplayColumn::ReceivedDamage => Some(SortColumn::ReceivedDamage),
            ReplayColumn::SpottingDamage => Some(SortColumn::SpottingDamage),
            ReplayColumn::PotentialDamage => Some(SortColumn::PotentialDamage),
            ReplayColumn::Hits => Some(SortColumn::Hits),
            ReplayColumn::Heals => Some(SortColumn::Heals),
            ReplayColumn::DistanceTraveled => Some(SortColumn::DistanceTraveled),
        }
    }
}

/// The column set for a fresh replay load, given the persisted display
/// settings: always-on columns unconditionally, the five optional columns
/// only when their `ReplaySettings` flag is set, in declaration order.
/// Mirrors `UiReport::update_visible_columns`.
pub fn default_columns(settings: &ReplaySettings) -> Vec<ReplayColumn> {
    ReplayColumn::ALL.into_iter().filter(|col| col.settings_gate().is_none_or(|gate| gate(settings))).collect()
}

/// Which part of the table a column sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Held at the table's left edge while the rest scroll.
    Pinned,
    Scrolling,
}

/// Where a moved column lands: its section, and how many of that section's
/// shown columns, other than itself, stand before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub section: Section,
    pub index: usize,
}

/// The columns a table shows, in the order it draws them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShownColumns {
    pub pinned: Vec<ReplayColumn>,
    pub scrolling: Vec<ReplayColumn>,
}

impl ShownColumns {
    /// Every shown column, left to right.
    pub fn iter(&self) -> impl Iterator<Item = ReplayColumn> + '_ {
        self.pinned.iter().chain(&self.scrolling).copied()
    }
}

/// Where the reader has put the columns: the pinned ones, which stay at the
/// table's left edge, then the rest, each in the order drawn. Every column
/// appears once whether it is shown or not, so a column the filters bring
/// back returns to where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnArrangement {
    pinned: Vec<ReplayColumn>,
    scrolling: Vec<ReplayColumn>,
}

impl ColumnArrangement {
    /// The player and the ship, which say whose row it is, as the egui
    /// table's frozen columns do.
    const DEFAULT_PINNED: [ReplayColumn; 2] = [ReplayColumn::Name, ReplayColumn::ShipName];

    pub fn new() -> Self {
        let pinned = Self::DEFAULT_PINNED.to_vec();
        let scrolling = ReplayColumn::ALL.into_iter().filter(|col| !pinned.contains(col)).collect();

        Self { pinned, scrolling }
    }

    pub fn is_pinned(&self, col: ReplayColumn) -> bool {
        self.pinned.contains(&col)
    }

    /// The columns of `shown`, in the order they are drawn.
    pub fn shown(&self, shown: &[ReplayColumn]) -> ShownColumns {
        let pick = |list: &[ReplayColumn]| list.iter().copied().filter(|col| shown.contains(col)).collect();

        ShownColumns { pinned: pick(&self.pinned), scrolling: pick(&self.scrolling) }
    }

    /// Pins `col` after the columns already pinned.
    pub fn pin(&mut self, col: ReplayColumn) {
        self.remove(col);
        self.pinned.push(col);
    }

    /// Unpins `col` to the first scrolling place, which is beside the pinned
    /// edge it stood against.
    pub fn unpin(&mut self, col: ReplayColumn) {
        self.remove(col);
        self.scrolling.insert(0, col);
    }

    /// Moves `col` to `placement`, counted among the columns in `shown`.
    pub fn place(&mut self, col: ReplayColumn, placement: Placement, shown: &[ReplayColumn]) {
        self.remove(col);
        let list = self.list_mut(placement.section);
        let shown_at: Vec<usize> =
            list.iter().enumerate().filter(|(_, other)| shown.contains(other)).map(|(position, _)| position).collect();

        // Past the last shown column it goes directly after that one, ahead
        // of any hidden ones behind it; with none shown, first.
        let at = match shown_at.get(placement.index) {
            Some(position) => *position,
            None => shown_at.last().map_or(0, |last| last + 1),
        };
        list.insert(at, col);
    }

    pub fn to_stored(&self) -> StoredArrangement {
        let keys = |list: &[ReplayColumn]| list.iter().map(|col| col.storage_key().to_owned()).collect();

        StoredArrangement { pinned: keys(&self.pinned), scrolling: keys(&self.scrolling) }
    }

    /// Reads a stored arrangement back, whatever version of the column set
    /// wrote it.
    pub fn from_stored(stored: &StoredArrangement) -> Self {
        let mut arrangement = Self { pinned: Vec::new(), scrolling: Vec::new() };

        // A name no column answers to is a column since removed, and a
        // repeated one would place a column twice: both are passed over.
        for (keys, section) in [(&stored.pinned, Section::Pinned), (&stored.scrolling, Section::Scrolling)] {
            for col in keys.iter().filter_map(|key| ReplayColumn::from_storage_key(key)) {
                if !arrangement.contains(col) {
                    arrangement.list_mut(section).push(col);
                }
            }
        }

        // A column the stored list does not name was added after it was
        // saved. It takes its default section, after the nearest column that
        // precedes it there by default, so the reader's own order is kept.
        let default = Self::new();
        for section in [Section::Pinned, Section::Scrolling] {
            let default_list = default.list(section);
            for (default_at, col) in default_list.iter().copied().enumerate() {
                if arrangement.contains(col) {
                    continue;
                }
                let list = arrangement.list_mut(section);
                let at = default_list[..default_at]
                    .iter()
                    .rev()
                    .find_map(|before| list.iter().position(|other| other == before))
                    .map_or(0, |position| position + 1);
                list.insert(at, col);
            }
        }

        arrangement
    }

    fn contains(&self, col: ReplayColumn) -> bool {
        self.pinned.contains(&col) || self.scrolling.contains(&col)
    }

    fn remove(&mut self, col: ReplayColumn) {
        self.pinned.retain(|other| *other != col);
        self.scrolling.retain(|other| *other != col);
    }

    fn list(&self, section: Section) -> &[ReplayColumn] {
        match section {
            Section::Pinned => &self.pinned,
            Section::Scrolling => &self.scrolling,
        }
    }

    fn list_mut(&mut self, section: Section) -> &mut Vec<ReplayColumn> {
        match section {
            Section::Pinned => &mut self.pinned,
            Section::Scrolling => &mut self.scrolling,
        }
    }
}

impl Default for ColumnArrangement {
    fn default() -> Self {
        Self::new()
    }
}

/// A `ColumnArrangement` as the settings database keeps it: columns by name,
/// so the list survives the column set changing between versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredArrangement {
    pub pinned: Vec<String>,
    pub scrolling: Vec<String>,
}

/// A color role a cell can carry, resolved to a real color by the render
/// layer (Milestone 2's palette resolver). Never an actual RGBA/HSLA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    /// Team/division-relative player color for stat cells and (via
    /// `PlayerColorKind::Abuser`) the name cell.
    Player(PlayerColorKind),
    /// Personal Rating tier tint.
    PrTier(PersonalRatingCategory),
    /// A rating band's canonical hue, for the chip background behind
    /// [`ColorRole::PrTier`] text.
    PrTierTint(PersonalRatingCategory),
    /// Captain-points tier tint.
    CaptainPoints(CaptainPointsTier),
    /// Battle-outcome coloring (win/loss/draw), for the per-replay header.
    WinLoss(BattleOutcome),
    /// A packed `0xRRGGBB` color read verbatim from replay/game data (e.g.
    /// the clan-league color).
    Fixed(u32),
}

/// Team/division/abuse-relative player color. Self = white, ally = light
/// green, enemy = light red, division-mate (not self) = gold, abuser = pink.
/// Mirrors `player_color_for_team_relation` plus the division/abuser
/// overrides in `ui/replay_parser/mod.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerColorKind {
    SelfPlayer,
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

/// Captain-points tier tint. 0-9 = Bad, 10-12 = Warning, 13-16 = Caution,
/// 17+ = Good; forced `Bad` on the "tower defense" / "no skills above tier
/// 2" warnings regardless of point count. Mirrors
/// `util::colorize_captain_points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptainPointsTier {
    Bad,
    Warning,
    Caution,
    Good,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleOutcome {
    Win,
    Loss,
    Draw,
}

pub const NDA: &str = "NDA";
const DASH: &str = "-";

/// Display text, optional color role, and optional hover text for one cell.
/// Plain data: no gpui_kit/egui type appears here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellValue {
    pub text: String,
    pub color: Option<ColorRole>,
    pub hover: Option<String>,
}

impl CellValue {
    fn plain(text: impl Into<String>) -> Self {
        CellValue { text: text.into(), color: None, hover: None }
    }

    fn colored(text: impl Into<String>, color: ColorRole) -> Self {
        CellValue { text: text.into(), color: Some(color), hover: None }
    }

    fn with_hover(mut self, hover: impl Into<String>) -> Self {
        self.hover = Some(hover.into());
        self
    }
}

/// The team/division-relative color of a row: the ship-class icon tint and
/// the actions menu heading.
pub fn player_color_kind(row: &PlayerRow) -> PlayerColorKind {
    if row.is_self_division_mate {
        return PlayerColorKind::DivisionMate;
    }
    if row.relation.is_self() {
        PlayerColorKind::SelfPlayer
    } else if row.relation.is_ally() {
        PlayerColorKind::Ally
    } else {
        PlayerColorKind::Enemy
    }
}

/// The actions menu heading's name color: pink for abusers, otherwise the
/// row's team/division color; see `player_color_kind`.
pub fn name_color_kind(row: &PlayerRow) -> PlayerColorKind {
    if row.is_abuser { PlayerColorKind::Abuser } else { player_color_kind(row) }
}

fn captain_points_tier(row: &PlayerRow) -> CaptainPointsTier {
    if row.skill_warning.is_some() {
        return CaptainPointsTier::Bad;
    }
    match row.skill_points {
        0..=9 => CaptainPointsTier::Bad,
        10..=12 => CaptainPointsTier::Warning,
        13..=16 => CaptainPointsTier::Caution,
        _ => CaptainPointsTier::Good,
    }
}

/// Cell text/color/hover for one column of one row. `debug` mirrors the
/// app's debug-mode flag, which lifts NDA hiding and the enemy-only Skills
/// gate (the brief's sketch omits this parameter, but NDA gating and the
/// egui source both key off it alongside `should_hide_stats()`). Mirrors the
/// per-column mapping in `ui/replay_parser/mod.rs`'s `TableDelegate::cell_ui`
/// (values only; icons, menus, and multi-segment name coloring are
/// Milestone 2).
pub fn cell_value(row: &PlayerRow, col: ReplayColumn, debug: bool) -> CellValue {
    let nda_active = row.should_hide_stats() && !debug;

    match col {
        ReplayColumn::Name => {
            let mut parts = Vec::new();
            if let Some(div) = row.division_label.as_ref() {
                parts.push(div.clone());
            }
            if let Some(clan) = row.clan_tag.as_ref() {
                parts.push(clan.clone());
            }
            parts.push(row.display_name.clone());
            // The stripe and class icon say the team; the name is marked only for abuse.
            let text = parts.join(" ");
            if row.is_abuser {
                CellValue::colored(text, ColorRole::Player(PlayerColorKind::Abuser))
            } else {
                CellValue::plain(text)
            }
        }
        ReplayColumn::ShipName => CellValue::plain(row.ship_name.clone()),
        ReplayColumn::Skills => {
            if row.relation.is_enemy() && !debug {
                CellValue::plain(DASH)
            } else if !row.has_vehicle_entity {
                CellValue::colored(DASH, ColorRole::CaptainPoints(CaptainPointsTier::Bad))
                    .with_hover(t!("ui.replay.build.not_spotted").into_owned())
            } else {
                let mut cell = CellValue::colored(
                    row.skill_label_text.clone(),
                    ColorRole::CaptainPoints(captain_points_tier(row)),
                );
                if let Some(hover) = row.skill_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
        }
        ReplayColumn::PersonalRating => match row.personal_rating.as_ref() {
            Some(pr) => CellValue::colored(format!("{:.0}", pr.pr), ColorRole::PrTier(pr.category))
                .with_hover(pr.category.name()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::BaseXp => match row.base_xp_text.as_ref() {
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::RawXp => match row.raw_xp_text.as_ref() {
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::Kills => CellValue::plain(row.kills.unwrap_or(row.observed_kills).to_string()),
        ReplayColumn::ObservedDamage => {
            if nda_active {
                CellValue::plain(NDA)
            } else {
                CellValue::plain(row.observed_damage_text.clone())
            }
        }
        // The table draws these two cells' per-type split as a bar with its
        // own tooltip (`damage_mix`).
        ReplayColumn::ActualDamage => match row.actual_damage_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::ReceivedDamage => match row.received_damage_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::SpottingDamage => match row.spotting_damage_text.as_ref() {
            Some(text) => {
                let mut cell = CellValue::plain(text.clone());
                if let Some(hover) = row.spotting_damage_breakdown.as_ref() {
                    cell = cell.with_hover(breakdown::padded_text(hover));
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::PotentialDamage => match row.potential_damage_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => {
                let mut cell = CellValue::plain(text.clone());
                if let Some(hover) = row.potential_damage_breakdown.as_ref() {
                    cell = cell.with_hover(breakdown::padded_text(hover));
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::Hits => match row.hits_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => {
                let mut cell = CellValue::plain(text.clone());
                if let Some(hover) = row.hits_breakdown.as_ref() {
                    cell = cell.with_hover(breakdown::padded_text(hover));
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::Heals => match row.heal_count {
            Some(count) => {
                CellValue::plain(count.to_string()).with_hover(t!("ui.replay.column.heals_tooltip").into_owned())
            }
            None => CellValue::plain(DASH).with_hover(t!("ui.replay.column.heals_no_repair_tooltip").into_owned()),
        },
        ReplayColumn::DistanceTraveled => match row.distance_traveled {
            Some(distance) => CellValue::plain(format!("{distance:.2}km")),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::TimeLived => match row.time_lived_text.as_ref() {
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
    }
}

/// One row's cells in `columns`, tab separated, which is what a spreadsheet
/// reads back as columns.
pub fn row_text(row: &PlayerRow, columns: &[ReplayColumn], debug: bool) -> String {
    columns.iter().map(|col| cell_value(row, *col, debug).text).collect::<Vec<_>>().join("\t")
}

/// The column headings, then every row in the order given.
pub fn table_text<'a>(rows: impl Iterator<Item = &'a PlayerRow>, columns: &[ReplayColumn], debug: bool) -> String {
    let mut lines = vec![columns.iter().map(|col| col.label()).collect::<Vec<_>>().join("\t")];
    lines.extend(rows.map(|row| row_text(row, columns, debug)));
    lines.join("\n")
}

/// `rows` as a Markdown table under the column headings.
pub fn markdown_table<'a>(rows: impl Iterator<Item = &'a PlayerRow>, columns: &[ReplayColumn], debug: bool) -> String {
    let line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
    let mut lines = vec![
        line(columns.iter().map(|col| markdown_cell(&col.label())).collect()),
        line(columns.iter().map(|_| "---".to_owned()).collect()),
    ];
    lines.extend(rows.map(|row| line(columns.iter().map(|col| markdown_cell(&cell_value(row, *col, debug).text)).collect())));
    lines.join("\n")
}

fn markdown_cell(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.replace("\r\n", "\n").chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' | '\r' => escaped.push_str("<br>"),
            '\\' | '|' | '`' | '*' | '_' | '[' | ']' | '~' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::personal_rating::PersonalRatingResult;
    use wows_replays::types::Relation;

    use super::*;
    use crate::test_support::base_row;

    #[test]
    fn a_row_and_the_table_copy_as_tab_separated_text() {
        let rows = [base_row(1, Relation::new(0), true), base_row(2, Relation::new(2), false)];
        let columns = ReplayColumn::ALL;

        let row = row_text(&rows[0], &columns, false);
        assert_eq!(row.split('\t').count(), columns.len(), "one field per visible column");
        let ship = cell_value(&rows[0], ReplayColumn::ShipName, false).text;
        assert!(row.split('\t').any(|field| field == ship), "the cell's text is one of the row's fields");

        let whole = table_text(rows.iter(), &columns, false);
        let lines: Vec<&str> = whole.lines().collect();
        assert_eq!(lines.len(), rows.len() + 1, "a heading line and one line per row");
        assert_eq!(lines[0].split('\t').count(), columns.len(), "the heading names every column");
        assert_eq!(lines[1], row, "the first line under the heading is the first row");
    }

    #[test]
    fn a_markdown_table_escapes_what_markdown_reads() {
        assert_eq!(markdown_cell("a|b_c\nd<e>"), "a\\|b\\_c<br>d&lt;e&gt;");

        let rows = [base_row(1, Relation::new(0), true)];
        let table = markdown_table(rows.iter(), &[ReplayColumn::Kills], false);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 3, "the heading, its rule and the row");
        assert_eq!(lines[1], "| --- |");
    }

    #[test]
    fn default_columns_always_on_only() {
        let settings = ReplaySettings {
            show_raw_xp: false,
            show_observed_damage: false,
            show_heals: false,
            show_received_damage: false,
            show_distance_traveled: false,
            ..ReplaySettings::default()
        };

        let columns = default_columns(&settings);

        assert!(!columns.contains(&ReplayColumn::RawXp));
        assert!(!columns.contains(&ReplayColumn::Heals));
        assert!(!columns.contains(&ReplayColumn::ReceivedDamage));
        assert!(!columns.contains(&ReplayColumn::DistanceTraveled));
        // ObservedDamage is off by ReplaySettings::default() already, and
        // stays off here too.
        assert!(!columns.contains(&ReplayColumn::ObservedDamage));
        // Always-on columns remain, in declaration order.
        assert_eq!(columns.first(), Some(&ReplayColumn::Name));
        assert!(columns.contains(&ReplayColumn::Name));
        assert!(columns.contains(&ReplayColumn::PersonalRating));
    }

    #[test]
    fn default_columns_includes_optional_columns_when_enabled() {
        let settings = ReplaySettings::default();
        let columns = default_columns(&settings);

        // ReplaySettings::default() enables heals/received/distance but not
        // raw_xp/observed_damage.
        assert!(columns.contains(&ReplayColumn::Heals));
        assert!(columns.contains(&ReplayColumn::ReceivedDamage));
        assert!(columns.contains(&ReplayColumn::DistanceTraveled));
        assert!(!columns.contains(&ReplayColumn::RawXp));
        assert!(!columns.contains(&ReplayColumn::ObservedDamage));

        // Declaration order is preserved (Heals sits before DistanceTraveled).
        let heals_ix = columns.iter().position(|c| *c == ReplayColumn::Heals).unwrap();
        let distance_ix = columns.iter().position(|c| *c == ReplayColumn::DistanceTraveled).unwrap();
        assert!(heals_ix < distance_ix);
    }

    #[test]
    fn cell_value_name_joins_division_clan_and_display_name() {
        let row = PlayerRow {
            display_name: "Steve".to_string(),
            clan_tag: Some("[ABC]".to_string()),
            division_label: Some("(A)".to_string()),
            ..base_row(1, Relation::new(1), false)
        };

        let cell = cell_value(&row, ReplayColumn::Name, false);

        assert_eq!(cell.text, "(A) [ABC] Steve");
        assert_eq!(cell.color, None, "the stripe and class icon say the team");
    }

    #[test]
    fn cell_value_name_colors_abuser_pink_even_as_division_mate() {
        let row = PlayerRow {
            display_name: "Rude".to_string(),
            is_abuser: true,
            is_self_division_mate: true,
            ..base_row(1, Relation::new(1), false)
        };

        let cell = cell_value(&row, ReplayColumn::Name, false);

        assert_eq!(cell.color, Some(ColorRole::Player(PlayerColorKind::Abuser)));
    }

    #[test]
    fn cell_value_actual_damage_shows_nda_when_hidden_and_not_debug() {
        let row = PlayerRow {
            is_test_ship: true,
            actual_damage_text: Some("50,000".to_string()),
            ..base_row(1, Relation::new(2), false)
        };

        let hidden = cell_value(&row, ReplayColumn::ActualDamage, false);
        assert_eq!(hidden.text, "NDA");

        let debug_visible = cell_value(&row, ReplayColumn::ActualDamage, true);
        assert_eq!(debug_visible.text, "50,000");
    }

    #[test]
    fn cell_value_actual_damage_missing_is_a_dash_regardless_of_nda() {
        let row = base_row(1, Relation::new(1), false);
        let cell = cell_value(&row, ReplayColumn::ActualDamage, false);
        assert_eq!(cell.text, "-");
    }

    #[test]
    fn cell_value_observed_damage_is_uncolored_unlike_actual_damage() {
        let row = PlayerRow { observed_damage_text: "12,345".to_string(), ..base_row(1, Relation::new(1), false) };

        let cell = cell_value(&row, ReplayColumn::ObservedDamage, false);

        assert_eq!(cell.text, "12,345");
        assert_eq!(cell.color, None, "ObservedDamage renders as a bare label in the egui original, never colored");
    }

    #[test]
    fn cell_value_observed_damage_shows_nda_when_hidden_and_not_debug() {
        let row = PlayerRow {
            is_test_ship: true,
            observed_damage_text: "12,345".to_string(),
            ..base_row(1, Relation::new(2), false)
        };

        let hidden = cell_value(&row, ReplayColumn::ObservedDamage, false);
        assert_eq!(hidden.text, "NDA");

        let debug_visible = cell_value(&row, ReplayColumn::ObservedDamage, true);
        assert_eq!(debug_visible.text, "12,345");
    }

    // The following two tests exercise only `cell_value`'s render mapping for
    // an already-`Some` `PersonalRatingResult`; they say nothing about how
    // `personal_rating` gets populated. `from_normalized` always leaves it
    // `None` (see `model.rs`'s `PlayerRow::personal_rating` doc); a real PR
    // value comes from `ReplayReportModel::populate_personal_ratings`.

    #[test]
    fn cell_value_personal_rating_shows_rounded_score_and_tier_color() {
        let row = PlayerRow {
            personal_rating: Some(PersonalRatingResult::new(1600.0)),
            ..base_row(1, Relation::new(0), true)
        };

        let cell = cell_value(&row, ReplayColumn::PersonalRating, false);

        assert_eq!(cell.text, "1600");
        assert_eq!(cell.color, Some(ColorRole::PrTier(PersonalRatingCategory::VeryGood)));
        assert_eq!(cell.hover.as_deref(), Some("Very Good"));
    }

    #[test]
    fn cell_value_personal_rating_missing_is_a_dash() {
        let row = base_row(1, Relation::new(0), true);
        let cell = cell_value(&row, ReplayColumn::PersonalRating, false);
        assert_eq!(cell.text, "-");
        assert_eq!(cell.color, None);
    }

    /// A stored arrangement from another version: a column it names that no
    /// longer exists and a repeated name are passed over, and the columns it
    /// does not name take their default places among the reader's own order.
    #[test]
    fn a_stored_arrangement_reads_back_across_column_set_changes() {
        let stored = StoredArrangement {
            pinned: vec!["skills".into(), "retired_column".into(), "name".into()],
            scrolling: vec!["kills".into(), "base_xp".into(), "kills".into(), "time_lived".into()],
        };

        let arrangement = ColumnArrangement::from_stored(&stored);

        assert_eq!(
            arrangement.shown(&ReplayColumn::ALL),
            ShownColumns {
                pinned: vec![ReplayColumn::Skills, ReplayColumn::Name, ReplayColumn::ShipName],
                scrolling: vec![
                    ReplayColumn::PersonalRating,
                    ReplayColumn::Kills,
                    ReplayColumn::ObservedDamage,
                    ReplayColumn::ActualDamage,
                    ReplayColumn::ReceivedDamage,
                    ReplayColumn::SpottingDamage,
                    ReplayColumn::PotentialDamage,
                    ReplayColumn::Hits,
                    ReplayColumn::Heals,
                    ReplayColumn::DistanceTraveled,
                    ReplayColumn::BaseXp,
                    ReplayColumn::RawXp,
                    ReplayColumn::TimeLived,
                ],
            }
        );
        assert_eq!(
            ColumnArrangement::from_stored(&arrangement.to_stored()),
            arrangement,
            "a saved arrangement reads back"
        );
    }

    /// A placement counts only the shown columns, so a column hidden by the
    /// filters does not shift where a dragged one lands.
    #[test]
    fn a_placement_counts_shown_columns_only() {
        let mut arrangement = ColumnArrangement::new();
        let shown = [ReplayColumn::Name, ReplayColumn::ShipName, ReplayColumn::Skills, ReplayColumn::Kills];

        arrangement.place(ReplayColumn::Kills, Placement { section: Section::Scrolling, index: 0 }, &shown);
        assert_eq!(arrangement.shown(&shown).scrolling, vec![ReplayColumn::Kills, ReplayColumn::Skills]);

        arrangement.place(ReplayColumn::Kills, Placement { section: Section::Pinned, index: 1 }, &shown);
        assert_eq!(
            arrangement.shown(&shown).pinned,
            vec![ReplayColumn::Name, ReplayColumn::Kills, ReplayColumn::ShipName]
        );
        assert!(arrangement.is_pinned(ReplayColumn::Kills));
    }
}

//! The values the tracker's TSX view draws: every string already worded in
//! the reader's language, every rule already applied.

use nsh_core::prelude::*;
use wows_replays::types::AccountId;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::EncounterSeverity;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;

use crate::js_newtype::js_newtype;

use super::error::TrackerError;

/// An account id as the view holds it: decimal text, since the view's
/// numbers cannot hold every `i64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdText(String);

js_newtype!(AccountIdText, String);

impl AccountIdText {
    pub fn to_account(&self) -> Result<AccountId, TrackerError> {
        self.0.parse::<i64>().map(AccountId).map_err(|_| TrackerError::AccountId { value: self.0.clone() })
    }
}

impl From<AccountId> for AccountIdText {
    fn from(account: AccountId) -> Self {
        Self(account.0.to_string())
    }
}

/// How far back the tracker looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Period {
    LastHour,
    LastSixHours,
    #[default]
    LastDay,
    LastWeek,
    LastMonth,
    AllTime,
}

impl From<TimePeriod> for Period {
    fn from(period: TimePeriod) -> Self {
        match period {
            TimePeriod::LastHour => Self::LastHour,
            TimePeriod::LastSixHours => Self::LastSixHours,
            TimePeriod::LastDay => Self::LastDay,
            TimePeriod::LastWeek => Self::LastWeek,
            TimePeriod::LastMonth => Self::LastMonth,
            TimePeriod::AllTime => Self::AllTime,
        }
    }
}

impl From<Period> for TimePeriod {
    fn from(period: Period) -> Self {
        match period {
            Period::LastHour => Self::LastHour,
            Period::LastSixHours => Self::LastSixHours,
            Period::LastDay => Self::LastDay,
            Period::LastWeek => Self::LastWeek,
            Period::LastMonth => Self::LastMonth,
            Period::AllTime => Self::AllTime,
        }
    }
}

/// A column of the players table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum PlayerColumn {
    Name,
    Clan,
    TotalEncounters,
    #[default]
    Encounters,
    LastEncountered,
}

impl From<SortColumn> for PlayerColumn {
    fn from(column: SortColumn) -> Self {
        match column {
            SortColumn::Name => Self::Name,
            SortColumn::Clan => Self::Clan,
            SortColumn::TotalEncounters => Self::TotalEncounters,
            SortColumn::Encounters => Self::Encounters,
            SortColumn::LastEncountered => Self::LastEncountered,
        }
    }
}

impl From<PlayerColumn> for SortColumn {
    fn from(column: PlayerColumn) -> Self {
        match column {
            PlayerColumn::Name => Self::Name,
            PlayerColumn::Clan => Self::Clan,
            PlayerColumn::TotalEncounters => Self::TotalEncounters,
            PlayerColumn::Encounters => Self::Encounters,
            PlayerColumn::LastEncountered => Self::LastEncountered,
        }
    }
}

/// A column of the clans table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum ClanColumn {
    Clan,
    Members,
    #[default]
    Encounters,
    EncountersInRange,
    Sightings,
    LastEncountered,
}

impl From<ClanSortColumn> for ClanColumn {
    fn from(column: ClanSortColumn) -> Self {
        match column {
            ClanSortColumn::Clan => Self::Clan,
            ClanSortColumn::Members => Self::Members,
            ClanSortColumn::Encounters => Self::Encounters,
            ClanSortColumn::EncountersInRange => Self::EncountersInRange,
            ClanSortColumn::Sightings => Self::Sightings,
            ClanSortColumn::LastEncountered => Self::LastEncountered,
        }
    }
}

impl From<ClanColumn> for ClanSortColumn {
    fn from(column: ClanColumn) -> Self {
        match column {
            ClanColumn::Clan => Self::Clan,
            ClanColumn::Members => Self::Members,
            ClanColumn::Encounters => Self::Encounters,
            ClanColumn::EncountersInRange => Self::EncountersInRange,
            ClanColumn::Sightings => Self::Sightings,
            ClanColumn::LastEncountered => Self::LastEncountered,
        }
    }
}

/// How loudly a number of encounters reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Severity {
    #[default]
    None,
    Noted,
    Warned,
    Heavy,
}

impl From<EncounterSeverity> for Severity {
    fn from(severity: EncounterSeverity) -> Self {
        match severity {
            EncounterSeverity::None => Self::None,
            EncounterSeverity::Noted => Self::Noted,
            EncounterSeverity::Warned => Self::Warned,
            EncounterSeverity::Heavy => Self::Heavy,
        }
    }
}

/// How much of each player the roster shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum ViewMode {
    Compact,
    #[default]
    Detailed,
}

impl From<CurrentMatchViewMode> for ViewMode {
    fn from(mode: CurrentMatchViewMode) -> Self {
        match mode {
            CurrentMatchViewMode::Compact => Self::Compact,
            CurrentMatchViewMode::Detailed => Self::Detailed,
        }
    }
}

impl From<ViewMode> for CurrentMatchViewMode {
    fn from(mode: ViewMode) -> Self {
        match mode {
            ViewMode::Compact => Self::Compact,
            ViewMode::Detailed => Self::Detailed,
        }
    }
}

/// Which scope a roster row's figures come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Scope {
    #[default]
    Overall,
    Ship,
}

impl From<WinRateMode> for Scope {
    fn from(mode: WinRateMode) -> Self {
        match mode {
            WinRateMode::Overall => Self::Overall,
            WinRateMode::Ship => Self::Ship,
        }
    }
}

impl From<Scope> for WinRateMode {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::Overall => Self::Overall,
            Scope::Ship => Self::Ship,
        }
    }
}

/// How a roster entry's name is coloured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Tint {
    SelfPlayer,
    #[default]
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

impl From<PlayerTint> for Tint {
    fn from(tint: PlayerTint) -> Self {
        match tint {
            PlayerTint::SelfPlayer => Self::SelfPlayer,
            PlayerTint::Ally => Self::Ally,
            PlayerTint::Enemy => Self::Enemy,
            PlayerTint::DivisionMate => Self::DivisionMate,
            PlayerTint::Abuser => Self::Abuser,
        }
    }
}

/// The tracker's fixed wording.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TrackerLabels {
    pub players_tab: String,
    pub current_match_tab: String,
    pub clans_tab: String,
    pub time_period: String,
    pub filter_hint: String,
    pub populate: String,
    pub clear: String,
    pub clear_confirm: String,
    pub cancel: String,
    pub show_division_mates: String,
    pub show_division_mates_hover: String,
    pub find_matches: String,
    pub find_clan_matches: String,
    pub notes_hint: String,
    pub copy_account_id: String,
    pub allies: String,
    pub player_filter: String,
    pub loading_players: String,
    pub loading_clans: String,
    /// "Copied WG ID %{id}", filled in by the view.
    pub copied_account_id: String,
    /// "Copied %{name}", filled in by the view.
    pub copied_login: String,
    pub enemies: String,
    pub player_column: String,
    pub ship_column: String,
    pub seen_column: String,
    pub open_wows_numbers: String,
    pub open_shipbuilds: String,
}

/// One choice of the period selector.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PeriodOption {
    pub period: Period,
    pub label: String,
}

/// One heading of the players table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlayerColumnHeader {
    pub column: PlayerColumn,
    pub label: String,
    /// The table is ordered by this column.
    pub active: bool,
    pub descending: bool,
}

/// One heading of the clans table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ClanColumnHeader {
    pub column: ClanColumn,
    pub label: String,
    pub active: bool,
    pub descending: bool,
}

/// One choice of the roster's density selector.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ViewModeOption {
    pub mode: ViewMode,
    pub label: String,
}

/// One choice of the roster's scope selector.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ScopeOption {
    pub scope: Scope,
    pub label: String,
}

/// The tracker's controls, as they stand.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TrackerControls {
    pub period: Period,
    pub periods: Vec<PeriodOption>,
    pub filter: String,
    pub show_division_mates: bool,
    pub view_mode: ViewMode,
    pub scope: Scope,
    pub view_modes: Vec<ViewModeOption>,
    pub scopes: Vec<ScopeOption>,
    pub player_columns: Vec<PlayerColumnHeader>,
    pub clan_columns: Vec<ClanColumnHeader>,
    /// Why the last note did not save, or why the notes could not be read.
    #[prop(skip_none)]
    pub note_error: Option<String>,
}

/// What an opened players row shows beneath itself.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlayerDetailView {
    #[prop(skip_none)]
    pub aliases: Option<String>,
    #[prop(skip_none)]
    pub last_encountered: Option<String>,
    pub match_count: String,
}

/// One row of the players table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlayerRowView {
    pub account_id: AccountIdText,
    pub name: String,
    pub clan: String,
    pub total_encounters: String,
    pub encounters_in_range: String,
    pub severity: Severity,
    pub last_seen: String,
    pub last_seen_exact: String,
    pub note: String,
    /// `None` for a player the tracker never recorded.
    #[prop(skip_none)]
    pub detail: Option<PlayerDetailView>,
}

/// The players table, or why it has no rows.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlayersTable {
    pub rows: Vec<PlayerRowView>,
    #[prop(skip_none)]
    pub status: Option<String>,
}

/// One member met from a clan.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ClanMemberView {
    pub account_id: AccountIdText,
    pub name: String,
    pub matches: String,
}

/// One row of the clans table.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ClanRowView {
    pub clan: String,
    pub members: String,
    pub matches: String,
    pub matches_in_range: String,
    pub severity: Severity,
    pub sightings: String,
    pub sightings_hover: String,
    pub last_seen: String,
    pub last_seen_exact: String,
    pub member_list: Vec<ClanMemberView>,
}

/// The clans table, or why it has no rows.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ClansTable {
    pub rows: Vec<ClanRowView>,
    #[prop(skip_none)]
    pub status: Option<String>,
    /// The table's index reads have not run since the last refresh.
    pub needs_load: bool,
}

/// A personal-rating band's colours, each `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct RatingBand {
    /// Laid faintly under a chip or a row.
    pub hue: String,
    /// The text colour on a dark theme.
    pub dark_text: String,
    /// The text colour on a light theme.
    pub light_text: String,
}

/// A figure coloured by the band it falls in.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct BandedText {
    pub text: String,
    pub band: RatingBand,
}

/// Why a roster row has no figures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum StatsMark {
    /// The player hid their statistics.
    Hidden,
    /// The service had nothing for them.
    #[default]
    Unavailable,
    /// Nothing was asked, or no answer came.
    Unknown,
}

/// The four figures one scope contributes to a roster row. A figure the
/// service did not give is `-`, and empty while the lookup is still running.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ScopeCellsView {
    /// Shown in place of the win rate when the row has no figures.
    #[prop(skip_none)]
    pub mark: Option<StatsMark>,
    #[prop(skip_none)]
    pub mark_hover: Option<String>,
    pub win_rate: String,
    #[prop(skip_none)]
    pub win_rate_band: Option<RatingBand>,
    /// Both scopes' rates, so one cell compares them.
    #[prop(skip_none)]
    pub win_rate_hover: Option<String>,
    pub personal_rating: String,
    #[prop(skip_none)]
    pub personal_rating_band: Option<RatingBand>,
    pub personal_rating_hover: String,
    pub avg_damage: String,
    pub battles: String,
}

/// One entry of the live roster.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct RosterRowView {
    /// The name with the clan tag the scan found, if any.
    pub name: String,
    pub tint: Tint,
    /// The ship class icon from the battle's build, as an SVG `data:` URL.
    /// The view tints it like the name and turns it upright.
    #[prop(skip_none)]
    pub class_icon: Option<String>,
    /// The ship class's name, the icon's hover.
    #[prop(skip_none)]
    pub class_name: Option<String>,
    pub ship: String,
    /// The band of the scope the row leads with, laid faintly under it.
    #[prop(skip_none)]
    pub band: Option<RatingBand>,
    /// One group of figures per visible scope, left to right.
    pub scopes: Vec<ScopeCellsView>,
    /// How often this player was met, empty when never.
    pub met: String,
    pub met_severity: Severity,
    pub met_hover: String,
    #[prop(skip_none)]
    pub note: Option<String>,
    /// Chat logins that plausibly name this player, alphabetical.
    pub twitch_logins: Vec<String>,
    pub twitch_hover: String,
    /// The player's pages, when the scan named the account and its realm.
    #[prop(skip_none)]
    pub wows_numbers_url: Option<String>,
    #[prop(skip_none)]
    pub shipbuilds_url: Option<String>,
    #[prop(skip_none)]
    pub account_id: Option<AccountIdText>,
}

/// One team of the live roster.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TeamView {
    pub title: String,
    pub players: String,
    /// The team's average win rate over the scope the rows lead with.
    #[prop(skip_none)]
    pub average_win_rate: Option<BandedText>,
    #[prop(skip_none)]
    pub average_rating: Option<BandedText>,
    pub rows: Vec<RosterRowView>,
}

/// The Current Match section.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LiveView {
    /// Why there is no roster: no directory, still checking, or no battle.
    #[prop(skip_none)]
    pub status: Option<String>,
    /// What is missing from the roster's figures, and why.
    #[prop(skip_none)]
    pub note: Option<String>,
    /// The scope headings over each group of figures, left to right.
    pub scope_headings: Vec<ScopeHeadings>,
    pub friendly: TeamView,
    pub enemy: TeamView,
}

/// The four figure headings one scope contributes to the roster header.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ScopeHeadings {
    pub win_rate: String,
    pub personal_rating: String,
    pub avg_damage: String,
    pub battles: String,
}

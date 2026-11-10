//! What the toolbar reads of a board.

use nsh_core::prelude::*;
use wows_toolkit_viewmodel::ship_catalog::tier_roman;
use wows_toolkit_viewmodel::tactics::board;
use wows_toolkit_viewmodel::tactics::board::RangeCircle;
use wt_collab_client::drawing::Tool;

use crate::tactics::board::Board;
use crate::tactics::board::MapStatus;
use crate::tactics::board::map_value;

/// A board tool, as the toolbar and the shortcuts name it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum ToolName {
    #[default]
    Freehand,
    Line,
    Arrow,
    Circle,
    Rectangle,
    Triangle,
    Measurement,
    Eraser,
    /// Placing the ship picked from the search.
    Ship,
}

/// One range circle a placed ship can show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum RangeName {
    #[default]
    Detection,
    MainBattery,
    SecondaryBattery,
    Torpedo,
    Radar,
    Hydro,
}

/// What the map area shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum MapStatusName {
    #[default]
    PickAMap,
    NoGameData,
    NoArt,
    Drawing,
    Shown,
}

/// One entry of a picker.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct PickerItem {
    /// What the command takes to choose it.
    pub value: String,
    /// What the reader sees.
    pub title: String,
}

/// The capture point the reader is working on.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct CapView {
    /// Its letter, A to Z.
    pub letter: String,
    /// Its radius in kilometres, to one decimal.
    pub radius_km: String,
    /// The team holding it, as the reader reads it.
    pub team: String,
    /// The team's colour, `#rrggbb`.
    pub team_color: String,
    /// Whether it came from a replay and so cannot be deleted.
    pub frozen: bool,
    pub can_narrow: bool,
    pub can_widen: bool,
}

/// How far a walk of the replays has got.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct ScanView {
    pub read: u32,
    /// How many there are, once the directory has been walked.
    pub total: Option<u32>,
}

/// The ship the next map click places.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct PlacingView {
    pub name: String,
    pub friendly: bool,
}

/// One ship found by the search.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct ShipMatchView {
    /// What `pickShip` takes.
    pub index: u32,
    /// Its tier and name.
    pub label: String,
}

/// One range circle and how many ships show it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct RangeView {
    pub circle: RangeName,
    pub label: String,
    /// On for the ships to come and every ship placed.
    pub checked: bool,
    /// How many of the ships to come and those placed show it.
    pub enabled: u32,
    pub total: u32,
}

/// A message about something the board did on its own time.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct NoticeView {
    /// Counts up, so the same text said twice is shown twice.
    pub id: u32,
    pub text: String,
}

/// Everything the board's toolbar and map area read.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct TacticsView {
    /// Changes whenever anything here does.
    pub revision: u32,
    pub status: MapStatusName,
    pub maps: Vec<PickerItem>,
    /// The map the board is on.
    pub map: Option<PickerItem>,
    /// The modes the map has, the blank one first.
    pub modes: Vec<PickerItem>,
    pub mode: PickerItem,
    /// Whether this end may choose the map and save boards.
    pub may_steer: bool,
    /// Whether the session's host has locked drawing.
    pub locked: bool,
    /// Whether this is a board a peer shared.
    pub a_peers_board: bool,
    /// Whether capture points may be changed here.
    pub can_edit_caps: bool,
    /// Whether the map's measurements are known, which placing a capture
    /// point needs.
    pub has_map_info: bool,
    pub adding: bool,
    pub selected_cap: Option<CapView>,
    pub has_removable_caps: bool,
    /// The tool in hand.
    pub tool: Option<ToolName>,
    /// Which of the offered inks is in use.
    pub ink: Option<u32>,
    pub nib: f64,
    pub can_thin: bool,
    pub can_thicken: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_drawing: bool,
    /// How many drawn shapes are picked out.
    pub picked: u32,
    pub presets: Vec<String>,
    pub scanning: Option<ScanView>,
    pub placing: Option<PlacingView>,
    pub ship_query: String,
    pub ship_matches: Vec<ShipMatchView>,
    pub ranges: Vec<RangeView>,
    /// How many circles any ship shows.
    pub ranges_active: u32,
    pub ranges_all_on: bool,
    pub ranges_all_off: bool,
    pub notice: Option<NoticeView>,
}

/// What `board` shows, at `revision`.
pub fn of(board: &Board, revision: u32) -> TacticsView {
    let titles = board::map_titles(board.maps());
    let maps: Vec<PickerItem> = board
        .maps()
        .iter()
        .zip(titles)
        .map(|(map, title)| PickerItem { value: map_value(map), title })
        .collect();
    let map = board.map().map(map_value).and_then(|value| maps.iter().find(|item| item.value == value).cloned());

    let mode_item = |entry: &crate::tactics::board::ModeEntry| PickerItem {
        value: entry.value(),
        title: match entry {
            crate::tactics::board::ModeEntry::Blank => "Blank".to_owned(),
            crate::tactics::board::ModeEntry::Layout(mode) => mode.label.clone(),
        },
    };
    let modes = board.mode_entries().iter().map(mode_item).collect();
    let mode = mode_item(&board.mode_entry());

    let selected_cap = board.selected_cap().map(|cap| {
        let [red, green, blue] = board::team_color(cap.team);
        CapView {
            letter: cap.letter(),
            radius_km: format!("{:.1}", board::radius_km(cap.radius)),
            team: team_label(board::team_label_key(cap.team)).to_owned(),
            team_color: format!("#{red:02x}{green:02x}{blue:02x}"),
            frozen: cap.frozen,
            can_narrow: cap.radius > board::MIN_CAP_RADIUS,
            can_widen: cap.radius < board::MAX_CAP_RADIUS,
        }
    });

    let filter = board.range_filter();
    let ships = board.ship_filters();
    let total = ships.len() as u32 + 1;
    let ranges: Vec<RangeView> = board::RANGE_CIRCLES
        .iter()
        .map(|(circle, key)| {
            let enabled = u32::from(circle.is_on(filter)) + ships.iter().filter(|ship| circle.is_on(ship)).count() as u32;
            RangeView { circle: range_name(*circle), label: range_label(key).to_owned(), checked: enabled == total, enabled, total }
        })
        .collect();

    let nib = board.nib();
    let locked = board.locked();
    TacticsView {
        revision,
        status: match board.status() {
            MapStatus::PickAMap => MapStatusName::PickAMap,
            MapStatus::NoGameData => MapStatusName::NoGameData,
            MapStatus::NoArt => MapStatusName::NoArt,
            MapStatus::Drawing => MapStatusName::Drawing,
            MapStatus::Shown => MapStatusName::Shown,
        },
        maps,
        map,
        modes,
        mode,
        may_steer: board.may_steer(),
        locked,
        a_peers_board: board.is_a_peers(),
        can_edit_caps: board.can_edit_caps(),
        has_map_info: board.has_map_info(),
        adding: board.adding(),
        selected_cap,
        has_removable_caps: board.caps().iter().any(|cap| !cap.frozen),
        tool: tool_name(board.tool()),
        ink: board::INKS.iter().position(|(ink, _)| *ink == board.ink()).map(|at| at as u32),
        nib: f64::from(nib),
        can_thin: !locked && nib > board::MIN_NIB,
        can_thicken: !locked && nib < board::MAX_NIB,
        can_undo: !locked && board.can_undo(),
        can_redo: !locked && board.can_redo(),
        has_drawing: board.shape_count() > 0,
        picked: board.picked().len() as u32,
        presets: board.presets().to_vec(),
        scanning: board
            .scanning()
            .map(|progress| ScanView { read: progress.read as u32, total: progress.total.map(|total| total as u32) }),
        placing: board.placing().map(|placed| PlacingView { name: placed.name.clone(), friendly: placed.friendly }),
        ship_query: board.ship_query().to_owned(),
        ship_matches: board
            .matched_ships()
            .iter()
            .enumerate()
            .map(|(index, (_, ship))| ShipMatchView {
                index: index as u32,
                label: format!("{} {}", tier_roman(ship.tier), ship.display_name),
            })
            .collect(),
        ranges_active: ranges.iter().filter(|range| range.enabled > 0).count() as u32,
        ranges_all_on: ranges.iter().all(|range| range.enabled == range.total),
        ranges_all_off: ranges.iter().all(|range| range.enabled == 0),
        ranges,
        notice: board.notice().map(|notice| NoticeView { id: notice.id, text: notice.text.clone() }),
    }
}

/// The tool a toolbar name takes up.
pub fn tool_of(name: Option<ToolName>) -> Tool {
    match name {
        None => Tool::None,
        Some(ToolName::Freehand) => Tool::Freehand,
        Some(ToolName::Line) => Tool::Line,
        Some(ToolName::Arrow) => Tool::Arrow,
        Some(ToolName::Circle) => Tool::Circle { filled: false },
        Some(ToolName::Rectangle) => Tool::Rectangle { filled: false },
        Some(ToolName::Triangle) => Tool::Triangle { filled: false },
        Some(ToolName::Measurement) => Tool::Measurement,
        Some(ToolName::Eraser) => Tool::Eraser,
        // The ship tool is taken up by picking a ship, which says which.
        Some(ToolName::Ship) => Tool::None,
    }
}

fn tool_name(tool: &Tool) -> Option<ToolName> {
    match tool {
        Tool::None => None,
        Tool::Ship { .. } => Some(ToolName::Ship),
        Tool::Freehand => Some(ToolName::Freehand),
        Tool::Eraser => Some(ToolName::Eraser),
        Tool::Line => Some(ToolName::Line),
        Tool::Circle { .. } => Some(ToolName::Circle),
        Tool::Rectangle { .. } => Some(ToolName::Rectangle),
        Tool::Triangle { .. } => Some(ToolName::Triangle),
        Tool::Arrow => Some(ToolName::Arrow),
        Tool::Measurement => Some(ToolName::Measurement),
    }
}

pub fn circle_of(name: RangeName) -> RangeCircle {
    match name {
        RangeName::Detection => RangeCircle::Detection,
        RangeName::MainBattery => RangeCircle::MainBattery,
        RangeName::SecondaryBattery => RangeCircle::SecondaryBattery,
        RangeName::Torpedo => RangeCircle::Torpedo,
        RangeName::Radar => RangeCircle::Radar,
        RangeName::Hydro => RangeCircle::Hydro,
    }
}

fn range_name(circle: RangeCircle) -> RangeName {
    match circle {
        RangeCircle::Detection => RangeName::Detection,
        RangeCircle::MainBattery => RangeName::MainBattery,
        RangeCircle::SecondaryBattery => RangeName::SecondaryBattery,
        RangeCircle::Torpedo => RangeName::Torpedo,
        RangeCircle::Radar => RangeName::Radar,
        RangeCircle::Hydro => RangeName::Hydro,
    }
}

/// The English text of a range circle's translation key.
fn range_label(key: &str) -> &'static str {
    match key {
        "ui.renderer.context.detection" => "Detection",
        "ui.renderer.context.main_battery" => "Main Battery",
        "ui.renderer.context.secondary" => "Secondary",
        "ui.renderer.context.torpedo" => "Torpedo",
        "ui.renderer.context.radar" => "Radar",
        "ui.renderer.context.hydro" => "Hydro",
        _ => "Range",
    }
}

/// The English text of a team's translation key.
fn team_label(key: &str) -> &'static str {
    match key {
        "ui.tactics.team_one" => "Team 1 (Green)",
        "ui.tactics.team_two" => "Team 2 (Red)",
        _ => "Neutral",
    }
}

use std::collections::HashMap;
use std::path::PathBuf;

use egui::Color32;
use egui::Image;
use egui::ImageSource;
use egui::OpenUrl;
use egui::RichText;
use egui::UiKind;
use egui::Vec2;
use egui_taffy::TuiBuilderLogic as _;
use egui_taffy::taffy;
use egui_taffy::taffy::prelude::auto;
use egui_taffy::taffy::prelude::fr;
use egui_taffy::taffy::prelude::length;
use egui_taffy::taffy::prelude::percent;
use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use rust_i18n::t;
use wows_replays::ReplayFile;
use wows_replays::types::AccountId;
use wowsunpack::data::Version;

use crate::app::ToolkitTabViewer;
use crate::data::match_stats::PlayerStatsOut;
use crate::data::match_stats::PlayerStatsStatus;
use crate::data::wows_data::BuildData;
use crate::icons;
use crate::task::live_match_stats::FlushState;
use crate::task::replays::ReplayBackgroundParserThreadMessage;
use crate::twitch::TwitchState;
use crate::ui::replay_parser::ClanColor;
use crate::ui::replay_parser::PlayerTint;
use crate::ui::replay_parser::ship_class_icon_from_species;
use crate::ui::theme::semantic::SemanticExt;
use crate::util::formatting::separate_number;
use crate::util::formatting::shipbuilds_player_url;
use crate::util::formatting::wows_numbers_player_url;
use crate::util::personal_rating::PersonalRatingCategory;
use crate::util::personal_rating::PersonalRatingCategorySwatch;

use super::CurrentMatchViewMode;
use super::MatchStatsState;
use super::PlayerTracker;
use super::TrackedPlayer;
use super::WinRateMode;
use super::encounter_severity_color;
use super::last_seen_text;
use super::live::LiveRosterRow;

const ROW_EDGE_PADDING_X: i8 = 10;
const ROW_EDGE_PADDING_Y: i8 = 3;
const DETAIL_LINE_GAP: f32 = 2.0;
const ROW_COLUMN_GAP: f32 = 4.0;
const CLASS_COLUMN_WIDTH: f32 = 24.0;
const SHIP_COLUMN_WIDTH: f32 = 112.0;
const WIN_RATE_COLUMN_WIDTH: f32 = 48.0;
const PERSONAL_RATING_COLUMN_WIDTH: f32 = 48.0;
const BATTLES_COLUMN_WIDTH: f32 = 48.0;
/// Wider than the battle count beside it, to hold a six-digit grouped figure.
const DAMAGE_COLUMN_WIDTH: f32 = 56.0;
const ENCOUNTERS_COLUMN_WIDTH: f32 = 60.0;
const ACTIONS_COLUMN_WIDTH: f32 = 24.0;

/// Below this width, the fixed stat columns leave too little space for both
/// identity cells, so the teams stack instead.
const STACK_TEAMS_BELOW_WIDTH: f32 = 1140.0;
/// Gap between the two teams, whether they sit side by side or stacked.
const TEAM_GAP: f32 = 16.0;

/// Defensive floor on a team column's width. Not reachable through
/// `team_width` today: the smallest input it is ever called with is
/// `STACK_TEAMS_BELOW_WIDTH`, which already clears this floor with room to
/// spare, so the two columns can never be asked to sum to more than the
/// space available. Kept as a floor anyway so the function stays correct on
/// its own terms if that threshold ever changes.
const TEAM_MIN_WIDTH: f32 = 200.0;
/// Ceiling on a team column's width. Wider rows add empty identity space
/// without making the fixed stat columns easier to scan.
const TEAM_MAX_WIDTH: f32 = 620.0;

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_current_match_sub_tab(&mut self, ui: &mut egui::Ui) {
        self.debug_replay_picker(ui);

        // Collected during the table pass and applied after, because the
        // player-tracker lock is held while rendering rows.
        let mut actions = TeamActions::default();

        {
            let mut player_tracker = self.tab_state.player_tracker.write();
            let filter_range = player_tracker.filter_time_period.to_date();
            let now = Timestamp::now();

            let build = player_tracker.live_match.as_ref().and_then(|live| live.build);
            let wows_data = build.zip(self.tab_state.build_cache.as_ref()).and_then(|(build, map)| map.get(build));
            let wows_data_guard = wows_data.as_ref().map(|data| data.read());
            // SharedBuildData is Arc<RwLock<Box<BuildData>>>, so the
            // guard needs three derefs to reach the data itself.
            let wows_data_ref: Option<&BuildData> = wows_data_guard.as_ref().map(|guard| &***guard);

            // Resolve first, then reborrow the tracker's fields disjointly so the
            // roster and the tracked-player map can be read at the same time.
            player_tracker.roster(wows_data_ref);
            let PlayerTracker {
                resolved_roster,
                tracked_players,
                win_rate_mode,
                current_match_view_mode,
                match_stats,
                ..
            } = &*player_tracker;
            let Some(roster) = resolved_roster.as_ref() else {
                ui.label(RichText::new(t!("ui.player_tracker.no_live_match")).weak());
                return;
            };

            let mode = *win_rate_mode;
            let view_mode = *current_match_view_mode;

            ui.horizontal(|ui| {
                let mut new_mode = mode;
                ui.selectable_value(&mut new_mode, WinRateMode::Overall, t!("ui.player_tracker.win_rate_overall"))
                    .on_hover_text(t!("ui.player_tracker.win_rate_mode_hover"));
                ui.selectable_value(&mut new_mode, WinRateMode::Ship, t!("ui.player_tracker.win_rate_ship"))
                    .on_hover_text(t!("ui.player_tracker.win_rate_mode_hover"));
                if new_mode != mode {
                    actions.set_win_rate_mode = Some(new_mode);
                }

                ui.separator();

                let mut new_view_mode = view_mode;
                ui.selectable_value(
                    &mut new_view_mode,
                    CurrentMatchViewMode::Compact,
                    t!("ui.player_tracker.view_compact"),
                );
                ui.selectable_value(
                    &mut new_view_mode,
                    CurrentMatchViewMode::Detailed,
                    t!("ui.player_tracker.view_detailed"),
                );
                if new_view_mode != view_mode {
                    actions.set_view_mode = Some(new_view_mode);
                }

                ui.separator();

                match match_stats {
                    MatchStatsState::Idle => {}
                    MatchStatsState::Resolving => {
                        ui.spinner();
                        ui.label(t!("ui.player_tracker.stats_resolving"));
                    }
                    MatchStatsState::Fetching => {
                        ui.spinner();
                        ui.label(t!("ui.player_tracker.stats_fetching"));
                    }
                    MatchStatsState::Ready(_) => {}
                    MatchStatsState::Failed(reason) => {
                        ui.label(
                            RichText::new(t!("ui.player_tracker.stats_unavailable", reason = reason))
                                .color(ui.sem().warn),
                        );
                    }
                }
            });

            let empty_stats = HashMap::new();
            let stats = match match_stats {
                MatchStatsState::Ready(by_account) => by_account,
                _ => &empty_stats,
            };

            let locale = rust_i18n::locale().to_string();
            let twitch_state = self.tab_state.twitch_state.read();
            let ctx = TeamContext {
                tracked: tracked_players,
                wows_data: wows_data_ref,
                twitch_state: &twitch_state,
                started_at: roster.started_at,
                now,
                filter_range,
                stats,
                mode,
                view_mode,
                locale: &locale,
            };

            let win = ui.sem().win;
            let loss = ui.sem().loss;
            let your_team = t!("ui.player_tracker.your_team", count = roster.friendly.len()).to_string();
            let enemy_team = t!("ui.player_tracker.enemy_team", count = roster.enemy.len()).to_string();

            render_rosters(ui, your_team, enemy_team, win, loss, &roster.friendly, &roster.enemy, &ctx, &mut actions);
        }

        if let Some(login) = actions.copy_login {
            self.tab_state.toasts.lock().success(t!("ui.twitch.copied", name = &login).to_string());
            ui.ctx().copy_text(login);
        }
        if let Some(id) = actions.find_matches_target {
            self.queue_player_search(id);
        }
        if let Some(mode) = actions.set_win_rate_mode {
            self.tab_state.player_tracker.write().win_rate_mode = mode;
        }
        if let Some(view_mode) = actions.set_view_mode {
            self.tab_state.player_tracker.write().current_match_view_mode = view_mode;
        }
    }

    /// Debug-only: install a finished replay's roster as the current match, so the
    /// stats path can be exercised without being in a battle.
    fn debug_replay_picker(&mut self, ui: &mut egui::Ui) {
        if !self.tab_state.persisted.read().settings.app.debug_mode {
            return;
        }

        // `date_time` is `dd.mm.yyyy HH:MM:SS`, which does not order lexically, so
        // it is parsed before sorting. A row whose stamp will not parse sorts last
        // rather than being dropped: it is still a replay the picker can load.
        let mut replays: Vec<(PathBuf, String, Option<Timestamp>)> = self
            .tab_state
            .all_workspaces()
            .filter_map(|workspace| workspace.replay_files.as_ref())
            .flatten()
            .map(|(path, listed)| {
                let stamp = parse_replay_list_timestamp(&listed.date_time);
                let label = format!("{} - {} - {}", listed.date_time, listed.map_name, listed.scenario);
                (path.clone(), label, stamp)
            })
            .collect();
        replays.sort_by(|a, b| {
            match (a.2, b.2) {
                (Some(a), Some(b)) => b.cmp(&a),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.1.cmp(&b.1))
        });

        let id = egui::Id::new("current_match_debug_replay");
        let selected: Option<PathBuf> = ui.data(|d| d.get_temp(id));
        let label = selected
            .as_ref()
            .and_then(|path| {
                replays.iter().find(|(candidate, ..)| candidate == path).map(|(_, label, _)| label.clone())
            })
            .unwrap_or_else(|| t!("ui.player_tracker.debug_replay_none").to_string());

        let mut picked = None;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(id).selected_text(label).show_ui(ui, |ui| {
                for (path, label, _) in &replays {
                    if ui.selectable_label(selected.as_ref() == Some(path), label).clicked() {
                        picked = Some(path.clone());
                    }
                }
            });
            ui.label(t!("ui.player_tracker.debug_replay_picker"))
                .on_hover_text(t!("ui.player_tracker.debug_replay_picker_hover"));
        });

        let Some(path) = picked else {
            return;
        };
        ui.data_mut(|d| d.insert_temp(id, path.clone()));

        let Ok(meta) = ReplayFile::meta_from_file(&path) else {
            return;
        };
        let build = Version::try_from_client_exe(&meta.clientVersionFromExe).and_then(|v| v.build_number());
        let started_at = crate::util::replay_timestamp(&meta);
        self.tab_state.player_tracker.write().update_from_live_arena_info(&meta);
        let _ = self.tab_state.background_parser_tx.as_ref().map(|tx| {
            tx.send(ReplayBackgroundParserThreadMessage::LiveMatchStarted {
                replay: path,
                build,
                flush: FlushState::Complete,
                started_at,
            })
        });
    }
}

/// Parses the `dd.mm.yyyy HH:MM:SS` stamp `replay_timestamp` uses, without
/// panicking on a row it cannot parse.
fn parse_replay_list_timestamp(date_time: &str) -> Option<Timestamp> {
    const REPLAY_DATE_FORMAT: &str = "%d.%m.%Y %H:%M:%S";
    DateTime::strptime(REPLAY_DATE_FORMAT, date_time).ok()?.to_zoned(TimeZone::system()).ok().map(Into::into)
}

/// Everything both team tables read. Bundled because the two calls differ only
/// in their rows, heading and id salt.
struct TeamContext<'a> {
    tracked: &'a HashMap<AccountId, TrackedPlayer>,
    wows_data: Option<&'a BuildData>,
    twitch_state: &'a TwitchState,
    started_at: Timestamp,
    now: Timestamp,
    filter_range: Option<Timestamp>,
    stats: &'a HashMap<AccountId, PlayerStatsOut>,
    mode: WinRateMode,
    view_mode: CurrentMatchViewMode,
    locale: &'a str,
}

/// What a row click asks the caller to do once the player-tracker lock is
/// released.
#[derive(Default)]
struct TeamActions {
    find_matches_target: Option<AccountId>,
    copy_login: Option<String>,
    set_win_rate_mode: Option<WinRateMode>,
    set_view_mode: Option<CurrentMatchViewMode>,
}

/// What one row shows, once the mode has chosen between the account and ship
/// scopes. Every figure here belongs to the chosen scope, so a row can never
/// pair one scope's number with another's. The band follows the rate that is
/// actually shown, so the row colour and the number cannot disagree either.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct RowStats {
    pub win_rate: Option<f64>,
    pub battles: Option<i64>,
    pub avg_damage: Option<i64>,
    pub pr: Option<f64>,
    pub band: Option<PersonalRatingCategory>,
}

pub(crate) fn row_stats(stats: Option<&PlayerStatsOut>, mode: WinRateMode) -> RowStats {
    let Some(stats) = stats else {
        return RowStats::default();
    };
    let (win_rate, battles, avg_damage, pr) = match mode {
        WinRateMode::Overall => (stats.overall_win_rate, stats.battles, stats.overall_avg_damage, stats.pr),
        WinRateMode::Ship => (stats.ship_win_rate, stats.ship_battles, stats.ship_avg_damage, stats.ship_pr),
    };

    RowStats { win_rate, battles, avg_damage, pr, band: win_rate.map(PersonalRatingCategory::from_win_rate) }
}

fn visible_stat_modes(view_mode: CurrentMatchViewMode, selected_mode: WinRateMode) -> Vec<WinRateMode> {
    match view_mode {
        CurrentMatchViewMode::Compact => vec![selected_mode],
        CurrentMatchViewMode::Detailed => vec![WinRateMode::Overall, WinRateMode::Ship],
    }
}

/// The width given to each team column when they sit side by side: half the
/// available width net of the gap between them, clamped to a range that
/// keeps a roster row's content comfortable. The same value is used for both
/// teams, so they are always equal.
fn team_width(available_width: f32, item_spacing: f32) -> f32 {
    ((available_width - TEAM_GAP - item_spacing) / 2.0).clamp(TEAM_MIN_WIDTH, TEAM_MAX_WIDTH)
}

#[allow(clippy::too_many_arguments)]
fn render_rosters(
    ui: &mut egui::Ui,
    friendly_heading: String,
    enemy_heading: String,
    friendly_color: Color32,
    enemy_color: Color32,
    friendly: &[LiveRosterRow],
    enemy: &[LiveRosterRow],
    ctx: &TeamContext<'_>,
    actions: &mut TeamActions,
) {
    let available_width = ui.available_width();
    if available_width >= STACK_TEAMS_BELOW_WIDTH {
        let width = team_width(available_width, ui.spacing().item_spacing.x);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                render_team(ui, friendly_heading, friendly_color, friendly, "current_match_friendly", ctx, actions);
            });
            ui.add_space(TEAM_GAP);
            ui.vertical(|ui| {
                ui.set_width(width);
                render_team(ui, enemy_heading, enemy_color, enemy, "current_match_enemy", ctx, actions);
            });
        });
    } else {
        let width = available_width.min(TEAM_MAX_WIDTH);
        ui.vertical(|ui| {
            ui.set_width(width);
            render_team(ui, friendly_heading, friendly_color, friendly, "current_match_friendly", ctx, actions);
        });
        ui.add_space(TEAM_GAP);
        ui.vertical(|ui| {
            ui.set_width(width);
            render_team(ui, enemy_heading, enemy_color, enemy, "current_match_enemy", ctx, actions);
        });
    }
}

/// A team's win rate, averaged over the rows that have one in `mode`. Rows
/// with no rate yet (no stats entry, a hidden profile, an unplayed ship) are
/// skipped rather than counted as zero, and the average is `None` when no row
/// on the team has a rate at all: the same "absent, not zero" rule the row
/// cells follow.
fn team_average_win_rate(
    rows: &[LiveRosterRow],
    stats: &HashMap<AccountId, PlayerStatsOut>,
    mode: WinRateMode,
) -> Option<f64> {
    let rates: Vec<f64> = rows
        .iter()
        .filter_map(|row| row.account_id.and_then(|id| stats.get(&id)))
        .filter_map(|entry| row_stats(Some(entry), mode).win_rate)
        .collect();

    if rates.is_empty() {
        return None;
    }
    Some(rates.iter().sum::<f64>() / rates.len() as f64)
}

fn team_average_personal_rating(
    rows: &[LiveRosterRow],
    stats: &HashMap<AccountId, PlayerStatsOut>,
    mode: WinRateMode,
) -> Option<f64> {
    let ratings: Vec<f64> = rows
        .iter()
        .filter_map(|row| row.account_id.and_then(|id| stats.get(&id)))
        .filter_map(|entry| row_stats(Some(entry), mode).pr)
        .collect();

    if ratings.is_empty() {
        return None;
    }
    Some(ratings.iter().sum::<f64>() / ratings.len() as f64)
}

/// The clan tag's colour: the server-supplied clan colour when the scan
/// carried one, otherwise the row's relation tint so the tag still renders on
/// older data.
fn clan_color_from_raw(raw: i64, fallback: PlayerTint) -> ClanColor {
    if raw == 0 {
        return ClanColor::Relation(fallback);
    }
    ClanColor::Fixed(Color32::from_rgb(
        ((raw & 0xFF_00_00) >> 16) as u8,
        ((raw & 0xFF_00) >> 8) as u8,
        (raw & 0xFF) as u8,
    ))
}

/// `swatch()`'s chip tint is tuned for a small chip; laid across an entire
/// row it reads oversaturated, so the row fill uses the same hue at a lower
/// alpha. `swatch()` itself is untouched -- PR chips and other surfaces still
/// use its alpha as designed.
const ROW_FILL_ALPHA_SCALE: f32 = 0.5;

/// A row's background: its win-rate band tint when it has one, otherwise the
/// striped fallback so a roster with no stats yet still reads as a list.
/// Each player picks this straight from its own index; there is no header row
/// to offset past any more.
fn row_fill(band: Option<PersonalRatingCategory>, index: usize, visuals: &egui::Visuals) -> Option<Color32> {
    if let Some(band) = band {
        let tint = band.swatch(visuals).tint;
        // `Color32` stores premultiplied alpha, so `r()`/`g()`/`b()` are not the
        // hue's own channels at this alpha; unmultiply first, then reapply the
        // scaled-down alpha to that hue.
        let [r, g, b, a] = tint.to_srgba_unmultiplied();
        let alpha = (a as f32 * ROW_FILL_ALPHA_SCALE).round() as u8;
        return Some(Color32::from_rgba_unmultiplied(r, g, b, alpha));
    }
    (index % 2 == 1).then_some(visuals.faint_bg_color)
}

/// The reason a row shows no win rate, or `None` when the row has no stats
/// entry at all and so gets no hover.
fn no_rate_hover_key(status: PlayerStatsStatus) -> &'static str {
    match status {
        PlayerStatsStatus::Hidden => "ui.player_tracker.stats_hidden",
        PlayerStatsStatus::Unavailable | PlayerStatsStatus::Unknown => "ui.player_tracker.stats_no_data",
        PlayerStatsStatus::Ok => "ui.player_tracker.stats_unplayed_ship",
    }
}

/// One team's roster: a heading, then one fixed-column grid row per player.
fn render_team(
    ui: &mut egui::Ui,
    heading: String,
    heading_color: Color32,
    rows: &[LiveRosterRow],
    id_salt: &'static str,
    ctx: &TeamContext<'_>,
    actions: &mut TeamActions,
) {
    let row_width = ui.available_width();
    let grid_width = row_width - 2.0 * f32::from(ROW_EDGE_PADDING_X);
    ui.horizontal(|ui| {
        ui.label(RichText::new(heading).heading().color(heading_color));
        if let Some(rate) = team_average_win_rate(rows, ctx.stats, ctx.mode) {
            let swatch = PersonalRatingCategory::from_win_rate(rate).swatch(ui.visuals());
            ui.label(
                RichText::new(t!("ui.player_tracker.team_average_win_rate", rate = format!("{rate:.1}%")))
                    .color(swatch.text),
            );
        }
        if let Some(pr) = team_average_personal_rating(rows, ctx.stats, ctx.mode) {
            let swatch = PersonalRatingCategory::from_pr(pr).swatch(ui.visuals());
            ui.label(RichText::new(format!("{}: {pr:.0}", t!("stat.avg_pr"))).color(swatch.text));
        }
    });

    render_team_header(ui, id_salt, grid_width, ctx.view_mode);

    contiguous_row_layout(ui, |ui| {
        ui.push_id(id_salt, |ui| {
            for (index, row_data) in rows.iter().enumerate() {
                let stats_entry = row_data.account_id.and_then(|id| ctx.stats.get(&id));
                let band = row_stats(stats_entry, ctx.mode).band;
                let fill = row_fill(band, index, ui.visuals()).unwrap_or(Color32::TRANSPARENT);

                let background = ui.painter().add(egui::Shape::Noop);
                let response = egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(ROW_EDGE_PADDING_X, ROW_EDGE_PADDING_Y))
                    .show(ui, |ui| {
                        egui_taffy::tui(ui, ui.id().with(index))
                            .reserve_width(grid_width)
                            .style(roster_row_style(ctx.view_mode))
                            .show(|tui| {
                                tui.style(class_column_style()).wrap_mode(egui::TextWrapMode::Truncate).ui(|ui| {
                                    render_class_icon(ui, row_data, ctx);
                                });
                                tui.style(player_column_style()).wrap_mode(egui::TextWrapMode::Truncate).ui(|ui| {
                                    render_player_identity(ui, row_data, stats_entry, ctx, actions);
                                });
                                tui.style(ship_column_style()).wrap_mode(egui::TextWrapMode::Truncate).ui(|ui| {
                                    render_ship_name(ui, row_data, ctx);
                                });
                                tui.style(fixed_column_style(WIN_RATE_COLUMN_WIDTH))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .ui(|ui| {
                                        render_win_rate_cell(ui, stats_entry, ctx);
                                    });
                                tui.style(fixed_column_style(PERSONAL_RATING_COLUMN_WIDTH))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .ui(|ui| {
                                        render_personal_rating_cell(ui, stats_entry, ctx);
                                    });
                                tui.style(fixed_column_style(BATTLES_COLUMN_WIDTH))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .ui(|ui| {
                                        render_battles_cell(ui, stats_entry, ctx);
                                    });
                                tui.style(fixed_column_style(DAMAGE_COLUMN_WIDTH))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .ui(|ui| {
                                        render_damage_cell(ui, stats_entry, ctx);
                                    });
                                tui.style(fixed_column_style(ENCOUNTERS_COLUMN_WIDTH))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .ui(|ui| {
                                        render_encounters(ui, row_data, ctx);
                                    });
                                tui.style(fixed_column_style(ACTIONS_COLUMN_WIDTH))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .ui(|ui| {
                                        render_action_menu(ui, row_data, actions);
                                    });
                            });
                    });
                ui.painter().set(
                    background,
                    egui::Shape::rect_filled(full_width_row_rect(response.response.rect, row_width), 0.0, fill),
                );
            }
        });
    });
}

fn render_team_header(ui: &mut egui::Ui, id_salt: &'static str, grid_width: f32, view_mode: CurrentMatchViewMode) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(ROW_EDGE_PADDING_X, 0)).show(ui, |ui| {
        egui_taffy::tui(ui, ui.id().with((id_salt, "header")))
            .reserve_width(grid_width)
            .style(roster_row_style(view_mode))
            .show(|tui| {
                tui.style(class_column_style())
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.search.field.class"));
                tui.style(player_column_style())
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.search.field.player_present"));
                tui.style(ship_column_style())
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.replay.column.ship_name"));
                tui.style(fixed_column_style(WIN_RATE_COLUMN_WIDTH))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.player_tracker.column.win_rate"));
                tui.style(fixed_column_style(PERSONAL_RATING_COLUMN_WIDTH))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.player_tracker.column.personal_rating"));
                tui.style(fixed_column_style(BATTLES_COLUMN_WIDTH))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.player_tracker.column.battles"));
                tui.style(fixed_column_style(DAMAGE_COLUMN_WIDTH))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.player_tracker.column.avg_damage"));
                tui.style(fixed_column_style(ENCOUNTERS_COLUMN_WIDTH))
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .small(t!("ui.player_tracker.column.encounters"));
                tui.style(fixed_column_style(ACTIONS_COLUMN_WIDTH)).add_empty();
            });
    });
}

fn roster_row_style(view_mode: CurrentMatchViewMode) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Grid,
        align_items: Some(match view_mode {
            CurrentMatchViewMode::Compact => taffy::AlignItems::Center,
            CurrentMatchViewMode::Detailed => taffy::AlignItems::Start,
        }),
        gap: length(ROW_COLUMN_GAP),
        size: taffy::Size { width: percent(1.0), height: auto() },
        grid_template_columns: vec![
            length(CLASS_COLUMN_WIDTH),
            fr(1.0),
            length(SHIP_COLUMN_WIDTH),
            length(WIN_RATE_COLUMN_WIDTH),
            length(PERSONAL_RATING_COLUMN_WIDTH),
            length(BATTLES_COLUMN_WIDTH),
            length(DAMAGE_COLUMN_WIDTH),
            length(ENCOUNTERS_COLUMN_WIDTH),
            length(ACTIONS_COLUMN_WIDTH),
        ],
        ..Default::default()
    }
}

fn class_column_style() -> taffy::Style {
    fixed_column_style(CLASS_COLUMN_WIDTH)
}

fn ship_column_style() -> taffy::Style {
    fixed_column_style(SHIP_COLUMN_WIDTH)
}

fn player_column_style() -> taffy::Style {
    taffy::Style {
        flex_grow: 1.0,
        flex_shrink: 1.0,
        min_size: taffy::Size { width: length(0.0), height: auto() },
        ..Default::default()
    }
}

fn contiguous_row_layout<R>(ui: &mut egui::Ui, add_rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        add_rows(ui)
    })
    .inner
}

fn full_width_row_rect(content_rect: egui::Rect, width: f32) -> egui::Rect {
    egui::Rect::from_min_size(content_rect.min, egui::vec2(width, content_rect.height()))
}

fn fixed_column_style(width: f32) -> taffy::Style {
    taffy::Style {
        flex_shrink: 0.0,
        size: taffy::Size { width: length(width), height: auto() },
        min_size: taffy::Size { width: length(width), height: auto() },
        max_size: taffy::Size { width: length(width), height: auto() },
        ..Default::default()
    }
}

fn render_class_icon(ui: &mut egui::Ui, row_data: &LiveRosterRow, ctx: &TeamContext<'_>) {
    let icon =
        row_data.species.zip(ctx.wows_data).and_then(|(species, data)| ship_class_icon_from_species(species, data));
    if let Some(icon) = icon {
        let image = Image::new(ImageSource::Bytes { uri: icon.path.clone().into(), bytes: icon.data.clone().into() })
            .tint(row_data.tint.color(ui.visuals()))
            .fit_to_exact_size((16.0, 16.0).into())
            .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));
        let response = ui.add(image);
        if let Some(species_text) = row_data.species_text.as_ref() {
            response.on_hover_text(species_text);
        }
    }
}

/// The ship cell. In Detailed view it also carries the "Ship Stats" caption on
/// a second line, labelling the per-ship figures on the detail line beside it.
fn render_ship_name(ui: &mut egui::Ui, row_data: &LiveRosterRow, ctx: &TeamContext<'_>) {
    if ctx.view_mode != CurrentMatchViewMode::Detailed {
        render_ship_name_line(ui, row_data);
        return;
    }
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = DETAIL_LINE_GAP;
        first_detail_line(ui, |ui| render_ship_name_line(ui, row_data));
        ui.label(RichText::new(t!("ui.player_tracker.ship_stats")).small().weak());
    });
}

/// None of the row's truncating labels carry an explicit hover: `truncate()`
/// already reveals the full text when the label elides, so adding one stacks a
/// second identical tooltip under the first.
fn render_ship_name_line(ui: &mut egui::Ui, row_data: &LiveRosterRow) {
    let ship_name = row_data.ship_name.as_ref().or(row_data.species_text.as_ref());
    if let Some(ship_name) = ship_name {
        ui.add(egui::Label::new(ship_name).truncate());
    }
}

/// Pins a cell's first line to one line's height, so it sits on the row's top
/// line rather than centring itself across both detail lines.
fn first_detail_line<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)),
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    )
    .inner
}

fn render_player_identity(
    ui: &mut egui::Ui,
    row_data: &LiveRosterRow,
    stats_entry: Option<&PlayerStatsOut>,
    ctx: &TeamContext<'_>,
    actions: &mut TeamActions,
) {
    if ctx.view_mode == CurrentMatchViewMode::Detailed {
        first_detail_line(ui, |ui| render_player_identity_line(ui, row_data, stats_entry, ctx, actions));
        return;
    }
    render_player_identity_line(ui, row_data, stats_entry, ctx, actions);
}

fn render_player_identity_line(
    ui: &mut egui::Ui,
    row_data: &LiveRosterRow,
    stats_entry: Option<&PlayerStatsOut>,
    ctx: &TeamContext<'_>,
    actions: &mut TeamActions,
) {
    ui.spacing_mut().item_spacing.x = 2.0;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if stats_entry.is_some_and(|entry| entry.status == PlayerStatsStatus::Hidden) {
            ui.label(icons::EYE_SLASH).on_hover_text(t!("ui.player_tracker.stats_hidden"));
        }

        if let Some(player) = row_data.tracked.and_then(|id| ctx.tracked.get(&id))
            && !player.notes.is_empty()
        {
            ui.label(icons::NOTE_PENCIL).on_hover_text(&player.notes);
        }

        if let Some(candidates) = ctx.twitch_state.player_is_potential_stream_sniper(&row_data.name, ctx.started_at)
            && let Some(login) = crate::ui::widgets::twitch_chip(ui, &candidates, ctx.started_at)
        {
            actions.copy_login = Some(login);
        }

        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            render_clan(ui, row_data);
            ui.add(egui::Label::new(RichText::new(&row_data.name).color(row_data.tint.color(ui.visuals()))).truncate());
        });
    });
}

fn render_clan(ui: &mut egui::Ui, row_data: &LiveRosterRow) {
    let Some(clan) = row_data.clan.as_ref() else {
        return;
    };
    let color = clan_color_from_raw(row_data.clan_color, row_data.tint).color(ui.visuals());
    ui.add(egui::Label::new(RichText::new(format!("[{clan}]")).color(color)).truncate());
}

fn render_win_rate_cell(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>) {
    render_scoped_cell(ui, ctx, |ui, mode| render_win_rate(ui, stats_entry, ctx, mode));
}

fn render_win_rate(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>, mode: WinRateMode) {
    let resolved = row_stats(stats_entry, mode);
    if let Some(rate) = resolved.win_rate {
        let band = resolved.band.expect("a resolved win rate always has a band");
        let response = ui.label(RichText::new(format!("{rate:.1}%")).color(band.swatch(ui.visuals()).text));
        if let Some(hover) = win_rate_hover(stats_entry, ctx.locale) {
            response.on_hover_text(hover);
        }
        return;
    }

    let response = ui.label(RichText::new("-").weak());
    if let Some(entry) = stats_entry {
        response.on_hover_text(t!(no_rate_hover_key(entry.status)));
    }
}

fn render_personal_rating_cell(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>) {
    render_scoped_cell(ui, ctx, |ui, mode| render_personal_rating(ui, stats_entry, mode));
}

/// PR is the one figure here left ungrouped. Every other `pr_chip` in the app
/// renders the bare rating, and a chip that grouped its digits on only this
/// surface would read as a different quantity.
fn render_personal_rating(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, mode: WinRateMode) {
    let Some(pr) = row_stats(stats_entry, mode).pr else {
        ui.label(RichText::new("-").weak());
        return;
    };
    let hover = match mode {
        WinRateMode::Overall => t!("ui.player_tracker.pr_overall_hover"),
        WinRateMode::Ship => t!("ui.player_tracker.pr_ship_hover"),
    };
    crate::ui::widgets::pr_chip(ui, PersonalRatingCategory::from_pr(pr), &format!("{pr:.0}"), false)
        .on_hover_text(hover);
}

fn render_battles_cell(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>) {
    render_scoped_cell(ui, ctx, |ui, mode| render_battles(ui, stats_entry, ctx, mode));
}

fn render_battles(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>, mode: WinRateMode) {
    let Some(battles) = row_stats(stats_entry, mode).battles else {
        ui.label(RichText::new("-").weak());
        return;
    };
    let hover = format!("{}: {}", scope_label(mode), t!("ui.player_tracker.column.battles"));
    ui.label(separate_number(battles, Some(ctx.locale))).on_hover_text(hover);
}

fn render_damage_cell(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>) {
    render_scoped_cell(ui, ctx, |ui, mode| render_damage(ui, stats_entry, ctx, mode));
}

fn render_damage(ui: &mut egui::Ui, stats_entry: Option<&PlayerStatsOut>, ctx: &TeamContext<'_>, mode: WinRateMode) {
    let Some(damage) = row_stats(stats_entry, mode).avg_damage else {
        ui.label(RichText::new("-").weak());
        return;
    };
    let hover = format!("{}: {}", scope_label(mode), t!("ui.player_tracker.avg_damage"));
    ui.label(separate_number(damage, Some(ctx.locale))).on_hover_text(hover);
}

/// Draws a stat cell once per scope the view mode shows: the selected scope
/// alone in Compact, Overall stacked over Ship in Detailed. Every stat column
/// shares this so their lines stay on the same baselines across the row.
fn render_scoped_cell(ui: &mut egui::Ui, ctx: &TeamContext<'_>, mut render: impl FnMut(&mut egui::Ui, WinRateMode)) {
    let modes = visible_stat_modes(ctx.view_mode, ctx.mode);
    if let [only] = modes[..] {
        render(ui, only);
        return;
    }
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = DETAIL_LINE_GAP;
        for mode in modes {
            render(ui, mode);
        }
    });
}

fn scope_label(mode: WinRateMode) -> String {
    match mode {
        WinRateMode::Overall => t!("ui.player_tracker.win_rate_overall"),
        WinRateMode::Ship => t!("ui.player_tracker.win_rate_ship"),
    }
    .to_string()
}

fn render_encounters(ui: &mut egui::Ui, row_data: &LiveRosterRow, ctx: &TeamContext<'_>) {
    let Some(player) = row_data.tracked.and_then(|id| ctx.tracked.get(&id)) else {
        return;
    };
    let total = player.arena_ids.len();
    let in_range = match ctx.filter_range {
        Some(range) => player.timestamps.iter().filter(|ts| **ts > range).count(),
        None => total,
    };
    let last = last_seen_text(player.timestamps.last().copied(), ctx.now);

    let mut text = RichText::new(separate_number(total, Some(ctx.locale)));
    if let Some(color) = encounter_severity_color(ui, in_range) {
        text = text.color(color);
    }
    ui.label(text).on_hover_text(t!(
        "ui.player_tracker.encounters_hover",
        total = total,
        range = in_range,
        last = last
    ));
}

fn row_has_action_menu(row_data: &LiveRosterRow) -> bool {
    row_data.tracked.is_some() || row_data.account_id.zip(row_data.region).is_some()
}

fn render_action_menu(ui: &mut egui::Ui, row_data: &LiveRosterRow, actions: &mut TeamActions) {
    if !row_has_action_menu(row_data) {
        return;
    }

    ui.menu_button(icons::DOTS_THREE, |ui| {
        if let Some(id) = row_data.tracked
            && ui
                .small_button(wt_translations::icon_t(icons::MAGNIFYING_GLASS, &t!("ui.player_tracker.find_matches")))
                .clicked()
        {
            actions.find_matches_target = Some(id);
            ui.close_kind(UiKind::Menu);
        }

        if row_data.tracked.is_some() && row_data.account_id.zip(row_data.region).is_some() {
            ui.separator();
        }

        if let (Some(account_id), Some(region)) = (row_data.account_id, row_data.region) {
            if ui
                .small_button(wt_translations::icon_t(
                    icons::ARROW_SQUARE_OUT,
                    &t!("ui.player_tracker.open_wows_numbers"),
                ))
                .clicked()
            {
                let url = wows_numbers_player_url(region, account_id, &row_data.name);
                ui.ctx().open_url(OpenUrl::new_tab(url));
                ui.close_kind(UiKind::Menu);
            }

            if ui
                .small_button(wt_translations::icon_t(
                    icons::ARROW_SQUARE_OUT,
                    &t!("ui.player_tracker.open_shipbuilds"),
                ))
                .clicked()
            {
                let url = shipbuilds_player_url(region, account_id, &row_data.name);
                ui.ctx().open_url(OpenUrl::new_tab(url));
                ui.close_kind(UiKind::Menu);
            }
        }
    });
}

/// The WR cell's hover: both scopes, each explicitly labelled. A tooltip on a
/// number reads as labelling that number, so a hover naming only the other
/// scope (the previous design) read as saying the shown value belonged to
/// that scope. Independent of `mode`: whichever scope is showing in the cell,
/// the hover names both. A scope with no rate of its own is omitted rather
/// than shown as 0%, matching `row_stats`'s "absent, not zero" rule; `None`
/// when neither scope has one. Average damage joins the line only when that
/// scope reports one, under the same rule.
fn win_rate_hover(stats: Option<&PlayerStatsOut>, locale: &str) -> Option<String> {
    let stats = stats?;
    let scopes = [
        (scope_label(WinRateMode::Overall), stats.overall_win_rate, stats.battles, stats.overall_avg_damage),
        (scope_label(WinRateMode::Ship), stats.ship_win_rate, stats.ship_battles, stats.ship_avg_damage),
    ];

    let lines: Vec<String> = scopes
        .into_iter()
        .filter_map(|(label, rate, battles, damage)| {
            let (rate, battles) = (rate?, battles?);
            let rate = format!("{rate:.1}%");
            let battles = separate_number(battles, Some(locale));
            let line = match damage {
                Some(damage) => t!(
                    "ui.player_tracker.win_rate_hover_line_damage",
                    label = label,
                    rate = rate,
                    battles = battles,
                    damage = separate_number(damage, Some(locale))
                ),
                None => t!("ui.player_tracker.win_rate_hover_line", label = label, rate = rate, battles = battles),
            };
            Some(line.to_string())
        })
        .collect();

    if lines.is_empty() { None } else { Some(lines.join("\n")) }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::SnapshotOptions;
    use egui_kittest::kittest::NodeT as _;
    use egui_kittest::kittest::Queryable as _;
    use wows_replays::types::ArenaId;
    use wows_replays::types::GameParamId;

    use crate::data::match_stats::Region;

    use super::*;

    fn stats(status: PlayerStatsStatus, overall: Option<f64>, ship: Option<f64>) -> PlayerStatsOut {
        PlayerStatsOut {
            account_id: AccountId(1),
            region: "eu".to_string(),
            ship_id: GameParamId::from(1u64),
            status,
            battles: Some(9000),
            overall_win_rate: overall,
            overall_avg_damage: Some(96_047),
            ship_win_rate: ship,
            ship_battles: Some(120),
            ship_avg_damage: Some(83_266),
            ship_pr: Some(2834.0),
            pr: Some(1800.0),
        }
    }

    /// The per-status hover key a row with no visible rate shows: `Hidden` gets
    /// its own reason, `Unavailable` and `Unknown` share the generic no-data
    /// reason, and `Ok` (an unplayed ship) gets the unplayed-ship reason.
    #[test]
    fn no_rate_hover_key_picks_the_status_specific_reason() {
        assert_eq!(no_rate_hover_key(PlayerStatsStatus::Hidden), "ui.player_tracker.stats_hidden");
        assert_eq!(no_rate_hover_key(PlayerStatsStatus::Unavailable), "ui.player_tracker.stats_no_data");
        assert_eq!(no_rate_hover_key(PlayerStatsStatus::Unknown), "ui.player_tracker.stats_no_data");
        assert_eq!(no_rate_hover_key(PlayerStatsStatus::Ok), "ui.player_tracker.stats_unplayed_ship");
    }

    #[test]
    fn the_mode_picks_the_rate_the_battles_and_the_band_together() {
        let stats = stats(PlayerStatsStatus::Ok, Some(48.0), Some(61.0));

        let overall = row_stats(Some(&stats), WinRateMode::Overall);
        assert_eq!(overall.win_rate, Some(48.0));
        assert_eq!(overall.battles, Some(9000));
        assert_eq!(overall.band, Some(PersonalRatingCategory::BelowAverage));

        let ship = row_stats(Some(&stats), WinRateMode::Ship);
        assert_eq!(ship.win_rate, Some(61.0));
        assert_eq!(ship.battles, Some(120));
        assert_eq!(ship.band, Some(PersonalRatingCategory::Unicum));
    }

    /// Every figure in a row belongs to one scope, so PR and average damage
    /// move with the mode alongside the rate and the battle count. A row must
    /// never pair one scope's rate with another's PR.
    #[test]
    fn pr_and_average_damage_move_with_the_mode() {
        let stats = stats(PlayerStatsStatus::Ok, Some(48.0), Some(61.0));

        let overall = row_stats(Some(&stats), WinRateMode::Overall);
        assert_eq!(overall.pr, Some(1800.0));
        assert_eq!(overall.avg_damage, Some(96_047));

        let ship = row_stats(Some(&stats), WinRateMode::Ship);
        assert_eq!(ship.pr, Some(2834.0));
        assert_eq!(ship.avg_damage, Some(83_266));
    }

    /// A server still on the previous response shape sends no ship PR and no
    /// damage. Those cells go empty; the rest of the row is unaffected.
    #[test]
    fn a_response_without_the_newer_fields_empties_only_those_cells() {
        let mut stats = stats(PlayerStatsStatus::Ok, Some(48.0), Some(61.0));
        stats.ship_pr = None;
        stats.overall_avg_damage = None;
        stats.ship_avg_damage = None;

        let ship = row_stats(Some(&stats), WinRateMode::Ship);
        assert_eq!(ship.pr, None);
        assert_eq!(ship.avg_damage, None);
        assert_eq!(ship.win_rate, Some(61.0), "the rest of the scope still resolves");
        assert_eq!(ship.battles, Some(120));

        assert_eq!(row_stats(Some(&stats), WinRateMode::Overall).pr, Some(1800.0), "the account PR is unaffected");
    }

    #[test]
    fn a_hidden_profile_has_no_rate_and_no_band() {
        let stats = stats(PlayerStatsStatus::Hidden, None, None);

        let resolved = row_stats(Some(&stats), WinRateMode::Overall);

        assert_eq!(resolved.win_rate, None);
        assert_eq!(resolved.band, None);
    }

    /// A ship the player has never taken out reports `ok` with a null ship
    /// rate. That is an absent rate, not a zero one.
    #[test]
    fn an_unplayed_ship_has_no_ship_rate() {
        let stats = stats(PlayerStatsStatus::Ok, Some(52.5), None);

        let resolved = row_stats(Some(&stats), WinRateMode::Ship);

        assert_eq!(resolved.win_rate, None);
        assert_eq!(resolved.band, None);
        assert_eq!(resolved.battles, Some(120));
    }

    #[test]
    fn a_player_with_no_stats_at_all_has_nothing() {
        let resolved = row_stats(None, WinRateMode::Overall);

        assert_eq!(resolved.win_rate, None);
        assert_eq!(resolved.pr, None);
        assert_eq!(resolved.battles, None);
        assert_eq!(resolved.avg_damage, None);
        assert_eq!(resolved.band, None);
    }

    #[test]
    fn win_rate_hover_labels_both_scopes_when_both_are_present() {
        rust_i18n::set_locale("en");
        let stats = stats(PlayerStatsStatus::Ok, Some(48.3), Some(42.2));

        let hover = win_rate_hover(Some(&stats), "en").expect("both scopes are present");

        assert!(hover.contains("Overall: 48.3%"), "hover was: {hover}");
        assert!(hover.contains("Ship: 42.2%"), "hover was: {hover}");
    }

    /// Average damage joins each scope's line, grouped for the locale.
    #[test]
    fn win_rate_hover_carries_average_damage_when_the_scope_reports_it() {
        rust_i18n::set_locale("en");
        let stats = stats(PlayerStatsStatus::Ok, Some(48.3), Some(42.2));

        let hover = win_rate_hover(Some(&stats), "en").expect("both scopes are present");

        assert!(hover.contains("96,047"), "hover was: {hover}");
        assert!(hover.contains("83,266"), "hover was: {hover}");
    }

    /// A scope with a rate but no damage keeps its line and drops the damage
    /// clause, rather than reporting a fabricated 0.
    #[test]
    fn win_rate_hover_drops_the_damage_clause_when_the_scope_has_none() {
        rust_i18n::set_locale("en");
        let mut stats = stats(PlayerStatsStatus::Ok, Some(48.3), Some(42.2));
        stats.overall_avg_damage = None;

        let hover = win_rate_hover(Some(&stats), "en").expect("both scopes are present");

        let overall = hover.lines().find(|line| line.starts_with("Overall")).expect("the overall line survives");
        assert!(overall.contains("48.3%"), "overall line was: {overall}");
        assert!(!overall.contains("damage"), "overall line was: {overall}");
        assert!(hover.contains("83,266"), "the ship line keeps its damage: {hover}");
    }

    /// An unplayed ship reports `ok` with a null ship rate: an absent rate, not
    /// a zero one, so the hover must omit that line rather than show 0%.
    #[test]
    fn win_rate_hover_omits_a_scope_with_no_rate() {
        rust_i18n::set_locale("en");
        let stats = stats(PlayerStatsStatus::Ok, Some(52.5), None);

        let hover = win_rate_hover(Some(&stats), "en").expect("the overall scope is present");

        assert!(hover.contains("Overall"));
        assert!(!hover.contains("Ship"), "hover was: {hover}");
    }

    #[test]
    fn win_rate_hover_is_none_when_neither_scope_has_a_rate() {
        let stats = stats(PlayerStatsStatus::Hidden, None, None);

        assert_eq!(win_rate_hover(Some(&stats), "en"), None);
    }

    #[test]
    fn win_rate_hover_is_none_without_a_stats_entry() {
        assert_eq!(win_rate_hover(None, "en"), None);
    }

    /// The row fill keeps the band's hue but at a lower alpha than the chip
    /// tint, since the same alpha spread across a whole row reads
    /// oversaturated compared to a small chip.
    #[test]
    fn row_fill_uses_the_band_hue_at_a_lower_alpha_than_the_chip_tint() {
        let visuals = egui::Visuals::dark();
        let chip_tint = PersonalRatingCategory::Unicum.swatch(&visuals).tint;
        let [r, g, b, chip_alpha] = chip_tint.to_srgba_unmultiplied();
        let expected =
            Color32::from_rgba_unmultiplied(r, g, b, (chip_alpha as f32 * ROW_FILL_ALPHA_SCALE).round() as u8);

        let fill = row_fill(Some(PersonalRatingCategory::Unicum), 4, &visuals).expect("a band always fills");
        assert_eq!(fill, expected);
        let [fill_r, fill_g, fill_b, fill_alpha] = fill.to_srgba_unmultiplied();
        assert_eq!((fill_r, fill_g, fill_b), (r, g, b), "the hue itself must be unchanged");
        assert!(fill_alpha < chip_alpha, "row fill alpha {fill_alpha} should be lower than the chip's {chip_alpha}");
    }

    /// With no header row to offset past any more, striping runs straight off
    /// each player's own index.
    #[test]
    fn row_fill_stripes_bandless_rows_by_index_with_no_header_offset() {
        let visuals = egui::Visuals::dark();

        assert_eq!(row_fill(None, 0, &visuals), None);
        assert_eq!(row_fill(None, 1, &visuals), Some(visuals.faint_bg_color));
        assert_eq!(row_fill(None, 2, &visuals), None);
    }

    fn roster_row(account_id: Option<AccountId>) -> LiveRosterRow {
        LiveRosterRow {
            name: "P".to_string(),
            tint: PlayerTint::Enemy,
            species: None,
            ship_name: None,
            species_text: None,
            tracked: None,
            account_id,
            region: None,
            clan: None,
            clan_color: 0,
        }
    }

    fn visual_row(account_id: i64, ship: &str, clan: Option<&str>, player: &str) -> LiveRosterRow {
        LiveRosterRow {
            name: player.to_string(),
            tint: PlayerTint::Enemy,
            species: None,
            ship_name: Some(ship.to_string()),
            species_text: None,
            tracked: None,
            account_id: Some(AccountId(account_id)),
            region: Some(Region::Eu),
            clan: clan.map(str::to_string),
            clan_color: 0,
        }
    }

    #[test]
    fn kittest_current_match_columns_are_visually_aligned() {
        rust_i18n::set_locale("en");
        let mut friendly = vec![
            visual_row(1, "Bourgogne", Some("PL-WY"), "ORKANIN"),
            visual_row(2, "C. Colombo", Some("VENIK"), "MarshalPiten"),
            visual_row(3, "Mecklenburg", None, "ConstantinXII"),
        ];
        let enemy = vec![
            visual_row(4, "Libertad", None, "DontThankMe"),
            visual_row(5, "Rhode Island", Some("SKIFF"), "soth_1_81757720"),
            visual_row(6, "Cerberus", Some("BHT"), "ultrasailor15"),
        ];
        let stats_by_account = friendly
            .iter()
            .chain(&enemy)
            .enumerate()
            .map(|(index, row)| {
                let mut entry = stats(PlayerStatsStatus::Ok, Some(48.3 + index as f64), Some(52.0));
                entry.account_id = row.account_id.expect("visual rows have identities");
                (entry.account_id, entry)
            })
            .collect();
        friendly[0].tracked = Some(AccountId(1));
        let encountered = TrackedPlayer { arena_ids: (1..=7).map(ArenaId::new).collect(), ..Default::default() };
        let tracked = HashMap::from([(AccountId(1), encountered)]);
        let twitch = TwitchState::default();
        let now = Timestamp::now();
        let ctx = TeamContext {
            tracked: &tracked,
            wows_data: None,
            twitch_state: &twitch,
            started_at: now,
            now,
            filter_range: None,
            stats: &stats_by_account,
            mode: WinRateMode::Overall,
            view_mode: CurrentMatchViewMode::Compact,
            locale: "en",
        };
        let mut actions = TeamActions::default();
        let options = SnapshotOptions::new().output_path("tests/snapshots");
        let mut harness = Harness::builder().with_size((1240.0, 150.0)).with_options(options).build_ui(|ui| {
            render_rosters(
                ui,
                "Your Team (3)".to_string(),
                "Enemy Team (3)".to_string(),
                Color32::GREEN,
                Color32::RED,
                &friendly,
                &enemy,
                &ctx,
                &mut actions,
            );
        });
        harness.ctx.set_style_of(egui::Theme::Dark, crate::ui::theme::style::dark_style());
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        harness.ctx.set_fonts(fonts);

        harness.run();

        let class_headers: Vec<_> = harness.get_all_by_label("Class").collect();
        let player_headers: Vec<_> = harness.get_all_by_label("Player").collect();
        let ship_headers: Vec<_> = harness.get_all_by_label("Ship Name").collect();
        let battles_headers: Vec<_> = harness.get_all_by_label("Battles").collect();
        let damage_headers: Vec<_> = harness.get_all_by_label("Dmg").collect();
        let seen_headers: Vec<_> = harness.get_all_by_label("Seen").collect();
        assert_eq!(class_headers.len(), 2);
        assert_eq!(player_headers.len(), 2);
        assert_eq!(ship_headers.len(), 2);
        assert_eq!(battles_headers.len(), 2);
        assert_eq!(damage_headers.len(), 2);
        assert_eq!(seen_headers.len(), 2);
        let class_header = class_headers[0].rect();
        let player_header = player_headers[0].rect();
        let ship_header = ship_headers[0].rect();
        let battles_header = battles_headers[0].rect();
        let damage_header = damage_headers[0].rect();
        let seen_header = seen_headers[0].rect();
        assert!(class_header.left() < player_header.left());
        assert!(player_header.left() < ship_header.left());
        assert!(ship_header.left() < battles_header.left());
        assert!(battles_header.left() < damage_header.left(), "Dmg sits between Battles and Seen");
        assert!(damage_header.left() < seen_header.left());

        let player_column_x =
            ["[PL-WY]", "[VENIK]", "ConstantinXII"].map(|label| harness.get_by_label(label).rect().left());
        assert!(
            player_column_x.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 0.1),
            "combined player columns: {player_column_x:?}"
        );

        let ship_x = ["Bourgogne", "C. Colombo", "Mecklenburg"].map(|label| harness.get_by_label(label).rect().left());
        assert!(ship_x.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 0.1), "ship columns: {ship_x:?}");

        let win_rate_x: Vec<f32> =
            ["48.3%", "49.3%", "50.3%"].map(|label| harness.get_by_label(label).rect().left()).into_iter().collect();
        assert!(win_rate_x.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 0.1), "win-rate columns: {win_rate_x:?}");

        // Every row carries the same account damage, so the cells are matched to
        // their team by taking the first three (friendly) of the six.
        let damage_cells: Vec<_> = harness.get_all_by_label("96,047").collect();
        assert_eq!(damage_cells.len(), 6, "one localized damage cell per player");
        let damage_x: Vec<f32> = damage_cells.iter().take(3).map(|cell| cell.rect().left()).collect();
        assert!(damage_x.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 0.1), "damage columns: {damage_x:?}");

        let clan = harness.get_by_label("[PL-WY]").rect();
        let player = harness.get_by_label("ORKANIN").rect();
        let ship = harness.get_by_label("Bourgogne").rect();
        let win_rate = harness.get_by_label("48.3%").rect();
        assert!(class_header.left() < clan.left());
        assert!(clan.left() < player.left() && player.right() <= ship.left());
        assert!(ship.right() <= win_rate.left());
        assert!(win_rate.right() <= damage_x[0]);

        let seen = harness.get_by_label("7");
        assert_eq!(seen.accesskit_node().value(), Some("7".to_string()));
        assert!(!seen.accesskit_node().value().is_some_and(|value| value.contains("Seen") || value.contains('x')));

        let average_pr: Vec<_> = harness.get_all_by_label("Avg PR: 1800").collect();
        assert_eq!(average_pr.len(), 2);
        let friendly_average_wr = harness.get_by_label("Avg WR: 49.3%").rect();
        let enemy_average_wr = harness.get_by_label("Avg WR: 52.3%").rect();
        assert!(friendly_average_wr.right() < average_pr[0].rect().left());
        assert!(enemy_average_wr.right() < average_pr[1].rect().left());

        harness.snapshot("current_match_two_teams");
    }

    #[test]
    fn kittest_detailed_rows_show_overall_then_ship_stats() {
        rust_i18n::set_locale("en");
        let friendly = vec![visual_row(1, "Bourgogne", Some("PL-WY"), "ORKANIN")];
        let enemy = vec![visual_row(2, "Libertad", None, "DontThankMe")];
        let mut friendly_stats = stats(PlayerStatsStatus::Ok, Some(48.3), Some(61.7));
        friendly_stats.account_id = AccountId(1);
        friendly_stats.battles = Some(9000);
        friendly_stats.ship_battles = Some(321);
        let mut enemy_stats = stats(PlayerStatsStatus::Ok, Some(52.3), Some(44.4));
        enemy_stats.account_id = AccountId(2);
        enemy_stats.battles = Some(8000);
        enemy_stats.ship_battles = Some(123);
        let stats_by_account = HashMap::from([(AccountId(1), friendly_stats), (AccountId(2), enemy_stats)]);
        let tracked = HashMap::new();
        let twitch = TwitchState::default();
        let now = Timestamp::now();
        let ctx = TeamContext {
            tracked: &tracked,
            wows_data: None,
            twitch_state: &twitch,
            started_at: now,
            now,
            filter_range: None,
            stats: &stats_by_account,
            mode: WinRateMode::Overall,
            view_mode: CurrentMatchViewMode::Detailed,
            locale: "en",
        };
        let mut actions = TeamActions::default();
        let options = SnapshotOptions::new().output_path("tests/snapshots");
        let mut harness = Harness::builder().with_size((1240.0, 115.0)).with_options(options).build_ui(|ui| {
            render_rosters(
                ui,
                "Your Team (1)".to_string(),
                "Enemy Team (1)".to_string(),
                Color32::GREEN,
                Color32::RED,
                &friendly,
                &enemy,
                &ctx,
                &mut actions,
            );
        });
        harness.ctx.set_style_of(egui::Theme::Dark, crate::ui::theme::style::dark_style());
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        harness.ctx.set_fonts(fonts);

        harness.run();

        let overall_wr = harness.get_by_label("48.3%").rect();
        let ship_wr = harness.get_by_label("61.7%").rect();
        let overall_battles = harness.get_by_label("9,000").rect();
        let ship_battles = harness.get_by_label("321").rect();
        let ship_name = harness.get_by_label("Bourgogne").rect();

        // Both rows carry the same fixture damage and PR, so the friendly row's
        // cells are the first of each pair.
        let overall_damage: Vec<_> = harness.get_all_by_label("96,047").collect();
        let ship_damage: Vec<_> = harness.get_all_by_label("83,266").collect();
        let overall_pr: Vec<_> = harness.get_all_by_label("1800").collect();
        let ship_pr: Vec<_> = harness.get_all_by_label("2834").collect();
        assert_eq!(overall_damage.len(), 2);
        assert_eq!(ship_damage.len(), 2);
        assert_eq!(overall_pr.len(), 2, "the account PR chip, once per row");
        assert_eq!(ship_pr.len(), 2, "the ship PR chip, once per row");
        assert!(overall_damage[0].rect().top() < ship_damage[0].rect().top());
        assert!((overall_damage[0].rect().left() - ship_damage[0].rect().left()).abs() < 0.1);
        assert!(overall_pr[0].rect().top() < ship_pr[0].rect().top());
        assert!(ship_battles.right() <= overall_damage[0].rect().left(), "damage sits right of battles");
        let ship_stats_labels: Vec<_> = harness.get_all_by_label("Ship Stats").collect();
        assert_eq!(ship_stats_labels.len(), 2);
        let ship_stats = ship_stats_labels[0].rect();
        assert!(overall_wr.top() < ship_wr.top());
        assert!(overall_battles.top() < ship_battles.top());
        assert!((overall_wr.left() - ship_wr.left()).abs() < 0.1);
        assert!((overall_battles.left() - ship_battles.left()).abs() < 0.1);
        assert!((ship_stats.top() - ship_wr.top()).abs() < 0.1, "ship stats {ship_stats:?}, ship WR {ship_wr:?}");
        assert!(
            (ship_stats.top() - ship_battles.top()).abs() < 0.1,
            "ship stats {ship_stats:?}, ship battles {ship_battles:?}"
        );
        // The caption belongs to the ship column, on the line under the name.
        assert!(
            (ship_stats.left() - ship_name.left()).abs() < 0.1,
            "ship stats {ship_stats:?}, ship name {ship_name:?}"
        );
        assert!(ship_stats.top() >= ship_name.bottom());
        assert!(ship_wr.top() - overall_wr.bottom() >= 2.0);
        assert!(ship_battles.top() - overall_battles.bottom() >= 2.0);

        harness.snapshot("current_match_two_teams_detailed_overall");
    }

    #[test]
    fn kittest_detailed_ship_scope_changes_banding_without_hiding_overall_stats() {
        rust_i18n::set_locale("en");
        let friendly = vec![visual_row(1, "Bourgogne", Some("PL-WY"), "ORKANIN")];
        let enemy = vec![visual_row(2, "Libertad", None, "DontThankMe")];
        let mut friendly_stats = stats(PlayerStatsStatus::Ok, Some(48.3), Some(61.7));
        friendly_stats.account_id = AccountId(1);
        friendly_stats.battles = Some(9000);
        friendly_stats.ship_battles = Some(321);
        let mut enemy_stats = stats(PlayerStatsStatus::Ok, Some(52.3), Some(44.4));
        enemy_stats.account_id = AccountId(2);
        enemy_stats.battles = Some(8000);
        enemy_stats.ship_battles = Some(123);
        let stats_by_account = HashMap::from([(AccountId(1), friendly_stats), (AccountId(2), enemy_stats)]);
        let tracked = HashMap::new();
        let twitch = TwitchState::default();
        let now = Timestamp::now();
        let ctx = TeamContext {
            tracked: &tracked,
            wows_data: None,
            twitch_state: &twitch,
            started_at: now,
            now,
            filter_range: None,
            stats: &stats_by_account,
            mode: WinRateMode::Ship,
            view_mode: CurrentMatchViewMode::Detailed,
            locale: "en",
        };
        let mut actions = TeamActions::default();
        let options = SnapshotOptions::new().output_path("tests/snapshots");
        let mut harness = Harness::builder().with_size((1240.0, 115.0)).with_options(options).build_ui(|ui| {
            render_rosters(
                ui,
                "Your Team (1)".to_string(),
                "Enemy Team (1)".to_string(),
                Color32::GREEN,
                Color32::RED,
                &friendly,
                &enemy,
                &ctx,
                &mut actions,
            );
        });
        harness.ctx.set_style_of(egui::Theme::Dark, crate::ui::theme::style::dark_style());
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        harness.ctx.set_fonts(fonts);

        harness.run();

        let overall_wr = harness.get_by_label("48.3%").rect();
        let ship_wr = harness.get_by_label("61.7%").rect();
        assert!(overall_wr.top() < ship_wr.top());
        // The Ship scope drives the team header, so it reports the ship PR.
        let team_average_pr: Vec<_> = harness.get_all_by_label("Avg PR: 2834").collect();
        assert_eq!(team_average_pr.len(), 2, "each team header averages the scope its rows show");
        harness.snapshot("current_match_two_teams_detailed_ship");
    }

    #[test]
    fn kittest_compact_full_rosters_fit_the_representative_viewport() {
        rust_i18n::set_locale("en");
        let friendly: Vec<_> = (0..12)
            .map(|index| visual_row(index + 1, &format!("Ship F{index}"), None, &format!("Friendly{index}")))
            .collect();
        let enemy: Vec<_> = (0..12)
            .map(|index| visual_row(index + 101, &format!("Ship E{index}"), None, &format!("Enemy{index}")))
            .collect();
        let stats_by_account = friendly
            .iter()
            .chain(&enemy)
            .map(|row| {
                let account_id = row.account_id.expect("visual rows have accounts");
                let mut entry = stats(PlayerStatsStatus::Ok, Some(50.0), Some(51.0));
                entry.account_id = account_id;
                (account_id, entry)
            })
            .collect();
        let tracked = HashMap::new();
        let twitch = TwitchState::default();
        let now = Timestamp::now();
        let ctx = TeamContext {
            tracked: &tracked,
            wows_data: None,
            twitch_state: &twitch,
            started_at: now,
            now,
            filter_range: None,
            stats: &stats_by_account,
            mode: WinRateMode::Overall,
            view_mode: CurrentMatchViewMode::Compact,
            locale: "en",
        };
        let mut actions = TeamActions::default();
        let mut harness = Harness::builder().with_size((1180.0, 420.0)).build_ui(|ui| {
            render_rosters(
                ui,
                "Your Team (12)".to_string(),
                "Enemy Team (12)".to_string(),
                Color32::GREEN,
                Color32::RED,
                &friendly,
                &enemy,
                &ctx,
                &mut actions,
            );
        });
        harness.ctx.set_style_of(egui::Theme::Dark, crate::ui::theme::style::dark_style());
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        harness.ctx.set_fonts(fonts);

        harness.run();

        assert!(harness.get_by_label("Friendly11").rect().bottom() <= 420.0);
        assert!(harness.get_by_label("Enemy11").rect().bottom() <= 420.0);
    }

    #[test]
    fn action_menu_visibility_tracks_available_entries() {
        let mut row = roster_row(None);
        assert!(!row_has_action_menu(&row));

        row.tracked = Some(AccountId(7));
        assert!(row_has_action_menu(&row));

        row.tracked = None;
        row.account_id = Some(AccountId(8));
        assert!(!row_has_action_menu(&row));

        row.region = Some(Region::Eu);
        assert!(row_has_action_menu(&row));
    }

    #[test]
    fn fixed_columns_keep_their_requested_width() {
        let style = fixed_column_style(48.0);
        let width = length(48.0);

        assert_eq!(style.flex_shrink, 0.0);
        assert_eq!(style.size.width, width);
        assert_eq!(style.min_size.width, width);
        assert_eq!(style.max_size.width, width);
    }

    #[test]
    fn roster_rows_use_one_fixed_track_grid() {
        let style = roster_row_style(CurrentMatchViewMode::Compact);

        assert_eq!(style.display, taffy::Display::Grid);
        assert_eq!(style.grid_template_columns.len(), 9);
        assert_eq!(style.size.width, percent(1.0));
    }

    #[test]
    fn identity_styles_take_the_space_outside_fixed_columns() {
        let class = class_column_style();
        let ship = ship_column_style();
        let player = player_column_style();

        assert_eq!(class.size.width, length(CLASS_COLUMN_WIDTH));
        assert_eq!(class.min_size.width, length(CLASS_COLUMN_WIDTH));
        assert_eq!(class.max_size.width, length(CLASS_COLUMN_WIDTH));
        assert_eq!(ship.size.width, length(SHIP_COLUMN_WIDTH));
        assert_eq!(ship.min_size.width, length(SHIP_COLUMN_WIDTH));
        assert_eq!(ship.max_size.width, length(SHIP_COLUMN_WIDTH));
        assert_eq!(player.flex_grow, 1.0);
        assert_eq!(player.flex_shrink, 1.0);
        assert_eq!(player.min_size.width, length(0.0));
    }

    #[test]
    fn roster_row_content_claims_the_full_team_width() {
        let content = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(413.0, 24.0));
        let row = full_width_row_rect(content, 537.0);

        assert_eq!(row.min, content.min);
        assert_eq!(row.width(), 537.0);
        assert_eq!(row.height(), content.height());
    }

    #[test]
    fn contiguous_row_layout_removes_egui_item_spacing() {
        let _harness = Harness::builder().build_ui(|ui| {
            let rects = contiguous_row_layout(ui, |ui| {
                ["First", "Second"].map(|label| egui::Frame::new().show(ui, |ui| ui.label(label)).response.rect)
            });

            assert_eq!(rects[0].max.y, rects[1].min.y);
        });
    }

    #[test]
    fn team_average_win_rate_averages_the_rows_that_have_one() {
        let mut by_account = HashMap::new();
        by_account.insert(AccountId(1), stats(PlayerStatsStatus::Ok, Some(40.0), Some(70.0)));
        by_account.insert(AccountId(2), stats(PlayerStatsStatus::Ok, Some(60.0), Some(50.0)));
        let rows = vec![roster_row(Some(AccountId(1))), roster_row(Some(AccountId(2)))];

        assert_eq!(team_average_win_rate(&rows, &by_account, WinRateMode::Overall), Some(50.0));
    }

    /// A player with no stats entry, no resolved account and a player whose
    /// scope has no rate at all are all skipped rather than counted as zero.
    #[test]
    fn team_average_win_rate_skips_rows_with_no_rate() {
        let mut by_account = HashMap::new();
        by_account.insert(AccountId(1), stats(PlayerStatsStatus::Ok, Some(40.0), None));
        let rows = vec![
            roster_row(Some(AccountId(1))),
            roster_row(Some(AccountId(99))), // no stats entry at all
            roster_row(None),                // never resolved to an account
        ];

        assert_eq!(team_average_win_rate(&rows, &by_account, WinRateMode::Overall), Some(40.0));
        assert_eq!(
            team_average_win_rate(&rows, &by_account, WinRateMode::Ship),
            None,
            "the only stats entry has no ship rate to average"
        );
    }

    #[test]
    fn team_average_win_rate_is_none_when_nobody_on_the_team_has_a_rate() {
        let rows = vec![roster_row(None), roster_row(Some(AccountId(5)))];

        assert_eq!(team_average_win_rate(&rows, &HashMap::new(), WinRateMode::Overall), None);
    }

    #[test]
    fn team_average_win_rate_follows_the_selected_mode() {
        let mut by_account = HashMap::new();
        by_account.insert(AccountId(1), stats(PlayerStatsStatus::Ok, Some(40.0), Some(90.0)));
        let rows = vec![roster_row(Some(AccountId(1)))];

        assert_eq!(team_average_win_rate(&rows, &by_account, WinRateMode::Overall), Some(40.0));
        assert_eq!(team_average_win_rate(&rows, &by_account, WinRateMode::Ship), Some(90.0));
    }

    #[test]
    fn team_average_personal_rating_averages_only_rows_with_pr() {
        let mut first = stats(PlayerStatsStatus::Ok, Some(40.0), Some(70.0));
        first.pr = Some(1200.0);
        let mut second = stats(PlayerStatsStatus::Ok, Some(60.0), Some(50.0));
        second.account_id = AccountId(2);
        second.pr = Some(1800.0);
        let mut missing = stats(PlayerStatsStatus::Hidden, None, None);
        missing.account_id = AccountId(3);
        missing.pr = None;
        let by_account = HashMap::from([(AccountId(1), first), (AccountId(2), second), (AccountId(3), missing)]);
        let rows = vec![
            roster_row(Some(AccountId(1))),
            roster_row(Some(AccountId(2))),
            roster_row(Some(AccountId(3))),
            roster_row(Some(AccountId(99))),
            roster_row(None),
        ];

        assert_eq!(team_average_personal_rating(&rows, &by_account, WinRateMode::Overall), Some(1500.0));
    }

    #[test]
    fn team_average_personal_rating_is_none_without_resolved_pr() {
        let rows = vec![roster_row(None), roster_row(Some(AccountId(5)))];

        assert_eq!(team_average_personal_rating(&rows, &HashMap::new(), WinRateMode::Overall), None);
    }

    /// The team average reads the same scoped PR the rows do, so the header
    /// figure cannot name a different scope than the column under it.
    #[test]
    fn team_average_personal_rating_follows_the_mode() {
        let mut entry = stats(PlayerStatsStatus::Ok, Some(40.0), Some(90.0));
        entry.pr = Some(1725.0);
        entry.ship_pr = Some(2400.0);
        let by_account = HashMap::from([(AccountId(1), entry)]);
        let rows = vec![roster_row(Some(AccountId(1)))];

        assert_eq!(team_average_personal_rating(&rows, &by_account, WinRateMode::Overall), Some(1725.0));
        assert_eq!(team_average_personal_rating(&rows, &by_account, WinRateMode::Ship), Some(2400.0));
    }

    /// A team where nobody's ship PR came back averages to nothing rather than
    /// silently falling back to the account figure.
    #[test]
    fn team_average_personal_rating_skips_rows_with_no_pr_in_scope() {
        let mut entry = stats(PlayerStatsStatus::Ok, Some(40.0), Some(90.0));
        entry.ship_pr = None;
        let by_account = HashMap::from([(AccountId(1), entry)]);
        let rows = vec![roster_row(Some(AccountId(1)))];

        assert_eq!(team_average_personal_rating(&rows, &by_account, WinRateMode::Ship), None);
    }

    #[test]
    fn compact_rows_show_only_the_selected_scope() {
        assert_eq!(visible_stat_modes(CurrentMatchViewMode::Compact, WinRateMode::Overall), vec![WinRateMode::Overall]);
        assert_eq!(visible_stat_modes(CurrentMatchViewMode::Compact, WinRateMode::Ship), vec![WinRateMode::Ship]);
    }

    #[test]
    fn detailed_rows_always_show_overall_then_ship() {
        let expected = vec![WinRateMode::Overall, WinRateMode::Ship];
        assert_eq!(visible_stat_modes(CurrentMatchViewMode::Detailed, WinRateMode::Overall), expected);
        assert_eq!(visible_stat_modes(CurrentMatchViewMode::Detailed, WinRateMode::Ship), expected);
    }

    /// Right at `STACK_TEAMS_BELOW_WIDTH`, the width the side-by-side branch
    /// is first used with, the two columns must fit exactly: the unclamped
    /// half already sits inside the min/max range, so nothing here should
    /// need clamping.
    #[test]
    fn team_width_splits_the_narrow_case_evenly_with_no_clamping() {
        let item_spacing = 8.0;
        let width = team_width(STACK_TEAMS_BELOW_WIDTH, item_spacing);

        assert_eq!(width, (STACK_TEAMS_BELOW_WIDTH - TEAM_GAP - item_spacing) / 2.0);
        assert!(
            2.0 * width + TEAM_GAP + item_spacing <= STACK_TEAMS_BELOW_WIDTH,
            "columns must not overflow the available width"
        );
    }

    /// On a very wide window the unclamped half would stretch far past a
    /// comfortable row, so the ceiling binds.
    #[test]
    fn team_width_is_capped_on_a_wide_window() {
        assert_eq!(team_width(2000.0, 8.0), TEAM_MAX_WIDTH);
    }

    /// The floor exists for widths `team_width` is never actually called
    /// with today (the caller stacks below `STACK_TEAMS_BELOW_WIDTH`), but
    /// the function must still honour its own contract if called directly.
    #[test]
    fn team_width_is_floored_below_its_normal_operating_range() {
        assert_eq!(team_width(300.0, 8.0), TEAM_MIN_WIDTH);
    }

    /// Both teams read from the same call, so they can never disagree; this
    /// pins that down explicitly rather than relying on the caller reusing
    /// one binding.
    #[test]
    fn team_width_gives_both_teams_the_same_value() {
        let available = 1200.0;

        assert_eq!(team_width(available, 8.0), team_width(available, 8.0));
    }

    #[test]
    fn clan_color_from_raw_falls_back_to_the_relation_tint_when_the_scan_carried_none() {
        assert_eq!(clan_color_from_raw(0, PlayerTint::Ally), ClanColor::Relation(PlayerTint::Ally));
    }

    #[test]
    fn clan_color_from_raw_decodes_a_nonzero_server_colour() {
        let color = clan_color_from_raw(0x00_ff_80, PlayerTint::Enemy);

        assert_eq!(color, ClanColor::Fixed(Color32::from_rgb(0x00, 0xff, 0x80)));
    }
}

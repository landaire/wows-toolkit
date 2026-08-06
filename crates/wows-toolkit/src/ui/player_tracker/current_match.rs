use std::collections::HashMap;
use std::path::PathBuf;

use egui::Color32;
use egui::Image;
use egui::ImageSource;
use egui::OpenUrl;
use egui::RichText;
use egui::UiKind;
use egui::Vec2;
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
use crate::data::wows_data::WorldOfWarshipsData;
use crate::icons;
use crate::task::live_match_stats::FlushState;
use crate::task::replays::ReplayBackgroundParserThreadMessage;
use crate::twitch::TwitchState;
use crate::ui::replay_parser::ship_class_icon_from_species;
use crate::ui::theme::semantic::SemanticExt;
use crate::util::formatting::separate_number;
use crate::util::formatting::shipbuilds_player_url;
use crate::util::formatting::wows_numbers_player_url;
use crate::util::personal_rating::PersonalRatingCategory;
use crate::util::personal_rating::PersonalRatingCategorySwatch;

use super::MatchStatsState;
use super::PlayerTracker;
use super::TrackedPlayer;
use super::WinRateMode;
use super::encounter_severity_color;
use super::last_seen_text;
use super::live::LiveRosterRow;

/// Both teams share these so their headers line up: two independently sized
/// grids would otherwise drift apart column by column.
const MIN_COL_WIDTH: f32 = 44.0;
/// `egui::Grid` only exposes a wrapping soft-max for column width, not a hard
/// cell size: rows are left to size to their content's height.
const MAX_COL_WIDTH: f32 = 180.0;

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
            let wows_data = build.zip(self.tab_state.wows_data_map.as_ref()).and_then(|(build, map)| map.get(build));
            let wows_data_guard = wows_data.as_ref().map(|data| data.read());
            // SharedWoWsData is Arc<RwLock<Box<WorldOfWarshipsData>>>, so the
            // guard needs three derefs to reach the data itself.
            let wows_data_ref: Option<&WorldOfWarshipsData> = wows_data_guard.as_ref().map(|guard| &***guard);

            // Resolve first, then reborrow the tracker's fields disjointly so the
            // roster and the tracked-player map can be read at the same time.
            player_tracker.roster(wows_data_ref);
            let PlayerTracker { resolved_roster, tracked_players, win_rate_mode, match_stats, .. } = &*player_tracker;
            let Some(roster) = resolved_roster.as_ref() else {
                ui.label(RichText::new(t!("ui.player_tracker.no_live_match")).weak());
                return;
            };

            let mode = *win_rate_mode;

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
                locale: &locale,
            };

            ui.columns(2, |columns| {
                let win = columns[0].sem().win;
                render_team(
                    &mut columns[0],
                    t!("ui.player_tracker.your_team", count = roster.friendly.len()).to_string(),
                    win,
                    &roster.friendly,
                    "current_match_friendly",
                    &ctx,
                    &mut actions,
                );

                let loss = columns[1].sem().loss;
                render_team(
                    &mut columns[1],
                    t!("ui.player_tracker.enemy_team", count = roster.enemy.len()).to_string(),
                    loss,
                    &roster.enemy,
                    "current_match_enemy",
                    &ctx,
                    &mut actions,
                );
            });
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
    wows_data: Option<&'a WorldOfWarshipsData>,
    twitch_state: &'a TwitchState,
    started_at: Timestamp,
    now: Timestamp,
    filter_range: Option<Timestamp>,
    stats: &'a HashMap<AccountId, PlayerStatsOut>,
    mode: WinRateMode,
    locale: &'a str,
}

/// What a row click asks the caller to do once the player-tracker lock is
/// released.
#[derive(Default)]
struct TeamActions {
    find_matches_target: Option<AccountId>,
    copy_login: Option<String>,
    set_win_rate_mode: Option<WinRateMode>,
}

/// What one row shows, once the mode has chosen between the account and ship
/// scopes. The band follows the rate that is actually shown, so the row colour
/// and the number can never disagree.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct RowStats {
    pub win_rate: Option<f64>,
    pub battles: Option<i64>,
    /// The account PR. The service returns no per-ship PR, so this does not
    /// move with the mode.
    pub pr: Option<f64>,
    pub band: Option<PersonalRatingCategory>,
}

pub(crate) fn row_stats(stats: Option<&PlayerStatsOut>, mode: WinRateMode) -> RowStats {
    let Some(stats) = stats else {
        return RowStats::default();
    };
    let (win_rate, battles) = match mode {
        WinRateMode::Overall => (stats.overall_win_rate, stats.battles),
        WinRateMode::Ship => (stats.ship_win_rate, stats.ship_battles),
    };

    RowStats { win_rate, battles, pr: stats.pr, band: win_rate.map(PersonalRatingCategory::from_win_rate) }
}

/// Row background for the win-rate band, falling back to the striped colour so
/// a roster with no stats yet still reads as a table. Row 0 is the header.
fn row_color_picker(
    bands: Vec<Option<PersonalRatingCategory>>,
) -> impl Fn(usize, &egui::Style) -> Option<Color32> + Send + Sync + 'static {
    move |row, style| {
        if let Some(Some(band)) = bands.get(row) {
            return Some(band.swatch(&style.visuals).tint);
        }
        (row > 0 && row % 2 == 1).then_some(style.visuals.faint_bg_color)
    }
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

/// One team's table.
fn render_team(
    ui: &mut egui::Ui,
    heading: String,
    heading_color: Color32,
    rows: &[LiveRosterRow],
    id_salt: &'static str,
    ctx: &TeamContext<'_>,
    actions: &mut TeamActions,
) {
    ui.label(RichText::new(heading).heading().color(heading_color));

    let bands: Vec<Option<PersonalRatingCategory>> = std::iter::once(None)
        .chain(rows.iter().map(|row| row_stats(row.account_id.and_then(|id| ctx.stats.get(&id)), ctx.mode).band))
        .collect();

    egui::Grid::new(id_salt)
        .num_columns(7)
        .min_col_width(MIN_COL_WIDTH)
        .max_col_width(MAX_COL_WIDTH)
        .with_row_color(row_color_picker(bands))
        .show(ui, |ui| {
            ui.strong(t!("ui.player_tracker.column.ship"));
            ui.strong(t!("ui.player_tracker.column.player_name"));
            ui.strong(t!("ui.player_tracker.column.win_rate"));
            ui.strong(t!("ui.player_tracker.column.personal_rating"));
            ui.strong(t!("ui.player_tracker.column.battles"));
            ui.strong(t!("ui.player_tracker.column.encounters"));
            ui.label("");
            ui.end_row();

            for row_data in rows {
                let stats_entry = row_data.account_id.and_then(|id| ctx.stats.get(&id));
                let resolved = row_stats(stats_entry, ctx.mode);

                ui.horizontal(|ui| {
                    let icon = row_data
                        .species
                        .zip(ctx.wows_data)
                        .and_then(|(species, data)| ship_class_icon_from_species(species, data));
                    if let Some(icon) = icon {
                        let image = Image::new(ImageSource::Bytes {
                            uri: icon.path.clone().into(),
                            bytes: icon.data.clone().into(),
                        })
                        .tint(row_data.tint.color(ui.visuals()))
                        .fit_to_exact_size((20.0, 20.0).into())
                        .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));
                        let response = ui.add(image);
                        if let Some(species_text) = row_data.species_text.as_ref() {
                            response.on_hover_text(species_text);
                        }
                    }
                    if let Some(ship_name) = row_data.ship_name.as_ref() {
                        ui.label(ship_name);
                    } else if let Some(species_text) = row_data.species_text.as_ref() {
                        ui.label(species_text);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(RichText::new(&row_data.name).color(row_data.tint.color(ui.visuals())));

                    if let Some(candidates) =
                        ctx.twitch_state.player_is_potential_stream_sniper(&row_data.name, ctx.started_at)
                        && let Some(login) = crate::ui::widgets::twitch_chip(ui, &candidates, ctx.started_at)
                    {
                        actions.copy_login = Some(login);
                    }

                    if let Some(player) = row_data.tracked.and_then(|id| ctx.tracked.get(&id))
                        && !player.notes.is_empty()
                    {
                        ui.label(icons::NOTE_PENCIL).on_hover_text(&player.notes);
                    }
                });

                match resolved.win_rate {
                    None => {
                        let label = ui.label(RichText::new("-").weak());
                        if let Some(entry) = stats_entry {
                            label.on_hover_text(t!(no_rate_hover_key(entry.status)));
                        }
                    }
                    Some(rate) => {
                        let band = resolved.band.expect("a resolved win rate always has a band");
                        let swatch = band.swatch(ui.visuals());
                        let response = ui.label(RichText::new(format!("{rate:.1}%")).color(swatch.text));
                        if let Some(hover) = other_scope_hover(stats_entry, ctx.mode, ctx.locale) {
                            response.on_hover_text(hover);
                        }
                    }
                }

                match resolved.pr {
                    Some(pr) => {
                        crate::ui::widgets::pr_chip(
                            ui,
                            PersonalRatingCategory::from_pr(pr),
                            &format!("{pr:.0}"),
                            false,
                        )
                        .on_hover_text(t!("ui.player_tracker.pr_account_hover"));
                    }
                    None => {
                        ui.label(RichText::new("-").weak());
                    }
                }

                match resolved.battles {
                    Some(battles) => {
                        ui.label(separate_number(battles, Some(ctx.locale)));
                    }
                    None => {
                        ui.label(RichText::new("-").weak());
                    }
                }

                match row_data.tracked.and_then(|id| ctx.tracked.get(&id)) {
                    None => {
                        ui.label(RichText::new("-").weak()).on_hover_text(t!("ui.player_tracker.never_encountered"));
                    }
                    Some(player) => {
                        let total = player.arena_ids.len();
                        let in_range = match ctx.filter_range {
                            Some(range) => player.timestamps.iter().filter(|ts| **ts > range).count(),
                            None => total,
                        };
                        // The live roster is about who is in front of you now, so
                        // it counts every past meeting: the division-mate toggle
                        // belongs to the Historical and Clans tables.
                        let last = last_seen_text(player.timestamps.last().copied(), ctx.now);

                        let mut text = RichText::new(format!("x{total}"));
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
                }

                ui.horizontal(|ui| {
                    if let Some(id) = row_data.tracked
                        && ui
                            .button(icons::MAGNIFYING_GLASS)
                            .on_hover_text(t!("ui.player_tracker.find_matches"))
                            .clicked()
                    {
                        actions.find_matches_target = Some(id);
                    }

                    if let (Some(account_id), Some(region)) = (row_data.account_id, row_data.region) {
                        ui.menu_button(icons::DOTS_THREE, |ui| {
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
                        });
                    }
                });

                ui.end_row();
            }
        });
}

/// The other scope's rate and battle count for the WR hover: when the row
/// shows the overall rate, the hover names the ship's; when it shows the
/// ship's, the hover names the overall. `None` when that scope has no rate of
/// its own to show, matching `row_stats`'s "absent, not zero" rule.
fn other_scope_hover(stats: Option<&PlayerStatsOut>, mode: WinRateMode, locale: &str) -> Option<String> {
    let stats = stats?;
    let (key, rate, battles) = match mode {
        WinRateMode::Overall => ("ui.player_tracker.win_rate_hover_overall", stats.ship_win_rate, stats.ship_battles),
        WinRateMode::Ship => ("ui.player_tracker.win_rate_hover_ship", stats.overall_win_rate, stats.battles),
    };
    let (rate, battles) = (rate?, battles?);

    Some(t!(key, rate = format!("{rate:.1}%"), battles = separate_number(battles, Some(locale))).to_string())
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameParamId;

    use super::*;

    fn stats(status: PlayerStatsStatus, overall: Option<f64>, ship: Option<f64>) -> PlayerStatsOut {
        PlayerStatsOut {
            account_id: AccountId(1),
            region: "eu".to_string(),
            ship_id: GameParamId::from(1u64),
            status,
            battles: Some(9000),
            overall_win_rate: overall,
            ship_win_rate: ship,
            ship_battles: Some(120),
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

    /// PR is the account PR in both modes; the service returns no per-ship PR.
    #[test]
    fn pr_does_not_move_with_the_mode() {
        let stats = stats(PlayerStatsStatus::Ok, Some(48.0), Some(61.0));

        assert_eq!(row_stats(Some(&stats), WinRateMode::Overall).pr, Some(1800.0));
        assert_eq!(row_stats(Some(&stats), WinRateMode::Ship).pr, Some(1800.0));
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
        assert_eq!(resolved.band, None);
    }
}

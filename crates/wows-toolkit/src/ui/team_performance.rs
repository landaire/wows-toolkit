use std::collections::HashMap;

use egui::Color32;
use egui::RichText;
use egui_plot::Legend;
use egui_plot::Line;
use egui_plot::MarkerShape;
use egui_plot::Plot;
use egui_plot::PlotPoint;
use egui_plot::PlotPoints;
use egui_plot::Points;
use egui_plot::Text;
use rust_i18n::t;

use crate::data::session_stats::TeamMemberGameStat;
use crate::tab_state::ChartableStat;
use crate::util::personal_rating::PersonalRatingData;
use crate::util::personal_rating::ShipBattleStats;

/// Generate a consistent color from a username string.
/// A hand-picked palette of maximally-distinct colors, tuned for visibility on dark backgrounds.
/// Using palette-by-index (not hash) guarantees contrast for small player counts.
fn palette_color(idx: usize) -> Color32 {
    const PALETTE: &[[u8; 3]] = &[
        [100, 160, 240], // blue   (self)
        [240, 120, 60],  // orange
        [80, 200, 90],   // green
        [230, 75, 75],   // red
        [170, 100, 220], // purple
        [50, 200, 190],  // teal
        [235, 110, 175], // pink
        [215, 190, 55],  // gold
        [160, 130, 55],  // olive
        [120, 210, 240], // sky blue
    ];
    let rgb = PALETTE[idx % PALETTE.len()];
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Extract a stat value from a TeamMemberGameStat, returning None if unavailable.
fn team_member_stat(
    member: &TeamMemberGameStat,
    stat: ChartableStat,
    is_win: bool,
    pr_data: Option<&PersonalRatingData>,
) -> Option<f64> {
    match stat {
        ChartableStat::Damage => member.damage.map(|v| v as f64),
        ChartableStat::SpottingDamage => member.spotting_damage.map(|v| v as f64),
        ChartableStat::Frags => member.frags.map(|v| v as f64),
        ChartableStat::RawXp => member.raw_xp.map(|v| v as f64),
        ChartableStat::BaseXp => member.base_xp.map(|v| v as f64),
        ChartableStat::WinRate => Some(if is_win { 100.0 } else { 0.0 }),
        ChartableStat::PersonalRating => {
            let pr_data = pr_data?;
            let stats = ShipBattleStats {
                ship_id: member.ship_id,
                battles: 1,
                damage: member.damage.unwrap_or(0),
                wins: if is_win { 1 } else { 0 },
                frags: member.frags.unwrap_or(0),
            };
            pr_data.calculate_pr(&[stats]).map(|r| r.pr)
        }
    }
}

/// Render the Div Performance chart tab.
///
/// `games` is the filtered list of session games in chronological order.
pub fn build_team_performance(tab_state: &mut crate::tab_state::TabState, ui: &mut egui::Ui) {
    let games: Vec<crate::data::session_stats::PerGameStat> = {
        let p = tab_state.persisted.read();
        p.session_stats.filtered_games().into_iter().cloned().collect()
    };

    if games.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(t!("ui.stats.no_stats").as_ref());
        });
        return;
    }

    // Collect all unique player names seen across the session.
    // The local player is always first, derived from `player_name` (or "You" for older data).
    let self_label: String = games
        .iter()
        .find_map(|g| if !g.player_name.is_empty() { Some(g.player_name.clone()) } else { None })
        .unwrap_or_else(|| t!("ui.stats.team.self_label").to_string());

    let mut all_players: Vec<String> = vec![self_label.clone()];
    for game in &games {
        for member in &game.team_members {
            if !all_players.contains(&member.username) {
                all_players.push(member.username.clone());
            }
        }
    }

    if all_players.len() == 1 {
        // Only self, no div mates recorded yet.
        ui.centered_and_justified(|ui| {
            ui.label(t!("ui.stats.team.no_team_data").as_ref());
        });
        return;
    }

    // Assign palette colors by stable position in all_players (self is always index 0).
    let player_colors: HashMap<String, Color32> =
        all_players.iter().enumerate().map(|(i, name)| (name.clone(), palette_color(i))).collect();

    let pr_data_arc = tab_state.personal_rating_data.clone();
    let pr_data = pr_data_arc.read();
    let pr_data_opt: Option<&PersonalRatingData> = if pr_data.is_loaded() { Some(&*pr_data) } else { None };

    let cfg = &mut tab_state.team_chart_config;

    // ── Controls bar ──
    ui.horizontal_wrapped(|ui| {
        // Stat selector
        ui.label(t!("ui.stats.stat_label").as_ref());
        let available_stats: &[ChartableStat] = &[
            ChartableStat::Damage,
            ChartableStat::SpottingDamage,
            ChartableStat::Frags,
            ChartableStat::RawXp,
            ChartableStat::BaseXp,
            ChartableStat::WinRate,
            ChartableStat::PersonalRating,
        ];
        for &stat in available_stats {
            let selected = cfg.selected_stat == stat;
            if ui.selectable_label(selected, stat.name()).clicked() && !selected {
                cfg.selected_stat = stat;
                cfg.reset_plot = true;
            }
        }

        ui.separator();

        // Rolling average
        let mut ra = cfg.rolling_average;
        if ui.checkbox(&mut ra, t!("ui.stats.rolling_avg").as_ref()).changed() {
            cfg.rolling_average = ra;
            cfg.reset_plot = true;
        }

        // Labels
        let mut sl = cfg.show_labels;
        if ui.checkbox(&mut sl, t!("ui.stats.labels").as_ref()).changed() {
            cfg.show_labels = sl;
        }
    });

    ui.separator();

    // ── Player selector ──
    ui.horizontal_wrapped(|ui| {
        ui.label(t!("ui.stats.team.players").as_ref());
        if ui.button(t!("ui.stats.all_ships").as_ref()).clicked() {
            cfg.deselected_players.clear();
        }
        if ui.button(t!("ui.stats.no_ships").as_ref()).clicked() {
            cfg.deselected_players = all_players.iter().cloned().collect();
        }
        for player in &all_players {
            let is_shown = !cfg.deselected_players.contains(player);
            let color = player_colors.get(player.as_str()).copied().unwrap_or(Color32::WHITE);
            let label = RichText::new(player).color(if is_shown { color } else { Color32::GRAY });
            if ui.selectable_label(is_shown, label).clicked() {
                if is_shown {
                    cfg.deselected_players.insert(player.clone());
                } else {
                    cfg.deselected_players.remove(player);
                }
            }
        }
    });

    ui.separator();

    let stat = cfg.selected_stat;
    let rolling_average = cfg.rolling_average;
    let show_labels = cfg.show_labels;
    let reset = cfg.reset_plot;
    cfg.reset_plot = false;

    // Build per-player time series.
    // For each player: Vec of (game_index: usize, stat_value: f64)
    let mut player_series: HashMap<String, Vec<(usize, f64)>> = HashMap::new();

    for (game_idx, game) in games.iter().enumerate() {
        // Synthesize a TeamMemberGameStat for the local player.
        let self_member = TeamMemberGameStat {
            username: self_label.clone(),
            ship_name: game.ship_name.clone(),
            ship_id: game.ship_id,
            damage: Some(game.damage),
            spotting_damage: Some(game.spotting_damage),
            frags: Some(game.frags),
            raw_xp: Some(game.raw_xp),
            base_xp: Some(game.base_xp),
        };

        let all_members = std::iter::once(&self_member).chain(game.team_members.iter());

        for member in all_members {
            if cfg.deselected_players.contains(&member.username) {
                continue;
            }
            if let Some(value) = team_member_stat(member, stat, game.is_win, pr_data_opt) {
                player_series.entry(member.username.clone()).or_default().push((game_idx, value));
            }
        }
    }

    if player_series.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(t!("chart.no_data").as_ref());
        });
        return;
    }

    // Sort players: self first, then allies by first appearance.
    let mut sorted_players: Vec<String> = player_series.keys().cloned().collect();
    sorted_players.sort_by_key(|name| {
        if name == &self_label {
            (0usize, 0usize) // always first
        } else {
            (1, player_series[name].first().map(|(idx, _)| *idx).unwrap_or(usize::MAX))
        }
    });

    // Build plot points per player.
    let mut series_data: Vec<(String, Vec<[f64; 2]>, Color32)> = Vec::new();

    for player in &sorted_players {
        let points = &player_series[player];
        let color = player_colors.get(player.as_str()).copied().unwrap_or(Color32::WHITE);
        let plot_points: Vec<[f64; 2]> = if rolling_average {
            if stat == ChartableStat::WinRate {
                let mut wins = 0u64;
                points
                    .iter()
                    .enumerate()
                    .map(|(i, (game_idx, v))| {
                        if *v > 50.0 {
                            wins += 1;
                        }
                        let rate = (wins as f64 / (i + 1) as f64) * 100.0;
                        [(*game_idx + 1) as f64, rate]
                    })
                    .collect()
            } else {
                let mut sum = 0.0;
                points
                    .iter()
                    .enumerate()
                    .map(|(i, (game_idx, v))| {
                        sum += v;
                        let avg = sum / (i + 1) as f64;
                        [(*game_idx + 1) as f64, avg]
                    })
                    .collect()
            }
        } else if stat == ChartableStat::WinRate {
            // Per-game win rate — skip (only meaningful as rolling average)
            Vec::new()
        } else {
            points.iter().map(|(game_idx, v)| [(*game_idx + 1) as f64, *v]).collect()
        };

        if !plot_points.is_empty() {
            series_data.push((player.clone(), plot_points, color));
        }
    }

    if series_data.is_empty() {
        if stat == ChartableStat::WinRate && !rolling_average {
            ui.label(t!("chart.win_rate_unavailable").as_ref());
        } else {
            ui.label(t!("chart.no_data").as_ref());
        }
        return;
    }

    let y_label: String = if rolling_average {
        match stat {
            ChartableStat::WinRate => t!("stat.win_rate_pct").into(),
            _ => stat.name(),
        }
    } else {
        stat.name()
    };

    let min_y = series_data.iter().flat_map(|(_, pts, _)| pts.iter().map(|p| p[1])).fold(0.0f64, f64::min);

    let title =
        if rolling_average { format!("{} {}", stat.name(), t!("chart.rolling_average_suffix")) } else { stat.name() };

    ui.group(|ui| {
        ui.vertical_centered(|ui| ui.heading(&title));

        let mut plot = Plot::new(egui::Id::new("team_performance_chart"))
            .legend(Legend::default())
            .x_axis_label(t!("chart.game_number"))
            .y_axis_label(y_label)
            .auto_bounds([true, true])
            .include_y(0.0_f64.min(min_y));

        if reset {
            plot = plot.reset();
        }

        plot.show(ui, |plot_ui| {
            for (name, points, color) in &series_data {
                plot_ui.line(Line::new(name.clone(), PlotPoints::from(points.clone())).color(*color));
                plot_ui.points(
                    Points::new(name.clone(), PlotPoints::from(points.clone()))
                        .color(*color)
                        .radius(4.0)
                        .shape(MarkerShape::Circle)
                        .filled(true),
                );
                if show_labels {
                    for point in points {
                        let label = if point[1] >= 1000.0 {
                            format!("{:.0}", point[1])
                        } else if point[1] >= 10.0 {
                            format!("{:.1}", point[1])
                        } else {
                            format!("{:.2}", point[1])
                        };
                        plot_ui.text(
                            Text::new("", PlotPoint::new(point[0], point[1]), RichText::new(label).size(14.0))
                                .color(*color)
                                .anchor(egui::Align2::CENTER_BOTTOM),
                        );
                    }
                }
            }
        });
    });
}

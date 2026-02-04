//! Session statistics chart window and rendering functions

use egui::Color32;
use egui_plot::Bar;
use egui_plot::BarChart;
use egui_plot::Legend;
use egui_plot::Line;
use egui_plot::MarkerShape;
use egui_plot::Plot;
use egui_plot::PlotPoints;
use egui_plot::Points;

use crate::personal_rating::PersonalRatingData;
use crate::session_stats::PerGameStat;
use crate::session_stats::PerformanceInfo;
use crate::tab_state::ChartableStat;

/// Generate a consistent color from a ship name using its hash.
/// Uses HSV with fixed saturation and value for good contrast.
pub fn color_from_name(name: &str) -> Color32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish();

    // Use the hash to generate a hue (0-360)
    let hue = (hash % 360) as f32;
    // Fixed saturation and value for vibrant, visible colors
    let saturation = 0.7;
    let value = 0.9;

    // Convert HSV to RGB
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = value - c;

    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    Color32::from_rgb(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

/// Render a line chart showing per-game statistics over time
pub fn render_line_chart(
    ui: &mut egui::Ui,
    per_game_data: &[&PerGameStat],
    stat: ChartableStat,
    selected_ships: &[String],
    pr_data: &PersonalRatingData,
    rolling_average: bool,
) {
    // Win rate doesn't make sense per-game (but rolling average win rate does)
    if stat == ChartableStat::WinRate && !rolling_average {
        ui.label("Win Rate is not available for per-game line charts (enable Rolling Average or use Bar chart)");
        return;
    }

    // Get unique ships from the data that are in selected_ships, preserving order
    let mut unique_ships: Vec<String> = Vec::new();
    for game in per_game_data {
        if selected_ships.contains(&game.ship_name) && !unique_ships.contains(&game.ship_name) {
            unique_ships.push(game.ship_name.clone());
        }
    }

    // Prepare data for each ship
    let mut ship_data: Vec<(String, Vec<[f64; 2]>, Color32)> = Vec::new();

    let pr_data_opt = if pr_data.is_loaded() { Some(pr_data) } else { None };

    for ship_name in &unique_ships {
        // For win rate with rolling average, we need to track wins separately
        if stat == ChartableStat::WinRate && rolling_average {
            let ship_games: Vec<bool> =
                per_game_data.iter().filter(|g| &g.ship_name == ship_name).map(|g| g.is_win).collect();

            if ship_games.is_empty() {
                continue;
            }

            // Calculate rolling win rate
            let mut wins = 0u64;
            let points: Vec<[f64; 2]> = ship_games
                .iter()
                .enumerate()
                .map(|(i, &is_win)| {
                    if is_win {
                        wins += 1;
                    }
                    let win_rate = (wins as f64 / (i + 1) as f64) * 100.0;
                    [i as f64 + 1.0, win_rate]
                })
                .collect();

            ship_data.push((ship_name.clone(), points, color_from_name(ship_name)));
        } else {
            let ship_games: Vec<f64> = per_game_data
                .iter()
                .filter(|g| &g.ship_name == ship_name)
                .map(|g| g.get_stat(stat, pr_data_opt))
                .collect();

            if ship_games.is_empty() {
                continue;
            }

            let points: Vec<[f64; 2]> = if rolling_average {
                // Calculate rolling average
                let mut sum = 0.0;
                ship_games
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        sum += v;
                        let avg = sum / (i + 1) as f64;
                        [i as f64 + 1.0, avg]
                    })
                    .collect()
            } else {
                // Use raw values
                ship_games.iter().enumerate().map(|(i, v)| [i as f64 + 1.0, *v]).collect()
            };

            ship_data.push((ship_name.clone(), points, color_from_name(ship_name)));
        }
    }

    if ship_data.is_empty() {
        ui.label("No data available for selected ships");
        return;
    }

    let y_label = if rolling_average {
        match stat {
            ChartableStat::WinRate => "Win Rate (%)",
            _ => stat.name(),
        }
    } else {
        stat.name()
    };

    Plot::new("line_chart")
        .legend(Legend::default())
        .x_axis_label("Game #")
        .y_axis_label(y_label)
        .auto_bounds([true, true])
        .show(ui, |plot_ui| {
            for (name, points, color) in &ship_data {
                // Draw the line
                plot_ui.line(Line::new(name.clone(), PlotPoints::from(points.clone())).color(*color));
                // Draw points/markers so single data points are visible
                plot_ui.points(
                    Points::new(name.clone(), PlotPoints::from(points.clone()))
                        .color(*color)
                        .radius(4.0)
                        .shape(MarkerShape::Circle)
                        .filled(true),
                );
            }
        });
}

/// Render a bar chart showing cumulative/average statistics per ship
pub fn render_bar_chart(
    ui: &mut egui::Ui,
    ship_stats: &[(&String, &PerformanceInfo)],
    stat: ChartableStat,
    pr_data: &PersonalRatingData,
) {
    let mut bar_charts: Vec<BarChart> = Vec::new();

    for (i, (ship_name, perf_info)) in ship_stats.iter().enumerate() {
        let value = match stat {
            ChartableStat::Damage => perf_info.avg_damage().unwrap_or_default(),
            ChartableStat::SpottingDamage => perf_info.avg_spotting_damage().unwrap_or_default(),
            ChartableStat::Frags => perf_info.avg_frags().unwrap_or_default(),
            ChartableStat::RawXp => perf_info.avg_xp().unwrap_or_default(),
            ChartableStat::BaseXp => perf_info.avg_win_adjusted_xp().unwrap_or_default(),
            ChartableStat::WinRate => perf_info.win_rate().unwrap_or_default(),
            ChartableStat::PersonalRating => perf_info.calculate_pr(pr_data).map(|r| r.pr).unwrap_or_default(),
        };

        let bar = Bar::new(i as f64, value).width(0.7);
        let chart = BarChart::new(ship_name.as_str(), vec![bar]).color(color_from_name(ship_name));

        bar_charts.push(chart);
    }

    let y_label = match stat {
        ChartableStat::Damage => "Avg Damage",
        ChartableStat::SpottingDamage => "Avg Spotting Damage",
        ChartableStat::Frags => "Avg Frags",
        ChartableStat::RawXp => "Avg Raw XP",
        ChartableStat::BaseXp => "Avg Base XP",
        ChartableStat::WinRate => "Win Rate (%)",
        ChartableStat::PersonalRating => "Avg PR",
    };

    Plot::new("bar_chart")
        .legend(Legend::default())
        .y_axis_label(y_label)
        .show_axes([false, true])
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .view_aspect(2.0)
        .show(ui, |plot_ui| {
            for chart in bar_charts {
                plot_ui.bar_chart(chart);
            }
        });
}

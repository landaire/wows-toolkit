use nsh_core::prelude::*;

use crate::native::database_runtime;

/// Aggregate battle statistics from the shared session database.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct StatisticsState {
    pub games: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub win_rate: f64,
    pub average_damage: f64,
    pub average_base_xp: f64,
    pub average_frags: f64,
    pub ships: Vec<ShipStatistics>,
}

/// Statistics for one ship.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ShipStatistics {
    pub ship_name: String,
    pub ship_id: String,
    pub games: u32,
    pub win_rate: f64,
    pub average_damage: f64,
    pub average_base_xp: f64,
}

#[derive(Debug)]
struct ShipAccumulator {
    ship_name: String,
    ship_id: u64,
    games: u32,
    wins: u32,
    damage: u64,
    base_xp: i64,
}

/// Session statistics services exposed to the TSX interface.
#[module]
pub struct Stats;

#[module]
impl Stats {
    /// Aggregates recorded battles from the shared session database.
    pub async fn load_statistics() -> Result<StatisticsState, String> {
        blocking(load_statistics).await.map_err(|error| format!("statistics reader task failed: {error}"))?
    }
}

fn load_statistics() -> Result<StatisticsState, String> {
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the statistics database: {error}"))?;
        let rows = wows_toolkit_config::queries::get_all_session_stats(&pool)
            .await
            .map_err(|error| format!("could not read session statistics: {error}"))?;
        let games = u32::try_from(rows.len()).map_err(|_| "too many session statistics rows".to_owned())?;
        let wins = u32::try_from(rows.iter().filter(|row| row.is_win).count())
            .map_err(|_| "too many winning session statistics rows".to_owned())?;
        let losses = u32::try_from(rows.iter().filter(|row| row.is_loss).count())
            .map_err(|_| "too many losing session statistics rows".to_owned())?;
        let draws = u32::try_from(rows.iter().filter(|row| row.is_draw).count())
            .map_err(|_| "too many drawn session statistics rows".to_owned())?;
        let total_damage = rows.iter().try_fold(0u64, |total, row| {
            total
                .checked_add(row.damage as u64)
                .ok_or_else(|| "total session damage exceeds the supported range".to_owned())
        })?;
        let total_base_xp = rows.iter().try_fold(0i64, |total, row| {
            total.checked_add(row.base_xp).ok_or_else(|| "total session base XP exceeds the supported range".to_owned())
        })?;
        let total_frags = rows.iter().try_fold(0i64, |total, row| {
            total.checked_add(row.frags).ok_or_else(|| "total session frags exceeds the supported range".to_owned())
        })?;
        let mut by_ship = std::collections::BTreeMap::<u64, ShipAccumulator>::new();
        for row in &rows {
            let ship_id = row.ship_id as u64;
            let ship = by_ship.entry(ship_id).or_insert_with(|| ShipAccumulator {
                ship_name: row.ship_name.clone(),
                ship_id,
                games: 0,
                wins: 0,
                damage: 0,
                base_xp: 0,
            });
            ship.games += 1;
            ship.wins += u32::from(row.is_win);
            ship.damage = ship
                .damage
                .checked_add(row.damage as u64)
                .ok_or_else(|| format!("damage for ship {ship_id} exceeds the supported range"))?;
            ship.base_xp = ship
                .base_xp
                .checked_add(row.base_xp)
                .ok_or_else(|| format!("base XP for ship {ship_id} exceeds the supported range"))?;
        }
        let mut ships = by_ship
            .into_values()
            .map(|ship| ShipStatistics {
                ship_name: ship.ship_name,
                ship_id: ship.ship_id.to_string(),
                games: ship.games,
                win_rate: percentage(ship.wins, ship.games),
                average_damage: average_u64(ship.damage, ship.games),
                average_base_xp: average(ship.base_xp, ship.games),
            })
            .collect::<Vec<_>>();
        ships.sort_by(|left, right| {
            right
                .games
                .cmp(&left.games)
                .then_with(|| left.ship_name.cmp(&right.ship_name))
                .then_with(|| left.ship_id.cmp(&right.ship_id))
        });

        Ok(StatisticsState {
            games,
            wins,
            losses,
            draws,
            win_rate: percentage(wins, games),
            average_damage: average_u64(total_damage, games),
            average_base_xp: average(total_base_xp, games),
            average_frags: average(total_frags, games),
            ships,
        })
    })
}

fn percentage(part: u32, total: u32) -> f64 {
    if total == 0 { 0.0 } else { f64::from(part) * 100.0 / f64::from(total) }
}

fn average_u64(total: u64, count: u32) -> f64 {
    if count == 0 { 0.0 } else { total as f64 / f64::from(count) }
}

fn average(total: i64, count: u32) -> f64 {
    if count == 0 { 0.0 } else { total as f64 / f64::from(count) }
}

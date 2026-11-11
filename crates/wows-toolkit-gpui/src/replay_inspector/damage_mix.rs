//! Damage by type: the kind each results key counts toward, the colour each
//! kind is drawn in, and the segments of a player's damage-mix bar. Shell,
//! torpedo and aircraft colours are the replay renderer's, so a type reads
//! the same in the table as on the minimap.

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use wows_minimap_renderer::renderer::COLOR_PLANES;
use wows_minimap_renderer::renderer::COLOR_TORP;
use wows_minimap_renderer::renderer::ammo_type_color;
use wows_replay_insights::battle_report::DAMAGE_ADBOMB;
use wows_replay_insights::battle_report::DAMAGE_ATBA_AP;
use wows_replay_insights::battle_report::DAMAGE_ATBA_AP_MANUAL;
use wows_replay_insights::battle_report::DAMAGE_ATBA_CS;
use wows_replay_insights::battle_report::DAMAGE_ATBA_CS_MANUAL;
use wows_replay_insights::battle_report::DAMAGE_ATBA_HE;
use wows_replay_insights::battle_report::DAMAGE_ATBA_HE_MANUAL;
use wows_replay_insights::battle_report::DAMAGE_BOMB;
use wows_replay_insights::battle_report::DAMAGE_BOMB_ALT;
use wows_replay_insights::battle_report::DAMAGE_DBOMB_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_FIRE;
use wows_replay_insights::battle_report::DAMAGE_FLOOD;
use wows_replay_insights::battle_report::DAMAGE_MAIN_AP;
use wows_replay_insights::battle_report::DAMAGE_MAIN_CS;
use wows_replay_insights::battle_report::DAMAGE_MAIN_HE;
use wows_replay_insights::battle_report::DAMAGE_RAM;
use wows_replay_insights::battle_report::DAMAGE_ROCKET;
use wows_replay_insights::battle_report::DAMAGE_ROCKET_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_SKIP;
use wows_replay_insights::battle_report::DAMAGE_SKIP_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_SKIP_ALT;
use wows_replay_insights::battle_report::DAMAGE_TBOMB;
use wows_replay_insights::battle_report::DAMAGE_TBOMB_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_TBOMB_ALT;
use wows_replay_insights::battle_report::DAMAGE_TPD_ALTER;
use wows_replay_insights::battle_report::DAMAGE_TPD_DEEP;
use wows_replay_insights::battle_report::DAMAGE_TPD_NORMAL;
use wows_replay_insights::battle_report::DAMAGE_TPD_PHOTON;
use wowsunpack::game_params::types::AmmoType;

use super::columns::ReplayColumn;
use super::model::PlayerRow;
use super::model::separate_number;

/// Height of the bar under a damage figure.
const BAR_HEIGHT: Pixels = px(4.);

/// Side of the colour key square beside a listed type.
pub const SWATCH_SIZE: f32 = 8.0;

/// A kind below this percent of the total is drawn as part of `Other`.
const MERGE_BELOW_PERCENT: u64 = 3;

/// What a damage type counts toward in the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DamageKind {
    Ap,
    Sap,
    He,
    Secondary,
    Plane,
    Ram,
    Torpedo,
    Fire,
    Flood,
    Other,
}

impl DamageKind {
    const ALL: [Self; 10] = [
        Self::Ap,
        Self::Sap,
        Self::He,
        Self::Secondary,
        Self::Plane,
        Self::Ram,
        Self::Torpedo,
        Self::Fire,
        Self::Flood,
        Self::Other,
    ];

    /// Drawn after the others in this order, whatever their size, so the
    /// same types sit in the same place from row to row.
    const FIXED_TAIL: [Self; 3] = [Self::Torpedo, Self::Fire, Self::Flood];

    /// The kind a results key (`DAMAGE_*`) counts toward.
    pub fn of_key(key: &str) -> Self {
        match key {
            DAMAGE_MAIN_AP => Self::Ap,
            DAMAGE_MAIN_CS => Self::Sap,
            DAMAGE_MAIN_HE => Self::He,
            DAMAGE_ATBA_AP
            | DAMAGE_ATBA_AP_MANUAL
            | DAMAGE_ATBA_CS
            | DAMAGE_ATBA_CS_MANUAL
            | DAMAGE_ATBA_HE
            | DAMAGE_ATBA_HE_MANUAL => Self::Secondary,
            DAMAGE_TPD_NORMAL | DAMAGE_TPD_DEEP | DAMAGE_TPD_ALTER | DAMAGE_TPD_PHOTON => Self::Torpedo,
            DAMAGE_BOMB
            | DAMAGE_DBOMB_AIRSUPPORT
            | DAMAGE_ADBOMB
            | DAMAGE_BOMB_ALT
            | DAMAGE_TBOMB
            | DAMAGE_TBOMB_ALT
            | DAMAGE_TBOMB_AIRSUPPORT
            | DAMAGE_ROCKET
            | DAMAGE_ROCKET_AIRSUPPORT
            | DAMAGE_SKIP
            | DAMAGE_SKIP_ALT
            | DAMAGE_SKIP_AIRSUPPORT => Self::Plane,
            DAMAGE_FIRE => Self::Fire,
            DAMAGE_FLOOD => Self::Flood,
            DAMAGE_RAM => Self::Ram,
            _ => Self::Other,
        }
    }

    /// The renderer's colours are tuned for a dark map; on a light theme
    /// each is darkened so a thin bar still stands off the panel.
    pub fn color(self) -> Hsla {
        let packed = match self {
            Self::Ap => ammo_type_color(&AmmoType::AP),
            Self::Sap => ammo_type_color(&AmmoType::SAP),
            Self::He => ammo_type_color(&AmmoType::HE),
            Self::Torpedo => COLOR_TORP,
            Self::Plane => COLOR_PLANES,
            Self::Secondary => [235, 140, 215],
            Self::Fire => [255, 225, 70],
            Self::Flood => [80, 110, 235],
            Self::Ram => [180, 130, 90],
            Self::Other => [160, 160, 160],
        };
        let [r, g, b] = packed;
        let color: Hsla = rgb(u32::from_be_bytes([0, r, g, b])).into();
        if crate::theme::is_dark_mode() { color } else { Hsla { l: color.l * 0.72, ..color } }
    }
}

/// One results key's damage, as a breakdown lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DamageLine {
    pub kind: DamageKind,
    pub label: &'static str,
    pub amount: u64,
}

impl DamageLine {
    /// The lines `descriptions` yields for `get`, in the table's own order,
    /// leaving out the keys with nothing behind them.
    pub fn from_results<F: Fn(&str) -> u64>(
        descriptions: &'static [(&'static str, &'static str)],
        get: F,
    ) -> Vec<Self> {
        descriptions
            .iter()
            .filter_map(|(key, label)| {
                let amount = get(key);
                (amount > 0).then(|| Self { kind: DamageKind::of_key(key), label, amount })
            })
            .collect()
    }
}

/// One stretch of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub kind: DamageKind,
    pub amount: u64,
}

/// A player's damage split into the bar's segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DamageMix {
    segments: Vec<Segment>,
    total: u64,
}

impl DamageMix {
    /// `None` when the lines add up to no damage.
    pub fn of(lines: &[DamageLine]) -> Option<Self> {
        let total = lines.iter().fold(0_u64, |sum, line| sum.saturating_add(line.amount));
        if total == 0 {
            return None;
        }

        let mut by_kind = [0_u64; DamageKind::ALL.len()];
        for line in lines {
            by_kind[line.kind as usize] = by_kind[line.kind as usize].saturating_add(line.amount);
        }
        let small = |amount: u64| u128::from(amount) * 100 < u128::from(total) * u128::from(MERGE_BELOW_PERCENT);

        let mut other = 0;
        let mut ranked = Vec::new();
        let mut fixed = Vec::new();
        for kind in DamageKind::ALL {
            let amount = by_kind[kind as usize];
            if amount == 0 {
                continue;
            }
            if kind == DamageKind::Other || small(amount) {
                other = amount.saturating_add(other);
            } else if DamageKind::FIXED_TAIL.contains(&kind) {
                fixed.push(Segment { kind, amount });
            } else {
                ranked.push(Segment { kind, amount });
            }
        }

        // Stable, so equal amounts keep the kinds' declared order.
        ranked.sort_by_key(|segment| std::cmp::Reverse(segment.amount));
        let mut segments = ranked;
        segments.extend(fixed);
        if other > 0 {
            segments.push(Segment { kind: DamageKind::Other, amount: other });
        }

        Some(Self { segments, total })
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The kind whose segment holds `kind`: itself, or `Other` when it was
    /// too small to draw on its own.
    pub fn shown_kind(&self, kind: DamageKind) -> DamageKind {
        if self.segments.iter().any(|segment| segment.kind == kind) { kind } else { DamageKind::Other }
    }

    /// `amount` as a percent of this mix's total.
    pub fn percent(&self, amount: u64) -> f64 {
        amount as f64 * 100. / self.total as f64
    }

    /// The bar, filling the width it is given.
    pub fn bar(&self) -> impl IntoElement {
        let last = self.segments.len() - 1;
        h_flex().w_full().h(BAR_HEIGHT).flex_none().children(self.segments.iter().enumerate().map(
            |(index, segment)| {
                let piece = div().h_full().bg(segment.kind.color());
                // The last takes what is left, so rounding leaves no gap at the end.
                if index == last {
                    piece.flex_1()
                } else {
                    piece.flex_none().w(relative((segment.amount as f64 / self.total as f64) as f32))
                }
            },
        ))
    }

    /// Every line with its colour key, amount and share of the total, for a
    /// tooltip.
    pub fn listing(&self, lines: &[DamageLine]) -> AnyElement {
        let column = || v_flex().gap_px();
        let mut keys = column();
        let mut amounts = column().items_end();
        let mut percents = column().items_end();
        for line in lines {
            keys = keys.child(h_flex().gap_1().child(swatch(self.shown_kind(line.kind))).child(line.label));
            amounts = amounts.child(separate_number(line.amount));
            percents = percents.child(format!("{:.0}%", self.percent(line.amount)));
        }

        h_flex().gap_2().items_start().text_xs().child(keys).child(amounts).child(percents).into_any_element()
    }
}

/// The per-type lines behind a damage column's figure. `None` for other
/// columns, for stats the row hides, and for a row the results carry no split
/// for.
pub fn shown_lines(row: &PlayerRow, col: ReplayColumn, debug: bool) -> Option<&[DamageLine]> {
    if row.should_hide_stats() && !debug {
        return None;
    }

    match col {
        ReplayColumn::ActualDamage => row.actual_damage_by_type.as_deref(),
        ReplayColumn::ReceivedDamage => row.received_damage_by_type.as_deref(),
        _ => None,
    }
}

/// The colour key square for `kind`.
pub fn swatch(kind: DamageKind) -> impl IntoElement {
    div().flex_none().size(px(SWATCH_SIZE)).bg(kind.color())
}

#[cfg(test)]
mod tests {
    use super::DamageKind;
    use super::DamageLine;
    use super::DamageMix;
    use super::Segment;

    fn line(kind: DamageKind, amount: u64) -> DamageLine {
        DamageLine { kind, label: "", amount }
    }

    fn kinds(mix: &DamageMix) -> Vec<(DamageKind, u64)> {
        mix.segments().iter().map(|segment| (segment.kind, segment.amount)).collect()
    }

    #[test]
    fn largest_first_with_torpedo_fire_and_flood_last_in_fixed_order() {
        let lines = [
            line(DamageKind::Flood, 10_000),
            line(DamageKind::He, 20_000),
            line(DamageKind::Fire, 30_000),
            line(DamageKind::Ap, 40_000),
            line(DamageKind::Torpedo, 5_000),
        ];

        let mix = DamageMix::of(&lines).expect("damage was dealt");

        assert_eq!(
            kinds(&mix),
            vec![
                (DamageKind::Ap, 40_000),
                (DamageKind::He, 20_000),
                (DamageKind::Torpedo, 5_000),
                (DamageKind::Fire, 30_000),
                (DamageKind::Flood, 10_000),
            ]
        );
    }

    #[test]
    fn small_kinds_merge_into_other_at_the_end() {
        let lines = [
            line(DamageKind::Ap, 9_000),
            line(DamageKind::Ram, 200),
            line(DamageKind::Fire, 250),
            line(DamageKind::Other, 100),
            line(DamageKind::Sap, 450),
        ];

        let mix = DamageMix::of(&lines).expect("damage was dealt");

        assert_eq!(kinds(&mix), vec![(DamageKind::Ap, 9_000), (DamageKind::Sap, 450), (DamageKind::Other, 550)]);
        assert_eq!(mix.shown_kind(DamageKind::Fire), DamageKind::Other);
        assert_eq!(mix.shown_kind(DamageKind::Sap), DamageKind::Sap);
    }

    #[test]
    fn lines_of_one_kind_add_into_one_segment() {
        let lines = [line(DamageKind::Secondary, 1_000), line(DamageKind::Secondary, 3_000)];

        let mix = DamageMix::of(&lines).expect("damage was dealt");

        assert_eq!(mix.segments(), [Segment { kind: DamageKind::Secondary, amount: 4_000 }]);
        assert_eq!(mix.percent(1_000), 25.);
    }

    #[test]
    fn no_damage_is_no_bar() {
        assert_eq!(DamageMix::of(&[]), None);
    }

    #[test]
    fn keys_of_one_weapon_count_toward_one_kind() {
        let lines =
            DamageLine::from_results(&wows_replay_insights::battle_report::DAMAGE_DESCRIPTIONS, |key| match key {
                "damage_atba_he" | "damage_atba_ap_manual" | "damage_skip" | "damage_tpd_deep" => 1,
                _ => 0,
            });

        let kinds: Vec<DamageKind> = lines.iter().map(|line| line.kind).collect();
        assert_eq!(kinds, [DamageKind::Secondary, DamageKind::Secondary, DamageKind::Torpedo, DamageKind::Plane]);
    }
}

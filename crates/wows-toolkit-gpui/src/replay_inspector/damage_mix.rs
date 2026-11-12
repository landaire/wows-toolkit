//! Damage by type: the kind each results key counts toward, the colour each
//! kind is drawn in, and the segments of a player's damage-mix bar.

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use wows_minimap_renderer::renderer::Armament;
use wows_replay_insights::battle_report::DAMAGE_ATBA_AP;
use wows_replay_insights::battle_report::DAMAGE_ATBA_AP_MANUAL;
use wows_replay_insights::battle_report::DAMAGE_ATBA_CS;
use wows_replay_insights::battle_report::DAMAGE_ATBA_CS_MANUAL;
use wows_replay_insights::battle_report::DAMAGE_ATBA_HE;
use wows_replay_insights::battle_report::DAMAGE_ATBA_HE_MANUAL;
use wows_replay_insights::battle_report::DAMAGE_BOMB_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_BOMB_ALT;
use wows_replay_insights::battle_report::DAMAGE_BOMB_AVIA;
use wows_replay_insights::battle_report::DAMAGE_DBOMB_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_FIRE;
use wows_replay_insights::battle_report::DAMAGE_FLOOD;
use wows_replay_insights::battle_report::DAMAGE_MAIN_AP;
use wows_replay_insights::battle_report::DAMAGE_MAIN_CS;
use wows_replay_insights::battle_report::DAMAGE_MAIN_HE;
use wows_replay_insights::battle_report::DAMAGE_RAM;
use wows_replay_insights::battle_report::DAMAGE_ROCKET_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_ROCKET_ALT;
use wows_replay_insights::battle_report::DAMAGE_ROCKET_AVIA;
use wows_replay_insights::battle_report::DAMAGE_SKIP_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_SKIP_ALT;
use wows_replay_insights::battle_report::DAMAGE_SKIP_AVIA;
use wows_replay_insights::battle_report::DAMAGE_TBOMB_AIRSUPPORT;
use wows_replay_insights::battle_report::DAMAGE_TBOMB_ALT;
use wows_replay_insights::battle_report::DAMAGE_TBOMB_AVIA;
use wows_replay_insights::battle_report::DAMAGE_TPD_ALTER;
use wows_replay_insights::battle_report::DAMAGE_TPD_DEEP;
use wows_replay_insights::battle_report::DAMAGE_TPD_NORMAL;
use wows_replay_insights::battle_report::DAMAGE_TPD_PHOTON;
use wowsunpack::game_params::types::AmmoType;

use super::columns::ReplayColumn;
use super::model::PlayerRow;
use super::model::separate_number;

/// Height of the stacked bar under a damage figure.
const BAR_HEIGHT: Pixels = px(4.);

/// Side of the colour key square beside a listed type.
pub const SWATCH_SIZE: f32 = 8.0;

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
    const FIXED_TAIL: [Self; 4] = [Self::Torpedo, Self::Fire, Self::Flood, Self::Other];

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
            DAMAGE_BOMB_AVIA
            | DAMAGE_BOMB_ALT
            | DAMAGE_BOMB_AIRSUPPORT
            | DAMAGE_DBOMB_AIRSUPPORT
            | DAMAGE_TBOMB_AVIA
            | DAMAGE_TBOMB_ALT
            | DAMAGE_TBOMB_AIRSUPPORT
            | DAMAGE_ROCKET_AVIA
            | DAMAGE_ROCKET_ALT
            | DAMAGE_ROCKET_AIRSUPPORT
            | DAMAGE_SKIP_AVIA
            | DAMAGE_SKIP_ALT
            | DAMAGE_SKIP_AIRSUPPORT => Self::Plane,
            DAMAGE_FIRE => Self::Fire,
            DAMAGE_FLOOD => Self::Flood,
            DAMAGE_RAM => Self::Ram,
            _ => Self::Other,
        }
    }

    /// The kind's colour before the theme's adjustment. Weapon kinds take
    /// the replay renderer's armament colour. The others have none there and
    /// are chosen to stay apart from that set: fire yellow, flooding deep
    /// blue, ramming brown, anything else grey.
    fn rgb(self) -> [u8; 3] {
        match self {
            Self::Ap => Armament::MainBattery(AmmoType::AP).color(),
            Self::Sap => Armament::MainBattery(AmmoType::SAP).color(),
            Self::He => Armament::MainBattery(AmmoType::HE).color(),
            Self::Secondary => Armament::Secondaries.color(),
            Self::Torpedo => Armament::Torpedoes.color(),
            Self::Plane => Armament::Planes.color(),
            Self::Fire => [255, 225, 70],
            Self::Flood => [80, 110, 235],
            Self::Ram => [180, 130, 90],
            Self::Other => [160, 160, 160],
        }
    }

    /// The kind's colour, the same wherever the kind is drawn. A light theme
    /// darkens every colour by one factor so a thin bar stands off the panel.
    pub fn color(self) -> Hsla {
        let [r, g, b] = self.rgb();
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

        let mut ranked = Vec::new();
        let mut fixed = Vec::new();
        for kind in DamageKind::ALL {
            let amount = by_kind[kind as usize];
            if amount == 0 {
                continue;
            }

            let segment = Segment { kind, amount };
            if DamageKind::FIXED_TAIL.contains(&kind) {
                fixed.push(segment);
            } else {
                ranked.push(segment);
            }
        }

        // Stable, so equal amounts keep the kinds' declared order.
        ranked.sort_by_key(|segment| std::cmp::Reverse(segment.amount));
        let mut segments = ranked;
        segments.extend(fixed);

        Some(Self { segments, total })
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// `amount` as a percent of this mix's total.
    pub fn percent(&self, amount: u64) -> f64 {
        amount as f64 * 100. / self.total as f64
    }

    /// Each segment's part of the bar, from 0 to 1, in drawing order.
    pub fn proportions(&self) -> impl Iterator<Item = (DamageKind, f32)> + '_ {
        self.segments.iter().map(|segment| (segment.kind, (segment.amount as f64 / self.total as f64) as f32))
    }

    /// The segments across the whole width given, in full colour under the
    /// figure: the types stay recognisable and the figure never sits on a
    /// coloured ground, so it reads the same on every panel and in every
    /// player colour.
    pub fn stacked_bar(&self) -> impl IntoElement {
        let last = self.segments.len() - 1;
        h_flex().w_full().h(BAR_HEIGHT).flex_none().overflow_hidden().children(self.proportions().enumerate().map(
            |(index, (kind, proportion))| {
                let piece = div().id(("damage-segment", index)).test_support().h_full().bg(kind.color());
                // The last takes what is left, so rounding leaves no gap at the end.
                if index == last { piece.flex_1() } else { piece.flex_none().w(relative(proportion)) }
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
            keys = keys.child(h_flex().gap_1().child(swatch(line.kind)).child(line.label));
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
    use wows_minimap_renderer::renderer::Armament;
    use wows_replay_insights::battle_report::DAMAGE_DESCRIPTIONS;
    use wows_replay_insights::battle_report::RECEIVED_DAMAGE_DESCRIPTIONS;

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
    fn a_small_kind_keeps_its_own_segment() {
        let lines = [
            line(DamageKind::Ap, 9_000),
            line(DamageKind::Ram, 200),
            line(DamageKind::Other, 100),
            line(DamageKind::Fire, 250),
            line(DamageKind::Sap, 450),
        ];

        let mix = DamageMix::of(&lines).expect("damage was dealt");

        assert_eq!(
            kinds(&mix),
            vec![
                (DamageKind::Ap, 9_000),
                (DamageKind::Sap, 450),
                (DamageKind::Ram, 200),
                (DamageKind::Fire, 250),
                (DamageKind::Other, 100),
            ]
        );
    }

    /// A minor share in one row and a major one in another, dealt and
    /// received alike, draw in the renderer's torpedo colour.
    #[test]
    fn a_kind_has_one_colour_in_every_row_and_direction() {
        let torpedo_color = |descriptions: &'static [(&'static str, &'static str)], torpedoes: u64| {
            let lines = DamageLine::from_results(descriptions, |key| match key {
                "damage_tpd_normal" => torpedoes,
                "damage_main_he" => 10_000,
                _ => 0,
            });
            let mix = DamageMix::of(&lines).expect("damage was dealt");
            mix.segments().iter().find(|segment| segment.amount == torpedoes).map(|segment| segment.kind.rgb())
        };

        let colors = [
            torpedo_color(&DAMAGE_DESCRIPTIONS, 100),
            torpedo_color(&DAMAGE_DESCRIPTIONS, 30_000),
            torpedo_color(&RECEIVED_DAMAGE_DESCRIPTIONS, 100),
            torpedo_color(&RECEIVED_DAMAGE_DESCRIPTIONS, 30_000),
        ];

        assert_eq!(colors, [Some(Armament::Torpedoes.color()); 4]);
    }

    #[test]
    fn kinds_without_an_armament_colour_stand_apart_from_the_renderer_set() {
        let armaments = [
            DamageKind::Ap,
            DamageKind::Sap,
            DamageKind::He,
            DamageKind::Secondary,
            DamageKind::Torpedo,
            DamageKind::Plane,
        ]
        .map(DamageKind::rgb);
        let others = [DamageKind::Fire, DamageKind::Flood, DamageKind::Ram, DamageKind::Other].map(DamageKind::rgb);

        for (index, color) in others.iter().enumerate() {
            assert!(!armaments.contains(color), "{color:?}");
            assert!(!others[index + 1..].contains(color), "{color:?}");
        }
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

    /// The stacked bar spans the width it is given and splits it in each
    /// type's proportion of the player's damage.
    #[gpui_kit::test]
    fn a_stack_splits_its_width_in_each_types_proportion(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::ParentElement as _;
        use gpui_kit::Styled as _;
        use gpui_kit::test::TestWindowExt as _;

        struct Bar(DamageMix);

        impl gpui_kit::Render for Bar {
            fn render(
                &mut self,
                _window: &mut gpui_kit::Window,
                _cx: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                gpui_kit::div().w(gpui_kit::px(500.)).child(self.0.stacked_bar())
            }
        }

        let lines = [line(DamageKind::Ap, 30_000), line(DamageKind::He, 10_000), line(DamageKind::Fire, 10_000)];
        let mix = DamageMix::of(&lines).expect("damage was dealt");
        cx.update(gpui_kit::init);
        let window = cx.open_window(gpui_kit::size(gpui_kit::px(600.), gpui_kit::px(100.)), |_window, _cx| Bar(mix));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let widths =
                [0usize, 1, 2].map(|index| window.find(("damage-segment", index)).bounds().size.width.as_f32());
            for (width, expected) in widths.iter().zip([300., 100., 100.]) {
                assert!((width - expected).abs() < 0.5, "{widths:?}");
            }
        })
        .expect("the window is open");
    }

    #[test]
    fn keys_of_one_weapon_count_toward_one_kind() {
        let lines =
            DamageLine::from_results(&wows_replay_insights::battle_report::DAMAGE_DESCRIPTIONS, |key| match key {
                "damage_atba_he" | "damage_atba_ap_manual" | "damage_skip_avia" | "damage_tpd_deep" => 1,
                _ => 0,
            });

        let kinds: Vec<DamageKind> = lines.iter().map(|line| line.kind).collect();
        assert_eq!(kinds, [DamageKind::Secondary, DamageKind::Secondary, DamageKind::Torpedo, DamageKind::Plane]);
    }
}

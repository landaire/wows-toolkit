//! A ship's alternative modules of each kind compared with the stock module
//! of that kind on the stock build: the values each option gives the ship and
//! a sentence per option naming every value it changes.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_ship_stats::DerivedRow;
use wows_ship_stats::DerivedStat;
use wowsunpack::game_params::ttx::StatLine;
use wowsunpack::game_params::ttx::constants::KM_TO_M;
use wowsunpack::game_params::ttx::constants::ballistic_speed;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::model::shell_qualifier;
use wowsunpack::game_params::ttx::render_stat_rows;

use crate::armament::Armament;
use crate::armament::Equipped;
use crate::armament::GunGroup;
use crate::armament::TorpedoArmament;
use crate::builds::Planner;
use crate::comparison::number;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Shell;
use crate::stats::Card;
use crate::stats::extended_horizontal_dispersion;
use crate::stats::extended_vertical_dispersion;
use crate::stats::stat_label;
use crate::torpedoes::describe_falloff;
use crate::torpedoes::falloff_points;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleComparisonArgs {
    /// Ship name, e.g. "G. Kurfurst".
    pub ship: String,

    /// Module kind to compare. Defaults to every kind the ship has more than one module of.
    pub kind: Option<ModuleKind>,

    /// Modules to compare with the stock module of their kind, e.g. ["420 mm/48 SK C/40"] or ["Hull B"]. Defaults to every module of each kind compared.
    pub modules: Option<Vec<String>>,

    /// Distance in km for AP penetration, shell flight time and dispersion, more than 0. Defaults to 12.
    pub range_km: Option<Km>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleComparisonOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: String,
    pub range_km: Km,
    pub kinds: Vec<KindComparison>,
    pub notes: [&'static str; 4],
}

/// The options of one module kind, stock first.
#[derive(Debug, Clone, Serialize)]
pub struct KindComparison {
    pub kind: ModuleKind,
    pub options: Vec<ModuleOption>,

    /// One sentence per option after the stock one, naming each value that
    /// differs from the stock option's.
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleOption {
    pub module: String,
    pub stock: bool,

    /// Modules of other kinds that are not stock and that this option is
    /// compared with, because the stock ones do not fit it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<String>,

    /// "label: value" lines: the values compared for every option of the
    /// kind, then other stat card values that differ between the options.
    /// A value the game data lacks for this option is left out.
    pub values: Vec<String>,
}

/// One labelled value of one option.
struct Row {
    label: String,
    value: String,
}

/// The values of one option: those compared for every option of the kind,
/// and every other card and derived row.
struct OptionValues {
    compared: Vec<Row>,
    other: Vec<Row>,
}

/// The rows of one option's stat card, marking those a compared value reads
/// so they are not listed again among the other values.
struct Values<'a> {
    game: &'a GameData,
    lines: Vec<StatLine>,
    derived: &'a [DerivedRow],
    lines_read: Vec<bool>,
    derived_read: Vec<bool>,
    compared: Vec<Row>,
}

impl<'a> Values<'a> {
    fn new(game: &'a GameData, card: &'a Card) -> Self {
        let lines = render_stat_rows(&card.stats.rows(), game.provider());
        let lines_read = vec![false; lines.len()];
        let derived_read = vec![false; card.derived.len()];

        Self { game, lines, derived: &card.derived, lines_read, derived_read, compared: Vec::new() }
    }

    /// The card row of `stat` with `qualifier`.
    fn card(&mut self, label: impl Into<String>, stat: TtxStat, qualifier: Option<&str>) {
        let found = self.lines.iter().position(|line| line.stat == stat && line.qualifier.as_deref() == qualifier);
        if let Some(index) = found {
            self.lines_read[index] = true;
            self.compared.push(Row { label: label.into(), value: self.lines[index].value.clone() });
        }
    }

    /// The derived row of `stat` with `qualifier`.
    fn derived(&mut self, label: impl Into<String>, stat: DerivedStat, qualifier: Option<&str>) {
        let found = self.derived.iter().position(|row| row.stat == stat && row.qualifier.as_deref() == qualifier);
        if let Some(index) = found {
            self.derived_read[index] = true;
            self.compared.push(Row { label: label.into(), value: self.derived[index].value.to_string() });
        }
    }

    /// A value computed outside the card; absent values are left out.
    fn computed(&mut self, label: impl Into<String>, value: Option<String>) {
        if let Some(value) = value {
            self.compared.push(Row { label: label.into(), value });
        }
    }

    /// Leaves every card row of `stat` out of the other values: a compared
    /// value states it another way.
    fn cover(&mut self, stat: TtxStat) {
        for (line, read) in self.lines.iter().zip(&mut self.lines_read) {
            if line.stat == stat {
                *read = true;
            }
        }
    }

    fn cover_derived(&mut self, stat: DerivedStat) {
        for (row, read) in self.derived.iter().zip(&mut self.derived_read) {
            if row.stat == stat {
                *read = true;
            }
        }
    }

    /// The compared values, and every row no compared value read. Torpedo
    /// rows name the torpedo by position, so they line up with another
    /// torpedo module's rows.
    fn finish(self, torpedoes: &[String]) -> OptionValues {
        let game = self.game;

        let mut other: Vec<Row> = self
            .lines
            .iter()
            .zip(&self.lines_read)
            .filter(|(_, read)| !**read)
            .map(|(line, _)| {
                let qualifier = torpedo_position(line.qualifier.as_deref(), torpedoes);
                Row { label: stat_label(&line.label, qualifier.as_deref(), game), value: line.value.clone() }
            })
            .collect();
        other.extend(self.derived.iter().zip(&self.derived_read).filter(|(_, read)| !**read).map(|(row, _)| {
            let qualifier = torpedo_position(row.qualifier.as_deref(), torpedoes);
            Row { label: stat_label(short_label(row.stat), qualifier.as_deref(), game), value: row.value.to_string() }
        }));

        OptionValues { compared: self.compared, other }
    }
}

/// One option before its values are laid out against the others.
struct Compared {
    module: Module,
    with: Vec<String>,
    values: OptionValues,
}

impl GameData {
    /// Each module kind of `args.ship` with alternatives: every option's
    /// values on the stock build and how each differs from the stock option.
    pub fn module_comparison(&self, args: &ModuleComparisonArgs) -> Result<ModuleComparisonOutput, QueryError> {
        // The range battle_impact, dispersion_at_range and gun_module_matchups use.
        const DEFAULT_RANGE: Km = Km::new(12.0);
        const NOTES: [&str; 4] = [
            "Every option is on the stock build: the ship's other modules stock unless the option needs others (listed in with), no captain skills, upgrades or consumables.",
            "Damage, fires and torpedoes per minute assume every shell or torpedo hits, on the reload with no consumable, against one target at the bearing where the most guns or launchers bear; values for every mount or launcher firing at once are a ceiling no single target sees. AA damage per second is before hit chance.",
            "AP raw penetration, flight time and impact velocity at range_km come from each shell's ballistic solution: penetration against a plate struck head-on. Dispersion at range_km is computed even beyond the option's maximum range.",
            "Torpedo reaction time is detectability / speed, with speed in knots x 2.69 m/s per knot: the time a target has from spotting the torpedo until it arrives.",
        ];

        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let range = args.range_km.unwrap_or(DEFAULT_RANGE);
        if !range.value().is_finite() || range.value() <= 0.0 {
            return Err(QueryError::InvalidArguments { reason: "range_km must be more than 0" });
        }

        let modules = self.ship_modules(ship);
        let requested = match args.modules.as_deref().filter(|queries| !queries.is_empty()) {
            Some(queries) => {
                let mut chosen: Vec<Module> = Vec::new();
                for query in queries {
                    for module in self.resolve_modules(ship, std::slice::from_ref(query))?.chosen {
                        if let Some(kind) = args.kind
                            && module.kind != kind
                        {
                            return Err(QueryError::ModuleKindMismatch {
                                ship: ship.name.clone(),
                                module: module.name,
                                kind: module.kind.label(),
                                requested: kind.label(),
                            });
                        }
                        if !chosen.contains(&module) {
                            chosen.push(module);
                        }
                    }
                }
                Some(chosen)
            }
            None => None,
        };

        let kinds: Vec<ModuleKind> = ModuleKind::ALL
            .into_iter()
            .filter(|kind| args.kind.is_none_or(|requested| requested == *kind))
            .filter(|kind| match &requested {
                Some(chosen) => chosen.iter().any(|module| module.kind == *kind && !module.stock),
                None => modules.iter().filter(|module| module.kind == *kind).count() > 1,
            })
            .collect();
        if kinds.is_empty() {
            return Err(match (&requested, args.kind) {
                (Some(chosen), _) => QueryError::StockModulesOnly {
                    ship: ship.name.clone(),
                    modules: chosen.iter().map(|module| module.name.clone()).collect(),
                },
                (None, Some(kind)) => QueryError::NoAlternativeModule {
                    ship: ship.name.clone(),
                    kind: kind.label(),
                    modules: modules
                        .iter()
                        .filter(|module| module.kind == kind)
                        .map(|module| module.name.clone())
                        .collect(),
                },
                (None, None) => QueryError::NoAlternativeModules { ship: ship.name.clone() },
            });
        }

        let planner = Planner::new(self, ship);
        let alternatives = requested.as_deref().unwrap_or(&modules);
        let mut comparisons = Vec::with_capacity(kinds.len());
        for kind in kinds {
            // The stock module leads; options named in `modules` follow in request order.
            let options = modules
                .iter()
                .filter(|module| module.kind == kind && module.stock)
                .chain(alternatives.iter().filter(|module| module.kind == kind && !module.stock));

            let mut compared = Vec::new();
            for module in options {
                compared.push(self.compared_option(&planner, module, range)?);
            }
            comparisons.push(lay_out(kind, &compared));
        }

        Ok(ModuleComparisonOutput {
            interpretations,
            ship: ship.label(),
            range_km: range,
            kinds: comparisons,
            notes: NOTES,
        })
    }

    /// `module` on the module combination that mounts it with the fewest
    /// other modules that are not stock.
    fn compared_option(&self, planner: &Planner<'_>, module: &Module, range: Km) -> Result<Compared, QueryError> {
        let ship = planner.ship();
        let Some(index) = planner
            .selections()
            .enumerate()
            .filter(|(_, selection)| module.kind.mounted(selection) == Some(module.param.as_str()))
            .min_by_key(|(index, _)| planner.selection_modules(*index).len())
            .map(|(index, _)| index)
        else {
            return Err(QueryError::ModuleFitsNoHull { ship: ship.name.clone(), module: module.name.clone() });
        };
        let selection = planner.selection(index);
        let with: Vec<String> =
            planner.selection_modules(index).iter().filter(|name| **name != module.name).cloned().collect();

        let card = self.card_with(ship, &self.stock_loadout(), selection)?;
        let armament = self.armament(ship, selection)?;
        let mut values = Values::new(self, &card);
        match module.kind {
            ModuleKind::Guns => self.gun_option_values(&mut values, planner, index, &card, &armament, range)?,
            ModuleKind::Torpedoes => self.torpedo_option_values(&mut values, &card, &armament)?,
            ModuleKind::Hull => hull_values(&mut values, &armament),
            ModuleKind::Engine => values.card("maximum speed", TtxStat::Speed, None),
            ModuleKind::FireControl => {
                values.card("maximum range", TtxStat::ArtilleryRange, None);
                dispersion_values(&mut values, planner, index, range)?;
            }
        }

        let torpedoes: Vec<String> = card
            .stats
            .torpedoes
            .iter()
            .flat_map(|torpedoes| torpedoes.torpedoes.iter().map(|torpedo| torpedo.name.clone()))
            .collect();

        Ok(Compared { module: module.clone(), with, values: values.finish(&torpedoes) })
    }

    fn gun_option_values(
        &self,
        values: &mut Values<'_>,
        planner: &Planner<'_>,
        index: usize,
        card: &Card,
        armament: &Armament,
        range: Km,
    ) -> Result<(), QueryError> {
        let ship = planner.ship();
        let selection = planner.selection(index);
        let km = number(range.value());

        values.card("caliber", TtxStat::GunCaliber, None);
        values.computed("mounts x barrels", main_layout(armament));
        values.derived("barrels", DerivedStat::MainBatteryBarrels, None);
        values.cover(TtxStat::GunNumBarrels);
        values.cover(TtxStat::GunNumGuns);
        values.card("reload", TtxStat::ArtilleryReloadTime, None);
        values.card("180 deg turret turn time", TtxStat::GunRotationTime, None);
        values.cover(TtxStat::GunRotationSpeed);
        values.card("maximum range", TtxStat::ArtilleryRange, None);

        let shells: Vec<Shell> = match selection.artillery.as_deref() {
            Some(param) => self.gun_module_shells(ship, param),
            None => Vec::new(),
        };
        for card_shell in card.stats.artillery.iter().flat_map(|artillery| &artillery.shells) {
            let shell = shells.iter().find(|shell| shell.param == card_shell.name);
            let ammo = shell.map(|shell| shell.ammo);
            let qualifier = shell_qualifier(card_shell);
            let label = ammo.map_or_else(|| qualifier.clone(), |ammo| ammo.label().to_owned());
            let qualifier = Some(qualifier.as_str());

            values.card(format!("{label} alpha damage"), TtxStat::ShellDamage, qualifier);
            values.derived(format!("{label} salvo damage"), DerivedStat::MainBatterySalvoDamage, qualifier);
            values.derived(format!("{label} damage per minute"), DerivedStat::MainBatteryDamagePerMinute, qualifier);
            if ammo == Some(Ammo::He) {
                values.card(format!("{label} fire chance"), TtxStat::ShellBurnChance, qualifier);
                values.derived(
                    format!("{label} fires per minute against one target"),
                    DerivedStat::FiresPerMinuteMainBattery,
                    qualifier,
                );
            }
            if ammo != Some(Ammo::Ap) {
                values.card(format!("{label} penetration"), TtxStat::ShellPenetration, qualifier);
            }
            values.card(format!("{label} muzzle velocity"), TtxStat::ShellSpeed, qualifier);
            values.cover(TtxStat::ShellCaliber);

            let impact = self.shell_impact(&card_shell.name, range)?;
            if ammo == Some(Ammo::Ap) {
                values.computed(
                    format!("AP raw penetration at {km} km"),
                    impact.as_ref().map(|impact| format!("{} mm", round_to(impact.raw_penetration.value(), 10.0))),
                );
            }
            values.computed(
                format!("{label} flight time to {km} km"),
                impact.as_ref().map(|impact| format!("{} s", number(impact.time_to_target.value()))),
            );
            values.computed(
                format!("{label} impact velocity at {km} km"),
                impact.as_ref().map(|impact| format!("{} m/s", impact.impact_velocity.value().round())),
            );
            if let Some(shell) = shell {
                values.computed(
                    format!("{label} overmatches plates up to"),
                    shell.overmatches_up_to_mm.map(|mm| format!("{} mm", number(mm.value()))),
                );
                values.computed(
                    format!("{label} fuse threshold"),
                    shell.fuse_threshold_mm.map(|mm| format!("{} mm", number(mm.value()))),
                );
                values.computed(
                    format!("{label} fuse time"),
                    shell.fuse_time_s.map(|seconds| format!("{} s", round_to(seconds.value(), 1000.0))),
                );
            }
        }

        dispersion_values(values, planner, index, range)
    }

    fn torpedo_option_values(
        &self,
        values: &mut Values<'_>,
        card: &Card,
        armament: &Armament,
    ) -> Result<(), QueryError> {
        values.computed("launchers x tubes", launcher_layout(&armament.torpedoes));
        values.derived("tubes", DerivedStat::TorpedoTubes, None);
        values.cover(TtxStat::LauncherNumBarrels);
        values.card("reload", TtxStat::TorpedoReloadTime, None);

        let torpedoes = card.stats.torpedoes.iter().flat_map(|torpedoes| &torpedoes.torpedoes).collect::<Vec<_>>();
        for (index, torpedo) in torpedoes.iter().enumerate() {
            let name = self.projectile_name(&torpedo.name);
            let qualifier = Some(torpedo.name.as_str());
            // Values name a torpedo by position, so they line up with another
            // torpedo module's values; a module with one torpedo needs no position.
            let prefix = if torpedoes.len() == 1 {
                values.computed("torpedo", Some(name));
                String::new()
            } else {
                let position = format!("torpedo {}", index + 1);
                values.computed(position.clone(), Some(name));
                format!("{position} ")
            };

            values.card(format!("{prefix}damage"), TtxStat::TorpedoDamage, qualifier);
            values.card(format!("{prefix}range"), TtxStat::TorpedoRange, qualifier);
            values.card(format!("{prefix}speed"), TtxStat::TorpedoSpeed, qualifier);
            values.card(format!("{prefix}detectability"), TtxStat::TorpedoVisibility, qualifier);
            let reaction = torpedo.visibility.zip(torpedo.speed).map(|(visibility, speed)| {
                format!("{} s", number(visibility.value() * KM_TO_M / ballistic_speed(speed).value()))
            });
            values.computed(format!("{prefix}reaction time"), reaction);
            let falloff = match torpedo.speed {
                Some(speed) => falloff_points(&self.projectile(&torpedo.name)?, speed),
                None => None,
            };
            if let Some(points) = falloff {
                values.computed(format!("{prefix}damage by distance"), Some(describe_falloff(&points)));
                values.cover(TtxStat::TorpedoDistanceOfMaxDamage);
            }
            values.derived(
                format!("{prefix}damage per minute against one target"),
                DerivedStat::TorpedoDamagePerMinute,
                qualifier,
            );
            values.derived(
                format!("{prefix}damage per minute, every launcher firing at once"),
                DerivedStat::TorpedoDamagePerMinuteAllLaunchers,
                qualifier,
            );
        }
        values.derived("torpedoes per minute against one target", DerivedStat::TorpedoesPerMinute, None);
        values.derived(
            "torpedoes per minute, every launcher firing at once",
            DerivedStat::TorpedoesPerMinuteAllLaunchers,
            None,
        );

        Ok(())
    }
}

fn hull_values(values: &mut Values<'_>, armament: &Armament) {
    values.card("hit points", TtxStat::Health, None);
    values.card("maximum speed", TtxStat::Speed, None);
    values.card("turning circle radius", TtxStat::TurningRadius, None);
    values.card("rudder shift time", TtxStat::RudderTime, None);
    values.card("surface detectability", TtxStat::SeaDetection, None);
    values.card("air detectability", TtxStat::AirDetection, None);

    values.computed("main battery", Some(gun_layout(&armament.main_battery)));
    values.computed("secondary battery", Some(gun_layout(&armament.secondary_battery)));
    values
        .computed("torpedo launchers", Some(launcher_layout(&armament.torpedoes).unwrap_or_else(|| "none".to_owned())));

    for stat in [
        TtxStat::GunNumGuns,
        TtxStat::GunNumBarrels,
        TtxStat::SecondaryGunNumGuns,
        TtxStat::SecondaryGunNumBarrels,
        TtxStat::LauncherNumBarrels,
    ] {
        values.cover(stat);
    }
    values.cover_derived(DerivedStat::MainBatteryBarrels);
    values.cover_derived(DerivedStat::TorpedoTubes);
}

/// Main battery dispersion at `range` and sigma of module combination `index`.
fn dispersion_values(
    values: &mut Values<'_>,
    planner: &Planner<'_>,
    index: usize,
    range: Km,
) -> Result<(), QueryError> {
    let outcome = planner.bare(index)?;
    let km = number(range.value());
    let meters = |value: Option<Meters>| value.map(|meters| format!("{} m", number(meters.value())));

    values.computed(
        format!("horizontal dispersion at {km} km"),
        meters(extended_horizontal_dispersion(planner, &outcome, range)),
    );
    values.computed(
        format!("vertical dispersion at {km} km"),
        meters(extended_vertical_dispersion(planner, &outcome, range)),
    );
    values.derived("sigma", DerivedStat::MainBatterySigma, None);
    values.cover_derived(DerivedStat::MainBatteryDispersionAtStockRange);
    values.cover_derived(DerivedStat::MainBatteryVerticalDispersionAtStockRange);

    Ok(())
}

/// Launchers x tubes per launcher, e.g. "3 x 5", groups joined by " + ".
fn launcher_layout(torpedoes: &Equipped<TorpedoArmament>) -> Option<String> {
    let Equipped::Some(torpedoes) = torpedoes else {
        return None;
    };

    Some(torpedoes.launchers.iter().map(|group| group.layout.clone()).collect::<Vec<_>>().join(" + "))
}

/// "4 x 3 406 mm", groups joined by " + ", or "none".
fn gun_layout(groups: &Equipped<Vec<GunGroup>>) -> String {
    match groups {
        Equipped::Some(groups) => groups.iter().map(|group| group.layout.clone()).collect::<Vec<_>>().join(" + "),
        Equipped::None => "none".to_owned(),
    }
}

/// Main battery mounts x barrels per mount, e.g. "4 x 3".
fn main_layout(armament: &Armament) -> Option<String> {
    let Equipped::Some(groups) = &armament.main_battery else {
        return None;
    };

    Some(
        groups
            .iter()
            .map(|group| format!("{} x {}", group.mounts.0, group.barrels_per_mount.0))
            .collect::<Vec<_>>()
            .join(" + "),
    )
}

/// The options of `kind` with their values side by side: the compared values
/// of every option, then other values that differ from the stock option's.
fn lay_out(kind: ModuleKind, compared: &[Compared]) -> KindComparison {
    let stock = &compared[0];
    let mut labels = first_seen_labels(compared.iter().map(|option| option.values.compared.as_slice()));
    let other_labels: Vec<String> = first_seen_labels(compared.iter().map(|option| option.values.other.as_slice()))
        .into_iter()
        .filter(|label| !labels.contains(label))
        .filter(|label| {
            let stock_value = value_of(&stock.values.other, label);
            compared[1..].iter().any(|option| value_of(&option.values.other, label) != stock_value)
        })
        .collect();
    labels.extend(other_labels);

    let tables: Vec<Vec<Option<&str>>> = compared
        .iter()
        .map(|option| {
            labels
                .iter()
                .map(|label| value_of(&option.values.compared, label).or_else(|| value_of(&option.values.other, label)))
                .collect()
        })
        .collect();

    let differences = (1..compared.len())
        .map(|index| difference(&compared[index], &labels, &tables[index], &stock.module, &tables[0]))
        .collect();
    let options = compared
        .iter()
        .zip(&tables)
        .map(|(option, cells)| ModuleOption {
            module: option.module.name.clone(),
            stock: option.module.stock,
            with: option.with.clone(),
            values: labels
                .iter()
                .zip(cells)
                .filter_map(|(label, value)| value.map(|value| format!("{label}: {value}")))
                .collect(),
        })
        .collect();

    KindComparison { kind, options, differences }
}

/// "420 mm/48 SK C/40: caliber 420 mm instead of 406 mm; reload 32.0 s
/// instead of 29.0 s."
fn difference(
    option: &Compared,
    labels: &[String],
    cells: &[Option<&str>],
    stock: &Module,
    stock_cells: &[Option<&str>],
) -> String {
    let changes: Vec<String> = labels
        .iter()
        .zip(cells.iter().zip(stock_cells))
        .filter_map(|(label, cell)| match cell {
            (Some(value), Some(stock_value)) if value != stock_value => {
                Some(format!("{label} {value} instead of {stock_value}"))
            }
            (Some(value), None) => Some(format!("{label} {value}, which {} has no value for", stock.name)),
            (None, Some(stock_value)) => Some(format!("no {label} value instead of {stock_value}")),
            _ => None,
        })
        .collect();
    let with = if option.with.is_empty() { String::new() } else { format!(" (with {})", option.with.join(", ")) };

    if changes.is_empty() {
        format!("{}{with}: every compared value equals the stock {}'s.", option.module.name, stock.name)
    } else {
        format!("{}{with}: {}.", option.module.name, changes.join("; "))
    }
}

/// Labels of every list in first-seen order.
fn first_seen_labels<'a>(lists: impl Iterator<Item = &'a [Row]>) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for row in lists.flatten() {
        if !labels.contains(&row.label) {
            labels.push(row.label.clone());
        }
    }

    labels
}

fn value_of<'a>(rows: &'a [Row], label: &str) -> Option<&'a str> {
    rows.iter().find(|row| row.label == label).map(|row| row.value.as_str())
}

/// `qualifier` with a torpedo's name replaced by its position among the
/// module's torpedoes, or dropped for a module's only torpedo.
fn torpedo_position(qualifier: Option<&str>, torpedoes: &[String]) -> Option<String> {
    let qualifier = qualifier?;

    match torpedoes.iter().position(|torpedo| torpedo == qualifier) {
        Some(_) if torpedoes.len() == 1 => None,
        Some(index) => Some(format!("torpedo {}", index + 1)),
        None => Some(qualifier.to_owned()),
    }
}

/// A short label for a derived rate whose assumptions the notes state, else
/// the full label.
fn short_label(stat: DerivedStat) -> &'static str {
    match stat {
        DerivedStat::MainBatterySalvoDamage => "main battery salvo damage",
        DerivedStat::MainBatteryDamagePerMinute => "main battery damage per minute",
        DerivedStat::FiresPerMinuteMainBattery => "main battery fires per minute against one target",
        DerivedStat::FiresPerMinuteSecondary => "secondary fires per minute against one target",
        DerivedStat::FiresPerMinuteSecondaryGroup => "secondary group fires per minute against one target",
        DerivedStat::FiresPerMinuteSecondaryAllMounts => "secondary fires per minute, every mount firing at once",
        DerivedStat::FiresPerMinuteTotal => "fires per minute against one target, all armament",
        DerivedStat::FiresPerMinuteTotalAllMounts => "fires per minute, every gun mount firing at once, all armament",
        DerivedStat::TorpedoesPerMinute => "torpedoes per minute against one target",
        DerivedStat::TorpedoesPerMinuteAllLaunchers => "torpedoes per minute, every launcher firing at once",
        DerivedStat::TorpedoDamagePerMinute => "torpedo damage per minute against one target",
        DerivedStat::TorpedoDamagePerMinuteAllLaunchers => "torpedo damage per minute, every launcher firing at once",
        DerivedStat::AaShortDps => "short-range AA damage per second",
        DerivedStat::AaMediumDps => "medium-range AA damage per second",
        DerivedStat::AaLongDps => "long-range AA damage per second",
        DerivedStat::AaRating => "AA rating (a judgment to compare ships, not a game value)",
        _ => stat.label(),
    }
}

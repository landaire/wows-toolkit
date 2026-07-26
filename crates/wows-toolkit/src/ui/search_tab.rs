//! The dockable Search tab: a chip-based `Query` builder over a results table
//! backed by the replay index.

use std::collections::HashMap;

use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;
use wowsunpack::game_params::types::Species;

use crate::app::ToolkitTabViewer;
use crate::armor_viewer::ship_selector::SHIP_SPECIES;
use crate::armor_viewer::ship_selector::ShipCatalog;
use crate::armor_viewer::ship_selector::species_name;
use crate::armor_viewer::ship_selector::tier_roman;
use crate::db::index::query;
use crate::db::index::query_model::Chip;
use crate::db::index::query_model::Connector;
use crate::db::index::query_model::Field;
use crate::db::index::query_model::Group;
use crate::db::index::query_model::Op;
use crate::db::index::query_model::Query;
use crate::db::index::query_model::StatKind;
use crate::db::index::query_model::Subject;
use crate::db::index::query_model::Value;
use crate::db::index::query_model::ValueKind;
use crate::db::index::rows::IndexSource;
use crate::db::index::rows::MatchHit;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::PlayerFacet;
use crate::db::index::rows::SourceId;

/// Max width of a single group's body in the horizontally-scrolling group
/// row, so its chip `horizontal_wrapped` has a finite wrap point instead of
/// inheriting the enclosing `ScrollArea::horizontal`'s infinite width.
const GROUP_MAX_WIDTH: f32 = 340.0;

/// Non-stat fields offered by the "Add filter" picker, in display order. The
/// seven `StatKind`s are appended to this list in the picker (see `StatKind::ALL`).
const NON_STAT_FIELDS: &[Field] = &[
    Field::Outcome,
    Field::Map,
    Field::Mode,
    Field::SelfShip,
    Field::Class,
    Field::Tier,
    Field::Date,
    Field::PlayerPresent,
    Field::EnemyShip,
    Field::AllyShip,
    Field::Group,
];

fn field_display_label(field: Field) -> String {
    match field {
        Field::Outcome => t!("ui.search.field.outcome").into(),
        Field::Map => t!("ui.search.field.map").into(),
        Field::Mode => t!("ui.search.field.mode").into(),
        Field::SelfShip => t!("ui.search.field.self_ship").into(),
        Field::Class => t!("ui.search.field.class").into(),
        Field::Tier => t!("ui.search.field.tier").into(),
        Field::Date => t!("ui.search.field.date").into(),
        Field::PlayerPresent => t!("ui.search.field.player_present").into(),
        Field::EnemyShip => t!("ui.search.field.enemy_ship").into(),
        Field::AllyShip => t!("ui.search.field.ally_ship").into(),
        Field::Group => t!("ui.search.field.group").into(),
        Field::Stat { kind, .. } => stat_kind_label(kind),
    }
}

fn stat_kind_label(kind: StatKind) -> String {
    match kind {
        StatKind::Damage => t!("ui.search.field.stat_damage"),
        StatKind::Kills => t!("ui.search.field.stat_kills"),
        StatKind::Spotting => t!("ui.search.field.stat_spotting"),
        StatKind::Potential => t!("ui.search.field.stat_potential"),
        StatKind::Received => t!("ui.search.field.stat_received"),
        StatKind::Pr => t!("ui.search.field.stat_pr"),
        StatKind::Survived => t!("ui.search.field.stat_survived"),
    }
    .into()
}

/// Selector label for a `Subject`: "Me" / "Any player" / the resolved player's
/// name (or "#<id>" if unresolved). `picking` overrides with the "Specific
/// player..." prompt while the user has opened the picker but not chosen yet.
fn subject_combo_label(subject: Subject, picking: bool, resolved_players: &HashMap<AccountId, String>) -> String {
    if picking {
        return t!("ui.search.subject_specific").into();
    }
    match subject {
        Subject::SelfPlayer => t!("ui.search.subject_me").into(),
        Subject::AnyPlayer => t!("ui.search.subject_any").into(),
        Subject::Player(id) => resolved_players.get(&id).cloned().unwrap_or_else(|| format!("#{}", id.raw())),
    }
}

fn op_label(op: Op) -> String {
    match op {
        Op::Contains => t!("ui.search.op.contains"),
        Op::Equals => t!("ui.search.op.equals"),
        Op::NotEquals => t!("ui.search.op.not_equals"),
        Op::Eq => t!("ui.search.op.eq"),
        Op::Ne => t!("ui.search.op.ne"),
        Op::Gt => t!("ui.search.op.gt"),
        Op::Ge => t!("ui.search.op.ge"),
        Op::Lt => t!("ui.search.op.lt"),
        Op::Le => t!("ui.search.op.le"),
        Op::Is => t!("ui.search.op.is"),
        Op::IsNot => t!("ui.search.op.is_not"),
        Op::Present => t!("ui.search.op.present"),
        Op::NotPresent => t!("ui.search.op.not_present"),
    }
    .into()
}

fn outcome_label(o: MatchOutcome) -> String {
    match o {
        MatchOutcome::Win => t!("ui.search.outcome_win"),
        MatchOutcome::Loss => t!("ui.search.outcome_loss"),
        MatchOutcome::Draw => t!("ui.search.outcome_draw"),
        MatchOutcome::Unknown => t!("ui.search.outcome_unknown"),
    }
    .into()
}

fn bool_label(b: bool) -> String {
    if b { t!("ui.search.bool_true") } else { t!("ui.search.bool_false") }.into()
}

/// Compact display text for a chip's value. Ship/account ids are resolved
/// through the search tab's name caches so pills don't just show a bare id.
fn value_label(
    value: &Value,
    resolved_ships: &HashMap<GameParamId, String>,
    resolved_players: &HashMap<AccountId, String>,
    sources: &[IndexSource],
) -> String {
    match value {
        Value::Text(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Outcome(o) => outcome_label(*o),
        Value::Class(s) => s.clone(),
        Value::Bool(b) => bool_label(*b),
        Value::Ship(id) => resolved_ships.get(id).cloned().unwrap_or_else(|| format!("#{}", id.raw())),
        Value::Account(a) => resolved_players.get(a).cloned().unwrap_or_else(|| format!("#{}", a.raw())),
        Value::Timestamp(t) => t.strftime("%Y-%m-%d").to_string(),
        Value::Source(s) => {
            sources.iter().find(|src| src.id == *s).map(|src| src.name.clone()).unwrap_or_else(|| format!("#{}", s.0))
        }
    }
}

/// Full pill text for one chip. Non-`Stat` fields keep the plain
/// "<field> <op> <value>" form; `Stat` fields are phrased with their subject:
/// "My <stat> <op> <value>", "Any player <stat> <op> <value>", or
/// "<name>'s <stat> <op> <value>". Numeric stat values use `separate_number`
/// (thousands separator) to match the results table.
fn chip_pill_label(
    chip: &Chip,
    resolved_ships: &HashMap<GameParamId, String>,
    resolved_players: &HashMap<AccountId, String>,
    sources: &[IndexSource],
    locale: Option<&str>,
) -> String {
    let Field::Stat { kind, subject } = chip.field else {
        return format!(
            "{} {} {}",
            field_display_label(chip.field),
            op_label(chip.op),
            value_label(&chip.value, resolved_ships, resolved_players, sources)
        );
    };
    let stat = stat_kind_label(kind);
    let op = op_label(chip.op);
    let value = match &chip.value {
        Value::Int(n) => crate::util::formatting::separate_number(*n, locale),
        Value::Bool(b) => bool_label(*b),
        other => value_label(other, resolved_ships, resolved_players, sources),
    };
    match subject {
        Subject::SelfPlayer => format!("{} {stat} {op} {value}", t!("ui.search.pill_my")),
        Subject::AnyPlayer => format!("{} {stat} {op} {value}", t!("ui.search.pill_any_player")),
        Subject::Player(id) => {
            let name = resolved_players.get(&id).cloned().unwrap_or_else(|| format!("#{}", id.raw()));
            format!("{name}{} {stat} {op} {value}", t!("ui.search.pill_possessive"))
        }
    }
}

/// Draft state for the "Add filter" picker within one group. Detached from the
/// query itself; only committed into a `Chip` when the user clicks Add.
#[derive(Clone)]
struct AddFilterDraft {
    field: Field,
    op: Op,
    text: String,
    int_val: i64,
    outcome: MatchOutcome,
    bool_val: bool,
    species: Species,
    ship_search: String,
    ship_id: Option<GameParamId>,
    ship_label: String,
    player_search: String,
    player_id: Option<AccountId>,
    player_label: String,
    player_results: Vec<PlayerFacet>,
    source_id: Option<SourceId>,
    date: jiff::civil::Date,
    /// Subject for a `Field::Stat` chip; irrelevant (but always valid, never a
    /// sentinel) for non-stat fields. Persists across stat-kind changes.
    subject: Subject,
    /// True while the "Specific player..." subject picker is open but no
    /// player has been chosen yet, so `subject` itself stays at its last
    /// valid value until a pick commits.
    subject_picking_player: bool,
    subject_player_search: String,
    subject_player_results: Vec<PlayerFacet>,
}

impl Default for AddFilterDraft {
    fn default() -> Self {
        let today = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
        let field = Field::Outcome;
        Self {
            field,
            op: field.allowed_ops()[0],
            text: String::new(),
            int_val: 0,
            outcome: MatchOutcome::Win,
            bool_val: true,
            species: Species::Destroyer,
            ship_search: String::new(),
            ship_id: None,
            ship_label: String::new(),
            player_search: String::new(),
            player_id: None,
            player_label: String::new(),
            player_results: Vec::new(),
            source_id: None,
            date: today,
            subject: Subject::SelfPlayer,
            subject_picking_player: false,
            subject_player_search: String::new(),
            subject_player_results: Vec::new(),
        }
    }
}

impl AddFilterDraft {
    /// Reset the op (and implicitly, the value editor) when the field changes,
    /// so a stale op/value from a different `ValueKind` cannot leak through.
    fn reset_for_field(&mut self, field: Field) {
        self.field = field;
        self.op = field.allowed_ops()[0];
    }

    fn to_value(&self) -> Option<Value> {
        match self.field.value_kind() {
            ValueKind::Text => (!self.text.is_empty()).then(|| Value::Text(self.text.clone())),
            ValueKind::Int => Some(Value::Int(self.int_val)),
            ValueKind::Outcome => Some(Value::Outcome(self.outcome)),
            ValueKind::Class => Some(Value::Class(format!("{:?}", self.species))),
            ValueKind::Bool => Some(Value::Bool(self.bool_val)),
            ValueKind::Ship => self.ship_id.map(Value::Ship),
            ValueKind::Account => self.player_id.map(Value::Account),
            ValueKind::Timestamp => match self.date.to_zoned(jiff::tz::TimeZone::UTC) {
                Ok(zoned) => Some(Value::Timestamp(zoned.timestamp())),
                Err(e) => {
                    tracing::warn!("search: date-to-timestamp conversion failed for {:?}: {e}", self.date);
                    None
                }
            },
            ValueKind::Source => self.source_id.map(Value::Source),
        }
    }

    /// Friendly label for the value being built (only known for pickers), used
    /// to seed the chip name caches so the pill doesn't show a bare id.
    fn value_display_label(&self) -> Option<String> {
        match self.field.value_kind() {
            ValueKind::Ship if self.ship_id.is_some() => Some(self.ship_label.clone()),
            ValueKind::Account if self.player_id.is_some() => Some(self.player_label.clone()),
            _ => None,
        }
    }
}

pub struct SearchTabState {
    pub query: Query,
    pub results: Vec<MatchHit>,
    /// True when `query` changed and results must be re-queried.
    pub dirty: bool,
    /// Per-group "add filter" draft; length kept in sync with `query.groups`.
    add_drafts: Vec<AddFilterDraft>,
    /// Per-group: whether the "add filter" draft row is expanded. Rendered
    /// inline (not in a popover/menu) so the draft's own `ComboBox` popups
    /// don't register as an outside click and dismiss it.
    add_draft_open: Vec<bool>,
    /// Cached replay groups, used by the Group/Source value editor.
    sources: Vec<IndexSource>,
    /// Friendly names for ship/account ids picked via the pickers, so chip
    /// pills show a name instead of a bare numeric id.
    resolved_ships: HashMap<GameParamId, String>,
    resolved_players: HashMap<AccountId, String>,
}

impl Default for SearchTabState {
    fn default() -> Self {
        // One group with its add-filter draft open, so the user can pick a
        // field/op/value immediately instead of adding a group then a filter.
        Self {
            query: Query { groups: vec![Group::default()], connector: Connector::And },
            results: Vec::new(),
            dirty: true,
            add_drafts: vec![AddFilterDraft::default()],
            add_draft_open: vec![true],
            sources: Vec::new(),
            resolved_ships: HashMap::new(),
            resolved_players: HashMap::new(),
        }
    }
}

impl SearchTabState {
    /// Replace the default single-open-group state with a query restored from
    /// persisted settings. Callers must only pass a non-empty query (an empty
    /// one should instead leave the default state, so a fresh install and a
    /// "cleared filters" restart behave the same). Draft vectors are rebuilt
    /// to match the restored groups, all closed since the restored groups
    /// already carry their filters; `dirty` is set so the tab re-queries.
    pub(crate) fn restore_query(&mut self, query: Query) {
        let group_count = query.groups.len();
        self.query = query;
        self.add_drafts = vec![AddFilterDraft::default(); group_count];
        self.add_draft_open = vec![false; group_count];
        self.dirty = true;
    }
}

/// Resolve any `Value::Account`/`Value::Ship` chip ids in `query` that are not already
/// in the name caches, by DB lookup. Chips seeded from the command palette or the
/// player tracker carry only an id, so without this the chip pill falls back to
/// `#<id>` forever. Errors are logged and swallowed; unknown ids are left unresolved
/// so the `#<id>` fallback still applies to them.
fn resolve_seeded_names(
    query: &Query,
    resolved_ships: &mut HashMap<GameParamId, String>,
    resolved_players: &mut HashMap<AccountId, String>,
    pool: &sqlx::SqlitePool,
    rt: &tokio::runtime::Runtime,
) {
    for group in &query.groups {
        for chip in &group.chips {
            match &chip.value {
                Value::Account(id) if !resolved_players.contains_key(id) => {
                    match rt.block_on(crate::db::index::query::player_name(pool, *id)) {
                        Ok(Some(name)) => {
                            resolved_players.insert(*id, name);
                        }
                        Ok(None) => {}
                        Err(e) => tracing::warn!("search: player_name lookup failed for {id:?}: {e}"),
                    }
                }
                Value::Ship(id) if !resolved_ships.contains_key(id) => {
                    match rt.block_on(crate::db::index::query::ship_name(pool, *id)) {
                        Ok(Some(name)) => {
                            resolved_ships.insert(*id, name);
                        }
                        Ok(None) => {}
                        Err(e) => tracing::warn!("search: ship_name lookup failed for {id:?}: {e}"),
                    }
                }
                _ => {}
            }
        }
    }
}

impl ToolkitTabViewer<'_> {
    /// Resolve a display name for the match's self ship, using the game data for the
    /// match's own build when it is loaded. Falls back to a bracketed id when no
    /// matching build is loaded or the ship id is unknown.
    fn search_ship_display_name(&self, hit: &MatchHit) -> Option<String> {
        let ship_id = hit.self_ship_id?;
        let data = hit.version_build.and_then(|build| self.tab_state.wows_data_map.as_ref()?.get(build));
        let guard = data.as_ref().map(|d| d.read());
        let provider = guard.as_ref().and_then(|g| g.game_metadata.as_deref());
        Some(crate::data::session_stats::resolve_ship_name(ship_id, provider))
    }

    pub fn build_search_tab(&mut self, ui: &mut egui::Ui) {
        let pool = self.tab_state.db_pool.clone();
        let rt = self.tab_state.tokio_runtime.clone();

        if let Some(q) = self.tab_state.pending_search_query.take() {
            self.tab_state.search_tab.query = q;
            self.tab_state.search_tab.dirty = true;
            if let (Some(pool), Some(rt)) = (pool.as_ref(), rt.as_ref()) {
                let search_tab = &mut self.tab_state.search_tab;
                resolve_seeded_names(
                    &search_tab.query,
                    &mut search_tab.resolved_ships,
                    &mut search_tab.resolved_players,
                    pool,
                    rt,
                );
            }
        }

        // Lazily build the ship catalog (used by the Ship value editor), the
        // same way the command palette does.
        if self.tab_state.ship_catalog.is_none()
            && let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref()
        {
            let wd = wows_data.read();
            if let Some(metadata) = wd.game_metadata.as_ref() {
                self.tab_state.ship_catalog = Some(ShipCatalog::build(metadata));
            }
        }

        // Lazily load the replay-group list (used by the Group/Source value editor).
        if self.tab_state.search_tab.sources.is_empty()
            && let (Some(pool), Some(rt)) = (pool.as_ref(), rt.as_ref())
        {
            match rt.block_on(query::list_sources(pool)) {
                Ok(sources) => self.tab_state.search_tab.sources = sources,
                Err(e) => tracing::warn!("search: list_sources failed: {e}"),
            }
        }

        let ship_catalog = self.tab_state.ship_catalog.as_ref();
        let locale = self.tab_state.persisted.read().settings.app.locale.clone();

        let mut chip_to_remove: Option<(usize, usize)> = None;
        let mut new_chip: Option<(usize, Chip, Option<String>)> = None;
        let mut group_to_remove: Option<usize> = None;
        let mut want_add_group = false;
        let mut want_clear = false;
        let mut changed = false;

        let search_tab = &mut self.tab_state.search_tab;
        while search_tab.add_drafts.len() < search_tab.query.groups.len() {
            search_tab.add_drafts.push(AddFilterDraft::default());
        }
        search_tab.add_drafts.truncate(search_tab.query.groups.len());
        while search_tab.add_draft_open.len() < search_tab.query.groups.len() {
            search_tab.add_draft_open.push(false);
        }
        search_tab.add_draft_open.truncate(search_tab.query.groups.len());

        let num_groups = search_tab.query.groups.len();
        if num_groups == 0 {
            ui.label(t!("ui.search.no_groups"));
        }

        // Groups render side by side (horizontally scrolling if they don't
        // fit), with the AND/OR connector shown as a small chip between each
        // adjacent pair. Only the arrangement of groups is horizontal; the
        // chips within a group still wrap vertically as before.
        egui::ScrollArea::horizontal().id_salt("search_groups_scroll").show(ui, |ui| {
            ui.horizontal(|ui| {
                if num_groups > 1 {
                    ui.label(t!("ui.search.connector_label"));
                }

                for group_idx in 0..num_groups {
                    ui.group(|ui| {
                        ui.set_max_width(GROUP_MAX_WIDTH);

                        ui.horizontal(|ui| {
                            ui.strong(t!("ui.search.group_label", index = group_idx + 1));
                            if ui.small_button(t!("ui.search.remove_group")).clicked() {
                                group_to_remove = Some(group_idx);
                            }
                        });

                        ui.horizontal_wrapped(|ui| {
                            let chips = search_tab.query.groups[group_idx].chips.clone();
                            for (chip_idx, chip) in chips.iter().enumerate() {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(chip_pill_label(
                                            chip,
                                            &search_tab.resolved_ships,
                                            &search_tab.resolved_players,
                                            &search_tab.sources,
                                            locale.as_deref(),
                                        ));
                                        if ui.small_button("x").on_hover_text(t!("ui.search.remove")).clicked() {
                                            chip_to_remove = Some((group_idx, chip_idx));
                                        }
                                    });
                                });
                            }
                        });

                        let mut draft = search_tab.add_drafts[group_idx].clone();
                        let sources_snapshot = search_tab.sources.clone();
                        let mut draft_open = search_tab.add_draft_open[group_idx];

                        if !draft_open {
                            if ui.button(t!("ui.search.add_filter")).clicked() {
                                draft_open = true;
                            }
                            search_tab.add_draft_open[group_idx] = draft_open;
                        } else {
                            ui.separator();
                            ui.indent(("search_add_draft", group_idx), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(t!("ui.search.field_label"));
                                    let prev_field = draft.field;
                                    egui::ComboBox::from_id_salt(("search_add_field", group_idx))
                                        .selected_text(field_display_label(draft.field))
                                        .show_ui(ui, |ui| {
                                            for &f in NON_STAT_FIELDS {
                                                ui.selectable_value(&mut draft.field, f, field_display_label(f));
                                            }
                                            ui.separator();
                                            for kind in StatKind::ALL {
                                                let selected =
                                                    matches!(draft.field, Field::Stat { kind: k, .. } if k == kind);
                                                if ui.selectable_label(selected, stat_kind_label(kind)).clicked() {
                                                    // Subject is finalized just below once we know whether
                                                    // the previous field was already a Stat (preserve) or not
                                                    // (default to Me); this placeholder subject is discarded.
                                                    draft.field = Field::Stat { kind, subject: Subject::SelfPlayer };
                                                }
                                            }
                                        });
                                    if draft.field != prev_field {
                                        if let Field::Stat { kind, .. } = draft.field {
                                            let subject = match prev_field {
                                                Field::Stat { subject, .. } => subject,
                                                _ => Subject::SelfPlayer,
                                            };
                                            draft.field = Field::Stat { kind, subject };
                                            draft.subject = subject;
                                        } else {
                                            draft.subject = Subject::SelfPlayer;
                                            draft.subject_picking_player = false;
                                        }
                                        draft.reset_for_field(draft.field);
                                    }
                                });

                                if let Field::Stat { kind, subject } = draft.field {
                                    ui.horizontal(|ui| {
                                        ui.label(t!("ui.search.subject_label"));
                                        egui::ComboBox::from_id_salt(("search_add_subject", group_idx))
                                            .selected_text(subject_combo_label(
                                                subject,
                                                draft.subject_picking_player,
                                                &search_tab.resolved_players,
                                            ))
                                            .show_ui(ui, |ui| {
                                                let me_selected =
                                                    !draft.subject_picking_player && subject == Subject::SelfPlayer;
                                                if ui
                                                    .selectable_label(me_selected, t!("ui.search.subject_me"))
                                                    .clicked()
                                                {
                                                    draft.subject = Subject::SelfPlayer;
                                                    draft.subject_picking_player = false;
                                                    draft.field = Field::Stat { kind, subject: Subject::SelfPlayer };
                                                }
                                                let any_selected =
                                                    !draft.subject_picking_player && subject == Subject::AnyPlayer;
                                                if ui
                                                    .selectable_label(any_selected, t!("ui.search.subject_any"))
                                                    .clicked()
                                                {
                                                    draft.subject = Subject::AnyPlayer;
                                                    draft.subject_picking_player = false;
                                                    draft.field = Field::Stat { kind, subject: Subject::AnyPlayer };
                                                }
                                                let specific_selected = draft.subject_picking_player
                                                    || matches!(subject, Subject::Player(_));
                                                if ui
                                                    .selectable_label(
                                                        specific_selected,
                                                        t!("ui.search.subject_specific"),
                                                    )
                                                    .clicked()
                                                {
                                                    draft.subject_picking_player = true;
                                                }
                                            });
                                    });

                                    if draft.subject_picking_player {
                                        ui.indent(("search_add_subject_player", group_idx), |ui| {
                                            if ui.text_edit_singleline(&mut draft.subject_player_search).changed()
                                                && let (Some(pool), Some(rt)) = (pool.as_ref(), rt.as_ref())
                                            {
                                                match rt.block_on(query::search_players(
                                                    pool,
                                                    &draft.subject_player_search,
                                                    50,
                                                )) {
                                                    Ok(results) => draft.subject_player_results = results,
                                                    Err(e) => {
                                                        tracing::warn!("search: subject search_players failed: {e}")
                                                    }
                                                }
                                            }
                                            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                                for p in draft.subject_player_results.clone() {
                                                    let label = if p.clan.is_empty() {
                                                        p.latest_name.clone()
                                                    } else {
                                                        format!("[{}] {}", p.clan, p.latest_name)
                                                    };
                                                    let selected =
                                                        matches!(subject, Subject::Player(id) if id == p.account_id);
                                                    if ui.selectable_label(selected, label.clone()).clicked() {
                                                        draft.subject = Subject::Player(p.account_id);
                                                        draft.field = Field::Stat {
                                                            kind,
                                                            subject: Subject::Player(p.account_id),
                                                        };
                                                        draft.subject_picking_player = false;
                                                        search_tab
                                                            .resolved_players
                                                            .entry(p.account_id)
                                                            .or_insert(label);
                                                    }
                                                }
                                            });
                                        });
                                    }
                                }

                                ui.horizontal(|ui| {
                                    ui.label(t!("ui.search.op_label"));
                                    egui::ComboBox::from_id_salt(("search_add_op", group_idx))
                                        .selected_text(op_label(draft.op))
                                        .show_ui(ui, |ui| {
                                            for &o in draft.field.allowed_ops() {
                                                ui.selectable_value(&mut draft.op, o, op_label(o));
                                            }
                                        });
                                });

                                ui.separator();

                                match draft.field.value_kind() {
                                    ValueKind::Text => {
                                        ui.text_edit_singleline(&mut draft.text);
                                    }
                                    ValueKind::Int => {
                                        ui.add(egui::DragValue::new(&mut draft.int_val));
                                    }
                                    ValueKind::Outcome => {
                                        egui::ComboBox::from_id_salt(("search_add_outcome", group_idx))
                                            .selected_text(outcome_label(draft.outcome))
                                            .show_ui(ui, |ui| {
                                                for o in [
                                                    MatchOutcome::Win,
                                                    MatchOutcome::Loss,
                                                    MatchOutcome::Draw,
                                                    MatchOutcome::Unknown,
                                                ] {
                                                    ui.selectable_value(&mut draft.outcome, o, outcome_label(o));
                                                }
                                            });
                                    }
                                    ValueKind::Bool => {
                                        egui::ComboBox::from_id_salt(("search_add_bool", group_idx))
                                            .selected_text(bool_label(draft.bool_val))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut draft.bool_val, true, bool_label(true));
                                                ui.selectable_value(&mut draft.bool_val, false, bool_label(false));
                                            });
                                    }
                                    ValueKind::Class => {
                                        egui::ComboBox::from_id_salt(("search_add_class", group_idx))
                                            .selected_text(species_name(&draft.species))
                                            .show_ui(ui, |ui| {
                                                for &s in SHIP_SPECIES {
                                                    ui.selectable_value(&mut draft.species, s, species_name(&s));
                                                }
                                            });
                                    }
                                    ValueKind::Ship => {
                                        ui.text_edit_singleline(&mut draft.ship_search);
                                        match ship_catalog {
                                            Some(catalog) => {
                                                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                                    for entry in catalog.search(&draft.ship_search, 30) {
                                                        let selected = draft.ship_id == Some(entry.ship_id);
                                                        let label = format!(
                                                            "{} (T{})",
                                                            entry.display_name,
                                                            tier_roman(entry.tier)
                                                        );
                                                        if ui.selectable_label(selected, label).clicked() {
                                                            draft.ship_id = Some(entry.ship_id);
                                                            draft.ship_label = entry.display_name.clone();
                                                        }
                                                    }
                                                });
                                            }
                                            None => {
                                                ui.label(t!("ui.search.ship_catalog_unavailable"));
                                            }
                                        }
                                    }
                                    ValueKind::Account => {
                                        if ui.text_edit_singleline(&mut draft.player_search).changed()
                                            && let (Some(pool), Some(rt)) = (pool.as_ref(), rt.as_ref())
                                        {
                                            match rt.block_on(query::search_players(pool, &draft.player_search, 50)) {
                                                Ok(results) => draft.player_results = results,
                                                Err(e) => tracing::warn!("search: search_players failed: {e}"),
                                            }
                                        }
                                        egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                            for p in draft.player_results.clone() {
                                                let label = if p.clan.is_empty() {
                                                    p.latest_name.clone()
                                                } else {
                                                    format!("[{}] {}", p.clan, p.latest_name)
                                                };
                                                let selected = draft.player_id == Some(p.account_id);
                                                if ui.selectable_label(selected, label.clone()).clicked() {
                                                    draft.player_id = Some(p.account_id);
                                                    draft.player_label = label;
                                                }
                                            }
                                        });
                                    }
                                    ValueKind::Timestamp => {
                                        let date_salt = format!("search_add_date_{group_idx}");
                                        ui.add(egui_extras::DatePickerButton::new(&mut draft.date).id_salt(&date_salt));
                                    }
                                    ValueKind::Source => {
                                        let selected_name = draft
                                            .source_id
                                            .and_then(|id| sources_snapshot.iter().find(|s| s.id == id))
                                            .map(|s| s.name.clone())
                                            .unwrap_or_else(|| t!("ui.search.source_any").into());
                                        egui::ComboBox::from_id_salt(("search_add_source", group_idx))
                                            .selected_text(selected_name)
                                            .show_ui(ui, |ui| {
                                                for s in &sources_snapshot {
                                                    ui.selectable_value(
                                                        &mut draft.source_id,
                                                        Some(s.id),
                                                        s.name.clone(),
                                                    );
                                                }
                                            });
                                    }
                                }

                                ui.separator();
                                ui.horizontal(|ui| {
                                    let add_disabled =
                                        matches!(draft.field, Field::Stat { .. }) && draft.subject_picking_player;
                                    if ui.add_enabled(!add_disabled, egui::Button::new(t!("ui.search.add"))).clicked()
                                        && let Some(value) = draft.to_value()
                                    {
                                        let label = draft.value_display_label();
                                        new_chip =
                                            Some((group_idx, Chip { field: draft.field, op: draft.op, value }, label));
                                        draft = AddFilterDraft::default();
                                        draft_open = false;
                                    }
                                    if ui.button(t!("ui.buttons.cancel")).clicked() {
                                        draft = AddFilterDraft::default();
                                        draft_open = false;
                                    }
                                });
                            });
                            search_tab.add_draft_open[group_idx] = draft_open;
                        }

                        search_tab.add_drafts[group_idx] = draft;
                    });

                    if group_idx + 1 < num_groups {
                        changed |= ui
                            .selectable_value(&mut search_tab.query.connector, Connector::And, t!("ui.search.and"))
                            .changed();
                        changed |= ui
                            .selectable_value(&mut search_tab.query.connector, Connector::Or, t!("ui.search.or"))
                            .changed();
                    }
                }

                if ui.button(t!("ui.search.add_group")).clicked() {
                    want_add_group = true;
                }
            });
        });

        ui.horizontal(|ui| {
            if ui.button(t!("ui.search.clear_filters")).clicked() {
                want_clear = true;
            }
        });

        if let Some((group_idx, chip_idx)) = chip_to_remove {
            self.tab_state.search_tab.query.groups[group_idx].chips.remove(chip_idx);
            changed = true;
        }
        if let Some((group_idx, chip, label)) = new_chip {
            if let Some(label) = label {
                match &chip.value {
                    Value::Ship(id) => {
                        self.tab_state.search_tab.resolved_ships.insert(*id, label);
                    }
                    Value::Account(id) => {
                        self.tab_state.search_tab.resolved_players.insert(*id, label);
                    }
                    _ => {}
                }
            }
            self.tab_state.search_tab.query.groups[group_idx].chips.push(chip);
            changed = true;
        }
        if let Some(group_idx) = group_to_remove {
            self.tab_state.search_tab.query.groups.remove(group_idx);
            self.tab_state.search_tab.add_drafts.remove(group_idx);
            self.tab_state.search_tab.add_draft_open.remove(group_idx);
            changed = true;
        }
        if want_add_group {
            self.tab_state.search_tab.query.groups.push(Group::default());
            changed = true;
        }
        if want_clear {
            self.tab_state.search_tab.query = Query::default();
            self.tab_state.search_tab.add_drafts.clear();
            changed = true;
        }

        if changed {
            self.tab_state.search_tab.dirty = true;
            // Mirror the query into persisted settings so it survives an app
            // restart; the background save task picks it up from there.
            self.tab_state.persisted.write().settings.search_query = self.tab_state.search_tab.query.clone();
        }

        ui.separator();

        // Re-query when the query changed and the DB is available.
        if self.tab_state.search_tab.dirty
            && let (Some(pool), Some(rt)) = (pool.as_ref(), rt.as_ref())
        {
            let query = self.tab_state.search_tab.query.clone();
            let search_tab = &mut self.tab_state.search_tab;
            resolve_seeded_names(&query, &mut search_tab.resolved_ships, &mut search_tab.resolved_players, pool, rt);
            match rt.block_on(crate::db::index::query::search_by_query(pool, &query, 500)) {
                Ok(hits) => self.tab_state.search_tab.results = hits,
                Err(e) => tracing::warn!("search query failed: {e}"),
            }
            self.tab_state.search_tab.dirty = false;
        }

        ui.label(t!("ui.search.match_count", count = self.tab_state.search_tab.results.len()));

        let mut open_path: Option<std::path::PathBuf> = None;
        egui::ScrollArea::horizontal().id_salt("search_results").show(ui, |ui| {
            use egui_extras::Column;
            use egui_extras::TableBuilder;
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(150.0)) // date
                .column(Column::initial(120.0)) // map
                .column(Column::initial(90.0)) // mode
                .column(Column::initial(140.0)) // ship
                .column(Column::initial(60.0)) // result
                .column(Column::initial(80.0)) // dmg
                .column(Column::initial(50.0)) // kills
                .column(Column::initial(60.0)) // pr
                .column(Column::remainder()) // open
                .header(20.0, |mut h| {
                    for label in [
                        t!("ui.search.column.date"),
                        t!("ui.search.column.map"),
                        t!("ui.search.column.mode"),
                        t!("ui.search.column.ship"),
                        t!("ui.search.column.result"),
                        t!("ui.search.column.damage"),
                        t!("ui.search.column.kills"),
                        t!("ui.search.column.pr"),
                    ] {
                        h.col(|ui| {
                            ui.strong(label);
                        });
                    }
                    h.col(|_ui| {});
                })
                .body(|mut body| {
                    for hit in &self.tab_state.search_tab.results {
                        let ship_name = self.search_ship_display_name(hit);
                        body.row(24.0, |mut row| {
                            row.col(|ui| {
                                ui.label(hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string());
                            });
                            row.col(|ui| {
                                ui.label(&hit.map);
                            });
                            row.col(|ui| {
                                ui.label(&hit.game_type);
                            });
                            row.col(|ui| {
                                ui.label(ship_name.clone().unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(match hit.outcome {
                                    MatchOutcome::Win => "W",
                                    MatchOutcome::Loss => "L",
                                    MatchOutcome::Draw => "D",
                                    MatchOutcome::Unknown => "-",
                                });
                            });
                            row.col(|ui| {
                                ui.label(
                                    hit.self_damage
                                        .map(|d| crate::util::formatting::separate_number(d, locale.as_deref()))
                                        .unwrap_or_default(),
                                );
                            });
                            row.col(|ui| {
                                ui.label(hit.self_kills.map(|k| k.to_string()).unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_default());
                            });
                            row.col(|ui| {
                                let exists = hit.replay_path.exists();
                                let btn = ui.add_enabled(exists, egui::Button::new(t!("ui.search.open")));
                                if !exists {
                                    btn.on_hover_text(t!("ui.search.open_missing"));
                                } else if btn.clicked() {
                                    open_path = Some(hit.replay_path.clone());
                                }
                            });
                        });
                    }
                });
        });

        if let Some(path) = open_path
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            crate::update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path, crate::task::ReplaySource::ManualOpen)
            );
        }
    }
}

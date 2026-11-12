//! The Players and Clans sections: a sortable header over a virtualized list whose rows open to
//! show more.
//!
//! The rows differ in height once opened, so the list is `VirtualList` with measured rows. The
//! landaire `Grid` builds every row in its one mode that opens rows, which is too many for an
//! established install's thousands of players.

use jiff::Timestamp;
use landaire_ui::MarkedTextProps;
use landaire_ui::RowIndex;
use landaire_ui::TABULAR_CLASS;
use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::EncounterSeverity;
use wows_toolkit_viewmodel::player_tracker::PlayerRow;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::SortOrder;
use wows_toolkit_viewmodel::player_tracker::clans::ClanRow;
use wows_toolkit_viewmodel::player_tracker::encounter_severity;
use wows_toolkit_viewmodel::player_tracker::history;
use zgui::component;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::InputProps;
use zgui_ui::SpinnerProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui::VirtualListProps;
use zgui_ui::virtualize::MeasuredRows;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::arrow::ARROW_DOWN;
use zgui_ui_icons::set::arrow::ARROW_UP;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::player_tracker::model::LoadState;
use crate::tabs::player_tracker::model::TrackerModel;
use crate::tabs::player_tracker::model::table_status;
use crate::text_marks::marks;

/// A row's height before it opens; the estimate a list gives a row it has not measured.
pub const ROW_HEIGHT: f32 = 24.0;

/// The `data-severity` a count is drawn with, or none for a number not worth marking.
pub fn severity(times: usize) -> Option<String> {
    match encounter_severity(times) {
        EncounterSeverity::None => None,
        EncounterSeverity::Noted => Some("noted".to_owned()),
        EncounterSeverity::Warned => Some("warned".to_owned()),
        EncounterSeverity::Heavy => Some("heavy".to_owned()),
    }
}

/// A count the tracker has no record of reads as a dash rather than a zero, which would claim the
/// player was never met.
fn count_text(count: Option<usize>) -> String {
    match count {
        Some(count) => separate_number(count as i64, None),
        None => "-".to_owned(),
    }
}

fn player_column_class(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Name => "wt-tracker-col--name",
        SortColumn::Clan => "wt-tracker-col--clan",
        SortColumn::TotalEncounters => "wt-tracker-col--count",
        SortColumn::Encounters => "wt-tracker-col--range",
        SortColumn::LastEncountered => "wt-tracker-col--seen",
    }
}

fn clan_column_class(column: ClanSortColumn) -> &'static str {
    match column {
        ClanSortColumn::Clan => "wt-tracker-col--tag",
        ClanSortColumn::Members => "wt-tracker-col--members",
        ClanSortColumn::Encounters | ClanSortColumn::Sightings => "wt-tracker-col--count",
        ClanSortColumn::EncountersInRange => "wt-tracker-col--range",
        ClanSortColumn::LastEncountered => "wt-tracker-col--seen",
    }
}

/// One sortable heading, with the arrow on the column the table is ordered by.
#[component]
fn SortHeading(
    #[prop(into)] label: String,
    class: &'static str,
    test_id: String,
    #[prop(into)] order: Signal<Option<SortOrder>>,
    #[prop(into)] on_sort: UnsyncCallback<()>,
) -> impl IntoView {
    view! {
        Button(
            variant = ButtonVariant::Ghost,
            size = ButtonSize::Xs,
            class = "wt-tracker-heading",
            class = class,
            attr:data-testid = test_id,
            attr:data-active = move || order.get().map(|_| String::new()),
            a11y:label = label.clone(),
            on:click = move |_| on_sort.run(())
        ) {
            text {{label.clone()}}
            {move || order.get().map(|order| {
                let icon = match order {
                    SortOrder::Ascending => ARROW_UP,
                    SortOrder::Descending => ARROW_DOWN,
                };
                view! { Icon(icon = icon, size = IconSize::Sm) }.into_view()
            })}
        }
    }
}

/// The triangle that opens a row, turned while it is open.
#[component]
fn ExpandCaret(
    test_id: String,
    #[prop(into)] open: Signal<bool>,
    #[prop(into)] on_toggle: UnsyncCallback<()>,
) -> impl IntoView {
    view! {
        Button(
            variant = ButtonVariant::Ghost,
            size = ButtonSize::IconXs,
            class = "wt-tracker-caret",
            attr:data-testid = test_id,
            attr:data-open = move || open.get().then(String::new),
            a11y:expanded = move || open.get(),
            on:click = move |_| on_toggle.run(())
        ) {
            Icon(icon = CHEVRON_RIGHT, size = IconSize::Sm)
        }
    }
}

/// A button with an icon and a hover.
#[component]
pub fn IconButton(
    glyph: Glyph,
    #[prop(into)] hover: String,
    test_id: String,
    #[prop(optional)] faint: bool,
    #[prop(into)] on_press: UnsyncCallback<()>,
) -> impl IntoView {
    let label = hover.clone();
    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconXs,
                    class = "wt-tracker-icon-button",
                    class:wt-tracker-faint = faint,
                    attr:data-testid = test_id,
                    a11y:label = label,
                    on:click = move |_| on_press.run(())
                ) {
                    AppIcon(glyph = glyph)
                }
            }
            TooltipContent {{hover.clone()}}
        }
    }
}

/// Text with a hover, or bare text when the hover is empty.
#[component]
pub fn HoverText(#[prop(into)] text: String, #[prop(into)] hover: String, class: &'static str) -> impl IntoView {
    if hover.is_empty() {
        return AnyView::new(view! { text(class = class) {{text}} });
    }
    let text = StoredValue::new(text);
    let hover = StoredValue::new(hover);
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = class) {
                text(a11y:label = hover.get_value()) {{text.get_value()}}
            }
            TooltipContent {{hover.get_value()}}
        }
    })
}

/// When a row was last met, as an age with the exact local time as its hover.
#[component]
fn LastSeen(at: Option<Timestamp>) -> impl IntoView {
    let relative = history::last_seen_text(at, Timestamp::now());
    let exact = history::last_seen_timestamp_text(at);
    view! { HoverText(text = relative, hover = exact, class = "wt-tracker-col--seen wt-tracker-dim") }
}

/// What a table shows in place of its rows, or the rows.
fn table_body(
    state: Signal<LoadState>,
    count: Signal<usize, LocalStorage>,
    loading_key: &'static str,
    empty_key: &'static str,
    list: impl Fn() -> AnyView + 'static,
) -> impl IntoView {
    let locale = Services::current().locale;
    move || {
        locale.track();
        let loading = state.with(|state| *state == LoadState::Loading);
        match state.with(|state| table_status(state, count.get(), loading_key, empty_key)) {
            Some(status) if loading => AnyView::new(view! {
                row(class = "wt-tracker-status", attr:data-testid = "tracker-status") {
                    Spinner(label = status.clone())
                    text {{status}}
                }
            }),
            Some(status) => AnyView::new(view! {
                row(class = "wt-tracker-status", attr:data-testid = "tracker-status") { text {{status}} }
            }),
            None => list(),
        }
    }
}

/// The Players section.
#[component]
pub fn PlayersSection(model: TrackerModel) -> impl IntoView {
    let locale = Services::current().locale;
    let shell = Shell::current();
    let rows = model.rows;
    let count = Signal::derive_local(move || rows.with(Vec::len));
    let state = Signal::derive(move || model.load.get());
    let headings = Signal::derive_local(move || {
        locale.track();
        SortColumn::ALL.to_vec()
    });

    view! {
        column(class = "wt-tracker-section", attr:data-testid = "tracker-players") {
            row(class = "wt-tracker-header") {
                for column in move || headings.get(), key = |column: &SortColumn| *column as usize {
                    SortHeading(
                        label = t!(column.label_key()).into_owned(),
                        class = player_column_class(column),
                        test_id = format!("tracker-sort-{column:?}"),
                        order = Signal::derive(move || {
                            let sort = model.sort.get();
                            (sort.column == column).then_some(sort.order)
                        }),
                        on_sort = move || model.sort_by(column)
                    )
                }
            }
            {table_body(state, count, "ui.player_tracker.loading_players", "ui.player_tracker.no_players", move || {
                AnyView::new(view! {
                    VirtualList(
                        count = count,
                        measured = MeasuredRows::new(ROW_HEIGHT).keyed(move |index| {
                            rows.with(|rows| rows.get(index).map(|row| row.facet.account_id.raw()))
                        }),
                        label = t!("ui.player_tracker.subtab_players").into_owned(),
                        class = "wt-tracker-list",
                        row = move |index: usize| view! { PlayerListRow(model = model, shell = shell, index = index) }
                    )
                })
            })}
        }
    }
}

/// One historical row, read by position from the sorted rows.
#[component]
fn PlayerListRow(model: TrackerModel, shell: Shell, index: usize) -> impl IntoView {
    move || {
        let Some(row) = model.rows.with(|rows| rows.get(index).cloned()) else {
            return AnyView::new(());
        };
        AnyView::new(view! { PlayerRowView(model = model, shell = shell, index = index, row = row) })
    }
}

#[component]
fn PlayerRowView(model: TrackerModel, shell: Shell, index: usize, row: PlayerRow) -> impl IntoView {
    let account = row.facet.account_id;
    let open = Signal::derive(move || model.expanded_players.with(|expanded| expanded.contains(&account)));
    let note = Signal::derive(move || {
        model
            .tracked
            .with(|tracked| tracked.get(&account).map(|player| player.notes.clone()).filter(|note| !note.is_empty()))
    });
    // Both counts are read in the tone the in-range one deserves: how often you have met someone
    // lately is what the ramp warns about.
    let tone = severity(row.encounters_in_range);
    let name = row.facet.latest_name.clone();
    let clan = row.facet.clan.clone();
    // Kept by its name or its tag; whichever the filter did not match is drawn plain.
    let (name_marks, clan_marks) = model.query.with_untracked(|query| (marks(query, &name), marks(query, &clan)));

    view! {
        column(
            class = "wt-tracker-row",
            class:lui-stripe = RowIndex(index).is_striped(),
            attr:data-testid = format!("tracker-player-{}", account.raw()),
            a11y:label = name.clone()
        ) {
            row(class = "wt-tracker-cells") {
                row(class = "wt-tracker-col--name") {
                    ExpandCaret(
                        test_id = format!("tracker-player-expand-{}", account.raw()),
                        open = open,
                        on_toggle = move || model.toggle_player_expanded(account)
                    )
                    MarkedText(class = "wt-tracker-truncate", text = name, ranges = name_marks)
                }
                MarkedText(class = "wt-tracker-col--clan wt-tracker-dim", text = clan, ranges = clan_marks)
                text(class = "wt-tracker-col--count", class = TABULAR_CLASS, attr:data-severity = tone.clone()) {{count_text(row.total_encounters)}}
                text(class = "wt-tracker-col--range", class = TABULAR_CLASS, attr:data-severity = tone) {{separate_number(row.encounters_in_range as i64, None)}}
                LastSeen(at = row.last_seen)
                IconButton(
                    glyph = Glyph::Search,
                    hover = t!("ui.player_tracker.find_matches").into_owned(),
                    test_id = format!("tracker-find-player-{}", account.raw()),
                    on_press = move || TrackerModel::find_player_matches(shell, account)
                )
                box(class = "wt-tracker-note-slot") {
                    {move || {
                        let note = note.get();
                        let faint = note.is_none();
                        let hover = note.unwrap_or_else(|| t!("ui.player_tracker.notes_hint").into_owned());
                        view! {
                            IconButton(
                                glyph = Glyph::NotebookPen,
                                hover = hover,
                                faint = faint,
                                test_id = format!("tracker-note-{}", account.raw()),
                                on_press = move || model.edit_note(account)
                            )
                        }
                    }}
                }
            }
            if move || open.get() {
                PlayerDetail(model = model, shell = shell, account = account)
            } else {}
        }
    }
}

/// The block under an opened historical row: what the tracker knows beyond the columns, and the
/// note editor when this is the row being written about.
#[component]
fn PlayerDetail(model: TrackerModel, shell: Shell, account: AccountId) -> impl IntoView {
    let locale = Services::current().locale;
    let lines = Signal::derive_local(move || {
        locale.track();
        let show_division_mates = model.show_division_mates.get();
        model.tracked.with(|tracked| {
            let Some(player) = tracked.get(&account) else { return Vec::new() };
            let mut aliases: Vec<String> =
                player.names.iter().filter(|name| **name != player.last_name).cloned().collect();
            aliases.sort();
            let mut lines = Vec::new();
            if !aliases.is_empty() {
                lines.push(t!("ui.player_tracker.aliases_hover", names = aliases.join(", ")).into_owned());
            }
            let exact = history::last_seen_timestamp_text(player.last_visible_timestamp(show_division_mates));
            if !exact.is_empty() {
                lines.push(t!("ui.player_tracker.last_encountered_exact", timestamp = exact).into_owned());
            }
            let count = player.visible_arena_ids(show_division_mates).count();
            lines.push(t!("ui.player_tracker.arena_count", count = count).into_owned());
            lines
        })
    });
    let id = account.raw().to_string();
    let copied = id.clone();

    view! {
        column(class = "wt-tracker-detail", attr:data-testid = format!("tracker-player-detail-{}", account.raw())) {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Xs,
                        class = "wt-tracker-account",
                        class = TABULAR_CLASS,
                        attr:data-testid = format!("tracker-account-id-{}", account.raw()),
                        on:click = move |_| {
                            shell.copy_text(copied.clone());
                            shell.toasts().ok(t!("ui.player_tracker.copied_wg_id", id = copied.clone()).into_owned());
                        }
                    ) {{id}}
                }
                TooltipContent {{t!("ui.player_tracker.copy_wg_id").into_owned()}}
            }
            for line in move || lines.get(), key = |line: &String| line.clone() {
                text(class = "wt-tracker-dim wt-tracker-small") {{line}}
            }
            if move || model.editing_note.get() == Some(account) {
                NoteEditor(model = model, account = account)
            } else {
                text(class = "wt-tracker-dim wt-tracker-small") {{move || { locale.track(); t!("ui.player_tracker.notes_hint").into_owned() }}}
            }
        }
    }
}

/// The note field, saved on Enter or when it is left, so a half-typed note never reaches the
/// database.
#[component]
fn NoteEditor(model: TrackerModel, account: AccountId) -> impl IntoView {
    let value = RwSignal::new_local(model.note_for(account));
    let commit = move || model.store_note(account, value.get_untracked());

    view! {
        column(class = "wt-tracker-note") {
            Input(
                value = value,
                placeholder = t!("ui.player_tracker.notes_hint").into_owned(),
                label = t!("ui.player_tracker.notes_hint").into_owned(),
                class = "wt-tracker-note__field",
                attr:data-testid = "tracker-note-input",
                on:key_down = move |ev| {
                    if ev.key == Key::Named(NamedKey::Enter) {
                        commit();
                    }
                },
                on:focus_out = move |_| commit()
            )
            box(class = "wt-tracker-note__status") {
                {move || model.note_error.get().map(|reason| view! {
                    text(class = "wt-tracker-note__error wt-tracker-small", attr:data-testid = "tracker-note-error") {
                        {t!("ui.player_tracker.note_not_saved", reason = reason).into_owned()}
                    }
                }.into_view())}
            }
        }
    }
}

/// The Clans section.
#[component]
pub fn ClansSection(model: TrackerModel) -> impl IntoView {
    let locale = Services::current().locale;
    let shell = Shell::current();
    let clans = model.clans;
    let count = Signal::derive_local(move || clans.with(Vec::len));
    let state = Signal::derive(move || model.clan_load.get());
    let headings = Signal::derive_local(move || {
        locale.track();
        ClanSortColumn::ALL.to_vec()
    });

    view! {
        column(class = "wt-tracker-section", attr:data-testid = "tracker-clans") {
            row(class = "wt-tracker-header") {
                for column in move || headings.get(), key = |column: &ClanSortColumn| *column as usize {
                    SortHeading(
                        label = t!(column.label_key()).into_owned(),
                        class = clan_column_class(column),
                        test_id = format!("tracker-clan-sort-{column:?}"),
                        order = Signal::derive(move || {
                            let sort = model.clan_sort.get();
                            (sort.column == column).then_some(sort.order)
                        }),
                        on_sort = move || model.sort_clans_by(column)
                    )
                }
            }
            {table_body(state, count, "ui.player_tracker.loading_clans", "ui.player_tracker.clan_no_data", move || {
                AnyView::new(view! {
                    VirtualList(
                        count = count,
                        measured = MeasuredRows::new(ROW_HEIGHT).keyed(move |index| {
                            clans.with(|clans| clans.get(index).map(|row| row.clan.clone()))
                        }),
                        label = t!("ui.player_tracker.subtab_clans").into_owned(),
                        class = "wt-tracker-list",
                        row = move |index: usize| view! { ClanListRow(model = model, shell = shell, index = index) }
                    )
                })
            })}
        }
    }
}

#[component]
fn ClanListRow(model: TrackerModel, shell: Shell, index: usize) -> impl IntoView {
    move || {
        let Some(row) = model.clans.with(|clans| clans.get(index).cloned()) else {
            return AnyView::new(());
        };
        AnyView::new(view! { ClanRowView(model = model, shell = shell, index = index, row = row) })
    }
}

#[component]
fn ClanRowView(model: TrackerModel, shell: Shell, index: usize, row: ClanRow) -> impl IntoView {
    let tag = row.clan.clone();
    let open_tag = tag.clone();
    let open = Signal::derive(move || model.expanded_clans.with(|expanded| expanded.contains(&open_tag)));
    let toggle_tag = tag.clone();
    let find_tag = tag.clone();
    // The tag and both battle counts read in the tone the in-range count deserves.
    let tone = severity(row.matches_in_range);
    let sightings_hover = t!("ui.player_tracker.clan_sightings_hover", range = row.sightings_in_range).into_owned();
    let sightings = separate_number(row.sightings as i64, None);
    let members = row.members.clone();
    let tag_marks = model.query.with_untracked(|query| marks(query, &tag));

    view! {
        column(
            class = "wt-tracker-row",
            class:lui-stripe = RowIndex(index).is_striped(),
            attr:data-testid = format!("tracker-clan-{tag}"),
            a11y:label = tag.clone()
        ) {
            row(class = "wt-tracker-cells") {
                row(class = "wt-tracker-col--tag") {
                    ExpandCaret(
                        test_id = format!("tracker-clan-expand-{tag}"),
                        open = open,
                        on_toggle = move || model.toggle_clan_expanded(toggle_tag.clone())
                    )
                    MarkedText(class = "wt-tracker-truncate", attr:data-severity = tone.clone(), text = tag.clone(), ranges = tag_marks)
                }
                text(class = "wt-tracker-col--members", class = TABULAR_CLASS) {{row.members.len().to_string()}}
                text(class = "wt-tracker-col--count", class = TABULAR_CLASS) {{separate_number(row.matches as i64, None)}}
                text(class = "wt-tracker-col--range", class = TABULAR_CLASS, attr:data-severity = tone) {{separate_number(row.matches_in_range as i64, None)}}
                HoverText(text = sightings, hover = sightings_hover, class = "wt-tracker-col--count")
                LastSeen(at = Some(row.last_seen))
                IconButton(
                    glyph = Glyph::Search,
                    hover = t!("ui.player_tracker.find_clan_matches").into_owned(),
                    test_id = format!("tracker-find-clan-{tag}"),
                    on_press = move || TrackerModel::find_clan_matches(shell, &find_tag)
                )
            }
            if move || open.get() {
                ClanMembers(model = model, shell = shell, members = members.clone())
            } else {}
        }
    }
}

/// The members met from a clan, most-met first, each offering to look up their matches.
#[component]
fn ClanMembers(model: TrackerModel, shell: Shell, members: Vec<(AccountId, usize)>) -> impl IntoView {
    let named: Vec<(AccountId, String, usize)> = model.tracked.with_untracked(|tracked| {
        members
            .iter()
            .map(|(account, matches)| {
                // A member recorded under no name is still worth a row: the id is what the search needs.
                let name = tracked
                    .get(account)
                    .map(|player| player.last_name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| account.raw().to_string());
                (*account, name, *matches)
            })
            .collect()
    });

    view! {
        column(class = "wt-tracker-detail wt-tracker-detail--clan") {
            for member in move || named.clone(), key = |member: &(AccountId, String, usize)| member.0.raw() {
                row(class = "wt-tracker-member") {
                    IconButton(
                        glyph = Glyph::Search,
                        hover = t!("ui.player_tracker.find_matches").into_owned(),
                        test_id = format!("tracker-find-member-{}", member.0.raw()),
                        on_press = move || TrackerModel::find_player_matches(shell, member.0)
                    )
                    text(class = "wt-tracker-small") {{member.1.clone()}}
                    text(class = "wt-tracker-small wt-tracker-dim") {{t!("ui.player_tracker.clan_member_matches", count = member.2).into_owned()}}
                }
            }
        }
    }
}

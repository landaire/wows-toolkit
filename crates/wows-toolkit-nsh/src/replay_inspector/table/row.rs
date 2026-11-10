//! One player row: its cells (the Name and Skills cells drawn piece by
//! piece), the detail an expanded row grows under each column, the actions
//! button at the end of the name and the row's context menu.

use std::rc::Rc;

use nsh_base::context_menu;
use nsh_base::menu;
use nsh_base::menu_item;
use nsh_core::prelude::*;
use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_toolkit_replay_data::cells::CellValue;
use wows_toolkit_replay_data::cells::cell_value;
use wows_toolkit_replay_data::cells::name_color_kind;
use wows_toolkit_replay_data::cells::player_color_kind;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::SkillWarning;
use wows_toolkit_replay_data::player_table::ROW_ACTIONS_WIDTH;
use wows_toolkit_replay_data::player_table::SHIP_ICON_WIDTH;
use wows_toolkit_replay_data::player_table::connection_hover_text;
use wows_toolkit_replay_data::player_table::menu_heading_text;
use wows_toolkit_replay_data::player_table::shows_build_links;
use wows_toolkit_replay_data::player_table::twitch_hover_text;
use wows_toolkit_viewmodel::twitch::SniperCandidate;

use crate::class_icon::ClassIcon;
use crate::icons::Icon;
use crate::icons::icon;
use crate::replay_inspector::table::Table;
use crate::replay_inspector::table::colors;
use crate::replay_inspector::table::detail;
use crate::replay_inspector::table::icons::TableIcons;
use crate::replay_inspector::table::parts::MENU_POPUP;
use crate::replay_inspector::table::parts::class;
use crate::replay_inspector::table::parts::glyph_button;
use crate::replay_inspector::table::parts::menu_entry;
use crate::replay_inspector::table::parts::menu_rule;
use crate::replay_inspector::table::parts::tip;
use crate::replay_inspector::table::parts::tip_lines;
use crate::replay_inspector::table::secondary;

/// Where a row menu opened from: the dots stand at the end of the name
/// rather than over a cell, so they offer no Copy Cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuOrigin {
    RowRightClick,
    RowDots,
}

/// The row at `at`, rebuilt when the list shows another row in its cell,
/// when the rows are sorted or re-read, or when it expands.
pub fn row(table: Table, at: Position) -> AnyElement {
    let player = computed(move || table.rows.with(|rows| rows.0.get(at.index()).map(|row| row.db_id)));
    let expanded = computed(move || player.get().is_some_and(|id| table.expanded.with(|set| set.contains(&id))));

    dynamic(move || {
        let index = at.index();
        let rows = table.rows.get();
        let is_expanded = expanded.get();
        let debug = table.debug.get();
        let icons = table.icons.get();
        table.columns.with(|_| ());

        let Some(row) = rows.0.get(index) else {
            return view().into_element();
        };
        let layout = RowLayout { table, index, row, is_expanded, debug, icons: &icons };
        untracked(|| layout.build())
    })
}

/// What one row's cells are built from.
struct RowLayout<'a> {
    table: Table,
    index: usize,
    row: &'a PlayerRow,
    is_expanded: bool,
    debug: bool,
    icons: &'a TableIcons,
}

impl RowLayout<'_> {
    fn build(&self) -> AnyElement {
        let table = self.table;
        let player = self.row.db_id;
        let stripe = self.index.is_multiple_of(2);
        let align = if self.is_expanded { "items-start" } else { "items-center" };

        let sticky = view()
            .app_class(class(if self.is_expanded {
                "flex-row flex-none overflow-hidden items-start"
            } else {
                "flex-row flex-none overflow-hidden items-center"
            }))
            .w(move || Length::from(px(table.sticky_width())))
            .children(table.sticky_columns().into_iter().map(|col| self.column_cell(col)).collect::<Vec<_>>());
        let scrolling = view().app_class(class("flex-1 min-w-0 overflow-hidden")).child(
            view()
                .app_class(class(if self.is_expanded {
                    "flex-row flex-none items-start"
                } else {
                    "flex-row flex-none items-center"
                }))
                .w(move || Length::from(px(table.scroll_width())))
                .translate_x(move || DefiniteLength::from(px(-table.scroll_x.get())))
                .children(table.scroll_columns().into_iter().map(|col| self.column_cell(col)).collect::<Vec<_>>()),
        );

        let row_view = view()
            .role(AccessibilityRole::Row)
            .label(self.row.display_name.clone())
            .app_class(move || {
                let background = if table.selected.get() == Some(player) {
                    "bg-selection"
                } else if stripe {
                    "bg-muted/40"
                } else {
                    "bg-background"
                };
                Some(SharedString::from(format!(
                    "group relative flex-row w-full py-0.5 {align} {background} hover:bg-muted"
                )))
            })
            .on_press(move |press: &PressEvent| {
                if secondary(&press.modifiers) {
                    table.toggle_selected(player);
                } else if press.clicks >= 2 {
                    table.toggle_expanded(player);
                }
            })
            .on_mouse_leave(move |_: &MouseEvent| {
                if table.hovered.with(|hovered| hovered.is_some_and(|(id, _)| id == player)) {
                    table.hovered.replace(None);
                }
            })
            .child(sticky)
            .child(scrolling)
            .child(self.row_actions());

        // The cell under the pointer when the menu opens is what Copy Cell
        // copies; a menu over the blank end of the row offers none.
        let menu_cell = signal(None::<ReplayColumn>);
        let data = Rc::new(MenuData::from_row(self.row, self.icons, self.debug));
        context_menu()
            .trigger(row_view)
            .popup_class(MENU_POPUP)
            .on_open(move || {
                let cell = table.hovered.with(|hovered| hovered.filter(|(id, _)| *id == player).map(|(_, col)| col));
                menu_cell.set(cell);
            })
            .children(menu_items(table, data, MenuOrigin::RowRightClick, menu_cell))
            .into_element()
    }

    /// One column's cell, and under it the column's detail when the row is
    /// expanded.
    fn column_cell(&self, col: ReplayColumn) -> AnyElement {
        let table = self.table;
        let player = self.row.db_id;
        let collapsed = match col {
            ReplayColumn::Name => self.name_cell(),
            ReplayColumn::Skills => self.skills_cell(),
            _ => self.plain_cell(col, cell_value(self.row, col, self.debug)),
        };

        let detail =
            self.is_expanded.then(|| detail::column_detail(table, col, self.row, self.debug, self.icons)).flatten();
        let cell = match detail {
            Some(detail) => view()
                .app_class(class("flex-col flex-none gap-1 overflow-hidden"))
                .w(move || Length::from(px(table.width(col))))
                .child(collapsed)
                .child(detail)
                .into_element(),
            None => collapsed,
        };

        view()
            .app_class(class("flex-none"))
            .on_mouse_enter(move |_: &MouseEvent| {
                table.hovered.replace(Some((player, col)));
            })
            .child(cell)
            .into_element()
    }

    /// A cell's text, ellipsized, in its role's color, with its hover text.
    fn plain_cell(&self, col: ReplayColumn, cell: CellValue) -> AnyElement {
        let table = self.table;
        let mut label = text(cell.text).app_class(class("text-sm text-foreground truncate"));
        if let Some(role) = cell.color {
            label = label.text_color(colors::role(role));
        }

        let base = view()
            .app_class(class("flex-row flex-none items-center px-1 overflow-hidden"))
            .w(move || Length::from(px(table.width(col))))
            .child(label);
        match cell.hover {
            Some(hover) => base.tooltip(tip_lines(&hover)).into_element(),
            None => base.into_element(),
        }
    }

    /// The expand control, the class icon (or its name), division, clan and
    /// name, then the hidden-profile, Twitch and connection marks.
    fn name_cell(&self) -> AnyElement {
        let table = self.table;
        let row = self.row;
        let player = row.db_id;

        let (caret, caret_label) = if self.is_expanded {
            (Icon::ChevronDown, format!("Collapse {}", row.display_name))
        } else {
            (Icon::ChevronRight, format!("Expand {}", row.display_name))
        };
        let mut cell = view()
            .app_class(class("flex-1 min-w-0 flex-row items-center gap-1 overflow-hidden"))
            .child(glyph_button(caret, caret_label).on_press(move |_: &PressEvent| table.toggle_expanded(player)));

        cell = cell.child(match self.icons.ship_class(row.ship_class) {
            Some(class_icon) => {
                let icon = class_icon.element(
                    colors::player(player_color_kind(row)),
                    SHIP_ICON_WIDTH,
                    row.ship_species_text.clone(),
                );
                if row.ship_species_text.is_empty() {
                    icon.into_element()
                } else {
                    icon.tooltip(tip(row.ship_species_text.clone())).into_element()
                }
            }
            None => {
                text(row.ship_species_text.clone()).app_class(class("flex-none text-xs text-foreground")).into_element()
            }
        });

        if let Some(division) = row.division_label.as_ref() {
            cell = cell.child(text(division.clone()).app_class(class("flex-none text-sm text-foreground")));
        }
        if let Some(clan) = row.clan.as_ref() {
            cell = cell.child(
                text(clan.tag.clone()).app_class(class("flex-none text-sm")).text_color(colors::fixed(clan.color_rgb)),
            );
        }
        cell = cell.child(
            text(row.display_name.clone())
                .app_class(class("flex-1 min-w-0 text-sm truncate"))
                .text_color(colors::player(name_color_kind(row))),
        );

        if row.is_hidden_profile {
            let hidden = t!("ui.replay.player.hidden_profile").into_owned();
            cell = cell.child(
                view()
                    .accessible(true)
                    .label(hidden.clone())
                    .app_class(class("flex-none text-muted-foreground"))
                    .tooltip(tip(hidden))
                    .child(icon(Icon::EyeOff, px(14.0))),
            );
        }
        if let Some(chip) = self.twitch_chip() {
            cell = cell.child(chip);
        }
        if let Some(note) = row.connection.as_ref() {
            let hover = connection_hover_text(note);
            cell = cell.child(
                view()
                    .accessible(true)
                    .label(hover.clone())
                    .app_class(class("flex-none text-muted-foreground"))
                    .tooltip(tip(hover))
                    .child(icon(Icon::Plug, px(14.0))),
            );
        }

        view()
            .app_class(class("flex-row flex-none items-center px-1 overflow-hidden"))
            .pr(px(ROW_ACTIONS_WIDTH + 4.0))
            .w(move || Length::from(px(table.width(ReplayColumn::Name))))
            .child(cell)
            .into_element()
    }

    /// The Twitch mark: one candidate copies its login on a press, several
    /// open a menu to pick which.
    fn twitch_chip(&self) -> Option<AnyElement> {
        let candidates = &self.row.twitch_candidates;
        let hover = twitch_hover_text(candidates)?;
        let table = self.table;

        if let [only] = candidates.as_slice() {
            let login = only.login.clone();
            return Some(
                glyph_button(Icon::Twitch, hover.clone())
                    .tooltip(tip(hover))
                    .on_press(move |_: &PressEvent| copy_login(table, &login))
                    .into_element(),
            );
        }

        let trigger = glyph_button(Icon::Twitch, hover.clone()).tooltip(tip(hover));
        Some(
            menu()
                .trigger(trigger)
                .popup_class(MENU_POPUP)
                .children(
                    candidates
                        .iter()
                        .map(|SniperCandidate { login, .. }| {
                            let login = login.clone();
                            menu_entry(login.clone(), Some(Icon::Copy), move || copy_login(table, &login))
                        })
                        .collect::<Vec<_>>(),
                )
                .into_element(),
        )
    }

    /// The Skills cell: the Dazzle, Incoming Fire Alert and build-warning
    /// marks in the tier's color before the label, where the build shows.
    fn skills_cell(&self) -> AnyElement {
        let table = self.table;
        let row = self.row;
        let cell = cell_value(row, ReplayColumn::Skills, self.debug);
        let shows_build = row.has_vehicle_entity && (!row.relation.is_enemy() || self.debug);
        if !shows_build {
            return self.plain_cell(ReplayColumn::Skills, cell);
        }

        let Some(role) = cell.color else {
            return self.plain_cell(ReplayColumn::Skills, cell);
        };
        let color = colors::role(role);
        let mut marks = Vec::new();
        if row.has_dazzle {
            marks.push(Icon::StarFilled);
        }
        if row.has_ifa {
            marks.push(Icon::Bell);
        }
        match row.skill_warning {
            Some(SkillWarning::TowerDefense) => marks.push(Icon::Castle),
            Some(SkillWarning::NoHighTier) => marks.push(Icon::TriangleAlert),
            None => {}
        }

        let base = view()
            .app_class(class("flex-row flex-none items-center gap-1 px-1 overflow-hidden"))
            .w(move || Length::from(px(table.width(ReplayColumn::Skills))))
            .text_color(color)
            .children(
                marks
                    .into_iter()
                    .map(|mark| icon(mark, px(14.0)).app_class(class("flex-none")).into_element())
                    .collect::<Vec<_>>(),
            )
            .child(text(cell.text).app_class(class("text-sm truncate")).text_color(color));
        match cell.hover {
            Some(hover) => base.tooltip(tip_lines(&hover)).into_element(),
            None => base.into_element(),
        }
    }

    /// The dots at the end of the name, shown while the pointer is on the
    /// row, opening the row's menu.
    fn row_actions(&self) -> AnyElement {
        let table = self.table;
        let trigger = view()
            .accessible(true)
            .role(AccessibilityRole::Button)
            .label("Player actions")
            .tooltip(tip(t!("ui.replay.row_actions_hint").into_owned()))
            .app_class(class(
                "items-center justify-center w-6 h-6 rounded-(--radius) text-muted-foreground opacity-0 \
                 group-hover:opacity-100 aria-expanded:opacity-100 hover:bg-accent hover:text-foreground",
            ))
            .child(icon(Icon::Ellipsis, px(14.0)));
        let data = Rc::new(MenuData::from_row(self.row, self.icons, self.debug));

        view()
            .app_class(class("absolute top-0"))
            .left(move || Length::from(px((table.width(ReplayColumn::Name) - ROW_ACTIONS_WIDTH).max(0.0))))
            .child(menu().trigger(trigger).popup_class(MENU_POPUP).children(menu_items(
                table,
                data,
                MenuOrigin::RowDots,
                signal(None),
            )))
            .into_element()
    }
}

fn copy_login(table: Table, login: &str) {
    set_clipboard(login.to_owned());
    table.notice(t!("ui.twitch.copied", name = login).into_owned());
}

/// What a row's menu reads of the row.
struct MenuData {
    player: AccountId,
    heading: MenuHeading,
    shows_build_links: bool,
    ship_config_url: Option<String>,
    short_ship_config_url: Option<String>,
    shiptool_url: Option<String>,
    wows_numbers_url: Option<String>,
    raw_metadata_json: Option<String>,
}

/// Who a menu is about, drawn as its first item: a right-click menu lands
/// wherever the pointer was, and the rows are one line apart.
struct MenuHeading {
    clan: Option<(String, AdaptiveColor)>,
    name: String,
    name_color: AdaptiveColor,
    /// The class icon, tinted as the Name cell draws it, else its name.
    class_icon: Option<(ClassIcon, AdaptiveColor)>,
    species: String,
    ship: Option<String>,
    spoken: String,
}

impl MenuData {
    fn from_row(row: &PlayerRow, icons: &TableIcons, debug: bool) -> Self {
        let heading = MenuHeading {
            clan: row.clan.as_ref().map(|clan| (clan.tag.clone(), colors::fixed(clan.color_rgb))),
            name: row.display_name.clone(),
            name_color: colors::player(name_color_kind(row)),
            class_icon: icons
                .ship_class(row.ship_class)
                .map(|icon| (icon.clone(), colors::player(player_color_kind(row)))),
            species: row.ship_species_text.clone(),
            ship: (!row.ship_name.is_empty()).then(|| row.ship_name.clone()),
            spoken: menu_heading_text(row),
        };

        Self {
            player: row.db_id,
            heading,
            shows_build_links: shows_build_links(row, debug),
            ship_config_url: row.ship_config_url.clone(),
            short_ship_config_url: row.short_ship_config_url.clone(),
            shiptool_url: row.shiptool_url.clone(),
            wows_numbers_url: row.wows_numbers_url.clone(),
            raw_metadata_json: debug.then(|| row.raw_metadata_json.clone()).flatten(),
        }
    }
}

fn heading_view(heading: &MenuHeading) -> AnyElement {
    let mut line = view().app_class(class("flex-row items-center gap-1"));
    if let Some((clan, color)) = heading.clan.clone() {
        line = line.child(text(clan).app_class(class("flex-none text-xs")).text_color(color));
    }
    line = line.child(text(heading.name.clone()).app_class(class("text-xs truncate")).text_color(heading.name_color));

    if let Some(ship) = heading.ship.clone() {
        line = match heading.class_icon.clone() {
            Some((icon, tint)) => line.child(icon.element(tint, SHIP_ICON_WIDTH, heading.species.clone())),
            None if !heading.species.is_empty() => {
                line.child(text(heading.species.clone()).app_class(class("flex-none text-xs text-foreground")))
            }
            None => line,
        };
        line = line.child(text(ship).app_class(class("text-xs truncate text-muted-foreground")));
    }

    line.into_element()
}

/// The row menu's items, as the dots and a right-click offer them.
fn menu_items(
    table: Table,
    data: Rc<MenuData>,
    origin: MenuOrigin,
    menu_cell: Signal<Option<ReplayColumn>>,
) -> Vec<AnyElement> {
    let player = data.player;
    let mut items = vec![
        menu_item()
            .disabled(true)
            .accessibility_label(data.heading.spoken.clone())
            .app_class(class("flex-row items-center min-h-7 px-2"))
            .child(heading_view(&data.heading))
            .into_element(),
        menu_rule(),
    ];

    let mut wrote_links = false;
    if data.shows_build_links {
        let mut build_links = Vec::new();
        if let Some(url) = data.shiptool_url.clone() {
            build_links.push(link_entry(t!("ui.replay.build.open_shiptool").into_owned(), url));
        }
        if let Some(url) = data.ship_config_url.clone() {
            build_links.push(link_entry(t!("ui.replay.build.open_in_browser").into_owned(), url.clone()));
            build_links.push(menu_entry(t!("ui.replay.build.copy_link").into_owned(), Some(Icon::Copy), move || {
                copy_link(table, &url)
            }));
        }
        if let Some(url) = data.short_ship_config_url.clone() {
            build_links.push(menu_entry(
                t!("ui.replay.build.copy_short_link").into_owned(),
                Some(Icon::Copy),
                move || copy_link(table, &url),
            ));
        }
        if !build_links.is_empty() {
            items.extend(build_links);
            items.push(menu_rule());
            wrote_links = true;
        }
    }
    if let Some(url) = data.wows_numbers_url.clone() {
        items.push(link_entry(t!("ui.replay.build.open_wows_numbers").into_owned(), url));
        wrote_links = true;
    }
    if wrote_links {
        items.push(menu_rule());
    }

    if origin == MenuOrigin::RowRightClick {
        items.push(
            when(
                move || menu_cell.with(Option::is_some),
                move || {
                    menu_entry(t!("ui.replay.context.copy_cell").into_owned(), Some(Icon::Copy), move || {
                        if let Some(col) = menu_cell.get_untracked() {
                            table.copy_cell(player, col);
                        }
                    })
                },
            )
            .into_element(),
        );
    }
    items.push(menu_entry(t!("ui.replay.context.copy_row").into_owned(), Some(Icon::Copy), move || {
        table.copy_row(player)
    }));
    items.push(menu_entry(t!("ui.replay.context.copy_table").into_owned(), Some(Icon::Copy), move || {
        table.copy_table()
    }));
    items.push(menu_entry("Copy row as Markdown", Some(Icon::Copy), move || table.copy_row_markdown(player)));
    items.push(menu_entry("Copy table as Markdown", Some(Icon::Copy), move || table.copy_table_markdown()));

    if let Some(json) = data.raw_metadata_json.clone() {
        items.push(menu_rule());
        items.push(menu_entry(t!("ui.replay.debug.view_raw_metadata").into_owned(), Some(Icon::File), move || {
            table.cx.emit(super::PlayerTableEvent::ViewRawJson(SharedString::from(json.clone())))
        }));
    }

    items
}

/// An item that opens `url` in the browser.
fn link_entry(label: String, url: String) -> AnyElement {
    menu_entry(label, Some(Icon::ExternalLink), move || {
        // The URL opens when asked; whether it did is not reported here.
        spawn(open_url(url.clone()));
    })
}

fn copy_link(table: Table, url: &str) {
    set_clipboard(url.to_owned());
    table.notice(t!("ui.replay.build.link_copied").into_owned());
}

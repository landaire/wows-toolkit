//! A row's actions: the menu a right-click on the row opens, which the dots at the end of the
//! player's name open too.

use std::sync::Arc;

use landaire_ui::MenuItems;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::AccountId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::columns::cell_value;
use wows_toolkit_replay_data::columns::markdown_table;
use wows_toolkit_replay_data::columns::name_color_kind;
use wows_toolkit_replay_data::columns::row_text;
use wows_toolkit_replay_data::columns::table_text;
use wows_toolkit_replay_data::model::PlayerRow;
use zgui::prelude::AnyView;
use zgui::reactive::Callable;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::EventCx;
use zgui::view::NodeRef;
use zgui::view::events;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuLabelProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::ELLIPSIS;
use zgui_ui_primitives::Binding;

use crate::desktop;
use crate::icons::AppIconProps;
use crate::tabs::replays::table::TableCx;
use crate::tabs::replays::table::cells::clan_ink;
use crate::tabs::replays::table::cells::class_mark;
use crate::tabs::replays::table::cells::player_color;

/// What builds `player`'s menu. Nothing when the player is no longer in the table.
pub(super) fn row_menu(player: AccountId, cx: TableCx) -> Option<MenuItems> {
    let row = cx
        .rows
        .with_untracked(|rows| rows.iter().find(|row| row.row.db_id == player).map(|row| Arc::clone(&row.row)))?;

    Some(MenuItems::new(move || row_menu_items(Arc::clone(&row), player, cx)))
}

/// The items of `player`'s menu, read from `row`.
fn row_menu_items(row: Arc<PlayerRow>, player: AccountId, cx: TableCx) -> AnyView {
    let debug = cx.debug.get_untracked();
    let mut items: Vec<AnyView> = vec![heading(&row, cx), AnyView::new(view! { MenuSeparator() })];

    let show_ship_config = (!row.relation.is_enemy() || debug) && row.has_vehicle_entity;
    let mut wrote_links = false;
    if show_ship_config {
        let mut added_any = false;
        if let Some(url) = row.shiptool_url.clone() {
            items.push(link(t!("ui.replay.build.open_shiptool").into_owned(), url));
            added_any = true;
        }
        if let Some(url) = row.ship_config_url.clone() {
            items.push(link(t!("ui.replay.build.open_in_browser").into_owned(), url.clone()));
            items.push(copy_item(t!("ui.replay.build.copy_link").into_owned(), cx, move || {
                Some((url.clone(), t!("ui.replay.build.link_copied").into_owned()))
            }));
            added_any = true;
        }
        if let Some(url) = row.short_ship_config_url.clone() {
            items.push(copy_item(t!("ui.replay.build.copy_short_link").into_owned(), cx, move || {
                Some((url.clone(), t!("ui.replay.build.link_copied").into_owned()))
            }));
            added_any = true;
        }
        if added_any {
            items.push(AnyView::new(view! { MenuSeparator() }));
        }
        wrote_links |= added_any;
    }

    if let Some(url) = row.wows_numbers_url.clone() {
        items.push(link(t!("ui.replay.build.open_wows_numbers").into_owned(), url));
        wrote_links = true;
    }
    if wrote_links {
        items.push(AnyView::new(view! { MenuSeparator() }));
    }

    if let Some((_, col)) = cx.noted.current().filter(|(noted, _)| *noted == player) {
        let row = Arc::clone(&row);
        items.push(copy_item(t!("ui.replay.context.copy_cell").into_owned(), cx, move || {
            Some((cell_value(&row, col, cx.debug.get_untracked()).text, copied()))
        }));
    }
    {
        let row = Arc::clone(&row);
        items.push(copy_item(t!("ui.replay.context.copy_row").into_owned(), cx, move || {
            Some((row_text(&row, &cx.drawn_columns(), cx.debug.get_untracked()), copied()))
        }));
    }
    items.push(copy_item(t!("ui.replay.context.copy_table").into_owned(), cx, move || {
        let text = cx.rows.with_untracked(|rows| {
            table_text(rows.iter().map(|row| row.row.as_ref()), &cx.drawn_columns(), cx.debug.get_untracked())
        });
        Some((text, copied()))
    }));
    {
        let row = Arc::clone(&row);
        items.push(copy_item("Copy row as Markdown".to_owned(), cx, move || {
            Some((
                markdown_table(std::iter::once(row.as_ref()), &cx.drawn_columns(), cx.debug.get_untracked()),
                copied(),
            ))
        }));
    }
    items.push(copy_item("Copy table as Markdown".to_owned(), cx, move || {
        let text = cx.rows.with_untracked(|rows| {
            markdown_table(rows.iter().map(|row| row.row.as_ref()), &cx.drawn_columns(), cx.debug.get_untracked())
        });
        Some((text, copied()))
    }));

    if debug
        && let Some(json) = row.raw_metadata_json.clone()
        && let Some(view_json) = cx.on_view_raw_json
    {
        let caption = t!("ui.replay.debug.view_raw_metadata").into_owned();
        let open = UnsyncCallback::new(move |()| view_json.run(json.clone()));
        items.push(AnyView::new(view! { MenuSeparator() }));
        items.push(AnyView::new(view! {
            MenuItem(on_select = open) {
                box(class = "wt-pt-item-icon") { AppIcon(glyph = Glyph::File) }
                {caption.clone()}
            }
        }));
    }

    AnyView::new(view! { {items} })
}

fn copied() -> String {
    t!("ui.replay.context.copied").into_owned()
}

/// Who the menu is about, first: a right-click menu lands wherever the pointer was, and the rows
/// it could have come from are one line apart.
fn heading(row: &Arc<PlayerRow>, cx: TableCx) -> AnyView {
    let clan = row.clan_tag.clone().filter(|clan| !clan.is_empty()).map(|clan| {
        let ink = clan_ink(row, cx);
        AnyView::new(view! { text({..ink}) {{clan}} })
    });
    let name = row.display_name.clone();
    let name_color = player_color(name_color_kind(row));
    let ship = (!row.ship_name.is_empty()).then(|| {
        let class = class_mark(row, cx);
        let ship = row.ship_name.clone();
        AnyView::new(view! {
            {class}
            text(class = "wt-pt-heading__ship", attr:data-testid = "player-menu-ship") {{ship}}
        })
    });

    AnyView::new(view! {
        MenuLabel {
            box(class = "wt-pt-heading", attr:data-testid = "player-menu-heading") {
                {clan}
                text(style:color = name_color, attr:data-testid = "player-menu-name") {{name}}
                {ship}
            }
        }
    })
}

/// An item that opens `url` in the browser.
fn link(caption: String, url: String) -> AnyView {
    let open = UnsyncCallback::new(move |()| desktop::open_url(&url));

    AnyView::new(view! {
        MenuItem(on_select = open) {
            box(class = "wt-pt-item-icon") { AppIcon(glyph = Glyph::SquareArrowOutUpRight) }
            {caption.clone()}
        }
    })
}

/// An item that copies the text `produce` gives and says what it copied.
fn copy_item(caption: String, cx: TableCx, produce: impl Fn() -> Option<(String, String)> + 'static) -> AnyView {
    let copy = UnsyncCallback::new(move |()| {
        if let Some((text, said)) = produce() {
            cx.copy(text, said);
        }
    });

    AnyView::new(view! {
        MenuItem(on_select = copy) {
            box(class = "wt-pt-item-icon") { AppIcon(glyph = Glyph::Copy) }
            {caption.clone()}
        }
    })
}

/// The dots that open the row's actions, shown while the pointer is on the row or the menu is
/// open. The Name cell holds their room so nothing moves when they appear.
pub(super) fn row_actions(player: AccountId, cx: TableCx) -> AnyView {
    let anchor = NodeRef::new();
    let hint = t!("ui.replay.row_actions_hint").into_owned();
    let label = hint.clone();
    let menus = cx.menus;
    // The tip stays shut while the menu it names is open over it.
    let tip_wanted = RwSignal::new_local(false);
    let tip_open = Signal::derive_local(move || tip_wanted.get() && menus.open_row() != Some(player));
    // A press on the dots is theirs, not a press on the row, and the menu it opens has no cell.
    let open = move |ev: &mut EventCx<'_, events::Click>| {
        ev.stop_propagation();
        cx.noted.clear();
        menus.open(player, anchor);
    };
    let keep_press = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation();
    let keep_double = |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation();

    AnyView::new(view! {
        box(
            class = "wt-pt-actions",
            attr:data-open = move || (menus.open_row() == Some(player)).then(String::new),
            on:pointer_down = keep_press,
            on:double_click = keep_double
        ) {
            Tooltip(open = Binding::controlled(tip_open, move |on: bool| tip_wanted.set(on))) {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        node_ref = anchor,
                        on:click = open,
                        attr:data-testid = format!("player-actions-{}", player.raw()),
                        a11y:label = label
                    ) {
                        Icon(icon = ELLIPSIS)
                    }
                }
                TooltipContent {{hint.clone()}}
            }
        }
    })
}

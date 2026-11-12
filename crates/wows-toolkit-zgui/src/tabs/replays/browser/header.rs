//! The listing's header: its caption, the selected directory's search and index actions, the
//! listing's options, and the open actions.

use std::path::PathBuf;

use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_viewmodel::settings::keys;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SeparatorProps;
use zgui_ui::ToggleGroupItemProps;
use zgui_ui::ToggleGroupProps;
use zgui_ui::ToggleSize;
use zgui_ui::ToggleVariant;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::services::GameDataState;
use crate::services::Services;
use crate::shell::palette::open_replay_file;
use crate::tabs::replays::browser::BrowserEvents;
use crate::tabs::replays::browser::ReplayBrowserEvent;
use crate::tabs::replays::browser::actions;
use crate::tabs::replays::browser::actions::Offer;
use crate::tabs::replays::browser::folders::BrowserControl;

const SHEET: &str = "wt-replay-listing-header";

// The options are 300px wide, which is where a long path wraps.
const CSS: &str = css!(
    ".wt-listing-header {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-listing-header__caption {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 14px;
        font-weight: 700;
    }
    .wt-listing-options {
        display: flex;
        flex-direction: column;
        gap: 4px;
        width: 300px;
        padding: 4px;
    }
    .wt-listing-options__path {
        display: flex;
        align-items: flex-start;
        gap: 4px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
        user-select: text;
        overflow-wrap: anywhere;
    }
    .wt-listing-options__item { display: flex; flex-direction: column; gap: 2px; }
    .wt-listing-options__action { width: 100%; justify-content: flex-start; }
    .wt-listing-options__hint { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-listing-options__title { font-size: 14px; font-weight: 700; }
    .wt-listing-options__check { display: flex; align-items: center; gap: 8px; font-size: 14px; }"
);

/// The grouping choices, in the egui header's order.
const GROUPINGS: [ReplayGrouping; 3] = [ReplayGrouping::Date, ReplayGrouping::Ship, ReplayGrouping::None];

/// One optional column the options show or hide: those with a column in the replay table that
/// the egui header lets the reader toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColumnFilter {
    RawXp,
    ObservedDamage,
    Heals,
}

impl ColumnFilter {
    fn label(self) -> String {
        match self {
            Self::RawXp => t!("stat.raw_xp"),
            Self::ObservedDamage => t!("ui.replay.column.observed_damage"),
            Self::Heals => t!("ui.replay.column.heals"),
        }
        .into_owned()
    }

    fn test_id(self) -> &'static str {
        match self {
            Self::RawXp => "replay-listing-filter-raw-xp",
            Self::ObservedDamage => "replay-listing-filter-observed-damage",
            Self::Heals => "replay-listing-filter-heals",
        }
    }

    fn shown(self, settings: &ReplaySettings) -> bool {
        match self {
            Self::RawXp => settings.show_raw_xp,
            Self::ObservedDamage => settings.show_observed_damage,
            Self::Heals => settings.show_heals,
        }
    }

    fn set(self, settings: &mut ReplaySettings, shown: bool) {
        match self {
            Self::RawXp => settings.show_raw_xp = shown,
            Self::ObservedDamage => settings.show_observed_damage = shown,
            Self::Heals => settings.show_heals = shown,
        }
    }
}

#[component]
pub fn ListingHeader(control: BrowserControl, events: BrowserEvents) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let search_offer = Signal::derive_local(move || {
        locale.track();
        actions::search_offer(control)
    });
    let index_offer = Signal::derive_local(move || {
        locale.track();
        actions::index_offer(control)
    });
    let search_tip =
        Signal::derive_local(move || offer_tip(search_offer.get(), t!("ui.replay.search_these_replays").into_owned()));
    let index_tip =
        Signal::derive_local(move || offer_tip(index_offer.get(), t!("ui.index_folder.tooltip").into_owned()));

    view! {
        row(class = "wt-listing-header", attr:data-testid = "replay-listing-header") {
            text(class = "wt-listing-header__caption") {{locale.text("ui.replay.listing_caption")}}
            HeaderAction(
                glyph = Glyph::Search,
                offer = search_offer,
                tip = search_tip,
                test_id = "replay-listing-search",
                on_press = UnsyncCallback::new(move |()| actions::search_selected_folder(control))
            )
            HeaderAction(
                glyph = Glyph::Database,
                offer = index_offer,
                tip = index_tip,
                test_id = "replay-listing-index",
                on_press = UnsyncCallback::new(move |()| actions::index_selected_folder(control))
            )
            ListingOptions(control = control, events = events)
            OpenMenu(control = control)
        }
    }
}

/// What an action's tooltip says: what it does, or why it is not offered.
fn offer_tip(offer: Offer, enabled: String) -> String {
    match offer {
        Offer::Enabled => enabled,
        Offer::Disabled(reason) => reason,
    }
}

/// An icon action, disabled with the reason as its tooltip when it is not offered.
#[component]
fn HeaderAction(
    glyph: Glyph,
    offer: Signal<Offer, LocalStorage>,
    tip: Signal<String, LocalStorage>,
    test_id: &'static str,
    on_press: UnsyncCallback<()>,
) -> impl IntoView {
    let disabled = Signal::derive_local(move || offer.with(|offer| matches!(offer, Offer::Disabled(_))));

    view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    disabled = disabled,
                    attr:data-testid = test_id,
                    a11y:label = move || tip.get(),
                    on:click = move |_| on_press.run(())
                ) {
                    AppIcon(glyph = glyph)
                }
            }
            TooltipContent {{move || tip.get()}}
        }
    }
}

/// The listing's options: the selected directory's path and the actions on it as a whole, then
/// autoload, grouping and the column filters of the egui app's replay header.
///
/// Grouping is a row of exclusive toggles rather than a select: a select's list is drawn outside
/// the popover, and a press on it would dismiss the popover before the choice landed.
#[component]
fn ListingOptions(control: BrowserControl, events: BrowserEvents) -> impl IntoView {
    let services = Services::current();
    let shell = control.shell();
    let locale = services.locale;
    let open = RwSignal::new_local(false);
    let events = StoredValue::new_local(events);
    let directory = Signal::derive_local(move || {
        let selected = control.selected.get();
        let listing = control.listings.with(|listings| listings.get(&selected).copied())?;
        listing.directory.get()
    });
    let ready = Signal::derive_local(move || matches!(services.game_data.state(), GameDataState::Ready(_)));
    let tip = locale.text("ui.replay.listing_options");

    let check_game_data = move |_: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
        let missing = control.selected_listing().map(|listing| listing.missing_builds()).unwrap_or_default();
        events.with_value(|events| events.send(ReplayBrowserEvent::CheckGameData(missing)));
        open.set(false);
    };
    let path_line = move || directory.get().map(|path| AnyView::new(view! { FolderPathLine(path = path) }));
    let stats = move || {
        let path = directory.get()?;
        let stats_tip = move || {
            locale.track();
            if ready.get() {
                t!("ui.stats.open_directory_stats_tooltip").into_owned()
            } else {
                t!("ui.stats.directory_stats_unavailable").into_owned()
            }
        };
        let path = StoredValue::new_local(path);
        Some(AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Sm,
                        class = "wt-listing-options__action",
                        disabled = Signal::derive_local(move || !ready.get()),
                        attr:data-testid = "directory-stats",
                        on:click = move |_| {
                            shell.open_directory_stats(path.get_value());
                            open.set(false);
                        }
                    ) {
                        AppIcon(glyph = Glyph::ChartColumn)
                        text {{locale.text("ui.stats.open_directory_stats")}}
                    }
                }
                TooltipContent {{stats_tip}}
            }
        }))
    };
    let auto_load = Signal::derive_local(move || {
        // Unchecked until the settings are read, as the setting's own default is.
        let on = services.settings.read(|settings| settings.auto_load_latest_replay).unwrap_or(false);
        if on { Checked::Yes } else { Checked::No }
    });
    let set_auto_load = move |checked: Checked| {
        let on = checked == Checked::Yes;
        services.settings.edit(keys::AUTO_LOAD_LATEST_REPLAY, |settings| {
            settings.auto_load_latest_replay = on;
            on
        });
    };
    let grouping = Signal::derive_local(move || vec![grouping_value(control.grouping.get()).to_owned()]);
    let set_grouping = move |values: Vec<String>| {
        // A press on the grouping already chosen would leave none; the choice stays.
        let Some(grouping) =
            values.first().and_then(|value| GROUPINGS.into_iter().find(|offered| grouping_value(*offered) == value))
        else {
            return;
        };
        control.grouping.set(grouping);
        services.settings.edit(keys::REPLAY_SETTINGS, |settings| {
            settings.replay.grouping = grouping;
            settings.replay.clone()
        });
    };

    view! {
        Tooltip {
            TooltipTrigger {
                Popover(open = open) {
                    PopoverTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "replay-listing-options",
                        a11y:label = tip
                    ) {
                        AppIcon(glyph = Glyph::Settings)
                    }
                    PopoverContent(
                        placement = Placement::new(Side::Bottom, Align::End),
                        class = "wt-listing-options",
                        attr:data-testid = "replay-listing-options-menu"
                    ) {
                        {path_line}
                        column(class = "wt-listing-options__item") {
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::Sm,
                                class = "wt-listing-options__action",
                                attr:data-testid = "directory-game-data",
                                on:click = check_game_data
                            ) {
                                AppIcon(glyph = Glyph::Download)
                                text {{locale.text("ui.replay.check_game_data")}}
                            }
                            text(class = "wt-listing-options__hint") {{locale.text("ui.replay.check_game_data_tooltip")}}
                        }
                        {stats}
                        Separator()
                        Tooltip {
                            TooltipTrigger {
                                row(class = "wt-listing-options__check") {
                                    Checkbox(
                                        checked = Binding::controlled(auto_load, set_auto_load),
                                        attr:data-testid = "replay-listing-auto-load-latest",
                                        a11y:label = t!("ui.replay.autoload_latest").into_owned()
                                    )
                                    text {{locale.text("ui.replay.autoload_latest")}}
                                }
                            }
                            TooltipContent {{locale.text("ui.replay.autoload_latest_tooltip")}}
                        }
                        Separator()
                        text(class = "wt-listing-options__title") {{locale.text("ui.replay.group_label")}}
                        ToggleGroup(
                            value = Binding::controlled(grouping, set_grouping),
                            variant = ToggleVariant::Outline,
                            size = ToggleSize::Sm,
                            label = t!("ui.replay.group_label").into_owned(),
                            attr:data-testid = "replay-listing-grouping"
                        ) {
                            ToggleGroupItem(value = grouping_value(ReplayGrouping::Date), attr:data-testid = "replay-listing-grouping-date") {
                                {locale.text("ui.replay.group.date")}
                            }
                            ToggleGroupItem(value = grouping_value(ReplayGrouping::Ship), attr:data-testid = "replay-listing-grouping-ship") {
                                {locale.text("ui.replay.group.ship")}
                            }
                            ToggleGroupItem(value = grouping_value(ReplayGrouping::None), attr:data-testid = "replay-listing-grouping-none") {
                                {locale.text("ui.replay.group.none")}
                            }
                        }
                        Separator()
                        text(class = "wt-listing-options__title") {{locale.text("ui.replay.column_filters")}}
                        ColumnFilterCheck(filter = ColumnFilter::RawXp)
                        ColumnFilterCheck(filter = ColumnFilter::ObservedDamage)
                        ColumnFilterCheck(filter = ColumnFilter::Heals)
                    }
                }
            }
            TooltipContent {{tip}}
        }
    }
}

/// The value a grouping toggle carries.
const fn grouping_value(grouping: ReplayGrouping) -> &'static str {
    match grouping {
        ReplayGrouping::Date => "date",
        ReplayGrouping::Ship => "ship",
        ReplayGrouping::None => "none",
    }
}

/// The selected directory's whole path, selectable for copying. It wraps rather than truncating.
#[component]
fn FolderPathLine(path: PathBuf) -> impl IntoView {
    let shown = shown_path(&path);

    view! {
        row(class = "wt-listing-options__path", attr:data-testid = "replay-listing-folder-path", a11y:label = shown.clone()) {
            AppIcon(glyph = Glyph::Folder)
            text {{shown}}
        }
    }
}

/// One column filter's checkbox, writing the replay settings as it changes.
#[component]
fn ColumnFilterCheck(filter: ColumnFilter) -> impl IntoView {
    let services = Services::current();
    let checked = Signal::derive_local(move || {
        // Unchecked until the settings are read; the column follows them once they are.
        let shown = services.settings.read(|settings| filter.shown(&settings.replay)).unwrap_or(false);
        if shown { Checked::Yes } else { Checked::No }
    });
    let set = move |checked: Checked| {
        services.settings.edit(keys::REPLAY_SETTINGS, |settings| {
            filter.set(&mut settings.replay, checked == Checked::Yes);
            settings.replay.clone()
        });
    };
    let label = filter.label();

    view! {
        row(class = "wt-listing-options__check") {
            Checkbox(
                checked = Binding::controlled(checked, set),
                attr:data-testid = filter.test_id(),
                a11y:label = label.clone()
            )
            text {{label}}
        }
    }
}

/// The open actions: one replay file, or a directory of replays opened as a tab of its own.
#[component]
fn OpenMenu(control: BrowserControl) -> impl IntoView {
    let shell = control.shell();
    let locale = shell.services().locale;
    let tip = locale.text("ui.replay.open");

    view! {
        DropdownMenu {
            Tooltip {
                TooltipTrigger {
                    DropdownMenuTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "replay-listing-open",
                        a11y:label = tip
                    ) {
                        AppIcon(glyph = Glyph::FolderOpen)
                    }
                }
                TooltipContent {{tip}}
            }
            DropdownMenuContent(placement = Placement::new(Side::Bottom, Align::End)) {
                MenuItem(on_select = UnsyncCallback::new(move |()| open_replay_file(shell))) {
                    {t!("ui.replay.open_file").into_owned()}
                }
                MenuItem(on_select = UnsyncCallback::new(move |()| control.browse_folder())) {
                    {t!("ui.replay.open_directory").into_owned()}
                }
            }
        }
    }
}

//! What the egui app's menu bar carries, at the end of the strip rather than in a row of its own:
//! a row holding a few items above the tabs is a web page's chrome, and this is a desktop window.

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use zgui::component;
use zgui::prelude::try_use_windows;
use zgui::reactive::UnsyncCallback;
use zgui::view;
use zgui::view::IntoView;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::ELLIPSIS;

use crate::desktop;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::update;
use crate::shell::update::Asked;

/// Where the menu's links go, as the egui menu bar sends them.
pub const ISSUES_URL: &str = "https://github.com/landaire/wows-toolkit/issues/new/choose";
pub const DISCORD_URL: &str = "https://discord.gg/RJXjXHUj7rh";

/// The menu behind the ellipsis at the end of the strip.
#[component]
pub fn AppMenu(shell: Shell) -> impl IntoView {
    let services = Services::current();
    let locale = services.locale;
    let item = move |run: fn(Services, Shell)| UnsyncCallback::new(move |()| run(services, shell));

    // The menu sits inside the tooltip's trigger: a tooltip holds an overlay state of its own, so a
    // menu trigger beneath it would open the tooltip rather than the menu.
    view! {
        Tooltip {
            TooltipTrigger {
                DropdownMenu {
                    DropdownMenuTrigger(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconSm,
                        attr:data-testid = "app-menu",
                        a11y:label = locale.text("ui.menu.file")
                    ) {
                        Icon(icon = ELLIPSIS)
                    }
                    DropdownMenuContent {
                        MenuItem(
                            on_select = UnsyncCallback::new(move |()| shell.tactics.open()),
                            attr:data-testid = "app-menu-tactics-board"
                        ) {
                            {locale.text("ui.windows.tactics_board")}
                        }
                        MenuSeparator()
                        MenuItem(on_select = item(|services, shell| update::check_for_update(services, shell, Asked::ByHand))) {
                            {locale.text("ui.menu.check_updates")}
                        }
                        MenuItem(on_select = item(|_, shell| crate::shell::about::show_about(shell))) {
                            {locale.text("ui.menu.about")}
                        }
                        MenuSeparator()
                        MenuItem(on_select = item(|_, _| desktop::open_url(ISSUES_URL))) {
                            {locale.text("ui.buttons.create_issue")}
                        }
                        MenuItem(on_select = item(|_, _| desktop::open_url(DISCORD_URL))) {
                            {locale.text("ui.buttons.discord")}
                        }
                        MenuSeparator()
                        MenuItem(on_select = UnsyncCallback::new(move |()| {
                            if let Some(windows) = try_use_windows() {
                                windows.quit();
                            }
                        })) {
                            {locale.text("ui.menu.quit")}
                        }
                    }
                }
            }
            TooltipContent {{locale.text("ui.menu.file")}}
        }
    }
}

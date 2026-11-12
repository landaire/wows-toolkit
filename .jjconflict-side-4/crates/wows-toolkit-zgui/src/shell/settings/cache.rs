//! What a game-data cache holding at least one build offers: its size, the jobs that check it, and
//! whatever those jobs found.

use landaire_ui::TABULAR_CLASS;
use rust_i18n::t;
use zgui::component;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::SpinnerProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;

use crate::desktop;
use crate::shell::Shell;

/// The cache's size and its jobs. Drawn only while the cache holds a build, which is when the row
/// is listed.
#[component]
pub fn CachedBuilds(shell: Shell) -> impl IntoView {
    let settings = shell.services().settings;
    let cache = shell.cache;
    let base = move || {
        settings.read(|settings| wows_toolkit_services::game_data_cache::base(&settings.game_data_cache_dir)).flatten()
    };
    let busy = Signal::derive_local(move || cache.busy());
    let stats = move || cache.with(|cache| cache.stats);
    let summary = move || {
        stats().map(|stats| {
            t!(
                "ui.settings.wows.cache.stats",
                size = humansize::format_size(stats.total_bytes, humansize::BINARY),
                count = stats.version_count,
            )
            .into_owned()
        })
    };
    let several = Signal::derive_local(move || stats().is_some_and(|stats| stats.version_count > 1));
    let progress = move || cache.with(|cache| cache.progress.filter(|step| step.total > 0));
    let updates = move || cache.with(|cache| cache.updates.len());
    let repair = move || cache.with(|cache| cache.repair.len());
    let failure = move || cache.with(|cache| cache.failure.clone());
    let with_base = move |run: fn(Shell, std::path::PathBuf)| {
        UnsyncCallback::new(move |()| {
            if let Some(base) = base() {
                run(shell, base);
            }
        })
    };

    view! {
        column(class = "wt-settings__control-stack") {
            row(class = "wt-settings__control-row") {
                text(class = "wt-settings__line", attr:data-testid = "cache-stats") {{move || summary().unwrap_or_default()}}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "cache-open-folder",
                    on:click = move |_| {
                        if let Some(base) = base() {
                            desktop::open_directory(&base);
                        }
                    }
                ) {
                    {t!("ui.settings.wows.cache.open_folder").into_owned()}
                }
                // Nothing to prune while there is one build: the newest is kept.
                if move || several.get() {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        disabled = busy,
                        attr:data-testid = "cache-delete-old",
                        on:click = move |_| {
                            if let Some(base) = base() {
                                shell.delete_old_cache_versions(base);
                            }
                        }
                    ) {
                        {t!("ui.settings.wows.cache.delete_old").into_owned()}
                    }
                } else {}
            }
            row(class = "wt-settings__control-row") {
                Tooltip {
                    TooltipTrigger {
                        DropdownMenu {
                            DropdownMenuTrigger(
                                variant = ButtonVariant::Outline,
                                size = ButtonSize::Sm,
                                disabled = busy,
                                attr:data-testid = "cache-actions"
                            ) {
                                {t!("ui.settings.wows.cache.actions").into_owned()}
                            }
                            DropdownMenuContent {
                                MenuItem(on_select = with_base(|shell, base| shell.check_cache_for_updates(base))) {
                                    {t!("ui.settings.wows.cache.check_updates").into_owned()}
                                }
                                MenuItem(on_select = with_base(|shell, base| shell.validate_cache(base))) {
                                    {t!("ui.settings.wows.cache.validate").into_owned()}
                                }
                            }
                        }
                    }
                    TooltipContent {{t!("ui.settings.wows.cache.actions_tooltip").into_owned()}}
                }
                if move || busy.get() {
                    Spinner(label = t!("ui.settings.wows.cache.actions").into_owned())
                } else {}
            }
            {move || progress().map(|step| view! {
                text(class = "wt-settings__line wt-settings__line--dim", class = TABULAR_CLASS) {{format!("{} / {}", step.done, step.total)}}
            }.into_view())}
            if move || updates() > 0 {
                row(class = "wt-settings__control-row") {
                    text(class = "wt-settings__line", attr:data-testid = "cache-updates") {{move || {
                        t!("ui.settings.wows.cache.updates_available", count = updates()).into_owned()
                    }}}
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        disabled = busy,
                        attr:data-testid = "cache-update-all",
                        on:click = move |_| {
                            if let Some(base) = base() {
                                shell.fetch_cache_builds(base, cache.with(|cache| cache.updates.clone()));
                            }
                        }
                    ) {
                        {t!("ui.settings.wows.cache.update_all").into_owned()}
                    }
                }
            } else {}
            if move || repair() > 0 {
                row(class = "wt-settings__control-row") {
                    text(class = "wt-settings__line wt-settings__line--error", attr:data-testid = "cache-repair-needed") {{move || {
                        t!("ui.settings.wows.cache.repair_needed", count = repair()).into_owned()
                    }}}
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        disabled = busy,
                        attr:data-testid = "cache-repair",
                        on:click = move |_| {
                            if let Some(base) = base() {
                                shell.fetch_cache_builds(base, cache.with(|cache| cache.repair.clone()));
                            }
                        }
                    ) {
                        {t!("ui.settings.wows.cache.repair").into_owned()}
                    }
                }
            } else {}
            {move || failure().map(|reason| view! {
                text(class = "wt-settings__line wt-settings__line--dim wt-settings__line--error", attr:data-testid = "cache-failure") {{reason}}
            }.into_view())}
        }
    }
}

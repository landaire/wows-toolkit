//! The strip along the bottom of the window: the selected tab's panel toggles, the mode a held
//! modifier key puts it in, and what is running.
//!
//! One job is named outright; several are counted, and a hover lists them.

use landaire_ui::FadeInProps;
use landaire_ui::Fraction;
use landaire_ui::PanelSide;
use landaire_ui::RunningBarProps;
use landaire_ui::StatusBarEnd;
use landaire_ui::StatusBarProps;
use landaire_ui::StatusBarStart;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::count_up;
use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::SpinnerProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;

use crate::icons::AppIconProps;
use crate::services::Job;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::ModeHint;
use crate::shell::tabs::PanelToggle;
use crate::shell::tabs::panel_glyph;

const SHEET: &str = "wt-status-bar";

const CSS: &str = css!(
    ".wt-status-mode, .wt-status-work, .wt-status-work__line {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
    }
    .wt-status-work__line { gap: 8px; }
    .wt-status-mode__key {
        flex: none;
        padding: 0 4px;
        border: 1px solid var(--lui-color-border);
        border-radius: 3px;
        background-color: var(--lui-color-widget);
        color: var(--lui-color-text);
    }
    .wt-status-truncate {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-status-work .zui-spinner {
        width: 12px;
        height: 12px;
    }
    .wt-status-work__list {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }"
);

/// What one job says: its name, and for one that counts, how far it has got.
fn said(job: &Job) -> String {
    match job.progress {
        Some(progress) => {
            format!("{} {}", job.name, t!("ui.app.status_of", done = progress.done, total = progress.total))
        }
        None => job.name.clone(),
    }
}

/// The bar: the toggles for left and bottom panels on the left, then the held-key mode; the jobs,
/// then the toggles for right panels, at the right end, so each toggle sits on the side of the
/// panel it opens.
#[component]
pub fn ShellStatusBar(shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();

    let toggles = move || shell.selected_chrome().map(|chrome| chrome.toggles()).unwrap_or_default();
    let mode = move || {
        let mode = shell.selected_chrome()?.mode()?;
        mode.key.is_down(shell.held_modifiers()).then_some(mode)
    };
    let jobs = Signal::derive_local(move || services.jobs.running());

    view! {
        StatusBar(attr:data-testid = "app-status-strip") {
            StatusBarStart(slot) {
                for toggle in move || toggles().into_iter().filter(|toggle| toggle.side != PanelSide::Right).collect::<Vec<_>>(), key = |toggle: &PanelToggle| toggle.test_id {
                    StatusPanelToggle(toggle = toggle)
                }
            }
            {move || mode().map(|mode| view! { ModeIndicator(mode = mode) }.into_view())}
            StatusBarEnd(slot) {
                {move || (!jobs.get().is_empty()).then(|| view! { Work(jobs = jobs) }.into_view())}
                for toggle in move || toggles().into_iter().filter(|toggle| toggle.side == PanelSide::Right).collect::<Vec<_>>(), key = |toggle: &PanelToggle| toggle.test_id {
                    StatusPanelToggle(toggle = toggle)
                }
            }
        }
    }
}

/// A toggle that stays pressed while its panel shows.
#[component]
fn StatusPanelToggle(toggle: PanelToggle) -> impl IntoView {
    let locale = Services::current().locale;
    let PanelToggle { test_id, name, side, open } = toggle;
    let tip = move || {
        locale.track();
        if open.get() {
            t!("ui.app.status_hide_panel", panel = name).into_owned()
        } else {
            t!("ui.app.status_show_panel", panel = name).into_owned()
        }
    };
    let tooltip = tip.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                Toggle(
                    pressed = open,
                    size = ToggleSize::Sm,
                    label = tip(),
                    class = "lui-status-toggle",
                    attr:data-testid = test_id
                ) {
                    AppIcon(glyph = panel_glyph(side))
                }
            }
            TooltipContent {{tooltip.clone()}}
        }
    }
}

/// The held key as a key cap, then the name of the mode it selects.
#[component]
fn ModeIndicator(mode: ModeHint) -> impl IntoView {
    let key = mode.key.label();
    let name = t!(mode.name_key).into_owned();
    let label = format!("{key}: {name}");

    view! {
        row(class = "wt-status-mode", attr:data-testid = "app-status-mode", a11y:label = label) {
            text(class = "wt-status-mode__key") {{key}}
            text(class = "wt-status-truncate") {{name}}
        }
    }
}

/// One job by name; several by count, listed on hover.
#[component]
fn Work(jobs: Signal<Vec<Job>, LocalStorage>) -> impl IntoView {
    let locale = Services::current().locale;
    let summary = move || {
        locale.track();
        match jobs.get().as_slice() {
            [only] => said(only),
            several => t!("ui.app.status_tasks", count = several.len()).into_owned(),
        }
    };

    view! {
        FadeIn {
            row(class = "wt-status-work", attr:data-testid = "app-status-work", a11y:label = summary) {
                Spinner(label = summary())
                if move || jobs.get().len() > 1 {
                    Tooltip {
                        TooltipTrigger {
                            text(class = "wt-status-truncate", class = TABULAR_CLASS) {{summary}}
                        }
                        TooltipContent {
                            column(class = "wt-status-work__list") {
                                // Two jobs can say the same thing, so a row is keyed by its place.
                                for row in move || jobs.get().iter().map(said).enumerate().collect::<Vec<_>>(), key = |row: &(usize, String)| row.0 {
                                    text {{row.1}}
                                }
                            }
                        }
                    }
                } else {
                    JobLine(job = Signal::derive_local(move || jobs.get().first().cloned()))
                }
            }
        }
    }
}

/// The only job by name, and for one that counts, how far it has got beside a bar of the share
/// done. The count rises to each new figure rather than jumping to it; the bar follows the count.
#[component]
fn JobLine(job: Signal<Option<Job>, LocalStorage>) -> impl IntoView {
    let locale = Services::current().locale;
    let done = Signal::derive_local(move || job.get().and_then(|job| job.progress).map_or(0, |progress| progress.done));
    let shown = count_up(done);
    let line = move || {
        locale.track();
        let Some(job) = job.get() else {
            return String::new();
        };
        match job.progress {
            Some(progress) => format!(
                "{} {}",
                job.name,
                t!("ui.app.status_of", done = shown.get().min(progress.done), total = progress.total)
            ),
            None => job.name,
        }
    };
    let share = Signal::derive_local(move || {
        let progress = job.get().and_then(|job| job.progress);
        progress.map_or(Fraction::NONE, |progress| Fraction::of(shown.get().min(progress.done), progress.total))
    });
    let counting = move || job.get().is_some_and(|job| job.progress.is_some());
    let label = job.get_untracked().map(|job| job.name).unwrap_or_default();

    view! {
        row(class = "wt-status-work__line") {
            text(class = "wt-status-truncate", class = TABULAR_CLASS) {{line}}
            Show(when = counting) {
                RunningBar(done = share, label = label.clone())
            }
        }
    }
}

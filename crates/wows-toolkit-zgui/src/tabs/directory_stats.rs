//! A Stats tab over every replay in one directory.
//!
//! The replays are read off the UI thread by
//! `wows_toolkit_replay_data::directory_stats::read_directory` and folded into the games
//! [`StatsBody`] shows as they arrive. Closing the tab drops the reading task, which stops the read.

#[cfg(test)]
mod tests;

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::directory_stats::DirectoryStatsUpdate;
use wows_toolkit_replay_data::directory_stats::ReadState;
use wows_toolkit_replay_data::directory_stats::read_directory;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetGames;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::WithValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::Timers;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ProgressProps;
use zgui_ui::SpinnerProps;

use crate::http::USER_AGENT;
use crate::icons::AppIconProps;
use crate::services::Job;
use crate::services::Progress;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::tabs::DirectoryStatsId;
use crate::tabs::stats::StatsBodyProps;
use crate::tabs::stats::StatsScope;

const SHEET: &str = "wt-directory-stats";

const CSS: &str = css!(
    ".wt-directory-stats {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }
    .wt-directory-stats__header {
        display: flex;
        flex-direction: column;
        flex: none;
    }
    .wt-directory-stats__bar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        min-width: 0;
        padding: 4px 8px;
        background-color: var(--lui-color-surface);
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-directory-stats__root {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
    }
    .wt-directory-stats__path {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-directory-stats__rule {
        flex: none;
        width: 1px;
        height: 16px;
        margin: 0 4px;
        background-color: var(--lui-color-border);
    }
    .wt-directory-stats__status {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        font-size: 0.85em;
    }
    .wt-directory-stats__skipped {
        font-size: 0.85em;
        color: var(--wt-color-text-dim);
    }
    .wt-directory-stats__body {
        display: flex;
        flex-direction: column;
        flex: 1 1 0;
        min-height: 0;
    }"
);

/// Folds of a fast read are this far apart: each one re-sorts the set and refilters every panel.
const FOLD_INTERVAL: Duration = Duration::from_millis(250);

/// How far the read of a directory has got, and what it skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectoryRead {
    state: ReadState,
    skips: ReplaySetSkips,
}

impl DirectoryRead {
    fn started() -> Self {
        Self { state: ReadState::Walking, skips: ReplaySetSkips::default() }
    }

    fn progress(self) -> Option<ReplaySetProgress> {
        match self.state {
            ReadState::Scanning(progress) | ReadState::Reading(progress) => Some(progress),
            ReadState::Walking | ReadState::Finished => None,
        }
    }

    /// The translated status line, `None` once the read has finished.
    fn status(self) -> Option<String> {
        match self.state {
            ReadState::Walking => Some(t!("ui.stats.directory_walking").into_owned()),
            ReadState::Scanning(progress) => {
                Some(t!("ui.stats.directory_scanning", done = progress.done, total = progress.total).into_owned())
            }
            ReadState::Reading(progress) => {
                Some(t!("ui.stats.directory_reading", done = progress.done, total = progress.total).into_owned())
            }
            ReadState::Finished => None,
        }
    }
}

#[component]
pub fn DirectoryStatsTab(id: DirectoryStatsId) -> impl IntoView {
    let services = Services::current();
    let shell = Shell::current();
    let game_data = Signal::derive_local(move || services.game_data.cache());
    // Built once rather than from a closure: a closure builds the view again whenever anything
    // read while building it changes, and the dock is read there, so choosing a dock tab would
    // reset it.
    let body = shell
        .directory_stats(id)
        .map(|tab| AnyView::new(view! { DirectoryStats(root = tab.root, game_data = game_data) }));

    view! {
        box(class = "wt-directory-stats", attr:data-testid = format!("directory-stats-tab-{}", id.0)) {
            {body}
        }
    }
}

/// The header and the stats body over the replays in `root`, read against `game_data` once the
/// settings say where the install is, and again whenever another cache is adopted.
#[component]
fn DirectoryStats(
    root: PathBuf,
    #[prop(into)] game_data: Signal<Option<GameDataCache>, LocalStorage>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let games: RwSignal<Arc<[PerGameStat]>> = RwSignal::new(Arc::from([]));
    let read = RwSignal::new_local(DirectoryRead::started());
    let personal_rating: RwSignal<Option<Arc<PersonalRatingData>>> = RwSignal::new(None);

    load_personal_rating(services, personal_rating);

    // A reload is asked for from the reload button, whose scope goes as the read restarts: the
    // read belongs to this component instead.
    let owner = Owner::current();
    let reader = StoredValue::new_local(None::<Task>);
    let start = {
        let root = root.clone();
        UnsyncCallback::new(move |game_data: GameDataCache| {
            reader.with_value(|task| task.as_ref().map(Task::cancel));
            let read_in = || start_read(services, root.clone(), game_data, games, read);
            let task = match &owner {
                Some(owner) => owner.with(read_in),
                None => read_in(),
            };
            reader.set_value(Some(task));
        })
    };
    // A cache adopted for another install reads the directory again through it.
    let following = RenderEffect::new(move |_| {
        if let Some(game_data) = game_data.get() {
            start.run(game_data);
        }
    });
    on_cleanup_local(move || drop(following));
    let reload = UnsyncCallback::new(move |()| {
        // A read still running would be cut off and its games thrown away.
        if read.get_untracked().state == ReadState::Finished
            && let Some(game_data) = game_data.get_untracked()
        {
            start.run(game_data);
        }
    });

    view! {
        column(class = "wt-directory-stats") {
            DirectoryHeader(root = root, read = read, on_reload = reload)
            box(class = "wt-directory-stats__body") {
                StatsBody(scope = StatsScope::Directory, games = games, personal_rating = personal_rating)
            }
        }
    }
}

/// Reads `root` off the UI thread, folding what it finds into `games` and `read`.
fn start_read(
    services: Services,
    root: PathBuf,
    game_data: GameDataCache,
    games: RwSignal<Arc<[PerGameStat]>>,
    read: RwSignal<DirectoryRead, LocalStorage>,
) -> Task {
    games.set(Arc::from([]));
    read.set(DirectoryRead::started());

    let (tx, rx) = futures::channel::mpsc::unbounded();
    // Detached from the task below on purpose: the reader stops at its next send once `rx` is
    // dropped with that task.
    let reading = blocking(move || {
        read_directory(&root, &game_data, |update| match tx.unbounded_send(update) {
            Ok(()) => ControlFlow::Continue(()),
            Err(_) => ControlFlow::Break(()),
        });
    });
    let timers = Timers::current();

    spawn_local(async move {
        let job = services.jobs.start(Job { name: t!("ui.stats.directory_reading_task").into_owned(), progress: None });
        let mut set = ReplaySetGames::new();
        fold_updates(rx, timers, |updates| {
            let mut shown = read.get_untracked();
            let mut found = Vec::new();
            for update in updates {
                shown.state = update.read_state();
                if let DirectoryStatsUpdate::Read { game, skips, .. } = update {
                    shown.skips = skips;
                    found.extend(game.map(|game| *game));
                }
            }
            if let Some(progress) = shown.progress() {
                job.report(Job {
                    name: t!("ui.stats.directory_reading_task").into_owned(),
                    progress: Progress::new(progress.done as u64, progress.total as u64),
                });
            }
            read.set(shown);
            if !found.is_empty() {
                set.extend(found);
                games.set(Arc::from(set.games()));
            }
        })
        .await;
        // The reader has returned once its sender is gone; a panic in it ends the read the same way.
        drop(reading);
        read.update(|read| read.state = ReadState::Finished);
    })
}

/// Hands every batch of updates waiting on `rx` to `fold`, at most once per [`FOLD_INTERVAL`],
/// until the reader drops its sender.
async fn fold_updates(
    mut rx: UnboundedReceiver<DirectoryStatsUpdate>,
    timers: Option<Timers>,
    mut fold: impl FnMut(Vec<DirectoryStatsUpdate>),
) {
    while let Some(first) = rx.next().await {
        let mut updates = vec![first];
        while let Ok(update) = rx.try_recv() {
            updates.push(update);
        }
        fold(updates);

        if let Some(timers) = &timers {
            let (fired, wait) = futures::channel::oneshot::channel();
            let _timeout = timers.set_timeout(FOLD_INTERVAL, move || {
                let _ = fired.send(());
            });
            let _ = wait.await;
        }
    }
}

/// Loads the personal-rating table, refreshing the cached copy first when it is stale.
fn load_personal_rating(services: Services, table: RwSignal<Option<Arc<PersonalRatingData>>>) {
    let proxy = services.settings.read_untracked(|settings| settings.proxy_url.clone()).unwrap_or_default();
    let loading = services
        .on_tokio(async move { wows_toolkit_services::personal_rating::load_refreshed(USER_AGENT, &proxy).await });
    spawn_local(async move {
        match loading.await {
            Ok(loaded) => table.set(loaded.map(Arc::new)),
            Err(error) => tracing::warn!("directory stats: the personal rating table was not loaded: {error}"),
        }
    });
}

/// The line above the stats body: the directory, how far the read has got, and what it skipped.
#[component]
fn DirectoryHeader(
    root: PathBuf,
    #[prop(into)] read: Signal<DirectoryRead, LocalStorage>,
    on_reload: UnsyncCallback<()>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let status = move || {
        locale.track();
        read.get().status()
    };
    let skipped = move || read.get().skips.total() > 0;

    view! {
        column(class = "wt-directory-stats__header", attr:data-testid = "directory-stats-header") {
            row(class = "wt-directory-stats__bar") {
                row(class = "wt-directory-stats__root") {
                    AppIcon(glyph = Glyph::FolderOpen)
                    text(class = "wt-directory-stats__path", attr:data-testid = "directory-stats-path") {
                        {shown_path(&root)}
                    }
                }
                box(class = "wt-directory-stats__rule")
                Show(when = move || status().is_some()) {
                    row(class = "wt-directory-stats__status", attr:data-testid = "directory-stats-status") {
                        Spinner(label = t!("ui.stats.directory_reading_task").into_owned())
                        text {{move || status().unwrap_or_default()}}
                    }
                }
                Show(when = move || read.get().state == ReadState::Finished) {
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                size = ButtonSize::Sm,
                                variant = ButtonVariant::Outline,
                                attr:data-testid = "directory-stats-reload",
                                on:click = move |_| on_reload.run(())
                            ) {
                                AppIcon(glyph = Glyph::RotateCw)
                                text {{move || {
                                    locale.track();
                                    t!("ui.stats.directory_reload").into_owned()
                                }}}
                            }
                        }
                        TooltipContent {{move || {
                            locale.track();
                            t!("ui.stats.directory_reload_tooltip").into_owned()
                        }}}
                    }
                }
                Show(when = skipped) {
                    box(class = "wt-directory-stats__rule")
                    Tooltip {
                        TooltipTrigger {
                            text(class = "wt-directory-stats__skipped", attr:data-testid = "directory-stats-skipped") {
                                {move || {
                                    locale.track();
                                    t!("ui.stats.directory_skipped", count = read.get().skips.total()).into_owned()
                                }}
                            }
                        }
                        TooltipContent {{move || {
                            locale.track();
                            let skips = read.get().skips;
                            t!(
                                "ui.stats.directory_skipped_detail",
                                unreadable = skips.unreadable,
                                missing_build = skips.missing_build,
                                no_results = skips.no_results
                            )
                            .into_owned()
                        }}}
                    }
                }
            }
            Show(when = move || read.get().progress().is_some()) {
                Progress(
                    attr:data-testid = "directory-stats-progress",
                    label = t!("ui.stats.directory_reading_task").into_owned(),
                    value = Signal::derive_local(move || {
                        read.get().progress().map(|progress| f64::from(progress.fraction()) * 100.0)
                    })
                )
            }
        }
    }
}

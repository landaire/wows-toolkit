//! Stats over every replay in one directory, rather than over the recorded
//! session.
//!
//! The replays are read by `wows_toolkit_replay_data::directory_stats` and
//! folded through `wows_toolkit_viewmodel::stats::replay_set`, shared with the
//! egui app.
//! [`StatsView`] renders them in its directory scope; nothing here touches the
//! session rows or the replay index.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::EventEmitter;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Task;
use gpui_kit::Window;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use rust_i18n::t;
use wows_toolkit_replay_data::directory_stats::DirectoryStatsUpdate;
use wows_toolkit_replay_data::directory_stats::ReadState;
use wows_toolkit_replay_data::directory_stats::read_directory;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetGames;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;

use crate::replay_inspector::view::ReplayInspectorView;

use super::view::StatsScope;
use super::view::StatsView;

/// The listing asks for a stats tab over the directory it is showing.
pub(crate) struct DirectoryStatsRequested(pub PathBuf);
impl EventEmitter<DirectoryStatsRequested> for ReplayInspectorView {}

/// The games of a stats view over one directory, and the read filling them.
pub struct DirectoryScope {
    root: PathBuf,
    game_data: GameDataCache,
    games: ReplaySetGames,
    read: ReadState,
    skips: ReplaySetSkips,
    /// Holds the receiving end of the reader's channel: dropping it, with the
    /// view or on a reload, is what stops the reader.
    read_task: Option<Task<()>>,
}

impl DirectoryScope {
    fn new(root: PathBuf, game_data: GameDataCache) -> Self {
        Self {
            root,
            game_data,
            games: ReplaySetGames::new(),
            read: ReadState::Walking,
            skips: ReplaySetSkips::default(),
            read_task: None,
        }
    }

    pub fn games(&self) -> &[PerGameStat] {
        self.games.games()
    }
}

impl StatsView {
    /// A stats view over every replay listed in `root`, read against
    /// `game_data`. Starts reading at once.
    pub fn new_for_directory(
        root: PathBuf,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = DirectoryScope::new(root, game_data);
        let mut view = Self::with_scope(StatsScope::Directory(Box::new(scope)), window, cx);
        view.start_directory_read(cx);
        view
    }

    /// Reads the directory again from the start, picking up replays added or
    /// removed since. Does nothing while a read is running.
    pub fn reload_directory(&mut self, cx: &mut Context<Self>) {
        let Some(scope) = self.directory_scope() else { return };
        if scope.read != ReadState::Finished {
            return;
        }

        self.start_directory_read(cx);
    }

    fn start_directory_read(&mut self, cx: &mut Context<Self>) {
        let Some(scope) = self.directory_scope_mut() else { return };
        scope.games = ReplaySetGames::new();
        scope.read = ReadState::Walking;
        scope.skips = ReplaySetSkips::default();
        scope.read_task = None;

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let root = scope.root.clone();
        let game_data = scope.game_data.clone();
        let spawned = std::thread::Builder::new().name("directory-stats".to_owned()).spawn(move || {
            read_directory(&root, &game_data, |update| match tx.unbounded_send(update) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            });
        });
        if let Err(err) = spawned {
            tracing::warn!("directory stats: no thread to read on: {err}");
            scope.read = ReadState::Finished;
            self.directory_games_changed(cx);
            return;
        }

        scope.read_task = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = rx.next().await {
                let mut updates = vec![first];
                while let Ok(update) = rx.try_recv() {
                    updates.push(update);
                }
                if this.update(cx, |this, cx| this.apply_directory_updates(updates, cx)).is_err() {
                    return;
                }
                // Each fold re-sorts the set and refilters every panel, so a
                // fast read is folded in batches.
                cx.background_executor().timer(Duration::from_millis(250)).await;
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(scope) = this.directory_scope_mut() {
                    scope.read = ReadState::Finished;
                }
                cx.notify();
            });
        }));
        self.directory_games_changed(cx);
    }

    fn apply_directory_updates(&mut self, updates: Vec<DirectoryStatsUpdate>, cx: &mut Context<Self>) {
        let Some(scope) = self.directory_scope_mut() else { return };

        let mut games = Vec::new();
        for update in updates {
            match update {
                DirectoryStatsUpdate::Scanning(progress) => scope.read = ReadState::Scanning(progress),
                DirectoryStatsUpdate::Read { game, progress, skips } => {
                    scope.read = ReadState::Reading(progress);
                    scope.skips = skips;
                    games.extend(game.map(|game| *game));
                }
            }
        }
        if games.is_empty() {
            cx.notify();
            return;
        }

        scope.games.extend(games);
        self.directory_games_changed(cx);
    }
}

/// The line above a directory view's filter bar: the directory, how far the
/// read has got, and what it skipped.
pub(super) fn directory_header(scope: &DirectoryScope, view: Entity<StatsView>, cx: &App) -> impl IntoElement {
    let progress = match scope.read {
        ReadState::Scanning(progress) | ReadState::Reading(progress) => Some(progress),
        ReadState::Walking | ReadState::Finished => None,
    };
    let status = match scope.read {
        ReadState::Walking => Some(t!("ui.stats.directory_walking").into_owned()),
        ReadState::Scanning(progress) => {
            Some(t!("ui.stats.directory_scanning", done = progress.done, total = progress.total).into_owned())
        }
        ReadState::Reading(progress) => {
            Some(t!("ui.stats.directory_reading", done = progress.done, total = progress.total).into_owned())
        }
        ReadState::Finished => None,
    };
    let skips = scope.skips;
    let skipped = (skips.total() > 0).then(|| {
        let detail = t!(
            "ui.stats.directory_skipped_detail",
            unreadable = skips.unreadable,
            missing_build = skips.missing_build,
            no_results = skips.no_results
        )
        .into_owned();
        div()
            .id("directory-stats-skipped")
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(t!("ui.stats.directory_skipped", count = skips.total()).into_owned())
            .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
    });

    let row = crate::ui::toolbar(cx)
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .min_w_0()
                .child(crate::icons::icon(crate::icons::Glyph::FolderOpen))
                .child(div().text_sm().truncate().child(scope.root.display().to_string())),
        )
        .child(crate::ui::rule_v(cx))
        .when_some(status.clone(), |row, status| {
            row.child(h_flex().gap_2().items_center().text_xs().child(Spinner::new().xsmall()).child(status))
        })
        .when(scope.read == ReadState::Finished, |row| {
            row.child(
                Button::new("directory-stats-reload")
                    .icon(crate::icons::Glyph::RotateCw)
                    .label(t!("ui.stats.directory_reload").into_owned())
                    .compact()
                    .tooltip(t!("ui.stats.directory_reload_tooltip").into_owned())
                    .on_click(move |_event, _window, cx: &mut App| {
                        view.update(cx, |this, cx| this.reload_directory(cx));
                    }),
            )
        })
        .when_some(skipped, |row, skipped| row.child(crate::ui::rule_v(cx)).child(skipped));

    v_flex().flex_none().child(row).when_some(progress, |column, progress| {
        column.child(
            Progress::new("directory-stats-progress")
                .accessibility_label(status.unwrap_or_default())
                .color(cx.theme().primary)
                .value(progress.fraction() * 100.),
        )
    })
}

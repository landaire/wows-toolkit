//! The board a window shows, as one handle its views share, and the work it asks for.

use std::time::Duration;

use landaire_ui::Toasts;
use rust_i18n::t;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::tactics::ScanProgress;
use wows_toolkit_replay_data::tactics::board::Board;
use wows_toolkit_replay_data::tactics::board::BoardNotice;
use wows_toolkit_replay_data::tactics::board::Redraw;
use wows_toolkit_replay_data::tactics::render::FrameRequest;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::frames::FrameSink;
use crate::services::Services;
use crate::windows::tactics::TacticsHost;

/// How often a board looks at what its session holds.
pub const COLLAB_TICK: Duration = Duration::from_millis(200);

/// The board, the frame it is drawn into, and where its messages go. A `Copy` handle.
#[derive(Clone, Copy)]
pub struct BoardModel {
    board: StoredValue<Board, LocalStorage>,
    /// The board's revision as views last saw it: reading the board through [`Self::with`]
    /// subscribes to this.
    revision: RwSignal<u64, LocalStorage>,
    sink: StoredValue<FrameSink, LocalStorage>,
    /// Whether a rasterisation is in flight, so a burst of edits asks for one redraw rather than
    /// one each.
    rasterising: StoredValue<bool>,
    /// Whether anything changed while one was in flight.
    stale: StoredValue<bool>,
    toasts: StoredValue<Option<Toasts>, LocalStorage>,
    services: Services,
    host: TacticsHost,
}

impl BoardModel {
    pub fn new(services: Services, host: TacticsHost, game_data: Option<GameDataCache>, layouts: CapLayoutDb) -> Self {
        Self {
            board: StoredValue::new_local(Board::new(game_data, layouts)),
            revision: RwSignal::new_local(0),
            sink: StoredValue::new_local(FrameSink::new()),
            rasterising: StoredValue::new(false),
            stale: StoredValue::new(false),
            toasts: StoredValue::new_local(Toasts::current()),
            services,
            host,
        }
    }

    pub fn sink(self) -> FrameSink {
        self.sink.get_value()
    }

    /// Reads the board, subscribing to every change of it.
    pub fn with<R>(self, read: impl FnOnce(&Board) -> R) -> R {
        self.revision.get();
        self.board.with_value(read)
    }

    /// Changes the board, then does what the change asked for.
    pub fn update(self, change: impl FnOnce(&mut Board)) {
        self.change(change);
    }

    /// [`Self::update`] for a change with a result. `None` once the window has gone.
    pub fn change<R>(self, change: impl FnOnce(&mut Board) -> R) -> Option<R> {
        let result = self.board.try_update_value(change)?;
        self.settle();
        Some(result)
    }

    /// [`Self::with`] without subscribing.
    pub fn with_untracked<R>(self, read: impl FnOnce(&Board) -> R) -> R {
        self.board.with_value(read)
    }

    /// Publishes a changed board to its views, and carries out what it asked for: a redraw, an
    /// announcement and messages.
    fn settle(self) {
        let Some(revision) = self.board.try_with_value(Board::revision) else { return };
        if self.revision.get_untracked() != revision {
            self.revision.set(revision);
        }
        if let Some(redraw) = self.board.try_update_value(Board::take_redraw).flatten() {
            match redraw {
                Redraw::Clear => self.sink.with_value(FrameSink::clear),
                Redraw::Draw(request) => self.rasterise(*request),
            }
        }
        if let Some(art) = self.board.try_update_value(Board::take_announcement).flatten() {
            spawn_local(async move {
                let art = blocking(move || art.load()).await;
                self.update(|board| board.finish_announce(art));
            });
        }
        for notice in self.board.try_update_value(Board::take_notices).unwrap_or_default() {
            self.tell(&notice);
        }
    }

    /// Draws the map off the UI thread. One at a time: the raster costs a map-sized image, and a
    /// reader dragging a capture point would otherwise queue one per frame.
    fn rasterise(self, request: FrameRequest) {
        if self.rasterising.get_value() {
            self.stale.set_value(true);
            return;
        }
        self.rasterising.set_value(true);
        spawn_local(async move {
            let space = request.space().to_owned();
            let frame = blocking(move || request.rasterise()).await;
            // A frame of a map the board has since left would be read with the new map's
            // coordinates, so it is dropped and the current map drawn instead.
            let current = self.board.try_with_value(|board| board.map().is_some_and(|map| map.space == space));
            let Some(current) = current else { return };
            let size = frame.as_ref().map(|frame| (frame.width() as f32, frame.height() as f32));
            if current && let Some(frame) = frame {
                self.sink.with_value(|sink| sink.push(frame));
            }
            self.rasterising.set_value(false);
            let stale = self.stale.get_value() || !current;
            self.stale.set_value(false);
            self.update(|board| {
                if current {
                    board.drawn(size);
                }
                if stale {
                    board.request_redraw();
                }
            });
        });
    }

    /// Shows `notice` in the board's window.
    fn tell(self, notice: &BoardNotice) {
        let Some(toasts) = self.toasts.get_value() else { return };
        match notice {
            BoardNotice::PresetNeedsName => toasts.warn(t!("ui.tactics.preset_needs_a_name").into_owned()),
            BoardNotice::PresetNeedsMap => toasts.warn(t!("ui.tactics.preset_needs_a_map").into_owned()),
            BoardNotice::PresetSaved { name } => toasts.info(t!("ui.tactics.preset_saved", name = name).into_owned()),
            BoardNotice::PresetSaveFailed { error } => {
                toasts.failed(t!("ui.tactics.preset_save_failed", error = error).into_owned())
            }
            BoardNotice::PresetLoadFailed { error } => {
                toasts.failed(t!("ui.tactics.preset_load_failed", error = error).into_owned())
            }
            BoardNotice::PresetDeleteFailed { error } => {
                toasts.failed(t!("ui.tactics.preset_delete_failed", error = error).into_owned())
            }
            BoardNotice::NoReplaysDir => toasts.warn(t!("ui.messages.wows_dir_not_set").into_owned()),
            BoardNotice::ScanNeedsGameData => toasts.warn(t!("ui.tactics.scan_needs_game_data").into_owned()),
            BoardNotice::ScanDone { added, total } => {
                toasts.info(t!("ui.tactics.scan_done", added = added, total = total).into_owned())
            }
        };
    }

    /// Reads every replay for the capture layouts of the modes they were played in, writing each
    /// new one to the store.
    pub fn scan_replays(self) {
        let Some(job) = self.change(Board::begin_scan).flatten() else { return };
        let services = self.services;
        let pool = services.settings.pool();
        let runtime = services.runtime();
        // The walk reports itself as it goes rather than being polled.
        let (reports, mut progress) = futures::channel::mpsc::unbounded::<ScanProgress>();
        spawn_local(async move {
            while let Some(step) = futures::StreamExt::next(&mut progress).await {
                self.update(|board| board.scan_progress(step));
            }
        });
        spawn_local(async move {
            let outcome = blocking(move || {
                let outcome = job.run(|step| {
                    let _ = reports.unbounded_send(step);
                });
                // Each new layout on its own rather than the whole store: this walk holds a copy
                // taken when the board opened, and writing all of it would put those rows back over
                // anything written since.
                if let (Some(pool), Some(runtime)) = (pool, runtime.as_ref()) {
                    for layout in &outcome.fresh {
                        let saved = runtime
                            .block_on(wows_toolkit_replay_data::cap_layout_store::save_layout_to_db(&pool, layout));
                        if let Err(err) = saved {
                            tracing::warn!("tactics: a capture layout was not saved: {err}");
                        }
                    }
                }
                outcome
            })
            .await;
            // The host holds the copy a later board starts from, which would otherwise not know
            // what this walk turned up.
            if let Some(found) = self.change(|board| board.finish_scan(outcome)).flatten() {
                self.host.adopt_layouts(found);
            }
        });
    }
}

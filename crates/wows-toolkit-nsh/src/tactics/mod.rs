//! The tactics board: a map with capture points and drawings on it, drawn on
//! the `tactics-board` frame surface and steered from `ui/tactics.tsx`.

mod board;
mod error;
mod game;
mod ids;
mod producer;
mod raster;
#[cfg(test)]
mod tests;
mod view;

use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use nsh_core::prelude::*;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_toolkit_viewmodel::tactics::board::INKS;
use wows_toolkit_viewmodel::tactics::board::RADIUS_STEP_KM;

use crate::frames::PixelPoint;
use crate::frames::SurfaceId;
use crate::tactics::board::Board;
use crate::tactics::error::TacticsError;
use crate::tactics::game::BoardGame;
use crate::tactics::ids::InkIndex;
use crate::tactics::ids::ShipMatchIndex;
use crate::tactics::ids::ViewRevision;
use crate::tactics::producer::Shared;
use crate::tactics::view::RangeName;
use crate::tactics::view::TacticsView;
use crate::tactics::view::ToolName;

/// The surface the board is drawn on, which `ui/tactics.tsx` names.
pub const SURFACE: &str = "tactics-board";

// Empty until the first open hands it a build and its layouts.
static SHARED: LazyLock<Arc<Shared>> =
    LazyLock::new(|| Shared::start(&SurfaceId::new(SURFACE), Board::new(None, CapLayoutDb::default(), None)));

fn shared() -> &'static Arc<Shared> {
    &SHARED
}

/// The task handing the open board each new session link.
static SESSION_FOLLOWER: std::sync::Mutex<Option<tokio::task::AbortHandle>> = std::sync::Mutex::new(None);

/// Hands the board the current session, if any, then each later link until
/// the board closes.
fn follow_session() {
    let shared = Arc::clone(shared());
    let mut links = crate::collab::subscribe();
    let link = links.borrow_and_update().clone();
    if link.is_active() {
        shared.set_collab(link);
    }
    let runtime = match crate::runtime::shared() {
        Ok(runtime) => runtime,
        Err(err) => {
            log::warn!("tactics: the board cannot follow the session: {err}");
            return;
        }
    };
    let task = runtime.spawn(shared.follow_links(links));
    if let Some(previous) = SESSION_FOLLOWER.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).replace(task.abort_handle()) {
        previous.abort();
    }
}

fn stop_following_session() {
    if let Some(task) = SESSION_FOLLOWER.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
        task.abort();
    }
}

/// Orders the board's opens against its closes. An open loads for a while
/// before it replaces the board, and one the board closed under, or a later
/// open overtook, must not bring the board back or follow the session.
struct OpenGate {
    current: Mutex<OpenTicket>,
}

impl OpenGate {
    const fn new() -> Self {
        Self { current: Mutex::new(OpenTicket(0)) }
    }

    fn lock(&self) -> MutexGuard<'_, OpenTicket> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts an open, overtaking any still under way.
    fn begin(&self) -> OpenTicket {
        let mut current = self.lock();
        *current = current.next();

        *current
    }

    /// Runs `apply` when `ticket` is still the current open, holding the gate
    /// so no close comes between the check and the change. Whether it ran.
    fn finish(&self, ticket: OpenTicket, apply: impl FnOnce()) -> bool {
        let current = self.lock();
        if *current != ticket {
            return false;
        }
        apply();

        true
    }

    /// Abandons any open under way and runs `apply` under the gate.
    fn close(&self, apply: impl FnOnce()) {
        let mut current = self.lock();
        *current = current.next();
        apply();
    }
}

/// One started open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenTicket(u64);

impl OpenTicket {
    fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

static OPENS: OpenGate = OpenGate::new();

/// Changes the board and answers what it now shows.
fn change(edit: impl FnOnce(&mut Board)) -> TacticsView {
    let shared = shared();
    shared.update(edit);
    current()
}

fn current() -> TacticsView {
    let shared = shared();
    let revision = *shared.subscribe().borrow();
    view::of(&shared.lock(), revision)
}

/// The tactics board's commands.
#[module]
pub struct Tactics;

#[module]
impl Tactics {
    /// Opens a fresh board on the installed build, with the capture layouts
    /// recorded so far. Loads the build the first time.
    pub async fn open() -> Result<TacticsView, TacticsError> {
        let ticket = OPENS.begin();
        let layouts = blocking(load_layouts).await.map_err(|source| TacticsError::LayoutTask { source })?;
        let loaded = blocking(crate::game_data::wait_current_build)
            .await
            .map_err(|source| TacticsError::GameDataTask { source })?;
        // Without a game directory the board opens anyway and says what it
        // needs; any other failure to load is the reader's to see.
        let game: Option<Arc<dyn BoardGame>> = match loaded {
            Ok(loaded) => Some(loaded),
            Err(crate::game_data::GameDataError::NotConfigured) => None,
            Err(error) => return Err(TacticsError::GameData(error)),
        };
        let replays_dir = crate::game_data::replays_dir();
        // An open the board closed under, or a later open overtook, leaves
        // the board as it is and answers what it shows.
        OPENS.finish(ticket, || {
            shared().update(|board| {
                board.close();
                let view_size = board.view_size();
                *board = Board::new(game, layouts, replays_dir);
                if let Some(view_size) = view_size {
                    board.resize(view_size);
                }
            });
            follow_session();
        });
        Ok(current())
    }

    /// Closes the board: a replay walk stops, and the session stops offering
    /// it. An open still loading is abandoned.
    pub fn close() {
        OPENS.close(|| {
            stop_following_session();
            shared().update(|board| {
                board.close();
                *board = board.reopened();
            });
        });
    }

    /// What the board shows now.
    pub fn view() -> TacticsView {
        current()
    }

    /// Resolves with what the board shows once it differs from `after`.
    pub async fn next_change(after: ViewRevision) -> TacticsView {
        let mut revisions = shared().subscribe();
        // The sender lives for the process, so the wait only ends on a change.
        let _ = revisions.wait_for(|revision| *revision != after).await;
        current()
    }

    /// Sets the board on the map named `value`.
    pub fn set_map(value: String) -> Result<TacticsView, TacticsError> {
        let shared = shared();
        let announcement = shared.update(|board| board.set_map_value(&value))?;
        shared.announce(announcement);
        Ok(current())
    }

    /// Sets the board on the mode named `value`, or on none.
    pub fn set_mode(value: String) -> Result<TacticsView, TacticsError> {
        shared().update(|board| board.set_mode_value(&value))?;
        Ok(current())
    }

    /// Takes up `tool`, or puts it down when it is in hand; `null` puts down
    /// whatever is.
    pub fn set_tool(tool: Option<ToolName>) -> TacticsView {
        change(|board| board.set_tool(view::tool_of(tool)))
    }

    /// Takes up `tool` from its keyboard shortcut.
    pub fn shortcut_tool(tool: ToolName) -> TacticsView {
        change(|board| board.set_shortcut_tool(view::tool_of(Some(tool))))
    }

    /// Draws in ink `index` of the offered inks from here on.
    pub fn set_ink(index: InkIndex) -> Result<TacticsView, TacticsError> {
        let (ink, _) = INKS.get(index.position()).ok_or(TacticsError::UnknownInk { index })?;
        Ok(change(|board| board.set_ink(*ink)))
    }

    /// Widens or narrows the nib by `by`.
    pub fn step_nib(by: f64) -> TacticsView {
        change(|board| board.step_nib(by as f32))
    }

    /// Takes the last change back.
    pub fn undo() -> TacticsView {
        change(Board::undo)
    }

    /// Puts back what was taken.
    pub fn redo() -> TacticsView {
        change(Board::redo)
    }

    /// Takes everything drawn off the board, leaving the capture points.
    pub fn clear_drawing() -> TacticsView {
        change(Board::clear_annotations)
    }

    /// Deletes what is picked out: the drawn shapes, or the capture point.
    pub fn delete_picked() -> TacticsView {
        change(Board::delete)
    }

    /// Whether the next map click places a capture point.
    pub fn set_adding(adding: bool) -> TacticsView {
        change(|board| board.set_adding(adding))
    }

    /// Widens the selected capture point by half a kilometre.
    pub fn widen_cap() -> TacticsView {
        change(|board| board.step_selected_radius(RADIUS_STEP_KM))
    }

    /// Narrows the selected capture point by half a kilometre.
    pub fn narrow_cap() -> TacticsView {
        change(|board| board.step_selected_radius(-RADIUS_STEP_KM))
    }

    /// Hands the selected capture point to the next team.
    pub fn cycle_cap_team() -> TacticsView {
        change(Board::cycle_selected_team)
    }

    /// Deletes the selected capture point, unless it came from a replay.
    pub fn delete_cap() -> TacticsView {
        change(Board::remove_selected)
    }

    /// Takes every capture point the reader added off the board.
    pub fn clear_caps() -> TacticsView {
        change(Board::clear_caps)
    }

    /// Puts the whole map back on screen.
    pub fn reset_view() -> TacticsView {
        change(Board::reset_view)
    }

    /// Restores the mode's capture points and clears the drawing.
    pub fn reset_board() -> TacticsView {
        change(Board::reset_board)
    }

    /// Turns one range circle on or off for every ship.
    pub fn set_range_circle(circle: RangeName, on: bool) -> TacticsView {
        change(|board| board.set_range_circle(view::circle_of(circle), on))
    }

    /// Turns every range circle on or off for every ship.
    pub fn set_all_range_circles(on: bool) -> TacticsView {
        change(|board| board.set_all_range_circles(on))
    }

    /// Finds the ships whose names match `query`.
    pub fn search_ships(query: String) -> TacticsView {
        change(|board| board.search_ships(&query))
    }

    /// Takes up ship match `index`, so the next map click places it.
    pub fn pick_ship(index: ShipMatchIndex) -> Result<TacticsView, TacticsError> {
        shared().update(|board| board.pick_ship(index))?;
        Ok(current())
    }

    /// Which side the ship being placed is on.
    pub fn set_placing_friendly(friendly: bool) -> TacticsView {
        change(|board| board.set_placing_friendly(friendly))
    }

    /// Saves the board under `name`.
    pub fn save_preset(name: String) -> Result<TacticsView, TacticsError> {
        shared().update(|board| board.save_preset(&name))?;
        Ok(current())
    }

    /// Opens the saved board `name`.
    pub fn load_preset(name: String) -> Result<TacticsView, TacticsError> {
        let shared = shared();
        let announcement = shared.update(|board| board.load_preset(&name))?;
        shared.announce(announcement);
        Ok(current())
    }

    /// Drops the saved board `name`.
    pub fn delete_preset(name: String) -> Result<TacticsView, TacticsError> {
        shared().update(|board| board.delete_preset(&name))?;
        Ok(current())
    }

    /// Reads the replays for the capture layouts of the modes they were
    /// played in.
    pub fn scan_replays() -> Result<TacticsView, TacticsError> {
        let shared = shared();
        if let Some(start) = shared.update(Board::begin_scan)? {
            shared.scan(start);
        }
        Ok(current())
    }

    /// Opens the context menu at `(x, y)` points from the frame's top left:
    /// puts the tool down and picks out the capture point there.
    pub fn context_menu(x: f64, y: f64) -> TacticsView {
        change(|board| {
            // Without a view nothing lies under the pointer, so any scale
            // serves.
            let scale = board.view_size().map_or(1.0, |view_size| view_size.scale.get());
            board.context_menu(PixelPoint { x: x as f32 * scale, y: y as f32 * scale });
        })
    }
}

/// The capture layouts recorded in the settings database. The board works
/// without them, so a failure to read them is logged and the board opens with
/// none recorded.
fn load_layouts() -> CapLayoutDb {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            log::warn!("tactics: the capture layouts could not be read: {err}");
            return CapLayoutDb::default();
        }
    };
    runtime.block_on(async {
        let pool = match wows_toolkit_config::open_db().await {
            Ok(pool) => pool,
            Err(err) => {
                log::warn!("tactics: the capture layouts could not be read: {err}");
                return CapLayoutDb::default();
            }
        };
        match wows_toolkit_viewmodel::tactics::layout_store::load_from_db(&pool).await {
            Ok(layouts) => layouts,
            Err(err) => {
                log::warn!("tactics: the capture layouts could not be read: {err}");
                CapLayoutDb::default()
            }
        }
    })
}

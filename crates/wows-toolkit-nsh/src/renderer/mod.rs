//! The replay renderer: a battle played back on its own minimap, drawn on a
//! frame surface per viewport and steered from `ui/renderer.tsx`; the
//! minimap hover preview; a battle the session plays elsewhere; and the batch
//! render of many replays to video.

mod armor_link;
pub(crate) mod batch;
mod error;
mod preview;
mod raster;
mod session;
#[cfg(test)]
mod tests;
mod view;
mod viewer;
mod watched;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use nsh_core::file_dialog::FileFilter;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::file_dialog::SaveDialog;
use nsh_core::prelude::*;
use rust_i18n::t;
use wows_minimap_renderer::SavedRenderOptions;
use wows_replays::types::EntityId;
use wows_toolkit_replay_data::playback;
use wows_toolkit_replay_data::playback::ExportError;
use wows_toolkit_replay_data::playback::INKS;
use wows_toolkit_replay_data::playback::LAYER_TOGGLES;
use wows_toolkit_replay_data::playback::SPECIES;
use wows_toolkit_viewmodel::settings::keys;
use wowsunpack::game_params::types::GameParamProvider;
use wt_collab_client::drawing::Tool;

use crate::frames::SurfaceId;
pub use crate::renderer::batch::BatchView;
pub use crate::renderer::error::RendererError;
use crate::renderer::preview::Preview;
pub use crate::renderer::preview::PreviewView;
use crate::renderer::session::Session;
pub use crate::renderer::view::RendererRange;
pub use crate::renderer::view::RendererText;
pub use crate::renderer::view::RendererTool;
pub use crate::renderer::view::RendererView;
pub use crate::renderer::view::SettingsView;
pub use crate::renderer::view::ShipMenuView;
pub use crate::renderer::view::TimelineView;
use crate::renderer::viewer::Viewer;
use crate::renderer::watched::Watched;
pub use crate::renderer::watched::WatchedView;

/// The open viewports, by id.
static SESSIONS: LazyLock<Mutex<HashMap<u32, Arc<Session>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The open hover previews and watched windows, by id.
static PREVIEWS: LazyLock<Mutex<HashMap<u32, Arc<Preview>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static WATCHED: LazyLock<Mutex<HashMap<u32, Arc<Watched>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Ids for viewports, previews and watched windows alike, so their surfaces
/// never share a name.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// What viewports open showing, read from the shared `render_options` row on
/// first use and replaced by Save Defaults.
static DEFAULTS: Mutex<Option<SavedRenderOptions>> = Mutex::new(None);

/// Whether the reader asked not to be told before a software encode.
static SOFTWARE_ENCODE_QUIET: LazyLock<AtomicBool> = LazyLock::new(|| {
    let quiet = wows_toolkit_config::load_startup_setting::<bool>(keys::SUPPRESS_GPU_ENCODER_WARNING)
        .inspect_err(|err| log::warn!("renderer: the software encode notice setting could not be read: {err}"))
        .ok()
        .flatten()
        .unwrap_or(false);
    AtomicBool::new(quiet)
});

fn surface_name(id: u32) -> String {
    format!("replay-renderer-{id}")
}

fn preview(id: u32) -> Result<Arc<Preview>, RendererError> {
    let previews = PREVIEWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    previews.get(&id).map(Arc::clone).ok_or(RendererError::UnknownRenderer { id })
}

fn watched(id: u32) -> Result<Arc<Watched>, RendererError> {
    let watched = WATCHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    watched.get(&id).map(Arc::clone).ok_or(RendererError::UnknownRenderer { id })
}

fn session(id: u32) -> Result<Arc<Session>, RendererError> {
    let sessions = SESSIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    sessions.get(&id).map(Arc::clone).ok_or(RendererError::UnknownRenderer { id })
}

fn current(id: u32, session: &Session) -> RendererView {
    let revision = *session.subscribe().borrow();
    let mut shown = view::of(id, &surface_name(id), &session.lock(), revision);
    if SOFTWARE_ENCODE_QUIET.load(Ordering::Relaxed) {
        shown.software_encode = false;
    }
    shown
}

/// Changes viewport `id` and answers what it now shows.
fn change(id: u32, edit: impl FnOnce(&mut Viewer)) -> Result<RendererView, RendererError> {
    let session = session(id)?;
    session.update(edit);
    Ok(current(id, &session))
}

/// The saved defaults, read from the settings database the first time.
async fn defaults() -> SavedRenderOptions {
    if let Some(held) = DEFAULTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone() {
        return held;
    }
    let read = match wows_toolkit_config::open_db().await {
        Ok(pool) => wows_toolkit_replay_data::render_defaults::load(&pool).await,
        Err(err) => {
            // Every viewport then opens with the renderer's own defaults,
            // which is what a fresh install does anyway.
            log::warn!("renderer: the saved defaults could not be read: {err}");
            SavedRenderOptions::default()
        }
    };
    *DEFAULTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(read.clone());
    read
}

/// The replay renderer's commands. Every command but `open` takes the id
/// `open` answered with.
#[module]
pub struct ReplayRenderer;

#[module]
impl ReplayRenderer {
    /// Opens a viewport on the replay at `path`, read with the other
    /// recordings of the same battle in `alts`, and starts reading it.
    pub async fn open(path: String, alts: Vec<String>) -> RendererView {
        let defaults = defaults().await;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let session = Session::open(
            &SurfaceId::new(surface_name(id)),
            PathBuf::from(path),
            alts.into_iter().map(PathBuf::from).collect(),
            defaults,
        );
        SESSIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id, Arc::clone(&session));
        current(id, &session)
    }

    /// Closes viewport `id`: its bake stops, and a session stops offering it.
    pub fn close(id: u32) {
        let removed = SESSIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
        if let Some(session) = removed {
            session.close();
        }
    }

    /// Bakes viewport `id` again through another set of recordings, for one
    /// opened before an alt was added.
    pub fn rebake(id: u32, alts: Vec<String>) -> Result<RendererView, RendererError> {
        let session = session(id)?;
        let alts: Vec<PathBuf> = alts.into_iter().map(PathBuf::from).collect();
        if session.lock().alts() == alts.as_slice() {
            return Ok(current(id, &session));
        }
        session.update(|viewer| viewer.rebake(alts));
        session.bake();
        Ok(current(id, &session))
    }

    /// What viewport `id` shows now.
    pub fn view(id: u32) -> Result<RendererView, RendererError> {
        let session = session(id)?;
        Ok(current(id, &session))
    }

    /// Resolves with what viewport `id` shows once it differs from `after`.
    pub async fn next_change(id: u32, after: u32) -> Result<RendererView, RendererError> {
        let session = session(id)?;
        let mut revisions = session.subscribe();
        // The sender lives as long as the session; a closed one answers as
        // it last was.
        let _ = revisions.wait_for(|revision| *revision != after).await;
        Ok(current(id, &session))
    }

    /// The labels of the renderer's controls.
    pub fn text() -> RendererText {
        view::text()
    }

    pub fn toggle_playing(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::toggle_playing)
    }

    /// Moves playback to frame `frame` of the track.
    pub fn seek(id: u32, frame: u32) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.go_to(frame as usize))
    }

    pub fn seek_end(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::go_to_end)
    }

    /// Moves playback `seconds` of game time, either way.
    pub fn seek_by(id: u32, seconds: f64) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.seek_by(seconds as f32))
    }

    pub fn previous_event(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::jump_to_previous_event)
    }

    pub fn next_event(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::jump_to_next_event)
    }

    /// Plays at speed `index` of the offered speeds.
    pub fn set_speed(id: u32, index: u32) -> Result<RendererView, RendererError> {
        if playback::SPEEDS.get(index as usize).is_none() {
            return Err(RendererError::UnknownSpeed { index });
        }
        change(id, |viewer| viewer.set_speed_index(index as usize))
    }

    pub fn set_zoom(id: u32, zoom: f64) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.set_zoom(zoom as f32))
    }

    /// Puts the whole map back on screen.
    pub fn reset_zoom(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::reset_view)
    }

    /// The settings popover's contents.
    pub fn settings(id: u32) -> Result<SettingsView, RendererError> {
        Ok(view::settings(&session(id)?.lock()))
    }

    /// Turns the layer `layer` on or off.
    pub fn set_layer(id: u32, layer: String, on: bool) -> Result<SettingsView, RendererError> {
        let toggle = LAYER_TOGGLES
            .iter()
            .find(|toggle| toggle.id == layer)
            .ok_or_else(|| RendererError::UnknownLayer { id: layer.clone() })?;
        let session = session(id)?;
        session.update(|viewer| viewer.set_options(|options, dead| (toggle.write)(options, dead, on)));
        Ok(view::settings(&session.lock()))
    }

    pub fn set_prefer_cpu(id: u32, on: bool) -> Result<SettingsView, RendererError> {
        let session = session(id)?;
        session.update(|viewer| viewer.set_export_settings(|settings| settings.prefer_cpu = on));
        Ok(view::settings(&session.lock()))
    }

    pub fn set_include_pre_battle(id: u32, on: bool) -> Result<SettingsView, RendererError> {
        let session = session(id)?;
        session.update(|viewer| viewer.set_export_settings(|settings| settings.include_pre_battle = on));
        Ok(view::settings(&session.lock()))
    }

    /// Encodes with the codec `codec` names; `null` lets the encoder pick.
    pub fn set_codec(id: u32, codec: Option<String>) -> Result<SettingsView, RendererError> {
        let codec = match codec {
            Some(value) => Some(view::codec_of(&value).ok_or(RendererError::UnknownCodec { value })?),
            None => None,
        };
        let session = session(id)?;
        session.update(|viewer| viewer.set_export_settings(|settings| settings.codec = codec));
        Ok(view::settings(&session.lock()))
    }

    /// Keeps what viewport `id` shows as what the next one opens with, here
    /// and in the other apps.
    pub async fn save_defaults(id: u32) -> Result<RendererView, RendererError> {
        let session = session(id)?;
        let stored = defaults().await;
        let saved = session.lock().defaults_over(&stored);
        *DEFAULTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(saved.clone());
        let written = match wows_toolkit_config::open_db().await {
            Ok(pool) => wows_toolkit_replay_data::render_defaults::save(&pool, &saved)
                .await
                .inspect_err(|err| log::error!("renderer: {err}"))
                .is_ok(),
            Err(err) => {
                log::error!("renderer: the defaults were not saved: {err}");
                false
            }
        };
        session.update(|viewer| viewer.defaults_saved(written));
        Ok(current(id, &session))
    }

    /// The event timeline's contents.
    pub fn timeline(id: u32) -> Result<TimelineView, RendererError> {
        Ok(view::timeline(&session(id)?.lock()))
    }

    pub fn set_timeline_kind(id: u32, index: u32, on: bool) -> Result<TimelineView, RendererError> {
        if index as usize >= wows_replay_insights::timeline::KIND_COUNT {
            return Err(RendererError::UnknownKind { index });
        }
        let session = session(id)?;
        session.update(|viewer| viewer.set_event_filter(|filter| filter.kinds[index as usize] = on));
        Ok(view::timeline(&session.lock()))
    }

    /// Shows every kind of event, or none.
    pub fn set_timeline_kinds(id: u32, on: bool) -> Result<TimelineView, RendererError> {
        let session = session(id)?;
        session.update(|viewer| viewer.set_all_kinds(on));
        Ok(view::timeline(&session.lock()))
    }

    pub fn set_timeline_search(id: u32, search: String) -> Result<TimelineView, RendererError> {
        let session = session(id)?;
        session.update(|viewer| viewer.set_event_filter(|filter| filter.search = search));
        Ok(view::timeline(&session.lock()))
    }

    /// Moves playback to `at` seconds into the battle.
    pub fn go_to_event(id: u32, at: f64) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.go_to_event_at(at as f32))
    }

    /// The menu of the ship under `(x, y)` points from the frame's top left,
    /// or `null` when no ship is close enough to have been meant.
    pub fn ship_menu(id: u32, x: f64, y: f64) -> Result<Option<ShipMenuView>, RendererError> {
        let session = session(id)?;
        let viewer = session.lock();
        Ok(viewer.ship_under((x as f32, y as f32)).map(|pick| view::ship_menu(&viewer, &pick)))
    }

    /// Opens the armor viewer on the ship `entity_id` (decimal) was in, with
    /// what it had taken by where playback is, and follows playback from then
    /// on. The viewer's health strip seeks this viewport.
    pub fn show_armor(id: u32, entity_id: String) -> Result<(), RendererError> {
        let raw: u32 = entity_id.parse().map_err(|_| RendererError::UnknownEntity { entity_id: entity_id.clone() })?;
        let session = session(id)?;
        let Some(show) = session.update(|viewer| viewer.show_armor(EntityId::from(raw))) else {
            return Err(RendererError::NoArmorToShow { entity_id });
        };
        crate::armor::show_realtime(show.param_index, show.display_name, show.hits, show.incoming);
        let seeking = Arc::downgrade(&session);
        crate::armor::set_seek_handler(move |clock| {
            if let Some(session) = seeking.upgrade() {
                session.update(|viewer| viewer.go_to_clock(clock));
            }
        });
        Ok(())
    }

    pub fn set_trail(id: u32, player: String, on: bool) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.set_trail_shown(&player, on))
    }

    /// Hides every trail but `player`'s.
    pub fn only_trail(id: u32, player: String) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.only_trail(&player))
    }

    pub fn set_range(id: u32, player: String, range: RendererRange, on: bool) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.set_ranges_for(&player, |filter| *view::range_switch(filter, range) = on))
    }

    /// Turns all of `player`'s ranges on, or all of them off.
    pub fn set_every_range(id: u32, player: String, on: bool) -> Result<RendererView, RendererError> {
        let all = if on { playback::ALL_RANGES } else { playback::NO_RANGES };
        change(id, |viewer| viewer.set_ranges_for(&player, |filter| *filter = all))
    }

    /// Leaves only `player`'s ranges on.
    pub fn only_ranges(id: u32, player: String) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.only_ranges(&player))
    }

    /// Turns every ship's ranges on.
    pub fn all_ranges(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::all_ranges)
    }

    /// Takes up `tool`; `null` puts down whatever is in hand.
    pub fn set_tool(id: u32, tool: Option<RendererTool>) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.take_up(tool.map_or(Tool::None, view::tool_of)))
    }

    /// Takes up a ship of species `short` (`DD` to `SS`) for one side.
    pub fn place_ship(id: u32, short: String, friendly: bool) -> Result<RendererView, RendererError> {
        let Some((_, species)) = SPECIES.iter().find(|(name, _)| *name == short) else {
            return Err(RendererError::UnknownShip { index: 0 });
        };
        change(id, |viewer| viewer.take_up(Tool::Ship { species: (*species).to_owned(), friendly, yaw: 0.0 }))
    }

    /// Draws in ink `index` of the offered inks from here on.
    pub fn set_ink(id: u32, index: u32) -> Result<RendererView, RendererError> {
        let (_, ink) = INKS.get(index as usize).ok_or(RendererError::UnknownInk { index })?;
        change(id, |viewer| viewer.set_ink(*ink))
    }

    /// Widens or narrows the nib by `by`.
    pub fn step_nib(id: u32, by: f64) -> Result<RendererView, RendererError> {
        change(id, |viewer| viewer.step_nib(by as f32))
    }

    pub fn undo(id: u32) -> Result<RendererView, RendererError> {
        change(id, Viewer::undo)
    }

    /// Finds the ships whose names match `query`, to name a placed one.
    pub fn search_ships(id: u32, query: String) -> Result<RendererView, RendererError> {
        change(id, |viewer| {
            viewer.search_ships(&query, || {
                let loaded = crate::game_data::newest_loaded()?;
                Some(Arc::new(wows_toolkit_viewmodel::ship_catalog::ShipCatalog::build(loaded.provider())))
            });
        })
    }

    /// Names the picked ship after match `index`.
    pub fn name_ship(id: u32, index: u32) -> Result<RendererView, RendererError> {
        let session = session(id)?;
        let param_index = session
            .lock()
            .matched_param_index(index as usize)
            .map(str::to_owned)
            .ok_or(RendererError::UnknownShip { index })?;
        let param_id = crate::game_data::newest_loaded()
            .and_then(|loaded| loaded.provider().game_param_by_index(&param_index))
            .map(|param| param.id().raw())
            .ok_or(RendererError::UnknownShipParam { param_index })?;
        session.update(|viewer| viewer.name_picked_ship(index as usize, param_id));
        Ok(current(id, &session))
    }

    /// Stops asking before a software encode, here and in the other apps.
    pub async fn quiet_software_encode() {
        SOFTWARE_ENCODE_QUIET.store(true, Ordering::Relaxed);
        match wows_toolkit_config::open_db().await {
            Ok(pool) => {
                if let Err(err) = wows_toolkit_config::queries::set_setting(&pool, keys::SUPPRESS_GPU_ENCODER_WARNING, &true).await {
                    log::warn!("renderer: the software encode notice setting was not saved: {err}");
                }
            }
            Err(err) => log::warn!("renderer: the software encode notice setting was not saved: {err}"),
        }
    }

    /// Asks where to write, then encodes what viewport `id` shows there.
    /// Resolves once the encode is under way; `false` when the dialog was
    /// cancelled.
    pub async fn export_video(id: u32) -> Result<bool, RendererError> {
        let session = session(id)?;
        let title = session.lock().title().to_owned();
        let dialog = SaveDialog::new()
            .title(t!("ui.replay.renderer.export_video").into_owned())
            .file_name(format!("{title}.mp4"))
            .filter(FileFilter::new("MP4", ["mp4"]));
        let picked = pick_save_path(dialog).await.map_err(|reason| RendererError::Dialog { reason })?;
        let Some(output) = picked else { return Ok(false) };

        start_export(&session, output, false)?;
        Ok(true)
    }

    /// Starts the hover preview of the replay at `path`. It waits the shared
    /// dwell before reading anything.
    pub fn open_preview(path: String) -> PreviewView {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let preview = Preview::open(id, &format!("minimap-preview-{id}"), PathBuf::from(path));
        PREVIEWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id, Arc::clone(&preview));
        preview.view()
    }

    /// Stops preview `id`: the pointer left its row.
    pub fn close_preview(id: u32) {
        let removed = PREVIEWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
        if let Some(preview) = removed {
            preview.close();
        }
    }

    /// Resolves with preview `id` once it differs from `after`.
    pub async fn next_preview_change(id: u32, after: u32) -> Result<PreviewView, RendererError> {
        Ok(preview(id)?.next_change(after).await)
    }

    /// Opens the session's window `replay_id` (decimal), which another end is
    /// playing.
    pub fn open_watched(replay_id: String) -> Result<WatchedView, RendererError> {
        let raw: u64 = replay_id.parse().map_err(|_| RendererError::UnknownWindow { replay_id: replay_id.clone() })?;
        let window = crate::collab::shared_windows()
            .into_iter()
            .find(|window| window.replay_id == raw)
            .ok_or(RendererError::UnknownWindow { replay_id })?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let watched = Watched::open(id, &format!("watched-playback-{id}"), window);
        WATCHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id, Arc::clone(&watched));
        Ok(watched.view())
    }

    /// Stops drawing watched window `id`.
    pub fn close_watched(id: u32) {
        let removed = WATCHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
        if let Some(watched) = removed {
            watched.close();
        }
    }

    /// Resolves with watched window `id` once it differs from `after`.
    pub async fn next_watched_change(id: u32, after: u32) -> Result<WatchedView, RendererError> {
        Ok(watched(id)?.next_change(after).await)
    }

    /// Resolves with the session's windows the host has asked every peer to
    /// open, by replay id, once there are some. Nothing is asked of a reader
    /// who switched automatic opening off.
    pub async fn next_forced_windows() -> Vec<String> {
        let mut revisions = crate::collab::revisions();
        loop {
            let auto_open = !wows_toolkit_config::load_startup_setting::<bool>(keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS)
                .ok()
                .flatten()
                .unwrap_or(false);
            let asked = crate::collab::link().take_forced_windows();
            if auto_open && !asked.is_empty() {
                return asked.into_iter().map(|id| id.to_string()).collect();
            }
            // The sender lives for the process, so the wait only ends on a
            // change.
            let _ = revisions.changed().await;
        }
    }

    /// Renders each replay in `paths` to its own video: into a folder the
    /// reader picks, or onto the clipboard. Resolves once the batch is under
    /// way; `false` when the folder dialog was cancelled.
    pub async fn render_many(paths: Vec<String>, to_clipboard: bool) -> Result<bool, RendererError> {
        if paths.is_empty() {
            return Err(RendererError::NothingToRender);
        }
        if batch::is_running() {
            return Err(RendererError::BatchRunning);
        }
        let (out_dir, target) = if to_clipboard {
            (playback::temporary_batch_dir().ok_or(RendererError::NoTemporaryFile)?, batch::BatchTarget::Clipboard)
        } else {
            let dialog = OpenDialog::folders().title(t!("ui.replay.renderer.export_video").into_owned());
            let picked = pick_paths(dialog).await.map_err(|reason| RendererError::Dialog { reason })?;
            let Some(dir) = picked.and_then(|mut paths| paths.pop()) else { return Ok(false) };
            (dir, batch::BatchTarget::Directory)
        };
        let defaults = defaults().await;
        let game_data = crate::game_data::cache().ok_or(RendererError::NoGameData)?;
        batch::start(paths.into_iter().map(PathBuf::from).collect(), game_data, out_dir, target, defaults);
        Ok(true)
    }

    /// The batch render's progress line.
    pub fn batch() -> BatchView {
        batch::view()
    }

    /// Resolves with the batch render's progress line once it differs from
    /// `after`.
    pub async fn next_batch_change(after: u32) -> BatchView {
        batch::next_change(after).await
    }

    /// Encodes what viewport `id` shows to a temporary file and puts the file
    /// on the clipboard. The file outlives this process: the clipboard holds
    /// its path, and deleting it would paste nothing.
    pub fn export_clipboard(id: u32) -> Result<RendererView, RendererError> {
        let session = session(id)?;
        let title = session.lock().title().to_owned();
        let output = playback::temporary_output(&format!("{title}.mp4")).ok_or(RendererError::NoTemporaryFile)?;
        start_export(&session, output, true)?;
        Ok(current(id, &session))
    }
}

/// Starts encoding viewport `session` to `output` on its own thread.
fn start_export(session: &Arc<Session>, output: PathBuf, to_clipboard: bool) -> Result<(), RendererError> {
    let job = session.update(Viewer::begin_export).ok_or(RendererError::NotReady)?;
    let session = Arc::clone(session);
    std::thread::spawn(move || {
        let outcome = playback::encode_track(&job.renderer, &job.frames, &output, job.settings, |step| {
            session.update(|viewer| viewer.export_progressed(step));
        });
        let outcome = match outcome {
            Ok(()) if to_clipboard => crate::clipboard::copy_files(&[output])
                .map(|()| None)
                .map_err(|reason| reason.to_string()),
            Ok(()) => Ok(Some(output.display().to_string())),
            Err(ExportError::PathUnusable { .. }) => Err(t!("ui.replay.renderer.export_path_unusable").into_owned()),
            Err(err) => Err(err.to_string()),
        };
        session.update(|viewer| viewer.export_finished(job.renderer, outcome));
    });

    Ok(())
}

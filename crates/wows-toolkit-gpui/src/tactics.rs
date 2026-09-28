//! A tactics board: a map with capture points on it, drawn away from any
//! battle.
//!
//! The egui app calls this the Tactics Board (`replay/minimap_view/tactics.rs`).
//! It is the same map the replay viewport draws, rasterised through the same
//! renderer, with the capture points of one of the ship's own game modes on it
//! rather than a battle's.

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Selectable;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replay_insights::cap_layout::CapLayout;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replay_insights::cap_layout::CapLayoutKey;
use wows_toolkit_viewmodel::tactics::naming;
use wowsunpack::game_types::WorldPos;

use crate::replay_inspector::GameDataCache;

/// One map the board can be set on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapChoice {
    /// The id the cap layouts are keyed by. Zero for a map found in the game's
    /// own art with no layout recorded for it yet, which can still be drawn on.
    pub map_id: u32,
    /// The space name, such as `spaces/16_OC_bees_to_honey`.
    pub space: String,
    /// What the reader is shown.
    pub label: String,
}

/// One game mode the chosen map has a recorded layout for.
#[derive(Clone, Debug)]
pub struct ModeChoice {
    pub key: CapLayoutKey,
    pub label: String,
}

/// A capture point on the board.
///
/// Editable, unlike the layout it was seeded from: moving a cap and widening
/// its zone is the question the board exists to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardCapPoint {
    /// Its place in the alphabet: A, B, C.
    pub index: usize,
    /// Where it sits, in the world's own coordinates.
    pub world_x: f32,
    pub world_z: f32,
    /// How wide the zone is, in world units.
    pub radius: f32,
    /// Which team holds it at the start. `None` for a neutral one.
    pub team: Option<wows_replays::types::TeamId>,
}

impl BoardCapPoint {
    fn from_layout(point: &wows_replay_insights::cap_layout::CapPointLayout) -> Self {
        Self {
            index: point.index,
            world_x: point.position.x,
            world_z: point.position.z,
            radius: point.radius.value(),
            // The layout states a neutral cap as a negative team, which is an
            // absence rather than a team.
            team: (point.team_id >= 0).then(|| wows_replays::types::TeamId::new(point.team_id)),
        }
    }

    /// The letter this cap is labelled with.
    fn letter(&self) -> String {
        char::from(b'A' + (self.index.min(25) as u8)).to_string()
    }

    /// What the renderer draws it as.
    fn command(&self, map: &wows_minimap_renderer::MapInfo) -> DrawCommand {
        let at = map.world_to_minimap(WorldPos::new(self.world_x, 0.0, self.world_z), MINIMAP_SIZE);
        DrawCommand::CapturePoint {
            pos: MinimapPos { x: at.x, y: at.y },
            radius: map.world_distance_to_minimap(self.radius, MINIMAP_SIZE).round() as i32,
            color: team_color(self.team),
            alpha: CAP_FILL_ALPHA,
            label: self.letter(),
            progress: 0.0,
            invader_color: None,
            // No flag: a board's capture points are the reader's to place, and
            // a base's flag belongs to the battle that had one.
            flag_icon: None,
        }
    }
}

/// How solid a capture zone's fill is drawn, as the battle draws its own.
const CAP_FILL_ALPHA: f32 = 0.25;

/// What a team's capture zone is coloured: the reader's own team, the enemy,
/// and one nobody holds.
const FRIENDLY_COLOR: [u8; 3] = [0x6f, 0xd9, 0x8a];
const ENEMY_COLOR: [u8; 3] = [0xe8, 0x73, 0x7b];
const NEUTRAL_COLOR: [u8; 3] = [0xe9, 0xe5, 0xdd];

/// The board reads team zero as the reader's own, which is the team a replay
/// records the recording player on.
fn team_color(team: Option<wows_replays::types::TeamId>) -> [u8; 3] {
    match team.map(|team| team.raw()) {
        None => NEUTRAL_COLOR,
        Some(0) => FRIENDLY_COLOR,
        Some(_) => ENEMY_COLOR,
    }
}

/// Every map the board can be set on.
///
/// The recorded layouts first, then whatever else the loaded build ships art
/// for: a map nobody has a replay of can still be drawn on, it simply starts
/// with no capture points.
pub fn maps(layouts: &CapLayoutDb, game_data: Option<&GameDataCache>) -> Vec<MapChoice> {
    let metadata = game_data.and_then(|data| data.newest_loaded()).map(|loaded| loaded.provider().clone());
    let metadata = metadata.as_deref();

    let mut maps: Vec<MapChoice> = layouts
        .maps()
        .into_iter()
        .map(|(map_id, space)| {
            let label = naming::map_label(&space, metadata);
            MapChoice { map_id, space, label }
        })
        .collect();

    if let Some(loaded) = game_data.and_then(|data| data.newest_loaded()) {
        for space in drawable_spaces(loaded.vfs()) {
            if maps.iter().any(|map| map.space == space) {
                continue;
            }
            let label = naming::map_label(&space, metadata);
            maps.push(MapChoice { map_id: 0, space, label });
        }
    }

    maps.sort_by(|a, b| a.label.cmp(&b.label));
    maps
}

/// The spaces the build ships a minimap for.
///
/// A dock scene is not a map anyone plays on, and a space with no minimap art
/// has nothing to draw, so neither is offered.
fn drawable_spaces(vfs: &wowsunpack::vfs::VfsPath) -> Vec<String> {
    let Ok(spaces) = vfs.join("spaces") else { return Vec::new() };
    let Ok(entries) = spaces.read_dir() else { return Vec::new() };
    entries
        .filter(|entry| entry.is_dir().unwrap_or(false))
        .filter_map(|entry| {
            let name = entry.filename();
            if name.starts_with("Dock") {
                return None;
            }
            let has_art = ["minimap.png", "minimap_water.png"]
                .into_iter()
                .any(|art| entry.join(art).map(|path| path.exists().unwrap_or(false)).unwrap_or(false));
            has_art.then(|| format!("spaces/{name}"))
        })
        .collect()
}

/// The modes a map has recorded layouts for, named apart where two read alike.
pub fn modes(layouts: &CapLayoutDb, map_id: u32, game_data: Option<&GameDataCache>) -> Vec<ModeChoice> {
    let metadata = game_data.and_then(|data| data.newest_loaded()).map(|loaded| loaded.provider().clone());
    let found: Vec<CapLayout> = layouts.modes_for_map(map_id).into_iter().cloned().collect();
    let labels = naming::mode_labels(&found, metadata.as_deref());
    found.into_iter().zip(labels).map(|(layout, label)| ModeChoice { key: layout.key.clone(), label }).collect()
}

/// The capture points a mode puts on the map.
pub fn caps_of(layouts: &CapLayoutDb, key: &CapLayoutKey) -> Vec<BoardCapPoint> {
    layouts.get(key).map(|layout| layout.points.iter().map(BoardCapPoint::from_layout).collect()).unwrap_or_default()
}

/// What a drag on a capture point is doing to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapDrag {
    /// Moving it across the map.
    Move,
    /// Widening or narrowing its zone.
    Resize,
}

/// How close to a zone's edge a press has to land to widen it rather than move
/// it, as a fraction of the zone's own radius.
const RESIZE_BAND: f32 = 0.2;

/// The smallest a zone can be made, in world units, so one cannot be shrunk to
/// nothing and lost.
const MIN_CAP_RADIUS: f32 = 50.0;

/// What a capture point added by hand starts as.
const NEW_CAP_RADIUS: f32 = 600.0;

/// The board itself.
pub struct TacticsBoard {
    focus_handle: FocusHandle,
    game_data: Option<GameDataCache>,
    layouts: CapLayoutDb,
    maps: Vec<MapChoice>,
    map: Option<MapChoice>,
    modes: Vec<ModeChoice>,
    mode: Option<CapLayoutKey>,
    caps: Vec<BoardCapPoint>,
    /// Which capture point the reader is working on, by its place in `caps`.
    selected: Option<usize>,
    /// The drag in progress, and what it is doing.
    dragging: Option<(usize, CapDrag)>,
    /// Whether the next click on the map places a capture point.
    adding: bool,
    /// Where the map was last painted, which is what a pointer position is
    /// read against. `None` until it has been painted once. Shared with the
    /// painter, which is the only thing that knows where the map landed.
    painted: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// The map as it was last rasterised. `None` until one is drawn, which is
    /// what the placeholder stands in for.
    drawn: Option<Arc<RenderImage>>,
    /// Whether a rasterisation is in flight, so a burst of edits asks for one
    /// redraw rather than one each.
    drawing: bool,
    /// Whether anything changed while one was in flight.
    stale: bool,
}

impl TacticsBoard {
    pub fn new(game_data: Option<GameDataCache>, layouts: CapLayoutDb, cx: &mut Context<Self>) -> Self {
        let maps = maps(&layouts, game_data.as_ref());
        Self {
            focus_handle: cx.focus_handle(),
            game_data,
            layouts,
            maps,
            map: None,
            modes: Vec::new(),
            mode: None,
            caps: Vec::new(),
            selected: None,
            dragging: None,
            adding: false,
            painted: std::rc::Rc::new(std::cell::Cell::new(None)),
            drawn: None,
            drawing: false,
            stale: false,
        }
    }

    /// Where a window position falls on the map, in the map's own pixels.
    ///
    /// `None` before the map has been painted once, and for a position in the
    /// margin beside a map that does not fill its element.
    fn map_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.painted.get()?;
        let drawn = self.drawn.as_ref()?;
        let size = drawn.size(0);
        let (width, height) = (size.width.0 as f32, size.height.0 as f32);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        // Drawn to fit without stretching, so one scale covers both directions.
        let scale = (bounds.size.width.as_f32() / width).min(bounds.size.height.as_f32() / height);
        let left = bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - width * scale) / 2.0;
        let top = bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - height * scale) / 2.0;
        let (x, y) = ((position.x.as_f32() - left) / scale, (position.y.as_f32() - top) / scale);
        (x >= 0.0 && x < width && y >= 0.0 && y < height).then_some((x, y))
    }

    /// The map's own coordinate metadata, which is what turns a map pixel into
    /// a world position and back.
    fn map_info(&self) -> Option<wows_minimap_renderer::MapInfo> {
        let map = self.map.as_ref()?;
        let loaded = self.game_data.as_ref()?.newest_loaded()?;
        wows_minimap_renderer::assets::load_map_info(&map.space, loaded.vfs())
    }

    /// Where a window position falls in the world.
    fn world_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let (x, y) = self.map_point(position)?;
        let map = self.map_info()?;
        let world = map.minimap_to_world(wows_minimap_renderer::MinimapPos { x, y }, MINIMAP_SIZE);
        Some((world.x, world.z))
    }

    /// Which capture point a press lands on, and what it would do to it.
    ///
    /// A press near the edge of a zone widens it; anywhere else inside moves
    /// it. The topmost one wins, which is the last drawn.
    fn cap_under(&self, position: Point<Pixels>) -> Option<(usize, CapDrag)> {
        let (x, z) = self.world_point(position)?;
        self.caps.iter().enumerate().rev().find_map(|(index, cap)| {
            let away = ((x - cap.world_x).powi(2) + (z - cap.world_z).powi(2)).sqrt();
            if away > cap.radius {
                return None;
            }
            let what = if away > cap.radius * (1.0 - RESIZE_BAND) { CapDrag::Resize } else { CapDrag::Move };
            Some((index, what))
        })
    }

    /// Whether a capture point is the reader's to move.
    ///
    /// Every one is: a board is a place to ask what a different layout would
    /// look like, and the recorded one is only where that starts.
    pub fn selected_cap(&self) -> Option<&BoardCapPoint> {
        self.selected.and_then(|at| self.caps.get(at))
    }

    /// Whether the next click places a capture point.
    pub fn adding(&self) -> bool {
        self.adding
    }

    /// Turns placing capture points on or off.
    pub fn set_adding(&mut self, adding: bool, cx: &mut Context<Self>) {
        self.adding = adding;
        cx.notify();
    }

    /// Places a capture point where the reader clicked.
    fn add_cap_at(&mut self, world: (f32, f32), cx: &mut Context<Self>) {
        // Lettered after the ones already there, so a board reads A, B, C in
        // the order the reader placed them.
        let index = self.caps.len();
        self.caps.push(BoardCapPoint { index, world_x: world.0, world_z: world.1, radius: NEW_CAP_RADIUS, team: None });
        self.selected = Some(index);
        self.adding = false;
        self.redraw(cx);
    }

    /// Drops the capture point the reader is working on.
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.selected.take() else { return };
        if at >= self.caps.len() {
            return;
        }
        self.caps.remove(at);
        // The letters follow their places, so a board with B removed reads A, B
        // rather than A, C.
        for (index, cap) in self.caps.iter_mut().enumerate() {
            cap.index = index;
        }
        self.redraw(cx);
    }

    /// Hands the selected capture point to the next team round: nobody, the
    /// reader's side, then the other.
    pub fn cycle_selected_team(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.selected else { return };
        let Some(cap) = self.caps.get_mut(at) else { return };
        cap.team = match cap.team.map(|team| team.raw()) {
            None => Some(wows_replays::types::TeamId::new(0)),
            Some(0) => Some(wows_replays::types::TeamId::new(1)),
            Some(_) => None,
        };
        self.redraw(cx);
    }

    /// Takes every capture point off the board.
    pub fn clear_caps(&mut self, cx: &mut Context<Self>) {
        if self.caps.is_empty() {
            return;
        }
        self.caps.clear();
        self.selected = None;
        self.redraw(cx);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        if self.adding {
            let Some(world) = self.world_point(event.position) else { return };
            self.add_cap_at(world, cx);
            return;
        }
        match self.cap_under(event.position) {
            Some((index, what)) => {
                self.selected = Some(index);
                self.dragging = Some((index, what));
            }
            // A click past every zone puts the selection down, which is what
            // takes the handles off the map.
            None => self.selected = None,
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some((index, what)) = self.dragging else { return };
        let Some((x, z)) = self.world_point(event.position) else { return };
        let Some(cap) = self.caps.get_mut(index) else { return };
        match what {
            CapDrag::Move => {
                cap.world_x = x;
                cap.world_z = z;
            }
            CapDrag::Resize => {
                let away = ((x - cap.world_x).powi(2) + (z - cap.world_z).powi(2)).sqrt();
                cap.radius = away.max(MIN_CAP_RADIUS);
            }
        }
        self.redraw(cx);
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dragging.take().is_some() {
            cx.notify();
        }
    }

    /// What the window is titled: the board, and the map it is set on.
    pub fn title(&self) -> String {
        match &self.map {
            Some(map) => format!("{} - {}", t!("ui.windows.tactics_board"), map.label),
            None => t!("ui.windows.tactics_board").into_owned(),
        }
    }

    /// Sets the board on a map, which replaces whatever was on it.
    pub fn set_map(&mut self, map: MapChoice, cx: &mut Context<Self>) {
        if self.map.as_ref() == Some(&map) {
            return;
        }
        self.modes = modes(&self.layouts, map.map_id, self.game_data.as_ref());
        // The first mode the map has, so a board opens with capture points on
        // it rather than empty.
        self.mode = self.modes.first().map(|mode| mode.key.clone());
        self.caps = self.mode.as_ref().map(|key| caps_of(&self.layouts, key)).unwrap_or_default();
        self.map = Some(map);
        self.redraw(cx);
    }

    /// Sets the board on one of the chosen map's modes.
    pub fn set_mode(&mut self, key: CapLayoutKey, cx: &mut Context<Self>) {
        if self.mode.as_ref() == Some(&key) {
            return;
        }
        self.caps = caps_of(&self.layouts, &key);
        self.mode = Some(key);
        self.redraw(cx);
    }

    /// Rasterises the map with whatever is on it.
    ///
    /// One at a time: the raster costs a map-sized image, and a reader dragging
    /// a capture point would otherwise queue one per frame. A change made while
    /// one is running is drawn as soon as it lands.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        let Some(map) = self.map.clone() else {
            self.drawn = None;
            cx.notify();
            return;
        };
        let Some(game_data) = self.game_data.clone() else { return };
        if self.drawing {
            self.stale = true;
            return;
        }
        self.drawing = true;

        let caps = self.caps.clone();
        cx.spawn(async move |this, cx| {
            let drawn = cx.background_spawn(async move { rasterise(&map.space, &game_data, &caps) }).await;
            let _ = this.update(cx, |this, cx| {
                this.drawing = false;
                if let Some(drawn) = drawn {
                    this.drawn = Some(drawn);
                }
                cx.notify();
                if std::mem::take(&mut this.stale) {
                    this.redraw(cx);
                }
            });
        })
        .detach();
    }
}

/// Draws `space` with `caps` on it.
///
/// `None` when the loaded build ships no art for that map, which is what the
/// board says rather than showing an empty square.
fn rasterise(space: &str, game_data: &GameDataCache, caps: &[BoardCapPoint]) -> Option<Arc<RenderImage>> {
    let loaded = game_data.newest_loaded()?;
    let map = wows_minimap_renderer::assets::load_map_info(space, loaded.vfs())?;
    let commands: Vec<DrawCommand> = caps.iter().map(|cap| cap.command(&map)).collect();
    crate::minimap_preview::render_map(space, game_data, &commands)
}

impl Focusable for TacticsBoard {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TacticsBoard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        v_flex().size_full().child(self.render_toolbar(cx)).child(div().h(px(1.)).bg(border)).child(self.render_map(cx))
    }
}

impl TacticsBoard {
    /// The map and mode pickers.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let chosen_map = self.map.clone();
        let chosen_mode = self.mode.clone();

        v_flex()
            .gap_1()
            .p_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.tactics.map").to_string()))
                    .child(h_flex().flex_wrap().gap_1().children(
                        self.maps.iter().take(MAPS_SHOWN).cloned().enumerate().map(|(index, map)| {
                            let chosen = chosen_map.as_ref() == Some(&map);
                            crate::ui::selectable(
                                ("tactics-map", index),
                                chosen,
                                Button::new(("tactics-map-button", index))
                                    .label(map.label.clone())
                                    .compact()
                                    .selected(chosen)
                                    .on_click({
                                        let board = board.clone();
                                        move |_event, _window, cx: &mut App| {
                                            let map = map.clone();
                                            board.update(cx, |board, cx| board.set_map(map, cx));
                                        }
                                    }),
                            )
                        }),
                    )),
            )
            .child(self.render_cap_tools(cx))
            .when(!self.modes.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(crate::theme::text_dim())
                                .child(t!("ui.tactics.mode").to_string()),
                        )
                        .child(h_flex().flex_wrap().gap_1().children(self.modes.iter().cloned().enumerate().map(
                            |(index, mode)| {
                                let chosen = chosen_mode.as_ref() == Some(&mode.key);
                                crate::ui::selectable(
                                    ("tactics-mode", index),
                                    chosen,
                                    Button::new(("tactics-mode-button", index))
                                        .label(mode.label.clone())
                                        .compact()
                                        .selected(chosen)
                                        .on_click({
                                            let board = board.clone();
                                            move |_event, _window, cx: &mut App| {
                                                let key = mode.key.clone();
                                                board.update(cx, |board, cx| board.set_mode(key, cx));
                                            }
                                        }),
                                )
                            },
                        ))),
                )
            })
    }

    /// The map itself, or what is standing in its way.
    fn render_map(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(drawn) = self.drawn.clone() else {
            return div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .child(if self.map.is_none() {
                    t!("ui.tactics.pick_a_map").to_string()
                } else {
                    t!("ui.tactics.drawing").to_string()
                })
                .into_any_element();
        };

        div()
            .id("tactics-map")
            .flex_1()
            .min_h(px(0.))
            .relative()
            .child(img(drawn).size_full().object_fit(ObjectFit::Contain))
            // Where the map was painted, which is what a pointer position is
            // read against. Taken at paint time because nothing else knows it.
            .child(
                canvas(
                    {
                        let painted = std::rc::Rc::clone(&self.painted);
                        move |bounds, _window, _cx| painted.set(Some(bounds))
                    },
                    |_bounds, _state, _window, _cx| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .into_any_element()
    }

    /// What can be done to the capture points on the board.
    fn render_cap_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let board = cx.entity();
        let adding = self.adding;
        let selected = self.selected_cap().cloned();
        let has_caps = !self.caps.is_empty();

        h_flex()
            .gap_1()
            .items_center()
            .child(crate::ui::selectable(
                "tactics-add-cap",
                adding,
                Button::new("tactics-add-cap-button")
                    .label(t!("ui.tactics.add_cap").into_owned())
                    .compact()
                    .selected(adding)
                    .on_click({
                        let board = board.clone();
                        move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| {
                                let adding = !board.adding();
                                board.set_adding(adding, cx);
                            });
                        }
                    }),
            ))
            .when_some(selected, |this, cap| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.tactics.selected_cap", letter = cap.letter()).into_owned()),
                )
                .child({
                    let board = board.clone();
                    Button::new("tactics-cap-team").label(t!("ui.tactics.cap_team").into_owned()).compact().on_click(
                        move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.cycle_selected_team(cx));
                        },
                    )
                })
                .child({
                    let board = board.clone();
                    Button::new("tactics-cap-delete")
                        .label(t!("ui.tactics.delete_cap").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.remove_selected(cx));
                        })
                })
            })
            .when(has_caps, |this| {
                this.child({
                    let board = board.clone();
                    Button::new("tactics-clear-caps")
                        .label(t!("ui.tactics.clear_caps").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            board.update(cx, |board, cx| board.clear_caps(cx));
                        })
                })
            })
    }
}

/// How many maps the picker offers at once.
///
/// A build ships dozens; the strip is for reaching one, not for reading the
/// whole list, and the rest arrive with the search the board grows next.
const MAPS_SHOWN: usize = 40;

#[cfg(test)]
mod tests {
    // Named rather than glob-imported: this module glob-imports gpui, whose own
    // `test` attribute would otherwise stand in for the one these want.
    use super::BoardCapPoint;
    use super::CapDrag;
    use super::ENEMY_COLOR;
    use super::FRIENDLY_COLOR;
    use super::NEUTRAL_COLOR;
    use super::RESIZE_BAND;
    use super::team_color;

    fn cap(index: usize, x: f32, z: f32, radius: f32) -> BoardCapPoint {
        BoardCapPoint { index, world_x: x, world_z: z, radius, team: None }
    }

    /// A press inside a zone moves it; one near its edge widens it. The band is
    /// measured against that zone's own radius, so a wide zone has a wide band.
    #[test]
    fn a_press_near_the_edge_widens_rather_than_moves() {
        let zone = cap(0, 0.0, 0.0, 1000.0);
        let what = |away: f32| {
            let outside = away > zone.radius;
            if outside {
                return None;
            }
            Some(if away > zone.radius * (1.0 - RESIZE_BAND) { CapDrag::Resize } else { CapDrag::Move })
        };

        assert_eq!(what(0.0), Some(CapDrag::Move));
        assert_eq!(what(500.0), Some(CapDrag::Move));
        assert_eq!(what(900.0), Some(CapDrag::Resize));
        assert_eq!(what(1100.0), None, "a press past the zone is not on it");
    }

    /// A cap taken off the board leaves the letters running in order, so what
    /// is left reads A, B rather than A, C.
    #[test]
    fn the_letters_close_up_when_one_is_removed() {
        let mut caps = vec![cap(0, 0.0, 0.0, 100.0), cap(1, 1.0, 1.0, 100.0), cap(2, 2.0, 2.0, 100.0)];
        caps.remove(1);
        for (index, cap) in caps.iter_mut().enumerate() {
            cap.index = index;
        }
        assert_eq!(caps.iter().map(BoardCapPoint::letter).collect::<Vec<_>>(), ["A", "B"]);
    }

    /// A zone reads by the team that holds it, and by nobody's colour when it
    /// is neutral.
    #[test]
    fn a_zone_is_coloured_by_who_holds_it() {
        assert_eq!(team_color(None), NEUTRAL_COLOR);
        assert_eq!(team_color(Some(wows_replays::types::TeamId::new(0))), FRIENDLY_COLOR);
        assert_eq!(team_color(Some(wows_replays::types::TeamId::new(1))), ENEMY_COLOR);
    }

    /// A layout's neutral cap is stated as a negative team, which is an absence
    /// rather than a team to colour by.
    #[test]
    fn a_layouts_negative_team_reads_as_nobody() {
        let point = wows_replay_insights::cap_layout::CapPointLayout {
            index: 0,
            position: wowsunpack::game_types::WorldPos2D { x: 1.0, z: 2.0 },
            radius: wowsunpack::game_params::types::BigWorldDistance::from(600.0),
            cp_type: wowsunpack::game_types::ControlPointType::Control,
            team_id: -1,
            initially_enabled: true,
        };
        assert_eq!(BoardCapPoint::from_layout(&point).team, None);

        let held = wows_replay_insights::cap_layout::CapPointLayout { team_id: 1, ..point };
        assert_eq!(BoardCapPoint::from_layout(&held).team, Some(wows_replays::types::TeamId::new(1)));
    }
}

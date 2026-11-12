//! Drawing over a battle: the tools, picking drawn shapes out to move and
//! turn, and undoing what this end changed.
//!
//! What is drawn lives in the session's [`CollabLink`], which holds it even
//! when nobody else is connected; this is the pointer handling and the
//! bookkeeping around it.

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_toolkit_icons::Glyph;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;
use wt_collab_client::drawing::Drawing;
use wt_collab_client::drawing::Drawn;
use wt_collab_client::drawing::Held;
use wt_collab_client::drawing::Selection;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;
use wt_collab_client::drawing::annotation_bounds;
use wt_collab_client::drawing::bearing;
use wt_collab_client::drawing::can_rotate;
use wt_collab_client::drawing::move_annotation;
use wt_collab_client::drawing::rotate_annotation;
use wt_collab_client::geometry::annotation_commands;
use wt_collab_client::geometry::ship_range_commands;
use wt_collab_client::types::Annotation;
use wt_collab_client::types::AnnotationShipConfig;

use crate::load::GameDataCache;
use crate::replay_render::track::Track;
use crate::replay_render::view::ElementPoint;
use crate::replay_render::view::MapView;
use crate::replay_render::view::WindowPoint;

/// How many ships a search offers at once.
pub const SHIP_MATCHES: usize = 10;

/// How many changes back an undo can reach.
pub const HISTORY_DEPTH: usize = 32;

/// How big a rotation handle is drawn, and how far above the shape it sits.
/// The egui renderer's own figures, in element pixels at any zoom.
pub const HANDLE_RADIUS: f32 = 5.0;
pub const HANDLE_DISTANCE: f32 = 25.0;

/// What a tool draws with until the reader says otherwise: the white and the
/// width the egui toolbar opens on.
pub const DEFAULT_INK: [u8; 4] = [255, 255, 255, 255];
pub const DEFAULT_NIB: f32 = 2.0;

/// What the bracket keys hold the nib between, as the egui board does.
pub const MIN_NIB: f32 = 1.0;
pub const MAX_NIB: f32 = 8.0;

/// The ships that can be placed on the map, as the egui toolbar offers
/// them: short name, then the species a placed ship carries.
pub const SPECIES: &[(&str, &str)] =
    &[("DD", "Destroyer"), ("CA", "Cruiser"), ("BB", "Battleship"), ("CV", "AirCarrier"), ("SS", "Submarine")];

/// What a placed ship's button is tinted by which side it is on, as RGB.
pub const FRIENDLY_TINT: u32 = 0x4CE8AA;
pub const ENEMY_TINT: u32 = 0xFE4D2A;

/// What a tool draws with, as the egui toolbar offers it: an element id, then
/// the RGBA ink.
pub const INKS: &[(&str, [u8; 4])] = &[
    ("replay-renderer-ink-white", [255, 255, 255, 255]),
    ("replay-renderer-ink-gray", [160, 160, 160, 255]),
    ("replay-renderer-ink-red", [230, 50, 50, 255]),
    ("replay-renderer-ink-orange", [240, 140, 30, 255]),
    ("replay-renderer-ink-yellow", [240, 230, 50, 255]),
    ("replay-renderer-ink-green", [50, 200, 50, 255]),
    ("replay-renderer-ink-blue", [50, 120, 230, 255]),
    ("replay-renderer-ink-purple", [180, 60, 230, 255]),
    ("replay-renderer-ink-pink", [255, 130, 180, 255]),
];

/// One line in the cheat sheet's actions: the chord, and the translation key
/// of what it does.
pub const ANNOTATION_ACTIONS: &[(&str, &str)] = &[
    ("Ctrl+Z", "ui.renderer.annotations.action_undo"),
    ("Ctrl+Click", "ui.renderer.annotations.action_multi_select"),
    ("[ / ]", "ui.renderer.annotations.action_stroke_width"),
    ("Del", "ui.renderer.annotations.action_delete"),
    ("Esc", "ui.renderer.annotations.action_cancel"),
];

/// One button on the toolbar: what it shows, what it is called, the chord that
/// takes it up, and what it puts in hand.
pub struct ToolButton {
    pub id: &'static str,
    pub glyph: Glyph,
    pub label_key: &'static str,
    /// `None` for the one tool that has no chord of its own: the selector, which
    /// escape goes back to.
    pub chord: Option<&'static str>,
    pub tool: fn() -> Tool,
}

/// The tools the toolbar offers, in the order the egui toolbar offers them.
///
/// Selecting is `Tool::None`: nothing is being drawn, so the pointer goes to
/// the map as it otherwise would.
pub const TOOLS: &[ToolButton] = &[
    ToolButton {
        id: "replay-renderer-tool-select",
        glyph: Glyph::MousePointer,
        label_key: "ui.renderer.annotations.tool_select",
        chord: None,
        tool: || Tool::None,
    },
    ToolButton {
        id: "replay-renderer-tool-arrow",
        glyph: Glyph::MoveUpRight,
        label_key: "ui.renderer.annotations.tool_arrow",
        chord: Some("Ctrl+1"),
        tool: || Tool::Arrow,
    },
    ToolButton {
        id: "replay-renderer-tool-freehand",
        glyph: Glyph::Paintbrush,
        label_key: "ui.renderer.annotations.tool_freehand",
        chord: Some("Ctrl+2"),
        tool: || Tool::Freehand,
    },
    ToolButton {
        id: "replay-renderer-tool-eraser",
        glyph: Glyph::Eraser,
        label_key: "ui.renderer.annotations.tool_eraser",
        chord: Some("Ctrl+3"),
        tool: || Tool::Eraser,
    },
    ToolButton {
        id: "replay-renderer-tool-line",
        glyph: Glyph::Slash,
        label_key: "ui.renderer.annotations.tool_line",
        chord: Some("Ctrl+4"),
        tool: || Tool::Line,
    },
    ToolButton {
        id: "replay-renderer-tool-circle",
        glyph: Glyph::Circle,
        label_key: "ui.renderer.annotations.tool_circle",
        chord: Some("Ctrl+5"),
        tool: || Tool::Circle { filled: false },
    },
    ToolButton {
        id: "replay-renderer-tool-rectangle",
        glyph: Glyph::Square,
        label_key: "ui.renderer.annotations.tool_rectangle",
        chord: Some("Ctrl+6"),
        tool: || Tool::Rectangle { filled: false },
    },
    ToolButton {
        id: "replay-renderer-tool-triangle",
        glyph: Glyph::Triangle,
        label_key: "ui.renderer.annotations.tool_triangle",
        chord: Some("Ctrl+7"),
        tool: || Tool::Triangle { filled: false },
    },
    ToolButton {
        id: "replay-renderer-tool-measure",
        glyph: Glyph::Ruler,
        label_key: "ui.renderer.annotations.tool_measurement",
        chord: Some("Ctrl+M"),
        tool: || Tool::Measurement,
    },
];

/// The handle a single picked shape is turned by.
///
/// Above the shape by a fixed number of element pixels rather than a map
/// distance, so the handle stays the same size and the same reach away at
/// every zoom, as the egui renderer's does. A front end draws it over the
/// frame rather than into it, so it stays out of an exported video.
#[derive(Clone, Debug, PartialEq)]
pub struct RotationHandle {
    pub index: usize,
    pub annotation: Annotation,
    /// The middle of the handle.
    pub handle: ElementPoint,
    /// Where the stem from the handle meets the shape.
    pub anchor: ElementPoint,
}

/// What a press on the map went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerDown {
    /// The press was not over the map.
    OffMap,
    /// The tool in hand began a shape.
    Stroked,
    /// A rotation handle was taken hold of.
    Turning,
    /// A move of the picked shapes began.
    Moving,
    /// Nothing here wants it, so it may pan the map.
    Unclaimed,
}

/// What a pointer move did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerMove {
    /// A held handle turned its shape.
    Turned,
    /// The tool in hand carried its shape on.
    Stroked,
    /// A tool is in hand but has nothing to draw; the map does not pan.
    Held,
    /// Nothing here wants it, so it may pan the map.
    Unclaimed,
}

/// What letting go of the pointer finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerUp {
    Stroked,
    Turned,
    Moved,
    Nothing,
}

/// What a click on the map did.
///
/// A click that cleared the selection reports `Ignored` or `ResetView`, so a
/// front end reads [`Annotator::picked`] again after any click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// The tool in hand placed or rubbed out a shape.
    Stroked,
    /// A drawn shape was picked out.
    Picked,
    /// A double-click with nothing to pick, which puts the whole map back.
    ResetView,
    Ignored,
}

/// The tool in hand, what is picked out, and what this end can undo.
pub struct Annotator {
    drawing: Drawing,
    /// Where the pointer last was on the map, which is what a part-drawn
    /// shape is previewed against.
    pointer_at: Option<[f32; 2]>,
    /// What the reader has picked out to move, with no tool in hand.
    picked: Selection,
    moving: Option<Move>,
    turning: Option<Turn>,
    /// What the session held before each change this end made, newest last.
    /// Bounded: a session left open all evening must not grow a history of
    /// every stroke in it.
    history: Vec<Vec<Held>>,
}

/// Where a move of the picked shapes began, in map space, and what they
/// looked like then. Kept so a move is one update at the end rather than one
/// per pixel of the drag.
struct Move {
    from: [f32; 2],
    before: Vec<Annotation>,
}

/// The shape being turned by its handle, and what it was before.
struct Turn {
    index: usize,
    before: Annotation,
}

impl Annotator {
    pub fn new() -> Self {
        Self {
            drawing: Drawing::new(DEFAULT_INK, DEFAULT_NIB),
            pointer_at: None,
            picked: Selection::default(),
            moving: None,
            turning: None,
            history: Vec::new(),
        }
    }

    /// The tool in hand, its ink and its nib.
    pub fn drawing(&self) -> &Drawing {
        &self.drawing
    }

    pub fn picked(&self) -> &Selection {
        &self.picked
    }

    /// Where the pointer last was on the map.
    pub fn pointer_at(&self) -> Option<[f32; 2]> {
        self.pointer_at
    }

    /// How many changes an undo can take back.
    pub fn undo_depth(&self) -> usize {
        self.history.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Whether a drawing tool is in hand, which is what decides who gets the
    /// pointer.
    pub fn has_tool(&self) -> bool {
        *self.drawing.tool() != Tool::None
    }

    /// Takes up `tool`, dropping whatever was half-drawn.
    pub fn take_up(&mut self, tool: Tool) {
        self.drawing.set_tool(tool);
    }

    pub fn set_ink(&mut self, ink: [u8; 4]) {
        self.drawing.set_color(ink);
    }

    /// Widens or narrows the nib by `step`, within the range the egui board
    /// offers. Returns whether it changed.
    pub fn step_nib(&mut self, step: f32) -> bool {
        let width = (self.drawing.width() + step).clamp(MIN_NIB, MAX_NIB);
        if (width - self.drawing.width()).abs() < f32::EPSILON {
            return false;
        }
        self.drawing.set_width(width);

        true
    }

    /// Remembers what the session holds, before changing it.
    ///
    /// Only while a session is running, since the undo is a list of what to
    /// tell it.
    pub fn remember(&mut self, link: &CollabLink) {
        if !link.is_active() {
            return;
        }
        self.history.push(link.annotations_held());
        if self.history.len() > HISTORY_DEPTH {
            self.history.remove(0);
        }
    }

    /// Puts the session back the way it was before the last change this end
    /// made. Returns whether there was one.
    pub fn undo(&mut self, link: &CollabLink) -> bool {
        let Some(was) = self.history.pop() else { return false };
        link.restore(&was);
        // What was picked out may no longer be there.
        self.picked.clear();

        true
    }

    /// Erases whatever is picked out. Returns whether anything was.
    pub fn erase_picked(&mut self, link: &CollabLink) -> bool {
        if self.picked.is_empty() {
            return false;
        }
        self.remember(link);

        // Highest index first: erasing shifts what is after it.
        let mut picked: Vec<usize> = self.picked.picked().to_vec();
        picked.sort_unstable_by(|a, b| b.cmp(a));
        for index in picked {
            link.erase_annotation(index);
        }
        self.picked.clear();

        true
    }

    /// Hands a pointer event to the tool and does what it drew.
    ///
    /// The frame carries the part-drawn shape, so a front end redraws after
    /// every stroke.
    pub fn stroke(&mut self, link: &CollabLink, stroke: Stroke) {
        let existing = link.annotations();
        match self.drawing.handle(stroke, &existing) {
            Some(Drawn::Added(annotation)) => {
                self.remember(link);
                link.add_annotation(annotation);
            }
            Some(Drawn::Erased(index)) => {
                self.remember(link);
                link.erase_annotation(index);
            }
            // The stroke carried a shape on without finishing one.
            None => {}
        }
    }

    /// Picks out the drawn shape nearest `at`, a map point, or adds it to
    /// what is picked when `add` is set.
    pub fn pick_at(&mut self, link: &CollabLink, at: [f32; 2], add: bool) {
        let annotations = link.annotations();
        self.picked.click(&annotations, at, add);
        self.picked.retain_within(&annotations);
    }

    /// Whether the picked shape is a ship, and what it is called if one has
    /// been chosen for it.
    pub fn picked_ship(&self, link: &CollabLink) -> Option<Option<String>> {
        let index = self.picked.single()?;
        let Annotation::Ship { config, .. } = link.annotation_at(index)? else {
            return None;
        };

        Some(config.as_ref().map(|config| config.ship_name.clone()).filter(|name| !name.is_empty()))
    }

    /// Gives the picked ship an identity, which is what its ranges are read
    /// from. Returns whether it was given one.
    ///
    /// `param_index` is the ship's game param index; a ship the loaded build
    /// cannot resolve is not named rather than named with an id of nothing.
    pub fn name_picked_ship(
        &mut self,
        link: &CollabLink,
        game_data: &GameDataCache,
        species: Species,
        param_index: &str,
        display_name: String,
    ) -> bool {
        let Some(index) = self.picked.single() else { return false };
        let Some(Annotation::Ship { pos, yaw, friendly, .. }) = link.annotation_at(index) else {
            return false;
        };
        let Some(loaded) = game_data.newest_loaded() else { return false };
        let Some(param) = loaded.provider().game_param_by_index(param_index) else { return false };

        self.remember(link);
        link.update_annotation(
            index,
            Annotation::Ship {
                pos,
                yaw,
                species: format!("{species:?}"),
                friendly,
                // Stock hull and no modifiers until the reader says otherwise,
                // which is where the egui chooser leaves it.
                config: Some(AnnotationShipConfig::new(param.id(), display_name)),
            },
        );

        true
    }

    /// The handle the single picked shape is turned by, when it has a bearing
    /// at all and is on screen.
    ///
    /// Turning several at once about their own middles is not what one handle
    /// means, and a circle looks the same at every angle.
    pub fn rotation_handle(&self, link: &CollabLink, view: &MapView, map_origin: (f32, f32)) -> Option<RotationHandle> {
        let index = self.picked.single()?;
        let annotation = link.annotation_at(index)?;
        if !can_rotate(&annotation) {
            return None;
        }
        let [left, top, right, _] = annotation_bounds(&annotation);
        let anchor = view.element_point(((left + right) / 2.0, top), map_origin)?;
        let handle = ElementPoint { x: anchor.x, y: anchor.y - HANDLE_DISTANCE };

        Some(RotationHandle { index, annotation, handle, anchor })
    }

    /// The shape whose rotation handle is under `position`, if the pointer is
    /// on one.
    fn handle_under(
        &self,
        link: &CollabLink,
        view: &MapView,
        map_origin: (f32, f32),
        position: WindowPoint,
    ) -> Option<Turn> {
        // A little past the drawn handle, so it does not take a steady hand.
        const REACH_MARGIN: f32 = 8.0;

        let handle = self.rotation_handle(link, view, map_origin)?;
        let at = view.to_element(position)?;
        let away = (at.x - handle.handle.x).hypot(at.y - handle.handle.y);

        (away < HANDLE_RADIUS + REACH_MARGIN).then_some(Turn { index: handle.index, before: handle.annotation })
    }

    /// A press on the map at `position`.
    ///
    /// A tool in hand takes the press: a reader dragging to draw is not
    /// asking to pan the map under their line. Nor is one taking hold of a
    /// rotation handle, or dragging what they have already picked out.
    pub fn pointer_down(
        &mut self,
        link: &CollabLink,
        view: &MapView,
        map_origin: (f32, f32),
        position: WindowPoint,
    ) -> PointerDown {
        let Some(at) = view.map_point(position, map_origin) else { return PointerDown::OffMap };
        if self.has_tool() {
            self.stroke(link, Stroke::Began { at: [at.0, at.1] });
            return PointerDown::Stroked;
        }

        if let Some(turn) = self.handle_under(link, view, map_origin, position) {
            // Snapshotted as the handle is taken hold of: a turn reaches the
            // session as the pointer moves, so by the time it is let go the
            // old bearing has already gone.
            self.remember(link);
            self.turning = Some(turn);
            return PointerDown::Turning;
        }

        if !self.picked.is_empty() {
            self.moving = Some(Move { from: [at.0, at.1], before: link.annotations() });
            return PointerDown::Moving;
        }

        PointerDown::Unclaimed
    }

    /// The pointer moved to `position`. `held` is whether the button that
    /// pressed is still down, and `straight` whether the reader asks for a
    /// straight line.
    pub fn pointer_move(
        &mut self,
        link: &CollabLink,
        view: &MapView,
        map_origin: (f32, f32),
        position: WindowPoint,
        held: bool,
        straight: bool,
    ) -> PointerMove {
        self.pointer_at = view.map_point(position, map_origin).map(|(x, y)| [x, y]);

        if let Some(turn) = &self.turning
            && held
            && let Some(at) = self.pointer_at
        {
            let [left, top, right, bottom] = annotation_bounds(&turn.before);
            let middle = [(left + right) / 2.0, (top + bottom) / 2.0];
            let mut turned = turn.before.clone();
            rotate_annotation(&mut turned, bearing(middle, at));
            link.update_annotation(turn.index, turned);
            return PointerMove::Turned;
        }

        if !self.has_tool() {
            return PointerMove::Unclaimed;
        }
        if self.drawing.is_drawing()
            && held
            && let Some(at) = self.pointer_at
        {
            self.stroke(link, Stroke::Moved { at, straight });
            return PointerMove::Stroked;
        }

        PointerMove::Held
    }

    /// The pointer was let go at `position`.
    ///
    /// A turn and a move each reach the session as one update at the end
    /// rather than one per pixel of the drag.
    pub fn pointer_up(
        &mut self,
        link: &CollabLink,
        view: &MapView,
        map_origin: (f32, f32),
        position: WindowPoint,
    ) -> PointerUp {
        if self.has_tool()
            && self.drawing.is_drawing()
            && let Some(at) = view.map_point(position, map_origin)
        {
            self.stroke(link, Stroke::Ended { at: [at.0, at.1] });
            return PointerUp::Stroked;
        }

        if let Some(turn) = self.turning.take() {
            if let Some(annotation) = link.annotation_at(turn.index) {
                link.update_annotation(turn.index, annotation);
            }
            return PointerUp::Turned;
        }

        if let Some(moving) = self.moving.take()
            && let Some((x, y)) = view.map_point(position, map_origin)
        {
            self.remember(link);
            let delta = [x - moving.from[0], y - moving.from[1]];
            for index in self.picked.picked() {
                let Some(annotation) = moving.before.get(*index) else { continue };
                let mut moved = annotation.clone();
                move_annotation(&mut moved, delta);
                link.update_annotation(*index, moved);
            }
            return PointerUp::Moved;
        }

        PointerUp::Nothing
    }

    /// A click on the map at `position`. `add` is whether the reader is adding
    /// to what is picked, and `click_count` how many clicks this one ends.
    ///
    /// A tool that places rather than drags wants the click, and a
    /// double-click must not put the whole map back under a half-drawn
    /// shape. With no tool, a click picks a drawn shape out to move, but only
    /// when there is something to pick: otherwise every click on the map would
    /// swallow the double-click that puts the whole map back.
    pub fn click(
        &mut self,
        link: &CollabLink,
        view: &MapView,
        map_origin: (f32, f32),
        position: WindowPoint,
        add: bool,
        click_count: usize,
    ) -> Click {
        let at = view.map_point(position, map_origin);
        if self.has_tool() {
            let Some(at) = at else { return Click::Ignored };
            self.stroke(link, Stroke::Clicked { at: [at.0, at.1] });
            return Click::Stroked;
        }

        if let Some((x, y)) = at
            && link.annotation_count() > 0
        {
            self.pick_at(link, [x, y], add);
            if !self.picked.is_empty() {
                return Click::Picked;
            }
        }

        if click_count < 2 {
            return Click::Ignored;
        }

        Click::ResetView
    }

    /// What is drawn over the battle: every shape in the session, the range
    /// circles of every placed ship, and the shape under the pointer drawn the
    /// way the finished one will be, so what a reader sees while dragging is
    /// what they get.
    pub fn overlay_commands(&self, link: &CollabLink, track: &Track, game_data: &GameDataCache) -> Vec<DrawCommand> {
        let annotations = link.annotations();
        let mut drawn_on: Vec<DrawCommand> = annotations.iter().flat_map(annotation_commands).collect();
        drawn_on.extend(placed_ship_ranges(&annotations, track, game_data));
        if let Some(at) = self.pointer_at
            && let Some(part_drawn) = self.drawing.in_progress(at)
        {
            drawn_on.extend(annotation_commands(&part_drawn));
        }

        drawn_on
    }
}

impl Default for Annotator {
    fn default() -> Self {
        Self::new()
    }
}

/// The range circles every placed ship asks for.
///
/// Resolved here rather than in the shared collab layer because a range is
/// read out of the ship's own game data, which only this end has. Nothing is
/// drawn until a ship has been chosen for the annotation, since there is no
/// ship to read the ranges of.
pub fn placed_ship_ranges(annotations: &[Annotation], track: &Track, game_data: &GameDataCache) -> Vec<DrawCommand> {
    if annotations.is_empty() {
        return Vec::new();
    }
    let Some(loaded) = game_data.newest_loaded() else { return Vec::new() };
    let provider = loaded.provider();

    let mut circles = Vec::new();
    for annotation in annotations {
        let Annotation::Ship { config: Some(config), .. } = annotation else { continue };
        let Some(param) = provider.game_param_by_id(config.param_id) else { continue };
        let Some(vehicle) = param.vehicle() else { continue };
        let hull = config.hull_name.as_deref();
        let ranges = vehicle.resolve_ranges(Some(provider.as_ref()), hull, track.version);
        circles.extend(ship_range_commands(annotation, &ranges, track.space_size));
    }

    circles
}

/// The tool a key takes up with ctrl held, in the order the egui board numbers
/// them (`replay/minimap_view/shapes.rs:88-104`): the two apps' readers learn
/// one set of keys. `m` is the measurement, as it is there.
pub fn tool_for_key(key: &str) -> Option<Tool> {
    Some(match key {
        "1" => Tool::Arrow,
        "2" => Tool::Freehand,
        "3" => Tool::Eraser,
        "4" => Tool::Line,
        "5" => Tool::Circle { filled: false },
        "6" => Tool::Rectangle { filled: false },
        "7" => Tool::Triangle { filled: false },
        "m" => Tool::Measurement,
        _ => return None,
    })
}

/// Whether two tools are the same one, ignoring whether a shape is filled.
///
/// A reader pressing the circle button while the circle tool is in hand is
/// not choosing a different tool, so the button reads as already chosen.
pub fn same_tool(chosen: &Tool, other: &Tool) -> bool {
    std::mem::discriminant(chosen) == std::mem::discriminant(other)
}

/// The tool that places a ship of `species` for one side.
pub fn ship_tool(species: &str, friendly: bool) -> Tool {
    Tool::Ship { species: species.to_owned(), friendly, yaw: 0.0 }
}

/// Whether `chosen` places ships of `species` for that side, which is when
/// its button reads as chosen.
pub fn places_ship(chosen: &Tool, species: &str, friendly: bool) -> bool {
    matches!(chosen, Tool::Ship { species: in_hand, friendly: side, .. } if in_hand == species && *side == friendly)
}

/// A ship the loaded build knows, as the chooser for a placed ship offers it.
#[derive(Clone, Debug)]
pub struct PlaceableShip {
    pub species: Species,
    /// The game param index [`Annotator::name_picked_ship`] resolves.
    pub param_index: String,
    pub display_name: String,
    pub tier: u32,
    key: FilterKey,
}

impl PlaceableShip {
    /// What the chooser's row says: the tier in roman numerals, then the name.
    pub fn label(&self) -> String {
        const ROMAN: [&str; 11] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI"];

        let tier = self.tier.checked_sub(1).and_then(|index| ROMAN.get(index as usize)).copied().unwrap_or("?");

        format!("{tier} {}", self.display_name)
    }
}

/// Every ship a placed ship can be given, as the egui chooser lists them: playable species only,
/// clan rentals left out since they duplicate a real ship.
pub struct ShipCatalog {
    ships: Vec<PlaceableShip>,
}

impl ShipCatalog {
    pub fn build(metadata: &GameMetadataProvider) -> Self {
        const SHIP_SPECIES: &[Species] = &[
            Species::AirCarrier,
            Species::Battleship,
            Species::Cruiser,
            Species::Destroyer,
            Species::Submarine,
            Species::Auxiliary,
        ];

        let mut ships: Vec<PlaceableShip> = metadata
            .params()
            .iter()
            .filter_map(|param| {
                let species = *param.species()?.known().filter(|species| SHIP_SPECIES.contains(species))?;
                let vehicle = param.vehicle()?;
                if vehicle.group() == "clan" {
                    return None;
                }
                // A ship the build has no name for is still one that can be chosen, by its param name.
                let display_name = wowsunpack::data::ResourceLoader::localized_name_from_param(metadata, param)
                    .unwrap_or_else(|| param.name().to_owned());

                Some(PlaceableShip {
                    species,
                    param_index: param.index().to_owned(),
                    key: FilterKey::new(&display_name),
                    display_name,
                    tier: vehicle.level(),
                })
            })
            .collect();
        ships.sort_by(|a, b| a.tier.cmp(&b.tier).then_with(|| a.display_name.cmp(&b.display_name)));

        Self { ships }
    }

    /// The ships of the newest build `game_data` has loaded, or `None` before one has.
    pub fn from_game_data(game_data: &GameDataCache) -> Option<Self> {
        let loaded = game_data.newest_loaded()?;

        Some(Self::build(loaded.provider()))
    }

    /// The ships whose names match `query`, best first and at most [`SHIP_MATCHES`] of them.
    /// Empty for a query with nothing in it, since every ship at once is not a choice.
    pub fn matching(&self, query: &FilterQuery) -> Vec<&PlaceableShip> {
        if !query.is_active() {
            return Vec::new();
        }

        query.rank(&self.ships, |ship| &ship.key).into_iter().take(SHIP_MATCHES).collect()
    }
}

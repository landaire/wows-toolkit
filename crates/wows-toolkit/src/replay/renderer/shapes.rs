use std::collections::HashMap;
use std::collections::HashSet;

use egui::Color32;
use egui::Vec2;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_replays::types::EntityId;

use super::Annotation;
use super::MapTransform;
use super::PaintTool;
use super::RendererTextures;

/// The renderer window's per-ship overrides, applied on top of `RenderOptions`.
pub(super) struct WindowCommandFilter {
    pub trail_hidden_ships: HashSet<String>,
    pub ship_range_overrides: HashMap<EntityId, ShipConfigFilter>,
}

impl wt_collab_egui::player::CommandFilter for WindowCommandFilter {
    fn keep(&self, cmd: &DrawCommand, alive_ships: &HashSet<EntityId>) -> bool {
        match cmd {
            DrawCommand::PositionTrail { player_name: Some(name), .. } => !self.trail_hidden_ships.contains(name),
            DrawCommand::ShipConfigCircle { entity_id, kind, .. } => {
                alive_ships.contains(entity_id)
                    && self.ship_range_overrides.get(entity_id).is_some_and(|f| f.is_enabled(kind))
            }
            _ => true,
        }
    }
}

// Re-export shared annotation helpers so `use shapes::*` in mod.rs still works.
pub(super) use crate::replay::minimap_view::shapes::GridStyle;
pub(super) use crate::replay::minimap_view::shapes::MapPing;
pub(super) use crate::replay::minimap_view::shapes::PING_DURATION;
pub(super) use crate::replay::minimap_view::shapes::ZoomPanConfig;
pub(super) use crate::replay::minimap_view::shapes::annotation_cursor_icon;
pub(super) use crate::replay::minimap_view::shapes::annotation_screen_bounds;
pub(super) use crate::replay::minimap_view::shapes::compute_map_clip_rect;
pub(super) use crate::replay::minimap_view::shapes::draw_annotation_edit_popup;
pub(super) use crate::replay::minimap_view::shapes::draw_pings;
pub(super) use crate::replay::minimap_view::shapes::draw_remote_cursors;
pub(super) use crate::replay::minimap_view::shapes::draw_shortcut_overlay;
pub(super) use crate::replay::minimap_view::shapes::game_font;
pub(super) use crate::replay::minimap_view::shapes::handle_annotation_select_move;
pub(super) use crate::replay::minimap_view::shapes::handle_tool_interaction;
pub(super) use crate::replay::minimap_view::shapes::handle_tool_shortcuts;
pub(super) use crate::replay::minimap_view::shapes::handle_viewport_zoom_pan;
pub(super) use crate::replay::minimap_view::shapes::register_game_fonts;
pub(super) use crate::replay::minimap_view::shapes::render_selection_highlight;
pub(super) use crate::replay::minimap_view::shapes::tool_label;

// Re-export shared draw-command helpers.
pub(super) use wt_collab_egui::draw_commands::color_from_rgb;

/// Render a single annotation onto the map painter.
/// Thin wrapper around the shared `minimap_view::shapes::render_annotation` that
/// adapts the `RendererTextures` parameter.
pub(super) fn render_annotation(
    ann: &Annotation,
    transform: &MapTransform,
    textures: &RendererTextures,
    painter: &egui::Painter,
    map_space_size: Option<f32>,
) {
    crate::replay::minimap_view::shapes::render_annotation(
        ann,
        transform,
        Some(&textures.ship_icons),
        painter,
        map_space_size,
    );
}

/// Render a preview of the active tool at the cursor position.
/// Thin wrapper around the shared `minimap_view::shapes::render_tool_preview` that
/// adapts the `RendererTextures` parameter.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tool_preview(
    tool: &PaintTool,
    minimap_pos: Vec2,
    color: Color32,
    stroke_width: f32,
    transform: &MapTransform,
    textures: &RendererTextures,
    painter: &egui::Painter,
    map_space_size: Option<f32>,
) {
    crate::replay::minimap_view::shapes::render_tool_preview(
        tool,
        minimap_pos,
        color,
        stroke_width,
        transform,
        Some(&textures.ship_icons),
        painter,
        map_space_size,
    );
}

/// Build the shared `DrawCommandTextures` from a desktop `RendererTextures`.
pub(super) fn make_shared_textures<'a>(
    textures: &'a RendererTextures,
) -> wt_collab_egui::draw_commands::DrawCommandTextures<'a> {
    wt_collab_egui::draw_commands::DrawCommandTextures {
        ship_icons: &textures.ship_icons,
        ship_icon_outlines: Some(&textures.ship_icon_outlines),
        plane_icons: &textures.plane_icons,
        building_icons: Some(&textures.building_icons),
        consumable_icons: Some(&textures.consumable_icons),
        ribbon_icons: Some(&textures.ribbon_icons),
        subribbon_icons: Some(&textures.subribbon_icons),
        death_cause_icons: Some(&textures.death_cause_icons),
        powerup_icons: Some(&textures.powerup_icons),
        silhouette_texture: textures.silhouette_texture.as_ref(),
    }
}

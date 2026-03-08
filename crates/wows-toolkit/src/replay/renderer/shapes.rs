use egui::Color32;
use egui::Rect;
use egui::Shape;
use egui::Vec2;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::RenderOptions;
use wt_translations::TextResolver;

use super::Annotation;
use super::MapTransform;
use super::PaintTool;
use super::RendererTextures;

// Re-export shared annotation helpers so `use shapes::*` in mod.rs still works.
pub(super) use crate::replay::minimap_view::shapes::GridStyle;
pub(super) use crate::replay::minimap_view::shapes::MapPing;
pub(super) use crate::replay::minimap_view::shapes::PING_DURATION;
pub(super) use crate::replay::minimap_view::shapes::ZoomPanConfig;
pub(super) use crate::replay::minimap_view::shapes::annotation_cursor_icon;
pub(super) use crate::replay::minimap_view::shapes::annotation_screen_bounds;
pub(super) use crate::replay::minimap_view::shapes::compute_canvas_layout;
pub(super) use crate::replay::minimap_view::shapes::compute_map_clip_rect;
pub(super) use crate::replay::minimap_view::shapes::draw_annotation_edit_popup;
pub(super) use crate::replay::minimap_view::shapes::draw_grid;
pub(super) use crate::replay::minimap_view::shapes::draw_map_background;
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

/// Check whether a DrawCommand should be drawn given the current RenderOptions.
/// This runs on the UI thread so option changes are instant (no cross-thread round-trip).
pub(super) fn should_draw_command(cmd: &DrawCommand, opts: &RenderOptions, show_dead_ships: bool) -> bool {
    match cmd {
        DrawCommand::ShotTracer { .. } => opts.show_tracers,
        DrawCommand::Torpedo { .. } => opts.show_torpedoes,
        DrawCommand::Smoke { .. } => opts.show_smoke,
        DrawCommand::Ship { .. } => true, // ships always drawn; name visibility handled below
        DrawCommand::HealthBar { .. } => opts.show_hp_bars,
        DrawCommand::DeadShip { .. } => show_dead_ships,
        DrawCommand::Plane { .. } => opts.show_planes,
        DrawCommand::ScoreBar { .. } => opts.show_score,
        DrawCommand::Timer { .. } => opts.show_timer,
        DrawCommand::PreBattleCountdown { .. } => opts.show_timer,
        DrawCommand::KillFeed { .. } => opts.show_kill_feed && !opts.show_stats_panel,
        DrawCommand::CapturePoint { .. } => opts.show_capture_points,
        DrawCommand::Building { .. } => opts.show_buildings,
        DrawCommand::TurretDirection { .. } => opts.show_turret_direction,
        DrawCommand::ConsumableRadius { .. } => opts.show_consumables,
        DrawCommand::PatrolRadius { .. } => opts.show_planes,
        DrawCommand::ConsumableIcons { .. } => opts.show_consumables,
        DrawCommand::PositionTrail { .. } => opts.show_trails || opts.show_speed_trails,
        DrawCommand::ShipConfigCircle { .. } => opts.show_ship_config,
        DrawCommand::BuffZone { .. } => opts.show_capture_points,
        DrawCommand::TeamBuffs { .. } => opts.show_buffs,
        DrawCommand::BattleResultOverlay { .. } => opts.show_battle_result,
        DrawCommand::ChatOverlay { .. } => opts.show_chat && !opts.show_stats_panel,
        DrawCommand::TeamAdvantage { .. } => opts.show_advantage,
        DrawCommand::WeatherZone { .. } => opts.show_weather,
        DrawCommand::StatsPanel { .. }
        | DrawCommand::StatsSilhouette { .. }
        | DrawCommand::StatsDamage { .. }
        | DrawCommand::StatsRibbons { .. }
        | DrawCommand::StatsActivityFeed { .. } => opts.show_stats_panel,
    }
}

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
        death_cause_icons: Some(&textures.death_cause_icons),
        powerup_icons: Some(&textures.powerup_icons),
        silhouette_texture: textures.silhouette_texture.as_ref(),
    }
}

/// Build the shared label options from desktop `RenderOptions`.
pub(super) fn make_label_opts(opts: &RenderOptions) -> wt_collab_egui::draw_commands::DrawCommandLabelOptions {
    wt_collab_egui::draw_commands::DrawCommandLabelOptions {
        show_player_names: opts.show_player_names,
        show_ship_names: opts.show_ship_names,
        show_dead_ship_names: opts.show_dead_ship_names,
        show_armament_color: opts.show_armament,
    }
}

/// Convert a single DrawCommand into epaint shapes.
/// Uses `MapTransform` for all coordinate mapping. `opts` filters name labels.
pub(super) fn draw_command_to_shapes(
    cmd: &DrawCommand,
    transform: &MapTransform,
    textures: &RendererTextures,
    ctx: &egui::Context,
    opts: &RenderOptions,
    placed_labels: &mut Vec<Rect>,
    text_resolver: &dyn TextResolver,
) -> Vec<Shape> {
    let shared_tex = make_shared_textures(textures);
    let label_opts = make_label_opts(opts);
    wt_collab_egui::draw_commands::draw_command_to_shapes(
        cmd,
        transform,
        &shared_tex,
        ctx,
        &label_opts,
        Some(placed_labels),
        text_resolver,
    )
}

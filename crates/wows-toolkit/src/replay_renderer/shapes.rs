use egui::Color32;
use egui::CornerRadius;
use egui::Pos2;
use egui::Rect;
use egui::Shape;
use egui::Stroke;
use egui::Vec2;

use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::RenderOptions;

use super::Annotation;
use super::ICON_SIZE;
use super::MapTransform;
use super::PaintTool;
use super::RendererTextures;

// Re-export shared annotation helpers so `use shapes::*` in mod.rs still works.
pub(super) use crate::minimap_view::shapes::GridStyle;
pub(super) use crate::minimap_view::shapes::MapPing;
pub(super) use crate::minimap_view::shapes::PING_DURATION;
pub(super) use crate::minimap_view::shapes::annotation_cursor_icon;
pub(super) use crate::minimap_view::shapes::annotation_screen_bounds;
pub(super) use crate::minimap_view::shapes::draw_annotation_edit_popup;
pub(super) use crate::minimap_view::shapes::draw_annotation_menu_common;
pub(super) use crate::minimap_view::shapes::draw_grid;
pub(super) use crate::minimap_view::shapes::draw_pings;
pub(super) use crate::minimap_view::shapes::draw_remote_cursors;
pub(super) use crate::minimap_view::shapes::game_font;
pub(super) use crate::minimap_view::shapes::handle_annotation_select_move;
pub(super) use crate::minimap_view::shapes::handle_scroll_yaw;
pub(super) use crate::minimap_view::shapes::handle_tool_interaction;
pub(super) use crate::minimap_view::shapes::register_game_fonts;
pub(super) use crate::minimap_view::shapes::render_selection_highlight;
pub(super) use crate::minimap_view::shapes::tool_label;

pub(super) fn color_from_rgb(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub(super) fn color_from_rgba(rgb: [u8; 3], alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], (alpha * 255.0) as u8)
}

pub(super) use crate::minimap_view::shapes::make_rotated_icon_mesh;

/// Build an unrotated textured quad mesh for a plane icon.
pub(super) fn make_icon_mesh(texture_id: egui::TextureId, center: Pos2, w: f32, h: f32) -> Shape {
    let half_w = w / 2.0;
    let half_h = h / 2.0;
    let rect = Rect::from_min_max(
        Pos2::new(center.x - half_w, center.y - half_h),
        Pos2::new(center.x + half_w, center.y + half_h),
    );
    let mut mesh = egui::Mesh::with_texture(texture_id);
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
    Shape::Mesh(mesh.into())
}

/// Draw player name and/or ship name labels centered above an icon.
/// `scale` controls font and offset sizing (1.0 at default 768px canvas).
/// `armament_color` is applied to ship_name first if shown, otherwise player_name.
pub(super) fn draw_ship_labels(
    ctx: &egui::Context,
    center: Pos2,
    scale: f32,
    player_name: Option<&str>,
    ship_name: Option<&str>,
    armament_color: Option<Color32>,
    shapes: &mut Vec<Shape>,
) {
    let label_font = game_font(10.0 * scale);
    let line_height = 12.0 * scale;
    let label_color = Color32::WHITE;
    let shadow_color = Color32::from_rgba_unmultiplied(0, 0, 0, 180);
    let shadow_offset = (1.0 * scale).min(2.0);

    let line_count = player_name.is_some() as i32 + ship_name.is_some() as i32;
    if line_count == 0 {
        return;
    }

    // Armament color goes on ship_name if shown, else on player_name
    let (pn_color, sn_color) = if ship_name.is_some() {
        (label_color, armament_color.unwrap_or(label_color))
    } else {
        (armament_color.unwrap_or(label_color), label_color)
    };

    // Position lines above the icon
    let base_y = center.y - 14.0 * scale - line_count as f32 * line_height;
    let mut cur_y = base_y;

    if let Some(name) = player_name {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), pn_color));
        let text_w = galley.size().x;
        let tx = center.x - text_w / 2.0;
        let shadow_galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx + shadow_offset, cur_y + shadow_offset), shadow_galley, shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx, cur_y), galley, pn_color));
        cur_y += line_height;
    }

    if let Some(name) = ship_name {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), sn_color));
        let text_w = galley.size().x;
        let tx = center.x - text_w / 2.0;
        let shadow_galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.to_string(), label_font.clone(), shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx + shadow_offset, cur_y + shadow_offset), shadow_galley, shadow_color));
        shapes.push(Shape::galley(Pos2::new(tx, cur_y), galley, sn_color));
    }
}

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
        DrawCommand::KillFeed { .. } => opts.show_kill_feed,
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
        DrawCommand::ChatOverlay { .. } => opts.show_chat,
        DrawCommand::TeamAdvantage { .. } => opts.show_advantage,
        DrawCommand::WeatherZone { .. } => opts.show_weather,
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
) {
    crate::minimap_view::shapes::render_annotation(ann, transform, Some(&textures.ship_icons), painter);
}

/// Render a preview of the active tool at the cursor position.
/// Thin wrapper around the shared `minimap_view::shapes::render_tool_preview` that
/// adapts the `RendererTextures` parameter.
pub(super) fn render_tool_preview(
    tool: &PaintTool,
    minimap_pos: Vec2,
    color: Color32,
    stroke_width: f32,
    transform: &MapTransform,
    textures: &RendererTextures,
    painter: &egui::Painter,
) {
    crate::minimap_view::shapes::render_tool_preview(
        tool,
        minimap_pos,
        color,
        stroke_width,
        transform,
        Some(&textures.ship_icons),
        painter,
    );
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
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let ws = transform.window_scale;

    match cmd {
        DrawCommand::ShotTracer { from, to, color } => {
            let p1 = transform.minimap_to_screen(from);
            let p2 = transform.minimap_to_screen(to);
            shapes.push(Shape::LineSegment {
                points: [p1, p2],
                stroke: Stroke::new(transform.scale_stroke(1.0), color_from_rgb(*color)),
            });
        }

        DrawCommand::Torpedo { pos, color } => {
            let center = transform.minimap_to_screen(pos);
            shapes.push(Shape::circle_filled(center, transform.scale_distance(2.0), color_from_rgb(*color)));
        }

        DrawCommand::Smoke { pos, radius, color, alpha } => {
            let center = transform.minimap_to_screen(pos);
            shapes.push(Shape::circle_filled(
                center,
                transform.scale_distance(*radius as f32),
                color_from_rgba(*color, *alpha),
            ));
        }

        DrawCommand::Ship {
            pos,
            yaw,
            species,
            color,
            visibility,
            opacity,
            is_self,
            player_name,
            ship_name,
            is_detected_teammate,
            name_color,
            ..
        } => {
            let center = transform.minimap_to_screen(pos);
            let icon_size = transform.scale_distance(ICON_SIZE);

            if let Some(sp) = species {
                let variant_key = match (*visibility, *is_self) {
                    (wows_minimap_renderer::ShipVisibility::Visible, true) => format!("{}_self", sp),
                    (wows_minimap_renderer::ShipVisibility::Visible, false) => sp.clone(),
                    (wows_minimap_renderer::ShipVisibility::MinimapOnly, _) => {
                        format!("{}_invisible", sp)
                    }
                    (wows_minimap_renderer::ShipVisibility::Undetected, _) => {
                        format!("{}_invisible", sp)
                    }
                };

                // Gold icon-shaped outline for detected teammates (drawn before icon)
                if *is_detected_teammate {
                    let outline_tex =
                        textures.ship_icon_outlines.get(&variant_key).or_else(|| textures.ship_icon_outlines.get(sp));
                    if let Some(otex) = outline_tex {
                        shapes.push(make_rotated_icon_mesh(otex.id(), center, icon_size, *yaw, Color32::WHITE));
                    }
                }

                let texture = textures.ship_icons.get(&variant_key).or_else(|| textures.ship_icons.get(sp));

                if let Some(tex) = texture {
                    let tint = if let Some(c) = color {
                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (*opacity * 255.0) as u8)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, (*opacity * 255.0) as u8)
                    };
                    shapes.push(make_rotated_icon_mesh(tex.id(), center, icon_size, *yaw, tint));
                } else {
                    let c = color.map(|c| color_from_rgba(c, *opacity)).unwrap_or(Color32::from_rgba_unmultiplied(
                        128,
                        128,
                        128,
                        (*opacity * 255.0) as u8,
                    ));
                    shapes.push(Shape::circle_filled(center, transform.scale_distance(5.0), c));
                }
            }
            let pname = if opts.show_player_names { player_name.as_deref() } else { None };
            let sname = if opts.show_ship_names { ship_name.as_deref() } else { None };
            let pn_color =
                if opts.show_armament { name_color.map(|c| Color32::from_rgb(c[0], c[1], c[2])) } else { None };
            draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, pn_color, &mut shapes);
        }

        DrawCommand::HealthBar { pos, fraction, fill_color, background_color, background_alpha, .. } => {
            let bar_w = transform.scale_distance(20.0);
            let bar_h = transform.scale_distance(3.0);
            let center = transform.minimap_to_screen(pos);
            let bar_x = center.x - bar_w / 2.0;
            let bar_y = center.y + transform.scale_distance(10.0);

            let bg_rect = Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(bar_w, bar_h));
            shapes.push(Shape::rect_filled(
                bg_rect,
                CornerRadius::ZERO,
                color_from_rgba(*background_color, *background_alpha),
            ));

            let fill_w = fraction.clamp(0.0, 1.0) * bar_w;
            if fill_w > 0.0 {
                let fill_rect = Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(fill_w, bar_h));
                shapes.push(Shape::rect_filled(fill_rect, CornerRadius::ZERO, color_from_rgb(*fill_color)));
            }
        }

        DrawCommand::DeadShip { pos, yaw, species, color, is_self, player_name, ship_name, .. } => {
            let center = transform.minimap_to_screen(pos);
            let icon_size = transform.scale_distance(ICON_SIZE);
            if let Some(sp) = species {
                let variant_key = if *is_self { format!("{}_dead_self", sp) } else { format!("{}_dead", sp) };

                let texture = textures.ship_icons.get(&variant_key).or_else(|| textures.ship_icons.get(sp));

                if let Some(tex) = texture {
                    let tint = if let Some(c) = color { Color32::from_rgb(c[0], c[1], c[2]) } else { Color32::WHITE };
                    shapes.push(make_rotated_icon_mesh(tex.id(), center, icon_size, *yaw, tint));
                } else {
                    let s = transform.scale_distance(6.0);
                    let stroke = Stroke::new(transform.scale_stroke(2.0), Color32::RED);
                    shapes.push(Shape::LineSegment {
                        points: [Pos2::new(center.x - s, center.y - s), Pos2::new(center.x + s, center.y + s)],
                        stroke,
                    });
                    shapes.push(Shape::LineSegment {
                        points: [Pos2::new(center.x + s, center.y - s), Pos2::new(center.x - s, center.y + s)],
                        stroke,
                    });
                }
            }
            if opts.show_dead_ship_names {
                let pname = if opts.show_player_names { player_name.as_deref() } else { None };
                let sname = if opts.show_ship_names { ship_name.as_deref() } else { None };
                draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, None, &mut shapes);
            }
        }

        DrawCommand::Plane { pos, icon_key, player_name, ship_name, .. } => {
            let center = transform.minimap_to_screen(pos);
            if let Some(tex) = textures.plane_icons.get(icon_key) {
                let size = tex.size();
                let w = transform.scale_distance(size[0] as f32);
                let h = transform.scale_distance(size[1] as f32);
                shapes.push(make_icon_mesh(tex.id(), center, w, h));
            } else {
                shapes.push(Shape::circle_filled(center, transform.scale_distance(3.0), Color32::YELLOW));
            }
            let pname = if opts.show_player_names { player_name.as_deref() } else { None };
            let sname = if opts.show_ship_names { ship_name.as_deref() } else { None };
            draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, None, &mut shapes);
        }

        DrawCommand::ScoreBar {
            team0,
            team1,
            team0_color,
            team1_color,
            max_score,
            team0_timer,
            team1_timer,
            advantage_label,
            advantage_team,
        } => {
            let canvas_w = transform.screen_canvas_width();
            let bar_height = HUD_HEIGHT as f32 * ws;
            let max_score = *max_score as f32;
            let half = canvas_w / 2.0;
            let center_gap = 2.0 * ws;

            let bar_origin = transform.hud_pos(0.0, 0.0);

            // Dark background
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(bar_origin, Vec2::new(canvas_w, bar_height)),
                CornerRadius::ZERO,
                Color32::from_rgba_unmultiplied(30, 30, 30, 204),
            ));

            // Team 0 progress: grows from left edge toward center
            let t0_frac = (*team0 as f32 / max_score).clamp(0.0, 1.0);
            let t0_width = t0_frac * (half - center_gap);
            if t0_width > 0.0 {
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(bar_origin, Vec2::new(t0_width, bar_height)),
                    CornerRadius::ZERO,
                    color_from_rgb(*team0_color),
                ));
            }

            // Team 1 progress: grows from right edge toward center
            let t1_frac = (*team1 as f32 / max_score).clamp(0.0, 1.0);
            let t1_width = t1_frac * (half - center_gap);
            if t1_width > 0.0 {
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(
                        Pos2::new(bar_origin.x + canvas_w - t1_width, bar_origin.y),
                        Vec2::new(t1_width, bar_height),
                    ),
                    CornerRadius::ZERO,
                    color_from_rgb(*team1_color),
                ));
            }

            let score_font = game_font(14.0 * ws);
            let timer_font = game_font(12.0 * ws);
            let adv_font = game_font(11.0 * ws);
            let t0_text = format!("{}", team0);
            let t1_text = format!("{}", team1);
            let timer_color = Color32::from_rgb(200, 200, 200);
            let pill_color = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
            let pill_pad_x = 4.0 * ws;
            let pill_pad_y = 1.0 * ws;
            let pill_rounding = CornerRadius::same((3.0 * ws) as u8);

            // ── Measure all team 0 elements ──
            let t0_score_g = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text.clone(), score_font.clone(), Color32::WHITE));
            let t0_score_w = t0_score_g.size().x;
            let t0_score_h = t0_score_g.size().y;
            drop(t0_score_g);

            let t0_timer_w = team0_timer.as_ref().map(|t| {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(t.clone(), timer_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                w
            });

            let t0_adv_w = if *advantage_team == 0 && !advantage_label.is_empty() {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                Some(w)
            } else {
                None
            };

            // Total width for team 0 pill
            let mut t0_total_w = t0_score_w;
            if let Some(tw) = t0_timer_w {
                t0_total_w += 4.0 * ws + tw;
            }
            if let Some(aw) = t0_adv_w {
                t0_total_w += 6.0 * ws + aw;
            }

            // Pill vertically centered within bar
            let pill_h = t0_score_h + pill_pad_y * 2.0;
            let pill_y = bar_origin.y + (bar_height - pill_h) / 2.0;

            // Draw team 0 pill + text
            let t0_pill_x = bar_origin.x + 8.0 * ws - pill_pad_x;
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(Pos2::new(t0_pill_x, pill_y), Vec2::new(t0_total_w + pill_pad_x * 2.0, pill_h)),
                pill_rounding,
                pill_color,
            ));

            // Position score text centered in pill, then align timer/advantage
            // to the same bottom edge so baselines visually match.
            let pill_cy = pill_y + pill_h / 2.0;

            let mut t0_cursor = bar_origin.x + 8.0 * ws;
            let t0_score_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t0_text, score_font.clone(), Color32::WHITE));
            let score_top = pill_cy - t0_score_galley.size().y / 2.0;
            shapes.push(Shape::galley(Pos2::new(t0_cursor, score_top), t0_score_galley, Color32::WHITE));
            t0_cursor += t0_score_w;

            if let Some(timer) = team0_timer {
                t0_cursor += 4.0 * ws;
                let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font.clone(), timer_color));
                let tw = tg.size().x;
                let ty = score_top + (t0_score_h - tg.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t0_cursor, ty), tg, timer_color));
                t0_cursor += tw;
            }

            let _t0_end_x = t0_cursor;

            if t0_adv_w.is_some() {
                t0_cursor += 6.0 * ws;
                let ag = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let ay = score_top + (t0_score_h - ag.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t0_cursor, ay), ag, Color32::WHITE));
            }

            // ── Measure all team 1 elements ──
            let t1_score_g = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text.clone(), score_font.clone(), Color32::WHITE));
            let t1_score_w = t1_score_g.size().x;
            let _t1_score_h = t1_score_g.size().y;
            drop(t1_score_g);

            let t1_timer_w = team1_timer.as_ref().map(|t| {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(t.clone(), timer_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                w
            });

            let t1_adv_w = if *advantage_team == 1 && !advantage_label.is_empty() {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                Some(w)
            } else {
                None
            };

            // Total width for team 1 pill
            let mut t1_total_w = t1_score_w;
            if let Some(tw) = t1_timer_w {
                t1_total_w += 4.0 * ws + tw;
            }
            if let Some(aw) = t1_adv_w {
                t1_total_w += 6.0 * ws + aw;
            }

            // Draw team 1 pill + text (right-aligned), reuse pill_h/pill_y from team 0
            let t1_pill_x = bar_origin.x + canvas_w - 8.0 * ws - t1_total_w - pill_pad_x;
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(Pos2::new(t1_pill_x, pill_y), Vec2::new(t1_total_w + pill_pad_x * 2.0, pill_h)),
                pill_rounding,
                pill_color,
            ));

            // Lay out team 1 elements right-to-left
            let mut t1_cursor = bar_origin.x + canvas_w - 8.0 * ws;

            // Score (rightmost)
            t1_cursor -= t1_score_w;
            let t1_score_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text, score_font, Color32::WHITE));
            let t1_score_top = pill_cy - t1_score_galley.size().y / 2.0;
            shapes.push(Shape::galley(Pos2::new(t1_cursor, t1_score_top), t1_score_galley, Color32::WHITE));
            let _t1_score_x = t1_cursor;

            // Timer (left of score)
            if let Some(timer) = team1_timer {
                let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font, timer_color));
                let tw = tg.size().x;
                t1_cursor -= 4.0 * ws + tw;
                let ty = t1_score_top + (_t1_score_h - tg.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t1_cursor, ty), tg, timer_color));
            }

            let _t1_start_x = t1_cursor;

            // Advantage (leftmost, if team 1)
            if let Some(aw) = t1_adv_w {
                t1_cursor -= 6.0 * ws + aw;
                let ag = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font, Color32::WHITE));
                let ay = t1_score_top + (_t1_score_h - ag.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t1_cursor, ay), ag, Color32::WHITE));
            }
        }

        DrawCommand::Timer { time_remaining, elapsed } => {
            // Don't show until battle has started (pre-battle uses PreBattleCountdown)
            if elapsed.seconds() <= 0.0 {
                return shapes;
            }
            let canvas_w = transform.screen_canvas_width();
            let main_font = game_font(16.0 * ws);
            let pill_color = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
            let pill_pad_x = 4.0 * ws;
            let pill_pad_y = 1.0 * ws;
            let pill_rounding = CornerRadius::same((3.0 * ws) as u8);

            // Match video renderer: main timer at Y=2, elapsed at Y=18 (in HUD-logical coords)
            let hud_h = HUD_HEIGHT as f32 * ws;
            if let Some(remaining) = time_remaining {
                let r = (*remaining).max(0) as u32;
                let remaining_text = format!("{:02}:{:02}", r / 60, r % 60);
                let small_font = game_font(11.0 * ws);
                let e = elapsed.seconds().max(0.0) as u32;
                let elapsed_text = format!("+{:02}:{:02}", e / 60, e % 60);
                let gray = Color32::from_rgb(180, 180, 180);

                let rg = ctx.fonts_mut(|f| f.layout_no_wrap(remaining_text, main_font, Color32::WHITE));
                let r_w = rg.size().x;
                let eg = ctx.fonts_mut(|f| f.layout_no_wrap(elapsed_text, small_font, gray));
                let e_w = eg.size().x;

                let hud_origin = transform.hud_pos(0.0, 0.0);
                let main_pos = transform.hud_pos(0.0, 2.0);
                let elapsed_pos = transform.hud_pos(0.0, 18.0);

                // Pill spans from main text to bottom of HUD, clamped
                let pill_w = r_w.max(e_w);
                let pill_top = main_pos.y - pill_pad_y;
                let pill_bottom = hud_origin.y + hud_h;
                let pill_h = (pill_bottom - pill_top).max(0.0);
                let pill_x = main_pos.x + canvas_w / 2.0 - pill_w / 2.0 - pill_pad_x;
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(Pos2::new(pill_x, pill_top), Vec2::new(pill_w + pill_pad_x * 2.0, pill_h)),
                    pill_rounding,
                    pill_color,
                ));

                let r_x = main_pos.x + canvas_w / 2.0 - r_w / 2.0;
                shapes.push(Shape::galley(Pos2::new(r_x, main_pos.y), rg, Color32::WHITE));

                let e_x = main_pos.x + canvas_w / 2.0 - e_w / 2.0;
                shapes.push(Shape::galley(Pos2::new(e_x, elapsed_pos.y), eg, gray));
            } else {
                // Fallback: just show elapsed time centered
                let e = elapsed.seconds().max(0.0) as u32;
                let text = format!("{:02}:{:02}", e / 60, e % 60);
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text, main_font, Color32::WHITE));
                let text_w = galley.size().x;
                let hud_origin = transform.hud_pos(0.0, 0.0);
                let pos = transform.hud_pos(0.0, 2.0);
                let x = pos.x + canvas_w / 2.0 - text_w / 2.0;
                let pill_top = pos.y - pill_pad_y;
                let pill_bottom = hud_origin.y + hud_h;
                let pill_h = (pill_bottom - pill_top).max(0.0);
                shapes.push(Shape::rect_filled(
                    Rect::from_min_size(
                        Pos2::new(x - pill_pad_x, pill_top),
                        Vec2::new(text_w + pill_pad_x * 2.0, pill_h),
                    ),
                    pill_rounding,
                    pill_color,
                ));
                shapes.push(Shape::galley(Pos2::new(x, pos.y), galley, Color32::WHITE));
            }
        }

        DrawCommand::PreBattleCountdown { seconds } => {
            // Reuse the BattleResultOverlay rendering with gold color and subtitle above
            let overlay = DrawCommand::BattleResultOverlay {
                text: format!("{}", seconds),
                subtitle: Some("BATTLE STARTS IN".to_string()),
                color: [255, 200, 50],
                subtitle_above: true,
            };
            shapes.extend(draw_command_to_shapes(&overlay, transform, textures, ctx, opts, placed_labels));
        }

        DrawCommand::TeamAdvantage { .. } => {
            // Rendering handled by ScoreBar; this command is kept for tooltip interaction only
        }

        DrawCommand::WeatherZone { pos, radius } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);
            shapes.push(Shape::circle_filled(center, r, Color32::from_rgba_unmultiplied(100, 100, 120, 40)));
            shapes.push(Shape::circle_stroke(
                center,
                r,
                Stroke::new(transform.scale_stroke(1.0), Color32::from_rgba_unmultiplied(120, 120, 150, 80)),
            ));
        }

        DrawCommand::KillFeed { entries } => {
            use wows_replays::analyzer::decoder::DeathCause;

            let canvas_w = transform.screen_canvas_width();
            let name_font = game_font(12.0 * ws);
            let line_h = 20.0 * ws;
            let icon_size = ICON_SIZE * ws;
            let cause_icon_size = icon_size;
            let gap = 2.0 * ws;
            let right_margin = 4.0 * ws;
            let start = transform.hud_pos(0.0, HUD_HEIGHT as f32);

            for (i, entry) in entries.iter().take(5).enumerate() {
                let y = start.y + i as f32 * line_h;

                let killer_color = color_from_rgb(entry.killer_color);
                let victim_color = color_from_rgb(entry.victim_color);

                let cause_key = match entry.cause.known() {
                    Some(DeathCause::Artillery | DeathCause::ApShell | DeathCause::HeShell | DeathCause::CsShell) => {
                        "main_caliber"
                    }
                    Some(DeathCause::Secondaries) => "atba",
                    Some(DeathCause::Torpedo | DeathCause::AerialTorpedo) => "torpedo",
                    Some(DeathCause::Fire) => "burning",
                    Some(DeathCause::Flooding) => "flood",
                    Some(DeathCause::DiveBomber) => "bomb",
                    Some(DeathCause::SkipBombs) => "skip",
                    Some(DeathCause::AerialRocket) => "rocket",
                    Some(DeathCause::Detonation) => "detonate",
                    Some(DeathCause::Ramming) => "ram",
                    Some(DeathCause::DepthCharge | DeathCause::AerialDepthCharge) => "depthbomb",
                    Some(DeathCause::Missile) => "missile",
                    _ => "main_caliber",
                };

                // Measure text segments
                let killer_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.killer_name.clone(), name_font.clone(), killer_color));
                let victim_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.victim_name.clone(), name_font.clone(), victim_color));
                let killer_name_w = killer_galley.size().x;
                let victim_name_w = victim_galley.size().x;

                let ship_font = name_font.clone();
                let killer_ship = entry.killer_ship_name.as_deref().unwrap_or("");
                let victim_ship = entry.victim_ship_name.as_deref().unwrap_or("");
                let killer_ship_galley = if !killer_ship.is_empty() {
                    Some(ctx.fonts_mut(|f| f.layout_no_wrap(killer_ship.to_string(), ship_font.clone(), killer_color)))
                } else {
                    None
                };
                let victim_ship_galley = if !victim_ship.is_empty() {
                    Some(ctx.fonts_mut(|f| f.layout_no_wrap(victim_ship.to_string(), ship_font.clone(), victim_color)))
                } else {
                    None
                };
                let killer_ship_w = killer_ship_galley.as_ref().map_or(0.0, |g| g.size().x);
                let victim_ship_w = victim_ship_galley.as_ref().map_or(0.0, |g| g.size().x);

                let has_cause_icon = textures.death_cause_icons.contains_key(cause_key);
                let cause_w = if has_cause_icon { cause_icon_size } else { 0.0 };

                let has_killer_icon =
                    entry.killer_species.as_ref().is_some_and(|sp| textures.ship_icons.contains_key(sp.as_str()));
                let has_victim_icon =
                    entry.victim_species.as_ref().is_some_and(|sp| textures.ship_icons.contains_key(sp.as_str()));

                // Total width: killer_name [gap icon gap] killer_ship gap cause gap victim_name [gap icon gap] victim_ship
                let mut total_w = killer_name_w;
                if has_killer_icon {
                    total_w += gap + icon_size + gap;
                } else if killer_ship_w > 0.0 {
                    total_w += gap;
                }
                if killer_ship_w > 0.0 {
                    total_w += killer_ship_w;
                }
                total_w += gap * 2.0 + cause_w + gap * 2.0;
                total_w += victim_name_w;
                if has_victim_icon {
                    total_w += gap + icon_size + gap;
                } else if victim_ship_w > 0.0 {
                    total_w += gap;
                }
                if victim_ship_w > 0.0 {
                    total_w += victim_ship_w;
                }

                // Semi-transparent background
                let bg_x = start.x + canvas_w - total_w - right_margin * 2.0;
                let bg_rect =
                    Rect::from_min_size(Pos2::new(bg_x, y - 1.0 * ws), Vec2::new(total_w + right_margin * 2.0, line_h));
                shapes.push(Shape::rect_filled(bg_rect, CornerRadius::ZERO, Color32::from_black_alpha(128)));

                let mut x = start.x + canvas_w - total_w - right_margin;
                // Vertically center icons with the text
                let row_rect = killer_galley.rows.first().map(|r| r.rect()).unwrap_or(egui::Rect::ZERO);
                let icon_center_y = y + row_rect.center().y;

                // Killer name
                shapes.push(Shape::galley(Pos2::new(x, y), killer_galley, Color32::TRANSPARENT));
                x += killer_name_w;

                // Killer ship icon (facing left: -90° from north)
                if has_killer_icon {
                    x += gap;
                    let sp = entry.killer_species.as_ref().unwrap();
                    if let Some(tex) = textures.ship_icons.get(sp.as_str()) {
                        let tint =
                            Color32::from_rgb(entry.killer_color[0], entry.killer_color[1], entry.killer_color[2]);
                        shapes.push(make_rotated_icon_mesh(
                            tex.id(),
                            Pos2::new(x + icon_size / 2.0, icon_center_y),
                            icon_size,
                            std::f32::consts::PI,
                            tint,
                        ));
                    }
                    x += icon_size + gap;
                } else if killer_ship_w > 0.0 {
                    x += gap;
                }

                // Killer ship name
                if let Some(galley) = killer_ship_galley {
                    shapes.push(Shape::galley(Pos2::new(x, y), galley, Color32::TRANSPARENT));
                    x += killer_ship_w;
                }

                // Death cause icon
                x += gap * 2.0;
                if let Some(tex) = textures.death_cause_icons.get(cause_key) {
                    let half = cause_icon_size / 2.0;
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_max(
                        Pos2::new(x, icon_center_y - half),
                        Pos2::new(x + cause_icon_size, icon_center_y + half),
                    );
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));
                }
                x += cause_w + gap * 2.0;

                // Victim name
                shapes.push(Shape::galley(Pos2::new(x, y), victim_galley, Color32::TRANSPARENT));
                x += victim_name_w;

                // Victim ship icon (facing right: +90° from north)
                if has_victim_icon {
                    x += gap;
                    let sp = entry.victim_species.as_ref().unwrap();
                    if let Some(tex) = textures.ship_icons.get(sp.as_str()) {
                        let tint =
                            Color32::from_rgb(entry.victim_color[0], entry.victim_color[1], entry.victim_color[2]);
                        shapes.push(make_rotated_icon_mesh(
                            tex.id(),
                            Pos2::new(x + icon_size / 2.0, icon_center_y),
                            icon_size,
                            0.0,
                            tint,
                        ));
                    }
                    x += icon_size + gap;
                } else if victim_ship_w > 0.0 {
                    x += gap;
                }

                // Victim ship name
                if let Some(galley) = victim_ship_galley {
                    shapes.push(Shape::galley(Pos2::new(x, y), galley, Color32::TRANSPARENT));
                }
            }
        }

        DrawCommand::CapturePoint { pos, radius, color, alpha, label, progress, invader_color, .. } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);

            shapes.push(Shape::circle_filled(center, r, color_from_rgba(*color, *alpha)));

            if *progress > 0.001
                && let Some(inv_color) = invader_color
            {
                let fill_alpha = (*alpha + 0.10).min(1.0);
                let sweep = *progress * std::f32::consts::TAU;
                let segments = 64;
                let start_angle = -std::f32::consts::FRAC_PI_2;
                let pie_color = color_from_rgba(*inv_color, fill_alpha);

                let mut mesh = egui::Mesh::default();
                mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: egui::pos2(0.0, 0.0), color: pie_color });
                let step_count = ((segments as f32 * (*progress)).ceil() as usize).max(1);
                let angle_step = sweep / step_count as f32;
                for i in 0..=step_count {
                    let angle = start_angle + i as f32 * angle_step;
                    let px = center.x + r * angle.cos();
                    let py = center.y + r * angle.sin();
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: egui::pos2(px, py),
                        uv: egui::pos2(0.0, 0.0),
                        color: pie_color,
                    });
                    if i > 0 {
                        let vi = mesh.vertices.len() as u32;
                        mesh.indices.extend_from_slice(&[0, vi - 2, vi - 1]);
                    }
                }
                shapes.push(Shape::Mesh(mesh.into()));
            }

            let outline_color = if *progress > 0.001 {
                invader_color.map(color_from_rgb).unwrap_or_else(|| color_from_rgb(*color))
            } else {
                color_from_rgb(*color)
            };
            shapes.push(Shape::circle_stroke(center, r, Stroke::new(transform.scale_stroke(1.5), outline_color)));

            if !label.is_empty() {
                let font = game_font(11.0 * ws);
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.clone(), font, Color32::WHITE));
                let text_w = galley.size().x;
                let text_h = galley.size().y;
                shapes.push(Shape::galley(
                    Pos2::new(center.x - text_w / 2.0, center.y - text_h / 2.0),
                    galley,
                    Color32::WHITE,
                ));
            }
        }

        DrawCommand::Building { pos, color, is_alive } => {
            let center = transform.minimap_to_screen(pos);
            let r = if *is_alive { transform.scale_distance(2.0) } else { transform.scale_distance(1.5) };
            shapes.push(Shape::circle_filled(center, r, color_from_rgb(*color)));
        }

        DrawCommand::TurretDirection { pos, yaw, color, length, .. } => {
            let start = transform.minimap_to_screen(pos);
            // yaw is screen-space: 0 = east, PI/2 = north
            let dx = *length as f32 * yaw.cos();
            let dy = -*length as f32 * yaw.sin();
            let end = Pos2::new(start.x + transform.scale_distance(dx), start.y + transform.scale_distance(dy));
            let stroke_width = transform.scale_stroke(1.5);
            let c = color_from_rgb(*color);
            let line_color = Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), 180);
            shapes.push(Shape::line_segment([start, end], Stroke::new(stroke_width, line_color)));
        }

        DrawCommand::ConsumableRadius { pos, radius_px, color, alpha, .. } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius_px as f32);
            let fill_color = color_from_rgba(*color, *alpha);
            shapes.push(Shape::circle_filled(center, r, fill_color));
            let outline_color = color_from_rgba(*color, 0.5);
            let stroke_w = transform.scale_stroke(2.0);
            shapes.push(Shape::circle_stroke(center, r, Stroke::new(stroke_w, outline_color)));
        }

        DrawCommand::PatrolRadius { pos, radius_px, color, alpha, .. } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius_px as f32);
            let fill_color = color_from_rgba(*color, *alpha);
            shapes.push(Shape::circle_filled(center, r, fill_color));
        }

        DrawCommand::ConsumableIcons { pos, icon_keys, has_hp_bar, .. } => {
            let center = transform.minimap_to_screen(pos);
            // Position below HP bar (10 bar top + 3 bar height + 11 half-icon + 2 gap = 26)
            // or below the ship icon if no HP bar (10 + 11 half-icon + 2 gap = 23)
            let base_offset = if *has_hp_bar { 26.0 } else { 23.0 };
            let icon_y = center.y + transform.scale_distance(base_offset);
            let icon_size = transform.scale_distance(16.0);
            let gap = transform.scale_distance(1.0);
            let count = icon_keys.len() as f32;
            let total_width = count * icon_size + (count - 1.0) * gap;
            let start_x = center.x - total_width / 2.0 + icon_size / 2.0;
            for (i, icon_key) in icon_keys.iter().enumerate() {
                let icon_x = start_x + i as f32 * (icon_size + gap);
                if let Some(tex) = textures.consumable_icons.get(icon_key) {
                    let half = icon_size / 2.0;
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_max(
                        Pos2::new(icon_x - half, icon_y - half),
                        Pos2::new(icon_x + half, icon_y + half),
                    );
                    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
                    shapes.push(Shape::Mesh(mesh.into()));
                }
            }
        }

        DrawCommand::PositionTrail { points, .. } => {
            let dot_radius = transform.scale_distance(1.5);
            for (pos, color) in points {
                let center = transform.minimap_to_screen(pos);
                shapes.push(Shape::circle_filled(center, dot_radius, color_from_rgb(*color)));
            }
        }

        DrawCommand::ShipConfigCircle { pos, radius_px, color, alpha, dashed, label, .. } => {
            let center = transform.minimap_to_screen(pos);
            let screen_radius = transform.scale_distance(*radius_px);
            let circle_color = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], (alpha * 255.0) as u8);
            let stroke = Stroke::new(1.5, circle_color);

            if *dashed {
                // Dashed circle: draw as series of arcs
                let segments = 48;
                let gap_ratio = 0.4;
                for i in 0..segments {
                    let t0 = i as f32 / segments as f32 * std::f32::consts::TAU;
                    let t1 = (i as f32 + 1.0 - gap_ratio) / segments as f32 * std::f32::consts::TAU;
                    let steps = 4;
                    let points: Vec<Pos2> = (0..=steps)
                        .map(|s| {
                            let t = t0 + (t1 - t0) * s as f32 / steps as f32;
                            Pos2::new(center.x + screen_radius * t.cos(), center.y + screen_radius * t.sin())
                        })
                        .collect();
                    shapes.push(Shape::line(points, stroke));
                }
            } else {
                shapes.push(Shape::circle_stroke(center, screen_radius, stroke));
            }

            // Draw label around the circle, rotating to avoid overlapping previously placed labels
            if let Some(text) = label {
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), game_font(10.0), circle_color));
                let text_w = galley.size().x;
                let text_h = galley.size().y;
                let gap = 4.0;

                // Try 8 positions around the circle (top, top-right, right, bottom-right, bottom, bottom-left, left, top-left)
                // Starting from top (angle = -PI/2) going clockwise
                let candidate_angles: [f32; 8] = [
                    -std::f32::consts::FRAC_PI_2,       // top
                    -std::f32::consts::FRAC_PI_4,       // top-right
                    0.0,                                // right
                    std::f32::consts::FRAC_PI_4,        // bottom-right
                    std::f32::consts::FRAC_PI_2,        // bottom
                    3.0 * std::f32::consts::FRAC_PI_4,  // bottom-left
                    std::f32::consts::PI,               // left
                    -3.0 * std::f32::consts::FRAC_PI_4, // top-left
                ];

                let compute_label_rect = |angle: f32| -> Rect {
                    let anchor_x = center.x + (screen_radius + gap) * angle.cos();
                    let anchor_y = center.y + (screen_radius + gap) * angle.sin();
                    // Position text so it's centered on the anchor point,
                    // biased outward from center
                    let cos = angle.cos();
                    let sin = angle.sin();
                    let x = if cos < -0.3 {
                        anchor_x - text_w // left side: right-align to anchor
                    } else if cos > 0.3 {
                        anchor_x // right side: left-align from anchor
                    } else {
                        anchor_x - text_w / 2.0 // top/bottom: center
                    };
                    let y = if sin < -0.3 {
                        anchor_y - text_h // top side: above anchor
                    } else if sin > 0.3 {
                        anchor_y // bottom side: below anchor
                    } else {
                        anchor_y - text_h / 2.0 // left/right: vertically center
                    };
                    Rect::from_min_size(Pos2::new(x, y), egui::vec2(text_w, text_h))
                };

                // Find first non-overlapping position
                let mut best_rect = compute_label_rect(candidate_angles[0]);
                for &angle in &candidate_angles {
                    let rect = compute_label_rect(angle);
                    let overlaps = placed_labels.iter().any(|prev| prev.intersects(rect));
                    if !overlaps {
                        best_rect = rect;
                        break;
                    }
                }

                placed_labels.push(best_rect);
                shapes.push(Shape::galley(best_rect.min, galley, Color32::TRANSPARENT));
            }
        }

        DrawCommand::BuffZone { pos, radius, color, alpha, marker_name } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);

            // Filled circle
            shapes.push(Shape::circle_filled(center, r, color_from_rgba(*color, *alpha)));
            // Border ring
            shapes.push(Shape::circle_stroke(
                center,
                r,
                Stroke::new(transform.scale_stroke(1.5), color_from_rgba(*color, 0.6)),
            ));

            // Powerup icon centered on zone
            if let Some(name) = marker_name
                && let Some(tex) = textures.powerup_icons.get(name.as_str())
            {
                let icon_size = transform.scale_distance(16.0);
                let half = icon_size / 2.0;
                let mut mesh = egui::Mesh::with_texture(tex.id());
                let rect = Rect::from_min_max(
                    Pos2::new(center.x - half, center.y - half),
                    Pos2::new(center.x + half, center.y + half),
                );
                mesh.add_rect_with_uv(
                    rect,
                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                shapes.push(Shape::Mesh(mesh.into()));
            }
        }

        DrawCommand::BattleResultOverlay { text, subtitle, color, subtitle_above } => {
            let canvas_w = transform.screen_canvas_width();
            let canvas_h = (transform.canvas_width + transform.hud_height) * transform.window_scale;
            let center_x = transform.origin.x + canvas_w / 2.0;
            let center_y = transform.origin.y + canvas_h / 2.0;

            // Main text: 1/8 of canvas width as font size
            let font_size = canvas_w / 8.0;
            let main_font = game_font(font_size);
            let main_galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), main_font, Color32::WHITE));
            let main_w = main_galley.size().x;
            let main_h = main_galley.size().y;

            // Subtitle: 1/4 of main font size
            let sub_galley = subtitle.as_ref().map(|s| {
                let sub_font = game_font(font_size / 4.0);
                ctx.fonts_mut(|f| f.layout_no_wrap(s.clone(), sub_font, Color32::from_gray(200)))
            });
            let sub_h = sub_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
            let gap = if subtitle.is_some() { 8.0 * ws } else { 0.0 };
            let total_h = main_h + gap + sub_h;

            // Position main and subtitle based on subtitle_above flag
            let block_top = center_y - total_h / 2.0;
            let (text_x, text_y, sub_top) = if *subtitle_above {
                // Subtitle above: [subtitle] [gap] [main]
                (center_x - main_w / 2.0, block_top + sub_h + gap, block_top)
            } else {
                // Subtitle below: [main] [gap] [subtitle]
                (center_x - main_w / 2.0, block_top, block_top + main_h + gap)
            };

            // Text glow layers matching video renderer approach:
            // dark shadows for contrast, then colored glow, then white text
            let offsets: &[(f32, f32)] =
                &[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)];
            let glow_layers: &[(f32, [u8; 3], f32)] = &[
                (6.0, [0, 0, 0], 0.15),
                (4.0, [0, 0, 0], 0.25),
                (3.0, *color, 0.30),
                (2.0, *color, 0.50),
                (1.0, *color, 0.70),
            ];

            for &(dist, c, opacity) in glow_layers {
                let layer_color = Color32::from_rgba_premultiplied(
                    (c[0] as f32 * opacity) as u8,
                    (c[1] as f32 * opacity) as u8,
                    (c[2] as f32 * opacity) as u8,
                    (255.0 * opacity) as u8,
                );
                let glow_font = game_font(font_size);
                for &(dx, dy) in offsets {
                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), glow_font.clone(), layer_color));
                    shapes.push(Shape::galley(
                        Pos2::new(text_x + dx * dist, text_y + dy * dist),
                        galley,
                        Color32::TRANSPARENT,
                    ));
                }
            }

            // Main white text on top
            shapes.push(Shape::galley(Pos2::new(text_x, text_y), main_galley, Color32::TRANSPARENT));

            // Subtitle
            if let Some(sub_galley) = sub_galley {
                let sub_w = sub_galley.size().x;
                let sub_x = center_x - sub_w / 2.0;
                let sub_y = sub_top;

                // Subtitle dark outline
                let sub_font = game_font(font_size / 4.0);
                for &(dx, dy) in offsets {
                    let outline = ctx.fonts_mut(|f| {
                        f.layout_no_wrap(
                            subtitle.as_ref().unwrap().clone(),
                            sub_font.clone(),
                            Color32::from_rgba_premultiplied(0, 0, 0, 180),
                        )
                    });
                    shapes.push(Shape::galley(
                        Pos2::new(sub_x + dx * 2.0, sub_y + dy * 2.0),
                        outline,
                        Color32::TRANSPARENT,
                    ));
                }

                shapes.push(Shape::galley(Pos2::new(sub_x, sub_y), sub_galley, Color32::TRANSPARENT));
            }
        }

        DrawCommand::TeamBuffs { friendly_buffs, enemy_buffs } => {
            let canvas_w = transform.screen_canvas_width();
            let icon_size = 16.0 * ws;
            let gap = 2.0 * ws;
            let buff_y = transform.hud_pos(0.0, HUD_HEIGHT as f32).y;
            let origin_x = transform.hud_pos(0.0, 0.0).x;

            // Friendly buffs: left side
            let mut x = origin_x + 4.0 * ws;
            for (marker, count) in friendly_buffs {
                if let Some(tex) = textures.powerup_icons.get(marker.as_str()) {
                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_size(Pos2::new(x, buff_y), Vec2::new(icon_size, icon_size));
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));

                    if *count > 1 {
                        let label = format!("{}", count);
                        let font = game_font(10.0 * ws);
                        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label, font, Color32::WHITE));
                        let tw = galley.size().x;
                        shapes.push(Shape::galley(
                            Pos2::new(x + icon_size, buff_y + 4.0 * ws),
                            galley,
                            Color32::TRANSPARENT,
                        ));
                        x += icon_size + tw + gap;
                    } else {
                        x += icon_size + gap;
                    }
                }
            }

            // Enemy buffs: right side
            let mut x = origin_x + canvas_w - 4.0 * ws;
            for (marker, count) in enemy_buffs {
                if let Some(tex) = textures.powerup_icons.get(marker.as_str()) {
                    if *count > 1 {
                        let label = format!("{}", count);
                        let font = game_font(10.0 * ws);
                        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label, font, Color32::WHITE));
                        let tw = galley.size().x;
                        x -= tw;
                        shapes.push(Shape::galley(Pos2::new(x, buff_y + 4.0 * ws), galley, Color32::TRANSPARENT));
                        x -= icon_size;
                    } else {
                        x -= icon_size;
                    }

                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    let rect = Rect::from_min_size(Pos2::new(x, buff_y), Vec2::new(icon_size, icon_size));
                    mesh.add_rect_with_uv(
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    shapes.push(Shape::Mesh(mesh.into()));
                    x -= gap;
                }
            }
        }

        DrawCommand::ChatOverlay { entries } => {
            let canvas_w = transform.screen_canvas_width();
            let canvas_h = (transform.canvas_width + transform.hud_height) * transform.window_scale;
            let header_font = game_font(11.0 * ws);
            let msg_font = game_font(11.0 * ws);
            let line_h = 14.0 * ws;
            let icon_size = 12.0 * ws;
            let padding = 6.0 * ws;
            let entry_gap = 6.0 * ws;

            // Chat box: left side, vertically centered, 25% of canvas width
            let box_w = canvas_w * 0.25;
            let box_x = transform.origin.x + 4.0 * ws;
            let inner_w = box_w - padding * 2.0;

            struct ChatLayout {
                /// Line 1: "[CLAN] PlayerName" — clan portion in clan color, rest in team color
                clan_galley: Option<std::sync::Arc<egui::Galley>>,
                name_galley: std::sync::Arc<egui::Galley>,
                /// Line 2: ship icon + ship name
                ship_icon_species: Option<String>,
                ship_name_galley: Option<std::sync::Arc<egui::Galley>>,
                /// Line 3+: word-wrapped message
                msg_galleys: Vec<std::sync::Arc<egui::Galley>>,
                opacity: f32,
                team_color: [u8; 3],
            }

            let mut layouts = Vec::new();
            let mut total_h = padding; // top padding
            for entry in entries {
                let opacity = entry.opacity;
                let alpha = (opacity * 255.0) as u8;
                let team_color = entry.team_color;
                let team_c = Color32::from_rgba_unmultiplied(team_color[0], team_color[1], team_color[2], alpha);

                // Line 1: clan tag + player name
                let clan_galley =
                    if !entry.clan_tag.is_empty() {
                        let clan_c = if let Some(cc) = entry.clan_color {
                            Color32::from_rgba_unmultiplied(cc[0], cc[1], cc[2], alpha)
                        } else {
                            team_c
                        };
                        Some(ctx.fonts_mut(|f| {
                            f.layout_no_wrap(format!("[{}] ", entry.clan_tag), header_font.clone(), clan_c)
                        }))
                    } else {
                        None
                    };
                let name_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.player_name.clone(), header_font.clone(), team_c));

                // Line 2: ship icon + ship name (optional)
                let ship_name_galley = entry
                    .ship_name
                    .as_ref()
                    .map(|sn| ctx.fonts_mut(|f| f.layout_no_wrap(sn.clone(), header_font.clone(), team_c)));
                let has_ship_line = ship_name_galley.is_some();

                // Message lines (word-wrapped)
                let msg_color = Color32::from_rgba_unmultiplied(
                    entry.message_color[0],
                    entry.message_color[1],
                    entry.message_color[2],
                    alpha,
                );
                let msg_galleys = ctx.fonts_mut(|f| {
                    let job =
                        egui::text::LayoutJob::simple(entry.message.clone(), msg_font.clone(), msg_color, inner_w);
                    let galley = f.layout_job(job);
                    vec![galley]
                });

                let msg_lines: usize = msg_galleys.iter().map(|g| g.rows.len().max(1)).sum();
                let line_count = 1 + has_ship_line as usize + msg_lines;
                total_h += line_count as f32 * line_h + entry_gap;

                layouts.push(ChatLayout {
                    clan_galley,
                    name_galley,
                    ship_icon_species: entry.ship_species.clone(),
                    ship_name_galley,
                    msg_galleys,
                    opacity,
                    team_color,
                });
            }

            if layouts.is_empty() {
                // nothing to draw
            } else {
                total_h += padding; // bottom padding
                let box_y = transform.origin.y + canvas_h / 2.0 - total_h / 2.0;

                // Semi-translucent background
                let bg_rect = Rect::from_min_size(Pos2::new(box_x, box_y), Vec2::new(box_w, total_h));
                shapes.push(Shape::rect_filled(bg_rect, CornerRadius::same(3), Color32::from_black_alpha(90)));

                let mut y = box_y + padding;
                for layout in &layouts {
                    let alpha = (layout.opacity * 255.0) as u8;
                    let x = box_x + padding;

                    // Line 1: [CLAN] PlayerName
                    let mut nx = x;
                    if let Some(ref cg) = layout.clan_galley {
                        shapes.push(Shape::galley(Pos2::new(nx, y), cg.clone(), Color32::TRANSPARENT));
                        nx += cg.size().x;
                    }
                    shapes.push(Shape::galley(Pos2::new(nx, y), layout.name_galley.clone(), Color32::TRANSPARENT));
                    y += line_h;

                    // Line 2: ship icon + ship name
                    if let Some(ref sng) = layout.ship_name_galley {
                        let mut sx = x;
                        if let Some(ref species) = layout.ship_icon_species {
                            if let Some(tex) = textures.ship_icons.get(species.as_str()) {
                                let tc = layout.team_color;
                                let tint = Color32::from_rgba_unmultiplied(tc[0], tc[1], tc[2], alpha);
                                // Vertically center icon with the text on this line
                                let icon_center_y = y + sng.size().y / 2.0;
                                shapes.push(make_rotated_icon_mesh(
                                    tex.id(),
                                    Pos2::new(sx + icon_size / 2.0, icon_center_y),
                                    icon_size,
                                    0.0,
                                    tint,
                                ));
                            }
                            sx += icon_size + 2.0 * ws;
                        }
                        shapes.push(Shape::galley(Pos2::new(sx, y), sng.clone(), Color32::TRANSPARENT));
                        y += line_h;
                    }

                    // Message text (word-wrapped)
                    for galley in &layout.msg_galleys {
                        shapes.push(Shape::galley(Pos2::new(x, y), galley.clone(), Color32::TRANSPARENT));
                        y += galley.rows.len().max(1) as f32 * line_h;
                    }

                    y += entry_gap;
                }
            }
        }
    }

    shapes
}
